// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The state across all waiting sessions (spec section 7.5): which one is
//! being answered, the session list behind `L`, the intro overlay behind
//! `i`, and notices about the queue. Sessions are kept in the order they are added, which the running
//! app keeps in queue order.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::scroll::scroll;
use super::{Effect, Extent, Offset, Page, SessionState};
use crate::format::{Answer, SessionResult, same_session_id};

/// How a finished session ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// With the person's note to the whole session, if they wrote one.
    Submitted {
        answers: Vec<Answer>,
        note: Option<String>,
    },
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
    /// The first line of the active session's intro shown in the open
    /// intro overlay.
    intro: Option<usize>,
    /// The intro's lines the overlay showed in the last draw, which is how
    /// far its keys can scroll it.
    intro_page: Extent,
    notice: Option<String>,
}

impl App {
    /// Adds a session at the end of the queue. A session whose id is known
    /// already is a replaced file: it takes the place of the open one and
    /// keeps the answers that still fit.
    ///
    /// When the active session's new file has no intro, its open intro
    /// overlay closes, as there is nothing left to show and it would still
    /// take every key. A shorter intro needs nothing here: the draw shows
    /// it from within its length, and the next key scrolls from there.
    pub fn add(&mut self, state: SessionState) {
        match self.position(state.id()) {
            Some(index) => {
                self.sessions[index].replace_session(state.session().clone());
                if index == self.active && state.session().intro.is_none() {
                    self.intro = None;
                }
            }
            None => self.sessions.push(state),
        }
    }

    /// Removes session `id`. The session after it takes over when it was
    /// the active one, or the one before when it was the last. An open
    /// session list keeps its cursor on a session, and closes when none is
    /// left.
    pub fn remove(&mut self, id: &str) {
        let Some(index) = self.position(id) else {
            return;
        };
        if index == self.active {
            self.intro = None;
        }
        self.sessions.remove(index);
        if index < self.active || self.active >= self.sessions.len() {
            self.active = self.active.saturating_sub(1);
        }
        let last = self.sessions.len().checked_sub(1);
        self.list = self.list.zip(last).map(|(cursor, last)| cursor.min(last));
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

    /// The first line of the intro shown while its overlay is open.
    pub fn intro_scroll(&self) -> Option<usize> {
        self.intro
    }

    /// Takes what the last draw of the intro overlay showed, which depends
    /// on the size of the terminal.
    pub fn set_intro_page(&mut self, page: Extent) {
        self.intro_page = page;
    }

    /// Hands the active session what its context pane showed in the last
    /// draw, which is how far the pane's keys can scroll it.
    pub fn set_pane_page(&mut self, page: Option<Page>) {
        if let Some(state) = self.sessions.get_mut(self.active) {
            state.set_pane_page(page);
        }
    }

    /// A key while the intro overlay is open. It reads like a pager, so
    /// only `esc`, `q` and `i` close it and other keys do nothing: a key
    /// meant to scroll never closes it by accident.
    fn intro_key(&mut self, key: KeyEvent, top: usize) -> AppEffect {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if control => return AppEffect::Quit,
            KeyCode::Esc | KeyCode::Char('q' | 'i') if !control => {
                self.intro = None;
                return AppEffect::None;
            }
            _ => {}
        }
        let page = Page {
            rows: self.intro_page,
            columns: Extent::default(),
        };
        if let Some(offset) = scroll(key, Offset { top, left: 0 }, page, true) {
            self.intro = Some(offset.top);
        }
        AppEffect::None
    }

    pub fn handle(&mut self, key: KeyEvent) -> AppEffect {
        self.notice = None;
        if let Some(scroll) = self.intro {
            return self.intro_key(key, scroll);
        }
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
                    // `remove` keeps the cursor in range. An index past
                    // the end would leave the person on "Nothing to
                    // answer" while sessions wait, so it is bounded here
                    // too.
                    self.active = cursor.min(self.sessions.len().saturating_sub(1));
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
        let effect = state.handle(key);
        self.apply(effect)
    }

    /// Text the person pasted. It goes into the active session's focused
    /// field or note, and nowhere while the intro or the session list
    /// shows, as they take every key.
    pub fn paste(&mut self, text: &str) -> AppEffect {
        self.notice = None;
        if self.intro.is_some() || self.list.is_some() {
            return AppEffect::None;
        }
        let Some(state) = self.sessions.get_mut(self.active) else {
            return AppEffect::None;
        };
        let effect = state.paste(text);
        self.apply(effect)
    }

