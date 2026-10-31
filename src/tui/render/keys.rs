// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The keys (spec sections 7.2 and 7.6): the key bar at the bottom of the
//! screen, with the hints that work where the focus is, and the help
//! overlay that lists every key.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::overlay::{overlay_block, popup};
use super::screen::MIN_WIDTH;
use super::source_line::columns;
use super::theme;
use crate::format::{ContextEntry, Kind};
use crate::tui::{DiffLayout, DrawnDiff, Extent, Focus, Page, Row, SessionState};

/// Every key, in the 13 lines the help has on the smallest screen.
const HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "move between the rows"),
    ("←/→, h/l", "prev/next question; in a field, at its edges"),
    ("enter", "pick and go on; open or submit on the review"),
    (
        "space, 1-9",
        "toggle an option; pick or toggle one directly",
    ),
    ("n, ctrl-j", "write a note; a new line in any field"),
    ("esc, ctrl-g", "stop typing; edit the field in your editor"),
    ("tab", "move into the context pane and back"),
    ("pgup/pgdn, ctrl-u/ctrl-d", "scroll by page; by half a page"),
    ("o, z, s", "open an entry; full screen; diff side by side"),
    (
        "L, w, backspace",
        "the sessions; the queues; clear the pick",
    ),
    (",", "the settings"),
    ("q, ctrl-c", "quit, keeping the draft"),
    ("?, i", "this help; the whole intro"),
];

/// The keys of the focused context pane.
const REPLAY_HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "move between the rows"),
    ("←/→, h/l", "previous/next question"),
    ("enter", "next question; on the result, open a question"),
    ("tab", "move into the context pane and back"),
    ("pgup/pgdn, ctrl-u/ctrl-d", "scroll by page; by half a page"),
    ("o, z, s", "open an entry; full screen; diff side by side"),
    ("d", "details of a kept file"),
    ("q, ctrl-c", "quit"),
    ("?, i", "this help; the whole intro"),
];

const PANE_HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "scroll by line"),
    ("←/→, h/l", "scroll sideways where lines are cut"),
    ("pgup/pgdn", "scroll by page"),
    ("ctrl-u/ctrl-d", "scroll by half a page"),
    ("g/G", "to the top and the bottom, also home/end"),
    ("z", "show the context full screen and back"),
    ("s", "a diff side by side or unified"),
    ("o", "open in the app for its file type"),
    ("tab/esc/q", "back to the question"),
    ("ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

/// The keys of the full-screen view. It goes back to where it was entered
/// from, the pane or the question, so its way out names no place.
const FULL_SCREEN_HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "scroll by line"),
    ("←/→, h/l", "scroll sideways where lines are cut"),
    ("pgup/pgdn", "scroll by page"),
    ("ctrl-u/ctrl-d", "scroll by half a page"),
    ("g/G", "to the top and the bottom, also home/end"),
    ("o", "open in the app for its file type"),
    ("s", "a diff side by side or unified"),
    ("z/tab/esc/q", "leave the full screen"),
    ("ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

/// The keys of the full-screen view reached from the one-line hint while
/// it shows an image with its caption, where `z` shows the image alone
/// first (ADR 69).
const CAPTION_FULL_SCREEN_HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "scroll by line"),
    ("←/→, h/l", "scroll sideways where lines are cut"),
    ("pgup/pgdn", "scroll by page"),
    ("ctrl-u/ctrl-d", "scroll by half a page"),
    ("g/G", "to the top and the bottom, also home/end"),
    ("o", "open in the app for its file type"),
    ("z", "the image alone"),
    ("tab/esc/q", "leave the full screen"),
    ("ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

/// The keys that work where the focus is: the full-screen view's, the
/// pane's while it has the focus, every key otherwise, with `version`,
/// asqr's name and version, at the left of the frame's bottom edge.
pub(super) fn draw_help(area: Rect, buf: &mut Buffer, state: &SessionState, version: &str) {
    let (keys, title) = if state.full_screen() && !state.full_screen_alone() {
        (CAPTION_FULL_SCREEN_HELP, "Keys of the full-screen view")
    } else if state.full_screen() {
        (FULL_SCREEN_HELP, "Keys of the full-screen view")
    } else if state.pane_focused() {
        (PANE_HELP, "Keys of the context pane")
    } else if state.is_read_only() {
        (REPLAY_HELP, "Keys of a replay")
    } else {
        (HELP, "Keys")
    };
    let height = u16::try_from(keys.len() + 2).expect("a short list");
    // Narrower than 70 columns, the help takes the whole width.
    let area = popup(area, 70, height, buf);
    // `format!` pads by characters, and every key is one column per
    // character, so the longest key plus one blank sets the column. The
    // frame's height is fixed before the lines wrap, so a wrapped line
    // would push the last one out of it: the column is never wider than
    // leaves the longest description its line on the smallest screen. A
    // key longer than the column is followed by one blank and pushes its
    // description along.
    let inner = overlay_block(title, None)
        .inner(Rect::new(0, 0, MIN_WIDTH, height))
        .width;
    let longest_what = keys
        .iter()
        .map(|(_, what)| columns(what))
        .max()
        .unwrap_or_default();
    let key_column = keys
        .iter()
        .map(|(keys, _)| keys.chars().count() + 1)
        .max()
        .unwrap_or_default()
        .max(11)
        .min(usize::from(inner).saturating_sub(longest_what));
    let pad = key_column.saturating_sub(1);
    let lines: Vec<Line> = keys
        .iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::styled(format!("{keys:<pad$} "), theme::key()),
                Span::from(*what),
            ])
        })
        .collect();
    let frame = overlay_block(title, Some("any key closes"))
        .title_bottom(Line::styled(format!(" {version} "), theme::secondary()).left_aligned());
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(frame)
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
/// then `←/→`, then the context pane's `tab`, then a note, which the
/// person may or may not write, and `ctrl-u`/`ctrl-d` last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Rank {
    /// `ctrl-u`/`ctrl-d`, beside the page keys and after `tab`.
    HalfPage,
    /// `n`, and `backspace` right of it while it can clear a pick.
    Note,
    Context,
    /// Switching questions, or scrolling the context pane sideways.
    LeftRight,
    Cursor,
    /// Quitting, going back from the context pane or the full-screen view,
    /// closing an overlay, and `z  image`, the step before going back from
    /// an image shown with its caption.
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
const HALF_PAGE_HINT: Hint = hint("ctrl-u/ctrl-d", "half page", Rank::HalfPage);

