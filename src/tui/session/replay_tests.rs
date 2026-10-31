// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A finished session shown again (ADR 101): the keys that change an
//! answer do nothing, and no key asks the app to save, send or open a
//! queue.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::format::{Answer, SessionResult};

const SESSION: &str = r#"{"asqr": 2, "title": "Pick", "intro": "Hello", "questions": [
    {"id": "single", "text": "?", "kind": "single", "custom": true,
     "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B", "default": true}],
     "context": [{"type": "code", "content": "x = 1"}]},
    {"id": "multi", "text": "?", "kind": "multi", "custom": true,
     "options": [{"id": "x", "label": "X"}, {"id": "y", "label": "Y"}]},
    {"id": "text", "text": "?", "kind": "text"}
]}"#;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn typed(letter: char) -> KeyEvent {
    key(KeyCode::Char(letter))
}

/// A replay whose person picked `a`, `x` and typed `hello`, with a note on
/// the first question.
fn replay() -> SessionState {
    let answers = vec![
        Answer {
            selected: vec!["a".to_owned()],
            note: Some("because".to_owned()),
            ..Answer::new("single")
        },
        Answer {
            selected: vec!["x".to_owned()],
            ..Answer::new("multi")
        },
        Answer {
            custom: Some("hello".to_owned()),
            ..Answer::new("text")
        },
    ];
    let mut draft = SessionResult::draft("batch", "single", answers);
    draft.note = Some("overall".to_owned());
    SessionState::replay(
        "batch",
        serde_json::from_str(SESSION).expect("parses"),
        Some(&draft),
    )
}

/// The effects of `keys`, pressed in order on the tab `tab`.
fn pressed(state: &mut SessionState, keys: &[KeyEvent]) -> Vec<Effect> {
    keys.iter().map(|key| state.handle(*key)).collect()
}

fn on_tab(tab: usize) -> SessionState {
    let mut state = replay();
    for _ in 0..tab {
        state.handle(key(KeyCode::Right));
    }
    assert_eq!(state.tab(), tab);
    state
}

#[test]
fn a_replay_is_read_only_and_starts_on_the_first_question() {
    let state = replay();

    assert!(state.is_read_only());
    assert_eq!(state.tab(), 0);
    assert!(
        !SessionState::new(
            "batch",
            serde_json::from_str(SESSION).expect("parses"),
            None
        )
        .is_read_only()
    );
}

#[test]
fn the_answers_of_the_result_show_as_picked() {
    let state = replay();

    let answers = state.answers();

    assert_eq!(answers[0].selected, ["a"]);
    assert_eq!(answers[1].selected, ["x"]);
    assert_eq!(answers[2].custom.as_deref(), Some("hello"));
    assert_eq!(state.session_note(), "overall");
}

#[test]
fn space_picks_nothing() {
    let mut state = on_tab(1);

    let effects = pressed(&mut state, &[typed(' '), key(KeyCode::Down), typed(' ')]);

    assert_eq!(state.answers()[1].selected, ["x"]);
    assert!(
        effects.iter().all(|effect| *effect == Effect::None),
        "{effects:?}"
    );
}

#[test]
fn a_digit_picks_nothing() {
    let mut state = replay();

    let effects = pressed(&mut state, &[typed('2'), typed('1')]);

    assert_eq!(state.answers()[0].selected, ["a"]);
    assert_eq!(state.tab(), 0, "a pick moves on in a live session");
    assert!(
        effects.iter().all(|effect| *effect == Effect::None),
        "{effects:?}"
    );
}

#[test]
fn n_opens_no_note() {
    let mut state = replay();

    state.handle(typed('n'));

    assert!(state.field().is_none());
    state.handle(typed('x'));
    assert_eq!(state.answers()[0].note.as_deref(), Some("because"));
}

#[test]
fn backspace_clears_no_pick() {
    let mut state = replay();

    state.handle(key(KeyCode::Backspace));

    assert_eq!(state.answers()[0].selected, ["a"]);
}

#[test]
fn typing_changes_no_answer_and_focuses_no_field() {
    let mut state = on_tab(2);

    let effects = pressed(
        &mut state,
        &[typed('x'), typed('y'), key(KeyCode::Backspace)],
    );

    assert!(state.field().is_none(), "no field takes the keys");
    assert_eq!(state.answers()[2].custom.as_deref(), Some("hello"));
    assert!(
        effects.iter().all(|effect| *effect == Effect::None),
        "{effects:?}"
    );
}

