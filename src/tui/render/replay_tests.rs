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
        bundle: None,
        zone: jiff::tz::TimeZone::UTC,
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

// =========================================================
// Files: cards, the bottom rule and the details
// =========================================================

const SHA: &str = "9f2c61d0b7e3a4c8d1e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7e2b41a6";

fn record(state: &str) -> ReplayFile {
    ReplayFile {
        original_path: "/tmp/claude-501/ci-run-8841/build.log".to_owned(),
        state: state.to_owned(),
        sha256: None,
        size: None,
        modified: None,
        kept_size: None,
        reason: None,
        copy: None,
        damaged: None,
    }
}

/// A replay of a session whose one question shows the code entry `path`,
/// with `file` as what the archive recorded about it.
fn file_app(path: &str, file: ReplayFile, bundle: Option<&str>) -> App {
    let session: Session = serde_json::from_value(serde_json::json!({
        "asqr": 2, "title": "CI failure on main",
        "questions": [{"id": "fix", "text": "Which fix first?", "kind": "single",
            "options": [{"id": "a", "label": "Pin"}, {"id": "b", "label": "Retry"}],
            "context": [{"type": "code", "path": path}]}]
    }))
    .expect("parses");
    let mut info = info(HistoryOutcome::Sent);
    info.files.insert(path.to_owned(), file);
    info.bundle = bundle.map(str::to_owned);
    let mut app = App::default();
    app.add(SessionState::replay("ci", session, None).with_replay_info(info));
    app
}

fn text_of(app: &App, width: u16, height: u16) -> String {
    rows(screen(app, width, height).buffer()).join("\n")
}

#[test]
fn a_missing_file_gets_the_red_card_with_its_original_path() {
    let file = record("missing");
    let app = file_app(&file.original_path.clone(), file, None);

    let backend = screen(&app, 80, 24);
    let text = rows(backend.buffer()).join("\n");

    assert!(
        text.contains("✗ Missing when the session finished"),
        "{text}"
    );
    assert!(text.contains("build.log"), "{text}");
    assert!(text.contains("/tmp/claude-501/ci-run-8841"), "{text}");
    assert!(
        text.contains("asqr could not read it: no copy, no fingerprint"),
        "{text}"
    );
    assert_eq!(
        style_at(backend.buffer(), "✗ Missing").fg,
        theme::error().fg
    );
}

#[test]
fn an_unreadable_file_gives_its_reason() {
    let mut file = record("unreadable");
    file.reason = Some("permission denied".to_owned());
    let app = file_app(&file.original_path.clone(), file, None);

    let text = text_of(&app, 80, 24);

    assert!(
        text.contains("✗ Could not be read when the session finished"),
        "{text}"
    );
    assert!(
        text.contains("asqr could not read it: permission denied"),
        "{text}"
    );
}

#[test]
fn a_file_that_was_not_kept_gets_the_warning_card_with_its_fingerprint() {
    let mut file = record("not_kept");
    file.sha256 = Some(SHA.to_owned());
    file.size = Some(1_420);
    file.modified = Some("2026-10-28T13:02:00Z".to_owned());
    let app = file_app(&file.original_path.clone(), file, None);

    let backend = screen(&app, 80, 24);
    let text = rows(backend.buffer()).join("\n");

    assert!(
        text.contains("! Not kept, only the fingerprint was recorded"),
        "{text}"
    );
    assert!(text.contains("1.4 KB"), "{text}");
    assert!(text.contains("sha256 9f2c61d0b7e3a4c8…7e2b41a6"), "{text}");
    assert_eq!(
        style_at(backend.buffer(), "! Not kept").fg,
        theme::warning().fg
    );
}

#[test]
fn a_file_over_the_limit_says_why_it_was_not_kept() {
    let mut file = record("not_kept");
    file.size = Some(70_000_000);
    file.reason = Some("larger than 64 MiB".to_owned());
    let app = file_app(&file.original_path.clone(), file, None);

    let text = text_of(&app, 80, 24);

    assert!(text.contains("larger than 64 MiB"), "{text}");
    assert!(text.contains("70 MB"), "{text}");
}

