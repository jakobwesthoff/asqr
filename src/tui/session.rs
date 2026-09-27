// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The state of one session while the person answers it (spec sections
//! 5.2, 7.2 and 7.3), driven by key events and nothing else.
//!
//! The working answers live here in the draft's shape. [`SessionState::handle`]
//! returns an [`Effect`] for everything the state cannot do itself: saving
//! the draft, finishing the session, quitting, opening an image. The
//! running app carries those out.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::TextArea;

pub use crate::format::Length;
use crate::format::{
    Answer, Kind, Question, Session, SessionResult, is_answered, result_answers,
    unanswered_required,
};

/// What the running app has to do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    /// The answers or the current question changed; save the draft.
    DraftChanged,
    /// The person confirmed the submit; these are the result's answers.
    Submit(Vec<Answer>),
    /// The person rejected the session, with a reason if they gave one.
    Reject(Option<String>),
    Quit,
    OpenImage(String),
    OpenSessionList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Custom,
    Note,
}

/// How many questions a submit would send as what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub answered: usize,
    pub skipped: usize,
    pub defaulted: usize,
}

/// What the keys currently act on.
#[derive(Debug, Clone)]
pub enum Mode {
    Browse,
    Editing {
        field: Field,
        editor: TextArea<'static>,
    },
    ConfirmSubmit(Counts),
    /// The editor holds the optional reason.
    ConfirmReject(TextArea<'static>),
    Help,
}

/// Modes compare by what they show; editors by their text.
impl PartialEq for Mode {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Mode::Browse, Mode::Browse) | (Mode::Help, Mode::Help) => true,
            (
                Mode::Editing {
                    field: a,
                    editor: x,
                },
                Mode::Editing {
                    field: b,
                    editor: y,
                },
            ) => a == b && x.lines() == y.lines(),
            (Mode::ConfirmSubmit(a), Mode::ConfirmSubmit(b)) => a == b,
            (Mode::ConfirmReject(x), Mode::ConfirmReject(y)) => x.lines() == y.lines(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerState {
    Answered,
    /// Answered by an untouched default.
    Defaulted,
    Skipped,
}

/// A question's line in the question list.
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
    custom: String,
    note: String,
    /// The person edited the question; a default then no longer counts as
    /// untouched (spec section 5.2).
    touched: bool,
}

#[derive(Debug, Clone)]
pub struct SessionState {
    id: String,
    session: Session,
    working: Vec<Working>,
    current: usize,
    cursor: usize,
    mode: Mode,
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
        let current = draft
            .and_then(|draft| draft.current.as_deref())
            .and_then(|current| {
                session
                    .questions
                    .iter()
                    .position(|question| question.id == current)
            })
            .unwrap_or(0);
        SessionState {
            id: id.into(),
            session,
            working,
            current,
            cursor: 0,
            mode: Mode::Browse,
            message: None,
            image_full_screen: false,
        }
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

    /// The index of the current question.
    pub fn current(&self) -> usize {
        self.current
    }

    /// The option under the cursor in the current question.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Feedback on the last key, shown until the next one.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn image_full_screen(&self) -> bool {
        self.image_full_screen
    }

