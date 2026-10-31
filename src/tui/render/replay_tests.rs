// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A finished session drawn again (ADR 101): the screen says in words and
//! marks that it is the past, and nothing on it looks writable.

use std::collections::HashMap;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};

use super::screen::{View, draw};
use super::test_support::{find, look_at, row, rows, style_at};
use super::{PaneLayout, theme};
use crate::format::{Answer, Session, SessionResult};
use crate::graphics::Images;
use crate::queue::{HistoryOutcome, ReplayFile};
use crate::tui::{App, ReplayInfo, SessionState};

const SESSION: &str = r#"{"asqr": 2, "title": "README hero image, round 2",
    "from": "claude-code, asqr-site",
    "intro": "Pick the hero for the README. B is the one with the light card.",
    "questions": [
    {"id": "hero", "header": "Hero", "text": "Which hero image should the README use?", "kind": "single",
     "options": [
       {"id": "a", "label": "A Dark terminal", "description": "The queue switcher in Ghostty."},
       {"id": "b", "label": "B  Light card", "description": "One session on a cream card."},
       {"id": "c", "label": "C Split", "description": "Terminal left, card right."}]},
    {"id": "badges", "header": "Badges", "text": "Which badges?", "kind": "multi",
     "options": [{"id": "ci", "label": "CI"}, {"id": "lic", "label": "Licence"}]}
]}"#;

fn info(outcome: HistoryOutcome) -> ReplayInfo {
    ReplayInfo {
        reference: "7C3FA19K".to_owned(),
        outcome,
        finished: "Thu 29 Oct 16:41".to_owned(),
        files: HashMap::<String, ReplayFile>::new(),
    }
}

fn replay_app(outcome: HistoryOutcome) -> App {
    let session: Session = serde_json::from_str(SESSION).expect("parses");
    let answers = vec![
        Answer {
            selected: vec!["b".to_owned()],
            note: Some("B, but crop the left margin.".to_owned()),
            ..Answer::new("hero")
        },
        Answer {
            selected: vec!["ci".to_owned()],
            ..Answer::new("badges")
        },
    ];
    let mut draft = SessionResult::draft("hero-2", "hero", answers);
    draft.note = Some("thanks".to_owned());
    draft.reason = Some("not this round".to_owned());
    let mut app = App::default();
    app.add(SessionState::replay("hero-2", session, Some(&draft)).with_replay_info(info(outcome)));
    app
}

fn screen_in(app: &App, view: &mut View, width: u16, height: u16) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| _ = draw(frame, app, view))
        .expect("draws");
    terminal.backend().clone()
}