#[test]
fn a_damaged_copy_gets_the_red_card() {
    let mut file = record("kept");
    file.damaged = Some("the kept copy is damaged".to_owned());
    file.copy = Some(format!("files/{SHA}.webp"));
    let app = file_app(&format!("files/{SHA}.webp"), file, None);

    let text = text_of(&app, 80, 24);

    assert!(text.contains("✗ The kept copy is damaged"), "{text}");
    assert!(
        text.contains("original /tmp/claude-501/ci-run-8841/build.log"),
        "{text}"
    );
}

#[test]
fn the_pane_header_names_the_original_file_not_the_copy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let copy = dir.path().join(format!("{SHA}.dat"));
    std::fs::write(&copy, "all:\n").expect("written");
    let mut file = record("kept");
    file.original_path = "/work/Makefile".to_owned();
    file.kept_size = Some(5);
    file.copy = Some(format!("files/{SHA}.dat"));
    let app = file_app(copy.to_str().expect("UTF-8"), file, None);

    let text = text_of(&app, 120, 30);

    assert!(text.contains(" Makefile "), "{text}");
    assert!(!text.contains(".dat"), "{text}");
}

#[test]
fn the_bottom_rule_says_what_became_of_the_file() {
    let rule = |file: ReplayFile, path: &str| {
        let text = text_of(&file_app(path, file, None), 120, 30);
        text.lines()
            .find(|line| line.contains("d details"))
            .map(str::to_owned)
            .unwrap_or(text)
    };
    let dir = tempfile::tempdir().expect("temp dir");
    let copy = dir.path().join("x.txt");
    std::fs::write(&copy, "x").expect("written");
    let mut kept = record("kept");
    kept.kept_size = Some(213_456);
    let mut not_kept = record("not_kept");
    not_kept.sha256 = Some(SHA.to_owned());

    assert!(rule(kept, copy.to_str().expect("UTF-8")).contains(" kept copy · 213 KB · d details "));
    assert!(rule(record("missing"), "/tmp/gone.log").contains(" no copy · d details "));
    assert!(rule(not_kept, "/tmp/gone.log").contains(" fingerprint only · d details "));
}

#[test]
fn d_opens_the_details_of_the_file_and_any_key_closes_them() {
    let dir = tempfile::tempdir().expect("temp dir");
    let copy = dir.path().join("x.txt");
    std::fs::write(&copy, "x").expect("written");
    let mut file = record("kept");
    file.original_path = "/work/hero-b.png".to_owned();
    file.sha256 = Some(SHA.to_owned());
    file.size = Some(213_456);
    file.kept_size = Some(41_207);
    file.modified = Some("2026-10-29T16:40:11Z".to_owned());
    file.copy = Some(format!("files/{SHA}.webp"));
    let mut app = file_app(copy.to_str().expect("UTF-8"), file, Some("2026-10.zip"));
    let before = text_of(&app, 120, 30);

    app.handle(key(KeyCode::Char('d')));
    let open = text_of(&app, 120, 30);

    assert!(!before.contains("SHA-256"), "{before}");
    assert!(open.contains("Kept copy"), "{open}");
    assert!(open.contains(&format!("files/{SHA}.webp")), "{open}");
    assert!(
        open.lines()
            .any(|line| line.contains("Stored in") && line.contains("2026-10.zip")),
        "{open}"
    );
    assert!(open.contains("/work/hero-b.png"), "{open}");
    assert!(open.contains("213456 bytes (kept 41207 bytes)"), "{open}");
    assert!(open.contains("Thu 29 Oct 16:40"), "{open}");
    assert!(
        open.lines()
            .any(|line| line.contains("SHA-256") && line.contains(SHA)),
        "{open}"
    );

    app.handle(key(KeyCode::Char('x')));
    assert!(!text_of(&app, 120, 30).contains("SHA-256"));
}

#[test]
fn d_on_an_entry_without_a_record_opens_nothing() {
    let mut app = replay_app(HistoryOutcome::Sent);

    app.handle(key(KeyCode::Char('d')));

    assert!(!text_of(&app, 120, 30).contains("SHA-256"));
}

#[test]
fn the_key_bar_names_d_where_a_file_has_details() {
    let file = record("missing");
    let app = file_app(&file.original_path.clone(), file, None);

    let bar = rows(screen(&app, 120, 30).buffer())[29].clone();

    assert!(bar.contains("d  details"), "{bar}");
}