    fn question(&self) -> &Question {
        &self.session.questions[self.current]
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

    pub fn to_draft(&self) -> SessionResult {
        SessionResult::draft(&self.id, &self.question().id, self.answers())
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

    /// The text in the open editor, if one is open.
    pub fn editor_text(&self) -> Option<String> {
        match &self.mode {
            Mode::Editing { editor, .. } | Mode::ConfirmReject(editor) => {
                Some(editor.lines().join("\n"))
            }
            _ => None,
        }
    }

    /// The length of the text in the open editor and the limits it is
    /// counted against, when the field has limits.
    pub fn editor_length(&self) -> Option<(usize, Length)> {
        let Mode::Editing { field, editor } = &self.mode else {
            return None;
        };
        let limits = self.limits(*field)?;
        Some((text_length(editor), limits))
    }

    fn limits(&self, field: Field) -> Option<Length> {
        let question = self.question();
        match (field, question.kind) {
            (Field::Note, _) => None,
            (Field::Custom, Kind::Text) => question.length,
            (Field::Custom, _) => question
                .custom
                .as_ref()
                .and_then(|custom| custom.length())
                .copied(),
        }
    }

    fn is_multiline(&self, field: Field) -> bool {
        let question = self.question();
        match (field, question.kind) {
            (Field::Note, _) | (Field::Custom, Kind::Text) => true,
            (Field::Custom, _) => question
                .custom
                .as_ref()
                .is_some_and(|custom| custom.is_multiline()),
        }
    }

    /// Handles one key.
    pub fn handle(&mut self, key: KeyEvent) -> Effect {
        self.message = None;
        match std::mem::replace(&mut self.mode, Mode::Browse) {
            Mode::Browse => self.browse(key),
            Mode::Help => Effect::None,
            Mode::Editing { field, editor } => self.edit(field, editor, key),
            Mode::ConfirmSubmit(counts) => match key.code {
                KeyCode::Enter => Effect::Submit(result_answers(&self.session, &self.answers())),
                KeyCode::Esc => Effect::None,
                _ => {
                    self.mode = Mode::ConfirmSubmit(counts);
                    Effect::None
                }
            },
            Mode::ConfirmReject(reason) => self.reject(reason, key),
        }
    }

    fn browse(&mut self, key: KeyEvent) -> Effect {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return if key.code == KeyCode::Char('c') {
                Effect::Quit
            } else {
                Effect::None
            };
        }
        let options = self.question().options.as_ref().map_or(0, Vec::len);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor = (self.cursor + 1).min(options.saturating_sub(1));
                Effect::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                Effect::None
            }
            KeyCode::Tab | KeyCode::Char('J') => self.go_to(self.current + 1),
            KeyCode::BackTab | KeyCode::Char('K') => match self.current.checked_sub(1) {
                Some(previous) => self.go_to(previous),
                None => Effect::None,
            },
            KeyCode::Char(' ') | KeyCode::Enter if self.question().kind == Kind::Text => {
                self.open_editor(Field::Custom)
            }
            KeyCode::Char(' ') | KeyCode::Enter => self.toggle(self.cursor),
            KeyCode::Char(digit @ '1'..='9') => {
                let index = digit as usize - '1' as usize;
                if index < options {
                    self.cursor = index;
                    // A digit picks: on a single question it never
                    // deselects the option it names.
                    let picked = self.question().kind == Kind::Single
                        && option_ids(self.question()).nth(index).is_some_and(|id| {
                            self.working[self.current].selected == [id.to_owned()]
                        });
                    if picked {
                        self.working[self.current].touched = true;
                        Effect::DraftChanged
                    } else {
                        self.toggle(index)
                    }
                } else {
                    Effect::None
                }
            }
            KeyCode::Char('c') => self.open_editor(Field::Custom),
            KeyCode::Char('n') => self.open_editor(Field::Note),
            KeyCode::Char('S') => self.ask_submit(),
            KeyCode::Char('X') => {
                self.mode = Mode::ConfirmReject(TextArea::default());
                Effect::None
            }
            KeyCode::Char('q') => Effect::Quit,
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                Effect::None
            }
            KeyCode::Char('L') => Effect::OpenSessionList,
            KeyCode::Char('o') => match &self.question().image {
                Some(image) => Effect::OpenImage(image.clone()),
                None => Effect::None,
            },
            KeyCode::Char('z') => {
                self.image_full_screen = self.question().image.is_some() && !self.image_full_screen;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn go_to(&mut self, index: usize) -> Effect {
        if index >= self.session.questions.len() {
            return Effect::None;
        }
        self.current = index;
        self.cursor = 0;
        self.image_full_screen = false;
        Effect::DraftChanged
    }

    fn toggle(&mut self, index: usize) -> Effect {
        let question = &self.session.questions[self.current];
        let id = option_ids(question)
            .nth(index)
            .map(str::to_owned)
            .expect("callers only toggle options the question has");
        let working = &mut self.working[self.current];
        match question.kind {
            Kind::Single if working.selected == [id.clone()] => working.selected.clear(),
            Kind::Single => {
                working.selected = vec![id];
                working.custom.clear();
            }
            _ if working.selected.contains(&id) => {
                working.selected.retain(|selected| *selected != id)
            }
            _ => {
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
        }
        working.touched = true;
        Effect::DraftChanged
    }

    fn open_editor(&mut self, field: Field) -> Effect {
        let question = self.question();
        let allowed = match field {
            Field::Note => question.note,
            Field::Custom => {
                question.kind == Kind::Text
                    || question
                        .custom
                        .as_ref()
                        .is_some_and(|custom| custom.is_enabled())
            }
        };
        if !allowed {
            self.message = Some(
                match field {
                    Field::Note => "this question takes no note",
                    Field::Custom => "this question takes no typed answer",
                }
                .to_owned(),
            );
            return Effect::None;
        }
        let working = &self.working[self.current];
        let text = match field {
            Field::Custom => &working.custom,
            Field::Note => &working.note,
        };
        self.mode = Mode::Editing {
            field,
            editor: editor_with(text),
        };
        Effect::None
    }

    fn edit(&mut self, field: Field, mut editor: TextArea<'static>, key: KeyEvent) -> Effect {
        if key.code == KeyCode::Esc {
            let text = editor.lines().join("\n");
            let working = &mut self.working[self.current];
            match field {
                Field::Custom => {
                    // A single question is answered by an option or by
                    // typed text, never both.
                    if self.session.questions[self.current].kind == Kind::Single
                        && !text.trim().is_empty()
                    {
                        working.selected.clear();
                    }
                    working.custom = text;
                }
                Field::Note => working.note = text,
            }
            working.touched = true;
            return Effect::DraftChanged;
        }

        // The key is tried on a copy, so input that breaks a rule of the
        // field (a line break in a single-line field, text beyond the hard
        // limit) is dropped as a whole.
        let before = editor.clone();
        editor.input(key);
        if !self.is_multiline(field) && editor.lines().len() > 1 {
            editor = before;
        } else if let Some(max) = self.limits(field).and_then(|limits| limits.max)
            && text_length(&editor) > max as usize
        {
            editor = before;
            self.message = Some(format!("at most {max} characters"));
        }
        self.mode = Mode::Editing { field, editor };
        Effect::None
    }

    fn ask_submit(&mut self) -> Effect {
        let answers = self.answers();
        let missing = unanswered_required(&self.session, &answers);
        if let Some(first) = missing.first() {
            self.message = Some(format!(
                "answer the required questions first: {}",
                missing.join(", ")
            ));
            let first = self
                .session
                .questions
                .iter()
                .position(|question| question.id == *first)
                .expect("unanswered_required returns ids of this session");
            return self.go_to(first);
        }
        let result = result_answers(&self.session, &answers);
        self.mode = Mode::ConfirmSubmit(Counts {
            answered: result.iter().filter(|answer| !answer.skipped).count(),
            skipped: result.iter().filter(|answer| answer.skipped).count(),
            defaulted: result.iter().filter(|answer| answer.defaulted).count(),
        });
        Effect::None
    }

    fn reject(&mut self, mut reason: TextArea<'static>, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Esc => Effect::None,
            KeyCode::Enter => {
                let reason = reason.lines().join(" ").trim().to_owned();
                Effect::Reject((!reason.is_empty()).then_some(reason))
            }
            _ => {
                reason.input(key);
                self.mode = Mode::ConfirmReject(reason);
                Effect::None
            }
        }
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
    editor.move_cursor(ratatui_textarea::CursorMove::Bottom);
    editor.move_cursor(ratatui_textarea::CursorMove::End);
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
        {"id": "multi", "text": "?", "kind": "multi", "max": 2, "custom": {"multiline": true},
         "options": [{"id": "x", "label": "X"}, {"id": "y", "label": "Y"}, {"id": "z", "label": "Z"}]},
        {"id": "text", "text": "?", "kind": "text", "length": {"max": 5}, "required": true},
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

    fn ch(c: char) -> KeyEvent {
        code(KeyCode::Char(c))
    }

    fn press(state: &mut SessionState, keys: &str) -> Effect {
        let mut last = Effect::None;
        for c in keys.chars() {
            last = state.handle(ch(c));
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

    // ---------------------------------------------------------------
    // Navigation
    // ---------------------------------------------------------------

    #[test]
    fn starts_on_the_first_question_and_option() {
        let state = state();

        assert_eq!((state.current(), state.cursor()), (0, 0));
        assert_eq!(state.mode(), &Mode::Browse);
    }

    #[test]
    fn moves_within_the_options_and_stops_at_the_ends() {
        let mut state = state();

        press(&mut state, "jjj");
        assert_eq!(state.cursor(), 1);
        state.handle(code(KeyCode::Up));
        state.handle(code(KeyCode::Up));
        assert_eq!(state.cursor(), 0);
        state.handle(code(KeyCode::Down));
        assert_eq!(state.cursor(), 1);
        press(&mut state, "k");
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn moves_between_questions_and_saves_the_position() {
        let mut state = state();

        assert_eq!(state.handle(code(KeyCode::Tab)), Effect::DraftChanged);
        assert_eq!(state.current(), 1);
        press(&mut state, "JJJJ");
        assert_eq!(state.current(), 3, "stops at the last question");
        state.handle(code(KeyCode::BackTab));
        press(&mut state, "K");
        assert_eq!(state.current(), 1);
        assert_eq!(
            state.cursor(),
            0,
            "each question starts on its first option"
        );
        assert_eq!(state.to_draft().current.as_deref(), Some("multi"));
    }

    #[test]
    fn moving_past_either_end_changes_nothing() {
        let mut state = state();

        assert_eq!(press(&mut state, "K"), Effect::None);
        assert_eq!(state.current(), 0);
    }

    // ---------------------------------------------------------------
    // Selecting
    // ---------------------------------------------------------------

    #[test]
    fn a_default_counts_until_the_person_edits_the_question() {
        let mut state = state();
        assert_eq!(
            answer(&state, "single"),
            Answer {
                selected: vec!["b".into()],
                defaulted: true,
                ..Answer::new("single")
            }
        );

        // Choosing another option and then the default again is still an
        // edit.
        press(&mut state, " ");
        state.handle(code(KeyCode::Down));
        state.handle(code(KeyCode::Enter));

        assert_eq!(answer(&state, "single").selected, ["b"]);
        assert!(!answer(&state, "single").defaulted);
    }

    #[test]
    fn single_selects_one_option_and_toggles_it_off_again() {
        let mut state = state();

        assert_eq!(press(&mut state, " "), Effect::DraftChanged);
        assert_eq!(answer(&state, "single").selected, ["a"]);
        press(&mut state, " ");
        assert!(answer(&state, "single").selected.is_empty());
    }

    #[test]
    fn digits_pick_an_option_directly() {
        let mut state = state();

        press(&mut state, "2");
        assert_eq!(
            (state.cursor(), answer(&state, "single").selected),
            (1, vec!["b".to_owned()])
        );
        assert_eq!(
            press(&mut state, "9"),
            Effect::None,
            "there is no ninth option"
        );
    }

    #[test]
    fn multi_toggles_options_up_to_max() {
        let mut state = state();
        press(&mut state, "J");

        press(&mut state, "31");
        assert_eq!(
            answer(&state, "multi").selected,
            ["x", "z"],
            "kept in option order"
        );
        assert_eq!(press(&mut state, "2"), Effect::None);
        assert_eq!(state.message(), Some("at most 2 options"));
        press(&mut state, "1");
        assert_eq!(answer(&state, "multi").selected, ["z"]);
    }

    #[test]
    fn selecting_does_nothing_on_a_text_question_but_opens_its_editor() {
        let mut state = state();
        press(&mut state, "JJ");

        press(&mut state, "1");
        assert_eq!(state.mode(), &Mode::Browse);
        state.handle(code(KeyCode::Enter));
        assert!(matches!(
            state.mode(),
            Mode::Editing {
                field: Field::Custom,
                ..
            }
        ));
    }

    // ---------------------------------------------------------------
    // Text fields
    // ---------------------------------------------------------------

    #[test]
    fn typed_custom_text_replaces_the_selection_of_a_single() {
        let mut state = state();
        press(&mut state, "1c");

        press(&mut state, "own");
        assert_eq!(state.handle(code(KeyCode::Esc)), Effect::DraftChanged);

        let single = answer(&state, "single");
        assert_eq!(
            (single.selected.len(), single.custom.as_deref()),
            (0, Some("own"))
        );
        // And selecting clears the text again.
        press(&mut state, "1");
        assert_eq!(answer(&state, "single").custom, None);
    }

    #[test]
    fn every_key_but_esc_goes_to_the_editor() {
        let mut state = state();
        press(&mut state, "c");

        press(&mut state, "qSXJ?1");
        state.handle(code(KeyCode::Tab));
        state.handle(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        state.handle(code(KeyCode::Esc));

        assert_eq!(state.current(), 0);
        assert_eq!(answer(&state, "single").custom.as_deref(), Some("qSXJ?1\t"));
    }

    #[test]
    fn a_single_line_custom_entry_ignores_enter() {
        let mut state = state();
        press(&mut state, "cab");

        state.handle(code(KeyCode::Enter));
        press(&mut state, "c");
        state.handle(code(KeyCode::Esc));

        assert_eq!(answer(&state, "single").custom.as_deref(), Some("abc"));
    }

    #[test]
    fn a_multiline_custom_entry_takes_new_lines() {
        let mut state = state();
        press(&mut state, "Jca");

        state.handle(code(KeyCode::Enter));
        press(&mut state, "b");
        state.handle(code(KeyCode::Esc));

        assert_eq!(answer(&state, "multi").custom.as_deref(), Some("a\nb"));
    }

    #[test]
    fn the_hard_limit_refuses_more_input() {
        let mut state = state();
        press(&mut state, "JJc");

        press(&mut state, "123456");

        assert_eq!(state.editor_text().as_deref(), Some("12345"));
        assert_eq!(state.message(), Some("at most 5 characters"));
        assert_eq!(
            state.editor_length(),
            Some((
                5,
                Length {
                    target: None,
                    warn: None,
                    max: Some(5)
                }
            ))
        );
        state.handle(code(KeyCode::Esc));
        assert_eq!(answer(&state, "text").custom.as_deref(), Some("12345"));
    }

    #[test]
    fn notes_are_multiline_and_count_as_an_edit() {
        let mut state = state();
        press(&mut state, "na");
        state.handle(code(KeyCode::Enter));
        press(&mut state, "b");
        state.handle(code(KeyCode::Esc));

        let single = answer(&state, "single");
        assert_eq!(single.note.as_deref(), Some("a\nb"));
        assert!(!single.defaulted, "the person edited the question");
    }

    #[test]
    fn questions_without_notes_or_custom_entry_say_so() {
        let mut state = state();
        press(&mut state, "JJJ");

        assert_eq!(press(&mut state, "n"), Effect::None);
        assert_eq!(state.message(), Some("this question takes no note"));
        press(&mut state, "c");
        assert_eq!(state.message(), Some("this question takes no typed answer"));
        assert_eq!(state.mode(), &Mode::Browse);
    }

    #[test]
    fn a_message_lasts_until_the_next_key() {
        let mut state = state();
        press(&mut state, "JJJn");

        press(&mut state, "k");

        assert_eq!(state.message(), None);
    }

    // ---------------------------------------------------------------
    // Submitting, rejecting, quitting
    // ---------------------------------------------------------------

    #[test]
    fn required_questions_block_submit_and_take_the_cursor() {
        let mut state = state();

        // The jump to the question is a change of position, saved with the
        // draft.
        assert_eq!(press(&mut state, "S"), Effect::DraftChanged);

        assert_eq!(state.mode(), &Mode::Browse);
        assert_eq!(state.current(), 2);
        assert_eq!(
            state.message(),
            Some("answer the required questions first: text")
        );
    }

    #[test]
    fn submit_asks_first_with_the_counts() {
        let mut state = state();
        press(&mut state, "JJcok");
        state.handle(code(KeyCode::Esc));

        press(&mut state, "S");
        assert_eq!(
            state.mode(),
            &Mode::ConfirmSubmit(Counts {
                answered: 2,
                skipped: 2,
                defaulted: 1
            })
        );
        assert_eq!(state.handle(code(KeyCode::Esc)), Effect::None);
        assert_eq!(state.mode(), &Mode::Browse);

        press(&mut state, "S");
        press(&mut state, "x");
        let Effect::Submit(answers) = state.handle(code(KeyCode::Enter)) else {
            panic!("enter confirms the submit");
        };
        assert_eq!(answers.len(), 4);
        assert_eq!(answers[1], Answer::skipped("multi"));
        assert_eq!(answers[2].custom.as_deref(), Some("ok"));
    }

    #[test]
    fn rejecting_asks_for_an_optional_reason() {
        let mut state = state();

        press(&mut state, "X");
        assert!(matches!(state.mode(), Mode::ConfirmReject(_)));
        press(&mut state, "  out of date ");
        assert_eq!(
            state.handle(code(KeyCode::Enter)),
            Effect::Reject(Some("out of date".into()))
        );

        press(&mut state, "X");
        assert_eq!(state.handle(code(KeyCode::Enter)), Effect::Reject(None));

        press(&mut state, "X");
        state.handle(code(KeyCode::Esc));
        assert_eq!(state.mode(), &Mode::Browse);
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let mut state = state();

        assert_eq!(press(&mut state, "q"), Effect::Quit);
        assert_eq!(
            state.handle(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Effect::Quit
        );
    }

    #[test]
    fn help_closes_on_any_key() {
        let mut state = state();

        press(&mut state, "?");
        assert_eq!(state.mode(), &Mode::Help);
        assert_eq!(
            press(&mut state, "q"),
            Effect::None,
            "the key only closes the help"
        );
        assert_eq!(state.mode(), &Mode::Browse);
    }

    #[test]
    fn l_asks_for_the_session_list() {
        assert_eq!(press(&mut state(), "L"), Effect::OpenSessionList);
    }

    #[test]
    fn images_open_and_toggle_full_screen_where_there_is_one() {
        let mut state = state();

        assert_eq!(press(&mut state, "o"), Effect::None);
        press(&mut state, "z");
        assert!(!state.image_full_screen());

        press(&mut state, "JJJ");
        assert_eq!(
            press(&mut state, "o"),
            Effect::OpenImage("/pictures/p.png".into())
        );
        press(&mut state, "z");
        assert!(state.image_full_screen());
        press(&mut state, "J");
        press(&mut state, "z");
        assert!(!state.image_full_screen());
    }

    #[test]
    fn keys_without_a_meaning_change_nothing() {
        let mut state = state();

        assert_eq!(press(&mut state, "w"), Effect::None);
        assert_eq!(state.handle(code(KeyCode::F(5))), Effect::None);
        assert_eq!(
            state.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            Effect::None
        );
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn knows_its_id_and_session() {
        let state = state();

        assert_eq!(state.id(), "batch");
        assert_eq!(state.session().questions.len(), 4);
    }

    #[test]
    fn has_no_editor_text_outside_an_editor() {
        let mut state = state();
        assert_eq!((state.editor_text(), state.editor_length()), (None, None));

        press(&mut state, "c");
        assert_eq!(state.editor_text().as_deref(), Some(""));
        assert_eq!(
            state.editor_length(),
            None,
            "the custom entry of this question has no limits"
        );
    }

    #[test]
    fn modes_compare_by_what_they_show() {
        let editing = |text: &str| Mode::Editing {
            field: Field::Note,
            editor: editor_with(text),
        };

        assert_eq!(editing("a"), editing("a"));
        assert_ne!(editing("a"), editing("b"));
        assert_ne!(
            editing("a"),
            Mode::Editing {
                field: Field::Custom,
                editor: editor_with("a")
            }
        );
        assert_eq!(
            Mode::ConfirmReject(editor_with("r")),
            Mode::ConfirmReject(editor_with("r"))
        );
        assert_ne!(Mode::ConfirmReject(editor_with("r")), Mode::Help);
    }

    // ---------------------------------------------------------------
    // Drafts
    // ---------------------------------------------------------------

    #[test]
    fn a_draft_restores_answers_and_position() {
        let mut state = state();
        press(&mut state, "1Jcown");
        state.handle(code(KeyCode::Esc));
        let draft = state.to_draft();

        let restored = SessionState::new(
            "batch",
            serde_json::from_str(SESSION).expect("parses"),
            Some(&draft),
        );

        assert_eq!(restored.answers(), state.answers());
        assert_eq!(restored.current(), 1);
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
        assert_eq!(
            restored.current(),
            0,
            "an unknown cursor question starts at the top"
        );
        assert_eq!(restored.answers().len(), 4);
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
        press(&mut state, "J1");
        assert_eq!(answer(&state, "multi").selected, ["x"]);
    }

    #[test]
    fn reports_each_questions_state_for_the_list() {
        let mut state = state();
        press(&mut state, "Jnwhy");
        state.handle(code(KeyCode::Esc));
        press(&mut state, "JJ1");

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
}
