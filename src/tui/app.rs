// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The state across all waiting sessions (spec section 7.5): which one is
//! being answered, the session list behind `L`, and notices about the
//! queue. Sessions are kept in the order they are added, which the running
//! app keeps in queue order.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Effect, SessionState};
use crate::format::{Answer, SessionResult, same_session_id};

/// How a finished session ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Submitted(Vec<Answer>),
    Rejected(Option<String>),
}

/// What the running app has to do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEffect {
    None,
    SaveDraft(SessionResult),
    /// Finish session `id`; the running app removes it afterwards.
    Finish {
        id: String,
        outcome: Outcome,
    },
    Quit,
    OpenImage(String),
}

#[derive(Debug, Default)]
pub struct App {
    sessions: Vec<SessionState>,
    active: usize,
    /// The cursor of the open session list.
    list: Option<usize>,
    notice: Option<String>,
}

impl App {
    /// Adds a session at the end of the queue. A session whose id is known
    /// already is a replaced file: it takes the place of the open one and
    /// keeps the answers that still fit.
    pub fn add(&mut self, state: SessionState) {
        match self.position(state.id()) {
            Some(index) => self.sessions[index].replace_session(state.session().clone()),
            None => self.sessions.push(state),
        }
    }

    /// Removes session `id`. The session after it takes over when it was
    /// the active one, or the one before when it was the last.
    pub fn remove(&mut self, id: &str) {
        let Some(index) = self.position(id) else {
            return;
        };
        self.sessions.remove(index);
        if index < self.active || self.active >= self.sessions.len() {
            self.active = self.active.saturating_sub(1);
        }
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.sessions
            .iter()
            .position(|state| same_session_id(state.id(), id))
    }

    pub fn sessions(&self) -> &[SessionState] {
        &self.sessions
    }

    pub fn active(&self) -> Option<&SessionState> {
        self.sessions.get(self.active)
    }

    /// The highlighted entry while the session list is open.
    pub fn list_cursor(&self) -> Option<usize> {
        self.list
    }

    /// Shows `text` until the next key, such as a session archived because
    /// it collided with an unread result (spec section 3.7).
    pub fn notice(&mut self, text: impl Into<String>) {
        self.notice = Some(text.into());
    }

