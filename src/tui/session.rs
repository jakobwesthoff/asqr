// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The state of one session while the person answers it (spec sections
//! 5.2 and 7.1 to 7.4, ADR 22), driven by key events and nothing else.
//!
//! A session is a row of tabs: one per question and the review last. Each
//! tab is a list of rows: the options and the own answer, a text answer,
//! or on the review the questions, Submit, the session note and Reject.
//! Rows that take text are live fields, typed into in place as soon as the
//! cursor lands on them. [`SessionState::handle`] returns an [`Effect`] for
//! everything the state cannot do itself: saving the draft, finishing the
//! session, quitting, opening an image, showing an overlay over all of it.

use std::collections::HashMap;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::{CursorMove, TextArea};

use super::scroll::scroll;
use super::{Offset, Page};

pub use crate::format::Length;
use crate::format::{
    Answer, ContextEntry, Kind, Question, Session, SessionResult, is_answered, result_answers,
};

/// The two ends of a field's text.
#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

/// What the running app has to do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    /// The answers or the current question changed; save the draft.
    DraftChanged,
    /// The person submitted: the result's answers, and the note to the
    /// whole session if they wrote one.
    Submit {
        answers: Vec<Answer>,
        note: Option<String>,
    },
    /// The person rejected the session, with a reason if they gave one.
    Reject(Option<String>),
    Quit,
    OpenImage(String),
    OpenSessionList,
    /// Show the session's whole intro, which the header may cut.
    ShowIntro,
}

/// One row of the current tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The option with this index.
    Option(usize),
    /// The own answer of a question with `custom`.
    Own,
    /// The answer of a `text` question.
    Answer,
    /// A question in the review, by index.
    Question(usize),
    Submit,
    /// The note to the whole session, below Submit. It is a row of its own
    /// so that Submit, where the review opens, stays a row on which keys
    /// are commands (ADR 29).
    Note,
    /// The reject row with its reason field.
    Reject,
}

impl Row {
    /// Rows that are text fields, focused when the cursor lands on them.
    fn is_field(self) -> bool {
        matches!(self, Row::Own | Row::Answer | Row::Note | Row::Reject)
    }
}

/// Which context entry a pane shows: the question's, or the one of the
/// option with this index. It names the entry across draws, so what belongs
/// to an entry, such as its viewer, outlives moving between rows and tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryKey {
    /// The question's index in the session.
    pub question: usize,
    pub option: Option<usize>,
}

/// The full-screen view of the entry shown (`z`): whether the pane had
/// the focus before, which leaving the view gives back.
#[derive(Debug, Clone, Copy)]
struct FullScreen {
    pane_focus: bool,
}

/// Where typed keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Keys are commands.
    None,
    /// The field of the current row.
    Field,
    /// The note of the current question.
    Note,
}

/// How many questions a submit would send as what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub answered: usize,
    pub skipped: usize,
    pub defaulted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerState {
    Answered,
    /// Answered by an untouched default.
    Defaulted,
    Skipped,
}

/// A question's mark in the tab bar and its line in the review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestionState {
    pub answer: AnswerState,
    pub note: bool,
}

/// The working answer to one question.
#[derive(Debug, Clone, Default)]
struct Working {
    /// Option ids, kept in option order.
    selected: Vec<String>,
    /// The own answer, or the answer of a `text` question. On a `single`
    /// question it counts only while no option is chosen (spec section
    /// 5.2).
    custom: String,
    note: String,
    /// The person edited the question; a default then no longer counts as
    /// untouched.
    touched: bool,
}

#[derive(Debug, Clone)]
pub struct SessionState {
    id: String,
    session: Session,
    working: Vec<Working>,
    /// The current tab; `session.questions.len()` is the review.
    tab: usize,
    row: usize,
    focus: Focus,
    /// The editor of the focused field or note.
    editor: Option<TextArea<'static>>,
    session_note: String,
    reject_reason: String,
    help: bool,
    message: Option<String>,
    /// The context entry shown, drawn over the question's area (`z`). It
    /// reads like a pager, so the pane has the focus while it shows.
    full_screen: Option<FullScreen>,
    /// The context pane takes the keys. Kept apart from `focus`, so the
    /// field or note being typed in has its text and cursor again when the
    /// pane gives the focus back.
    pane_focus: bool,
    /// What the context pane showed in the last draw; `None` when there was
    /// no pane, as on a terminal that only had room for a hint.
    pane_page: Option<Page>,
    /// Where the person left each entry they scrolled. Offsets of different
    /// entries have nothing to do with each other, so each entry keeps its
    /// own, and one shown for the first time starts at its top.
    offsets: HashMap<EntryKey, Offset>,
    /// See [`Self::revision`].
    revision: u64,
}

/// Whether `question` or any of its options has context.
pub fn has_context(question: &Question) -> bool {
    !question.context.is_empty()
        || question
            .options
            .iter()
            .flatten()
            .any(|option| !option.context.is_empty())
}

impl SessionState {
    /// The state of session `id`, starting from `draft` when there is one.
    /// Answers in the draft are matched by question and option id; what no
    /// longer fits the session is dropped.
    pub fn new(id: impl Into<String>, session: Session, draft: Option<&SessionResult>) -> Self {
        let working = session
            .questions
            .iter()
            .map(|question| {
                let saved = draft.and_then(|draft| {
                    draft
                        .answers
                        .iter()
                        .find(|answer| answer.question == question.id)
                });
                match saved {
                    Some(answer) => restored(question, answer),
                    None => Working {
                        selected: option_ids(question)
                            .filter(|id| is_default(question, id))
                            .map(str::to_owned)
                            .collect(),
                        ..Working::default()
                    },
                }
            })
            .collect();
        let tab = draft
            .and_then(|draft| draft.current.as_deref())
            .and_then(|current| {
                session
                    .questions
                    .iter()
                    .position(|question| question.id == current)
            })
            .unwrap_or(0);
        let mut state = SessionState {
            id: id.into(),
            session,
            working,
            tab,
            row: 0,
            focus: Focus::None,
            editor: None,
            session_note: draft
                .and_then(|draft| draft.note.clone())
                .unwrap_or_default(),
            reject_reason: draft
                .and_then(|draft| draft.reason.clone())
                .unwrap_or_default(),
            help: false,
            message: None,
            full_screen: None,
            pane_focus: false,
            pane_page: None,
            offsets: HashMap::new(),
            revision: 0,
        };
        state.arrive();
        state
    }

    /// Takes over a session file that was replaced while it was open,
    /// keeping the answers that still fit (review F1).
    pub fn replace_session(&mut self, session: Session) {
        let draft = self.to_draft();
        let revision = self.revision + 1;
        *self = SessionState::new(self.id.clone(), session, Some(&draft));
        self.revision = revision;
    }

    /// How often the session file was replaced while open, so what was
    /// measured of the old file can be told from the new one.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// The current tab: a question index, or the review after the last.
    pub fn tab(&self) -> usize {
        self.tab
    }

    pub fn tab_count(&self) -> usize {
        self.session.questions.len() + 1
    }

    pub fn on_review(&self) -> bool {
        self.tab == self.session.questions.len()
    }

    /// The row under the cursor, an index into [`Self::rows`].
    pub fn row(&self) -> usize {
        self.row
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn help(&self) -> bool {
        self.help
    }

    /// Feedback on the last key, shown until the next one.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn full_screen(&self) -> bool {
        self.full_screen.is_some()
    }

    /// Whether the context pane takes the keys.
    pub fn pane_focused(&self) -> bool {
        self.pane_focus
    }

    /// Where the entry shown starts, as far as the person scrolled it.
    pub fn pane_offset(&self) -> Offset {
        self.shown_entry()
            .and_then(|(key, _)| self.offsets.get(&key).copied())
            .unwrap_or_default()
    }

    /// Takes what the last draw of the context pane showed, which depends
    /// on the size of the terminal. Without a pane there is nothing to
    /// focus or show full screen, so the question gets the keys back.
    pub fn set_pane_page(&mut self, page: Option<Page>) {
        self.pane_page = page;
        if page.is_none() {
            self.pane_focus = false;
            self.full_screen = None;
        }
    }

    /// The note to the whole session, as typed.
    pub fn session_note(&self) -> &str {
        &self.session_note
    }

    pub fn reject_reason(&self) -> &str {
        &self.reject_reason
    }

    /// The question of the current tab; the review has none.
    pub fn question(&self) -> Option<&Question> {
        self.session.questions.get(self.tab)
    }

    /// The context entry shown on the current tab, and which one it is. The
    /// option under the cursor shows its own context, so the person can
    /// browse the options before picking; every other row shows the
    /// question's (spec section 7.7).
    pub fn shown_entry(&self) -> Option<(EntryKey, &ContextEntry)> {
        let question = self.question()?;
        let option_entry = match self.current_row() {
            Some(Row::Option(index)) => question
                .options
                .as_ref()
                .and_then(|options| options[index].context.first())
                .map(|entry| (Some(index), entry)),
            _ => None,
        };
        let (option, entry) =
            option_entry.or_else(|| question.context.first().map(|entry| (None, entry)))?;
        let key = EntryKey {
            question: self.tab,
            option,
        };
        Some((key, entry))
    }

