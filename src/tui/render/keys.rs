// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The keys (spec sections 7.1 and 7.2): the key bar at the bottom of the
//! screen, with the hints that work where the focus is, and the help
//! overlay that lists every key.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::overlay::{overlay_block, popup};
use super::source_line::columns;
use super::theme;
use crate::format::Kind;
use crate::tui::{Extent, Focus, Page, Row, SessionState};

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
                Span::styled(format!("{keys:<key_column$}"), theme::key()),
                Span::from(*what),
            ])
        })
        .collect();
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(overlay_block(title, Some("any key closes")))
        .render(area, buf);
}

/// A hint in the key bar: a key, what it does where the focus is, and how
/// long it stays in a bar too narrow for every hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Hint {
    pub(super) key: &'static str,
    pub(super) what: &'static str,
    pub(super) rank: Rank,
}

/// How long a hint stays in a key bar too narrow for all of them; a later
/// rank stays longer. The help stays longest, as it names every other
/// key, then the ways out, as quitting or going back is what a lost
/// person looks for. The keys that act where the cursor is come next,
/// then `←/→`, then the context pane's `tab`, and a note last, which is
/// optional on every question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Rank {
    Note,
    Context,
    /// Switching questions, or scrolling the context pane sideways.
    LeftRight,
    Cursor,
    /// Quitting, going back from the context pane or the full-screen view,
    /// closing an overlay.
    Leave,
    Help,
}

const fn hint(key: &'static str, what: &'static str, rank: Rank) -> Hint {
    Hint { key, what, rank }
}

/// The hints that more than one set shares.
const NOTE_HINT: Hint = hint("n", "note", Rank::Note);
const CONTEXT_HINT: Hint = hint("tab", "context", Rank::Context);
const QUESTION_HINT: Hint = hint("←/→", "question", Rank::LeftRight);
const HELP_HINT: Hint = hint("?", "help", Rank::Help);
const QUIT_HINT: Hint = hint("q", "quit", Rank::Leave);

/// The keys worth showing right now, without `n` where no note is taken.
/// `pane` is what the context pane showed in this draw: where one shows,
/// `tab` leads into it, and while it has the focus its own keys show.
pub(super) fn key_hints(state: &SessionState, list: bool, pane: Option<Page>) -> Vec<Hint> {
    if let Some(page) = pane.filter(|_| state.pane_focused() && !list && !state.help()) {
        return pane_key_hints(state, page);
    }
    let mut hints = all_key_hints(state, list);
    if state.question().is_some_and(|question| !question.note) {
        hints.retain(|hint| hint.rank != Rank::Note);
    }
    if pane.is_some() && !list && !state.help() {
        // Before the help, or last in a field, whose keys have no help.
        let at = hints
            .iter()
            .position(|hint| hint.rank == Rank::Help)
            .unwrap_or(hints.len());
        hints.insert(at, CONTEXT_HINT);
    }
    hints
}

/// The keys of the intro overlay. Scrolling shows only where the intro is
/// longer than the overlay, as `page` says.
pub(super) fn intro_hints(page: Extent) -> Vec<Hint> {
    let close = hint("esc/q/i", "close", Rank::Leave);
    if page.total > page.shown {
        vec![
            hint("↑/↓", "scroll", Rank::Cursor),
            hint("pgup/pgdn", "page", Rank::Cursor),
            close,
        ]
    } else {
        vec![close]
    }
}

/// The keys over an empty queue.
/// The hint while the dialog of an invalid session shows, which any key
/// closes.
pub(super) fn dialog_hints() -> Vec<Hint> {
    vec![hint("any key", "close", Rank::Leave)]
}

pub(super) fn empty_queue_hints() -> Vec<Hint> {
    vec![QUIT_HINT]
}

/// The keys of the focused pane and the full-screen view, where `z` is
/// one of the keys that go back. Sideways and `o` only show where they do
/// something.
fn pane_key_hints(state: &SessionState, page: Page) -> Vec<Hint> {
    let mut hints = vec![hint("↑/↓", "scroll", Rank::Cursor)];
    if page.columns.total > page.columns.shown {
        hints.push(hint("←/→", "sideways", Rank::LeftRight));
    }
    hints.push(hint("pgup/pgdn", "page", Rank::Cursor));
    if !state.full_screen() {
        hints.push(hint("z", "full screen", Rank::Cursor));
    }
    if state.shown_image().is_some() {
        hints.push(hint("o", "open", Rank::Cursor));
    }
    hints.push(HELP_HINT);
    hints.push(if state.full_screen() {
        hint("z/esc/q", "back", Rank::Leave)
    } else {
        hint("tab/esc/q", "back", Rank::Leave)
    });
    hints
}