    pub fn current_notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn handle(&mut self, key: KeyEvent) -> AppEffect {
        self.notice = None;
        let quit = key.code == KeyCode::Char('q')
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));

        if let Some(cursor) = self.list {
            return match key.code {
                _ if quit => AppEffect::Quit,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.list = Some((cursor + 1).min(self.sessions.len().saturating_sub(1)));
                    AppEffect::None
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.list = Some(cursor.saturating_sub(1));
                    AppEffect::None
                }
                KeyCode::Enter => {
                    self.active = cursor;
                    self.list = None;
                    AppEffect::None
                }
                KeyCode::Esc | KeyCode::Char('L') => {
                    self.list = None;
                    AppEffect::None
                }
                _ => AppEffect::None,
            };
        }

        let Some(state) = self.sessions.get_mut(self.active) else {
            return if quit {
                AppEffect::Quit
            } else {
                AppEffect::None
            };
        };
        match state.handle(key) {
            Effect::None => AppEffect::None,
            Effect::DraftChanged => AppEffect::SaveDraft(state.to_draft()),
            Effect::Submit(answers) => AppEffect::Finish {
                id: state.id().to_owned(),
                outcome: Outcome::Submitted(answers),
            },
            Effect::Reject(reason) => AppEffect::Finish {
                id: state.id().to_owned(),
                outcome: Outcome::Rejected(reason),
            },
            Effect::Quit => AppEffect::Quit,
            Effect::OpenImage(path) => AppEffect::OpenImage(path),
            Effect::OpenSessionList => {
                self.list = Some(self.active);
                AppEffect::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::format::Session;

    fn session(question: &str) -> Session {
        serde_json::from_str(&format!(
            r#"{{"asqr": 1, "questions": [{{"id": "{question}", "text": "?", "kind": "single",
                "options": [{{"id": "a", "label": "A"}}]}}]}}"#
        ))
        .expect("test session parses")
    }

    fn state(id: &str) -> SessionState {
        SessionState::new(id, session("q"), None)
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn app(ids: &[&str]) -> App {
        let mut app = App::default();
        for id in ids {
            app.add(state(id));
        }
        app
    }

    fn active_id(app: &App) -> Option<&str> {
        app.active().map(SessionState::id)
    }

    #[test]
    fn answers_the_first_session_in_queue_order() {
        let app = app(&["first", "second"]);

        assert_eq!(active_id(&app), Some("first"));
        assert_eq!(app.sessions().len(), 2);
    }

    #[test]
    fn with_nothing_waiting_only_quitting_does_anything() {
        let mut app = App::default();

        assert_eq!(app.active().map(SessionState::id), None);
        assert_eq!(app.handle(key('1')), AppEffect::None);
        assert_eq!(app.handle(key('q')), AppEffect::Quit);
        assert_eq!(
            app.handle(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            AppEffect::Quit
        );
    }

    #[test]
    fn turns_session_effects_into_app_effects() {
        let mut app = app(&["one"]);

        let AppEffect::SaveDraft(draft) = app.handle(key('1')) else {
            panic!("selecting saves the draft");
        };
        assert_eq!(draft.id, "one");

        app.handle(key('S'));
        let AppEffect::Finish { id, outcome } = app.handle(code(KeyCode::Enter)) else {
            panic!("confirming finishes the session");
        };
        assert_eq!(id, "one");
        assert!(matches!(outcome, Outcome::Submitted(answers) if answers.len() == 1));

        app.handle(key('X'));
        assert_eq!(
            app.handle(code(KeyCode::Enter)),
            AppEffect::Finish {
                id: "one".into(),
                outcome: Outcome::Rejected(None)
            }
        );
        assert_eq!(app.handle(key('q')), AppEffect::Quit);
    }

    #[test]
    fn opening_an_image_passes_through() {
        let mut app = App::default();
        let with_image: Session = serde_json::from_str(
            r#"{"asqr": 1, "questions": [{"id": "q", "text": "?", "kind": "text", "image": "/i.png"}]}"#,
        )
        .expect("parses");
        app.add(SessionState::new("img", with_image, None));

        assert_eq!(app.handle(key('o')), AppEffect::OpenImage("/i.png".into()));
    }

    #[test]
    fn the_list_switches_between_sessions() {
        let mut app = app(&["first", "second", "third"]);

        assert_eq!(app.handle(key('L')), AppEffect::None);
        assert_eq!(app.list_cursor(), Some(0));
        app.handle(key('j'));
        app.handle(code(KeyCode::Down));
        app.handle(code(KeyCode::Down));
        assert_eq!(app.list_cursor(), Some(2), "stops at the last session");
        app.handle(key('k'));
        app.handle(code(KeyCode::Up));
        app.handle(code(KeyCode::Up));
        app.handle(key('j'));
        assert_eq!(app.handle(code(KeyCode::Enter)), AppEffect::None);

        assert_eq!(app.list_cursor(), None);
        assert_eq!(active_id(&app), Some("second"));
    }

    #[test]
    fn the_list_closes_without_switching() {
        let mut app = app(&["first", "second"]);

        for close in [code(KeyCode::Esc), key('L')] {
            app.handle(key('L'));
            app.handle(key('j'));
            app.handle(close);
            assert_eq!((app.list_cursor(), active_id(&app)), (None, Some("first")));
        }
        app.handle(key('L'));
        assert_eq!(
            app.handle(key('x')),
            AppEffect::None,
            "other keys do nothing in the list"
        );
        assert_eq!(
            app.handle(key('q')),
            AppEffect::Quit,
            "quitting works from the list"
        );
    }

    #[test]
    fn adding_a_known_id_replaces_that_session_keeping_answers() {
        let mut app = app(&["batch"]);
        app.handle(key('1'));

        app.add(SessionState::new("BATCH", session("q"), None));

        assert_eq!(app.sessions().len(), 1);
        let answers = app.active().expect("active").answers();
        assert_eq!(
            answers[0].selected,
            ["a"],
            "the answer still fits the new file"
        );
    }

    #[test]
    fn removing_sessions_keeps_the_active_one_where_possible() {
        let mut app = app(&["first", "second", "third"]);
        app.handle(key('L'));
        app.handle(key('j'));
        app.handle(code(KeyCode::Enter));

        app.remove("first");
        assert_eq!(active_id(&app), Some("second"));

        app.remove("SECOND");
        assert_eq!(active_id(&app), Some("third"), "the next one takes over");

        app.remove("third");
        app.remove("unknown");
        assert_eq!(active_id(&app), None);
    }

    #[test]
    fn removing_the_last_session_moves_back_to_the_one_before() {
        let mut app = app(&["first", "second"]);
        app.handle(key('L'));
        app.handle(key('j'));
        app.handle(code(KeyCode::Enter));

        app.remove("second");

        assert_eq!(active_id(&app), Some("first"));
    }

    #[test]
    fn a_notice_lasts_until_the_next_key() {
        let mut app = app(&["one"]);

        app.notice("batch-01 collided with an unread result and was archived");
        assert_eq!(
            app.current_notice(),
            Some("batch-01 collided with an unread result and was archived")
        );

        app.handle(key('j'));
        assert_eq!(app.current_notice(), None);
    }
}