    /// The context entry shown on the current tab; `z` shows it full
    /// screen.
    pub fn shown_context(&self) -> Option<&ContextEntry> {
        self.shown_entry().map(|(_, entry)| entry)
    }

    /// The image shown on the current tab; `o` opens it.
    pub fn shown_image(&self) -> Option<&str> {
        self.shown_context().and_then(ContextEntry::image_path)
    }

    /// Whether the current tab keeps room for context: when the question
    /// or any of its options has some, even while the row under the cursor
    /// shows none, so the layout does not jump while browsing.
    pub fn has_context_area(&self) -> bool {
        self.question().is_some_and(has_context)
    }

    /// The rows of the current tab.
    pub fn rows(&self) -> Vec<Row> {
        if self.on_review() {
            let questions = (0..self.session.questions.len()).map(Row::Question);
            return questions
                .chain([Row::Submit, Row::Note, Row::Reject])
                .collect();
        }
        self.rows_of(self.tab)
    }

    /// The rows of question `index`, whichever tab is current.
    pub fn rows_of(&self, index: usize) -> Vec<Row> {
        let question = &self.session.questions[index];
        if question.kind == Kind::Text {
            return vec![Row::Answer];
        }
        let options = (0..question.options.as_ref().map_or(0, Vec::len)).map(Row::Option);
        let own = question
            .custom
            .as_ref()
            .is_some_and(|custom| custom.is_enabled())
            .then_some(Row::Own);
        options.chain(own).collect()
    }

    fn current_row(&self) -> Option<Row> {
        self.rows().get(self.row).copied()
    }

    /// The working answers in the draft's shape, one per question.
    pub fn answers(&self) -> Vec<Answer> {
        self.session
            .questions
            .iter()
            .zip(&self.working)
            .map(|(question, working)| {
                let filled = |text: &str| (!text.is_empty()).then(|| text.to_owned());
                Answer {
                    question: question.id.clone(),
                    selected: working.selected.clone(),
                    custom: filled(&working.custom),
                    note: filled(&working.note),
                    defaulted: !working.touched && !working.selected.is_empty(),
                    skipped: false,
                }
            })
            .collect()
    }

    /// What a submit would send, counted.
    pub fn counts(&self) -> Counts {
        let result = result_answers(&self.session, &self.answers());
        Counts {
            answered: result.iter().filter(|answer| !answer.skipped).count(),
            skipped: result.iter().filter(|answer| answer.skipped).count(),
            defaulted: result.iter().filter(|answer| answer.defaulted).count(),
        }
    }

    /// The draft; on the review it points at the last question. It also
    /// keeps the session note and the reject reason typed so far.
    pub fn to_draft(&self) -> SessionResult {
        let current = self.tab.min(self.session.questions.len().saturating_sub(1));
        let current = self
            .session
            .questions
            .get(current)
            .map_or("", |question| question.id.as_str());
        let mut draft = SessionResult::draft(&self.id, current, self.answers());
        // Kept as typed, so a restored field reads exactly as it was left.
        if !self.session_note.is_empty() {
            draft.note = Some(self.session_note.clone());
        }
        if !self.reject_reason.is_empty() {
            draft.reason = Some(self.reject_reason.clone());
        }
        draft
    }

    pub fn question_states(&self) -> Vec<QuestionState> {
        self.session
            .questions
            .iter()
            .zip(self.answers())
            .map(|(question, answer)| QuestionState {
                answer: match (is_answered(question, &answer), answer.defaulted) {
                    (false, _) => AnswerState::Skipped,
                    (true, true) => AnswerState::Defaulted,
                    (true, false) => AnswerState::Answered,
                },
                note: answer.note.is_some(),
            })
            .collect()
    }

    /// The text of the focused field.
    pub fn field_text(&self) -> Option<String> {
        (self.focus == Focus::Field)
            .then(|| self.editor_text())
            .flatten()
    }

    /// The text of the note being edited.
    pub fn note_text(&self) -> Option<String> {
        (self.focus == Focus::Note)
            .then(|| self.editor_text())
            .flatten()
    }

    fn editor_text(&self) -> Option<String> {
        self.editor.as_ref().map(|editor| editor.lines().join("\n"))
    }

    /// The text cursor (line, column) of the focused field or note.
    pub fn field_cursor(&self) -> Option<(usize, usize)> {
        self.editor.as_ref().map(|editor| {
            let cursor = editor.cursor();
            (cursor.0, cursor.1)
        })
    }

    /// The length of the focused field and the limits it is counted
    /// against, when the field has limits.
    pub fn field_length(&self) -> Option<(usize, Length)> {
        if self.focus != Focus::Field {
            return None;
        }
        let limits = self.limits(self.current_row()?)?;
        Some((text_length(self.editor.as_ref()?), limits))
    }

    fn limits(&self, row: Row) -> Option<Length> {
        let question = self.question()?;
        // Only the answer of a text question and the own answer carry a
        // length; the reject reason has none, and the review has no
        // question at all.
        if row == Row::Answer {
            question.length
        } else {
            question
                .custom
                .as_ref()
                .and_then(|custom| custom.length())
                .copied()
        }
    }

    /// Handles one key.
    pub fn handle(&mut self, key: KeyEvent) -> Effect {
        self.message = None;
        if self.help {
            self.help = false;
            return Effect::None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Effect::Quit;
        }
        if let Some(effect) = self.pane_key(key) {
            return effect;
        }
        match self.focus {
            Focus::None => self.command(key),
            Focus::Field => self.field_key(key),
            Focus::Note => self.note_key(key),
        }
    }

    // -----------------------------------------------------------------
    // Keys of the context pane
    // -----------------------------------------------------------------

    /// The keys that belong to the context pane, before a field or the
    /// question sees them; `None` leaves `key` to them.
    ///
    /// The focused pane and the full-screen view read like a pager: they
    /// take every key, and only `?`, `z` and `o` keep their meaning from the
    /// question. `tab`, `esc` and `q` go back, a typed word that starts with
    /// `q` included, so no word quits asqr; `ctrl-c` still does. Without the
    /// focus, `tab` and the page keys still reach the pane, also from a
    /// field, as none of them types text (plan, *Keys*).
    fn pane_key(&mut self, key: KeyEvent) -> Option<Effect> {
        if self.pane_focus {
            let control = key.modifiers.contains(KeyModifiers::CONTROL);
            let effect = match key.code {
                KeyCode::Tab | KeyCode::Esc => self.leave_pane(),
                KeyCode::Char('q') if !control => self.leave_pane(),
                KeyCode::Char('z') if !control && self.full_screen.is_some() => self.leave_pane(),
                KeyCode::Char('z') if !control => self.show_full_screen(),
                KeyCode::Char('?') if !control => {
                    self.help = true;
                    Effect::None
                }
                KeyCode::Char('o') if !control => self.open(),
                _ => {
                    self.scroll_pane(key, true);
                    Effect::None
                }
            };
            return Some(effect);
        }
        match key.code {
            // A tab never types: a tab character in a one-line answer is a
            // mistake, and on a text question, whose field has the focus
            // from the start, the pane would otherwise take `esc` first.
            KeyCode::Tab => {
                self.pane_focus = self.pane_page.is_some();
                Some(Effect::None)
            }
            // Without a pane, `ctrl-u` and `ctrl-d` keep the meaning the
            // field's editor gives them.
            _ if self.scroll_pane(key, false) => Some(Effect::None),
            _ => None,
        }
    }

    /// Leaves the full-screen view for where the focus was before it, or
    /// the focused pane for the question.
    fn leave_pane(&mut self) -> Effect {
        self.pane_focus = self
            .full_screen
            .take()
            .is_some_and(|full_screen| full_screen.pane_focus);
        Effect::None
    }

    /// Shows the entry shown full screen, which then takes the keys, so
    /// none of them can move the cursor to another entry while it shows.
    fn show_full_screen(&mut self) -> Effect {
        if self.shown_entry().is_some() {
            self.full_screen = Some(FullScreen {
                pane_focus: self.pane_focus,
            });
            self.pane_focus = true;
        }
        Effect::None
    }

    /// `o`: opens the image shown, or says why it cannot. Outside the
    /// full-screen view it points to `z`, which shows any entry whole.
    fn open(&mut self) -> Effect {
        match self.shown_context() {
            Some(entry) => match entry.image_path() {
                Some(image) => Effect::OpenImage(image.to_owned()),
                None => {
                    let message = if self.full_screen.is_some() {
                        "o opens images only"
                    } else {
                        "o opens images only; z shows this entry full screen"
                    };
                    self.message = Some(message.to_owned());
                    Effect::None
                }
            },
            None => {
                self.message = Some("nothing to open here".to_owned());
                Effect::None
            }
        }
    }

    /// Scrolls the entry shown when `key` scrolls the pane, and says
    /// whether it did.
    fn scroll_pane(&mut self, key: KeyEvent, focused: bool) -> bool {
        let (Some(page), Some((entry, _))) = (self.pane_page, self.shown_entry()) else {
            return false;
        };
        let offset = self.offsets.get(&entry).copied().unwrap_or_default();
        match scroll(key, offset, page, focused) {
            Some(scrolled) => {
                self.offsets.insert(entry, scrolled);
                true
            }
            None => false,
        }
    }

