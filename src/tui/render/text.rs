// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Markdown of the intro, question texts, option labels and option
//! descriptions (spec section 7.10, ADRs 35 and 55): rendered by patine
//! for the width the text gets,
//! with every newline a line break and raw HTML as written, laid out by
//! the adapter's one rule for Markdown, so a line patine leaves wider
//! wraps behind a dim `↪`. The first line of a text may get fewer columns
//! than the rest, for the marker of a default option.

use std::cell::RefCell;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use patine::{Html, QuoteStyle, RenderOptions, SoftBreaks, StyledLine};
use ratatui::text::{Line, Span};

use super::loading::contain;
use super::patine_adapter::{markdown_rows_from, plain_rows_from, source_line};
use super::source_line::SourceLine;

/// How the text fields differ from Markdown documents (ADR 35): a single
/// newline breaks the line, and raw HTML shows as written. Quotes take
/// `quote`, as in a Markdown entry.
const fn field_options(quote: QuoteStyle) -> RenderOptions {
    RenderOptions::new()
        .with_soft_breaks(SoftBreaks::Break)
        .with_html(Html::Literal)
        .with_quote_style(quote)
}

/// `source` rendered for `width` columns, as lines of at most that width,
/// with quotes in `quote`. Text without any content renders to no lines at
/// all.
pub(super) fn field_lines(source: &str, width: usize, quote: QuoteStyle) -> Vec<Line<'static>> {
    field_lines_from(source, width, width, quote)
}

/// `source` rendered as [`field_lines`] renders it, with its first line at
/// most `first` columns wide and every later line `width`, for a text
/// whose first line shares its row with the marker of a default option
/// (ADR 79).
pub(super) fn field_lines_from(
    source: &str,
    first: usize,
    width: usize,
    quote: QuoteStyle,
) -> Vec<Line<'static>> {
    field_lines_with(patine::render_lines_with, source, first, width, quote)
}

/// Renders Markdown to styled lines for a width. Tests put a renderer in
/// its place that panics on any text.
type Render = fn(&str, usize, &RenderOptions) -> Vec<StyledLine>;

