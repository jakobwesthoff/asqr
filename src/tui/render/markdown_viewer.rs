// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewer of Markdown entries (spec section 8.7, ADR 34): the text
//! rendered by patine for the body's width and wrapped, without a gutter
//! and without sideways scrolling. Lines that patine leaves wider, such as
//! those of a code block, wrap behind a dim `↪`. A text of more than 1 MiB
//! is not rendered: it shows as plain text, wrapped the same way, below a
//! dim note.

use std::path::Path;

use patine::{QuoteStyle, RenderOptions, StyledLine};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use super::loading::contain;
use super::patine_adapter::{markdown_rows, plain_rows};
use super::source_file::{
    Cut, LineBreaks, Source, Unreadable, between_cut_notes, inline, read_source,
};
use super::source_line::{Note, SourceLine, rows_from};
use super::viewer::{Content, ContentSize, Placeholder, Viewer};
use crate::format::{LARGE_ENTRY_BYTES, MarkdownEntry};
use crate::graphics::PlacedImage;
use crate::tui::Offset;

/// The viewer of `entry`: reads the text, whose quotes show in `quote`.
/// Rendering depends on the width, so it waits for `lay_out`. The home
/// folder `home` shows as `~` in the path of a file that cannot be read.
pub(super) fn markdown_content(
    entry: &MarkdownEntry,
    quote: QuoteStyle,
    home: Option<&Path>,
) -> Content<Markdown> {
    let source = read_source(
        entry.path.as_deref(),
        entry.content.as_deref(),
        LineBreaks::Any,
    );
    content_of(source, quote, home)
}

/// The caption of an image entry, `text`, shown as a Markdown entry's
/// content is, cut by the same line limit and plain above the same size,
/// with quotes in `quote`.
pub(super) fn caption(text: &str, quote: QuoteStyle) -> Markdown {
    document(inline(text, LineBreaks::Any), quote)
}

/// Why a text shows as plain text instead of rendered Markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plain {
    /// The text is more than 1 MiB.
    TooLarge,
    /// The Markdown parser panicked on the text.
    ParserFailed,
}

impl Plain {
    /// What heads the plain text, above the notes of a cut.
    fn note(self) -> Note {
        match self {
            Plain::TooLarge => Note::new("over 1 MiB: shown as plain text"),
            Plain::ParserFailed => Note::new("Markdown failed: shown as plain text"),
        }
    }
}

/// Renders Markdown to styled lines for a width. Tests put a renderer in
/// its place that panics on any text.
type Render = fn(&str, usize, &RenderOptions) -> Vec<StyledLine>;

/// The size above which a text shows as plain text instead of rendered
/// Markdown: the size the warning about a large entry counts.
fn plain_limit() -> usize {
    usize::try_from(LARGE_ENTRY_BYTES).expect("1 MiB fits a usize")
}

/// What the viewer shows of `source`, as read for its entry: its
/// Markdown with quotes in `quote`, a card when the file cannot be read,
/// or a note when there is no text at all. A cut text whose kept lines
/// are all blank has more after them, so it shows the notes of its cut
/// rather than "empty".
fn content_of(
    source: Result<Source, Unreadable>,
    quote: QuoteStyle,
    home: Option<&Path>,
) -> Content<Markdown> {
    content_within(source, quote, home, plain_limit())
}

/// [`content_of`] with plain text above `limit` bytes.
fn content_within(
    source: Result<Source, Unreadable>,
    quote: QuoteStyle,
    home: Option<&Path>,
    limit: usize,
) -> Content<Markdown> {
    match source {
        Err(unreadable) => Content::Problem(unreadable.card(home)),
        Ok(source) if source.cut.is_none() && source.text.trim().is_empty() => {
            Content::Note(Placeholder::note("empty"))
        }
        Ok(source) => Content::Body(document_within(source, quote, limit)),
    }
}

/// The Markdown `source`, the start of a longer text where the limit
/// `cut` ended it, with quotes in `quote`, not laid out yet.
fn document(source: Source, quote: QuoteStyle) -> Markdown {
    document_within(source, quote, plain_limit())
}

/// [`document`] shown as plain text above `limit` bytes.
fn document_within(source: Source, quote: QuoteStyle, limit: usize) -> Markdown {
    let Source { text, cut, size } = source;
    // Only a cut text gets here blank. It shows the notes of its cut alone:
    // it has no text to show as plain text, so its size, which counts the
    // bytes before the cut, does not turn it into a plain-text note.
    let blank = text.trim().is_empty();
    let source = if blank { String::new() } else { text };
    Markdown {
        plain: (!blank && (size > limit || source.len() > limit)).then_some(Plain::TooLarge),
        render: patine::render_lines_with,
        source,
        cut,
        // A document keeps patine's defaults, where a single newline joins
        // the lines and raw HTML is left out (ADR 35). Only its quotes take
        // the style the screen asks for.
        options: RenderOptions::new().with_quote_style(quote),
        rows: Vec::new(),
        laid_out: None,
        #[cfg(test)]
        renders: 0,
    }
}

