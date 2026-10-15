// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The context pane (spec section 7.7): a header line that names the entry
//! shown, and below it the entry's viewer. The same pane fills the tab's
//! area when the person shows the entry full screen with `z`. Where the
//! terminal has room for no pane at all, a one-line hint stands in for it.

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::source_line::columns;
use super::theme;
use super::viewer::Viewers;
use super::wrap::cut;
use crate::format::ContextEntry;
use crate::tui::{Extent, Offset, Page, SessionState};

/// Draws the pane for the entry `state` shows into `area`, scrolled as far
/// as the person scrolled it, and says what it showed. The header takes the
/// first line and the viewer the rest. A row without context leaves the
/// pane empty, so it keeps its place while the cursor moves on.
pub fn draw_pane(
    area: Rect,
    buf: &mut Buffer,
    state: &SessionState,
    viewers: &mut Viewers,
) -> Option<Page> {
    let (key, entry) = state.shown_entry()?;
    let [header, body] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);

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
    let offset = page.clamp(state.pane_offset());
    viewer.draw(body, buf, offset);
    let line = header_line(entry, page, offset, header.width, !state.pane_focused());
    // The focused pane's header takes the focus fill from the question's
    // current row, so the person sees where the keys go.
    let line = if state.pane_focused() {
        theme::focus_row(line)
    } else {
        line
    };
    line.render(header, buf);
    Some(page)
}

/// The pane's header: the entry's name in bold, and on the right, where
/// the content is taller than the body, which of its lines show.
///
/// Where the content is taller or wider than the body and `offer_tab`
/// holds, as it does while the pane lacks the focus, the right end also
/// names `tab`, the key that moves into the pane to scroll it. The key bar
/// is the first to drop its `tab` hint on a narrow terminal, which leaves
/// the header the only place that names it. The hint goes before the name
/// would have to be cut.
fn header_line(
    entry: &ContextEntry,
    page: Page,
    offset: Offset,
    width: u16,
    offer_tab: bool,
) -> Line<'static> {
    let width = usize::from(width);
    let position = page.rows.position(offset.top);
    let taller = position.is_some();
    let wider = page.columns.total > page.columns.shown;
    let name = entry_name(entry);
    let with_tab = match &position {
        Some(position) => format!(" {position} · tab"),
        None => " tab".to_owned(),
    };
    let right = match position {
        _ if offer_tab && (taller || wider) && columns(&name) + columns(&with_tab) <= width => {
            Some(with_tab)
        }
        Some(position) => Some(format!(" {position}")),
        None => None,
    };
    let right_width = right.as_deref().map_or(0, columns);
    let name = cut(&name, width.saturating_sub(right_width));
    let gap = width.saturating_sub(columns(&name) + right_width);
    let mut spans = vec![
        Span::styled(name, theme::title()),
        Span::from(" ".repeat(gap)),
    ];
    spans.extend(right.map(|text| Span::styled(text, theme::secondary())));
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
/// that `z`, as `key` names it, shows it.
pub fn context_hint(entry: Option<&ContextEntry>, key: &str, width: u16) -> Line<'static> {
    let Some(entry) = entry else {
        return Line::default();
    };
    let text = match kind_and_name(entry) {
        (kind, Some(name)) => format!("▸ {kind}: {name} · {key}: full screen"),
        (kind, None) => format!("▸ {kind} · {key}: full screen"),
    };
    Line::styled(cut(&text, usize::from(width)), theme::secondary())
}