/// [`field_lines_from`] with `render` in place of patine's.
fn field_lines_with(
    render: Render,
    source: &str,
    first: usize,
    width: usize,
    quote: QuoteStyle,
) -> Vec<Line<'static>> {
    let options = field_options(quote).with_first_line_width(first);
    let rows = match rendered(render, source, width, &options) {
        Some(lines) => markdown_rows_from(&lines, first, width),
        None => plain_rows_from(&patine::highlight(source, None), first, width),
    };
    rows.iter()
        .map(|row| {
            Line::from(
                row.pieces()
                    .map(|(text, style)| Span::styled(text.to_owned(), style))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// `source` rendered for `width` columns as patine lays it out, with
/// quotes in `quote`: each line whole, also where patine leaves it wider,
/// for a text shown on one line that a cut ends.
pub(super) fn field_source_lines(source: &str, width: usize, quote: QuoteStyle) -> Vec<SourceLine> {
    field_source_lines_with(patine::render_lines_with, source, width, quote)
}

/// [`field_source_lines`] with `render` in place of patine's.
fn field_source_lines_with(
    render: Render,
    source: &str,
    width: usize,
    quote: QuoteStyle,
) -> Vec<SourceLine> {
    let lines = rendered(render, source, width, &field_options(quote))
        .unwrap_or_else(|| patine::highlight(source, None));
    lines.iter().map(source_line).collect()
}

/// How many sources that made the parser panic are remembered.
const FAILED_TEXTS: usize = 64;

thread_local! {
    /// Fingerprints of the texts the parser panicked on, on this thread.
    static FAILED: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
}

/// The lines `render` makes of `source`, or `None` where the parser
/// panics on it.
///
/// The fields render on the drawing thread, once to count their rows and
/// again to draw them, every frame. A text that made the parser panic is
/// remembered, so it is not parsed again for each of them, and its panic
/// is logged once. The panic message names a place in the parser's code,
/// never the text. A panic does not depend on the width or on the
/// options, so the text alone is the key.
fn rendered(
    render: Render,
    source: &str,
    width: usize,
    options: &RenderOptions,
) -> Option<Vec<StyledLine>> {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let fingerprint = hasher.finish();
    if FAILED.with_borrow(|failed| failed.contains(&fingerprint)) {
        return None;
    }
    match contain(|| render(source, width, options)) {
        Ok(lines) => Some(lines),
        Err(message) => {
            tracing::warn!(
                panic = message,
                "the Markdown parser panicked; showing the text as plain text"
            );
            FAILED.with_borrow_mut(|failed| {
                if failed.len() >= FAILED_TEXTS {
                    failed.clear();
                }
                failed.insert(fingerprint);
            });
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::patine_adapter::quote_style;
    use super::super::source_line::columns;
    use super::super::test_support::{
        LONG_LINK_RECORDS_AT_40, LONG_LINK_TABLE, TWELVE_COLUMNS, logged,
    };
    use ratatui::style::{Color, Modifier, Style};

    use super::*;

    /// `source` as the fields render it with colours.
    fn lines(source: &str, width: usize) -> Vec<Line<'static>> {
        field_lines(source, width, quote_style(false))
    }

    /// The text of each line.
    fn texts(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    /// Each span of `line` with its style.
    fn spans(line: &Line) -> Vec<(String, Style)> {
        line.spans
            .iter()
            .map(|span| (span.content.to_string(), span.style))
            .collect()
    }

    // -----------------------------------------------------------------
    // Where the parser panics
    // -----------------------------------------------------------------

    thread_local! {
        /// How often the renderer that panics was asked on this thread.
        static PARSER_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn parser_panics(_: &str, _: usize, _: &RenderOptions) -> Vec<StyledLine> {
        PARSER_CALLS.set(PARSER_CALLS.get() + 1);
        panic!("the parser panics");
    }

    /// The text of each piece of each source line.
    fn source_texts(lines: &[SourceLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.pieces().map(|(text, _)| text).collect())
            .collect()
    }

    #[test]
    fn a_text_the_parser_panics_on_shows_as_written_within_its_first_width() {
        let rows = field_lines_with(
            parser_panics,
            "**alpha** beta gamma delta",
            11,
            20,
            quote_style(false),
        );

        let texts = texts(&rows);
        assert_eq!(texts[0], "**alpha** b", "the first row fits 11 columns");
        assert_eq!(texts[1], "↪ eta gamma delta", "the rest takes 20");
    }

    #[test]
    fn a_label_the_parser_panics_on_shows_each_line_whole_on_one_line() {
        let rows = field_source_lines_with(
            parser_panics,
            "**one**\nsecond line is longer than the room",
            8,
            quote_style(false),
        );

        assert_eq!(
            source_texts(&rows),
            ["**one**", "second line is longer than the room"]
        );
    }

    #[test]
    fn a_text_the_parser_fails_on_is_asked_once() {
        PARSER_CALLS.set(0);
        let text = "**again** and again";

        for _ in 0..3 {
            field_lines_with(parser_panics, text, 20, 20, quote_style(false));
            field_source_lines_with(parser_panics, text, 20, quote_style(false));
        }

        assert_eq!(PARSER_CALLS.get(), 1);
    }

    #[test]
    fn a_memory_of_failed_texts_that_is_full_starts_over() {
        PARSER_CALLS.set(0);
        for number in 0..=FAILED_TEXTS {
            field_lines_with(
                parser_panics,
                &format!("text {number}"),
                20,
                20,
                quote_style(false),
            );
        }
        assert_eq!(PARSER_CALLS.get(), FAILED_TEXTS + 1);

        field_lines_with(parser_panics, "text 0", 20, 20, quote_style(false));

        assert_eq!(PARSER_CALLS.get(), FAILED_TEXTS + 2, "forgotten again");
    }

    #[test]
    fn a_text_the_parser_fails_on_is_logged_once_without_the_text() {
        // `logged` keeps this test independent of the tests beside it, which
        // may reach the `warn!` first without a subscriber.
        let log = logged(|| {
            for _ in 0..3 {
                field_lines_with(parser_panics, "secret words", 20, 20, quote_style(false));
            }
        });

        assert_eq!(log.lines().count(), 1, "{log}");
        assert!(
            log.contains("the Markdown parser panicked; showing the text as plain text"),
            "{log}"
        );
        assert!(log.contains("the parser panics"), "{log}");
        assert!(!log.contains("secret words"), "{log}");
    }

    /// `markdown` 1.0.0 panics on this text. Once a later version handles
    /// it, it renders; the tests above keep the fallback pinned.
    #[test]
    fn an_unclosed_fence_in_a_numbered_item_before_a_bullet_does_not_end_a_field() {
        assert!(!lines("1. ~~~\n*", 40).is_empty());
    }

    #[test]
    fn a_text_takes_its_first_width_on_its_first_line_only() {
        let words = "a01 a02 a03 a04 a05 a06 a07 a08 a09 a10 a11 a12";

        assert_eq!(
            texts(&field_lines_from(words, 20, 31, quote_style(false))),
            ["a01 a02 a03 a04 a05", "a06 a07 a08 a09 a10 a11 a12"]
        );
    }

    #[test]
    fn emphasis_and_inline_code_keep_patines_styles() {
        let lines = lines("a **bold** *it* `code`", 40);

        assert_eq!(
            spans(&lines[0]),
            [
                ("a ".to_owned(), Style::new()),
                ("bold".to_owned(), Style::new().add_modifier(Modifier::BOLD)),
                (" ".to_owned(), Style::new()),
                ("it".to_owned(), Style::new().add_modifier(Modifier::ITALIC)),
                (" ".to_owned(), Style::new()),
                ("code".to_owned(), Style::new().fg(Color::Indexed(3))),
            ]
        );
    }

    #[test]
    fn a_quote_shows_its_text_upright_behind_the_bar() {
        let text = [
            ("a ".to_owned(), Style::new()),
            ("b".to_owned(), Style::new().add_modifier(Modifier::ITALIC)),
            (" c".to_owned(), Style::new()),
        ];
        let quoted = |no_color| spans(&field_lines("> a *b* c", 40, quote_style(no_color))[0]);

        let coloured = quoted(false);
        assert_eq!(
            coloured[0],
            ("│ ".to_owned(), Style::new().fg(Color::Indexed(4))),
            "a blue bar"
        );
        assert_eq!(coloured[1..], text, "emphasis shows inside the quote");
        let colourless = quoted(true);
        assert_eq!(colourless[0], ("│ ".to_owned(), Style::new().dim()));
        assert_eq!(colourless[1..], text);
    }

    #[test]
    fn lists_paragraphs_and_headings_render() {
        assert_eq!(
            texts(&lines("# Plan\n\n- one\n- two\n\n1. first\n2. second", 40)),
            ["Plan", "", "• one", "• two", "", "1. first", "2. second"]
        );
    }

    #[test]
    fn prose_wraps_at_the_width() {
        assert_eq!(
            texts(&lines("Some text that wraps around nicely here.", 24)),
            ["Some text that wraps", "around nicely here."]
        );
    }

    #[test]
    fn a_wide_code_line_wraps_behind_the_dim_marker() {
        let lines = lines("```\nlet total = first + second + third;\n```\n", 20);

        assert_eq!(
            texts(&lines),
            ["  let total = first ", "  ↪ + second + third", "  ↪ ;"]
        );
        assert_eq!(lines[1].spans[1].style, Style::new().dim());
        assert!(
            lines
                .iter()
                .all(|line| columns(&texts(std::slice::from_ref(line))[0]) <= 20)
        );
    }

    /// The text fields render a table as a Markdown entry does: a table
    /// whose box would have to break a word, here a link's URL, shows as
    /// a record, each column's name beside its value.
    #[test]
    fn a_table_that_would_break_a_word_shows_as_records() {
        assert_eq!(texts(&lines(LONG_LINK_TABLE, 40)), LONG_LINK_RECORDS_AT_40);
    }

    /// A table with more columns than fit at one character each shows as
    /// records too, and no row wraps behind a `↪`.
    #[test]
    fn a_table_with_more_columns_than_fit_shows_as_records() {
        assert_eq!(texts(&lines(TWELVE_COLUMNS, 20)), vec!["a  b"; 12]);
    }

    /// patine counts a halfwidth sound mark as the column it takes on
    /// screen, so it wraps such text between its words, and no word breaks
    /// behind a `↪`.
    #[test]
    fn a_halfwidth_sound_mark_in_running_text_wraps_between_words() {
        assert_eq!(texts(&lines("ｶﾞｶﾞ ｶﾞｶﾞ ｶﾞｶﾞ ｶﾞｶﾞ", 12)), ["ｶﾞｶﾞ ｶﾞｶﾞ", "ｶﾞｶﾞ ｶﾞｶﾞ"]);
    }

    #[test]
    fn a_single_newline_breaks_the_line() {
        assert_eq!(
            texts(&lines("first line\nsecond line\n\nnext paragraph", 40)),
            ["first line", "second line", "", "next paragraph"]
        );
        assert_eq!(
            texts(&lines("first\r\nsecond", 40)),
            ["first", "second"],
            "a Windows line ending too"
        );
        assert_eq!(
            texts(&lines("> quoted\nsecond", 40)),
            ["│ quoted", "│ second"],
            "inside a quote, behind its bar"
        );
    }

    #[test]
    fn a_single_newline_in_a_list_item_goes_on_under_its_text() {
        assert_eq!(
            texts(&lines("- item\nmore\n- next", 40)),
            ["• item", "  more", "• next"]
        );
    }

    #[test]
    fn code_keeps_its_newlines_and_angle_brackets() {
        assert_eq!(
            texts(&lines("```\nlet v: Vec<u8> = a;\nnext\n```", 40)),
            ["  let v: Vec<u8> = a;", "  next"]
        );
        assert_eq!(
            texts(&lines("`Vec<u8>` and `a\nb`", 40)),
            ["Vec<u8> and a b"],
            "inline code shows a newline as a space"
        );
    }

    #[test]
    fn raw_html_shows_as_written() {
        assert_eq!(
            texts(&lines("Use a Vec<String> here.", 40)),
            ["Use a Vec<String> here."]
        );
        assert_eq!(
            texts(&lines("<div>\nblock\n</div>\n\nafter", 40)),
            ["<div>", "block", "</div>", "", "after"]
        );
        assert_eq!(texts(&lines("<!-- note -->", 40)), ["<!-- note -->"]);
    }

    #[test]
    fn text_glued_to_html_wraps_as_one_word() {
        // `Use a Vec` fits 16 columns and `Use a Vec<String>` does not, so
        // a break between `Vec` and `<String>` would show here.
        assert_eq!(
            texts(&lines("Use a Vec<String> here", 16)),
            ["Use a", "Vec<String> here"]
        );
    }

    #[test]
    fn an_autolink_stays_a_link() {
        let lines = lines("<https://example.com>", 60);

        assert!(!texts(&lines)[0].contains('<'), "{:?}", texts(&lines));
        assert_eq!(lines[0].spans[0].content, "https://example.com");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn text_without_content_has_no_lines() {
        assert!(lines("", 40).is_empty());
        assert!(lines("   ", 40).is_empty());
    }
}
