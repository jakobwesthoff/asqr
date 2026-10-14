// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewer of code entries (spec section 7.7, ADR 34): text exactly as
//! written, highlighted by patine, with its line numbers in a dim gutter.
//! Lines never wrap; the pane scrolls sideways under the gutter.

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::patine_adapter::source_line;
use super::source_file::{CUT_NOTE, Source, Unreadable, read_source};
use super::source_line::{SourceLine, columns};
use super::viewer::{ContentSize, Placeholder, Viewer};
use crate::format::CodeEntry;
use crate::tui::Offset;

pub struct CodeViewer {
    entry: CodeEntry,
    content: Option<Content>,
}

/// What `load` made of the entry: its lines, or a note in their place.
enum Content {
    Lines(Lines),
    /// The file cannot be read, or there is no text at all.
    Note(Placeholder),
}

impl CodeViewer {
    pub fn new(entry: &CodeEntry) -> Self {
        CodeViewer {
            entry: entry.clone(),
            content: None,
        }
    }

    fn content(&mut self) -> &mut Content {
        self.content
            .as_mut()
            .expect("the pane loads a viewer before laying it out")
    }

    /// What the viewer shows of `source`, as read for its entry.
    fn content_of(&self, source: Result<Source, Unreadable>) -> Content {
        match source {
            Err(unreadable) => Content::Note(unreadable.placeholder("code not shown")),
            Ok(Source { text, cut }) => {
                let lines: Vec<SourceLine> =
                    patine::highlight(&text, language(&self.entry).as_deref())
                        .iter()
                        .map(source_line)
                        .collect();
                if lines.is_empty() {
                    Content::Note(Placeholder::note("empty"))
                } else {
                    Content::Lines(Lines::new(lines, cut))
                }
            }
        }
    }
}

impl Viewer for CodeViewer {
    /// Reads and highlights the whole text once, as nothing of it depends
    /// on the body's size.
    fn load(&mut self) {
        let source = read_source(self.entry.path.as_deref(), self.entry.content.as_deref());
        self.content = Some(self.content_of(source));
    }

    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        match self.content() {
            Content::Note(note) => note.lay_out(width, height),
            Content::Lines(lines) => lines.size(usize::from(width)),
        }
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) {
        match self.content() {
            Content::Note(note) => note.draw(area, buf, offset),
            Content::Lines(lines) => lines.draw(area, buf, offset),
        }
    }
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

/// The highlighted lines, how many digits their numbers take, and whether
/// they are only the start of a longer file.
struct Lines {
    lines: Vec<SourceLine>,
    digits: usize,
    cut: bool,
}

impl Lines {
    fn new(lines: Vec<SourceLine>, cut: bool) -> Self {
        let digits = lines.len().to_string().len();
        Lines { lines, digits, cut }
    }

    /// The columns before a line's text: its number and a blank.
    fn gutter_width(&self) -> usize {
        self.digits + 1
    }

    /// The rows above the lines: the note of a cut file, which says so
    /// before the person scrolls to where the text stops.
    fn notes(&self) -> usize {
        usize::from(self.cut)
    }

    fn size(&self, width: usize) -> ContentSize {
        let widest = self.lines.iter().map(SourceLine::width).max().unwrap_or(0);
        // The note never scrolls, so it counts no wider than the body.
        let note = if self.cut {
            columns(CUT_NOTE).min(width)
        } else {
            0
        };
        ContentSize {
            lines: self.notes() + self.lines.len(),
            columns: (self.gutter_width() + widest).max(note),
        }
    }