    // -----------------------------------------------------------------
    // Keys outside fields
    // -----------------------------------------------------------------

    fn command(&mut self, key: KeyEvent) -> Effect {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return Effect::None;
        }
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => self.move_row(-1),
            KeyCode::Char('j') | KeyCode::Down => self.move_row(1),
            KeyCode::Char('h') | KeyCode::Left => match self.tab.checked_sub(1) {
                Some(previous) => self.go_to_tab(previous),
                None => Effect::None,
            },
            KeyCode::Char('l') | KeyCode::Right => self.go_to_tab(self.tab + 1),
            KeyCode::Enter => self.enter(),
            KeyCode::Char(' ') => match self.current_row() {
                Some(Row::Option(index)) if self.kind() == Some(Kind::Multi) => self.toggle(index),
                _ => Effect::None,
            },
            KeyCode::Char(digit @ '1'..='9') => {
                let index = digit as usize - '1' as usize;
                if !self.rows().contains(&Row::Option(index)) {
                    return Effect::None;
                }
                self.row = index;
                match self.kind() {
                    Some(Kind::Multi) => self.toggle(index),
                    _ => self.pick(index),
                }
            }
            KeyCode::Char('n') => self.open_note(),
            KeyCode::Char('q') => Effect::Quit,
            KeyCode::Char('?') => {
                self.help = true;
                Effect::None
            }
            KeyCode::Char('L') => Effect::OpenSessionList,
            KeyCode::Char('i') if self.session.intro.is_some() => Effect::ShowIntro,
            KeyCode::Char('o') => self.open(),
            KeyCode::Char('z') => self.show_full_screen(),
            _ => Effect::None,
        }
    }

    fn kind(&self) -> Option<Kind> {
        self.question().map(|question| question.kind)
    }

    /// `enter` on the current row.
    fn enter(&mut self) -> Effect {
        let row = self
            .current_row()
            .expect("validation leaves every tab at least one row");
        match row {
            Row::Option(index) if self.kind() == Some(Kind::Single) => self.pick(index),
            Row::Option(_) | Row::Answer => self.go_to_tab(self.tab + 1),
            Row::Own => self.pick_own(),
            Row::Question(index) => self.go_to_tab(index),
            Row::Submit | Row::Note => self.submit(),
            Row::Reject => {
                let reason = self.reject_reason.trim();
                Effect::Reject((!reason.is_empty()).then(|| reason.to_owned()))
            }
        }
    }

    fn move_row(&mut self, delta: isize) -> Effect {
        let last = self.rows().len().saturating_sub(1);
        let row = self.row.saturating_add_signed(delta).min(last);
        // Landing on a row focuses its field; a field left with esc is
        // landed on again when the move stays on it, as on a text question
        // with its single row.
        if row != self.row || self.focus == Focus::None {
            self.row = row;
            self.arrive();
        }
        Effect::None
    }

    fn go_to_tab(&mut self, tab: usize) -> Effect {
        if tab >= self.tab_count() {
            return Effect::None;
        }
        self.tab = tab;
        // The review opens on Submit, the usual next step (user,
        // 2026-09-28); questions start on their first row.
        self.row = if self.on_review() {
            self.session.questions.len()
        } else {
            0
        };
        self.arrive();
        Effect::DraftChanged
    }

    /// Focuses the field of the current row, if it is one. Called whenever
    /// the cursor lands on a row.
    fn arrive(&mut self) {
        match self.current_row() {
            Some(row) if row.is_field() => {
                let text = match row {
                    Row::Note => self.session_note.clone(),
                    Row::Reject => self.reject_reason.clone(),
                    _ => self.working[self.tab].custom.clone(),
                };
                self.editor = Some(editor_with(&text));
                self.focus = Focus::Field;
            }
            _ => {
                self.editor = None;
                self.focus = Focus::None;
            }
        }
    }

    /// Picks option `index` of a single question and moves on.
    fn pick(&mut self, index: usize) -> Effect {
        let id = self
            .question()
            .and_then(|question| option_ids(question).nth(index).map(str::to_owned))
            .expect("callers only pick options the question has");
        let working = &mut self.working[self.tab];
        working.selected = vec![id];
        working.touched = true;
        self.go_to_tab(self.tab + 1)
    }

    /// Picks the own answer and moves on. On a single question that
    /// deselects the options, since the own answer counts only without one.
    fn pick_own(&mut self) -> Effect {
        if self.kind() == Some(Kind::Single) {
            let working = &mut self.working[self.tab];
            working.selected.clear();
            working.touched = true;
        }
        self.go_to_tab(self.tab + 1)
    }

    fn toggle(&mut self, index: usize) -> Effect {
        let question = &self.session.questions[self.tab];
        let id = option_ids(question)
            .nth(index)
            .map(str::to_owned)
            .expect("callers only toggle options the question has");
        let working = &mut self.working[self.tab];
        if working.selected.contains(&id) {
            working.selected.retain(|selected| *selected != id);
        } else {
            if let Some(max) = question.max
                && working.selected.len() >= max as usize
            {
                self.message = Some(format!("at most {max} options"));
                return Effect::None;
            }
            working.selected.push(id);
            let order: Vec<&str> = option_ids(question).collect();
            working
                .selected
                .sort_by_key(|selected| order.iter().position(|id| id == selected));
        }
        working.touched = true;
        Effect::DraftChanged
    }

    /// Every question is optional (user, 2026-09-28), so a submit always
    /// goes through; unanswered questions come back as skipped. The session
    /// note is trimmed like the reject reason, as both are one-line fields.
    fn submit(&mut self) -> Effect {
        let note = self.session_note.trim();
        Effect::Submit {
            answers: result_answers(&self.session, &self.answers()),
            note: (!note.is_empty()).then(|| note.to_owned()),
        }
    }

    fn open_note(&mut self) -> Effect {
        let Some(question) = self.question() else {
            return Effect::None;
        };
        if !question.note {
            self.message = Some("this question takes no note".to_owned());
            return Effect::None;
        }
        self.editor = Some(editor_with(&self.working[self.tab].note));
        self.focus = Focus::Note;
        Effect::None
    }

    // -----------------------------------------------------------------
    // Keys inside fields
    // -----------------------------------------------------------------

    fn field_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::None;
                self.editor = None;
                Effect::None
            }
            KeyCode::Up => self.move_row(-1),
            KeyCode::Down => self.move_row(1),
            KeyCode::Enter => self.enter(),
            // At the edges of the text the arrows leave the field for the
            // question before or after, so a field never traps the person
            // (user, 2026-09-28). Inside the text they move the cursor.
            KeyCode::Left
                if key.modifiers.is_empty() && self.tab > 0 && self.cursor_at(Edge::Start) =>
            {
                self.go_to_tab(self.tab - 1)
            }
            KeyCode::Right
                if key.modifiers.is_empty()
                    && self.tab + 1 < self.tab_count()
                    && self.cursor_at(Edge::End) =>
            {
                self.go_to_tab(self.tab + 1)
            }
            _ => {
                // Only a text answer spans several lines; own answers, the
                // session note and the reject reason are one line (ADR 22).
                let row = self.current_row();
                if !self.edit(key, row == Some(Row::Answer)) {
                    return Effect::None;
                }
                let text = self.editor_text().unwrap_or_default();
                match row {
                    Some(Row::Note) => self.session_note = text,
                    Some(Row::Reject) => self.reject_reason = text,
                    _ => {
                        let working = &mut self.working[self.tab];
                        working.custom = text;
                        working.touched = true;
                    }
                }
                Effect::DraftChanged
            }
        }
    }

    /// Whether the cursor of the focused field sits at `edge` of its text.
    fn cursor_at(&self, edge: Edge) -> bool {
        let editor = self.editor.as_ref().expect("a focused field has an editor");
        let cursor = editor.cursor();
        let (row, column) = (cursor.0, cursor.1);
        match edge {
            Edge::Start => (row, column) == (0, 0),
            Edge::End => {
                let lines = editor.lines();
                row + 1 == lines.len() && column == lines[row].chars().count()
            }
        }
    }

    fn note_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.arrive();
                Effect::None
            }
            _ => {
                if !self.edit(key, true) {
                    return Effect::None;
                }
                let working = &mut self.working[self.tab];
                working.note = self
                    .editor
                    .as_ref()
                    .map(|editor| editor.lines().join("\n"))
                    .unwrap_or_default();
                working.touched = true;
                Effect::DraftChanged
            }
        }
    }

    /// Applies `key` to the editor and says whether the text changed.
    /// `ctrl-j` adds a line where `multiline` allows one; any other way to
    /// a second line in a one-line field is undone.
    fn edit(&mut self, key: KeyEvent, multiline: bool) -> bool {
        let editor = self
            .editor
            .as_mut()
            .expect("a focused field or note always has its editor");
        let before = editor.lines().to_vec();
        if key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if multiline {
                editor.insert_newline();
            }
        } else {
            let previous = editor.clone();
            editor.input(key);
            if !multiline && editor.lines().len() > 1 {
                *editor = previous;
            }
        }
        editor.lines() != before.as_slice()
    }
}

fn option_ids(question: &Question) -> impl Iterator<Item = &str> {
    question
        .options
        .iter()
        .flatten()
        .map(|option| option.id.as_str())
}

