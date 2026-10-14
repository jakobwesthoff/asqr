// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The context pane (spec section 7.7): a header line that names the entry
//! shown, and below it the entry's viewer. The same pane fills the tab's
//! area when the person shows the entry full screen with `z`. Where the
//! terminal has room for no pane at all, a one-line hint stands in for it.

use std::path::Path;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};

use super::parts::cut;
use super::viewer::Viewers;
use crate::format::ContextEntry;
use crate::tui::{Extent, Offset, Page, SessionState};

/// Draws the pane for the entry `state` shows into `area`, scrolled as far
/// as the person scrolled it, and says what it showed. The header takes the
/// first line and the viewer the rest. A row without context leaves the
/// pane empty, so it keeps its place while the cursor moves on.
pub fn draw_pane(
    frame: &mut Frame,
    state: &SessionState,
    area: Rect,
    viewers: &mut Viewers,
) -> Option<Page> {
    let (key, entry) = state.shown_entry()?;
    let header = Rect { height: 1, ..area };
    let body = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };

    let (viewer, size) = viewers.laid_out(state.id(), key, entry, body.width, body.height);
    // Content reports lines wider than the body only where it cuts them, so
    // its size alone says whether the pane scrolls sideways.
    let page = Page {
        rows: Extent {
            shown: usize::from(body.height),
            total: size.lines,
        },
        columns: Extent {
            shown: usize::from(body.width),
            total: size.columns,
        },
    };
    // A smaller terminal may have left the offset past the end.
    let offset = state.pane_offset();
    let offset = Offset {
        top: offset.top.min(page.rows.last_start()),
        left: offset.left.min(page.columns.last_start()),
    };
    viewer.draw(body, frame.buffer_mut(), offset);
    let line = header_line(entry, page, offset, header.width);
    // The focused pane's header stands out the way the current tab does,
    // so the person sees where the keys go.
    let line = if state.pane_focused() {
        line.reversed()
    } else {
        line
    };
    frame.render_widget(line, header);
    Some(page)
}

/// The pane's header: the entry's name in bold, and on the right, where
/// the content is taller than the body, which of its lines show.
fn header_line(entry: &ContextEntry, page: Page, offset: Offset, width: u16) -> Line<'static> {
    let width = usize::from(width);
    let position = (page.rows.total > page.rows.shown).then(|| {
        format!(
            " {}–{} of {}",
            offset.top + 1,
            offset.top + page.rows.shown,
            page.rows.total
        )
    });
    let position_width = position.as_ref().map_or(0, |text| text.chars().count());
    let name = cut(&entry_name(entry), width.saturating_sub(position_width));
    let gap = width.saturating_sub(name.chars().count() + position_width);
    let mut spans = vec![Span::from(name).bold(), Span::from(" ".repeat(gap))];
    spans.extend(position.map(|text| Span::from(text).dim()));
    Line::from(spans)
}

/// What the header calls the entry: its `title`, the file name of its
/// path (a diff's `name`), or what kind of entry it is.
fn entry_name(entry: &ContextEntry) -> String {
    let (kind, name) = kind_and_name(entry);
    name.unwrap_or_else(|| kind.to_owned())
}