#[test]
fn a_paste_changes_no_answer() {
    let mut state = on_tab(2);

    let effect = state.paste("pasted text");

    assert_eq!(effect, Effect::None);
    assert_eq!(state.answers()[2].custom.as_deref(), Some("hello"));
    assert!(
        state.message().is_none(),
        "no notice says where a paste goes"
    );
}

#[test]
fn ctrl_g_opens_no_editor() {
    let mut state = on_tab(2);

    let effect = state.handle(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));

    assert_eq!(effect, Effect::None);
}

#[test]
fn l_w_and_the_comma_open_nothing() {
    let mut state = replay();

    let effects = pressed(&mut state, &[typed('L'), typed('w'), typed(',')]);

    assert!(
        effects.iter().all(|effect| *effect == Effect::None),
        "{effects:?}"
    );
}

#[test]
fn enter_walks_to_the_result_and_stops_there() {
    let mut state = replay();

    let effects = pressed(&mut state, &[key(KeyCode::Enter); 5]);

    assert!(state.on_review(), "the last tab is where enter ends");
    assert_eq!(
        state.answers()[0].selected,
        ["a"],
        "enter on an option picks nothing"
    );
    assert!(
        effects.iter().all(|effect| *effect == Effect::None),
        "{effects:?}"
    );
}

#[test]
fn nothing_on_the_result_tab_sends_or_rejects() {
    let mut state = on_tab(3);

    let effects = pressed(
        &mut state,
        &[
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Enter),
        ],
    );

    assert!(
        effects.iter().all(|effect| matches!(effect, Effect::None)),
        "{effects:?}"
    );
}

#[test]
fn moving_between_questions_saves_no_draft() {
    let mut state = replay();

    let effects = pressed(
        &mut state,
        &[
            key(KeyCode::Right),
            key(KeyCode::Left),
            typed('l'),
            typed('h'),
        ],
    );

    assert!(
        effects.iter().all(|effect| *effect == Effect::None),
        "{effects:?}"
    );
}