fn is_default(question: &Question, id: &str) -> bool {
    question
        .options
        .iter()
        .flatten()
        .any(|option| option.id == id && option.default)
}

/// A working answer from a draft, keeping only what fits the question.
fn restored(question: &Question, answer: &Answer) -> Working {
    let mut selected: Vec<String> = option_ids(question)
        .filter(|id| answer.selected.iter().any(|selected| selected == id))
        .map(str::to_owned)
        .collect();
    if question.kind == Kind::Single {
        selected.truncate(1);
    }
    Working {
        selected,
        custom: answer.custom.clone().unwrap_or_default(),
        note: answer.note.clone().unwrap_or_default(),
        touched: !answer.defaulted,
    }
}

fn editor_with(text: &str) -> TextArea<'static> {
    let mut editor = TextArea::new(text.split('\n').map(str::to_owned).collect());
    editor.set_hard_tab_indent(true);
    editor.move_cursor(CursorMove::Bottom);
    editor.move_cursor(CursorMove::End);
    editor
}

/// Characters in the editor, a line break counting as one.
fn text_length(editor: &TextArea<'_>) -> usize {
    editor
        .lines()
        .iter()
        .map(|line| line.chars().count())
        .sum::<usize>()
        + editor.lines().len().saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::format::{Answer, SessionResult};
    use crate::tui::Extent;

    const SESSION: &str = r#"{"asqr": 2, "questions": [
        {"id": "single", "text": "?", "kind": "single", "custom": true,
         "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B", "default": true}]},
        {"id": "multi", "text": "?", "kind": "multi", "max": 2, "custom": true,
         "options": [{"id": "x", "label": "X"}, {"id": "y", "label": "Y"}, {"id": "z", "label": "Z"}]},
        {"id": "text", "text": "?", "kind": "text", "length": {"warn": 5}},
        {"id": "plain", "text": "?", "kind": "single", "note": false, "context": [{"type": "image", "path": "/pictures/p.png"}],
         "options": [{"id": "only", "label": "Only"}]}
    ]}"#;

    fn state() -> SessionState {
        SessionState::new(
            "batch",
            serde_json::from_str(SESSION).expect("test session parses"),
            None,
        )
    }

    fn code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn press(state: &mut SessionState, keys: &str) -> Effect {
        let mut last = Effect::None;
        for c in keys.chars() {
            last = state.handle(code(KeyCode::Char(c)));
        }
        last
    }

    fn answer(state: &SessionState, question: &str) -> Answer {
        state
            .answers()
            .into_iter()
            .find(|answer| answer.question == question)
            .expect("every question has an answer")
    }

    /// Moves right until tab `tab` is current, leaving any focused field
    /// first, as a person would with `esc`.
    fn go_to_tab(state: &mut SessionState, tab: usize) {
        while state.tab() < tab {
            if state.focus() != Focus::None {
                state.handle(code(KeyCode::Esc));
            }
            state.handle(code(KeyCode::Right));
        }
    }

    // ---------------------------------------------------------------
    // Tabs and rows
    // ---------------------------------------------------------------

    #[test]
    fn starts_on_the_first_row_of_the_first_question() {
        let state = state();

        assert_eq!((state.tab(), state.row()), (0, 0));
        assert_eq!(state.focus(), Focus::None);
        assert_eq!(state.tab_count(), 5, "four questions and the review");
    }

    #[test]
    fn the_rows_are_the_options_then_the_own_answer() {
        let mut state = state();

        assert_eq!(state.rows(), [Row::Option(0), Row::Option(1), Row::Own]);
        go_to_tab(&mut state, 2);
        assert_eq!(state.rows(), [Row::Answer]);
        go_to_tab(&mut state, 4);
        assert_eq!(
            state.rows(),
            [
                Row::Question(0),
                Row::Question(1),
                Row::Question(2),
                Row::Question(3),
                Row::Submit,
                Row::Note,
                Row::Reject
            ]
        );
    }

    #[test]
    fn every_questions_rows_are_known_from_any_tab() {
        let mut state = state();
        go_to_tab(&mut state, 4);

        assert_eq!(
            (0..4).map(|index| state.rows_of(index)).collect::<Vec<_>>(),
            [
                vec![Row::Option(0), Row::Option(1), Row::Own],
                vec![Row::Option(0), Row::Option(1), Row::Option(2), Row::Own],
                vec![Row::Answer],
                vec![Row::Option(0)],
            ]
        );
        assert_eq!(
            state
                .session()
                .questions
                .iter()
                .map(has_context)
                .collect::<Vec<_>>(),
            [false, false, false, true]
        );
    }

    #[test]
    fn the_review_opens_on_submit() {
        let mut state = state();

        go_to_tab(&mut state, 4);

        assert_eq!(state.rows()[state.row()], Row::Submit);
    }

    #[test]
    fn up_and_down_move_between_rows_and_stop_at_the_ends() {
        let mut state = state();

        press(&mut state, "k");
        assert_eq!(state.row(), 0);
        press(&mut state, "j");
        state.handle(code(KeyCode::Down));
        assert_eq!(state.row(), 2);
        state.handle(code(KeyCode::Down));
        assert_eq!(state.row(), 2, "the own-answer row is the last");
        state.handle(code(KeyCode::Up));
        assert_eq!(state.row(), 1);
    }

    #[test]
    fn left_and_right_switch_questions_up_to_the_review() {
        let mut state = state();

        assert_eq!(state.handle(code(KeyCode::Right)), Effect::DraftChanged);
        assert_eq!(state.tab(), 1);
        press(&mut state, "l");
        // The text question's field takes the keys until esc.
        state.handle(code(KeyCode::Esc));
        press(&mut state, "lll");
        assert_eq!(state.tab(), 4, "the review is the last tab");
        assert!(state.on_review());
        press(&mut state, "h");
        state.handle(code(KeyCode::Left));
        assert_eq!((state.tab(), state.row()), (2, 0));
        assert_eq!(state.to_draft().current.as_deref(), Some("text"));
    }

    #[test]
    fn switching_past_either_end_changes_nothing() {
        let mut state = state();

        assert_eq!(press(&mut state, "h"), Effect::None);
        assert_eq!(state.tab(), 0);
    }

    // ---------------------------------------------------------------
    // Picking
    // ---------------------------------------------------------------

    #[test]
    fn enter_picks_on_a_single_question_and_moves_on() {
        let mut state = state();

        assert_eq!(state.handle(code(KeyCode::Enter)), Effect::DraftChanged);

        assert_eq!(answer(&state, "single").selected, ["a"]);
        assert_eq!(state.tab(), 1);
    }

    #[test]
    fn a_digit_picks_directly_and_moves_on() {
        let mut state = state();

        press(&mut state, "2");
        assert_eq!(answer(&state, "single").selected, ["b"]);
        assert!(
            !answer(&state, "single").defaulted,
            "picking the default is an answer"
        );
        assert_eq!(state.tab(), 1);

        go_to_tab(&mut state, 3);
        assert_eq!(
            press(&mut state, "9"),
            Effect::None,
            "there is no ninth option"
        );
    }

    #[test]
    fn space_does_nothing_on_a_single_question() {
        let mut state = state();

        assert_eq!(press(&mut state, " "), Effect::None);
        assert_eq!(answer(&state, "single").selected, ["b"]);
    }

    #[test]
    fn a_default_counts_until_the_person_edits_the_question() {
        let state = state();

        assert_eq!(
            answer(&state, "single"),
            Answer {
                selected: vec!["b".into()],
                defaulted: true,
                ..Answer::new("single")
            }
        );
    }

    #[test]
    fn multi_toggles_with_space_and_digits_up_to_max_and_enter_moves_on() {
        let mut state = state();
        press(&mut state, "l");

        press(&mut state, " 3");
        assert_eq!(
            answer(&state, "multi").selected,
            ["x", "z"],
            "kept in option order"
        );
        assert_eq!(press(&mut state, "2"), Effect::None);
        assert_eq!(state.message(), Some("at most 2 options"));
        press(&mut state, "1");
        assert_eq!(answer(&state, "multi").selected, ["z"]);
        assert_eq!(state.tab(), 1, "toggling stays on the question");

        state.handle(code(KeyCode::Enter));
        assert_eq!(state.tab(), 2);
    }

    #[test]
    fn enter_on_the_last_question_moves_on_to_the_review() {
        let mut state = state();
        go_to_tab(&mut state, 3);

        state.handle(code(KeyCode::Enter));

        assert!(state.on_review());
    }

    // ---------------------------------------------------------------
    // The own answer
    // ---------------------------------------------------------------

    #[test]
    fn landing_on_the_own_answer_row_focuses_its_field() {
        let mut state = state();

        press(&mut state, "jj");

        assert_eq!(state.focus(), Focus::Field);
        // Letters that are keys elsewhere type into the field.
        press(&mut state, "jkhlnq1 ");
        assert_eq!(state.field_text().as_deref(), Some("jkhlnq1 "));
        assert_eq!(answer(&state, "single").custom.as_deref(), Some("jkhlnq1 "));
    }

    #[test]
    fn arrows_edit_inside_the_field() {
        let mut state = state();
        press(&mut state, "jjac");

        state.handle(code(KeyCode::Left));
        press(&mut state, "b");

        assert_eq!(state.field_text().as_deref(), Some("abc"));
        assert_eq!(
            state.tab(),
            0,
            "left moved the text cursor, not the question"
        );
    }

    #[test]
    fn arrows_at_the_edges_of_a_field_switch_questions() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        press(&mut state, "ok");
        assert_eq!(state.focus(), Focus::Field);

        // The cursor is at the end, so → moves on to the next question.
        state.handle(code(KeyCode::Right));
        assert_eq!(state.tab(), 3);

        // Back on the text question its field is focused with the cursor
        // at the end; ← walks through the text and, at its start, on to
        // the question before.
        state.handle(code(KeyCode::Left));
        assert_eq!((state.tab(), state.focus()), (2, Focus::Field));
        state.handle(code(KeyCode::Left));
        state.handle(code(KeyCode::Left));
        assert_eq!(state.tab(), 2, "two lefts only reach the start");
        state.handle(code(KeyCode::Left));
        assert_eq!(state.tab(), 1);
        assert_eq!(answer(&state, "text").custom.as_deref(), Some("ok"));
    }

    #[test]
    fn an_empty_field_switches_both_ways_at_once() {
        let mut state = state();
        go_to_tab(&mut state, 2);

        state.handle(code(KeyCode::Left));
        assert_eq!(state.tab(), 1);
        go_to_tab(&mut state, 2);
        state.handle(code(KeyCode::Right));
        assert_eq!(state.tab(), 3);
    }

    #[test]
    fn the_first_question_and_the_reject_field_stay_put_at_their_ends() {
        let mut state = state();
        press(&mut state, "jj");
        assert_eq!(state.focus(), Focus::Field, "the own answer of question 1");
        state.handle(code(KeyCode::Left));
        assert_eq!((state.tab(), state.focus()), (0, Focus::Field));

        go_to_tab(&mut state, 4);
        press(&mut state, "j");
        state.handle(code(KeyCode::Down));
        assert_eq!(state.current_row(), Some(Row::Reject));
        state.handle(code(KeyCode::Right));
        assert_eq!((state.tab(), state.focus()), (4, Focus::Field));
    }

    #[test]
    fn up_leaves_the_field_and_keeps_the_text() {
        let mut state = state();
        press(&mut state, "jjown");

        state.handle(code(KeyCode::Up));

        assert_eq!((state.row(), state.focus()), (1, Focus::None));
        assert_eq!(answer(&state, "single").custom.as_deref(), Some("own"));
    }

    #[test]
    fn a_chosen_option_wins_over_typed_text_until_the_own_answer_is_picked() {
        let mut state = state();
        press(&mut state, "jjown");
        state.handle(code(KeyCode::Up));

        // The default b is still chosen, so it counts.
        let single = answer(&state, "single");
        assert_eq!(
            (single.selected.clone(), single.custom.as_deref()),
            (vec!["b".to_owned()], Some("own"))
        );

        state.handle(code(KeyCode::Down));
        assert_eq!(state.handle(code(KeyCode::Enter)), Effect::DraftChanged);

        let single = answer(&state, "single");
        assert!(
            single.selected.is_empty(),
            "picking the own answer deselects the options"
        );
        assert_eq!(single.custom.as_deref(), Some("own"));
        assert_eq!(state.tab(), 1, "and moves on");
    }

    #[test]
    fn esc_leaves_the_field_so_keys_work_again() {
        let mut state = state();
        press(&mut state, "jjx");

        state.handle(code(KeyCode::Esc));
        assert_eq!((state.row(), state.focus()), (2, Focus::None));
        press(&mut state, "l");

        assert_eq!(state.tab(), 1);
    }

    #[test]
    fn the_own_answer_is_one_line() {
        let mut state = state();
        press(&mut state, "jja");

        state.handle(ctrl('j'));
        press(&mut state, "b");

        assert_eq!(state.field_text().as_deref(), Some("ab"));
    }

    #[test]
    fn on_a_multi_question_the_own_answer_counts_once_it_has_text() {
        let mut state = state();
        press(&mut state, "l ");
        press(&mut state, "jjjmore");
        state.handle(code(KeyCode::Enter));

        let multi = answer(&state, "multi");
        assert_eq!(
            (multi.selected, multi.custom.as_deref()),
            (vec!["x".to_owned()], Some("more"))
        );
        assert_eq!(state.tab(), 2);
    }

    // ---------------------------------------------------------------
    // Text questions
    // ---------------------------------------------------------------

    #[test]
    fn a_text_question_types_right_away_and_takes_new_lines() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        assert_eq!(state.focus(), Focus::Field);

        press(&mut state, "one");
        state.handle(ctrl('j'));
        press(&mut state, "two");

        assert_eq!(answer(&state, "text").custom.as_deref(), Some("one\ntwo"));
        assert_eq!(
            state.field_length(),
            Some((
                7,
                Length {
                    target: None,
                    warn: Some(5)
                }
            ))
        );

        state.handle(code(KeyCode::Enter));
        assert_eq!(state.tab(), 3, "enter moves on");
    }

    #[test]
    fn a_text_question_can_be_left_with_esc() {
        let mut state = state();
        go_to_tab(&mut state, 2);

        state.handle(code(KeyCode::Esc));
        press(&mut state, "h");

        assert_eq!(state.tab(), 1);
    }

    #[test]
    fn up_or_down_on_a_left_field_focuses_it_again() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        state.handle(code(KeyCode::Esc));

        state.handle(code(KeyCode::Down));

        assert_eq!(state.focus(), Focus::Field);
    }

    #[test]
    fn coming_back_to_a_field_row_focuses_it_again() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        state.handle(code(KeyCode::Esc));

        press(&mut state, "lh");

        assert_eq!(state.focus(), Focus::Field);
    }

    // ---------------------------------------------------------------
    // Notes
    // ---------------------------------------------------------------

    #[test]
    fn n_edits_the_note_in_place_over_several_lines() {
        let mut state = state();

        press(&mut state, "n");
        assert_eq!(state.focus(), Focus::Note);
        press(&mut state, "a");
        state.handle(ctrl('j'));
        press(&mut state, "b");
        assert_eq!(state.handle(code(KeyCode::Enter)), Effect::None);

        assert_eq!(state.focus(), Focus::None);
        let single = answer(&state, "single");
        assert_eq!(single.note.as_deref(), Some("a\nb"));
        assert!(!single.defaulted, "a note is an edit of the question");
        assert_eq!(state.tab(), 0, "enter leaves the note without moving on");
    }

    #[test]
    fn esc_leaves_the_note_too() {
        let mut state = state();
        press(&mut state, "nx");

        state.handle(code(KeyCode::Esc));

        assert_eq!(
            (state.focus(), answer(&state, "single").note.as_deref()),
            (Focus::None, Some("x"))
        );
    }

    #[test]
    fn questions_without_notes_say_so() {
        let mut state = state();
        go_to_tab(&mut state, 3);

        assert_eq!(press(&mut state, "n"), Effect::None);

        assert_eq!(state.message(), Some("this question takes no note"));
        assert_eq!(state.focus(), Focus::None);
    }

    #[test]
    fn a_message_lasts_until_the_next_key() {
        let mut state = state();
        go_to_tab(&mut state, 3);
        press(&mut state, "n");

        press(&mut state, "k");

        assert_eq!(state.message(), None);
    }

    // ---------------------------------------------------------------
    // The review
    // ---------------------------------------------------------------

    #[test]
    fn enter_on_a_question_in_the_review_goes_there() {
        let mut state = state();
        go_to_tab(&mut state, 4);

        press(&mut state, "kkk");
        state.handle(code(KeyCode::Enter));

        assert_eq!((state.tab(), state.row()), (1, 0));
    }

    #[test]
    fn every_question_is_optional() {
        let mut state = state();
        go_to_tab(&mut state, 4);

        let Effect::Submit { answers, note } = state.handle(code(KeyCode::Enter)) else {
            panic!("nothing blocks a submit");
        };

        assert_eq!(answers[2], Answer::skipped("text"));
        assert_eq!(note, None);
    }

    #[test]
    fn submitting_sends_every_question() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        press(&mut state, "ok");
        state.handle(code(KeyCode::Enter));
        go_to_tab(&mut state, 4);

        let Effect::Submit { answers, .. } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on Submit submits");
        };

        assert_eq!(answers.len(), 4);
        assert_eq!(answers[1], Answer::skipped("multi"));
        assert_eq!(answers[2].custom.as_deref(), Some("ok"));
    }

    /// Moves from Submit, where the review opens, down to `row`.
    fn review_row(state: &mut SessionState, row: Row) {
        go_to_tab(state, 4);
        while state.current_row() != Some(row) {
            state.handle(code(KeyCode::Down));
        }
    }

    #[test]
    fn the_note_row_below_submit_is_a_field_and_enter_submits_with_it() {
        let mut state = state();
        go_to_tab(&mut state, 4);
        press(&mut state, "j");
        assert_eq!(
            (state.current_row(), state.focus()),
            (Some(Row::Note), Focus::Field)
        );

        assert_eq!(
            press(&mut state, "  only if the tests pass "),
            Effect::DraftChanged
        );
        assert_eq!(state.session_note(), "  only if the tests pass ");
        let Effect::Submit { answers, note } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on the note submits");
        };

        assert_eq!(answers.len(), 4);
        assert_eq!(note.as_deref(), Some("only if the tests pass"), "trimmed");
    }

    #[test]
    fn a_blank_session_note_is_no_note() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        press(&mut state, "   ");

        assert!(matches!(
            state.handle(code(KeyCode::Enter)),
            Effect::Submit { note: None, .. }
        ));
    }

    #[test]
    fn the_note_row_is_left_like_any_other_field() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        press(&mut state, "ok");

        state.handle(code(KeyCode::Up));
        assert_eq!(
            (state.current_row(), state.focus()),
            (Some(Row::Submit), Focus::None)
        );
        state.handle(code(KeyCode::Down));
        state.handle(code(KeyCode::Down));
        assert_eq!(state.current_row(), Some(Row::Reject));

        state.handle(code(KeyCode::Up));
        state.handle(code(KeyCode::Esc));
        assert_eq!(
            (state.current_row(), state.focus()),
            (Some(Row::Note), Focus::None)
        );
        press(&mut state, "h");
        assert_eq!(state.tab(), 3, "after esc, keys are commands again");
        assert_eq!(state.session_note(), "ok");
    }

    #[test]
    fn left_at_the_start_of_the_session_note_goes_to_the_last_question() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        press(&mut state, "a");

        state.handle(code(KeyCode::Left));
        assert_eq!(state.tab(), 4, "left moved the text cursor first");
        state.handle(code(KeyCode::Left));

        assert_eq!(state.tab(), 3);
    }

    #[test]
    fn the_session_note_is_saved_with_the_draft_as_typed() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        press(&mut state, " later ");

        let draft = state.to_draft();
        assert_eq!(draft.note.as_deref(), Some(" later "));

        let session = serde_json::from_str(SESSION).expect("test session parses");
        let restored = SessionState::new("batch", session, Some(&draft));
        assert_eq!(restored.session_note(), " later ");
    }

    #[test]
    fn an_empty_session_note_is_no_draft_note() {
        assert_eq!(state().to_draft().note, None);
    }

    #[test]
    fn rejecting_leaves_the_session_note_out() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        press(&mut state, "approved");
        state.handle(code(KeyCode::Down));

        assert_eq!(state.handle(code(KeyCode::Enter)), Effect::Reject(None));
    }

    #[test]
    fn the_reject_row_takes_an_optional_reason() {
        let mut state = state();
        review_row(&mut state, Row::Reject);
        assert_eq!(state.focus(), Focus::Field);

        press(&mut state, "  out of date ");
        assert_eq!(
            state.handle(code(KeyCode::Enter)),
            Effect::Reject(Some("out of date".into()))
        );

        let mut empty = self::state();
        review_row(&mut empty, Row::Reject);
        assert_eq!(empty.handle(code(KeyCode::Enter)), Effect::Reject(None));
    }

    #[test]
    fn the_reject_reason_is_saved_with_the_draft() {
        let mut state = state();
        review_row(&mut state, Row::Reject);

        assert_eq!(press(&mut state, "stale"), Effect::DraftChanged);
        let draft = state.to_draft();
        assert_eq!(draft.reason.as_deref(), Some("stale"));

        let session = serde_json::from_str(SESSION).expect("test session parses");
        let restored = SessionState::new("batch", session, Some(&draft));
        assert_eq!(restored.reject_reason(), "stale");
    }

    #[test]
    fn an_empty_reject_reason_is_no_draft_reason() {
        assert_eq!(state().to_draft().reason, None);
    }

    #[test]
    fn the_review_counts_what_a_submit_would_send() {
        let state = state();

        assert_eq!(
            state.counts(),
            Counts {
                answered: 1,
                skipped: 3,
                defaulted: 1
            }
        );
    }

    // ---------------------------------------------------------------
    // Everywhere
    // ---------------------------------------------------------------

    #[test]
    fn q_quits_outside_fields_and_ctrl_c_quits_everywhere() {
        let mut state = state();
        assert_eq!(press(&mut state, "q"), Effect::Quit);

        press(&mut state, "jj");
        assert_eq!(state.handle(ctrl('c')), Effect::Quit);
    }

    #[test]
    fn help_closes_on_any_key() {
        let mut state = state();

        press(&mut state, "?");
        assert!(state.help());
        assert_eq!(
            press(&mut state, "q"),
            Effect::None,
            "the key only closes the help"
        );
        assert!(!state.help());
    }

    #[test]
    fn l_asks_for_the_session_list_and_images_open_or_fill_the_screen() {
        let mut state = state();
        assert_eq!(press(&mut state, "L"), Effect::OpenSessionList);

        assert_eq!(
            press(&mut state, "o"),
            Effect::None,
            "no image on this question"
        );
        press(&mut state, "z");
        assert!(!state.full_screen());

        go_to_tab(&mut state, 3);
        assert_eq!(
            press(&mut state, "o"),
            Effect::OpenImage("/pictures/p.png".into())
        );
        press(&mut state, "z");
        assert!(state.full_screen());
        press(&mut state, "l");
        assert_eq!(
            (state.full_screen(), state.tab()),
            (true, 3),
            "the full-screen view takes the keys"
        );
    }

    #[test]
    fn o_says_why_it_does_nothing() {
        let mut state = state();
        assert_eq!(press(&mut state, "o"), Effect::None);
        assert_eq!(state.message(), Some("nothing to open here"));

        let mut state = with_pane();
        let code_entry = "o opens images only; z shows this entry full screen";
        assert_eq!(press(&mut state, "o"), Effect::None);
        assert_eq!(state.message(), Some(code_entry));
        state.handle(code(KeyCode::Tab));
        assert_eq!(press(&mut state, "o"), Effect::None);
        assert_eq!(state.message(), Some(code_entry), "from the focused pane");
        press(&mut state, "z");
        assert_eq!(press(&mut state, "o"), Effect::None);
        assert_eq!(
            state.message(),
            Some("o opens images only"),
            "the full-screen view already shows the entry whole"
        );
    }

    #[test]
    fn the_context_follows_the_option_under_the_cursor() {
        let session = r#"{"asqr": 2, "questions": [
            {"id": "pictured", "text": "?", "kind": "single", "custom": true,
             "context": [{"type": "image", "path": "/pictures/question.png"}],
             "options": [{"id": "a", "label": "A", "context": [{"type": "image", "path": "/pictures/a.png"}]}, {"id": "b", "label": "B"}]},
            {"id": "bare", "text": "?", "kind": "multi",
             "options": [{"id": "x", "label": "X", "context": [{"type": "image", "path": "/pictures/x.png"}]}, {"id": "y", "label": "Y"}]}
        ]}"#;
        let mut state = SessionState::new(
            "batch",
            serde_json::from_str(session).expect("test session parses"),
            None,
        );

        assert_eq!(state.shown_image(), Some("/pictures/a.png"));
        assert_eq!(
            press(&mut state, "o"),
            Effect::OpenImage("/pictures/a.png".into())
        );
        press(&mut state, "j");
        assert_eq!(
            state.shown_image(),
            Some("/pictures/question.png"),
            "an option without an image shows the question's"
        );
        press(&mut state, "j");
        assert_eq!(state.current_row(), Some(Row::Own));
        assert_eq!(state.shown_image(), Some("/pictures/question.png"));

        state.handle(code(KeyCode::Esc));
        go_to_tab(&mut state, 1);
        assert_eq!(state.shown_image(), Some("/pictures/x.png"));
        assert!(state.has_context_area());
        press(&mut state, "j");
        assert_eq!(state.shown_context(), None);
        assert!(
            state.has_context_area(),
            "the area stays, so the layout does not jump between options"
        );

        go_to_tab(&mut state, 2);
        assert_eq!(
            (state.shown_context(), state.has_context_area()),
            (None, false)
        );
        assert_eq!(
            state
                .session()
                .questions
                .iter()
                .map(has_context)
                .collect::<Vec<_>>(),
            [true, true],
            "from any tab, by the question's or an option's context"
        );
    }

    #[test]
    fn the_shown_entry_is_known_by_its_question_and_option() {
        let session = r#"{"asqr": 2, "questions": [
            {"id": "pictured", "text": "?", "kind": "single",
             "context": [{"type": "image", "path": "/pictures/question.png"}],
             "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B", "context": [{"type": "image", "path": "/pictures/b.png"}]}]}
        ]}"#;
        let mut state = SessionState::new(
            "batch",
            serde_json::from_str(session).expect("test session parses"),
            None,
        );
        let key = |state: &SessionState| state.shown_entry().map(|(key, _)| key);

        assert_eq!(
            key(&state),
            Some(EntryKey {
                question: 0,
                option: None
            }),
            "an option without context shows the question's"
        );
        press(&mut state, "j");
        assert_eq!(
            key(&state),
            Some(EntryKey {
                question: 0,
                option: Some(1)
            })
        );
        go_to_tab(&mut state, 1);
        assert_eq!(key(&state), None, "the review shows no context");
    }

    #[test]
    fn only_an_image_entry_opens_but_any_entry_fills_the_screen() {
        let session = r#"{"asqr": 2, "questions": [
            {"id": "q", "text": "?", "kind": "single",
             "context": [{"type": "code", "title": "Log", "content": "ok"}],
             "options": [{"id": "a", "label": "A"}]}]}"#;
        let mut state = SessionState::new(
            "batch",
            serde_json::from_str(session).expect("test session parses"),
            None,
        );

        assert_eq!(
            state.shown_context().and_then(ContextEntry::title),
            Some("Log")
        );
        assert!(state.has_context_area());
        assert_eq!(state.shown_image(), None);
        assert_eq!(press(&mut state, "o"), Effect::None);
        assert!(state.message().is_some(), "o says why");
        press(&mut state, "z");
        assert!(state.full_screen());
        press(&mut state, "z");
        assert!(!state.full_screen(), "z toggles back");
    }

    #[test]
    fn i_asks_for_the_intro_where_there_is_one() {
        let mut state = state();
        assert_eq!(
            press(&mut state, "i"),
            Effect::None,
            "this session has none"
        );

        let with_intro =
            SESSION.replacen(r#"{"asqr": 2,"#, r#"{"asqr": 2, "intro": "Round two.","#, 1);
        let mut state = SessionState::new(
            "batch",
            serde_json::from_str(&with_intro).expect("test session parses"),
            None,
        );
        assert_eq!(press(&mut state, "i"), Effect::ShowIntro);

        press(&mut state, "jj");
        assert_eq!(state.focus(), Focus::Field);
        assert_eq!(
            press(&mut state, "i"),
            Effect::DraftChanged,
            "in a field, i is typed"
        );
    }

    // ---------------------------------------------------------------
    // The context pane
    // ---------------------------------------------------------------

    /// A question whose code entry shows on every row but the second,
    /// which has an entry of its own, then a text question with an image.
    const WITH_CONTEXT: &str = r#"{"asqr": 2, "questions": [
        {"id": "q", "text": "?", "kind": "single", "custom": true,
         "context": [{"type": "code", "content": "the question's"}],
         "options": [{"id": "a", "label": "A"},
                     {"id": "b", "label": "B", "context": [{"type": "markdown", "content": "the option's"}]}]},
        {"id": "t", "text": "?", "kind": "text", "context": [{"type": "image", "path": "/pictures/p.png"}]}
    ]}"#;

    /// What a draw of a pane 10 lines high and 20 columns wide showed: 40
    /// lines of content 50 columns wide.
    const PANE: Page = Page {
        rows: Extent {
            shown: 10,
            total: 40,
        },
        columns: Extent {
            shown: 20,
            total: 50,
        },
    };

    /// The session with `WITH_CONTEXT`, its pane drawn as `PANE`.
    fn with_pane() -> SessionState {
        let mut state = SessionState::new(
            "batch",
            serde_json::from_str(WITH_CONTEXT).expect("test session parses"),
            None,
        );
        state.set_pane_page(Some(PANE));
        state
    }

    fn top(state: &SessionState) -> usize {
        state.pane_offset().top
    }

    #[test]
    fn tab_gives_the_pane_the_focus_and_tab_or_esc_give_it_back() {
        let mut state = with_pane();

        for back in [code(KeyCode::Tab), code(KeyCode::Esc)] {
            assert_eq!(state.handle(code(KeyCode::Tab)), Effect::None);
            assert!(state.pane_focused());
            assert_eq!(state.handle(back), Effect::None);
            assert!(!state.pane_focused(), "{back:?}");
            assert_eq!((state.tab(), state.row()), (0, 0), "the cursor stays");
        }
    }

    #[test]
    fn tab_does_nothing_where_no_pane_shows() {
        let mut state = state();

        state.handle(code(KeyCode::Tab));
        assert!(!state.pane_focused());

        let mut state = with_pane();
        state.set_pane_page(None);
        state.handle(code(KeyCode::Tab));
        assert!(!state.pane_focused(), "a hint is no pane");
    }

    #[test]
    fn tab_never_types_into_a_field() {
        let mut state = state();
        press(&mut state, "jjab");

        assert_eq!(state.handle(code(KeyCode::Tab)), Effect::None);

        assert_eq!(state.field_text().as_deref(), Some("ab"));
    }

    #[test]
    fn the_focused_pane_scrolls_by_line_page_and_to_its_ends() {
        let mut state = with_pane();
        state.handle(code(KeyCode::Tab));

        let mut scrolled = |event: KeyEvent| {
            assert_eq!(state.handle(event), Effect::None);
            state.pane_offset()
        };
        assert_eq!(scrolled(code(KeyCode::Char('j'))).top, 1);
        assert_eq!(scrolled(code(KeyCode::Down)).top, 2);
        assert_eq!(scrolled(code(KeyCode::Char('k'))).top, 1);
        assert_eq!(scrolled(code(KeyCode::PageDown)).top, 11);
        assert_eq!(scrolled(ctrl('d')).top, 21);
        assert_eq!(scrolled(code(KeyCode::Char('G'))).top, 30);
        assert_eq!(
            scrolled(code(KeyCode::Char('j'))).top,
            30,
            "stops at the end"
        );
        assert_eq!(scrolled(ctrl('u')).top, 20);
        assert_eq!(scrolled(code(KeyCode::Home)).top, 0);
        assert_eq!(scrolled(code(KeyCode::End)).top, 30);
        assert_eq!(scrolled(code(KeyCode::Char('g'))).top, 0);
        assert_eq!(scrolled(code(KeyCode::Char('l'))).left, 4);
        assert_eq!(scrolled(code(KeyCode::Right)).left, 8);
        assert_eq!(scrolled(code(KeyCode::Left)).left, 4);
        assert_eq!(scrolled(code(KeyCode::Char('h'))).left, 0);
        assert_eq!((state.tab(), state.row()), (0, 0), "the question stays");
    }

    #[test]
    fn the_focused_pane_takes_every_other_key_but_help_z_and_o() {
        let mut state = with_pane();
        state.handle(code(KeyCode::Tab));

        for ignored in ['1', 'n', ' ', 'L', 'i', 'x'] {
            assert_eq!(press(&mut state, &ignored.to_string()), Effect::None);
        }
        assert_eq!(state.handle(code(KeyCode::Enter)), Effect::None);
        assert_eq!(state.answers()[0].selected, Vec::<String>::new());
        assert_eq!(state.focus(), Focus::None, "no note was opened");
        assert!(state.pane_focused());

        press(&mut state, "z");
        assert!(state.full_screen(), "z shows the entry full screen");
        press(&mut state, "z");
        assert!(!state.full_screen());
        press(&mut state, "?");
        assert!(state.help());
        press(&mut state, "x");
        assert!(
            state.pane_focused(),
            "a key that closes the help does no more"
        );
    }

    #[test]
    fn q_in_the_focused_pane_gives_the_focus_back_and_ctrl_c_quits() {
        let mut state = with_pane();
        press(&mut state, "jjown");
        state.handle(code(KeyCode::Tab));

        assert_eq!(press(&mut state, "q"), Effect::None);
        assert!(!state.pane_focused());
        assert_eq!(
            state.field_text().as_deref(),
            Some("own"),
            "back in the field, without a q typed"
        );

        state.handle(code(KeyCode::Tab));
        assert_eq!(state.handle(ctrl('c')), Effect::Quit);
    }

    #[test]
    fn the_full_screen_view_takes_every_key_like_a_pager() {
        let mut state = with_pane();
        press(&mut state, "z");
        assert!(state.full_screen());
        assert!(state.pane_focused(), "the keys go to the entry shown");

        assert_eq!(state.handle(code(KeyCode::Enter)), Effect::None);
        for ignored in ["1", "n", " ", "L", "i"] {
            assert_eq!(press(&mut state, ignored), Effect::None, "{ignored}");
        }
        assert_eq!(press(&mut state, "j"), Effect::None);

        assert_eq!(top(&state), 1, "j scrolls the entry");
        assert_eq!((state.tab(), state.row()), (0, 0), "the cursor stays");
        assert_eq!(state.focus(), Focus::None, "no note was opened");
        assert_eq!(state.answers()[0].selected, Vec::<String>::new());
        assert!(state.full_screen());
        press(&mut state, "?");
        assert!(state.help());
    }

    #[test]
    fn z_esc_tab_and_q_leave_the_full_screen_view_for_where_the_focus_was() {
        for back in [
            code(KeyCode::Char('z')),
            code(KeyCode::Esc),
            code(KeyCode::Tab),
            code(KeyCode::Char('q')),
        ] {
            let mut state = with_pane();
            press(&mut state, "z");
            assert_eq!(state.handle(back), Effect::None, "{back:?} does not quit");
            assert_eq!(
                (state.full_screen(), state.pane_focused()),
                (false, false),
                "{back:?} returns to the question"
            );

            state.handle(code(KeyCode::Tab));
            press(&mut state, "z");
            state.handle(back);
            assert_eq!(
                (state.full_screen(), state.pane_focused()),
                (false, true),
                "{back:?} returns to the pane"
            );
        }
    }

    #[test]
    fn the_full_screen_view_returns_to_the_field_it_was_reached_from() {
        let mut state = with_pane();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Tab));
        press(&mut state, "z");

        state.handle(code(KeyCode::Esc));
        state.handle(code(KeyCode::Esc));

        assert_eq!(state.focus(), Focus::Field);
        press(&mut state, "c");
        assert_eq!(state.field_text().as_deref(), Some("abc"));
    }

    #[test]
    fn a_pane_that_goes_away_ends_the_full_screen_view() {
        let mut state = with_pane();
        press(&mut state, "z");

        state.set_pane_page(None);

        assert_eq!((state.full_screen(), state.pane_focused()), (false, false));
        press(&mut state, "j");
        assert_eq!(state.row(), 1, "keys move the cursor again");
    }

    #[test]
    fn o_opens_the_image_from_the_focused_pane() {
        let mut state = with_pane();
        state.handle(code(KeyCode::Right));
        state.set_pane_page(Some(PANE));
        state.handle(code(KeyCode::Tab));

        assert_eq!(
            press(&mut state, "o"),
            Effect::OpenImage("/pictures/p.png".into())
        );
    }

    #[test]
    fn leaving_the_pane_returns_to_the_field_as_it_was() {
        let mut state = with_pane();
        press(&mut state, "jjown");
        state.handle(code(KeyCode::Left));

        state.handle(code(KeyCode::Tab));
        assert_eq!(press(&mut state, "j"), Effect::None, "j scrolls the pane");
        state.handle(code(KeyCode::Esc));

        assert_eq!(state.focus(), Focus::Field);
        assert_eq!(state.field_cursor(), Some((0, 2)));
        press(&mut state, "e");
        assert_eq!(state.field_text().as_deref(), Some("owen"));
    }

    #[test]
    fn page_keys_scroll_the_pane_from_the_question_and_from_a_field() {
        let mut state = with_pane();

        state.handle(code(KeyCode::PageDown));
        assert_eq!(top(&state), 10);
        state.handle(ctrl('d'));
        assert_eq!(top(&state), 20);
        assert_eq!(press(&mut state, "j"), Effect::None);
        assert_eq!(state.row(), 1, "j still moves the cursor");

        // The own answer shows the question's entry again.
        press(&mut state, "j");
        assert_eq!(state.focus(), Focus::Field);
        press(&mut state, "ab");
        state.handle(code(KeyCode::Left));
        state.handle(ctrl('u'));
        assert_eq!(top(&state), 10);
        state.handle(code(KeyCode::PageUp));
        assert_eq!(top(&state), 0);
        assert_eq!(
            state.field_text().as_deref(),
            Some("ab"),
            "ctrl-u did not undo"
        );
        state.handle(ctrl('d'));
        assert_eq!(
            state.field_text().as_deref(),
            Some("ab"),
            "nor ctrl-d delete"
        );
    }

    #[test]
    fn without_a_pane_fields_keep_their_own_ctrl_keys() {
        let mut state = state();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Left));

        assert_eq!(state.handle(ctrl('d')), Effect::DraftChanged);

        assert_eq!(state.field_text().as_deref(), Some("a"), "ctrl-d deleted");
        assert_eq!(state.handle(code(KeyCode::PageDown)), Effect::None);
    }

    #[test]
    fn each_entry_keeps_its_own_position() {
        let mut state = with_pane();
        state.handle(code(KeyCode::PageDown));
        assert_eq!(top(&state), 10);

        press(&mut state, "j");
        assert_eq!(top(&state), 0, "the option's entry starts at its top");
        state.handle(code(KeyCode::PageDown));
        state.handle(code(KeyCode::PageDown));
        assert_eq!(top(&state), 20);

        press(&mut state, "k");
        assert_eq!(top(&state), 10, "back on the question's entry");
        state.handle(code(KeyCode::Right));
        state.handle(code(KeyCode::Esc));
        state.handle(code(KeyCode::Left));
        assert_eq!(top(&state), 10, "kept while moving between questions");
        press(&mut state, "j");
        assert_eq!(top(&state), 20, "and the option's too");
    }

    #[test]
    fn a_pane_that_goes_away_takes_the_focus_with_it() {
        let mut state = with_pane();
        state.handle(code(KeyCode::Tab));

        state.set_pane_page(None);

        assert!(!state.pane_focused());
        press(&mut state, "j");
        assert_eq!(state.row(), 1, "keys move the cursor again");
    }

    #[test]
    fn keys_without_a_meaning_change_nothing() {
        let mut state = state();

        assert_eq!(press(&mut state, "w"), Effect::None);
        assert_eq!(state.handle(code(KeyCode::F(5))), Effect::None);
        assert_eq!(state.handle(ctrl('j')), Effect::None);
        assert_eq!((state.tab(), state.row()), (0, 0));
    }

    // ---------------------------------------------------------------
    // Drafts and the review list
    // ---------------------------------------------------------------

    #[test]
    fn a_draft_restores_answers_and_position() {
        let mut state = state();
        press(&mut state, "1jjjown");
        let draft = state.to_draft();

        let restored = SessionState::new(
            "batch",
            serde_json::from_str(SESSION).expect("parses"),
            Some(&draft),
        );

        assert_eq!(restored.answers(), state.answers());
        assert_eq!(restored.tab(), 1);
    }

    #[test]
    fn restoring_drops_what_no_longer_fits() {
        let draft = SessionResult::draft(
            "batch",
            "gone",
            vec![
                Answer {
                    selected: vec!["a".into(), "b".into(), "vanished".into()],
                    ..Answer::new("single")
                },
                Answer {
                    custom: Some("x".into()),
                    ..Answer::new("no-longer-asked")
                },
            ],
        );

        let restored = SessionState::new(
            "batch",
            serde_json::from_str(SESSION).expect("parses"),
            Some(&draft),
        );

        assert_eq!(
            answer(&restored, "single").selected,
            ["a"],
            "one option for a single"
        );
        assert_eq!(restored.tab(), 0, "an unknown question starts at the top");
        assert_eq!(restored.answers().len(), 4);
    }

    #[test]
    fn the_review_is_saved_as_the_last_question() {
        let mut state = state();
        go_to_tab(&mut state, 4);

        assert_eq!(state.to_draft().current.as_deref(), Some("plain"));
    }

    #[test]
    fn a_replaced_session_keeps_the_answers_that_still_fit() {
        let mut state = state();
        press(&mut state, "1");
        let replacement = SESSION.replace(r#"{"id": "a", "label": "A"}, "#, "");

        state.replace_session(serde_json::from_str(&replacement).expect("parses"));

        assert!(
            answer(&state, "single").selected.is_empty(),
            "option a is gone"
        );
        assert_eq!(state.id(), "batch");
        assert_eq!(
            state.session().questions[0].options.as_ref().map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn each_replacement_counts_as_a_new_revision() {
        let mut state = state();
        assert_eq!(state.revision(), 0);

        state.replace_session(serde_json::from_str(SESSION).expect("parses"));
        assert_eq!(state.revision(), 1, "even an unchanged file counts");
        state.replace_session(serde_json::from_str(SESSION).expect("parses"));
        assert_eq!(state.revision(), 2);
    }

    #[test]
    fn reports_each_questions_state_for_the_tabs_and_the_review() {
        let mut state = state();
        press(&mut state, "lnwhy");
        state.handle(code(KeyCode::Esc));
        go_to_tab(&mut state, 3);
        press(&mut state, "1");

        assert_eq!(
            state.question_states(),
            [
                QuestionState {
                    answer: AnswerState::Defaulted,
                    note: false
                },
                QuestionState {
                    answer: AnswerState::Skipped,
                    note: true
                },
                QuestionState {
                    answer: AnswerState::Skipped,
                    note: false
                },
                QuestionState {
                    answer: AnswerState::Answered,
                    note: false
                },
            ]
        );
    }

    #[test]
    fn keys_that_change_nothing_in_a_field_save_nothing() {
        let mut state = state();
        press(&mut state, "jja");

        assert_eq!(state.handle(code(KeyCode::F(5))), Effect::None);
        // ctrl-m is another way to a line break, refused in a one-line field.
        assert_eq!(state.handle(ctrl('m')), Effect::None);
        assert_eq!(state.field_text().as_deref(), Some("a"));
    }

    #[test]
    fn the_review_has_no_n_key_and_its_fields_no_counter() {
        let mut state = state();
        go_to_tab(&mut state, 4);

        assert_eq!(press(&mut state, "n"), Effect::None);
        assert_eq!(state.focus(), Focus::None);
        for row in [Row::Note, Row::Reject] {
            review_row(&mut state, row);
            assert_eq!(
                (state.focus(), state.field_length()),
                (Focus::Field, None),
                "{row:?}"
            );
        }
    }

    #[test]
    fn a_key_that_changes_nothing_in_a_note_saves_nothing() {
        let mut state = state();
        press(&mut state, "n");

        assert_eq!(state.handle(code(KeyCode::F(5))), Effect::None);
    }

    #[test]
    fn exposes_the_editors_for_drawing() {
        let mut state = state();
        assert_eq!(
            (state.field_text(), state.note_text(), state.field_cursor()),
            (None, None, None)
        );

        press(&mut state, "jjab");
        state.handle(code(KeyCode::Left));
        assert_eq!(state.field_cursor(), Some((0, 1)));
        assert_eq!(state.field_length(), None, "this own answer has no length");

        state.handle(code(KeyCode::Esc));
        press(&mut state, "nx");
        assert_eq!(state.note_text().as_deref(), Some("x"));
        assert_eq!(state.field_cursor(), Some((0, 1)));
    }
}