fn screen(app: &App, width: u16, height: u16) -> TestBackend {
    screen_in(
        app,
        &mut View::for_tests("default", PaneLayout::Auto, Images::half_blocks()),
        width,
        height,
    )
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

// =========================================================
// The title bar
// =========================================================

#[test]
fn the_title_bar_says_read_only_right_after_asqr_and_names_the_replay() {
    let backend = screen(&replay_app(HistoryOutcome::Sent), 120, 30);

    let title = row(backend.buffer(), 0);

    assert!(
        title.starts_with(" asqr ◷ read-only  README hero image, round 2"),
        "{title}"
    );
    assert!(title.ends_with("default · replay 7C3FA19K"), "{title}");
}

#[test]
fn the_title_mark_has_the_warning_colour_and_no_background() {
    let backend = screen(&replay_app(HistoryOutcome::Sent), 120, 30);

    let cell = &backend.buffer()[find(backend.buffer(), "◷ read-only")];

    assert_eq!(cell.fg, theme::warning().fg.expect("a colour"));
    assert!(!cell.modifier.contains(Modifier::REVERSED), "no fill");
    assert_eq!(cell.bg, Color::Reset);
}

#[test]
fn no_color_keeps_the_mark_as_text() {
    let mut view =
        View::for_tests("default", PaneLayout::Auto, Images::half_blocks()).no_color(true);

    let backend = screen_in(&replay_app(HistoryOutcome::Sent), &mut view, 120, 30);

    let (fg, _) = look_at(backend.buffer(), "◷ read-only");
    assert_eq!(fg, Color::Reset);
    assert!(row(backend.buffer(), 0).contains("◷ read-only"));
}

#[test]
fn the_mark_stays_in_full_screen_and_under_the_help() {
    let mut app = replay_app(HistoryOutcome::Sent);
    // The question shows no context, so the help is what covers the screen.
    app.handle(key(KeyCode::Char('?')));

    let backend = screen(&app, 80, 24);

    assert!(
        row(backend.buffer(), 0).starts_with(" asqr ◷ read-only"),
        "{}",
        row(backend.buffer(), 0)
    );
}

#[test]
fn a_session_being_answered_shows_no_mark() {
    let session: Session = serde_json::from_str(SESSION).expect("parses");
    let mut app = App::default();
    app.add(SessionState::new("hero-2", session, None));

    let backend = screen(&app, 120, 30);

    assert!(
        !rows(backend.buffer())
            .iter()
            .any(|row| row.contains("read-only"))
    );
}

// =========================================================
// The status line, the tabs and the keys
// =========================================================

#[test]
fn the_status_line_at_rest_says_what_the_screen_is() {
    let backend = screen(&replay_app(HistoryOutcome::Sent), 120, 30);

    assert_eq!(
        row(backend.buffer(), 28),
        "◷ replay · sent Thu 29 Oct 16:41 · read-only · only your answers stand out"
    );
    assert_eq!(
        style_at(backend.buffer(), "◷ replay").fg,
        theme::warning().fg
    );
}

#[test]
fn a_withdrawn_replay_says_what_dim_means() {
    let backend = screen(&replay_app(HistoryOutcome::Withdrawn), 120, 30);

    assert_eq!(
        row(backend.buffer(), 28),
        "◷ replay · withdrawn Thu 29 Oct 16:41 · read-only · dim is your unsent draft"
    );
}

#[test]
fn a_notice_replaces_the_status_line_and_the_mark_stays() {
    let mut app = replay_app(HistoryOutcome::Sent);
    app.notice(crate::tui::Tone::Warning, "something happened");

    let backend = screen(&app, 120, 30);

    assert_eq!(row(backend.buffer(), 28), "! something happened");
    assert!(row(backend.buffer(), 0).starts_with(" asqr ◷ read-only"));
}

#[test]
fn the_last_tab_is_the_result() {
    let backend = screen(&replay_app(HistoryOutcome::Sent), 120, 30);

    let tabs = rows(backend.buffer())[4].clone();

    assert!(tabs.contains("✔ Result"), "{tabs}");
    assert!(!tabs.contains("Review"), "{tabs}");
}

#[test]
fn a_withdrawn_result_tab_carries_the_withdrawn_mark() {
    let backend = screen(&replay_app(HistoryOutcome::Withdrawn), 120, 30);

    assert!(
        rows(backend.buffer())[4].contains("↩ Result"),
        "{}",
        rows(backend.buffer())[4]
    );
}

#[test]
fn the_key_bar_names_only_the_keys_that_work() {
    let backend = screen(&replay_app(HistoryOutcome::Sent), 120, 30);

    assert_eq!(
        row(backend.buffer(), 29),
        " ↑/↓  move   ←/→  question   ?  help   q  quit"
    );
}

// =========================================================
// Sessions that wait
// =========================================================

#[test]
fn waiting_sessions_are_counted_in_the_title_bar_and_the_status_line_says_where_to_answer() {
    let mut app = replay_app(HistoryOutcome::Sent);
    app.set_replay_waiting(2);

    let backend = screen(&app, 120, 30);

    assert!(
        row(backend.buffer(), 0).ends_with("default · replay 7C3FA19K · 2 waiting"),
        "{}",
        row(backend.buffer(), 0)
    );
    assert_eq!(
        row(backend.buffer(), 28),
        "◷ replay · sent Thu 29 Oct 16:41 · read-only · 2 waiting, answer in the asqr that watches the queue"
    );
    assert_eq!(
        style_at(backend.buffer(), "2 waiting").add_modifier,
        theme::secondary().add_modifier,
        "dim, as the count of a live title bar"
    );
}

#[test]
fn a_replay_counts_no_waiting_session_of_its_own() {
    let backend = screen(&replay_app(HistoryOutcome::Sent), 120, 30);

    assert!(
        !row(backend.buffer(), 0).contains("waiting"),
        "{}",
        row(backend.buffer(), 0)
    );
}

#[test]
fn a_narrow_bar_gives_up_the_queue_before_the_title_or_the_count() {
    let mut app = replay_app(HistoryOutcome::Sent);
    app.set_replay_waiting(1);

    let backend = screen(&app, 60, 24);

    let title = row(backend.buffer(), 0);
    assert!(
        title.starts_with(" asqr ◷ read-only  README hero image"),
        "{title}"
    );
    assert!(title.ends_with("1 waiting"), "{title}");
}

#[test]
fn the_status_line_words_every_way_a_session_can_have_ended() {
    let line = |outcome| {
        let backend = screen(&replay_app(outcome), 120, 30);
        row(backend.buffer(), 28)
    };

    assert_eq!(
        line(HistoryOutcome::Rejected),
        "◷ replay · rejected Thu 29 Oct 16:41 · read-only · only your answers stand out"
    );
    assert_eq!(
        line(HistoryOutcome::Error),
        "◷ replay · error Thu 29 Oct 16:41 · read-only"
    );
    assert_eq!(
        line(HistoryOutcome::NoResult),
        "◷ replay · no result, archived Thu 29 Oct 16:41 · read-only"
    );
}

#[test]
fn the_time_is_written_as_a_weekday_a_date_and_the_minute() {
    let time = std::time::SystemTime::from(
        "2026-10-29T16:41:07Z"
            .parse::<jiff::Timestamp>()
            .expect("a timestamp"),
    );

    assert_eq!(
        crate::tui::replay_time_text(time, &jiff::tz::TimeZone::UTC),
        "Thu 29 Oct 16:41"
    );
}

// =========================================================
// The options
// =========================================================

fn dim(buf: &ratatui::buffer::Buffer, text: &str) -> bool {
    buf[find(buf, text)].modifier.contains(Modifier::DIM)
}

#[test]
fn unpicked_options_are_dim_and_the_pick_is_not() {
    let mut app = replay_app(HistoryOutcome::Sent);
    // The cursor stands on the first option, so the pick is not the row
    // the fill covers.
    let backend = screen(&app, 120, 30);
    let buf = backend.buffer();

    assert!(dim(buf, "( ) 3."), "an unpicked option is dim");
    assert!(dim(buf, "C Split"), "its label too");
    assert!(!dim(buf, "(•) 2."), "the pick is not dim");
    assert_eq!(look_at(buf, "(•)").0, theme::chosen().fg.expect("a colour"));
    app.handle(key(KeyCode::Down));
}

#[test]
fn the_cursor_row_keeps_its_fill_on_an_unpicked_option() {
    let backend = screen(&replay_app(HistoryOutcome::Sent), 120, 30);
    let buf = backend.buffer();

    // The cursor starts on the first option, which is not the pick.
    let cell = &buf[find(buf, "A Dark terminal")];

    assert!(
        cell.modifier.contains(Modifier::REVERSED),
        "the cursor row is filled"
    );
    assert!(!cell.modifier.contains(Modifier::DIM), "and not dim");
}

#[test]
fn a_withdrawn_draft_shows_dim_with_its_marker_on_the_pick() {
    let mut app = replay_app(HistoryOutcome::Withdrawn);
    // Off the pick, so the pick row is not under the fill.
    app.handle(key(KeyCode::Up));
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Down));
    let backend = screen(&app, 120, 30);
    let buf = backend.buffer();

    let marked = rows(buf)
        .into_iter()
        .find(|row| row.contains("B Light card"))
        .expect("a row");

    assert!(marked.trim_end().ends_with("◀ draft"), "{marked}");
    assert!(dim(buf, "(•) 2."), "the draft is dim, the pick too");
    assert_eq!(
        rows(buf)
            .iter()
            .filter(|row| row.contains("◀ draft"))
            .count(),
        1
    );
}