/// The keys worth showing right now. `pane` is what the context pane
/// showed in this draw: where one shows, `tab` leads into it and
/// `ctrl-u`/`ctrl-d` scroll it, and while it has the focus its own keys
/// show.
pub(super) fn key_hints(state: &SessionState, list: bool, pane: Option<Page>) -> Vec<Hint> {
    if let Some(page) = pane.filter(|_| state.pane_focused() && !list && !state.help()) {
        return pane_key_hints(state, page);
    }
    let mut hints = all_key_hints(state, list);
    if pane.is_some() && !list && !state.help() {
        // `tab` and the half-page keys: before the help, or last in a
        // field, whose keys have no help.
        let at = hints
            .iter()
            .position(|hint| hint.rank == Rank::Help)
            .unwrap_or(hints.len());
        hints.splice(at..at, [CONTEXT_HINT, HALF_PAGE_HINT]);
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
            HALF_PAGE_HINT,
            close,
        ]
    } else {
        vec![close]
    }
}

/// The hint while the dialog of an invalid session shows, which any key
/// closes.
pub(super) fn dialog_hints() -> Vec<Hint> {
    vec![hint("any key", "close", Rank::Leave)]
}

/// The keys over an empty queue.
pub(super) fn empty_queue_hints() -> Vec<Hint> {
    vec![
        hint("w", "queues", Rank::Cursor),
        hint(",", "settings", Rank::Cursor),
        QUIT_HINT,
    ]
}

/// The keys of the settings pane. `←`/`→` show only where the row under
/// the cursor can change, which a flag or a variable holding it stops.
pub(super) fn settings_hints(locked: bool) -> Vec<Hint> {
    let mut hints = vec![hint("↑/↓", "move", Rank::Cursor)];
    if !locked {
        hints.push(hint("←/→", "change", Rank::Cursor));
    }
    hints.push(hint("esc", "close", Rank::Leave));
    hints
}

/// The keys of the first-start screen: `enter` saves where the cursor
/// stands on the button.
pub(super) fn first_start_hints(on_button: bool) -> Vec<Hint> {
    vec![
        hint("↑/↓", "move", Rank::Cursor),
        hint(
            "enter",
            if on_button { "save" } else { "pick" },
            Rank::Cursor,
        ),
        QUIT_HINT,
    ]
}

/// The keys of the start picker, whose new-queue row is a field while
/// it is `typing`.
pub(super) fn picker_hints(typing: bool) -> Vec<Hint> {
    if typing {
        switcher_hints(true)
    } else {
        vec![
            hint("↑/↓", "move", Rank::Cursor),
            hint("enter", "watch", Rank::Cursor),
            QUIT_HINT,
        ]
    }
}

/// The keys of the queue switcher, whose new-queue row is a field while
/// it is `typing`.
pub(super) fn switcher_hints(typing: bool) -> Vec<Hint> {
    if typing {
        vec![
            hint("enter", "create", Rank::Cursor),
            hint("esc", "stop typing", Rank::Leave),
        ]
    } else {
        vec![
            hint("↑/↓", "move", Rank::Cursor),
            hint("enter", "switch", Rank::Cursor),
            hint("esc/q", "close", Rank::Leave),
        ]
    }
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
    hints.push(HALF_PAGE_HINT);
    if !state.full_screen() {
        hints.push(hint("z", "full screen", Rank::Cursor));
    }
    if state.shown_context().is_some_and(ContextEntry::opens) {
        hints.push(hint("o", "open", Rank::Cursor));
    }
    hints.push(HELP_HINT);
    if state.full_screen() && !state.full_screen_alone() {
        // `z` shows the image alone before it goes back. The view comes
        // from the one-line hint, so from a narrow terminal, and the bar
        // must still say what the next `z` does (ADR 69): the hint ranks
        // with the way back.
        hints.push(hint("z", "image", Rank::Leave));
        hints.push(hint("esc/q", "back", Rank::Leave));
    } else if state.full_screen() {
        hints.push(hint("z/esc/q", "back", Rank::Leave));
    } else {
        hints.push(hint("tab/esc/q", "back", Rank::Leave));
    }
    hints
}