#[test]
fn the_help_of_a_replay_lists_the_keys_that_work() {
    let mut app = replay_app(HistoryOutcome::Sent);
    app.handle(key(KeyCode::Char('?')));

    let text = text_of(&app, 120, 30);

    assert!(text.contains("details of a kept file"), "{text}");
    assert!(
        !text.contains("toggle an option") && !text.contains("write a note"),
        "{text}"
    );
}

// =========================================================
// Whole screens with a file
// =========================================================

/// A replay whose question shows `entry`, with the files the archive
/// recorded.
fn app_showing(
    entry: serde_json::Value,
    files: Vec<(String, ReplayFile)>,
    bundle: Option<&str>,
) -> App {
    let session: Session = serde_json::from_value(serde_json::json!({
        "asqr": 2, "title": "README hero image, round 2",
        "from": "claude-code, asqr-site",
        "questions": [{"id": "hero", "header": "Hero", "text": "Which hero image should the README use?",
            "kind": "single",
            "options": [{"id": "a", "label": "Dark terminal"}, {"id": "b", "label": "Light card"}],
            "context": [entry]}]
    }))
    .expect("parses");
    let answers = vec![Answer {
        selected: vec!["b".to_owned()],
        ..Answer::new("hero")
    }];
    let draft = SessionResult::draft("hero-2", "hero", answers);
    let mut info = info(HistoryOutcome::Sent);
    info.files.extend(files);
    info.bundle = bundle.map(str::to_owned);
    let mut app = App::default();
    app.add(SessionState::replay("hero-2", session, Some(&draft)).with_replay_info(info));
    app
}

/// A small picture in a file of `dir`.
fn picture(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join(format!("{SHA}.png"));
    let mut image = image::RgbaImage::from_pixel(64, 40, image::Rgba([240, 228, 190, 255]));
    for x in 8..40 {
        for y in 10..16 {
            image.put_pixel(x, y, image::Rgba([150, 130, 110, 255]));
        }
    }
    image.save(&path).expect("saved");
    path
}

#[test]
fn replay_kept_image_screens() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = picture(dir.path()).to_str().expect("UTF-8").to_owned();
    let mut file = record("kept");
    file.original_path = "/tmp/claude-501/hero-b.png".to_owned();
    file.kept_size = Some(213_456);
    file.copy = Some(format!("files/{SHA}.png"));
    for (width, height) in [(80, 24), (120, 30)] {
        let app = app_showing(
            serde_json::json!({"type": "image", "path": path}),
            vec![(path.clone(), file.clone())],
            None,
        );
        insta::assert_snapshot!(
            format!("replay_kept_image_{width}x{height}"),
            screen(&app, width, height)
        );
    }
}

#[test]
fn replay_missing_screens() {
    let file = record("missing");
    let path = file.original_path.clone();
    for (width, height) in [(80, 24), (120, 30)] {
        let app = app_showing(
            serde_json::json!({"type": "code", "path": path}),
            vec![(path.clone(), file.clone())],
            None,
        );
        insta::assert_snapshot!(
            format!("replay_missing_{width}x{height}"),
            screen(&app, width, height)
        );
    }
}

#[test]
fn replay_not_kept_screens() {
    let mut file = record("not_kept");
    file.original_path = "/tmp/claude-501/changelog-short.md".to_owned();
    file.sha256 = Some(SHA.to_owned());
    file.size = Some(1_420);
    file.modified = Some("2026-10-28T13:02:00Z".to_owned());
    let path = file.original_path.clone();
    for (width, height) in [(80, 24), (120, 30)] {
        let app = app_showing(
            serde_json::json!({"type": "markdown", "path": path}),
            vec![(path.clone(), file.clone())],
            None,
        );
        insta::assert_snapshot!(
            format!("replay_not_kept_{width}x{height}"),
            screen(&app, width, height)
        );
    }
}

#[test]
fn replay_details_screens() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = picture(dir.path()).to_str().expect("UTF-8").to_owned();
    let mut file = record("kept");
    file.original_path = "/tmp/claude-501/hero-b.png".to_owned();
    file.sha256 = Some(SHA.to_owned());
    file.size = Some(213_456);
    file.kept_size = Some(41_207);
    file.modified = Some("2026-10-29T16:40:11Z".to_owned());
    file.copy = Some(format!("files/{SHA}.png"));
    for (width, height) in [(80, 24), (120, 30)] {
        let mut app = app_showing(
            serde_json::json!({"type": "image", "path": path}),
            vec![(path.clone(), file.clone())],
            Some("2026-10.zip"),
        );
        app.handle(key(KeyCode::Char('d')));
        insta::assert_snapshot!(
            format!("replay_details_{width}x{height}"),
            screen(&app, width, height)
        );
    }
}

