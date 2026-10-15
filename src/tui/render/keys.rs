// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The keys (spec sections 7.1 and 7.2): the key bar at the bottom of the
//! screen, with the hints that work where the focus is, and the help
//! overlay that lists every key.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::overlay::{overlay_block, popup};
use super::source_line::columns;
use crate::format::Kind;
use crate::tui::{Focus, Page, Row, SessionState};

/// Every key, in the 13 lines the help has on the smallest screen.
const HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "move between the rows"),
    ("←/→, h/l", "previous and next question"),
    ("", "in a field, arrows leave it at its edges"),
    ("enter", "pick and go on; open or submit on the review"),
    (
        "space, 1-9",
        "toggle an option; pick or toggle one directly",
    ),
    ("n, ctrl-j", "write a note; a new line in any field"),
    ("esc", "stop typing in a field"),
    ("tab", "move into the context pane and back"),
    ("pgup/pgdn", "scroll the context pane, also ctrl-u/ctrl-d"),
    ("o, z", "open the image; show the context full screen"),
    ("L", "the list of waiting sessions"),
    ("q, ctrl-c", "quit, keeping the draft"),
    ("?, i", "this help; the whole intro"),
];

/// The keys of the focused context pane.
const PANE_HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "scroll by line"),
    ("←/→, h/l", "scroll sideways where lines are cut"),
    ("pgup/pgdn", "scroll by page, also ctrl-u/ctrl-d"),
    ("g/G", "to the top and the bottom, also home/end"),
    ("z", "show the context full screen and back"),
    ("o", "open an image in the system viewer"),
    ("tab/esc/q", "back to the question"),
    ("ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

/// The keys of the full-screen view. It goes back to where it was entered
/// from, the pane or the question, so its way out names no place.
const FULL_SCREEN_HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "scroll by line"),
    ("←/→, h/l", "scroll sideways where lines are cut"),
    ("pgup/pgdn", "scroll by page, also ctrl-u/ctrl-d"),
    ("g/G", "to the top and the bottom, also home/end"),
    ("o", "open an image in the system viewer"),
    ("z/tab/esc/q", "leave the full screen"),
    ("ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

/// The keys that work where the focus is: the full-screen view's, the
/// pane's while it has the focus, every key otherwise.
pub(super) fn draw_help(area: Rect, buf: &mut Buffer, state: &SessionState) {
    let (keys, title) = if state.full_screen() {
        (FULL_SCREEN_HELP, "Keys of the full-screen view")
    } else if state.pane_focused() {
        (PANE_HELP, "Keys of the context pane")
    } else {
        (HELP, "Keys")
    };
    let height = u16::try_from(keys.len() + 2).expect("a short list");
    // Narrower than 70 columns, the help takes the whole width.
    let area = popup(area, 70, height, buf);
    // `format!` pads by characters, and every key is one column per
    // character, so the longest key plus one blank sets the column.
    let key_column = keys
        .iter()
        .map(|(keys, _)| keys.chars().count() + 1)
        .max()
        .unwrap_or_default()
        .max(11);
    let lines: Vec<Line> = keys
        .iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::from(format!("{keys:<key_column$}")).bold(),
                Span::from(*what),
            ])
        })
        .collect();
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(overlay_block(title, Some(Line::from("any key closes"))))
        .render(area, buf);
}

/// The keys worth showing right now, without `n` where no note is taken.
/// `pane` is what the context pane showed in this draw: where one shows,
/// `tab` leads into it, and while it has the focus its own keys show.
pub(super) fn key_hints(
    state: &SessionState,
    list: bool,
    pane: Option<Page>,
) -> Vec<(&'static str, &'static str)> {
    if let Some(page) = pane.filter(|_| state.pane_focused() && !list && !state.help()) {
        return pane_key_hints(state, page);
    }
    let mut hints = all_key_hints(state, list);
    if state.question().is_some_and(|question| !question.note) {
        hints.retain(|(key, _)| *key != "n");
    }
    if pane.is_some() && !list && !state.help() {
        // Before the help, or last in a field, whose keys have no help.
        let at = hints
            .iter()
            .position(|(key, _)| *key == "?")
            .unwrap_or(hints.len());
        hints.insert(at, ("tab", "context"));
    }
    hints
}