#[test]
fn a_sent_answer_has_no_draft_marker() {
    let backend = screen(&replay_app(HistoryOutcome::Sent), 120, 30);

    assert!(
        !rows(backend.buffer())
            .iter()
            .any(|row| row.contains("◀ draft"))
    );
}

// =========================================================
// The result tab
// =========================================================

fn result_tab(outcome: HistoryOutcome) -> Vec<String> {
    let mut app = replay_app(outcome);
    app.handle(key(KeyCode::Right));
    app.handle(key(KeyCode::Right));
    let backend = screen(&app, 120, 30);
    rows(backend.buffer())
}

#[test]
fn the_result_tab_says_how_it_ended_and_lists_the_answers_and_the_note() {
    let rows = result_tab(HistoryOutcome::Sent);
    let text = rows.join("\n");

    assert!(text.contains("Sent Thu 29 Oct 16:41"), "{text}");
    assert!(
        text.contains("Hero") && text.contains("B Light card"),
        "{text}"
    );
    assert!(
        text.contains("answered 2 · skipped 0 · defaulted 0"),
        "{text}"
    );
    assert!(text.contains("✎ Note to the asker: thanks"), "{text}");
    assert!(
        !text.contains("[ Submit ]") && !text.contains("Reject"),
        "{text}"
    );
}

