// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewer of code entries (spec section 7.7, ADR 34): text exactly as
//! written, highlighted by patine, with its line numbers in a dim gutter.
//! Lines never wrap; the pane scrolls sideways under the gutter.

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Widget;

use super::patine_adapter::source_line;
use super::source_file::{Source, Unreadable, between_cut_notes, read_source};
use super::source_line::{GutterRow, Note, SourceLine, rows_from};
use super::theme;
use super::viewer::{Content, ContentSize, Placeholder, Viewer};
use crate::format::CodeEntry;
use crate::graphics::PlacedImage;
use crate::tui::Offset;

/// The viewer of `entry`: reads and highlights the whole text once, as
/// nothing of it depends on the body's size. The home folder `home` shows
/// as `~` in the path of a file that cannot be read.
pub(super) fn code_content(entry: &CodeEntry, home: Option<&Path>) -> Content<Lines> {
    let source = read_source(entry.path.as_deref(), entry.content.as_deref());
    content_of(entry, source, home)
}

/// What the viewer of `entry` shows of `source`, as read for the entry:
/// its lines, a card when the file cannot be read, or a note when there is
/// no text at all.
fn content_of(
    entry: &CodeEntry,
    source: Result<Source, Unreadable>,
    home: Option<&Path>,
) -> Content<Lines> {
    match source {
        Err(unreadable) => Content::Problem(unreadable.card(home)),
        Ok(Source { text, cut }) => {
            let lines: Vec<SourceLine> =
                patine::highlight(&text, language_for(entry, &text).as_deref())
                    .iter()
                    .map(source_line)
                    .collect();
            if lines.is_empty() {
                Content::Note(Placeholder::note("empty"))
            } else {
                Content::Body(Lines::new(lines, cut))
            }
        }
    }
}

/// How long a text may be and still be highlighted. A code entry shows
/// only once it is highlighted, and the log grammar takes about 0.19 ms
/// a line: 23 s for the 8 MiB asqr reads of a log (measured on
/// 2026-10-16). Up to 1 MiB that stays at a few seconds.
const HIGHLIGHT_LIMIT: usize = 1024 * 1024;

/// The language to highlight `text`, the text of `entry`, in: none for a
/// text longer than [`HIGHLIGHT_LIMIT`], else the entry's [`language`].
fn language_for(entry: &CodeEntry, text: &str) -> Option<String> {
    (text.len() <= HIGHLIGHT_LIMIT)
        .then(|| language(entry))
        .flatten()
}

/// The language to highlight `entry` in: its `language`, else what the
/// file name of its `path`, then of its `title`, says. patine falls back
/// to plain text for a name it does not know.
fn language(entry: &CodeEntry) -> Option<String> {
    entry
        .language
        .clone()
        .or_else(|| entry.path.as_deref().and_then(kind_of_file))
        .or_else(|| entry.title.as_deref().and_then(kind_of_file))
}

/// What a file name says about its language: its extension, or the whole
/// name when it has none, as `Makefile` and `Dockerfile` do. syntect's
/// grammars list such names among their extensions.
fn kind_of_file(name: &str) -> Option<String> {
    let path = Path::new(name);
    path.extension()
        .or_else(|| path.file_name())
        .map(|kind| kind.to_string_lossy().into_owned())
}

/// The highlighted lines with the notes of a cut file around them, and how
/// many digits the lines' numbers take.
pub(super) struct Lines {
    rows: Vec<Row>,
    digits: usize,
}

enum Row {
    /// A line of the text, numbered from 1.
    Line {
        number: usize,
        text: SourceLine,
    },
    Note(Note),
}

impl Lines {
    /// `lines`, the whole text or, when `cut`, the start of a longer file.
    fn new(lines: Vec<SourceLine>, cut: bool) -> Self {
        let digits = lines.len().to_string().len();
        let numbered = (1..)
            .zip(lines)
            .map(|(number, text)| Row::Line { number, text });
        Lines {
            rows: between_cut_notes(cut, numbered, Row::Note).collect(),
            digits,
        }
    }