    /// Draws the rows from `offset.top` on. The gutter stays in place
    /// while the text scrolls sideways under `offset.left`, so every line
    /// keeps its number in sight.
    fn draw(&self, area: Rect, buf: &mut Buffer, offset: Offset) {
        let dim = Style::new().dim();
        let digits = self.digits;
        let fixed = u16::try_from(self.gutter_width()).unwrap_or(u16::MAX);
        for (y, row) in (area.top()..area.bottom()).zip(offset.top..) {
            let Some(index) = row.checked_sub(self.notes()) else {
                buf.set_stringn(area.x, y, CUT_NOTE, usize::from(area.width), dim);
                continue;
            };
            let Some(line) = self.lines.get(index) else {
                break;
            };
            let number = format!("{:>digits$} ", index + 1);
            buf.set_stringn(area.x, y, number, usize::from(area.width), dim);
            let text_area = Rect {
                x: area.x.saturating_add(fixed).min(area.right()),
                y,
                width: area.width.saturating_sub(fixed),
                height: 1,
            };
            line.draw(buf, text_area, offset.left);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::super::source_line::columns;
    use ratatui::style::{Color, Modifier};

    use super::*;

    fn entry(json: &str) -> CodeEntry {
        serde_json::from_str(json).expect("test entry parses")
    }

    fn loaded(json: &str) -> CodeViewer {
        let mut viewer = CodeViewer::new(&entry(json));
        viewer.load();
        viewer
    }

    /// The viewer drawn into `width`×`height` cells at `offset`.
    fn drawn(viewer: &mut CodeViewer, width: u16, height: u16, offset: Offset) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        viewer.lay_out(width, height);
        viewer.draw(area, &mut buf, offset);
        buf
    }

    /// The rows of `buf` as the terminal shows them, each in quotes so its
    /// width shows.
    fn rows(buf: &Buffer) -> String {
        let mut rows = Vec::new();
        for y in 0..buf.area.height {
            let mut row = String::new();
            let mut x = 0;
            while x < buf.area.width {
                let symbol = buf[(x, y)].symbol();
                row.push_str(symbol);
                x += u16::try_from(columns(symbol).max(1)).expect("a cell is narrow");
            }
            rows.push(format!("\"{row}\""));
        }
        rows.join("\n")
    }

    /// The whole content, `width` columns wide from column `left` on.
    fn shown(viewer: &mut CodeViewer, width: u16, left: usize) -> String {
        let size = viewer.lay_out(width, 1);
        let height = u16::try_from(size.lines).expect("a short text");
        rows(&drawn(viewer, width, height, Offset { top: 0, left }))
    }

    /// The text of row `y`, without the blanks after it.
    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    /// The colour of the first cell of row `y` that shows `text`.
    fn colour_of(buf: &Buffer, y: u16, text: &str) -> Option<Color> {
        let row = (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>();
        let x = row.find(text).expect("the text is on the row");
        buf[(u16::try_from(x).expect("a narrow row"), y)].fg.into()
    }

    const FETCH: &str = r#"{"type": "code", "title": "fetch.rs", "content": "use std::time::Duration;\n\n// Fetches a page.\npub fn fetch(url: &str) -> Result<Response> {\n    get(url)\n}\n"}"#;

    #[test]
    fn code_shows_with_numbered_lines_under_its_language() {
        let mut viewer = loaded(FETCH);

        insta::assert_snapshot!("rust_40", shown(&mut viewer, 40, 0));
    }

    #[test]
    fn highlighting_colours_the_tokens_from_the_terminals_palette() {
        let mut viewer = loaded(FETCH);
        let buf = drawn(&mut viewer, 60, 6, Offset::default());

        assert_eq!(
            row(&buf, 3),
            "4 pub fn fetch(url: &str) -> Result<Response> {"
        );
        assert_eq!(
            colour_of(&buf, 3, "fn"),
            Some(Color::Indexed(5)),
            "a keyword"
        );
        assert_eq!(
            colour_of(&buf, 3, "fetch"),
            Some(Color::Indexed(4)),
            "a name"
        );
        assert_eq!(
            colour_of(&buf, 2, "Fetches"),
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
        let mut viewer = loaded(FETCH);

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
        let mut viewer = loaded(FETCH);

        insta::assert_snapshot!("scrolled", shown(&mut viewer, 24, 20));
        let buf = drawn(&mut viewer, 24, 6, Offset::default());
        assert_eq!(row(&buf, 3), "4 pub fn fetch(url: &st›");
        assert!(buf[(23, 3)].modifier.contains(Modifier::DIM), "the cut");
    }

    #[test]
    fn it_draws_from_the_offsets_line_on() {
        let mut viewer = loaded(FETCH);

        let buf = drawn(&mut viewer, 40, 2, Offset { top: 4, left: 0 });

        assert_eq!(row(&buf, 0), "5     get(url)");
        assert_eq!(row(&buf, 1), "6 }");
    }

    #[test]
    fn the_gutter_grows_with_the_number_of_lines() {
        let text: String = (1..=12).map(|line| format!("line {line}\\n")).collect();
        let mut viewer = loaded(&format!(r#"{{"type": "code", "content": "{text}"}}"#));

        let buf = drawn(&mut viewer, 20, 12, Offset::default());

        assert_eq!(row(&buf, 0), " 1 line 1");
        assert_eq!(row(&buf, 11), "12 line 12");
    }

    #[test]
    fn tabs_line_up_at_stops_of_four_and_controls_show_their_pictures() {
        let mut viewer = loaded(r#"{"type": "code", "content": "\tx = 1;\u001b\n"}"#);

        let buf = drawn(&mut viewer, 20, 1, Offset::default());

        assert_eq!(row(&buf, 0), "1     x = 1;␛");
    }

    #[test]
    fn windows_line_ends_do_not_show() {
        let mut viewer = loaded(r#"{"type": "code", "content": "a\r\nb\r\n"}"#);

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
    fn a_name_without_an_extension_picks_its_grammar() {
        let mut viewer =
            loaded(r#"{"type": "code", "title": "Dockerfile", "content": "FROM rust:1.97\n"}"#);

        let buf = drawn(&mut viewer, 30, 1, Offset::default());

        assert_eq!(colour_of(&buf, 0, "FROM"), Some(Color::Indexed(5)));
    }

    #[test]
    fn markdown_as_code_shows_its_source() {
        let mut viewer = loaded(
            r##"{"type": "code", "title": "README.md", "content": "# Retries\n\n**Three** tries.\n"}"##,
        );

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 0), "1 # Retries");
        assert_eq!(row(&buf, 2), "3 **Three** tries.");
        assert_eq!(
            colour_of(&buf, 2, "**Three**"),
            Some(Color::Indexed(3)),
            "highlighted as Markdown source"
        );
    }

    #[test]
    fn without_a_language_the_text_stays_plain() {
        let mut viewer = loaded(r#"{"type": "code", "content": "fn main() {}\n"}"#);

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

        let mut viewer = loaded(&json);

        let buf = drawn(&mut viewer, 20, 1, Offset::default());
        assert_eq!(row(&buf, 0), "1 fn main() {}");
        assert_eq!(colour_of(&buf, 0, "fn"), Some(Color::Indexed(5)));
    }

    #[test]
    fn the_start_of_a_file_too_long_to_read_whole_says_so_above_its_lines() {
        let mut viewer = CodeViewer::new(&entry(r#"{"type": "code", "path": "/var/log/big.log"}"#));
        viewer.content = Some(viewer.content_of(Ok(Source {
            text: "one\ntwo\n".into(),
            cut: true,
        })));

        assert_eq!(
            viewer.lay_out(12, 3),
            ContentSize {
                lines: 3,
                columns: 12
            },
            "the note counts no wider than the body"
        );
        let buf = drawn(&mut viewer, 20, 3, Offset::default());
        assert_eq!(row(&buf, 0), "first 8 MiB shown");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(row(&buf, 1), "1 one");
        assert_eq!(row(&buf, 2), "2 two");

        let buf = drawn(&mut viewer, 20, 3, Offset { top: 1, left: 0 });
        assert_eq!(row(&buf, 0), "1 one", "the note scrolls away with the rows");
        assert_eq!(row(&buf, 2), "", "nothing after the last line");
    }

    #[test]
    fn a_file_that_cannot_be_read_shows_why_as_the_content() {
        let mut viewer = loaded(r#"{"type": "code", "path": "/nowhere/fetch.rs"}"#);

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 0), "code not shown");
        assert_eq!(row(&buf, 1), "/nowhere/fetch.rs");
        assert!(!row(&buf, 2).is_empty(), "the reason");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn empty_text_says_so() {
        let mut viewer = loaded(r#"{"type": "code", "content": ""}"#);

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