#[test]
fn a_rejected_result_shows_its_reason() {
    let text = result_tab(HistoryOutcome::Rejected).join("\n");

    assert!(text.contains("Rejected Thu 29 Oct 16:41"), "{text}");
    assert!(
        text.contains("✗ Rejected, reason: not this round"),
        "{text}"
    );
}

#[test]
fn a_withdrawn_result_says_the_draft_was_never_sent() {
    let text = result_tab(HistoryOutcome::Withdrawn).join("\n");

    assert!(
        text.contains("Withdrawn Thu 29 Oct 16:41, the answers below were never sent"),
        "{text}"
    );
    assert!(!text.contains("Rejected, reason"), "{text}");
}

#[test]
fn a_session_without_a_result_says_so_on_the_result_tab() {
    let text = result_tab(HistoryOutcome::NoResult).join("\n");

    assert!(text.contains("No result answers this session"), "{text}");
}

// =========================================================
// Whole screens
// =========================================================

/// The cursor stands on the option a withdrawn draft picked, as a person
/// who moved to the pick would see it.
fn on_the_pick(app: &mut App) {
    app.handle(key(KeyCode::Down));
}

#[test]
fn replay_withdrawn_screens() {
    for (width, height) in [(80, 24), (120, 30)] {
        let mut app = replay_app(HistoryOutcome::Withdrawn);
        on_the_pick(&mut app);
        app.handle(key(KeyCode::Down));
        insta::assert_snapshot!(
            format!("replay_withdrawn_{width}x{height}"),
            screen(&app, width, height)
        );
    }
}

#[test]
fn replay_sent_screens() {
    for (width, height) in [(80, 24), (120, 30)] {
        let app = replay_app(HistoryOutcome::Sent);
        insta::assert_snapshot!(
            format!("replay_sent_{width}x{height}"),
            screen(&app, width, height)
        );
    }
}

#[test]
fn replay_result_tab_screens() {
    for (width, height) in [(80, 24), (120, 30)] {
        let mut app = replay_app(HistoryOutcome::Sent);
        app.handle(key(KeyCode::Right));
        app.handle(key(KeyCode::Right));
        insta::assert_snapshot!(
            format!("replay_result_tab_{width}x{height}"),
            screen(&app, width, height)
        );
    }
}

#[test]
fn replay_no_color_screen() {
    let mut view =
        View::for_tests("default", PaneLayout::Auto, Images::half_blocks()).no_color(true);

    insta::assert_snapshot!(
        "replay_no_color_120x30",
        screen_in(&replay_app(HistoryOutcome::Sent), &mut view, 120, 30)
    );
}

#[test]
fn an_error_result_says_it_ended_in_an_error() {
    let text = result_tab(HistoryOutcome::Error).join("\n");

    assert!(
        text.contains("Ended in an error Thu 29 Oct 16:41"),
        "{text}"
    );
}

#[test]
fn a_note_that_was_never_written_shows_as_a_dash() {
    let session: Session = serde_json::from_str(SESSION).expect("parses");
    let mut app = App::default();
    app.add(
        SessionState::replay("hero-2", session, None).with_replay_info(info(HistoryOutcome::Sent)),
    );
    app.handle(key(KeyCode::Right));
    app.handle(key(KeyCode::Right));

    let text = rows(screen(&app, 120, 30).buffer()).join("\n");

    assert!(text.contains("✎ Note to the asker: —"), "{text}");
}