    /// What the active session's `effect` asks of the app.
    fn apply(&mut self, effect: Effect) -> AppEffect {
        let state = &self.sessions[self.active];
        match effect {
            Effect::None => AppEffect::None,
            Effect::DraftChanged => AppEffect::SaveDraft(state.to_draft()),
            Effect::Submit { answers, note } => AppEffect::Finish {
                id: state.id().to_owned(),
                outcome: Outcome::Submitted { answers, note },
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
            Effect::ShowIntro => {
                self.intro = Some(0);
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
            r#"{{"asqr": 2, "questions": [{{"id": "{question}", "text": "?", "kind": "single",
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

        // Picking moves on to the review, which opens on Submit; the session
        // note and Reject are the rows below.
        let AppEffect::SaveDraft(draft) = app.handle(key('1')) else {
            panic!("picking saves the draft");
        };
        assert_eq!(draft.id, "one");

        let AppEffect::Finish { id, outcome } = app.handle(code(KeyCode::Enter)) else {
            panic!("enter on Submit finishes the session");
        };
        assert_eq!(id, "one");
        assert!(
            matches!(outcome, Outcome::Submitted { answers, note: None } if answers.len() == 1)
        );

        app.handle(key('j'));
        app.handle(key('k'));
        assert_eq!(
            app.handle(code(KeyCode::Enter)),
            AppEffect::Finish {
                id: "one".into(),
                outcome: Outcome::Submitted {
                    answers: vec![Answer {
                        selected: vec!["a".into()],
                        ..Answer::new("q")
                    }],
                    note: Some("k".into())
                }
            },
            "the session note took the k"
        );

        app.handle(code(KeyCode::Down));
        assert_eq!(
            app.handle(code(KeyCode::Enter)),
            AppEffect::Finish {
                id: "one".into(),
                outcome: Outcome::Rejected(None)
            }
        );
        app.handle(code(KeyCode::Esc));
        assert_eq!(app.handle(key('q')), AppEffect::Quit);
    }

    #[test]
    fn opening_an_image_passes_through() {
        let mut app = App::default();
        let with_image: Session = serde_json::from_str(
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single", "context": [{"type": "image", "path": "/i.png"}],
                "options": [{"id": "a", "label": "A"}]}]}"#,
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

    /// An app with one session that has an intro, its overlay open, after
    /// a draw that showed `rows` of its `lines`.
    fn intro_open(rows: usize, lines: usize) -> App {
        let mut app = App::default();
        let session: Session = serde_json::from_str(
            r#"{"asqr": 2, "intro": "A long intro.", "questions": [{"id": "q", "text": "?", "kind": "single",
                "options": [{"id": "a", "label": "A"}]}]}"#,
        )
        .expect("parses");
        app.add(SessionState::new("intro", session, None));
        assert_eq!(app.handle(key('i')), AppEffect::None);
        app.set_intro_page(Extent {
            shown: rows,
            total: lines,
        });
        app
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn i_opens_the_intro_at_its_start() {
        let app = intro_open(10, 40);
        assert_eq!(app.intro_scroll(), Some(0));
    }

    #[test]
    fn the_intro_scrolls_by_line_and_page_within_its_ends() {
        let mut app = intro_open(10, 40);
        let mut scrolled = |event: KeyEvent| {
            assert_eq!(app.handle(event), AppEffect::None);
            app.intro_scroll().expect("still open")
        };

        assert_eq!(scrolled(key('j')), 1);
        assert_eq!(scrolled(code(KeyCode::Down)), 2);
        assert_eq!(scrolled(key('k')), 1);
        assert_eq!(scrolled(code(KeyCode::Up)), 0);
        assert_eq!(scrolled(key('k')), 0, "stops at the start");
        assert_eq!(scrolled(code(KeyCode::PageDown)), 10);
        assert_eq!(scrolled(ctrl('d')), 20);
        assert_eq!(
            scrolled(ctrl('d')),
            30,
            "the last page ends with the last line"
        );
        assert_eq!(scrolled(key('j')), 30, "stops at the end");
        assert_eq!(scrolled(code(KeyCode::PageUp)), 20);
        assert_eq!(scrolled(ctrl('u')), 10);
        assert_eq!(scrolled(key('g')), 0);
        assert_eq!(scrolled(key('G')), 30);
        assert_eq!(scrolled(code(KeyCode::Home)), 0);
        assert_eq!(scrolled(code(KeyCode::End)), 30);
    }

    #[test]
    fn an_intro_that_fits_does_not_scroll() {
        let mut app = intro_open(10, 4);
        for event in [key('j'), key('G'), code(KeyCode::PageDown)] {
            app.handle(event);
            assert_eq!(app.intro_scroll(), Some(0));
        }
    }

