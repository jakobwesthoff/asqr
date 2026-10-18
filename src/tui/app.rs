// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The state across all waiting sessions (spec section 7.9): which one is
//! being answered, the session list behind `L`, the intro overlay behind
//! `i`, and notices about the queue. Sessions are kept in the order they are added, which the running
//! app keeps in queue order.

use std::collections::VecDeque;
use std::ops::Range;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::scroll::scroll;
use super::{Effect, Extent, FieldShape, Offset, Page, PanePage, SessionState};
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

/// What shows over the screen, at most one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// The active session's keys.
    Help,
    /// The waiting sessions, with the cursor on this one.
    SessionList(usize),
    /// The active session's whole intro, from this line on.
    Intro(usize),
}

/// What kind of news a notice or a session's message is, which decides
/// its colour in the status line (ADR 38).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Plain news, such as a session file that was reloaded.
    Info,
    /// asqr refused an action or set something aside on purpose, such as
    /// a refused key or an invalid session answered with an error.
    Warning,
    /// An operation failed and nothing was done, such as a draft that
    /// could not be saved.
    Error,
    /// A session was sent.
    Success,
}

/// A line for the status line: an app's notice or a session's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub tone: Tone,
    pub text: String,
    /// Where `text` holds a session's name, which has no bound on its
    /// length: the status line cuts the name to the room its other parts
    /// leave, so they show whole (ADR 38).
    pub name: Option<Range<usize>>,
}

impl Notice {
    pub fn new(tone: Tone, text: impl Into<String>) -> Self {
        Notice {
            tone,
            text: text.into(),
            name: None,
        }
    }

    /// A notice that reads `before`, `name` and `after` in a row and keeps
    /// where the name stands.
    pub fn naming(tone: Tone, before: &str, name: &str, after: &str) -> Self {
        let start = before.len();
        Notice {
            tone,
            text: format!("{before}{name}{after}"),
            name: Some(start..start + name.len()),
        }
    }
}