/// The Markdown of an entry with the rows it was last laid out into.
pub(super) struct Markdown {
    source: String,
    /// Why `source` shows as plain text, wrapped to the width, below the
    /// note of that reason, if it does.
    plain: Option<Plain>,
    /// How the Markdown is rendered.
    render: Render,
    /// The limit that ended `source`, the start of a longer text.
    cut: Option<Cut>,
    /// How patine renders `source`.
    options: RenderOptions,
    rows: Vec<Row>,
    /// The width `rows` were rendered for and the size they take.
    laid_out: Option<(u16, ContentSize)>,
    /// How often patine rendered the text, so tests see a render skipped.
    #[cfg(test)]
    renders: usize,
}

enum Row {
    Text(SourceLine),
    Note(Note),
}

impl Markdown {
    /// The source as plain text, wrapped to `columns`.
    fn plain_text(&self, columns: usize) -> Vec<SourceLine> {
        plain_rows(&patine::highlight(&self.source, None), columns)
    }
}

impl Viewer for Markdown {
    /// Renders the Markdown for `width` columns. Every row fits the width,
    /// so the pane never scrolls sideways. The rows depend on the width
    /// alone, so a body that changed only its height keeps the rows
    /// rendered before.
    fn lay_out(&mut self, width: u16, _height: u16) -> ContentSize {
        if let Some((rendered_for, size)) = self.laid_out
            && rendered_for == width
        {
            return size;
        }
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let columns = usize::from(width);
        // The rows of the width before are dropped first, so that they
        // and the new ones never sit in memory together.
        self.rows = Vec::new();
        let text = match self.plain {
            Some(_) => self.plain_text(columns),
            None => match contain(|| {
                markdown_rows(
                    &(self.render)(&self.source, columns, &self.options),
                    columns,
                )
            }) {
                Ok(rendered) => rendered,
                Err(message) => {
                    // The parser's panic message names a place in its code,
                    // never the text.
                    tracing::warn!(
                        panic = message,
                        "the Markdown parser panicked; showing the text as plain text"
                    );
                    // Kept for the widths that follow, which would panic
                    // the same way.
                    self.plain = Some(Plain::ParserFailed);
                    self.plain_text(columns)
                }
            },
        };
        self.rows = self
            .plain
            .map(|plain| Row::Note(plain.note()))
            .into_iter()
            .chain(between_cut_notes(
                self.cut,
                text.into_iter().map(Row::Text),
                Row::Note,
            ))
            .collect();
        // Only a character wider than the whole body is wider than a row,
        // and it shows cut.
        let widest = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Text(text) => text.width(),
                Row::Note(note) => note.columns(columns),
            })
            .max()
            .unwrap_or(0);
        let size = ContentSize {
            lines: self.rows.len(),
            columns: widest.min(columns),
        };
        self.laid_out = Some((width, size));
        size
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        for (row_area, row) in rows_from(area, &self.rows, offset.top) {
            match row {
                Row::Text(text) => text.draw(buf, row_area, 0),
                Row::Note(note) => note.render(row_area, buf),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ratatui::style::{Color, Modifier};

    use super::super::patine_adapter::quote_style;
    use super::super::test_support::{
        LONG_LINK_RECORDS_AT_40, LONG_LINK_TABLE, TWELVE_COLUMNS, drawn, row, shown,
    };
    use super::*;

    /// The viewer of the entry `json`, as drawn with colours.
    fn built(json: &str) -> Content<Markdown> {
        let entry: MarkdownEntry = serde_json::from_str(json).expect("test entry parses");
        markdown_content(&entry, quote_style(false), None)
    }

    fn inline(markdown: &str) -> Content<Markdown> {
        built(&serde_json::json!({"type": "markdown", "content": markdown}).to_string())
    }

    const NOTES: &str = "# Retries\n\nA failed request is tried **again** up to three times, with `delay` doubling.\n\n## Settings\n\n- tries: 3\n- delay: 100 ms\n\n```rust\nlet delay = Duration::from_millis(100);\n```\n";

    /// 500,000 line ends and then `x`: a text of 500,001 lines whose
    /// first 500,000 are blank. Rendering many lines of text takes minutes
    /// in a test build, so the tests of the line limit cut blank lines.
    fn blank_lines_then_x() -> String {
        format!("{}x", "\n".repeat(super::super::source_file::MAX_LINES))
    }

    #[test]
    fn a_text_cut_at_the_line_limit_says_so_first_and_last() {
        let mut viewer = content_of(
            Ok(Source {
                text: "**Plan**\n".into(),
                cut: Some(Cut::Lines),
                size: 9,
            }),
            quote_style(false),
            None,
        );

        let buf = drawn(&mut viewer, 40, 3, Offset::default());
        assert_eq!(row(&buf, 0), "first 500,000 lines shown");
        assert_eq!(row(&buf, 1), "Plan");
        assert_eq!(row(&buf, 2), "… the rest is cut after 500,000 lines");
    }

    #[test]
    fn a_cut_text_of_blank_lines_shows_its_notes() {
        let mut viewer = inline(&blank_lines_then_x());

        assert_eq!(viewer.lay_out(40, 2).lines, 2);
        let buf = drawn(&mut viewer, 40, 2, Offset::default());
        assert_eq!(row(&buf, 0), "first 500,000 lines shown");
        assert_eq!(row(&buf, 1), "… the rest is cut after 500,000 lines");
    }

    #[test]
    fn a_caption_above_the_line_limit_says_so() {
        let mut viewer = caption(&blank_lines_then_x(), quote_style(false));

        assert_eq!(viewer.lay_out(40, 2).lines, 2);
        let buf = drawn(&mut viewer, 40, 2, Offset::default());
        assert_eq!(row(&buf, 0), "first 500,000 lines shown");
        assert_eq!(row(&buf, 1), "… the rest is cut after 500,000 lines");
    }

    // -----------------------------------------------------------------
    // Plain text above the size limit
    // -----------------------------------------------------------------

    const PLAIN_NOTE: &str = "over 1 MiB: shown as plain text";

    /// Markdown whose syntax shows only when it is rendered.
    const MIXED: &str = "**bold** and `code`\n# Heading\n";

    fn source(text: &str) -> Source {
        Source {
            text: text.to_owned(),
            cut: None,
            size: text.len(),
        }
    }

    /// The viewer of `text` as a text of `source` and `limit` would show it.
    fn within(source: Source, limit: usize) -> Content<Markdown> {
        content_within(Ok(source), quote_style(false), None, limit)
    }

    #[test]
    fn a_text_over_the_limit_shows_as_plain_text_below_a_note() {
        let mut viewer = within(source(MIXED), MIXED.len() - 1);

        let buf = drawn(&mut viewer, 40, 3, Offset::default());

        assert_eq!(row(&buf, 0), PLAIN_NOTE);
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(row(&buf, 1), "**bold** and `code`");
        assert!(!buf[(2, 1)].modifier.contains(Modifier::BOLD));
        assert_eq!(row(&buf, 2), "# Heading");
        assert!(!buf[(0, 2)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_text_of_exactly_the_limit_is_rendered() {
        let mut viewer = within(source(MIXED), MIXED.len());

        let buf = drawn(&mut viewer, 40, 1, Offset::default());

        assert_eq!(row(&buf, 0), "bold and code");
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_text_whose_size_as_given_is_over_the_limit_shows_plain_though_its_kept_lines_are_not() {
        let cut = Source {
            text: MIXED.to_owned(),
            cut: Some(Cut::Lines),
            size: 100,
        };
        let mut viewer = within(cut, 99);

        let buf = drawn(&mut viewer, 40, 3, Offset::default());

        assert_eq!(row(&buf, 1), "first 500,000 lines shown");
        assert_eq!(row(&buf, 0), PLAIN_NOTE);
        assert_eq!(row(&buf, 2), "**bold** and `code`");
    }

    #[test]
    fn a_text_that_decoding_made_longer_than_the_limit_shows_plain() {
        let decoded = Source {
            text: MIXED.to_owned(),
            cut: None,
            size: 10,
        };
        let mut viewer = within(decoded, MIXED.len() - 1);

        let buf = drawn(&mut viewer, 40, 2, Offset::default());

        assert_eq!(row(&buf, 0), PLAIN_NOTE);
        assert_eq!(row(&buf, 1), "**bold** and `code`");
    }

    #[test]
    fn a_blank_text_over_the_limit_is_still_empty() {
        let blank = Source {
            text: "\n\n".to_owned(),
            cut: None,
            size: 1000,
        };
        let mut viewer = within(blank, 10);

        let buf = drawn(&mut viewer, 40, 1, Offset::default());

        assert_eq!(row(&buf, 0), "empty");
    }

    /// A cut text of blank lines whose size is over the limit shows the
    /// notes of its cut, not a plain-text note for its size.
    #[test]
    fn a_cut_blank_text_over_the_limit_shows_its_notes_alone() {
        let blank = Source {
            text: "\n\n".to_owned(),
            cut: Some(Cut::Lines),
            size: 1000,
        };
        let mut viewer = within(blank, 10);

        assert_eq!(viewer.lay_out(40, 2).lines, 2);
        let buf = drawn(&mut viewer, 40, 2, Offset::default());
        assert_eq!(row(&buf, 0), "first 500,000 lines shown");
        assert_eq!(row(&buf, 1), "… the rest is cut after 500,000 lines");
    }

    #[test]
    fn plain_text_wraps_behind_a_marker_and_keeps_its_leading_blanks() {
        let text = "  alpha beta gamma delta\n";
        let mut viewer = within(source(text), 5);

        let buf = drawn(&mut viewer, 14, 3, Offset::default());

        assert_eq!(row(&buf, 1), "  alpha beta g");
        assert_eq!(row(&buf, 2), "  ↪ amma delta");
        assert!(buf[(2, 2)].modifier.contains(Modifier::DIM), "the marker");
    }

    #[test]
    fn a_plain_line_that_starts_like_a_numbered_item_wraps_without_an_item_indent() {
        let mut viewer = within(source("1. alpha beta gamma delta\n"), 5);

        let buf = drawn(&mut viewer, 14, 3, Offset::default());

        assert_eq!(row(&buf, 1), "1. alpha beta");
        assert_eq!(row(&buf, 2), "↪ gamma delta");
    }

    #[test]
    fn a_plain_line_that_starts_like_a_bullet_wraps_without_an_item_indent() {
        let mut viewer = within(source("• alpha beta gamma delta\n"), 5);

        let buf = drawn(&mut viewer, 14, 3, Offset::default());

        assert_eq!(row(&buf, 1), "• alpha beta g");
        assert_eq!(row(&buf, 2), "↪ amma delta");
    }

    #[test]
    fn plain_text_expands_tabs_and_pictures_controls() {
        let mut viewer = within(source("\tx = 1;\u{1b}\n"), 5);

        let buf = drawn(&mut viewer, 20, 2, Offset::default());

        assert_eq!(row(&buf, 1), "    x = 1;␛");
    }

    #[test]
    fn the_plain_note_comes_before_the_notes_of_a_cut_in_bytes() {
        let cut = Source {
            text: "**x**\n".to_owned(),
            cut: Some(Cut::Bytes),
            size: 8 * 1024 * 1024,
        };
        let mut viewer = content_of(Ok(cut), quote_style(false), None);

        assert_eq!(
            shown(&mut viewer, 40, 0),
            [
                format!("{PLAIN_NOTE:<40}"),
                format!("{:<40}", "first 8 MiB shown"),
                format!("{:<40}", "**x**"),
                format!("{:<40}", "… the rest is cut after 8 MiB"),
            ]
            .map(|row| format!("{row:?}"))
            .join("\n")
        );
    }

    #[test]
    fn the_plain_note_comes_before_the_notes_of_a_cut_in_lines() {
        let cut = Source {
            text: "**x**\n".to_owned(),
            cut: Some(Cut::Lines),
            size: 2 * 1024 * 1024,
        };
        let mut viewer = content_of(Ok(cut), quote_style(false), None);

        let buf = drawn(&mut viewer, 40, 4, Offset::default());

        assert_eq!(row(&buf, 0), PLAIN_NOTE);
        assert_eq!(row(&buf, 1), "first 500,000 lines shown");
        assert_eq!(row(&buf, 2), "**x**");
        assert_eq!(row(&buf, 3), "… the rest is cut after 500,000 lines");
    }

    #[test]
    fn plain_rows_are_wrapped_again_for_a_new_width_only() {
        let mut viewer = within(source("abc def ghi jkl mno pqr\n"), 4);

        let wide = viewer.lay_out(40, 10);
        assert_eq!(wide.lines, 2, "the note and the line");
        assert_eq!(viewer.lay_out(40, 12), wide, "the rows depend on the width");
        assert_eq!(renders(&viewer), 1);

        assert_eq!(viewer.lay_out(10, 12).lines, 4, "the note and three rows");
        assert_eq!(renders(&viewer), 2);
    }

    #[test]
    fn a_plain_text_is_as_wide_as_the_body_at_most() {
        let mut viewer = within(source("abc def ghi jkl mno pqr\n"), 4);

        assert_eq!(viewer.lay_out(10, 10).columns, 10);
        assert_eq!(viewer.lay_out(60, 10).columns, 31, "the note is the widest");
    }

    #[test]
    fn plain_text_shows_in_the_narrowest_context_pane() {
        let mut viewer = within(source(MIXED), 5);

        let buf = drawn(&mut viewer, 40, 1, Offset::default());

        assert_eq!(row(&buf, 0), PLAIN_NOTE, "the whole note fits 40 columns");
    }

    #[test]
    fn plain_text_snapshot() {
        let text = "Retries\n\nA **failed** request is tried again up to three times,\n  with `delay` doubling.\n\n```rust\nlet delay = Duration::from_millis(100);\n```\n";
        let mut viewer = within(source(text), 5);

        insta::assert_snapshot!("plain_24", shown(&mut viewer, 24, 0));
    }

    /// A text of 1 MiB and one byte, the least that shows plain. Its lines
    /// are short, so it has more than 500,000 of them: the line limit cuts
    /// it, and only the size as given tells that it is large.
    fn one_mib_and_a_byte() -> String {
        let limit = usize::try_from(LARGE_ENTRY_BYTES).expect("1 MiB fits a usize");
        format!("{}x", "x\n".repeat(limit / 2))
    }

    // A wiring check of the constant: the old code needs over nine minutes
    // for this text in a debug build, so it cannot fail in reasonable time.
    #[test]
    fn an_inline_text_of_one_mib_and_a_byte_shows_as_plain_text() {
        let text = one_mib_and_a_byte();
        let mut viewer = inline(&text);

        let buf = drawn(&mut viewer, 40, 3, Offset::default());

        assert_eq!(row(&buf, 0), PLAIN_NOTE);
        assert_eq!(row(&buf, 1), "first 500,000 lines shown", "524,289 lines");
        assert_eq!(row(&buf, 2), "x");
    }

    // A wiring check of the constant, as for the inline text above.
    #[test]
    fn a_caption_of_one_mib_and_a_byte_shows_as_plain_text() {
        let mut viewer = caption(&one_mib_and_a_byte(), quote_style(false));

        let buf = drawn(&mut viewer, 40, 3, Offset::default());

        assert_eq!(row(&buf, 0), PLAIN_NOTE);
        assert_eq!(row(&buf, 1), "first 500,000 lines shown", "524,289 lines");
        assert_eq!(row(&buf, 2), "x");
    }

    // -----------------------------------------------------------------
    // Plain text where the parser panics
    // -----------------------------------------------------------------

    const PARSER_NOTE: &str = "Markdown failed: shown as plain text";

    thread_local! {
        /// How often the renderer that panics was asked on this thread.
        static PARSER_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn parser_panics(_: &str, _: usize, _: &RenderOptions) -> Vec<StyledLine> {
        PARSER_CALLS.set(PARSER_CALLS.get() + 1);
        panic!("the parser panics");
    }

    /// The Markdown `text`, whose renderer panics.
    fn failing(text: &str) -> Markdown {
        let mut markdown = document(source(text), quote_style(false));
        markdown.render = parser_panics;
        markdown
    }

    #[test]
    fn a_parser_panic_shows_the_text_as_plain_text_below_a_note() {
        let mut viewer = failing(MIXED);

        let buf = drawn(&mut viewer, 40, 3, Offset::default());

        assert_eq!(row(&buf, 0), PARSER_NOTE);
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(row(&buf, 1), "**bold** and `code`");
        assert!(!buf[(2, 1)].modifier.contains(Modifier::BOLD));
        assert_eq!(row(&buf, 2), "# Heading");
    }

    #[test]
    fn after_a_parser_panic_another_width_asks_the_parser_no_more() {
        PARSER_CALLS.set(0);
        let mut viewer = failing("abc def ghi jkl mno pqr\n");

        assert_eq!(viewer.lay_out(40, 10).lines, 2, "the note and the line");
        assert_eq!(viewer.lay_out(10, 10).lines, 4, "the note and three rows");

        assert_eq!(PARSER_CALLS.get(), 1);
    }

    #[test]
    fn a_caption_the_parser_panics_on_shows_as_plain_text_too() {
        let mut viewer = caption(MIXED, quote_style(false));
        viewer.render = parser_panics;

        let buf = drawn(&mut viewer, 40, 2, Offset::default());

        assert_eq!(row(&buf, 0), PARSER_NOTE);
        assert_eq!(row(&buf, 1), "**bold** and `code`");
    }

    /// The narrowest body a Markdown entry gets is that of the narrowest
    /// context pane, 40 columns less the 2 of its indent.
    #[test]
    fn the_note_of_a_parser_panic_shows_whole_at_the_narrowest_body() {
        let mut viewer = failing(MIXED);

        let buf = drawn(&mut viewer, 38, 1, Offset::default());

        assert_eq!(row(&buf, 0), PARSER_NOTE);
    }

    /// Texts of unclosed fences, definitions and front matter, each an edge
    /// case of the Markdown grammar. The viewer lays each out.
    fn lays_out(text: &str) {
        let mut viewer = inline(text);

        assert!(viewer.lay_out(40, 10).lines > 0);
    }

    #[test]
    fn a_definition_followed_by_an_underline_does_not_end_the_viewer() {
        lays_out("[a]:b\n=\n=+\r-");
    }

    #[test]
    fn an_unclosed_fence_in_a_numbered_item_before_a_bullet_does_not_end_the_viewer() {
        lays_out("1. ~~~\n*");
    }

    #[test]
    fn an_unclosed_frontmatter_fence_before_a_list_does_not_end_the_viewer() {
        lays_out("---\n\n- a");
    }

    #[test]
    fn an_unclosed_frontmatter_fence_before_a_footnote_does_not_end_the_viewer() {
        lays_out("---\n\n[^a]:");
    }

    #[test]
    fn markdown_renders_wrapped_to_the_body() {
        let mut viewer = inline(NOTES);

        insta::assert_snapshot!("notes_40", shown(&mut viewer, 40, 0));
        insta::assert_snapshot!("notes_24", shown(&mut viewer, 24, 0));
    }

    /// Reference links show like inline links, a footnote definition comes
    /// after the text that refers to it, and a task item with no text
    /// shows its box.
    const REFERENCES: &str =
        "[a guide][g] and [^1].\n\n[^1]: The note.\n\n- [ ]\n\n[g]: https://example.com/guide\n";

    #[test]
    fn references_footnotes_and_empty_tasks_render() {
        let mut viewer = inline(REFERENCES);

        insta::assert_snapshot!("references_40", shown(&mut viewer, 40, 0));
    }

    /// Quotes nested past the width stop indenting, so the text under them
    /// keeps room instead of taking a line for each word.
    #[test]
    fn deeply_nested_quotes_leave_room_for_their_text() {
        let mut viewer = inline(&format!("{}alpha beta gamma delta", "> ".repeat(30)));

        assert_eq!(viewer.lay_out(40, 10).lines, 2);
    }

    #[test]
    fn it_never_reports_more_columns_than_the_body() {
        let mut viewer = inline(NOTES);

        for width in [24, 40, 80] {
            let size = viewer.lay_out(width, 10);
            assert!(size.columns <= usize::from(width), "{width}: {size:?}");
        }
        assert_eq!(viewer.lay_out(44, 10).lines, 11);
        assert_eq!(viewer.lay_out(40, 10).lines, 12, "the code line wraps");
        assert!(
            viewer.lay_out(24, 10).lines > 12,
            "the text wraps more when narrower"
        );
    }

    /// How often `viewer`'s Markdown was laid out.
    fn renders(viewer: &Content<Markdown>) -> usize {
        match viewer {
            Content::Body(markdown) => markdown.renders,
            _ => panic!("the viewer holds Markdown"),
        }
    }

    #[test]
    fn only_a_new_width_renders_the_markdown_again() {
        let mut viewer = inline(NOTES);

        let size = viewer.lay_out(40, 10);
        assert_eq!(viewer.lay_out(40, 12), size, "the rows depend on the width");
        assert_eq!(renders(&viewer), 1);

        viewer.lay_out(30, 12);
        assert_eq!(renders(&viewer), 2);
    }

    #[test]
    fn headings_emphasis_and_code_keep_patines_styles() {
        let mut viewer = inline(NOTES);
        // Wide enough for the code block's line.
        let buf = drawn(&mut viewer, 44, 11, Offset::default());

        assert_eq!(row(&buf, 0), "Retries");
        assert!(
            buf[(0, 0)]
                .modifier
                .contains(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED),
            "a level-one heading"
        );
        assert_eq!(row(&buf, 2), "A failed request is tried again up to three");
        assert!(buf[(26, 2)].modifier.contains(Modifier::BOLD), "again");
        assert!(!buf[(25, 2)].modifier.contains(Modifier::BOLD));
        assert_eq!(row(&buf, 3), "times, with delay doubling.");
        assert_eq!(buf[(12, 3)].fg, Color::Indexed(3), "inline code");
        assert_eq!(buf[(11, 3)].fg, Color::Reset);
        assert_eq!(row(&buf, 5), "Settings");
        assert!(
            buf[(0, 5)]
                .modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED)
        );
        assert_eq!(row(&buf, 7), "• tries: 3");
        assert_eq!(row(&buf, 10), "  let delay = Duration::from_millis(100);");
        assert_eq!(buf[(2, 10)].fg, Color::Indexed(5), "a highlighted keyword");
    }

    #[test]
    fn it_draws_from_the_offsets_row_on() {
        let mut viewer = inline(NOTES);

        let buf = drawn(&mut viewer, 40, 2, Offset { top: 7, left: 0 });

        assert_eq!(row(&buf, 0), "• tries: 3");
        assert_eq!(row(&buf, 1), "• delay: 100 ms");
    }

    #[test]
    fn a_wide_code_line_wraps_behind_a_dim_marker() {
        let mut viewer = inline("```\nlet total = first + second + third;\n```\n");

        assert_eq!(
            shown(&mut viewer, 20, 0),
            "\"  let total = first \"\n\"  ↪ + second + third\"\n\"  ↪ ;               \""
        );
        let buf = drawn(&mut viewer, 20, 3, Offset::default());
        assert!(buf[(2, 1)].modifier.contains(Modifier::DIM));
        assert_eq!(viewer.lay_out(20, 3).columns, 20);
    }

    #[test]
    fn a_quote_bar_takes_the_style_it_is_built_with() {
        let entry: MarkdownEntry =
            serde_json::from_str(r#"{"type": "markdown", "content": "> quoted"}"#)
                .expect("test entry parses");
        let bar_and_text = |no_color| {
            let mut viewer = markdown_content(&entry, quote_style(no_color), None);
            let buf = drawn(&mut viewer, 20, 1, Offset::default());
            assert_eq!(row(&buf, 0), "│ quoted");
            (buf[(0, 0)].clone(), buf[(2, 0)].clone())
        };

        let (bar, text) = bar_and_text(false);
        assert_eq!(
            (bar.fg, bar.modifier),
            (Color::Indexed(4), Modifier::empty())
        );
        assert_eq!(text.modifier, Modifier::empty(), "upright, not dim");
        let (bar, text) = bar_and_text(true);
        assert_eq!((bar.fg, bar.modifier), (Color::Reset, Modifier::DIM));
        assert_eq!(text.modifier, Modifier::empty());
    }

    #[test]
    fn a_long_word_in_a_task_item_wraps_under_its_text() {
        let mut viewer = inline("- [ ] https://example.com/a/very/long/path\n- [x] done\n");

        let buf = drawn(&mut viewer, 24, 3, Offset::default());

        assert_eq!(
            [row(&buf, 0), row(&buf, 1), row(&buf, 2)],
            ["☐ https://example.com/a/", "  ↪ very/long/path", "☑ done"]
        );
        assert!(buf[(0, 2)].modifier.contains(Modifier::DIM), "a done box");
        assert!(buf[(2, 2)].modifier.contains(Modifier::DIM), "and its text");
        assert!(!buf[(0, 0)].modifier.contains(Modifier::DIM), "an open box");
    }

    #[test]
    fn table_columns_follow_the_delimiter_row() {
        let mut viewer = inline("| left | right | mid |\n|:--|--:|:-:|\n| a | b | c |\n");

        let buf = drawn(&mut viewer, 40, 5, Offset::default());

        assert_eq!(row(&buf, 1), "│ left │ right │ mid │");
        assert_eq!(
            row(&buf, 3),
            "│ a    │     b │  c  │",
            "left, right and centred"
        );
    }

    /// A table cell shows as running text does: bold stays bold, and a
    /// link shows its URL dimmed after it.
    #[test]
    fn a_table_cell_keeps_its_inline_styles() {
        let mut viewer = inline(
            "| name | doc |\n|--|--|\n| **retry** | see [docs](https://example.com/retry/policy) |\n",
        );

        let buf = drawn(&mut viewer, 80, 5, Offset::default());

        assert_eq!(
            row(&buf, 3),
            "│ retry │ see docs (https://example.com/retry/policy) │"
        );
        let at = |text: &str| {
            let x = row(&buf, 3).find(text).expect("the text shows");
            let column = row(&buf, 3)[..x].chars().count();
            buf[(u16::try_from(column).expect("a narrow row"), 3)].modifier
        };
        assert!(at("retry").contains(Modifier::BOLD), "{buf:?}");
        assert!(at("https").contains(Modifier::DIM), "{buf:?}");
    }

    /// Every row of `viewer` laid out for `width` columns, without the
    /// blanks after its text.
    fn all_rows(viewer: &mut Content<Markdown>, width: u16) -> Vec<String> {
        let height = u16::try_from(viewer.lay_out(width, 1).lines).expect("a short test");
        let buf = drawn(viewer, width, height, Offset::default());
        (0..height).map(|y| row(&buf, y)).collect()
    }

    /// A table whose box would have to break a word, here a link's URL,
    /// shows as a record: each column's name, bold and dim, beside its
    /// value. The URL breaks in the room beside the names and stays dim on
    /// each of its lines.
    #[test]
    fn a_table_that_would_break_a_word_shows_as_records() {
        let mut viewer = inline(LONG_LINK_TABLE);

        assert_eq!(all_rows(&mut viewer, 40), LONG_LINK_RECORDS_AT_40);
        let buf = drawn(&mut viewer, 40, 4, Offset::default());
        for y in [0, 1] {
            let name = buf[(0, y)].modifier;
            assert!(
                name.contains(Modifier::BOLD | Modifier::DIM),
                "{y}: {name:?}"
            );
        }
        assert!(!buf[(6, 0)].modifier.contains(Modifier::DIM), "a value");
        assert!(buf[(6, 2)].modifier.contains(Modifier::DIM), "the URL");
        assert!(buf[(6, 3)].modifier.contains(Modifier::DIM), "its rest");
    }

    /// Twelve columns take 49 at one character each, so at 20 the table
    /// shows as a record of twelve lines, and no row wraps behind a `↪`
    /// with the table's border read as a quote bar.
    #[test]
    fn a_table_with_more_columns_than_fit_shows_as_records() {
        let mut viewer = inline(TWELVE_COLUMNS);

        assert_eq!(all_rows(&mut viewer, 20), vec!["a  b"; 12]);
    }

    /// The rows of a one-row table whose second cell holds `cell`, at 20
    /// columns.
    fn boxed(cell: &str) -> Vec<String> {
        all_rows(
            &mut inline(&format!("| a | b |\n|--|--|\n| x | {cell} |\n")),
            20,
        )
    }

    /// patine counts a halfwidth sound mark as the column it takes on
    /// screen, so the box is as wide as the row it holds.
    #[test]
    fn a_halfwidth_sound_mark_lines_up_in_a_table() {
        assert_eq!(
            boxed("ｶﾞｶﾞｶﾞ"),
            [
                "┌───┬────────┐",
                "│ a │   b    │",
                "├───┼────────┤",
                "│ x │ ｶﾞｶﾞｶﾞ │",
                "└───┴────────┘"
            ]
        );
    }

    /// patine gives each letter of a ligature such as Lam-Alef `لا` the
    /// column it is drawn in, so the box is as wide as the row it holds.
    #[test]
    fn a_ligature_lines_up_in_a_table() {
        assert_eq!(
            boxed("لالالا"),
            [
                "┌───┬────────┐",
                "│ a │   b    │",
                "├───┼────────┤",
                "│ x │ لالالا │",
                "└───┴────────┘"
            ]
        );
    }

    /// A table in a quote shows its records behind the quote's bar. Beside
    /// the names `operations` would break, so every value of the table
    /// stands under its name, and a plain rule separates the two records.
    #[test]
    fn a_table_in_a_quote_shows_as_records_behind_its_bar() {
        let mut viewer = inline(
            "> | step | owner |\n> |--|--|\n> | retrying | operations |\n> | waiting | queue |\n",
        );

        assert_eq!(
            all_rows(&mut viewer, 14),
            [
                "│ step",
                "│   retrying",
                "│ owner",
                "│   operations",
                "│ ────────────",
                "│ step",
                "│   waiting",
                "│ owner",
                "│   queue"
            ]
        );
        let buf = drawn(&mut viewer, 14, 9, Offset::default());
        for y in 0..9 {
            assert_eq!(buf[(0, y)].fg, Color::Indexed(4), "the bar of row {y}");
        }
        let rule = &buf[(2, 4)];
        assert_eq!((rule.fg, rule.modifier), (Color::Reset, Modifier::empty()));
    }

    /// A word that breaks in a record keeps each character whole, and a
    /// flag is one character made of two regional indicators.
    #[test]
    fn a_flag_in_a_record_is_never_cut() {
        let mut viewer = inline(&format!("| a | b |\n|--|--|\n| x | {} |\n", "🇩🇪".repeat(4)));

        let rows = all_rows(&mut viewer, 9);

        let indicators = |row: &String| {
            row.chars()
                .filter(|c| ('\u{1F1E6}'..='\u{1F1FF}').contains(c))
                .count()
        };
        assert!(rows.iter().all(|row| indicators(row) % 2 == 0), "{rows:?}");
        assert_eq!(rows, ["a  x", "b  🇩🇪🇩🇪🇩🇪", "   🇩🇪"]);
    }

    #[test]
    fn tabs_in_code_blocks_line_up_at_stops_of_four() {
        let mut viewer = inline("```\n\tx = 1;\u{1b}\n```\n");

        let buf = drawn(&mut viewer, 20, 1, Offset::default());

        assert_eq!(row(&buf, 0), "    x = 1;␛", "the block's two and a tab");
    }

    #[test]
    fn a_file_is_read_and_rendered() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("plan.md");
        fs::write(&path, "**Plan**\n").expect("file is writable");

        let mut viewer = built(&serde_json::json!({"type": "markdown", "path": path}).to_string());

        let buf = drawn(&mut viewer, 20, 1, Offset::default());
        assert_eq!(row(&buf, 0), "Plan");
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_start_of_a_file_too_long_to_read_whole_says_so_first_and_last() {
        let mut viewer = content_of(
            Ok(Source {
                text: "**Plan**\n".into(),
                cut: Some(Cut::Bytes),
                size: 9,
            }),
            quote_style(false),
            None,
        );

        let buf = drawn(&mut viewer, 40, 3, Offset::default());
        assert_eq!(row(&buf, 0), "first 8 MiB shown");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(row(&buf, 1), "Plan");
        assert_eq!(row(&buf, 2), "… the rest is cut after 8 MiB");
        assert!(buf[(0, 2)].modifier.contains(Modifier::DIM));

        assert_eq!(
            shown(&mut viewer, 10, 0),
            "\"first 8 Mi\"\n\"Plan      \"\n\"… the rest\"",
            "the notes are cut at the width, as in a diff"
        );
        assert_eq!(
            viewer.lay_out(10, 3),
            ContentSize {
                lines: 3,
                columns: 10
            }
        );
        let buf = drawn(&mut viewer, 10, 2, Offset { top: 1, left: 0 });
        assert_eq!(row(&buf, 1), "… the rest", "the notes scroll with the rows");
        assert!(buf[(0, 1)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_file_that_cannot_be_read_shows_why_as_the_content() {
        let mut viewer = built(r#"{"type": "markdown", "path": "/nowhere/plan.md"}"#);

        let buf = drawn(&mut viewer, 50, 8, Offset::default());

        assert!(row(&buf, 2).contains("│ ✗ No such file"), "{buf:?}");
        assert!(row(&buf, 4).contains("│ plan.md "), "{buf:?}");
        assert!(row(&buf, 5).contains("│ /nowhere/ "), "{buf:?}");
        assert!(viewer.failed());
    }

    #[test]
    fn empty_markdown_says_so() {
        let mut viewer = inline("\n\n");

        let buf = drawn(&mut viewer, 30, 1, Offset::default());

        assert_eq!(row(&buf, 0), "empty");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
    }
}