    #[test]
    fn esc_q_and_i_close_the_intro_and_other_keys_do_nothing() {
        for close in [code(KeyCode::Esc), key('q'), key('i')] {
            let mut app = intro_open(10, 40);
            assert_eq!(
                app.handle(close),
                AppEffect::None,
                "{close:?} does not quit"
            );
            assert_eq!(app.intro_scroll(), None, "{close:?} closes it");
        }

        let mut app = intro_open(10, 40);
        for ignored in [
            key('1'),
            key('L'),
            code(KeyCode::Enter),
            key('?'),
            ctrl('j'),
        ] {
            assert_eq!(app.handle(ignored), AppEffect::None);
            assert_eq!(app.intro_scroll(), Some(0), "{ignored:?} is ignored");
        }
        assert_eq!(
            app.active().expect("active").answers()[0].selected,
            Vec::<String>::new(),
            "nothing reached the question"
        );
        assert_eq!(app.handle(ctrl('c')), AppEffect::Quit, "ctrl-c still quits");
    }

    #[test]
    fn the_intro_closes_when_its_session_leaves() {
        let mut app = intro_open(10, 40);
        app.remove("intro");
        assert_eq!(app.intro_scroll(), None);
    }

    #[test]
    fn the_intro_closes_when_a_replaced_file_drops_it() {
        let mut app = intro_open(10, 40);

        app.add(SessionState::new("intro", session("q"), None));

        assert_eq!(app.intro_scroll(), None);
        assert_eq!(
            app.handle(key('1')),
            AppEffect::SaveDraft(app.active().expect("active").to_draft()),
            "keys reach the question again"
        );
    }

    #[test]
    fn a_replaced_intro_scrolls_from_within_its_new_length() {
        let mut app = intro_open(10, 40);
        app.handle(key('G'));
        assert_eq!(app.intro_scroll(), Some(30));
        let shorter: Session = serde_json::from_str(
            r#"{"asqr": 2, "intro": "Short now.", "questions": [{"id": "q", "text": "?", "kind": "single",
                "options": [{"id": "a", "label": "A"}]}]}"#,
        )
        .expect("parses");

        app.add(SessionState::new("intro", shorter, None));
        // The draw clamps the overlay to what the new intro has, and keys
        // move from there.
        app.set_intro_page(Extent {
            shown: 10,
            total: 4,
        });
        app.handle(key('k'));

        assert_eq!(app.intro_scroll(), Some(0));
    }

    #[test]
    fn removing_sessions_keeps_the_list_cursor_on_one_and_closes_an_empty_list() {
        let mut app = app(&["first", "second", "third"]);
        app.handle(key('L'));
        app.handle(key('j'));
        app.handle(key('j'));

        app.remove("third");
        assert_eq!(app.list_cursor(), Some(1));
        app.handle(code(KeyCode::Enter));
        assert_eq!(active_id(&app), Some("second"));

        app.handle(key('L'));
        app.remove("first");
        assert_eq!(app.list_cursor(), Some(0));
        app.remove("second");
        assert_eq!(app.list_cursor(), None, "nothing left to list");
        assert_eq!(app.handle(key('q')), AppEffect::Quit);
    }

    #[test]
    fn the_pane_scrolls_as_far_as_the_last_draw_showed() {
        let mut app = App::default();
        app.set_pane_page(None);
        let with_text: Session = serde_json::from_str(
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
                "context": [{"type": "code", "content": "long"}], "options": [{"id": "a", "label": "A"}]}]}"#,
        )
        .expect("parses");
        app.add(SessionState::new("text", with_text, None));
        let page = Page {
            rows: Extent {
                shown: 10,
                total: 15,
            },
            columns: Extent::default(),
        };

        app.set_pane_page(Some(page));
        app.handle(code(KeyCode::PageDown));

        let state = app.active().expect("active");
        assert_eq!(state.pane_offset().top, 5, "the last line is at the bottom");
    }

    #[test]
    fn a_paste_reaches_the_field_but_not_the_intro_or_the_list() {
        assert_eq!(App::default().paste("x"), AppEffect::None, "no session");

        let mut app = App::default();
        let text: Session = serde_json::from_str(
            r#"{"asqr": 2, "intro": "Hello.", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
        )
        .expect("parses");
        app.add(SessionState::new("text", text, None));

        let AppEffect::SaveDraft(draft) = app.paste("ab") else {
            panic!("a paste into the answer saves the draft");
        };
        assert_eq!(draft.answers[0].custom.as_deref(), Some("ab"));

        app.handle(code(KeyCode::Esc));
        app.handle(key('i'));
        assert_eq!(app.paste("c"), AppEffect::None, "the intro has the keys");
        assert_eq!(app.intro_scroll(), Some(0), "and stays open");
        app.handle(code(KeyCode::Esc));
        app.handle(key('L'));
        assert_eq!(app.paste("c"), AppEffect::None, "the list has the keys");
        assert_eq!(app.list_cursor(), Some(0), "and stays open");
        app.handle(code(KeyCode::Esc));

        app.handle(code(KeyCode::Down));
        assert!(matches!(app.paste("c"), AppEffect::SaveDraft(_)));
        let answers = app.active().expect("active").answers();
        assert_eq!(answers[0].custom.as_deref(), Some("abc"));
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