/// The kind of `entry` and its name, when it has one.
fn kind_and_name(entry: &ContextEntry) -> (&'static str, Option<String>) {
    // A path names the entry by its file name, which fits a line where
    // the whole path often does not.
    let file_name = |path: &str| {
        Path::new(path).file_name().map_or_else(
            || path.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    };
    match entry {
        ContextEntry::Image(image) => (
            "image",
            image.title.clone().or_else(|| Some(file_name(&image.path))),
        ),
        ContextEntry::Code(code) => (
            "code",
            code.title
                .clone()
                .or_else(|| code.path.as_deref().map(file_name)),
        ),
        ContextEntry::Markdown(markdown) => (
            "markdown",
            markdown
                .title
                .clone()
                .or_else(|| markdown.path.as_deref().map(file_name)),
        ),
        ContextEntry::Diff(diff) => ("diff", diff.title.clone().or_else(|| diff.name.clone())),
    }
}

/// The one line that stands in for the pane when the terminal has room
/// for it neither beside nor below the question: what the entry is, and
/// that `z` shows it.
pub fn context_hint(entry: Option<&ContextEntry>, width: u16) -> Line<'static> {
    let Some(entry) = entry else {
        return Line::default();
    };
    let text = match kind_and_name(entry) {
        (kind, Some(name)) => format!("▸ {kind}: {name} · z: full screen"),
        (kind, None) => format!("▸ {kind} · z: full screen"),
    };
    Line::from(cut(&text, usize::from(width))).dim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(json: &str) -> ContextEntry {
        serde_json::from_str(json).expect("entry parses")
    }

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn the_hint_names_the_entry_shown() {
        let hint = |entry: Option<&ContextEntry>, width: u16| text(&context_hint(entry, width));

        let cases = [
            (
                r#"{"type": "image", "path": "/pictures/cat.png"}"#,
                "▸ image: cat.png · z: full screen",
            ),
            (
                r#"{"type": "image", "title": "The cat", "path": "/pictures/cat.png"}"#,
                "▸ image: The cat · z: full screen",
            ),
            (
                r#"{"type": "image", "path": "/"}"#,
                "▸ image: / · z: full screen",
            ),
            (
                r#"{"type": "code", "path": "/src/fetch.rs"}"#,
                "▸ code: fetch.rs · z: full screen",
            ),
            (
                r#"{"type": "code", "content": "inline"}"#,
                "▸ code · z: full screen",
            ),
            (
                r#"{"type": "markdown", "path": "/notes/plan.md"}"#,
                "▸ markdown: plan.md · z: full screen",
            ),
            (
                r#"{"type": "markdown", "content": "*inline*"}"#,
                "▸ markdown · z: full screen",
            ),
            (
                r#"{"type": "diff", "name": "src/fetch.rs", "new": {"content": "x"}}"#,
                "▸ diff: src/fetch.rs · z: full screen",
            ),
            (
                r#"{"type": "diff", "new": {"content": "x"}}"#,
                "▸ diff · z: full screen",
            ),
        ];
        for (json, expected) in cases {
            assert_eq!(hint(Some(&entry(json)), 78), expected, "{json}");
        }
        assert_eq!(
            hint(
                Some(&entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#)),
                20
            ),
            "▸ image: cat.png · …",
            "cut to the line"
        );
        assert_eq!(hint(None, 78), "", "a row without context leaves it empty");
    }

    #[test]
    fn the_header_names_the_entry_by_title_file_name_or_kind() {
        let fits = Page {
            rows: Extent {
                shown: 10,
                total: 3,
            },
            columns: Extent::default(),
        };
        let header = |json: &str| text(&header_line(&entry(json), fits, Offset::default(), 30));

        let cases = [
            (
                r#"{"type": "image", "path": "/pictures/cat.png"}"#,
                "cat.png",
            ),
            (r#"{"type": "code", "title": "Log", "content": "x"}"#, "Log"),
            (r#"{"type": "code", "content": "x"}"#, "code"),
            (
                r#"{"type": "markdown", "path": "/notes/plan.md"}"#,
                "plan.md",
            ),
            (r#"{"type": "markdown", "content": "x"}"#, "markdown"),
            (
                r#"{"type": "diff", "name": "src/fetch.rs", "new": {"content": "x"}}"#,
                "src/fetch.rs",
            ),
            (r#"{"type": "diff", "new": {"content": "x"}}"#, "diff"),
        ];
        for (json, expected) in cases {
            assert_eq!(header(json).trim_end(), expected, "{json}");
            assert_eq!(header(json).chars().count(), 30, "fills the line: {json}");
        }
    }

    #[test]
    fn the_header_says_which_lines_show_when_the_content_is_taller() {
        let page = Page {
            rows: Extent {
                shown: 12,
                total: 120,
            },
            columns: Extent::default(),
        };
        let titled = entry(
            r#"{"type": "code", "title": "A very long title for a short line", "content": "x"}"#,
        );

        let line = text(&header_line(&titled, page, Offset { top: 3, left: 0 }, 30));

        assert_eq!(line, "A very long title… 4–15 of 120");
    }
}
