// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The state of one session while the person answers it (spec sections
//! 5.2 and 7.1 to 7.4, ADR 22), driven by key events and nothing else.
//!
//! A session is a row of tabs: one per question and the review last. Each
//! tab is a list of rows: the options and the own answer, a text answer,
//! or on the review the questions, Submit and Reject. Rows that take text
//! are live fields, typed into in place as soon as the cursor lands on
//! them. [`SessionState::handle`] returns an [`Effect`] for everything the
//! state cannot do itself: saving the draft, finishing the session,
//! quitting, opening an image.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::{CursorMove, TextArea};

pub use crate::format::Length;
use crate::format::{Answer, Kind, Question, Session, SessionResult, is_answered, result_answers};

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
    /// The person submitted; these are the result's answers.
    Submit(Vec<Answer>),
    /// The person rejected the session, with a reason if they gave one.
    Reject(Option<String>),
    Quit,
    OpenImage(String),
    OpenSessionList,
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
    /// The reject row with its reason field.
    Reject,
}

impl Row {
    /// Rows that are text fields, focused when the cursor lands on them.
    fn is_field(self) -> bool {
        matches!(self, Row::Own | Row::Answer | Row::Reject)
    }
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
    reject_reason: String,
    help: bool,
    message: Option<String>,
    image_full_screen: bool,
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
            reject_reason: draft
                .and_then(|draft| draft.reason.clone())
                .unwrap_or_default(),
            help: false,
            message: None,
            image_full_screen: false,
        };
        state.arrive();
        state
    }

    /// Takes over a session file that was replaced while it was open,
    /// keeping the answers that still fit (review F1).
    pub fn replace_session(&mut self, session: Session) {
        let draft = self.to_draft();
        *self = SessionState::new(self.id.clone(), session, Some(&draft));
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

    pub fn image_full_screen(&self) -> bool {
        self.image_full_screen
    }

    pub fn reject_reason(&self) -> &str {
        &self.reject_reason
    }

    /// The question of the current tab; the review has none.
    pub fn question(&self) -> Option<&Question> {
        self.session.questions.get(self.tab)
    }

    /// The image shown on the current tab; `o` and `z` act on it. The
    /// option under the cursor shows its own image, so the person can
    /// browse the options before picking; every other row shows the
    /// question's image (spec section 7.7).
    pub fn shown_image(&self) -> Option<&str> {
        let question = self.question()?;
        let option_image = match self.current_row() {
            Some(Row::Option(index)) => question
                .options
                .as_ref()
                .and_then(|options| options[index].image.as_deref()),
            _ => None,
        };
        option_image.or(question.image.as_deref())
    }

    /// Whether the current tab keeps room for an image: when the question
    /// or any of its options has one, even while the row under the cursor
    /// shows none, so the layout does not jump while browsing.
    pub fn has_image_area(&self) -> bool {
        self.question().is_some_and(|question| {
            question.image.is_some()
                || question
                    .options
                    .iter()
                    .flatten()
                    .any(|option| option.image.is_some())
        })
    }

    /// The rows of the current tab.
    pub fn rows(&self) -> Vec<Row> {
        let Some(question) = self.question() else {
            let questions = (0..self.session.questions.len()).map(Row::Question);
            return questions.chain([Row::Submit, Row::Reject]).collect();
        };
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
    /// keeps the reject reason typed so far.
    pub fn to_draft(&self) -> SessionResult {
        let current = self.tab.min(self.session.questions.len().saturating_sub(1));
        let current = self
            .session
            .questions
            .get(current)
            .map_or("", |question| question.id.as_str());
        let mut draft = SessionResult::draft(&self.id, current, self.answers());
        // Kept as typed, so a restored field reads exactly as it was left.
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
        match self.focus {
            Focus::None => self.command(key),
            Focus::Field => self.field_key(key),
            Focus::Note => self.note_key(key),
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
            KeyCode::Char('o') => match self.shown_image() {
                Some(image) => Effect::OpenImage(image.to_owned()),
                None => Effect::None,
            },
            KeyCode::Char('z') => {
                self.image_full_screen = self.shown_image().is_some() && !self.image_full_screen;
                Effect::None
            }
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
            Row::Submit => self.submit(),
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
        self.image_full_screen = false;
        self.arrive();
        Effect::DraftChanged
    }

    /// Focuses the field of the current row, if it is one. Called whenever
    /// the cursor lands on a row.
    fn arrive(&mut self) {
        match self.current_row() {
            Some(row) if row.is_field() => {
                let text = match row {
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
    /// goes through; unanswered questions come back as skipped.
    fn submit(&mut self) -> Effect {
        Effect::Submit(result_answers(&self.session, &self.answers()))
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
                // Only a text answer spans several lines; own answers and
                // the reject reason are one line (ADR 22).
                let multiline = self.current_row() == Some(Row::Answer);
                if !self.edit(key, multiline) {
                    return Effect::None;
                }
                let text = self.editor_text().unwrap_or_default();
                if self.current_row() == Some(Row::Reject) {
                    self.reject_reason = text;
                    return Effect::DraftChanged;
                }
                let working = &mut self.working[self.tab];
                working.custom = text;
                working.touched = true;
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

    const SESSION: &str = r#"{"asqr": 1, "questions": [
        {"id": "single", "text": "?", "kind": "single", "custom": true,
         "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B", "default": true}]},
        {"id": "multi", "text": "?", "kind": "multi", "max": 2, "custom": true,
         "options": [{"id": "x", "label": "X"}, {"id": "y", "label": "Y"}, {"id": "z", "label": "Z"}]},
        {"id": "text", "text": "?", "kind": "text", "length": {"warn": 5}},
        {"id": "plain", "text": "?", "kind": "single", "note": false, "image": "/pictures/p.png",
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
                Row::Reject
            ]
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

        let Effect::Submit(answers) = state.handle(code(KeyCode::Enter)) else {
            panic!("nothing blocks a submit");
        };

        assert_eq!(answers[2], Answer::skipped("text"));
    }

    #[test]
    fn submitting_sends_every_question() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        press(&mut state, "ok");
        state.handle(code(KeyCode::Enter));
        go_to_tab(&mut state, 4);

        let Effect::Submit(answers) = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on Submit submits");
        };

        assert_eq!(answers.len(), 4);
        assert_eq!(answers[1], Answer::skipped("multi"));
        assert_eq!(answers[2].custom.as_deref(), Some("ok"));
    }

    #[test]
    fn the_reject_row_takes_an_optional_reason() {
        let mut state = state();
        go_to_tab(&mut state, 4);
        press(&mut state, "j");
        assert_eq!(state.focus(), Focus::Field);

        press(&mut state, "  out of date ");
        assert_eq!(
            state.handle(code(KeyCode::Enter)),
            Effect::Reject(Some("out of date".into()))
        );

        let mut empty = self::state();
        go_to_tab(&mut empty, 4);
        press(&mut empty, "j");
        assert_eq!(empty.handle(code(KeyCode::Enter)), Effect::Reject(None));
    }

    #[test]
    fn the_reject_reason_is_saved_with_the_draft() {
        let mut state = state();
        go_to_tab(&mut state, 4);
        press(&mut state, "j");

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
        assert!(!state.image_full_screen());

        go_to_tab(&mut state, 3);
        assert_eq!(
            press(&mut state, "o"),
            Effect::OpenImage("/pictures/p.png".into())
        );
        press(&mut state, "z");
        assert!(state.image_full_screen());
        press(&mut state, "l");
        assert!(
            !state.image_full_screen(),
            "another tab ends the full-screen view"
        );
    }

    #[test]
    fn the_image_follows_the_option_under_the_cursor() {
        let session = r#"{"asqr": 1, "questions": [
            {"id": "pictured", "text": "?", "kind": "single", "custom": true,
             "image": "/pictures/question.png",
             "options": [{"id": "a", "label": "A", "image": "/pictures/a.png"}, {"id": "b", "label": "B"}]},
            {"id": "bare", "text": "?", "kind": "multi",
             "options": [{"id": "x", "label": "X", "image": "/pictures/x.png"}, {"id": "y", "label": "Y"}]}
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
        assert!(state.has_image_area());
        press(&mut state, "j");
        assert_eq!(state.shown_image(), None);
        assert!(
            state.has_image_area(),
            "the area stays, so the layout does not jump between options"
        );

        go_to_tab(&mut state, 2);
        assert_eq!((state.shown_image(), state.has_image_area()), (None, false));
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
    fn the_review_takes_no_note_and_its_reject_field_no_counter() {
        let mut state = state();
        go_to_tab(&mut state, 4);

        assert_eq!(press(&mut state, "n"), Effect::None);
        assert_eq!(state.focus(), Focus::None);
        press(&mut state, "j");
        assert_eq!((state.focus(), state.field_length()), (Focus::Field, None));
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