#[test]
fn the_keys_that_only_look_keep_working() {
    let mut state = replay();

    assert_eq!(state.handle(typed('q')), Effect::Quit);
    assert_eq!(
        state.handle(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Effect::Quit
    );
    assert_eq!(state.handle(typed('i')), Effect::ShowIntro);
    assert!(matches!(state.handle(typed('o')), Effect::Open(_)));
    state.handle(typed('?'));
    assert!(state.help());
    state.handle(typed('x'));
    state.handle(typed('z'));
    assert!(state.full_screen());
}

#[test]
fn a_replay_asks_the_app_for_nothing_but_quit_and_open() {
    let mut state = replay();
    let mut asked = Vec::new();
    let every_key = [
        key(KeyCode::Enter),
        key(KeyCode::Down),
        key(KeyCode::Up),
        key(KeyCode::Tab),
        key(KeyCode::Esc),
        key(KeyCode::Backspace),
        typed(' '),
        typed('1'),
        typed('n'),
        typed('L'),
        typed('w'),
        typed(','),
        typed('x'),
        key(KeyCode::Right),
        key(KeyCode::Left),
        key(KeyCode::PageDown),
    ];
    for _ in 0..4 {
        for pressed in every_key {
            asked.push(state.handle(pressed));
        }
        state.paste("text");
    }

    assert!(
        asked
            .iter()
            .all(|effect| matches!(effect, Effect::None | Effect::Quit | Effect::Open(_))),
        "{asked:?}"
    );
}

#[test]
fn enter_on_a_question_of_the_result_tab_goes_to_that_question() {
    let mut state = on_tab(3);
    // The result tab lists the questions above Submit.
    for _ in 0..3 {
        state.handle(key(KeyCode::Up));
    }

    let effect = state.handle(key(KeyCode::Enter));

    assert_eq!(effect, Effect::None);
    assert_eq!(state.tab(), 0);
}

// =========================================================
// `o` opens only what the archive kept
// =========================================================

fn file_in_archive(state: &str, damaged: Option<&str>) -> crate::queue::ReplayFile {
    crate::queue::ReplayFile {
        original_path: "/work/build.log".to_owned(),
        state: state.to_owned(),
        sha256: None,
        size: None,
        modified: None,
        kept_size: None,
        reason: None,
        copy: None,
        damaged: damaged.map(str::to_owned),
    }
}

/// A replay whose one question shows `entry`, with `files` as what the
/// archive recorded.
fn replay_showing(
    entry: serde_json::Value,
    files: Vec<(&str, crate::queue::ReplayFile)>,
) -> SessionState {
    let session = serde_json::from_value(serde_json::json!({"asqr": 2, "title": "T",
        "questions": [{"id": "q", "text": "?", "kind": "single",
            "options": [{"id": "a", "label": "A"}], "context": [entry]}]}))
    .expect("parses");
    let info = ReplayInfo {
        reference: "7C3FA19K".to_owned(),
        outcome: crate::queue::HistoryOutcome::Sent,
        finished: "Thu 29 Oct 16:41".to_owned(),
        files: files
            .into_iter()
            .map(|(path, file)| (path.to_owned(), file))
            .collect(),
        bundle: None,
        zone: jiff::tz::TimeZone::UTC,
    };
    SessionState::replay("t", session, None).with_replay_info(info)
}

fn refused(state: &mut SessionState) -> bool {
    let effect = state.handle(typed('o'));
    effect == Effect::None
        && state
            .message()
            .is_some_and(|notice| notice.text.contains("kept no copy"))
}

fn code_at(path: &str) -> serde_json::Value {
    serde_json::json!({"type": "code", "path": path})
}

#[test]
fn o_is_refused_on_a_file_without_a_record() {
    let mut state = replay_showing(
        code_at("/work/build.log"),
        vec![("/work/build.log", file_in_archive("not_recorded", None))],
    );

    assert!(refused(&mut state));
}

#[test]
fn o_is_refused_on_a_file_that_was_missing() {
    let mut state = replay_showing(
        code_at("/work/build.log"),
        vec![("/work/build.log", file_in_archive("missing", None))],
    );

    assert!(refused(&mut state));
}

#[test]
fn o_is_refused_on_a_file_that_was_not_kept() {
    let mut state = replay_showing(
        code_at("/work/build.log"),
        vec![("/work/build.log", file_in_archive("not_kept", None))],
    );

    assert!(refused(&mut state));
}

#[test]
fn o_is_refused_on_a_file_that_could_not_be_read() {
    let mut state = replay_showing(
        code_at("/work/build.log"),
        vec![("/work/build.log", file_in_archive("unreadable", None))],
    );

    assert!(refused(&mut state));
}

#[test]
fn o_is_refused_on_a_damaged_copy_whose_path_is_relative() {
    let mut state = replay_showing(
        code_at("files/abc.dat"),
        vec![(
            "files/abc.dat",
            file_in_archive("kept", Some("the kept copy is damaged")),
        )],
    );

    assert!(refused(&mut state));
}

#[test]
fn o_is_refused_on_a_diff_when_one_side_was_not_kept() {
    let mut state = replay_showing(
        serde_json::json!({"type": "diff",
            "old": {"path": "/scratch/1/old.rs"}, "new": {"path": "/work/new.rs"}}),
        vec![
            ("/scratch/1/old.rs", file_in_archive("kept", None)),
            ("/work/new.rs", file_in_archive("not_recorded", None)),
        ],
    );

    assert!(refused(&mut state));
}

#[test]
fn o_opens_a_kept_file_and_a_diff_of_two_kept_files() {
    let mut file = state_of(code_at("/scratch/1/a.rs"), &["/scratch/1/a.rs"]);
    let mut diff = state_of(
        serde_json::json!({"type": "diff",
            "old": {"path": "/scratch/1/old.rs"}, "new": {"path": "/scratch/2/new.rs"}}),
        &["/scratch/1/old.rs", "/scratch/2/new.rs"],
    );

    assert!(matches!(file.handle(typed('o')), Effect::Open(_)));
    assert!(matches!(diff.handle(typed('o')), Effect::Open(_)));
}

fn state_of(entry: serde_json::Value, kept: &[&str]) -> SessionState {
    replay_showing(
        entry,
        kept.iter()
            .map(|path| (*path, file_in_archive("kept", None)))
            .collect(),
    )
}
