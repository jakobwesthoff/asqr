// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewer of Markdown entries (spec section 7.7, ADR 34): the text
//! rendered by patine for the body's width and wrapped, without a gutter
//! and without sideways scrolling. Lines that patine leaves wider, such as
//! those of a code block, wrap behind a dim `↪`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::patine_adapter::markdown_rows;
use super::source_file::read_source;
use super::source_line::SourceLine;
use super::viewer::{ContentSize, Placeholder, Viewer};
use crate::format::MarkdownEntry;
use crate::tui::Offset;

pub struct MarkdownViewer {
    entry: MarkdownEntry,
    content: Option<Content>,
}

/// What `load` made of the entry: its Markdown with the rows it was last
/// laid out into, or a note in their place.
enum Content {
    Markdown {
        source: String,
        rows: Vec<SourceLine>,
    },
    /// The file cannot be read, or there is no text at all.
    Note(Placeholder),
}

impl MarkdownViewer {
    pub fn new(entry: &MarkdownEntry) -> Self {
        MarkdownViewer {
            entry: entry.clone(),
            content: None,
        }
    }

    fn content(&mut self) -> &mut Content {
        self.content
            .as_mut()
            .expect("the pane loads a viewer before laying it out")
    }
}

impl Viewer for MarkdownViewer {
    /// Reads the text. Rendering depends on the width, so it waits for
    /// `lay_out`.
    fn load(&mut self) {
        let source = read_source(self.entry.path.as_deref(), self.entry.content.as_deref());
        self.content = Some(match source {
            Err(unreadable) => Content::Note(unreadable.placeholder("markdown not shown")),
            Ok(source) if source.trim().is_empty() => Content::Note(Placeholder::note("empty")),
            Ok(source) => Content::Markdown {
                source,
                rows: Vec::new(),
            },
        });
    }

    /// Renders the Markdown for `width` columns. Every row fits the width,
    /// so the pane never scrolls sideways.
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        match self.content() {
            Content::Note(note) => note.lay_out(width, height),
            Content::Markdown { source, rows } => {
                let width = usize::from(width);
                *rows = markdown_rows(&patine::render_lines(source, width), width);
                // Only a character wider than the whole body is wider than
                // a row, and it shows cut.
                let widest = rows.iter().map(SourceLine::width).max().unwrap_or(0);
                ContentSize {
                    lines: rows.len(),
                    columns: widest.min(width),
                }
            }
        }
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) {
        match self.content() {
            Content::Note(note) => note.draw(area, buf, offset),
            Content::Markdown { rows, .. } => {
                for (y, row) in (area.top()..area.bottom()).zip(rows.iter().skip(offset.top)) {
                    row.draw(
                        buf,
                        Rect {
                            y,
                            height: 1,
                            ..area
                        },
                        0,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::super::source_line::columns;
    use ratatui::style::{Color, Modifier};

    use super::*;

    fn loaded(json: &str) -> MarkdownViewer {
        let entry: MarkdownEntry = serde_json::from_str(json).expect("test entry parses");
        let mut viewer = MarkdownViewer::new(&entry);
        viewer.load();
        viewer
    }

    fn inline(markdown: &str) -> MarkdownViewer {
        loaded(&serde_json::json!({"type": "markdown", "content": markdown}).to_string())
    }

    /// The viewer drawn into `width`×`height` cells at `offset`.
    fn drawn(viewer: &mut MarkdownViewer, width: u16, height: u16, offset: Offset) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        viewer.lay_out(width, height);
        viewer.draw(area, &mut buf, offset);
        buf
    }

    /// The whole content laid out `width` columns wide, each row in
    /// quotes so its width shows.
    fn shown(viewer: &mut MarkdownViewer, width: u16) -> String {
        let size = viewer.lay_out(width, 1);
        let height = u16::try_from(size.lines).expect("a short text");
        let buf = drawn(viewer, width, height, Offset::default());
        let mut rows = Vec::new();
        for y in 0..height {
            let mut row = String::new();
            let mut x = 0;
            while x < width {
                let symbol = buf[(x, y)].symbol();
                row.push_str(symbol);
                x += u16::try_from(columns(symbol).max(1)).expect("a cell is narrow");
            }
            rows.push(format!("\"{row}\""));
        }
        rows.join("\n")
    }

    /// The text of row `y`, without the blanks after it.
    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    const NOTES: &str = "# Retries\n\nA failed request is tried **again** up to three times, with `delay` doubling.\n\n## Settings\n\n- tries: 3\n- delay: 100 ms\n\n```rust\nlet delay = Duration::from_millis(100);\n```\n";

    #[test]
    fn markdown_renders_wrapped_to_the_body() {
        let mut viewer = inline(NOTES);

        insta::assert_snapshot!("notes_40", shown(&mut viewer, 40));
        insta::assert_snapshot!("notes_24", shown(&mut viewer, 24));
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
            shown(&mut viewer, 20),
            "\"  let total = first \"\n\"  ↪ + second + third\"\n\"  ↪ ;               \""
        );
        let buf = drawn(&mut viewer, 20, 3, Offset::default());
        assert!(buf[(2, 1)].modifier.contains(Modifier::DIM));
        assert_eq!(viewer.lay_out(20, 3).columns, 20);
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

        let mut viewer = loaded(&serde_json::json!({"type": "markdown", "path": path}).to_string());

        let buf = drawn(&mut viewer, 20, 1, Offset::default());
        assert_eq!(row(&buf, 0), "Plan");
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_file_that_cannot_be_read_shows_why_as_the_content() {
        let mut viewer = loaded(r#"{"type": "markdown", "path": "/nowhere/plan.md"}"#);

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 0), "markdown not shown");
        assert_eq!(row(&buf, 1), "/nowhere/plan.md");
        assert!(!row(&buf, 2).is_empty(), "the reason");
    }

    #[test]
    fn empty_markdown_says_so() {
        let mut viewer = inline("\n\n");

        let buf = drawn(&mut viewer, 30, 1, Offset::default());

        assert_eq!(row(&buf, 0), "empty");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
    }
}
