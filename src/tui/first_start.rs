// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The first-start screen (spec section 7.1, ADR 106): the first time the
//! TUI starts with no settings file, it asks the settings that need a
//! deliberate answer, writes the file and does not ask again. Like the
//! start picker, it runs before the run loop, on the run loop's channel of
//! events. This module holds the screen's state, its keys and its texts.

use std::path::Path;
use std::sync::mpsc::Receiver;

use anyhow::Context;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::render::draw_first_start;
use super::{Event, Notice, Tone, save_failed_text, saved_text};
use crate::settings::{Setting, SettingsStore};

/// What stands above the questions.
pub const LEAD: &str =
    "Before your first session, one setting needs your answer. Press , later to change it.";

/// The button that writes the file.
pub const BUTTON: &str = "[ Save and start ]";

/// What stands under the button.
pub const QUIT_LINE: &str = "q quits without writing; the next start asks again.";

/// A setting the screen asks, with the question and why it is asked.
pub struct Question {
    pub setting: Setting,
    pub text: &'static str,
    pub why: &'static str,
}

/// The settings the screen asks, in order.
pub const QUESTIONS: [Question; 1] = [Question {
    setting: Setting::KeepFiles,
    text: "Keep copies of the files a session showed you?",
    why: "Files a session shows are often temporary. Replay can show them later only if a copy was kept.",
}];

/// What the cursor stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// Value number `value` of question number `question`.
    Value {
        question: usize,
        value: usize,
    },
    Button,
}

/// What a key did on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstStartKey {
    /// It moved the cursor, picked a value, or did nothing.
    Stay,
    /// The button was pressed: the answers are written.
    Save,
    Quit,
}

/// The screen's state: the value picked for each question and the row the
/// cursor stands on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstStart {
    picked: [usize; QUESTIONS.len()],
    /// An index into [`FirstStart::rows`].
    cursor: usize,
}

impl Default for FirstStart {
    /// Every question has its default picked, and the cursor stands on the
    /// first question's.
    fn default() -> Self {
        let picked = QUESTIONS.map(|question| super::default_choice(question.setting));
        FirstStart {
            picked,
            cursor: picked[0],
        }
    }
}

impl FirstStart {
    /// The rows the cursor goes through, top to bottom: the values of each
    /// question, then the button.
    fn rows() -> Vec<Cursor> {
        let values = QUESTIONS.iter().enumerate().flat_map(|(question, asked)| {
            (0..asked.setting.choices()).map(move |value| Cursor::Value { question, value })
        });
        values.chain([Cursor::Button]).collect()
    }

    pub fn cursor(&self) -> Cursor {
        Self::rows()[self.cursor]
    }

    /// The value picked for question number `question`.
    pub fn picked(&self, question: usize) -> usize {
        self.picked[question]
    }

    /// The setting and value number of every answer.
    pub fn choices(&self) -> Vec<(Setting, usize)> {
        QUESTIONS
            .iter()
            .zip(self.picked)
            .map(|(question, value)| (question.setting, value))
            .collect()
    }

    /// Handles one key. `enter` on a value picks it and moves on to the
    /// next question, or the button after the last one; `space` only picks.
    pub fn handle(&mut self, key: KeyEvent) -> FirstStartKey {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('c') {
            return FirstStartKey::Quit;
        }
        if control {
            return FirstStartKey::Stay;
        }
        let rows = Self::rows();
        match (key.code, rows[self.cursor]) {
            (KeyCode::Char('q'), _) => FirstStartKey::Quit,
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                self.cursor = self.cursor.saturating_sub(1);
                FirstStartKey::Stay
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                self.cursor = (self.cursor + 1).min(rows.len() - 1);
                FirstStartKey::Stay
            }
            (KeyCode::Enter, Cursor::Button) => FirstStartKey::Save,
            (KeyCode::Enter, Cursor::Value { question, value }) => {
                self.picked[question] = value;
                self.cursor = self.next_question_row(question);
                FirstStartKey::Stay
            }
            (KeyCode::Char(' '), Cursor::Value { question, value }) => {
                self.picked[question] = value;
                FirstStartKey::Stay
            }
            _ => FirstStartKey::Stay,
        }
    }

    /// The row after `question`: its next question's picked value, or the
    /// button.
    fn next_question_row(&self, question: usize) -> usize {
        let next = question + 1;
        Self::rows()
            .iter()
            .position(|row| match *row {
                Cursor::Value { question, value } => question == next && value == self.picked[next],
                Cursor::Button => next == QUESTIONS.len(),
            })
            .expect("every question has a picked row and the button follows the last")
    }
}