fn all_key_hints(state: &SessionState, list: bool) -> Vec<Hint> {
    let cursor = |key, what| hint(key, what, Rank::Cursor);
    if list {
        return vec![
            cursor("↑/↓", "move"),
            cursor("enter", "open"),
            hint("esc", "close", Rank::Leave),
        ];
    }
    if state.help() {
        return vec![hint("any key", "close", Rank::Leave)];
    }
    let row = state.rows().get(state.row()).copied();
    let kind = state.question().map(|question| question.kind);
    let typing = |enter| {
        vec![
            cursor("ctrl-j", "new line"),
            cursor("enter", enter),
            cursor("esc", "stop typing"),
        ]
    };
    match (state.focus(), row) {
        (Focus::Note, _) => vec![cursor("ctrl-j", "new line"), cursor("enter/esc", "done")],
        (Focus::Field, Some(Row::Answer)) => typing("next"),
        (Focus::Field, Some(Row::Note)) => typing("submit"),
        (Focus::Field, Some(Row::Reject)) => typing("reject"),
        (Focus::Field, _) => typing("pick"),
        (Focus::None, _) if state.on_review() => vec![
            cursor("↑/↓", "move"),
            cursor("enter", "open/submit"),
            QUESTION_HINT,
            HELP_HINT,
            QUIT_HINT,
        ],
        (Focus::None, _) if kind == Some(Kind::Text) => vec![
            cursor("↑/↓", "type"),
            cursor("enter", "next"),
            QUESTION_HINT,
            NOTE_HINT,
            HELP_HINT,
            QUIT_HINT,
        ],
        (Focus::None, _) if kind == Some(Kind::Multi) => vec![
            cursor("↑/↓", "move"),
            cursor("space", "toggle"),
            cursor("enter", "next"),
            QUESTION_HINT,
            NOTE_HINT,
            HELP_HINT,
            QUIT_HINT,
        ],
        (Focus::None, _) => vec![
            cursor("↑/↓", "move"),
            cursor("enter", "pick"),
            QUESTION_HINT,
            NOTE_HINT,
            HELP_HINT,
            QUIT_HINT,
        ],
    }
}