/// The keys of the focused pane and the full-screen view, where `z` is
/// one of the keys that go back. Sideways and `o` only show where they do
/// something.
fn pane_key_hints(state: &SessionState, page: Page) -> Vec<(&'static str, &'static str)> {
    let mut hints = vec![("↑/↓", "scroll")];
    if page.columns.total > page.columns.shown {
        hints.push(("←/→", "sideways"));
    }
    hints.push(("pgup/pgdn", "page"));
    if !state.full_screen() {
        hints.push(("z", "full screen"));
    }
    if state.shown_image().is_some() {
        hints.push(("o", "open"));
    }
    hints.push(("?", "help"));
    hints.push(if state.full_screen() {
        ("z/esc/q", "back")
    } else {
        ("tab/esc/q", "back")
    });
    hints
}

fn all_key_hints(state: &SessionState, list: bool) -> Vec<(&'static str, &'static str)> {
    if list {
        return vec![("↑/↓", "move"), ("enter", "open"), ("esc", "close")];
    }
    if state.help() {
        return vec![("any key", "close")];
    }
    let row = state.rows().get(state.row()).copied();
    let kind = state.question().map(|question| question.kind);
    match (state.focus(), row) {
        (Focus::Note, _) => vec![("ctrl-j", "new line"), ("enter/esc", "done")],
        (Focus::Field, Some(Row::Answer)) => vec![
            ("ctrl-j", "new line"),
            ("enter", "next"),
            ("esc", "stop typing"),
        ],
        (Focus::Field, Some(Row::Note)) => vec![
            ("ctrl-j", "new line"),
            ("enter", "submit"),
            ("esc", "stop typing"),
        ],
        (Focus::Field, Some(Row::Reject)) => vec![
            ("ctrl-j", "new line"),
            ("enter", "reject"),
            ("esc", "stop typing"),
        ],
        (Focus::Field, _) => vec![
            ("ctrl-j", "new line"),
            ("enter", "pick"),
            ("esc", "stop typing"),
        ],
        (Focus::None, _) if state.on_review() => vec![
            ("↑/↓", "move"),
            ("enter", "open/submit"),
            ("←/→", "question"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (Focus::None, _) if kind == Some(Kind::Text) => vec![
            ("↑/↓", "type"),
            ("enter", "next"),
            ("←/→", "question"),
            ("n", "note"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (Focus::None, _) if kind == Some(Kind::Multi) => vec![
            ("↑/↓", "move"),
            ("space", "toggle"),
            ("enter", "next"),
            ("←/→", "question"),
            ("n", "note"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (Focus::None, _) => vec![
            ("↑/↓", "move"),
            ("enter", "pick"),
            ("←/→", "question"),
            ("n", "note"),
            ("?", "help"),
            ("q", "quit"),
        ],
    }
}

/// The key bar in `width` columns. Hints that do not fit whole are left
/// out rather than cut, the least important first (`hint_rank`), and the
/// rest keep their order; the help lists every key anyway.
pub(super) fn key_line(keys: &[(&str, &str)], width: u16) -> Line<'static> {
    // Each hint is ` key ` and ` what  `; the blanks after the last one may
    // run past the edge.
    let hint_width = |(key, what): &(&str, &str)| columns(key) + columns(what) + 5;
    let mut kept: Vec<&(&str, &str)> = keys.iter().collect();
    loop {
        let used: usize = kept.iter().map(|hint| hint_width(hint)).sum();
        if used.saturating_sub(2) <= usize::from(width) {
            break;
        }
        // Among hints of the same rank, the one further right goes first.
        let least = (0..kept.len())
            .rev()
            .min_by_key(|&index| hint_rank(kept[index].0))
            .expect("no hints at all always fit");
        kept.remove(least);
    }

    let mut spans = Vec::new();
    for (key, what) in kept {
        spans.push(Span::from(format!(" {key} ")).add_modifier(Modifier::REVERSED));
        spans.push(Span::from(format!(" {what}  ")));
    }
    Line::from(spans)
}

/// How long a hint stays in a key bar too narrow for all of them; higher
/// stays longer. `?` and `q` stay longest, as the help names every other
/// key and quitting, or going back from the context pane, is what a lost
/// person looks for. The keys that act where the cursor is come next, then
/// switching questions, then the context pane's `tab`, and a note last,
/// which is optional on every question.
fn hint_rank(key: &str) -> u8 {
    match key {
        "?" => 6,
        "q" | "tab/esc/q" | "z/esc/q" => 5,
        "←/→" => 3,
        "tab" => 2,
        "n" => 1,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::super::test_support::{find, line_texts, row};
    use super::*;
    use crate::tui::Extent;

    #[test]
    fn the_focused_pane_names_sideways_and_open_only_where_they_work() {
        let mut state = SessionState::new(
            "pictured",
            serde_json::from_str(
                r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
                    "context": [{"type": "image", "path": "/pictures/cat.png"}], "options": [{"id": "a", "label": "A"}]}]}"#,
            )
            .expect("parses"),
            None,
        );
        let page = |columns: usize| Page {
            rows: Extent {
                shown: 10,
                total: 10,
            },
            columns: Extent {
                shown: 20,
                total: columns,
            },
        };
        state.set_pane_page(Some(page(20)));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let keys_of = |hints: Vec<(&str, &str)>| {
            hints
                .iter()
                .map(|(key, _)| key.to_string())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(20)))),
            ["↑/↓", "pgup/pgdn", "z", "o", "?", "tab/esc/q"]
        );
        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(50)))),
            ["↑/↓", "←/→", "pgup/pgdn", "z", "o", "?", "tab/esc/q"]
        );
        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!(
            key_hints(&state, false, Some(page(20))),
            [
                ("↑/↓", "scroll"),
                ("pgup/pgdn", "page"),
                ("o", "open"),
                ("?", "help"),
                ("z/esc/q", "back")
            ],
            "the full-screen view"
        );

        let text = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "context": [{"type": "code", "content": "x"}], "options": [{"id": "a", "label": "A"}]}]}"#;
        let mut state =
            SessionState::new("text", serde_json::from_str(text).expect("parses"), None);
        state.set_pane_page(Some(page(20)));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(20)))),
            ["↑/↓", "pgup/pgdn", "z", "?", "tab/esc/q"],
            "o opens images only"
        );
    }

    #[test]
    fn the_key_bar_leaves_out_what_does_not_fit_whole() {
        let text = |width: u16| -> String {
            let hints = [
                ("↑/↓", "move"),
                ("enter", "pick"),
                ("←/→", "question"),
                ("n", "note"),
                ("tab", "context"),
                ("?", "help"),
                ("q", "quit"),
            ];
            let line = key_line(&hints, width);
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_owned()
        };

        assert_eq!(
            text(100),
            " ↑/↓  move   enter  pick   ←/→  question   n  note   tab  context   ?  help   q  quit"
        );
        assert_eq!(
            text(85),
            " ↑/↓  move   enter  pick   ←/→  question   n  note   tab  context   ?  help   q  quit",
            "the blanks after the last hint may run past the edge"
        );
        assert_eq!(
            text(80),
            " ↑/↓  move   enter  pick   ←/→  question   tab  context   ?  help   q  quit",
            "the note goes first"
        );
        assert_eq!(
            text(60),
            " ↑/↓  move   enter  pick   ←/→  question   ?  help   q  quit",
            "then the pane's tab"
        );
        assert_eq!(text(40), " ↑/↓  move   ?  help   q  quit");
        assert_eq!(text(20), " ?  help   q  quit");
        assert_eq!(text(5), "");
    }

    #[test]
    fn the_key_bar_measures_wide_hints_in_columns() {
        let text = |width: u16| {
            line_texts(&[key_line(&[("日本", "表示"), ("q", "quit")], width)])[0]
                .trim_end()
                .to_owned()
        };

        // ` 日本 ` and ` 表示  ` take 13 columns, ` q ` and ` quit  ` 10,
        // of which the last two blanks may run past the edge.
        assert_eq!(text(21), " 日本  表示   q  quit");
        assert_eq!(text(20), " q  quit");
    }

    #[test]
    fn the_help_frame_names_the_keys_and_how_to_close_it() {
        let state = SessionState::new(
            "batch",
            serde_json::from_str(
                r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
            )
            .expect("parses"),
            None,
        );
        let area = Rect::new(0, 0, 74, 15);
        let mut buf = Buffer::empty(area);
        draw_help(area, &mut buf, &state);

        assert_eq!(row(&buf, 0), format!("  ┌ Keys {}┐", "─".repeat(62)));
        assert_eq!(
            row(&buf, 1),
            format!("  │↑/↓, k/j   move between the rows{}│", " ".repeat(36))
        );
        assert_eq!(
            row(&buf, 14),
            format!("  └{} any key closes ┘", "─".repeat(52))
        );
        assert_eq!(buf[find(&buf, "Keys")].modifier, Modifier::empty());
        assert_eq!(buf[find(&buf, "any key")].modifier, Modifier::empty());
        assert_eq!(buf[find(&buf, "↑/↓, k/j")].modifier, Modifier::BOLD);
        assert_eq!(buf[find(&buf, "move between")].modifier, Modifier::empty());
    }
}