#[cfg(test)]
mod tests {
    use crate::graphics::Images;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Style, Stylize};

    use super::super::test_support::{find, row};
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
        let hint =
            |entry: Option<&ContextEntry>, width: u16| text(&context_hint(entry, "z", width));

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
    fn the_hint_names_the_key_as_given() {
        let image = entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#);

        assert_eq!(
            text(&context_hint(Some(&image), "esc, z", 78)),
            "▸ image: cat.png · esc, z: full screen"
        );
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
        let header = |json: &str| {
            text(&header_line(
                &entry(json),
                fits,
                Offset::default(),
                30,
                true,
            ))
        };

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
    fn the_header_shows_the_name_bold_and_the_position_dim() {
        let page = Page {
            rows: Extent {
                shown: 10,
                total: 31,
            },
            columns: Extent::default(),
        };
        let log = entry(r#"{"type": "code", "title": "Log", "content": "x"}"#);
        let line = header_line(&log, page, Offset::default(), 30, false);

        assert_eq!(line.spans[0], Span::from("Log").bold());
        assert_eq!(
            line.spans.last(),
            Some(&Span::styled(" 1–10 of 31", Style::new().dim()))
        );
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

        let line = text(&header_line(
            &titled,
            page,
            Offset { top: 3, left: 0 },
            30,
            true,
        ));

        assert_eq!(
            line, "A very long title… 4–15 of 120",
            "the tab hint goes before the name is cut"
        );
    }

    #[test]
    fn the_unfocused_header_offers_tab_where_the_entry_scrolls() {
        let page = |rows: usize, columns: usize| Page {
            rows: Extent {
                shown: 10,
                total: rows,
            },
            columns: Extent {
                shown: 20,
                total: columns,
            },
        };
        let log = entry(r#"{"type": "code", "title": "Log", "content": "x"}"#);
        let header = |page: Page, offer_tab: bool| {
            text(&header_line(&log, page, Offset::default(), 30, offer_tab))
        };

        assert_eq!(header(page(31, 20), true), "Log           1–10 of 31 · tab");
        assert_eq!(
            header(page(10, 50), true),
            "Log                        tab",
            "wider"
        );
        assert_eq!(
            header(page(31, 20), false),
            "Log                 1–10 of 31",
            "focused"
        );
        assert_eq!(
            header(page(10, 20), true).trim_end(),
            "Log",
            "nothing to scroll"
        );
    }

    #[test]
    fn the_header_fills_its_columns_with_a_name_of_wide_characters() {
        let fits = Page {
            rows: Extent {
                shown: 10,
                total: 3,
            },
            columns: Extent::default(),
        };
        let short = entry(r#"{"type": "code", "title": "日本語の🎉", "content": "x"}"#);
        let line = text(&header_line(&short, fits, Offset::default(), 30, true));
        assert_eq!(columns(&line), 30, "fills the line: {line:?}");

        let page = Page {
            rows: Extent {
                shown: 12,
                total: 120,
            },
            columns: Extent::default(),
        };
        let long =
            entry(r#"{"type": "code", "title": "日本語のとても長いタイトルです", "content": "x"}"#);
        // The position takes 12 columns, which leaves the name 18: 16 for
        // 日本語のとても長, as い would cross into the `…`, then the `…`
        // and one blank.
        let line = text(&header_line(
            &long,
            page,
            Offset { top: 3, left: 0 },
            30,
            true,
        ));
        assert_eq!(line, "日本語のとても長…  4–15 of 120");
        assert_eq!(columns(&line), 30);
    }

    #[test]
    fn a_pane_scrolled_past_the_end_of_a_taller_body_draws_its_last_page() {
        let content = (1..=30)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\\n");
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "text",
                "context": [{{"type": "code", "title": "Log", "content": "{content}"}}]}}]}}"#
        );
        let mut state =
            SessionState::new("batch", serde_json::from_str(&json).expect("parses"), None);
        // A terminal 5 lines tall let the person scroll down to line 26.
        state.set_pane_page(Some(Page {
            rows: Extent {
                shown: 5,
                total: 30,
            },
            columns: Extent::default(),
        }));
        for code in [KeyCode::Tab, KeyCode::End] {
            state.handle(KeyEvent::new(code, KeyModifiers::NONE));
        }
        assert_eq!(state.pane_offset().top, 25);

        let area = Rect::new(0, 0, 40, 11);
        let mut buf = Buffer::empty(area);
        let mut viewers = Viewers::new(Images::half_blocks());
        draw_pane(area, &mut buf, &state, &mut viewers);

        assert!(row(&buf, 0).ends_with("21–30 of 30"), "{:?}", row(&buf, 0));
        assert_eq!(find(&buf, "line 21").1, 1);
    }
}