/// How the first start ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstStartEnd {
    /// The person quit: nothing was written, and the next start asks again.
    Quit,
    /// The answers are in `store`, written to the file where that worked.
    /// `notice` tells which.
    Answered {
        store: SettingsStore,
        notice: Notice,
    },
}

/// Shows the first-start screen until the person answers or quits. The
/// answers are chosen in a copy of `store`, which goes back with the
/// notice the save earned: the pane's own `✔ saved` text, or its text for
/// a write that failed, where the choice holds for this run and the next
/// start asks again. `home` shows as `~` in the file's path, `version` is
/// asqr's name and version for the key bar, and `no_color` draws the
/// screen without colours.
///
/// The screen reads `events`, the run loop's channel, so a signal that
/// quits asqr quits the screen too. Only keys, pastes, resizes, a quit and
/// the end of the input matter before the run loop starts, and the rest is
/// dropped.
pub fn ask_first_start<B: Backend>(
    terminal: &mut Terminal<B>,
    events: &Receiver<Event>,
    version: &str,
    store: &SettingsStore,
    home: Option<&Path>,
    no_color: bool,
) -> anyhow::Result<FirstStartEnd>
where
    B::Error: Send + Sync + 'static,
{
    let mut screen = FirstStart::default();
    loop {
        terminal
            .draw(|frame| {
                draw_first_start(frame, &screen, store.path(), home, version, no_color);
            })
            .context("drawing the screen")?;
        let Ok(event) = events.recv() else {
            return Ok(FirstStartEnd::Quit);
        };
        let answer = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => screen.handle(key),
            Event::Quit => return Ok(FirstStartEnd::Quit),
            Event::InputEnded(reason) => {
                return Err(anyhow::Error::msg(reason)).context("reading the terminal's input");
            }
            _ => continue,
        };
        match answer {
            FirstStartKey::Stay => {}
            FirstStartKey::Quit => return Ok(FirstStartEnd::Quit),
            FirstStartKey::Save => return Ok(save(&screen, store)),
        }
    }
}