/// A session asqr answered with an error result because its file is
/// invalid, which a dialog shows with every error (spec section 7.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub id: String,
    /// Each problem the file has, apart, as validation found them.
    pub errors: Vec<String>,
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
    notice: Option<Notice>,
    /// The invalid sessions whose dialogs are still to be closed, the one
    /// shown first.
    invalid: VecDeque<Invalid>,
    /// Whether the last frame showed the dialog in front. False until a
    /// frame does, as before the first or under the message of a terminal
    /// too small for asqr.
    dialog_seen: bool,
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

    /// Gives session `id` the title of the session its `follows` names.
    pub fn set_follows_title(&mut self, id: &str, title: Option<String>) {
        if let Some(index) = self.position(id) {
            self.sessions[index].set_follows_title(title);
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

    /// Shows `text` as `tone` until the next key or paste, such as a
    /// session archived because it collided with an unread result (spec
    /// section 5.7).
    pub fn notice(&mut self, tone: Tone, text: impl Into<String>) {
        self.show(Notice::new(tone, text));
    }

    /// Shows `notice` until the next key or paste, as [`App::notice`]
    /// does, for a notice that names a session.
    pub fn show(&mut self, notice: Notice) {
        self.notice = Some(notice);
    }

    pub fn current_notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// Shows the dialog of `invalid` after those still open, so a scan
    /// that finds several invalid files shows each of them in turn.
    pub fn report_invalid(&mut self, invalid: Invalid) {
        self.invalid.push_back(invalid);
    }

    /// Takes whether the last frame showed the dialog of an invalid
    /// session, which decides whether the next key closes it.
    pub fn set_dialog_shown(&mut self, shown: bool) {
        self.dialog_seen = shown;
    }

    /// Closes the dialog in front if the last frame showed it, and says
    /// whether it did. The next dialog waits for a frame of its own.
    fn close_seen_dialog(&mut self) -> bool {
        if self.dialog_seen && self.invalid.pop_front().is_some() {
            self.dialog_seen = false;
            return true;
        }
        false
    }

    /// The invalid session whose dialog shows.
    pub fn invalid_shown(&self) -> Option<&Invalid> {
        self.invalid.front()
    }

    /// The first line of the intro shown while its overlay is open.
    pub fn intro_scroll(&self) -> Option<usize> {
        self.intro
    }

    /// The overlay the screen shows, `None` with an empty queue. Keys open
    /// one at a time, since each takes the key that would open another; were
    /// several open, the one shown is the one [`App::handle`] gives the keys
    /// to first. The intro shows only for a session that has one, also an
    /// empty one; [`App::add`] closes it when a replaced file drops it.
    pub fn overlay(&self) -> Option<Overlay> {
        let state = self.active()?;
        if let Some(scroll) = self.intro
            && state.session().intro.is_some()
        {
            return Some(Overlay::Intro(scroll));
        }
        if let Some(cursor) = self.list {
            return Some(Overlay::SessionList(cursor));
        }
        state.help().then_some(Overlay::Help)
    }

    /// Takes what the last draw of the intro overlay showed, which depends
    /// on the size of the terminal.
    pub fn set_intro_page(&mut self, page: Extent) {
        self.intro_page = page;
    }

    /// Hands the active session what its context pane showed in the last
    /// draw, which is how far the pane's keys can scroll it.
    pub fn set_pane_page(&mut self, page: Option<PanePage>) {
        if let Some(state) = self.sessions.get_mut(self.active) {
            state.set_pane_page(page);
        }
    }

    /// Hands the active session where its field or note that has the keys
    /// was drawn, which `↑`/`↓` move through, and says whether that moved
    /// the text cursor, so the screen shows it where it went. A draw
    /// without one, as under an overlay, leaves the last one.
    pub fn set_field_shape(&mut self, shape: Option<FieldShape>) -> bool {
        match (self.sessions.get_mut(self.active), shape) {
            (Some(state), Some(shape)) => state.set_field_shape(shape),
            _ => false,
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
        // The dialog of an invalid session closes on any key, `ctrl-c`
        // included, as the help does, and the key does nothing else: it
        // stands over the screen, so the person cannot see what the key
        // would act on. A dialog no frame showed, as under the message of
        // a terminal too small for asqr, is not what the person sees, so
        // the key acts as without it.
        if self.close_seen_dialog() {
            return AppEffect::None;
        }
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

    /// Text the person pasted, which the active session places (see
    /// [`SessionState::paste`]). While the intro or the session list shows
    /// it goes nowhere, as they take every key.
    pub fn paste(&mut self, text: &str) -> AppEffect {
        self.notice = None;
        // As over the help, a paste closes the dialog the person saw and
        // goes nowhere.
        if self.close_seen_dialog() {
            return AppEffect::None;
        }
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
    use crate::graphics::{Graphics, Images, Protocol};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::format::Session;
    use crate::tui::render::{PaneLayout, View, draw};

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

        app.set_pane_page(Some(PanePage::shown(page)));
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
        let message = |app: &App| app.active().and_then(SessionState::message).cloned();
        assert_eq!(message(&app), None, "silently");
        app.handle(code(KeyCode::Esc));
        app.handle(key('L'));
        assert_eq!(app.paste("c"), AppEffect::None, "the list has the keys");
        assert_eq!(app.list_cursor(), Some(0), "and stays open");
        assert_eq!(message(&app), None, "silently");
        app.handle(code(KeyCode::Esc));

        app.handle(code(KeyCode::Down));
        assert!(matches!(app.paste("c"), AppEffect::SaveDraft(_)));
        let answers = app.active().expect("active").answers();
        assert_eq!(answers[0].custom.as_deref(), Some("abc"));
    }

    #[test]
    fn a_notice_naming_a_session_keeps_where_the_name_stands() {
        let notice = Notice::naming(Tone::Info, "✗ rejected \"", "Release 0.2.0", "\"");

        assert_eq!(notice.text, "✗ rejected \"Release 0.2.0\"");
        let name = notice.name.clone().expect("a name");
        assert_eq!(&notice.text[name], "Release 0.2.0");
    }

    #[test]
    fn a_notice_lasts_until_the_next_key() {
        let mut app = app(&["one"]);

        let text = "batch-01 collided with an unread result and was archived";
        app.notice(Tone::Warning, text);
        assert_eq!(
            app.current_notice(),
            Some(&Notice {
                tone: Tone::Warning,
                text: text.to_owned(),
                name: None,
            })
        );

        app.handle(key('j'));
        assert_eq!(app.current_notice(), None);
    }

    // -----------------------------------------------------------------
    // Which overlay shows
    // -----------------------------------------------------------------

    /// One session with an intro and an image entry, with the help, the
    /// session list and the intro open as the flags say. Keys open at most
    /// one of them, since each takes the key that would open another, so
    /// the tests set the fields to pin which one shows when several are set.
    fn overlays(help: bool, list: bool, intro: bool) -> App {
        let session: Session = serde_json::from_str(
            r#"{"asqr": 2, "intro": "Read this first.", "questions": [{"id": "q", "text": "?",
                "kind": "single", "context": [{"type": "image", "path": "/nowhere/cat.png"}],
                "options": [{"id": "a", "label": "A"}]}]}"#,
        )
        .expect("parses");
        let mut state = SessionState::new("over", session, None);
        if help {
            state.handle(key('?'));
            assert!(state.help(), "? opens the help");
        }
        let mut app = App::default();
        app.add(state);
        app.list = list.then_some(0);
        app.intro = intro.then_some(0);
        app
    }

    /// The rows of `app` drawn at 80×24 with `view`, which remembers the
    /// draw for [`View::must_clear`].
    fn drawn(app: &App, view: &mut View) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        terminal
            .draw(|frame| {
                draw(frame, app, view);
            })
            .expect("draws");
        let buf = terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    /// Draws `app` at 80×24 with `view` and hands the app whether the
    /// frame showed the dialog of an invalid session, as the run loop does.
    fn drawn_and_handed_on(app: &mut App, view: &mut View) {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut shown = false;
        terminal
            .draw(|frame| shown = draw(frame, app, view).invalid_dialog)
            .expect("draws");
        app.set_dialog_shown(shown);
    }

    /// A view under the Sixel protocol, whose image cells a closed overlay
    /// leaves to be cleared.
    fn view() -> View<'static> {
        let sixel = Graphics {
            protocol: Protocol::Sixel,
            ..Graphics::HALF_BLOCKS
        };
        View::for_tests("default", PaneLayout::Auto, Images::new(sixel))
    }

    /// Whether the screen must be cleared once every overlay of `app` is
    /// closed, after a draw with them open over its image.
    fn clears_after_closing(mut app: App) -> bool {
        let mut view = view();
        drawn(&app, &mut view);
        assert!(!view.must_clear(&app), "nothing to clear while it is open");
        app.intro = None;
        app.list = None;
        if app.sessions[0].help() {
            app.sessions[0].handle(key('x'));
        }
        view.must_clear(&app)
    }

    #[test]
    fn the_intro_wins_over_the_other_overlays_as_it_takes_the_keys_first() {
        for (help, list) in [(true, false), (false, true), (true, true)] {
            let app = overlays(help, list, true);
            assert_eq!(app.overlay(), Some(Overlay::Intro(0)));
            let screen = drawn(&app, &mut view());

            assert!(
                screen[23].contains(" esc/q/i  close"),
                "help {help}, list {list}: the key bar is the intro's: {}",
                screen[23]
            );
            assert!(
                !screen
                    .iter()
                    .any(|row| row.contains(" Keys ") || row.contains(" Waiting sessions ")),
                "help {help}, list {list}: only the intro shows: {screen:#?}"
            );
            assert!(clears_after_closing(app));
        }
    }

    #[test]
    fn the_session_list_wins_over_the_help() {
        let app = overlays(true, true, false);
        assert_eq!(app.overlay(), Some(Overlay::SessionList(0)));
        let screen = drawn(&app, &mut view());

        let list_only = drawn(&overlays(false, true, false), &mut view());
        assert!(
            screen.iter().any(|row| row.contains(" Waiting sessions ")),
            "{screen:#?}"
        );
        assert_eq!(screen[23], list_only[23], "the key bar is the list's");
        assert_eq!(screen, list_only, "and the help does not show");
        assert!(clears_after_closing(app));
    }

    #[test]
    fn each_overlay_shows_alone_and_none_without_a_flag() {
        for (help, list, intro, title, overlay) in [
            (true, false, false, " Keys ", Overlay::Help),
            (
                false,
                true,
                false,
                " Waiting sessions ",
                Overlay::SessionList(0),
            ),
            (false, false, true, " Intro ", Overlay::Intro(0)),
        ] {
            let app = overlays(help, list, intro);
            assert_eq!(app.overlay(), Some(overlay));
            let screen = drawn(&app, &mut view());

            assert!(
                screen.iter().any(|row| row.contains(title)),
                "{title}: {screen:#?}"
            );
            assert!(clears_after_closing(app), "{title}");
        }

        let mut view = view();
        let app = overlays(false, false, false);
        assert_eq!(app.overlay(), None);
        drawn(&app, &mut view);
        assert!(!view.must_clear(&app), "no overlay, nothing to clear");
    }

    #[test]
    fn an_intro_flag_without_an_intro_shows_no_overlay() {
        let mut app = overlays(true, false, false);
        let mut view = view();
        drawn(&app, &mut view);
        app.sessions[0].handle(key('x'));
        app.add(SessionState::new("over", session("q"), None));
        app.intro = Some(0);

        assert_eq!(app.overlay(), None);
        assert!(view.must_clear(&app), "the help over the image is gone");
        let screen = drawn(&app, &mut view);
        assert!(!screen.iter().any(|row| row.contains(" Intro ")));
    }

    // -----------------------------------------------------------------
    // The dialog of an invalid session
    // -----------------------------------------------------------------

    fn invalid(id: &str) -> Invalid {
        Invalid {
            id: id.to_owned(),
            errors: vec![format!("questions[0].id: duplicate id in {id}")],
        }
    }

    /// `key` on the dialog in front, after a frame showed it, as the run
    /// loop hands on before every key.
    fn press_on_dialog(app: &mut App, key: KeyEvent) -> AppEffect {
        app.set_dialog_shown(true);
        app.handle(key)
    }

    #[test]
    fn an_invalid_session_closes_on_the_next_key_which_does_nothing_else() {
        let mut app = app(&["one"]);
        app.report_invalid(invalid("broken"));
        assert_eq!(app.invalid_shown(), Some(&invalid("broken")));

        assert_eq!(
            press_on_dialog(&mut app, key('q')),
            AppEffect::None,
            "q only closes it"
        );
        assert_eq!(app.invalid_shown(), None);
        assert_eq!(active_id(&app), Some("one"));
        assert_eq!(app.handle(key('q')), AppEffect::Quit);
    }

    #[test]
    fn ctrl_c_and_esc_close_the_dialog_like_any_key() {
        let mut app = app(&["one"]);
        for close in [
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            code(KeyCode::Esc),
            code(KeyCode::Enter),
        ] {
            app.report_invalid(invalid("broken"));
            assert_eq!(
                press_on_dialog(&mut app, close),
                AppEffect::None,
                "{close:?}"
            );
            assert_eq!(app.invalid_shown(), None, "{close:?}");
        }
        assert_eq!(
            app.active().expect("active").answers()[0].selected,
            Vec::<String>::new(),
            "enter picked nothing"
        );
    }

    #[test]
    fn a_paste_closes_the_dialog_and_goes_nowhere() {
        let session: Session = serde_json::from_str(
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
        )
        .expect("parses");
        let mut app = App::default();
        app.add(SessionState::new("typed", session, None));
        app.report_invalid(invalid("broken"));

        app.set_dialog_shown(true);
        assert_eq!(app.paste("pasted"), AppEffect::None);

        assert_eq!(app.invalid_shown(), None);
        assert_eq!(app.active().expect("active").answers()[0].custom, None);
    }

    #[test]
    fn invalid_sessions_show_one_after_another() {
        let mut app = app(&["one"]);
        app.report_invalid(invalid("first"));
        app.report_invalid(invalid("second"));

        assert_eq!(app.invalid_shown(), Some(&invalid("first")));
        press_on_dialog(&mut app, key('x'));
        assert_eq!(app.invalid_shown(), Some(&invalid("second")));
        press_on_dialog(&mut app, key('x'));
        assert_eq!(app.invalid_shown(), None);
    }

    #[test]
    fn the_dialog_shows_over_the_empty_queue() {
        let mut app = App::default();
        app.report_invalid(invalid("broken"));

        assert_eq!(press_on_dialog(&mut app, key('q')), AppEffect::None);
        assert_eq!(app.handle(key('q')), AppEffect::Quit);
    }

    /// Before the first frame, no dialog has shown, so `ctrl-c` quits and
    /// the dialog waits for the frame that never comes.
    #[test]
    fn a_dialog_no_frame_showed_takes_no_key() {
        let mut app = App::default();
        app.report_invalid(invalid("broken"));

        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle(ctrl_c), AppEffect::Quit);
        assert_eq!(app.invalid_shown(), Some(&invalid("broken")));
    }

    /// A frame too small for the dialog shows the message that says so;
    /// the person sees the session's keys at work, not the dialog.
    #[test]
    fn a_dialog_a_small_frame_hid_lets_the_key_act() {
        let mut app = app(&["one"]);
        app.report_invalid(invalid("broken"));
        app.set_dialog_shown(true);
        app.set_dialog_shown(false);

        assert_eq!(app.handle(key('L')), AppEffect::None);

        assert_eq!(app.list_cursor(), Some(0), "L opened the session list");
        assert_eq!(app.invalid_shown(), Some(&invalid("broken")));
    }

    #[test]
    fn a_paste_under_a_dialog_no_frame_showed_reaches_the_field() {
        let session: Session = serde_json::from_str(
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
        )
        .expect("parses");
        let mut app = App::default();
        app.add(SessionState::new("typed", session, None));
        app.report_invalid(invalid("broken"));

        assert!(matches!(app.paste("pasted"), AppEffect::SaveDraft(_)));
        assert_eq!(app.invalid_shown(), Some(&invalid("broken")));
    }

    #[test]
    fn the_screen_is_cleared_after_a_dialog_over_a_sixel_image() {
        let mut app = overlays(false, false, false);
        app.report_invalid(invalid("broken"));
        let mut view = view();
        drawn_and_handed_on(&mut app, &mut view);
        assert!(!view.must_clear(&app), "nothing to clear while it is open");

        app.handle(key('x'));

        assert!(view.must_clear(&app));
    }

    #[test]
    fn an_empty_queue_shows_no_overlay() {
        let app = App {
            list: Some(0),
            intro: Some(0),
            ..App::default()
        };

        assert_eq!(app.overlay(), None);
        let screen = drawn(&app, &mut view());

        assert!(
            !screen
                .iter()
                .any(|row| row.contains(" Waiting sessions ") || row.contains(" Intro ")),
            "{screen:#?}"
        );
    }
}