/// `hints` with `s` added where the focused pane or the full-screen view
/// shows a diff, as `diff` says the draw showed it.
pub(super) fn with_diff_hint(
    hints: Vec<Hint>,
    state: &SessionState,
    diff: Option<DrawnDiff>,
) -> Vec<Hint> {
    // Beside a card or `loading…` there is nothing to switch, and a body
    // too narrow for two columns refuses side by side. The bar follows
    // the layout the draw used, not the switch: a diff switched side by
    // side in a body too narrow for it shows unified, and `s` there only
    // turns the switch off, so no hint promises a change. Over the help
    // any key only closes it.
    let drawn = diff.map(|diff| diff.layout);
    let switches =
        diff.is_some_and(|diff| diff.fits_side_by_side) || drawn == Some(DiffLayout::SideBySide);
    if !(state.pane_focused() && switches) || state.help() {
        return hints;
    }
    let what = if drawn == Some(DiffLayout::SideBySide) {
        "unified"
    } else {
        "side by side"
    };
    let mut hints = hints;
    let at = hints
        .iter()
        .position(|hint| hint.rank == Rank::Help)
        .unwrap_or(hints.len());
    hints.insert(at, hint("s", what, Rank::Context));
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
    if state.is_read_only() {
        let mut hints = vec![cursor("↑/↓", "move"), QUESTION_HINT];
        if state.shown_file().is_some() {
            hints.push(cursor("d", "details"));
        }
        hints.extend([HELP_HINT, QUIT_HINT]);
        return hints;
    }
    let row = state.rows().get(state.row()).copied();
    let kind = state.question().map(|question| question.kind);
    // `ctrl-g` stands last of a field's keys, so a bar too narrow for all
    // of them leaves it out first (ADR 74).
    let typing = |enter| {
        vec![
            cursor("ctrl-j", "new line"),
            cursor("enter", enter),
            cursor("esc", "stop typing"),
            cursor("ctrl-g", "editor"),
        ]
    };
    match (state.focus(), row) {
        (Focus::Note, _) => vec![
            cursor("ctrl-j", "new line"),
            cursor("enter/esc", "done"),
            cursor("ctrl-g", "editor"),
        ],
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
        // An info item's one row leaves nothing to move between.
        (Focus::None, _) if kind == Some(Kind::Info) => vec![
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
        (Focus::None, _) => {
            // `backspace` clears a pick only on the picked option's row. It
            // ranks with the note and stands right of it, so a narrow bar
            // leaves it out first.
            let clear = state
                .on_pick()
                .then_some(hint("backspace", "clear", Rank::Note));
            [
                cursor("↑/↓", "move"),
                cursor("enter", "pick"),
                QUESTION_HINT,
                NOTE_HINT,
            ]
            .into_iter()
            .chain(clear)
            .chain([HELP_HINT, QUIT_HINT])
            .collect()
        }
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

/// The key bar of the empty queue: [`key_line`] in `width` columns, and
/// `version`, asqr's name and version, dim at its right end, one blank
/// from the edge. The version takes only the room the hints leave, with
/// the two blanks after the last hint between them; where it does not fit
/// there, the bar shows the hints alone.
pub(super) fn key_line_with_version(hints: &[Hint], width: u16, version: &str) -> Line<'static> {
    let mut line = key_line(hints, width);
    // The hints' line counts the blanks after the last hint, which may run
    // past the edge, so the room is measured by adding up, never by taking
    // the hints away from `width`.
    let used = line.width() + columns(version) + 1;
    if used > usize::from(width) {
        return line;
    }
    line.spans
        .push(Span::raw(" ".repeat(usize::from(width) - used)));
    line.spans
        .push(Span::styled(format!("{version} "), theme::secondary()));
    line
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{find, line_texts, look_at, row, rows};
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
            [
                "↑/↓",
                "pgup/pgdn",
                "ctrl-u/ctrl-d",
                "z",
                "o",
                "?",
                "tab/esc/q"
            ]
        );
        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(50)))),
            [
                "↑/↓",
                "←/→",
                "pgup/pgdn",
                "ctrl-u/ctrl-d",
                "z",
                "o",
                "?",
                "tab/esc/q"
            ]
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
            "the half-page keys go first, then sideways"
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
                ("ctrl-u/ctrl-d", "half page"),
                ("o", "open"),
                ("?", "help"),
                ("z/esc/q", "back")
            ],
            "the full-screen view"
        );

        let focused_on = |entry: &str| {
            let text = format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "single",
                    "context": [{entry}], "options": [{{"id": "a", "label": "A"}}]}}]}}"#
            );
            let mut state =
                SessionState::new("text", serde_json::from_str(&text).expect("parses"), None);
            state.set_pane_page(Some(crate::tui::PanePage::shown(page(20))));
            state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            keys_of(key_hints(&state, false, Some(page(20))))
        };
        assert_eq!(
            focused_on(r#"{"type": "code", "content": "x"}"#),
            [
                "↑/↓",
                "pgup/pgdn",
                "ctrl-u/ctrl-d",
                "z",
                "o",
                "?",
                "tab/esc/q"
            ],
            "o opens code"
        );
        assert_eq!(
            focused_on(r#"{"type": "ansi", "content": "x"}"#),
            ["↑/↓", "pgup/pgdn", "ctrl-u/ctrl-d", "z", "?", "tab/esc/q"],
            "o does not open ANSI text"
        );
    }

    /// The keys and descriptions of `hints`, in order.
    fn pairs(hints: &[Hint]) -> Vec<(&'static str, &'static str)> {
        hints.iter().map(|hint| (hint.key, hint.what)).collect()
    }

    // -----------------------------------------------------------------
    // ctrl-u and ctrl-d in the key bar
    // -----------------------------------------------------------------

    /// A `kind` question that takes a note, whose code entry of `lines`
    /// lines shows in a pane 10 rows high, and that pane's page.
    fn beside_code(kind: &str, lines: usize) -> (SessionState, Page) {
        let content: String = (1..=lines).map(|line| format!("line {line}\\n")).collect();
        let options = if kind == "text" {
            ""
        } else {
            r#", "options": [{"id": "a", "label": "A"}]"#
        };
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "{kind}",
                "context": [{{"type": "code", "content": "{content}"}}]{options}}}]}}"#
        );
        let mut state =
            SessionState::new("beside", serde_json::from_str(&json).expect("parses"), None);
        let page = Page {
            rows: Extent {
                shown: 10,
                total: lines,
            },
            columns: Extent {
                shown: 20,
                total: 20,
            },
        };
        state.set_pane_page(Some(crate::tui::PanePage::shown(page)));
        (state, page)
    }

    /// The key bar of `state` beside a pane showing `page`, in `width`
    /// columns.
    fn bar_of(state: &SessionState, page: Page, width: u16) -> String {
        let hints = key_hints(state, false, Some(page));
        line_texts(&[key_line(&hints, width)])[0]
            .trim_end()
            .to_owned()
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// A question whose context is a diff, its pane focused.
    fn focused_diff() -> SessionState {
        let text = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "context": [{"type": "diff", "old": {"content": "a\n"}, "new": {"content": "b\n"}}],
            "options": [{"id": "a", "label": "A"}]}]}"#;
        let mut state =
            SessionState::new("diff", serde_json::from_str(text).expect("parses"), None);
        state.set_pane_page(Some(crate::tui::PanePage {
            diff: unified(true),
            ..crate::tui::PanePage::shown(Page::default())
        }));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        state
    }

    /// A diff drawn unified, with room for two columns where `fits`.
    fn unified(fits: bool) -> Option<DrawnDiff> {
        Some(DrawnDiff {
            layout: crate::tui::DiffLayout::Unified,
            fits_side_by_side: fits,
            translated: None,
        })
    }

    /// A diff drawn side by side, which only a body with room for two
    /// columns draws.
    fn side_by_side() -> Option<DrawnDiff> {
        Some(DrawnDiff {
            layout: crate::tui::DiffLayout::SideBySide,
            fits_side_by_side: true,
            translated: None,
        })
    }

    /// The key bar's hints for `state`, whose pane drew `diff`.
    fn diff_hints(
        state: &SessionState,
        diff: Option<DrawnDiff>,
    ) -> Vec<(&'static str, &'static str)> {
        pairs(&with_diff_hint(
            key_hints(state, false, Some(Page::default())),
            state,
            diff,
        ))
    }

    #[test]
    fn the_focused_pane_names_s_before_the_help_on_a_diff() {
        let mut state = focused_diff();

        assert_eq!(
            diff_hints(&state, unified(true)),
            [
                ("↑/↓", "scroll"),
                ("pgup/pgdn", "page"),
                ("ctrl-u/ctrl-d", "half page"),
                ("z", "full screen"),
                ("o", "open"),
                ("s", "side by side"),
                ("?", "help"),
                ("tab/esc/q", "back")
            ]
        );
        state.handle(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(
            diff_hints(&state, side_by_side()).contains(&("s", "unified")),
            "{:?}",
            diff_hints(&state, side_by_side())
        );
    }

    #[test]
    fn the_key_bar_names_no_s_while_side_by_side_is_too_narrow() {
        let mut state = focused_diff();
        state.handle(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(state.side_by_side());

        // The body became too narrow: the draw fell back to unified.
        let hints = diff_hints(&state, unified(false));

        assert!(!hints.iter().any(|(key, _)| *key == "s"), "{hints:?}");
    }

    #[test]
    fn the_key_bar_names_s_only_where_it_switches() {
        let state = focused_diff();

        let has_s = |diff| diff_hints(&state, diff).iter().any(|(key, _)| *key == "s");
        assert!(has_s(unified(true)), "a diff with room for two columns");
        assert!(!has_s(None), "a card or loading");
        assert!(!has_s(unified(false)), "two columns do not fit");
        let mut help = focused_diff();
        help.handle(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(
            diff_hints(&help, unified(true)),
            [("any key", "close")],
            "over the help"
        );
        let mut question = focused_diff();
        question.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(
            !diff_hints(&question, unified(true))
                .iter()
                .any(|(key, _)| *key == "s"),
            "the question's rows"
        );
    }

    #[test]
    fn the_pane_and_full_screen_helps_list_s_after_z_and_o() {
        let mut state = focused_diff();
        let pane = smallest_help_lines(&state);
        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        let full_screen = smallest_help_lines(&state);

        let after = |lines: &[String], key: &str| {
            let at = lines
                .iter()
                .position(|line| line.starts_with(key))
                .expect("the line of the key");
            lines[at + 1].clone()
        };
        let s = "s             a diff side by side or unified";
        assert_eq!(after(&pane, "z "), s);
        assert_eq!(after(&full_screen, "o "), s);
    }

    #[test]
    fn the_general_help_names_s_with_o_and_z() {
        let lines = smallest_help_lines(&general_help_state());

        assert_eq!(
            lines.iter().find(|line| line.starts_with("o, z")),
            Some(&"o, z, s    open an entry; full screen; diff side by side".to_owned())
        );
    }

    #[test]
    fn the_focused_pane_names_ctrl_u_and_ctrl_d_beside_the_page_keys() {
        let (mut state, page) = beside_code("single", 1);
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(
            pairs(&key_hints(&state, false, Some(page))),
            [
                ("↑/↓", "scroll"),
                ("pgup/pgdn", "page"),
                ("ctrl-u/ctrl-d", "half page"),
                ("z", "full screen"),
                ("o", "open"),
                ("?", "help"),
                ("tab/esc/q", "back")
            ]
        );
    }

    #[test]
    fn the_full_screen_view_names_ctrl_u_and_ctrl_d_beside_the_page_keys() {
        let (mut state, page) = beside_code("single", 1);
        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert!(state.full_screen());

        assert_eq!(
            pairs(&key_hints(&state, false, Some(page))),
            [
                ("↑/↓", "scroll"),
                ("pgup/pgdn", "page"),
                ("ctrl-u/ctrl-d", "half page"),
                ("o", "open"),
                ("?", "help"),
                ("z/esc/q", "back")
            ]
        );
    }

    /// A question whose image has a caption, shown full screen from the
    /// one-line hint, where image and caption show first.
    fn caption_full_screen_from_the_hint() -> (SessionState, Page) {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "context": [{"type": "image", "path": "/pictures/p.png", "caption": "Draft 3"}],
            "options": [{"id": "a", "label": "A"}]}]}"#;
        let mut state =
            SessionState::new("caption", serde_json::from_str(json).expect("parses"), None);
        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        let page = Page::default();
        state.set_pane_page(Some(crate::tui::PanePage::shown(page)));
        (state, page)
    }

    #[test]
    fn the_full_screen_view_from_the_hint_says_that_z_shows_the_image() {
        let (mut state, page) = caption_full_screen_from_the_hint();

        assert_eq!(
            pairs(&key_hints(&state, false, Some(page))),
            [
                ("↑/↓", "scroll"),
                ("pgup/pgdn", "page"),
                ("ctrl-u/ctrl-d", "half page"),
                ("o", "open"),
                ("?", "help"),
                ("z", "image"),
                ("esc/q", "back")
            ]
        );

        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!(
            pairs(&key_hints(&state, false, Some(page))).last(),
            Some(&("z/esc/q", "back")),
            "the image alone goes back on z"
        );
    }

    #[test]
    fn a_narrow_key_bar_from_the_hint_keeps_z_image_and_the_way_back() {
        let (state, page) = caption_full_screen_from_the_hint();

        // The view occurs below 101 columns, where the one-line hint shows.
        let bar = bar_of(&state, page, 60);

        assert!(bar.contains(" z  image "), "{bar}");
        assert!(bar.contains(" esc/q  back"), "{bar}");
    }

    #[test]
    fn the_full_screen_help_from_the_hint_says_what_z_does_first() {
        let (state, _) = caption_full_screen_from_the_hint();

        let lines = smallest_help_lines(&state);

        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("z ") && line.contains("the image alone")),
            "{lines:?}"
        );
    }

    #[test]
    fn an_unfocused_pane_names_ctrl_u_and_ctrl_d_after_tab() {
        let (state, page) = beside_code("single", 1);

        assert_eq!(
            pairs(&key_hints(&state, false, Some(page))),
            [
                ("↑/↓", "move"),
                ("enter", "pick"),
                ("←/→", "question"),
                ("n", "note"),
                ("tab", "context"),
                ("ctrl-u/ctrl-d", "half page"),
                ("?", "help"),
                ("q", "quit")
            ]
        );
    }

    #[test]
    fn a_field_beside_a_pane_names_ctrl_u_and_ctrl_d_last() {
        let (state, page) = beside_code("text", 1);
        assert_eq!(state.focus(), Focus::Field);

        assert_eq!(
            pairs(&key_hints(&state, false, Some(page))),
            [
                ("ctrl-j", "new line"),
                ("enter", "next"),
                ("esc", "stop typing"),
                ("ctrl-g", "editor"),
                ("tab", "context"),
                ("ctrl-u/ctrl-d", "half page")
            ]
        );
    }

    #[test]
    fn the_half_page_hint_leaves_a_narrow_question_bar_before_the_note() {
        let (state, page) = beside_code("single", 1);

        assert_eq!(
            bar_of(&state, page, 100),
            " ↑/↓  move   enter  pick   ←/→  question   n  note   tab  context   ?  help   q  quit"
        );
    }

    #[test]
    fn the_half_page_hint_leaves_a_narrow_pane_bar_before_full_screen() {
        let (mut state, page) = beside_code("single", 1);
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(
            bar_of(&state, page, 100),
            " ↑/↓  scroll   pgup/pgdn  page   z  full screen   o  open   ?  help   tab/esc/q  back"
        );
    }

    #[test]
    fn ctrl_u_and_ctrl_d_show_in_a_wide_key_bar() {
        let (state, page) = beside_code("single", 1);

        assert_eq!(
            bar_of(&state, page, 120),
            " ↑/↓  move   enter  pick   ←/→  question   n  note   tab  context   ctrl-u/ctrl-d  half page   ?  help   q  quit"
        );
    }

    /// An intro of 20 lines in an overlay that shows 5 of them.
    const SCROLLING_INTRO: Extent = Extent {
        shown: 5,
        total: 20,
    };

    #[test]
    fn the_intro_overlay_names_ctrl_u_and_ctrl_d_beside_the_page_keys() {
        let hints = intro_hints(SCROLLING_INTRO);

        assert_eq!(
            pairs(&hints),
            [
                ("↑/↓", "scroll"),
                ("pgup/pgdn", "page"),
                ("ctrl-u/ctrl-d", "half page"),
                ("esc/q/i", "close")
            ]
        );
        assert_eq!(hints[2], HALF_PAGE_HINT, "the same hint, rank included");
    }

    #[test]
    fn the_intro_overlay_shows_ctrl_u_and_ctrl_d_in_80_columns() {
        let hints = intro_hints(SCROLLING_INTRO);

        assert_eq!(
            line_texts(&[key_line(&hints, 80)])[0].trim_end(),
            " ↑/↓  scroll   pgup/pgdn  page   ctrl-u/ctrl-d  half page   esc/q/i  close"
        );
    }

    #[test]
    fn the_unfocused_bar_names_ctrl_u_and_ctrl_d_where_they_scroll_the_pane() {
        let half_page = ("ctrl-u/ctrl-d", "half page");
        // A text question's field has the focus from the start.
        let in_field = || beside_code("text", 40);
        let on_question = || {
            let (mut state, page) = in_field();
            state.handle(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            (state, page)
        };
        let in_note = || {
            let (mut state, page) = on_question();
            state.handle(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
            (state, page)
        };

        for (beside, focus) in [
            (on_question(), Focus::None),
            (in_field(), Focus::Field),
            (in_note(), Focus::Note),
        ] {
            let (mut state, page) = beside;
            assert_eq!(state.focus(), focus);
            assert!(
                pairs(&key_hints(&state, false, Some(page))).contains(&half_page),
                "{focus:?}"
            );
            state.handle(ctrl('d'));
            assert_eq!(state.pane_offset().top, 5, "{focus:?}");
        }

        // Without a pane the field keeps `ctrl-d`, and the bar does not
        // name it.
        let (mut state, _) = in_field();
        state.set_pane_page(None);
        for c in ['a', 'b'] {
            state.handle(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        state.handle(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(!pairs(&key_hints(&state, false, None)).contains(&half_page));
        assert_eq!(state.handle(ctrl('d')), crate::tui::Effect::DraftChanged);
    }

    #[test]
    fn the_note_shows_on_every_question_and_tab_where_a_pane_shows() {
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
                ("n", "note"),
                ("tab", "context"),
                ("ctrl-u/ctrl-d", "half page"),
                ("?", "help"),
                ("q", "quit")
            ],
            "a note though the file sets note: false, and tab before the help"
        );
        let mut typing = session(true, "text");
        typing.handle(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            pairs(&key_hints(&typing, false, Some(page))),
            [
                ("ctrl-j", "new line"),
                ("enter", "next"),
                ("esc", "stop typing"),
                ("ctrl-g", "editor"),
                ("tab", "context"),
                ("ctrl-u/ctrl-d", "half page")
            ],
            "a field's keys have no help, so tab and the half-page keys go last"
        );
        assert_eq!(
            pairs(&key_hints(&session(true, "single"), true, Some(page))),
            [("↑/↓", "move"), ("enter", "open"), ("esc", "close")],
            "the session list names no pane"
        );
    }

    /// A `single` question with an own answer whose option `a` is picked
    /// as its default.
    fn picked_single() -> SessionState {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "custom": true, "options": [{"id": "a", "label": "A", "default": true}, {"id": "b", "label": "B"}]}]}"#;
        SessionState::new("picked", serde_json::from_str(json).expect("parses"), None)
    }

    #[test]
    fn a_single_with_a_pick_names_backspace_after_the_note() {
        let mut state = picked_single();

        assert_eq!(
            pairs(&key_hints(&state, false, None)),
            [
                ("↑/↓", "move"),
                ("enter", "pick"),
                ("←/→", "question"),
                ("n", "note"),
                ("backspace", "clear"),
                ("?", "help"),
                ("q", "quit")
            ]
        );

        let names_backspace = |state: &SessionState| {
            pairs(&key_hints(state, false, None)).contains(&("backspace", "clear"))
        };
        state.handle(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(!names_backspace(&state), "nothing is picked");
        let mut own = picked_single();
        for key in [KeyCode::Down, KeyCode::Down, KeyCode::Esc] {
            own.handle(KeyEvent::new(key, KeyModifiers::NONE));
        }
        assert!(!names_backspace(&own), "the own-answer row");
        let multi = SessionState::new(
            "multi",
            serde_json::from_str(
                r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "multi",
                    "options": [{"id": "a", "label": "A", "default": true}]}]}"#,
            )
            .expect("parses"),
            None,
        );
        assert!(!names_backspace(&multi), "a multi");
    }

    #[test]
    fn a_single_names_backspace_only_on_the_picked_options_row() {
        let names_backspace = |state: &SessionState| {
            pairs(&key_hints(state, false, None)).contains(&("backspace", "clear"))
        };
        let press = |state: &mut SessionState, code: KeyCode| {
            state.handle(KeyEvent::new(code, KeyModifiers::NONE));
        };
        let mut state = picked_single();
        assert!(names_backspace(&state), "the picked option");

        press(&mut state, KeyCode::Down);
        assert!(!names_backspace(&state), "another option");

        // Clear the default, type an own answer, and go back to an option:
        // the own answer is picked, but not on an option row.
        press(&mut state, KeyCode::Up);
        press(&mut state, KeyCode::Backspace);
        for code in [
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::Char('x'),
            KeyCode::Esc,
            KeyCode::Up,
        ] {
            press(&mut state, code);
        }
        assert!(!names_backspace(&state), "an own answer picked");
    }

    #[test]
    fn the_clear_hint_leaves_a_narrow_bar_first() {
        let state = picked_single();
        let bar = |width: u16| {
            let hints = key_hints(&state, false, None);
            line_texts(&[key_line(&hints, width)])[0]
                .trim_end()
                .to_owned()
        };

        assert_eq!(
            bar(89),
            " ↑/↓  move   enter  pick   ←/→  question   n  note   backspace  clear   ?  help   q  quit"
        );
        assert_eq!(
            bar(88),
            " ↑/↓  move   enter  pick   ←/→  question   n  note   ?  help   q  quit"
        );
    }

    #[test]
    fn the_general_help_names_w_and_backspace_on_the_line_of_l() {
        let lines = smallest_help_lines(&general_help_state());

        assert_eq!(
            lines.iter().find(|line| line.starts_with("L")),
            Some(&"L, w, backspace the sessions; the queues; clear the pick".to_owned())
        );
    }

    #[test]
    fn a_field_names_ctrl_g_in_its_key_bar() {
        let state = SessionState::new(
            "field",
            serde_json::from_str(
                r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
            )
            .expect("parses"),
            None,
        );
        assert_eq!(state.focus(), Focus::Field);

        assert_eq!(
            pairs(&key_hints(&state, false, None)),
            [
                ("ctrl-j", "new line"),
                ("enter", "next"),
                ("esc", "stop typing"),
                ("ctrl-g", "editor")
            ]
        );
    }

    #[test]
    fn the_note_names_ctrl_g_in_its_key_bar() {
        let mut state = general_help_state();
        state.handle(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        state.handle(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(state.focus(), Focus::Note);

        assert_eq!(
            pairs(&key_hints(&state, false, None)),
            [
                ("ctrl-j", "new line"),
                ("enter/esc", "done"),
                ("ctrl-g", "editor")
            ]
        );
    }

    #[test]
    fn a_narrow_field_bar_leaves_ctrl_g_out_first_of_its_own_keys() {
        let (state, page) = beside_code("text", 1);

        assert_eq!(
            bar_of(&state, page, 80),
            " ctrl-j  new line   enter  next   esc  stop typing   ctrl-g  editor"
        );
        assert_eq!(
            line_texts(&[key_line(&key_hints(&state, false, None), 60)])[0].trim_end(),
            " ctrl-j  new line   enter  next   esc  stop typing"
        );
    }

    #[test]
    fn the_general_help_names_ctrl_g_on_the_line_of_esc() {
        let lines = smallest_help_lines(&general_help_state());

        assert_eq!(
            lines.iter().find(|line| line.starts_with("esc")),
            Some(&"esc, ctrl-g stop typing; edit the field in your editor".to_owned())
        );
    }

    #[test]
    fn an_info_item_names_enter_next_and_no_moving() {
        // Its one row leaves nothing to move between.
        let json = r#"{"asqr": 2, "questions": [{"id": "i", "text": "Done.", "kind": "info"}]}"#;
        let state = SessionState::new("hints", serde_json::from_str(json).expect("parses"), None);

        assert_eq!(
            pairs(&key_hints(&state, false, None)),
            [
                ("enter", "next"),
                ("←/→", "question"),
                ("n", "note"),
                ("?", "help"),
                ("q", "quit")
            ]
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

    /// The bar of `↑/↓  move` and `q  quit`, 22 columns with the blanks
    /// after `quit`, with the version `asqr 0.0.0` where `width` has room.
    fn bar_with_version(width: u16) -> Line<'static> {
        let hints = [hint("↑/↓", "move", Rank::Cursor), QUIT_HINT];
        key_line_with_version(&hints, width, "asqr 0.0.0")
    }

    /// The same bar without the version.
    fn bar_alone(width: u16) -> Line<'static> {
        key_line(&[hint("↑/↓", "move", Rank::Cursor), QUIT_HINT], width)
    }

    #[test]
    fn the_version_stands_one_blank_from_the_right_edge_where_there_is_room() {
        // 22 columns of hints, the version's 10 and one blank.
        let text = line_texts(&[bar_with_version(33)])[0].clone();

        assert_eq!(text, " ↑/↓  move   q  quit  asqr 0.0.0 ");
    }

    #[test]
    fn the_version_leaves_out_where_the_hints_leave_no_room_for_it() {
        assert_eq!(bar_with_version(32), bar_alone(32));
    }

    #[test]
    fn the_version_leaves_out_where_the_hints_run_past_the_edge() {
        // The blanks after `quit` run 2 columns past an edge at 20.
        assert_eq!(bar_with_version(20), bar_alone(20));
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
        draw_help(area, &mut buf, &state, "asqr 0.0.0");

        assert_eq!(row(&buf, 0), format!("  ╭ Keys {}╮", "─".repeat(62)));
        assert_eq!(
            row(&buf, 1),
            format!("  │ ↑/↓, k/j   move between the rows{}│", " ".repeat(35))
        );
        assert_eq!(
            row(&buf, 14),
            format!("  ╰ asqr 0.0.0 {} any key closes ╯", "─".repeat(40))
        );
        assert_eq!(buf[find(&buf, "Keys")].modifier, Modifier::BOLD);
        assert_eq!(buf[find(&buf, "any key")].modifier, Modifier::DIM);
        assert_eq!(look_at(&buf, "↑/↓, k/j"), (Color::Cyan, Modifier::BOLD));
        assert_eq!(
            look_at(&buf, "move between"),
            (Color::Reset, Modifier::empty())
        );
    }

    /// The lines inside the help's frame on the smallest screen, 60x15,
    /// without the border and the blanks around them.
    fn smallest_help_lines(state: &SessionState) -> Vec<String> {
        let area = Rect::new(0, 0, 60, 15);
        let mut buf = Buffer::empty(area);
        draw_help(area, &mut buf, state, "asqr 0.0.0");
        let rows = rows(&buf);
        let edge = |corner| {
            rows.iter()
                .position(|row| row.contains(corner))
                .expect("the help is drawn in a frame")
        };
        rows[edge('╭') + 1..edge('╰')]
            .iter()
            .map(|row| row.trim().trim_matches('│').trim().to_owned())
            .collect()
    }

    /// The bottom edge of the help's frame on the smallest screen, 60x15,
    /// drawn with the version `asqr 0.0.0`, and the buffer it is in.
    fn help_bottom(state: &SessionState) -> (String, Buffer) {
        let area = Rect::new(0, 0, 60, 15);
        let mut buf = Buffer::empty(area);
        draw_help(area, &mut buf, state, "asqr 0.0.0");
        let bottom = rows(&buf)
            .into_iter()
            .find(|row| row.contains('╰'))
            .expect("the help is drawn in a frame");
        (bottom, buf)
    }

    /// What the bottom edge of every help reads on 60 columns.
    fn bottom_with_version() -> String {
        format!("╰ asqr 0.0.0 {} any key closes ╯", "─".repeat(30))
    }

    #[test]
    fn the_general_help_names_the_version_at_the_left_of_its_bottom_edge() {
        let (bottom, buf) = help_bottom(&general_help_state());

        assert_eq!(bottom.trim_end(), bottom_with_version());
        // Dim on the border, in the border's colour, as the footer opposite.
        assert_eq!(look_at(&buf, "asqr 0.0.0"), (Color::Cyan, Modifier::DIM));
        assert_eq!(look_at(&buf, "asqr 0.0.0"), look_at(&buf, "any key closes"));
    }

    #[test]
    fn the_pane_help_names_the_version_at_the_left_of_its_bottom_edge() {
        let (mut state, _) = beside_code("single", 1);
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(state.pane_focused());

        assert_eq!(help_bottom(&state).0.trim_end(), bottom_with_version());
    }

    #[test]
    fn the_full_screen_help_names_the_version_at_the_left_of_its_bottom_edge() {
        let (mut state, _) = beside_code("single", 1);
        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert!(state.full_screen());

        assert_eq!(help_bottom(&state).0.trim_end(), bottom_with_version());
    }

    /// A state whose `?` shows the general help.
    fn general_help_state() -> SessionState {
        SessionState::new(
            "batch",
            serde_json::from_str(
                r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
            )
            .expect("parses"),
            None,
        )
    }

    #[test]
    fn the_general_help_names_both_scroll_distances_on_one_line() {
        let lines = smallest_help_lines(&general_help_state());

        assert_eq!(
            lines.iter().find(|line| line.starts_with("pgup/pgdn")),
            Some(&"pgup/pgdn, ctrl-u/ctrl-d scroll by page; by half a page".to_owned())
        );
    }

    #[test]
    fn the_general_help_names_the_settings_key_on_a_line_of_its_own() {
        let lines = smallest_help_lines(&general_help_state());

        assert_eq!(HELP.len(), 13, "the help keeps its 13 lines");
        assert!(
            lines
                .iter()
                .any(|line| line.split_whitespace().collect::<Vec<_>>() == [",", "the", "settings"]),
            "{lines:#?}"
        );
    }

    #[test]
    fn the_arrows_line_of_the_general_help_says_where_a_field_lets_them_go() {
        let lines = smallest_help_lines(&general_help_state());

        assert_eq!(
            lines.iter().find(|line| line.starts_with("←/→, h/l")),
            Some(&"←/→, h/l   prev/next question; in a field, at its edges".to_owned())
        );
        assert!(
            lines.iter().all(|line| !line.starts_with("in a field")),
            "{lines:#?}"
        );
    }

    #[test]
    fn the_settings_bar_names_the_arrows_only_where_the_row_can_change() {
        let bar = |locked| {
            line_texts(&[key_line(&settings_hints(locked), 80)])[0]
                .trim_end()
                .to_owned()
        };

        assert_eq!(bar(false), " ↑/↓  move   ←/→  change   esc  close");
        assert_eq!(bar(true), " ↑/↓  move   esc  close");
    }

    #[test]
    fn the_empty_queue_bar_names_the_settings_key() {
        let bar = line_texts(&[key_line(&empty_queue_hints(), 80)])[0]
            .trim_end()
            .to_owned();

        assert_eq!(bar, " w  queues   ,  settings   q  quit");
    }

    #[test]
    fn the_general_help_keeps_its_13_lines_whole_on_the_smallest_screen() {
        let lines = smallest_help_lines(&general_help_state());

        // Every line in a key column of 11, and a key longer than that
        // followed by one blank.
        let whole: Vec<String> = HELP
            .iter()
            .map(|(key, what)| format!("{key:<10} {what}").trim().to_owned())
            .collect();
        assert_eq!(lines, whole);
    }

    #[test]
    fn the_pane_helps_list_ctrl_u_and_ctrl_d_apart_and_fit_the_smallest_screen() {
        let text = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "context": [{"type": "code", "content": "x"}], "options": [{"id": "a", "label": "A"}]}]}"#;
        let mut state =
            SessionState::new("text", serde_json::from_str(text).expect("parses"), None);
        state.set_pane_page(Some(crate::tui::PanePage::shown(Page::default())));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let pane = smallest_help_lines(&state);
        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert!(state.full_screen());
        let full_screen = smallest_help_lines(&state);

        let scroll_lines = |lines: &[String]| {
            let starting = |key: &str| lines.iter().find(|line| line.starts_with(key)).cloned();
            (starting("pgup/pgdn"), starting("ctrl-u/ctrl-d"))
        };
        let expected = (
            Some("pgup/pgdn     scroll by page".to_owned()),
            Some("ctrl-u/ctrl-d scroll by half a page".to_owned()),
        );
        assert_eq!(
            (scroll_lines(&pane), scroll_lines(&full_screen)),
            (expected.clone(), expected)
        );

        // The frame's height is fixed before the lines wrap, so a line that
        // wraps pushes the last one out of it.
        for lines in [&pane, &full_screen] {
            let whole = |key: &str, end: &str| {
                lines
                    .iter()
                    .any(|line| line.starts_with(key) && line.ends_with(end))
            };
            assert!(
                lines
                    .last()
                    .is_some_and(|last| last.starts_with('?') && last.ends_with("this help"))
                    && whole("g/G", "home/end"),
                "{lines:#?}"
            );
        }
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