/// Writes the answers of `screen` into the settings file, one key each, as
/// the pane does. The first write that fails ends the saving.
fn save(screen: &FirstStart, store: &SettingsStore) -> FirstStartEnd {
    let mut store = store.clone();
    let mut notice = None;
    for (setting, choice) in screen.choices() {
        store.choose(setting, choice);
        if let Err(error) = store.save(setting) {
            notice = Some(Notice::new(Tone::Error, save_failed_text(&error)));
            break;
        }
        notice = Some(Notice::new(Tone::Success, saved_text(setting, choice)));
    }
    FirstStartEnd::Answered {
        store,
        notice: notice.expect("the screen asks at least one question"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use ratatui::backend::TestBackend;

    use super::*;
    use crate::keep::KeepFiles;
    use crate::settings::Locks;

    fn press(screen: &mut FirstStart, codes: &[KeyCode]) -> Vec<FirstStartKey> {
        codes
            .iter()
            .map(|code| screen.handle(KeyEvent::new(*code, KeyModifiers::NONE)))
            .collect()
    }

    const FIRST: Cursor = Cursor::Value {
        question: 0,
        value: 0,
    };
    const SECOND: Cursor = Cursor::Value {
        question: 0,
        value: 1,
    };
    const THIRD: Cursor = Cursor::Value {
        question: 0,
        value: 2,
    };

    #[test]
    fn the_default_is_picked_and_the_cursor_stands_on_it() {
        let screen = FirstStart::default();

        assert_eq!(screen.cursor(), FIRST);
        assert_eq!(screen.picked(0), 0, "compact copies");
        assert_eq!(screen.choices(), [(Setting::KeepFiles, 0)]);
    }

    #[test]
    fn the_cursor_moves_through_the_values_to_the_button_and_stops_at_both_ends() {
        let mut screen = FirstStart::default();

        press(&mut screen, &[KeyCode::Up, KeyCode::Char('k')]);
        assert_eq!(screen.cursor(), FIRST, "no row above the first");
        let expected = [
            (KeyCode::Down, SECOND),
            (KeyCode::Char('j'), THIRD),
            (KeyCode::Down, Cursor::Button),
            (KeyCode::Down, Cursor::Button),
            (KeyCode::Up, THIRD),
            (KeyCode::Char('k'), SECOND),
        ];
        for (code, cursor) in expected {
            assert_eq!(press(&mut screen, &[code]), [FirstStartKey::Stay]);
            assert_eq!(screen.cursor(), cursor, "{code:?}");
        }
        assert_eq!(screen.picked(0), 0, "moving picks nothing");
    }

    #[test]
    fn enter_picks_the_value_and_moves_to_the_button() {
        let mut screen = FirstStart::default();

        let answers = press(&mut screen, &[KeyCode::Down, KeyCode::Enter]);

        assert_eq!(answers, [FirstStartKey::Stay, FirstStartKey::Stay]);
        assert_eq!(screen.picked(0), 1);
        assert_eq!(screen.cursor(), Cursor::Button);
        assert_eq!(screen.choices(), [(Setting::KeepFiles, 1)]);
    }

    #[test]
    fn space_picks_the_value_and_stays() {
        let mut screen = FirstStart::default();

        press(&mut screen, &[KeyCode::Down, KeyCode::Down]);
        press(&mut screen, &[KeyCode::Char(' ')]);

        assert_eq!(screen.picked(0), 2);
        assert_eq!(screen.cursor(), THIRD);
    }

    #[test]
    fn enter_on_the_button_saves_and_space_there_does_nothing() {
        let mut screen = FirstStart::default();
        press(&mut screen, &[KeyCode::Enter]);
        assert_eq!(screen.cursor(), Cursor::Button);

        assert_eq!(
            press(&mut screen, &[KeyCode::Char(' ')]),
            [FirstStartKey::Stay]
        );
        assert_eq!(press(&mut screen, &[KeyCode::Enter]), [FirstStartKey::Save]);
    }

    #[test]
    fn q_and_ctrl_c_quit_and_every_other_key_does_nothing() {
        let mut screen = FirstStart::default();

        assert_eq!(
            press(&mut screen, &[KeyCode::Char('q')]),
            [FirstStartKey::Quit]
        );
        assert_eq!(
            screen.handle(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            FirstStartKey::Quit
        );
        let others = [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL),
        ];
        for key in others {
            assert_eq!(screen.handle(key), FirstStartKey::Stay, "{key:?}");
        }
        assert_eq!(screen, FirstStart::default());
    }

    // -----------------------------------------------------------------
    // The loop
    // -----------------------------------------------------------------

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// A store for a settings file in `scratch` that is not there.
    fn store_in(scratch: &tempfile::TempDir) -> SettingsStore {
        let path = scratch.path().join("asqr").join("settings.toml");
        SettingsStore::load(Some(path), Locks::default()).store
    }

    /// Asks the first start over `events` and gives how it ended and the
    /// screen it left.
    fn ask(store: &SettingsStore, events: Vec<Event>) -> (anyhow::Result<FirstStartEnd>, String) {
        let (sender, receiver) = mpsc::channel();
        for event in events {
            sender.send(event).expect("sent");
        }
        drop(sender);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let ended = ask_first_start(
            &mut terminal,
            &receiver,
            "asqr 0.0.0",
            store,
            Some(Path::new("/home/me")),
            false,
        );
        (ended, terminal.backend().to_string())
    }

    fn written(store: &SettingsStore) -> Option<String> {
        std::fs::read_to_string(store.path().expect("a path")).ok()
    }

    fn answered(ended: anyhow::Result<FirstStartEnd>) -> (SettingsStore, Notice) {
        match ended.expect("ends") {
            FirstStartEnd::Answered { store, notice } => (store, notice),
            FirstStartEnd::Quit => panic!("the person answered"),
        }
    }

    #[test]
    fn saving_the_default_writes_the_file_and_says_so() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let store = store_in(&scratch);

        let (ended, _) = ask(&store, vec![key(KeyCode::Enter), key(KeyCode::Enter)]);

        let (held, notice) = answered(ended);
        assert_eq!(notice.tone, Tone::Success);
        assert_eq!(notice.text, "✔ saved keep files: compact copies");
        let file = written(&store).expect("the file is written");
        assert!(file.contains("keep_files = \"compact\""), "{file}");
        assert!(file.starts_with("# asqr settings"), "{file}");
        assert_eq!(held.effective().keep_files, KeepFiles::Compact);
    }

    #[test]
    fn the_value_picked_is_the_one_written_and_held() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let store = store_in(&scratch);
        let events = vec![
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Char(' ')),
            key(KeyCode::Down),
            key(KeyCode::Enter),
        ];

        let (ended, _) = ask(&store, events);

        let (held, notice) = answered(ended);
        assert_eq!(notice.text, "✔ saved keep files: no copies");
        let file = written(&store).expect("written");
        assert!(file.contains("keep_files = \"off\""), "{file}");
        assert_eq!(held.effective().keep_files, KeepFiles::Off);
        assert_eq!(
            store.effective().keep_files,
            KeepFiles::Compact,
            "the store asked with is left as it was"
        );
    }

    #[test]
    fn q_and_a_signal_write_nothing() {
        for ending in [key(KeyCode::Char('q')), Event::Quit] {
            let scratch = tempfile::tempdir().expect("temp dir");
            let store = store_in(&scratch);

            let (ended, _) = ask(&store, vec![ending.clone(), key(KeyCode::Enter)]);

            assert_eq!(ended.expect("ends"), FirstStartEnd::Quit, "{ending:?}");
            assert!(!store.path().expect("a path").exists(), "{ending:?}");
        }
    }

    #[test]
    fn running_out_of_events_is_a_quit_and_writes_nothing() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let store = store_in(&scratch);

        let (ended, screen) = ask(&store, vec![key(KeyCode::Down)]);

        assert_eq!(ended.expect("ends"), FirstStartEnd::Quit);
        assert!(!store.path().expect("a path").exists());
        assert!(screen.contains("╭ First start "), "{screen}");
    }

    #[test]
    fn the_end_of_the_input_is_an_error() {
        let scratch = tempfile::tempdir().expect("temp dir");

        let (ended, _) = ask(
            &store_in(&scratch),
            vec![Event::InputEnded("the terminal is gone".into())],
        );

        assert!(ended.is_err());
    }

    #[test]
    fn released_keys_and_the_other_events_do_nothing() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let store = store_in(&scratch);
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Enter,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        let events = vec![
            release,
            Event::Resize,
            Event::InboxChanged,
            Event::Loaded,
            Event::Paste("x".into()),
        ];

        let (ended, _) = ask(&store, events);

        assert_eq!(
            ended.expect("ends"),
            FirstStartEnd::Quit,
            "the events ran out"
        );
        assert!(!store.path().expect("a path").exists());
    }

    #[test]
    fn a_file_that_cannot_be_written_is_said_in_the_panes_words_and_the_choice_holds() {
        let scratch = tempfile::tempdir().expect("temp dir");
        // A table where `keep_files` belongs cannot be saved into.
        let path = scratch.path().join("settings.toml");
        std::fs::write(&path, "[keep_files]\na = 1\n").expect("written");
        let store = SettingsStore::load(Some(path.clone()), Locks::default()).store;

        let (ended, _) = ask(
            &store,
            vec![key(KeyCode::Down), key(KeyCode::Enter), key(KeyCode::Enter)],
        );

        let (held, notice) = answered(ended);
        assert_eq!(notice.tone, Tone::Error);
        assert_eq!(
            notice.text,
            "cannot save the setting: keep_files in settings.toml is a table, so the file is left as it is; the choice holds until asqr quits"
        );
        assert_eq!(held.effective().keep_files, KeepFiles::Exact);
        assert_eq!(
            std::fs::read_to_string(&path).expect("still there"),
            "[keep_files]\na = 1\n"
        );
    }
}