    /// The columns before a line's text: its number and a blank.
    fn gutter_width(&self) -> usize {
        self.digits + 1
    }
}

impl Viewer for Lines {
    /// Lines never wrap, so only the notes depend on the width.
    fn lay_out(&mut self, width: u16, _height: u16) -> ContentSize {
        let columns = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Line { text, .. } => self.gutter_width() + text.width(),
                Row::Note(note) => note.columns(usize::from(width)),
            })
            .max()
            .unwrap_or(0);
        ContentSize {
            lines: self.rows.len(),
            columns,
        }
    }

    /// Draws the rows from `offset.top` on, each line with its number in
    /// a dim gutter.
    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        let digits = self.digits;
        for (row_area, row) in rows_from(area, &self.rows, offset.top) {
            match row {
                Row::Line { number, text } => GutterRow {
                    gutter: Line::styled(format!("{number:>digits$} "), theme::secondary()),
                    line: text,
                    left: offset.left,
                }
                .render(row_area, buf),
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

    use super::super::test_support::{drawn, row, shown, style_at};
    use super::*;

    fn entry(json: &str) -> CodeEntry {
        serde_json::from_str(json).expect("test entry parses")
    }

    fn built(json: &str) -> Content<Lines> {
        code_content(&entry(json), None)
    }

    const FETCH: &str = r#"{"type": "code", "title": "fetch.rs", "content": "use std::time::Duration;\n\n// Fetches a page.\npub fn fetch(url: &str) -> Result<Response> {\n    get(url)\n}\n"}"#;

    #[test]
    fn code_shows_with_numbered_lines_under_its_language() {
        let mut viewer = built(FETCH);

        insta::assert_snapshot!("rust_40", shown(&mut viewer, 40, 0));
    }

    #[test]
    fn highlighting_colours_the_tokens_from_the_terminals_palette() {
        let mut viewer = built(FETCH);
        let buf = drawn(&mut viewer, 60, 6, Offset::default());

        assert_eq!(
            row(&buf, 3),
            "4 pub fn fetch(url: &str) -> Result<Response> {"
        );
        assert_eq!(
            style_at(&buf, "fn").fg,
            Some(Color::Indexed(5)),
            "a keyword"
        );
        assert_eq!(
            style_at(&buf, "fetch").fg,
            Some(Color::Indexed(4)),
            "a name"
        );
        assert_eq!(
            style_at(&buf, "Fetches").fg,
            Some(Color::Indexed(2)),
            "a comment"
        );
        assert!(
            buf[(0, 3)].modifier.contains(Modifier::DIM),
            "the gutter is dim"
        );
    }

    #[test]
    fn the_content_is_as_wide_as_the_gutter_and_the_widest_line() {
        let mut viewer = built(FETCH);

        let size = viewer.lay_out(30, 10);

        let widest = "pub fn fetch(url: &str) -> Result<Response> {".len();
        assert_eq!(
            size,
            ContentSize {
                lines: 6,
                columns: 2 + widest
            }
        );
        assert_eq!(viewer.lay_out(80, 3), size, "nothing wraps");
    }

    #[test]
    fn a_long_line_scrolls_sideways_under_a_gutter_that_stays() {
        let mut viewer = built(FETCH);

        insta::assert_snapshot!("scrolled", shown(&mut viewer, 24, 20));
        let buf = drawn(&mut viewer, 24, 6, Offset::default());
        assert_eq!(row(&buf, 3), "4 pub fn fetch(url: &st›");
        assert!(buf[(23, 3)].modifier.contains(Modifier::DIM), "the cut");
    }

    #[test]
    fn it_draws_from_the_offsets_line_on() {
        let mut viewer = built(FETCH);

        let buf = drawn(&mut viewer, 40, 2, Offset { top: 4, left: 0 });

        assert_eq!(row(&buf, 0), "5     get(url)");
        assert_eq!(row(&buf, 1), "6 }");
    }

    #[test]
    fn the_gutter_grows_with_the_number_of_lines() {
        let text: String = (1..=12).map(|line| format!("line {line}\\n")).collect();
        let mut viewer = built(&format!(r#"{{"type": "code", "content": "{text}"}}"#));

        let buf = drawn(&mut viewer, 20, 12, Offset::default());

        assert_eq!(row(&buf, 0), " 1 line 1");
        assert_eq!(row(&buf, 11), "12 line 12");
    }

    #[test]
    fn tabs_line_up_at_stops_of_four_and_controls_show_their_pictures() {
        let mut viewer = built(r#"{"type": "code", "content": "\tx = 1;\u001b\n"}"#);

        let buf = drawn(&mut viewer, 20, 1, Offset::default());

        assert_eq!(row(&buf, 0), "1     x = 1;␛");
    }

    #[test]
    fn windows_line_ends_do_not_show() {
        let mut viewer = built(r#"{"type": "code", "content": "a\r\nb\r\n"}"#);

        assert_eq!(shown(&mut viewer, 10, 0), "\"1 a       \"\n\"2 b       \"");
    }

    // -----------------------------------------------------------------
    // The language
    // -----------------------------------------------------------------

    #[test]
    fn the_language_comes_from_the_entry_then_the_path_then_the_title() {
        let language = |json: &str| language(&entry(json));

        assert_eq!(
            language(
                r#"{"type": "code", "language": "toml", "path": "/a.rs", "title": "b.py", "content": ""}"#
            ),
            Some("toml".into())
        );
        assert_eq!(
            language(r#"{"type": "code", "path": "/src/a.rs", "title": "b.py"}"#),
            Some("rs".into())
        );
        assert_eq!(
            language(r#"{"type": "code", "title": "b.py", "content": ""}"#),
            Some("py".into())
        );
        assert_eq!(
            language(r#"{"type": "code", "path": "/project/Makefile"}"#),
            Some("Makefile".into()),
            "a name without an extension counts whole"
        );
        assert_eq!(language(r#"{"type": "code", "content": ""}"#), None);
    }

    #[test]
    fn the_language_is_dropped_above_one_mib() {
        let rust = entry(r#"{"type": "code", "language": "rust", "content": ""}"#);

        assert_eq!(
            language_for(&rust, &"x".repeat(HIGHLIGHT_LIMIT)),
            Some("rust".into()),
            "up to the limit"
        );
        assert_eq!(language_for(&rust, &"x".repeat(HIGHLIGHT_LIMIT + 1)), None);
    }

    #[test]
    fn a_code_entry_above_one_mib_shows_plain() {
        let lines = HIGHLIGHT_LIMIT / "fn main() {}\n".len() + 1;
        let text = "fn main() {}\n".repeat(lines);
        let json = serde_json::json!({"type": "code", "language": "rust", "content": text});
        let mut viewer = built(&json.to_string());

        let buf = drawn(&mut viewer, 30, 1, Offset::default());

        assert!(row(&buf, 0).ends_with("fn main() {}"), "{:?}", row(&buf, 0));
        assert_eq!(style_at(&buf, "fn").fg, Some(Color::Reset));
    }

    #[test]
    fn a_name_without_an_extension_picks_its_grammar() {
        let mut viewer =
            built(r#"{"type": "code", "title": "Dockerfile", "content": "FROM rust:1.97\n"}"#);

        let buf = drawn(&mut viewer, 30, 1, Offset::default());

        assert_eq!(style_at(&buf, "FROM").fg, Some(Color::Indexed(5)));
    }

    #[test]
    fn markdown_as_code_shows_its_source() {
        let mut viewer = built(
            r##"{"type": "code", "title": "README.md", "content": "# Retries\n\n**Three** tries.\n"}"##,
        );

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 0), "1 # Retries");
        assert_eq!(row(&buf, 2), "3 **Three** tries.");
        assert_eq!(
            style_at(&buf, "**Three**").fg,
            Some(Color::Indexed(3)),
            "highlighted as Markdown source"
        );
    }

    #[test]
    fn without_a_language_the_text_stays_plain() {
        let mut viewer = built(r#"{"type": "code", "content": "fn main() {}\n"}"#);

        let buf = drawn(&mut viewer, 20, 1, Offset::default());

        assert!((2..14).all(|x| buf[(x, 0)].fg == Color::Reset));
    }

    // -----------------------------------------------------------------
    // Files
    // -----------------------------------------------------------------

    #[test]
    fn a_file_is_read_and_highlighted_by_its_extension() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("main.rs");
        fs::write(&path, "fn main() {}\n").expect("file is writable");
        let json = serde_json::json!({"type": "code", "path": path}).to_string();

        let mut viewer = built(&json);

        let buf = drawn(&mut viewer, 20, 1, Offset::default());
        assert_eq!(row(&buf, 0), "1 fn main() {}");
        assert_eq!(style_at(&buf, "fn").fg, Some(Color::Indexed(5)));
    }

    #[test]
    fn the_start_of_a_file_too_long_to_read_whole_says_so_above_and_below_its_lines() {
        let source = Ok(Source {
            text: "one\ntwo\n".into(),
            cut: true,
        });
        let mut viewer = content_of(
            &entry(r#"{"type": "code", "path": "/var/log/big.log"}"#),
            source,
            None,
        );

        assert_eq!(
            viewer.lay_out(12, 3),
            ContentSize {
                lines: 4,
                columns: 12
            },
            "the notes count no wider than the body"
        );
        let buf = drawn(&mut viewer, 40, 4, Offset::default());
        assert_eq!(row(&buf, 0), "first 8 MiB shown");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(row(&buf, 1), "1 one");
        assert_eq!(row(&buf, 2), "2 two");
        assert_eq!(row(&buf, 3), "… the rest is cut after 8 MiB");
        assert!(buf[(0, 3)].modifier.contains(Modifier::DIM));

        let buf = drawn(&mut viewer, 40, 4, Offset { top: 1, left: 0 });
        assert_eq!(row(&buf, 0), "1 one", "the note scrolls away with the rows");
        assert_eq!(row(&buf, 2), "… the rest is cut after 8 MiB");
        assert_eq!(row(&buf, 3), "", "nothing after the note at the end");

        let buf = drawn(&mut viewer, 40, 1, Offset { top: 3, left: 4 });
        assert_eq!(
            row(&buf, 0),
            "… the rest is cut after 8 MiB",
            "the note does not scroll sideways"
        );

        let buf = drawn(&mut viewer, 10, 4, Offset::default());
        assert_eq!(row(&buf, 0), "first 8 Mi", "cut at the width");
        assert_eq!(row(&buf, 3), "… the rest");
    }

    #[test]
    fn a_body_narrower_than_the_gutter_shows_what_fits_of_it() {
        let mut viewer = built(FETCH);

        let buf = drawn(&mut viewer, 1, 1, Offset::default());
        assert_eq!(row(&buf, 0), "1");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));

        let buf = drawn(&mut viewer, 3, 1, Offset::default());
        assert_eq!(row(&buf, 0), "1 ›", "one column of text, cut");
    }

    #[test]
    fn a_file_that_cannot_be_read_shows_why_as_the_content() {
        let mut viewer = built(r#"{"type": "code", "path": "/nowhere/fetch.rs"}"#);

        let buf = drawn(&mut viewer, 50, 8, Offset::default());

        assert!(row(&buf, 2).contains("│ ✗ No such file"), "{buf:?}");
        assert!(row(&buf, 4).contains("│ fetch.rs "), "{buf:?}");
        assert!(row(&buf, 5).contains("│ /nowhere/ "), "{buf:?}");
        assert!(viewer.failed());
    }

    #[test]
    fn empty_text_says_so() {
        let mut viewer = built(r#"{"type": "code", "content": ""}"#);

        assert_eq!(
            viewer.lay_out(30, 5),
            ContentSize {
                lines: 1,
                columns: 5
            }
        );
        let buf = drawn(&mut viewer, 30, 1, Offset::default());
        assert_eq!(row(&buf, 0), "empty");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
    }
}