/// The key bar in `width` columns. Hints that do not fit whole are left
/// out rather than cut, the lowest [`Rank`] first, and the rest keep their
/// order; the help lists every key anyway.
pub(super) fn key_line(hints: &[Hint], width: u16) -> Line<'static> {
    // Each hint is ` key ` and ` what  `; the blanks after the last one may
    // run past the edge.
    let hint_width = |hint: &Hint| columns(hint.key) + columns(hint.what) + 5;
    let mut kept: Vec<&Hint> = hints.iter().collect();
    loop {
        let used: usize = kept.iter().map(|hint| hint_width(hint)).sum();
        if used.saturating_sub(2) <= usize::from(width) {
            break;
        }
        // Among hints of the same rank, the one further right goes first.
        let least = (0..kept.len())
            .rev()
            .min_by_key(|&index| kept[index].rank)
            .expect("no hints at all always fit");
        kept.remove(least);
    }

    let mut spans = Vec::new();
    for hint in kept {
        spans.push(Span::styled(format!(" {} ", hint.key), theme::key()));
        spans.push(Span::styled(
            format!(" {}  ", hint.what),
            theme::secondary(),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{find, line_texts, look_at, row};
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
        state.set_pane_page(Some(crate::tui::PanePage::shown(page(20))));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let keys_of = |hints: Vec<Hint>| hints.iter().map(|hint| hint.key).collect::<Vec<_>>();

        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(20)))),
            ["↑/↓", "pgup/pgdn", "z", "o", "?", "tab/esc/q"]
        );
        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(50)))),
            ["↑/↓", "←/→", "pgup/pgdn", "z", "o", "?", "tab/esc/q"]
        );
        let bar = |width: u16| {
            let hints = key_hints(&state, false, Some(page(50)));
            line_texts(&[key_line(&hints, width)])[0]
                .trim_end()
                .to_owned()
        };
        assert_eq!(
            bar(90),
            " ↑/↓  scroll   pgup/pgdn  page   z  full screen   o  open   ?  help   tab/esc/q  back",
            "sideways goes first"
        );
        assert_eq!(
            bar(60),
            " ↑/↓  scroll   pgup/pgdn  page   ?  help   tab/esc/q  back",
            "then the keys further right, the help and the way back last"
        );
        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!(
            pairs(&key_hints(&state, false, Some(page(20)))),
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
        state.set_pane_page(Some(crate::tui::PanePage::shown(page(20))));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(20)))),
            ["↑/↓", "pgup/pgdn", "z", "?", "tab/esc/q"],
            "o opens images only"
        );
    }

    /// The keys and descriptions of `hints`, in order.
    fn pairs(hints: &[Hint]) -> Vec<(&'static str, &'static str)> {
        hints.iter().map(|hint| (hint.key, hint.what)).collect()
    }

    #[test]
    fn the_note_shows_where_one_is_taken_and_tab_where_a_pane_shows() {
        let session = |note: bool, kind: &str| {
            let json = format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "{kind}",
                    "note": {note}, "options": [{{"id": "a", "label": "A"}}]}}]}}"#
            );
            SessionState::new("hints", serde_json::from_str(&json).expect("parses"), None)
        };
        let page = Page::default();

        assert_eq!(
            pairs(&key_hints(&session(true, "single"), false, None)),
            [
                ("↑/↓", "move"),
                ("enter", "pick"),
                ("←/→", "question"),
                ("n", "note"),
                ("?", "help"),
                ("q", "quit")
            ]
        );
        assert_eq!(
            pairs(&key_hints(&session(false, "single"), false, Some(page))),
            [
                ("↑/↓", "move"),
                ("enter", "pick"),
                ("←/→", "question"),
                ("tab", "context"),
                ("?", "help"),
                ("q", "quit")
            ],
            "no note, and tab before the help"
        );
        let mut typing = session(true, "text");
        typing.handle(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            pairs(&key_hints(&typing, false, Some(page))),
            [
                ("ctrl-j", "new line"),
                ("enter", "next"),
                ("esc", "stop typing"),
                ("tab", "context")
            ],
            "a field's keys have no help, so tab goes last"
        );
        assert_eq!(
            pairs(&key_hints(&session(true, "single"), true, Some(page))),
            [("↑/↓", "move"), ("enter", "open"), ("esc", "close")],
            "the session list names no pane"
        );
    }

    #[test]
    fn the_key_bar_leaves_out_what_does_not_fit_whole() {
        let text = |width: u16| -> String {
            let hints = [
                hint("↑/↓", "move", Rank::Cursor),
                hint("enter", "pick", Rank::Cursor),
                QUESTION_HINT,
                NOTE_HINT,
                CONTEXT_HINT,
                HELP_HINT,
                QUIT_HINT,
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
            let hints = [hint("日本", "表示", Rank::Cursor), QUIT_HINT];
            line_texts(&[key_line(&hints, width)])[0]
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

        assert_eq!(row(&buf, 0), format!("  ╭ Keys {}╮", "─".repeat(62)));
        assert_eq!(
            row(&buf, 1),
            format!("  │ ↑/↓, k/j   move between the rows{}│", " ".repeat(35))
        );
        assert_eq!(
            row(&buf, 14),
            format!("  ╰{} any key closes ╯", "─".repeat(52))
        );
        assert_eq!(buf[find(&buf, "Keys")].modifier, Modifier::BOLD);
        assert_eq!(buf[find(&buf, "any key")].modifier, Modifier::DIM);
        assert_eq!(look_at(&buf, "↑/↓, k/j"), (Color::Cyan, Modifier::BOLD));
        assert_eq!(
            look_at(&buf, "move between"),
            (Color::Reset, Modifier::empty())
        );
    }

    #[test]
    fn the_key_bar_shows_each_key_bold_in_the_primary_colour_and_no_fill() {
        let line = key_line(&[hint("↑/↓", "move", Rank::Cursor), QUIT_HINT], 40);
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        line.render(area, &mut buf);

        assert_eq!(row(&buf, 0), " ↑/↓  move   q  quit");
        let key = (Color::Cyan, Modifier::BOLD);
        assert_eq!(look_at(&buf, " ↑/↓ "), key, "the blank before the key");
        assert_eq!(look_at(&buf, "↑/↓"), key);
        assert_eq!(look_at(&buf, "q "), key);
        let what = (Color::Reset, Modifier::DIM);
        assert_eq!(look_at(&buf, "move"), what);
        assert_eq!(look_at(&buf, "quit"), what);
        assert!(
            buf.content()
                .iter()
                .all(|cell| !cell.modifier.contains(Modifier::REVERSED)),
            "no fill"
        );
    }
}