#[test]
fn a_pane_taller_than_its_body_names_the_lines_before_the_words() {
    let dir = tempfile::tempdir().expect("temp dir");
    let copy = dir.path().join("long.txt");
    std::fs::write(
        &copy,
        (1..=200).map(|n| format!("line {n}\n")).collect::<String>(),
    )
    .expect("written");
    let mut file = record("kept");
    file.kept_size = Some(1_800);
    let app = file_app(copy.to_str().expect("UTF-8"), file, None);

    let text = text_of(&app, 120, 30);

    let rule = text
        .lines()
        .find(|line| line.contains("d details"))
        .expect("a rule");
    assert!(
        rule.contains(" of 200 · kept copy · 1.8 KB · d details "),
        "{rule}"
    );
}

#[test]
fn the_entrys_own_title_names_the_pane_not_the_original_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let copy = dir.path().join("x.txt");
    std::fs::write(&copy, "x").expect("written");
    let mut file = record("kept");
    file.original_path = "/work/build.log".to_owned();
    let app = app_showing(
        serde_json::json!({"type": "ansi", "path": copy, "title": "The CI log"}),
        vec![(copy.to_str().expect("UTF-8").to_owned(), file)],
        None,
    );

    let text = text_of(&app, 120, 30);

    assert!(text.contains(" The CI log "), "{text}");
    assert!(!text.contains("build.log"), "{text}");
}

#[test]
fn the_details_of_a_file_that_was_not_kept_or_is_damaged_say_why() {
    let details = |file: ReplayFile| {
        let path = file
            .copy
            .clone()
            .unwrap_or_else(|| file.original_path.clone());
        let mut app = file_app(&path, file, None);
        app.handle(key(KeyCode::Char('d')));
        text_of(&app, 120, 30)
    };
    let mut over = record("not_kept");
    over.size = Some(70_000_000);
    over.reason = Some("larger than 64 MiB".to_owned());
    let mut damaged = record("kept");
    damaged.copy = Some(format!("files/{SHA}.webp"));
    damaged.damaged = Some("the kept copy is damaged".to_owned());

    let over = details(over);
    let damaged = details(damaged);

    assert!(
        over.contains("Size      70000000 bytes") || over.contains("70000000 bytes"),
        "{over}"
    );
    assert!(
        over.contains("Reason") && over.contains("larger than 64 MiB"),
        "{over}"
    );
    assert!(
        damaged.contains("Problem") && damaged.contains("the kept copy is damaged"),
        "{damaged}"
    );
}

#[test]
fn a_kept_file_without_a_recorded_size_still_names_itself() {
    let dir = tempfile::tempdir().expect("temp dir");
    let copy = dir.path().join("x.txt");
    std::fs::write(&copy, "x").expect("written");
    let file = record("kept");

    let text = text_of(
        &file_app(copy.to_str().expect("UTF-8"), file, None),
        120,
        30,
    );

    assert!(
        text.lines()
            .any(|line| line.contains("─ kept copy · d details ")),
        "{text}"
    );
}

#[test]
fn an_ansi_and_a_markdown_file_are_named_by_their_title_or_else_by_the_original() {
    let dir = tempfile::tempdir().expect("temp dir");
    let copy = dir.path().join("copy.txt");
    std::fs::write(&copy, "x").expect("written");
    let path = copy.to_str().expect("UTF-8").to_owned();
    let named = |kind: &str, title: Option<&str>| {
        let mut file = record("kept");
        file.original_path = "/work/session.out".to_owned();
        let mut entry = serde_json::json!({"type": kind, "path": path});
        if let Some(title) = title {
            entry["title"] = serde_json::json!(title);
        }
        text_of(
            &app_showing(entry, vec![(path.clone(), file)], None),
            120,
            30,
        )
    };

    for kind in ["ansi", "markdown"] {
        let titled = named(kind, Some("Titled"));
        assert!(
            titled.contains(" Titled ") && !titled.contains("session.out"),
            "{kind}: {titled}"
        );
        let untitled = named(kind, None);
        assert!(untitled.contains(" session.out "), "{kind}: {untitled}");
    }
}
