// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The state of one session while the person answers it (spec sections
//! 4.2 and 7.2 to 7.8, ADR 8), driven by key events and nothing else.
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

use super::scroll::scroll;
use super::text_field::cleaned;
use super::{
    EditRequest, FieldAbout, FieldKey, FieldName, FieldShape, Leave, Notice, Offset, Page,
    QuestionAbout, TextField, Tone, holds_scissors,
};

pub use crate::format::Length;
use crate::format::{
    Answer, ContextEntry, Kind, Question, Session, SessionResult, has_note, is_answered,
    rejected_answers, result_answers, takes_own_text,
};

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
    /// The person rejected the session: the reason if they gave one, the
    /// note to the whole session if they wrote one, and the answers of the
    /// questions with an answer or a note (spec section 4.1).
    Reject {
        reason: Option<String>,
        note: Option<String>,
        answers: Vec<Answer>,
    },
    Quit,
    OpenImage(String),
    OpenSessionList,
    /// Show the session's whole intro, which the header may cut.
    ShowIntro,
    /// Open the focused field in the person's editor (ADR 74).
    Edit(EditRequest),
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
    /// The one row of an info item, `next`: `enter` on it moves on to the
    /// next tab, as the item takes no answer.
    Next,
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
    pub(crate) fn is_field(self) -> bool {
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
    /// Whether it shows an image without its caption. From a pane it does
    /// at once, as the caption shows in the pane. From the one-line hint,
    /// where no pane shows, it first shows image and caption together,
    /// and `z` then shows the image alone (ADR 69).
    alone: bool,
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
    /// The questions that take an answer, which leaves out info items.
    pub questions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerState {
    Answered,
    /// Answered by an untouched default.
    Defaulted,
    Skipped,
    /// An info item, which takes no answer and so is neither.
    Info,
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
    /// question it counts only while no option is chosen and it is not
    /// unpicked (spec section 4.2).
    custom: String,
    /// On a `single`, the own text stays typed but is not the pick: it was
    /// typed, then an option was picked and `backspace` cleared that pick.
    /// `enter` on the own-answer row or an edit of the text picks it again.
    /// Set only while there is text to show unpicked.
    custom_unpicked: bool,
    note: String,
    /// The person edited the question; a default then no longer counts as
    /// untouched.
    touched: bool,
}

/// What the context pane showed in a draw: how far its entry scrolls,
/// whether the entry was still loading, with `loading…` in its place, and
/// for a diff that showed, how it was laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanePage {
    pub page: Page,
    pub loading: bool,
    pub diff: Option<DrawnDiff>,
}

#[cfg(test)]
impl PanePage {
    /// The pane as it shows an entry that loaded, `page` of it.
    pub(crate) fn shown(page: Page) -> Self {
        PanePage {
            page,
            loading: false,
            diff: None,
        }
    }
}

/// How a diff entry is laid out (spec section 8.5): unified, the removed
/// lines above the added ones, or side by side, the old text left of the
/// new.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffLayout {
    #[default]
    Unified,
    SideBySide,
}

/// What a draw of the pane showed of a diff: the layout its viewer used,
/// whether the body had room for two columns, and, where the entry's
/// offset counted rows of the other layout, the top the draw translated it
/// to and the top it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawnDiff {
    pub layout: DiffLayout,
    pub fits_side_by_side: bool,
    pub translated: Option<(usize, usize)>,
}

/// The layout whose rows an entry's offset counts, and the way back of the
/// translation that set it: the top it set and the top it came from. The
/// rows of a unified and a side-by-side diff differ, so a top kept as a
/// number would show another line in the other layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutTag {
    pub layout: DiffLayout,
    pub back: Option<(usize, usize)>,
}

// ---------------------------------------------------------------------
// The view of the question's area
// ---------------------------------------------------------------------
//
// Every line of the question's area can be reached with the keys that move
// the cursor (spec section 7.4). For the row under the cursor the area can
// show a range of positions: on the first row up to the question's first
// line, on the last row down to its last line, and through a row other
// than a field that does not fit the area with the line above and below
// it. The keys move the view through that range a line at a time before
// they move the cursor. Only the draw knows the range, so it hands it back
// as a `RowCut`, as it hands back what the context pane showed.

/// What the last draw showed of the range of positions the question's area
/// can take for the row under the cursor, when the range holds more than
/// one: how many lines of it lie above and below the view, and how far
/// from its upper end the place is the row returns to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowCut {
    pub above: usize,
    pub below: usize,
    pub home: usize,
}

/// Where the question's area stands in the range of the row under the
/// cursor (see [`RowCut`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowTop {
    /// Where the row shows as the cursor reaches it: for a row other than
    /// a field that does not fit, the line above it and its first lines,
    /// else the whole row, or a field's text cursor, at rest.
    #[default]
    Start,
    /// For a row other than a field that does not fit, its last lines and
    /// the line below it, as the cursor moving up onto it shows them.
    End,
    /// This many lines from the upper end of the range.
    Line(usize),
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
    /// The focused field or note.
    field: Option<TextField>,
    session_note: String,
    reject_reason: String,
    help: bool,
    message: Option<Notice>,
    /// The context entry shown, drawn over the question's area (`z`). It
    /// reads like a pager, so the pane has the focus while it shows.
    full_screen: Option<FullScreen>,
    /// The diff the person switched to side by side (`s`), which shows so
    /// while it is the entry shown (see [`Self::side_by_side`]).
    side_by_side: Option<EntryKey>,
    /// The context pane takes the keys. Kept apart from `focus`, so the
    /// field or note being typed in has its text and cursor again when the
    /// pane gives the focus back.
    pane_focus: bool,
    /// What the context pane showed in the last draw; `None` when there was
    /// no pane, as on a terminal that only had room for a hint.
    pane_page: Option<PanePage>,
    /// Where the person left each entry they scrolled. Offsets of different
    /// entries have nothing to do with each other, so each entry keeps its
    /// own, and one shown for the first time starts at its top.
    offsets: HashMap<EntryKey, Offset>,
    /// The layout whose rows each offset of a diff counts. An entry without
    /// one counts unified rows, as every entry but a diff does.
    offset_layouts: HashMap<EntryKey, LayoutTag>,
    /// See [`Self::revision`].
    revision: u64,
    /// See [`Self::follows_title`].
    follows_title: Option<String>,
    /// See [`Self::seen`].
    seen: bool,
    /// Where the question's area stands for the row under the cursor, with
    /// the tab and row it was set for (see [`Self::row_top`]).
    row_top: Option<(usize, usize, RowTop)>,
    /// What the last draw showed of the range of the row under the cursor,
    /// with the tab and row it was drawn for, so a cut of another row is
    /// never used.
    row_cut: Option<(usize, usize, RowCut)>,
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
                        selected: defaults(question),
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
            field: None,
            session_note: cleaned(
                draft
                    .and_then(|draft| draft.note.as_deref())
                    .unwrap_or_default(),
            ),
            reject_reason: cleaned(
                draft
                    .and_then(|draft| draft.reason.as_deref())
                    .unwrap_or_default(),
            ),
            help: false,
            message: None,
            full_screen: None,
            side_by_side: None,
            pane_focus: false,
            pane_page: None,
            offsets: HashMap::new(),
            offset_layouts: HashMap::new(),
            revision: 0,
            follows_title: None,
            seen: false,
            row_top: None,
            row_cut: None,
        };
        state.arrive();
        state
    }

    /// Takes over a session file that was replaced while it was open,
    /// keeping the answers that still fit and where the person
    /// was, so the reload does not throw them back to the start. A
    /// question the person has not touched takes the new file's defaults.
    ///
    /// The question shown is found again by its id, as a draft finds it,
    /// and the review stays the review. Where the row is found again, the
    /// focus stays as it was: a field left with `esc` stays without the
    /// keys, and a field typed in keeps its text cursor. A row that is gone,
    /// such as an own answer turned off, gives way to the row in its place,
    /// or the last row when the tab has fewer, which is entered as if moved
    /// to. An open note stays open with its text cursor while its question
    /// shows, whatever row the cursor is on, as every question takes a
    /// note (ADR 68) and the note belongs to no row. The pane's
    /// focus, the full-screen view and a diff shown side by side stay while
    /// the same entry shows; another entry under them ends them, as the
    /// person did not choose to look at it. Each entry keeps its scroll
    /// position, and the layout it counts in, while it reads as before.
    ///
    /// A message says that the file was reloaded, but only for a session
    /// the person has seen (see [`Self::seen`]): they never had any other
    /// on screen, so "was reloaded" tells them nothing about it.
    pub fn replace_session(&mut self, session: Session) {
        // A question the person has not touched holds nothing of theirs,
        // so it starts from the new file's defaults, as on a fresh start.
        // Left in the draft, an empty answer would restore as touched and
        // keep a default the file added from being picked.
        let mut draft = self.to_draft();
        draft.answers.retain(|answer| {
            self.session
                .questions
                .iter()
                .zip(&self.working)
                .any(|(question, working)| question.id == answer.question && working.touched)
        });
        let mut replaced = SessionState::new(self.id.clone(), session, Some(&draft));
        replaced.revision = self.revision + 1;
        replaced.seen = self.seen;

        if self.on_review() {
            replaced.tab = replaced.session.questions.len();
        }
        // On the review neither has a question, which counts as the same.
        let same_tab = self.question().map(|question| &question.id)
            == replaced.question().map(|question| &question.id);
        let found = same_tab
            .then(|| {
                let rows = replaced.rows();
                self.current_row()
                    .and_then(|row| rows.iter().position(|&new| new == row))
            })
            .flatten();
        match found {
            Some(row) => {
                replaced.row = row;
                replaced.keep_focus(self);
                // The draw clamps the view to the row's new range.
                replaced.row_top = self
                    .row_top
                    .filter(|&(tab, old, _)| (tab, old) == (self.tab, self.row))
                    .map(|(_, _, top)| (replaced.tab, row, top));
            }
            None => {
                if same_tab {
                    replaced.row = self.row.min(replaced.rows().len().saturating_sub(1));
                }
                replaced.arrive();
                if same_tab && self.focus == Focus::Note {
                    replaced.keep_note(self);
                }
            }
        }

        replaced.pane_page = self.pane_page;
        let shown = |state: &SessionState| state.shown_entry().map(|(key, _)| key);
        if shown(self).is_some() && shown(self) == shown(&replaced) {
            replaced.pane_focus = self.pane_focus;
            replaced.full_screen = self.full_screen;
            replaced.side_by_side = self.side_by_side;
        }
        // An offset and the layout it counts in go together.
        let reads_as_before = |key: &EntryKey| {
            entry_at(&self.session, *key)
                .is_some_and(|entry| entry_at(&replaced.session, *key) == Some(entry))
        };
        replaced.offsets = self
            .offsets
            .iter()
            .filter(|(key, _)| reads_as_before(key))
            .map(|(key, offset)| (*key, *offset))
            .collect();
        replaced.offset_layouts = self
            .offset_layouts
            .iter()
            .filter(|(key, _)| reads_as_before(key))
            .map(|(key, tag)| (*key, *tag))
            .collect();

        if replaced.seen {
            replaced.message = Some(Notice::new(
                Tone::Info,
                format!("{} changed on disk and was reloaded", replaced.name()),
            ));
        }
        *self = replaced;
    }

    /// Whether the person has had the session on screen: a frame showed it
    /// as the active session, at asqr's minimum size or more, its tab or
    /// its full-screen view with no overlay and no invalid session's dialog
    /// over it. It stays seen while it waits, also across reloads. A
    /// session that only waited in the queue is unseen, and so is one that
    /// took over and was reloaded before a frame showed it, as when one
    /// sync takes the session on screen out and reloads the next one.
    pub fn seen(&self) -> bool {
        self.seen
    }

    /// Notes that a frame showed the session (see [`Self::seen`]).
    pub fn mark_seen(&mut self) {
        self.seen = true;
    }

    /// Where the question's area stands for the row under the cursor; a
    /// row the cursor just reached starts at [`RowTop::Start`].
    pub fn row_top(&self) -> RowTop {
        match self.row_top {
            Some((tab, row, top)) if (tab, row) == (self.tab, self.row) => top,
            _ => RowTop::Start,
        }
    }

    /// Takes what the last draw showed of the range of the row under the
    /// cursor, which is how far the keys can move the view through it.
    pub fn set_row_cut(&mut self, cut: Option<RowCut>) {
        self.row_cut = cut.map(|cut| (self.tab, self.row, cut));
    }

    /// The cut of the row under the cursor, if the last draw left one.
    fn row_cut(&self) -> Option<RowCut> {
        self.row_cut
            .filter(|&(tab, row, _)| (tab, row) == (self.tab, self.row))
            .map(|(_, _, cut)| cut)
    }

    fn set_row_top(&mut self, top: RowTop) {
        self.row_top = Some((self.tab, self.row, top));
    }

    /// Puts the question's area back where the row under the cursor shows,
    /// before a key acts on the question, so the row and what the key does
    /// to it are in view.
    fn reset_view(&mut self) {
        self.row_top = None;
        self.row_cut = None;
    }

    /// Moves the view through the range of the row under the cursor by a
    /// line, down for `down`, and says whether it could.
    fn scroll_row(&mut self, down: bool) -> bool {
        let Some(cut) = self.row_cut() else {
            return false;
        };
        let line = match down {
            true if cut.below > 0 => cut.above + 1,
            false if cut.above > 0 => cut.above - 1,
            _ => return false,
        };
        self.set_row_top(RowTop::Line(line));
        // The next draw measures the cut again; until then it follows the
        // view, so a key handled before that draw goes on from here.
        self.row_cut = Some((
            self.tab,
            self.row,
            RowCut {
                above: line,
                below: (cut.above + cut.below) - line,
                ..cut
            },
        ));
        true
    }

    /// Takes over the focus of `old`, the state this one replaces, on the
    /// same row (see [`Self::replace_session`]). The text cursor keeps its
    /// place as far as the text still reaches; where the field was drawn
    /// is learned again from the next draw.
    fn keep_focus(&mut self, old: &SessionState) {
        match old.focus {
            Focus::None => {
                self.focus = Focus::None;
                self.field = None;
            }
            Focus::Field => {
                let cursor = old.field_cursor().unwrap_or_default();
                let text = self.row_text().unwrap_or_default();
                self.field = Some(TextField::with_cursor(text, cursor));
                self.focus = Focus::Field;
            }
            Focus::Note => self.keep_note(old),
        }
    }

    /// Opens the note of the current question again with the text cursor
    /// `old`'s note had, as far as the note still reaches.
    fn keep_note(&mut self, old: &SessionState) {
        let cursor = old.field_cursor().unwrap_or_default();
        let note = &self.working[self.tab].note;
        self.field = Some(TextField::with_cursor(note, cursor));
        self.focus = Focus::Note;
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

    /// What the screen calls the session: its title, else its id.
    pub fn name(&self) -> &str {
        self.session.title.as_deref().unwrap_or(&self.id)
    }

    /// The title of the session `follows` names, as the inbox found it
    /// when it loaded this one (spec section 7.2); `None` where it found
    /// none. A replaced file starts without it until the inbox looks again.
    pub fn follows_title(&self) -> Option<&str> {
        self.follows_title.as_deref()
    }

    pub fn set_follows_title(&mut self, title: Option<String>) {
        self.follows_title = title;
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
    pub fn message(&self) -> Option<&Notice> {
        self.message.as_ref()
    }

    pub fn full_screen(&self) -> bool {
        self.full_screen.is_some()
    }

    /// Whether the full-screen view shows an image without its caption.
    pub fn full_screen_alone(&self) -> bool {
        self.full_screen
            .is_some_and(|full_screen| full_screen.alone)
    }

    /// Whether the diff shown shows side by side.
    pub fn side_by_side(&self) -> bool {
        self.side_by_side.is_some() && self.side_by_side == self.shown_entry().map(|(key, _)| key)
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

    /// The layout whose rows the offset of the entry shown counts.
    pub fn pane_layout_tag(&self) -> LayoutTag {
        self.shown_entry()
            .and_then(|(key, _)| self.offset_layouts.get(&key).copied())
            .unwrap_or_default()
    }

    /// Takes what the last draw of the context pane showed, which depends
    /// on the size of the terminal. Without a pane there is nothing to
    /// focus or show full screen, so the question gets the keys back.
    ///
    /// A diff drawn in another layout than its offset counted in was drawn
    /// from the translated top, which becomes the entry's offset from now
    /// on, together with the way back. Its column stays. Every other draw
    /// leaves the offsets as they are, also one the draw had to clamp, so a
    /// terminal made larger and smaller again shows the same lines again.
    pub fn set_pane_page(&mut self, page: Option<PanePage>) {
        self.pane_page = page;
        if page.is_none() {
            self.pane_focus = false;
            self.full_screen = None;
        }
        let drawn = page.and_then(|page| page.diff);
        if let (Some(diff), Some((key, _))) = (drawn, self.shown_entry())
            && let Some((top, from)) = diff.translated
        {
            self.offsets.entry(key).or_default().top = top;
            self.offset_layouts.insert(
                key,
                LayoutTag {
                    layout: diff.layout,
                    back: Some((top, from)),
                },
            );
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
    /// question's (spec section 8.1).
    pub fn shown_entry(&self) -> Option<(EntryKey, &ContextEntry)> {
        self.question()?;
        self.entry_at(self.tab, self.current_row()?)
    }

    /// The context entry row `row` of question `index` shows: the option's
    /// own, or else the question's.
    fn entry_at(&self, index: usize, row: Row) -> Option<(EntryKey, &ContextEntry)> {
        let question = &self.session.questions[index];
        let option_entry = match row {
            Row::Option(option) => question
                .options
                .as_ref()
                .and_then(|options| options[option].context.first())
                .map(|entry| (Some(option), entry)),
            _ => None,
        };
        let (option, entry) =
            option_entry.or_else(|| question.context.first().map(|entry| (None, entry)))?;
        let key = EntryKey {
            question: index,
            option,
        };
        Some((key, entry))
    }

    /// The context entries asqr keeps loaded while the current question
    /// shows, in the order they load (ADR 46): the one shown, then those
    /// the question's other rows show, from the row below the cursor on
    /// and round to the one above it, then those of the next question and
    /// of the one before, each from its first row. Each entry is listed
    /// once. The review keeps none.
    pub fn window(&self) -> Vec<(EntryKey, &ContextEntry)> {
        if self.question().is_none() {
            return Vec::new();
        }
        let rows = self.rows();
        let below_the_cursor =
            (1..=rows.len()).map(|step| (self.tab, rows[(self.row + step) % rows.len()]));
        let of_question =
            |index: usize| self.rows_of(index).into_iter().map(move |row| (index, row));
        let next = (self.tab + 1 < self.session.questions.len()).then(|| of_question(self.tab + 1));
        let before = self.tab.checked_sub(1).map(of_question);
        let mut window: Vec<(EntryKey, &ContextEntry)> = self.shown_entry().into_iter().collect();
        for (index, row) in below_the_cursor
            .chain(next.into_iter().flatten())
            .chain(before.into_iter().flatten())
        {
            if let Some((key, entry)) = self.entry_at(index, row)
                && !window.iter().any(|(listed, _)| *listed == key)
            {
                window.push((key, entry));
            }
        }
        window
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
        match question.kind {
            Kind::Text => return vec![Row::Answer],
            Kind::Info => return vec![Row::Next],
            Kind::Single | Kind::Multi => {}
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
                    custom_unpicked: working.custom_unpicked,
                    note: filled(&working.note),
                    defaulted: !working.touched && !working.selected.is_empty(),
                    skipped: false,
                }
            })
            .collect()
    }

    /// What a submit would send, counted. Info items take no answer, so
    /// they count neither as answered nor as skipped, also when a note puts
    /// them into the result.
    pub fn counts(&self) -> Counts {
        let asks = |question: &&Question| question.kind != Kind::Info;
        let asked: Vec<&str> = self
            .session
            .questions
            .iter()
            .filter(asks)
            .map(|question| question.id.as_str())
            .collect();
        let result: Vec<Answer> = result_answers(&self.session, &self.answers())
            .into_iter()
            .filter(|answer| asked.contains(&answer.question.as_str()))
            .collect();
        Counts {
            answered: result.iter().filter(|answer| !answer.skipped).count(),
            skipped: result.iter().filter(|answer| answer.skipped).count(),
            defaulted: result.iter().filter(|answer| answer.defaulted).count(),
            questions: asked.len(),
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

    /// Whether the cursor is on the picked option of a single question, a
    /// default included: the one place where `backspace` clears the pick.
    pub fn on_pick(&self) -> bool {
        let (Some(question), Some(Row::Option(index))) = (self.question(), self.current_row())
        else {
            return false;
        };
        question.kind == Kind::Single
            && option_ids(question).nth(index).is_some_and(|id| {
                self.working[self.tab]
                    .selected
                    .iter()
                    .any(|picked| picked == id)
            })
    }

    pub fn question_states(&self) -> Vec<QuestionState> {
        self.session
            .questions
            .iter()
            .zip(self.answers())
            .map(|(question, answer)| match question.kind {
                // A blank note on an info item is no note, as the result
                // leaves the item out then.
                Kind::Info => QuestionState {
                    answer: AnswerState::Info,
                    note: has_note(&answer),
                },
                Kind::Single | Kind::Multi | Kind::Text => QuestionState {
                    answer: match (is_answered(question, &answer), answer.defaulted) {
                        (false, _) => AnswerState::Skipped,
                        (true, true) => AnswerState::Defaulted,
                        (true, false) => AnswerState::Answered,
                    },
                    note: answer.note.is_some(),
                },
            })
            .collect()
    }

    /// Takes where the focused field or note was drawn, for `↑`/`↓`, and
    /// says whether that moved its text cursor, which then needs drawing
    /// again (see [`TextField::set_shape`]).
    pub fn set_field_shape(&mut self, shape: FieldShape) -> bool {
        self.field
            .as_mut()
            .is_some_and(|field| field.set_shape(shape))
    }

    /// The focused field or note.
    pub fn field(&self) -> Option<&TextField> {
        self.field.as_ref()
    }

    /// The text of the focused field.
    pub fn field_text(&self) -> Option<String> {
        (self.focus == Focus::Field)
            .then(|| self.field.as_ref().map(TextField::text))
            .flatten()
    }

    /// The text of the note being edited.
    pub fn note_text(&self) -> Option<String> {
        (self.focus == Focus::Note)
            .then(|| self.field.as_ref().map(TextField::text))
            .flatten()
    }

    /// The text cursor (line, column) of the focused field or note.
    pub fn field_cursor(&self) -> Option<(usize, usize)> {
        self.field.as_ref().map(TextField::cursor)
    }

    /// The limits the focused field is counted against, when it has any.
    pub fn field_limits(&self) -> Option<Length> {
        if self.focus != Focus::Field {
            return None;
        }
        self.limits(self.current_row()?)
    }

    /// The length of the focused field and the limits it is counted
    /// against, when the field has limits.
    pub fn field_length(&self) -> Option<(usize, Length)> {
        let limits = self.field_limits()?;
        Some((self.field.as_ref()?.length(), limits))
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
    ///
    /// A diff switched to side by side shows so while it is the entry
    /// shown, as the full-screen view does: once a key shows another entry,
    /// the switch ends, and coming back shows the diff unified. A paste
    /// never moves the cursor, so it never ends it.
    pub fn handle(&mut self, key: KeyEvent) -> Effect {
        let effect = self.handle_key(key);
        if !self.side_by_side() {
            self.side_by_side = None;
        }
        effect
    }

    fn handle_key(&mut self, key: KeyEvent) -> Effect {
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
            Focus::Field | Focus::Note => self.typing(key),
        }
    }

    // -----------------------------------------------------------------
    // Keys of the context pane
    // -----------------------------------------------------------------

    /// The keys that belong to the context pane, before a field or the
    /// question sees them; `None` leaves `key` to them.
    ///
    /// The focused pane and the full-screen view read like a pager: they
    /// take every key, and only `?`, `z`, `o` and `s` keep their meaning
    /// from the question. `tab`, `esc` and `q` go back, a typed word that starts with
    /// `q` included, so no word quits asqr; `ctrl-c` still does. Without the
    /// focus, `tab` and the page keys still reach the pane, also from a
    /// field, as none of them types text (plan, *Keys*). `shift-tab` acts as
    /// `tab` everywhere.
    fn pane_key(&mut self, key: KeyEvent) -> Option<Effect> {
        if self.pane_focus {
            let control = key.modifiers.contains(KeyModifiers::CONTROL);
            let effect = match key.code {
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc => self.leave_pane(),
                KeyCode::Char('q') if !control => self.leave_pane(),
                KeyCode::Char('z') if !control && self.full_screen.is_some() => {
                    self.next_full_screen_step()
                }
                KeyCode::Char('z') if !control => self.show_full_screen(),
                KeyCode::Char('?') if !control => {
                    self.help = true;
                    Effect::None
                }
                KeyCode::Char('o') if !control => self.open(),
                KeyCode::Char('s') if !control => self.switch_side_by_side(),
                _ => {
                    self.scroll_pane(key, true);
                    Effect::None
                }
            };
            return Some(effect);
        }
        match key.code {
            // A tab never types: a tab character in an answer is a
            // mistake, and on a text question, whose field has the focus
            // from the start, the pane would otherwise take `esc` first.
            // `shift-tab` does the same, as one pane is all there is to
            // move to.
            KeyCode::Tab | KeyCode::BackTab => {
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
    /// An image's caption shows there only when no pane showed it, as on
    /// the one-line hint (ADR 69).
    fn show_full_screen(&mut self) -> Effect {
        if let Some(entry) = self.shown_context() {
            let alone = self.pane_page.is_some() || entry.shown_caption().is_none();
            self.full_screen = Some(FullScreen {
                pane_focus: self.pane_focus,
                alone,
            });
            self.pane_focus = true;
        }
        Effect::None
    }

    /// `z` in the full-screen view: from image and caption on to the
    /// image alone, from there back to where the focus was.
    fn next_full_screen_step(&mut self) -> Effect {
        match &mut self.full_screen {
            Some(full_screen) if !full_screen.alone => {
                full_screen.alone = true;
                Effect::None
            }
            _ => self.leave_pane(),
        }
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
                    self.message = Some(Notice::new(Tone::Warning, message));
                    Effect::None
                }
            },
            None => {
                self.message = Some(Notice::new(Tone::Warning, "nothing to open here"));
                Effect::None
            }
        }
    }

    /// `s`: switches the diff shown between unified and side by side (spec
    /// section 8.5), or says why it cannot. Back to unified always works.
    /// Side by side asks the last draw whether two columns fit; where no
    /// pane shows, as beside the one-line hint, the switch holds for the
    /// full-screen view, and while the diff loads, its draw decides.
    ///
    /// A diff switched side by side in a body too narrow for two columns
    /// shows unified while the switch stays on. `s` there turns the switch
    /// off, which changes nothing on screen, so it says why the diff stays
    /// unified. The last draw's room decides, not its layout: a second `s`
    /// before any draw meets a page laid out unified that has the room.
    fn switch_side_by_side(&mut self) -> Effect {
        let refusal = match self.shown_entry() {
            None => Some("nothing to show side by side here"),
            Some((_, entry)) if !matches!(entry, ContextEntry::Diff(_)) => {
                Some("only a diff shows side by side")
            }
            Some(_) if self.side_by_side() => {
                self.side_by_side = None;
                if let Some(PanePage {
                    diff: Some(diff), ..
                }) = self.pane_page
                    && !diff.fits_side_by_side
                {
                    let message = if self.full_screen.is_some() {
                        "too narrow for side by side, switched to unified"
                    } else {
                        "too narrow for side by side, switched to unified; z shows it full screen"
                    };
                    self.message = Some(Notice::new(Tone::Warning, message));
                }
                return Effect::None;
            }
            Some(_) => match self.pane_page {
                None => {
                    self.message = Some(Notice::new(Tone::Info, "side by side shows with z"));
                    None
                }
                Some(PanePage { loading: true, .. }) => None,
                Some(PanePage { diff: None, .. }) => Some("this diff cannot be shown side by side"),
                Some(PanePage {
                    diff: Some(diff), ..
                }) if !diff.fits_side_by_side => Some(if self.full_screen.is_some() {
                    "too narrow for side by side"
                } else {
                    "too narrow for side by side; z shows it full screen"
                }),
                Some(_) => None,
            },
        };
        match refusal {
            Some(refusal) => self.message = Some(Notice::new(Tone::Warning, refusal)),
            None => self.side_by_side = self.shown_entry().map(|(key, _)| key),
        }
        Effect::None
    }

    /// Scrolls the entry shown when `key` scrolls the pane, and says
    /// whether it did. While the pane shows `loading…`, its page is the
    /// placeholder's one line, not the entry's: the key is taken and the
    /// entry keeps where the person left it.
    fn scroll_pane(&mut self, key: KeyEvent, focused: bool) -> bool {
        let (Some(PanePage { page, loading, .. }), Some((entry, _))) =
            (self.pane_page, self.shown_entry())
        else {
            return false;
        };
        let offset = self.offsets.get(&entry).copied().unwrap_or_default();
        match scroll(key, offset, page, focused) {
            Some(_) if loading => true,
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
        // On a field's row left with `esc`, `ctrl-g` gives the field the
        // keys again, as a paste there does, and opens it in the editor.
        if is_ctrl_g(key) && self.current_row().is_some_and(Row::is_field) {
            self.arrive();
            return self.edit_request();
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return Effect::None;
        }
        // Only the keys that move the cursor move the view through the
        // row's range. The keys that act on the row under the cursor bring
        // it back into view first, so the person sees what they did to it;
        // switching questions does so in `go_to_tab`. `?`, `L`, `i`, `o`,
        // `z`, `s` and `q` act on no row, and the keys bound to nothing do
        // nothing, so they leave the view where the person scrolled it.
        if matches!(
            key.code,
            KeyCode::Enter | KeyCode::Backspace | KeyCode::Char(' ' | 'n' | '1'..='9')
        ) {
            self.reset_view();
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
            KeyCode::Backspace => self.unpick(),
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
            KeyCode::Char('s') => self.switch_side_by_side(),
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
            Row::Option(_) | Row::Answer | Row::Next => self.go_to_tab(self.tab + 1),
            Row::Own => self.pick_own(),
            Row::Question(index) => self.go_to_tab(index),
            Row::Submit | Row::Note => self.submit(),
            Row::Reject => self.reject(),
        }
    }

    fn move_row(&mut self, delta: isize) -> Effect {
        // The view goes through the row's range before the cursor moves.
        if self.scroll_row(delta > 0) {
            return Effect::None;
        }
        let last = self.rows().len().saturating_sub(1);
        let row = self.row.saturating_add_signed(delta).min(last);
        // Landing on a row focuses its field; a field left with esc is
        // landed on again when the move stays on it, as on a text question
        // with its single row. A field below the row left is entered at
        // its first row.
        if row != self.row || self.focus == Focus::None {
            let (from_above, moved) = (row > self.row, row != self.row);
            self.row = row;
            self.arrive_from(from_above);
            // A row other than a field that does not fit the area shows
            // the end the cursor came from, as a field does (ADR 36).
            if moved {
                self.reset_view();
                if !self.current_row().is_some_and(Row::is_field) {
                    self.set_row_top(if from_above {
                        RowTop::Start
                    } else {
                        RowTop::End
                    });
                }
            }
        }
        Effect::None
    }

    fn go_to_tab(&mut self, tab: usize) -> Effect {
        if tab >= self.tab_count() {
            return Effect::None;
        }
        self.reset_view();
        self.tab = tab;
        // The review opens on Submit, the usual next step; questions start
        // on their first row.
        self.row = if self.on_review() {
            self.session.questions.len()
        } else {
            0
        };
        self.arrive();
        Effect::DraftChanged
    }

    /// Focuses the field of the current row, if it is one, the text
    /// cursor at the end of its text. Called whenever the cursor lands on
    /// a row.
    fn arrive(&mut self) {
        self.arrive_from(false);
    }

    /// [`Self::arrive`], for a row the cursor moved down to when
    /// `from_above` holds: its field starts at the end of its first row
    /// (ADR 36).
    fn arrive_from(&mut self, from_above: bool) {
        match self.row_text() {
            Some(text) => {
                self.field = Some(if from_above {
                    TextField::entered_from_above(text)
                } else {
                    TextField::new(text)
                });
                self.focus = Focus::Field;
            }
            None => {
                self.field = None;
                self.focus = Focus::None;
            }
        }
    }

    /// The text kept for the field of the current row, if it is one.
    fn row_text(&self) -> Option<&str> {
        let text = match self.current_row()? {
            Row::Note => &self.session_note,
            Row::Reject => &self.reject_reason,
            Row::Own | Row::Answer => &self.working[self.tab].custom,
            Row::Option(_) | Row::Next | Row::Question(_) | Row::Submit => return None,
        };
        Some(text)
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

    /// Clears the pick of a single question, a default included, so that
    /// every question can be skipped (spec section 4.2). Only the picked
    /// option's own row clears it, so moving around never clears a pick by
    /// accident. Typed own text stays in its field, unpicked. The cursor
    /// stays on its row.
    fn unpick(&mut self) -> Effect {
        if !self.on_pick() {
            return Effect::None;
        }
        let working = &mut self.working[self.tab];
        working.selected.clear();
        working.custom_unpicked = !working.custom.trim().is_empty();
        working.touched = true;
        Effect::DraftChanged
    }

    /// Picks the own answer and moves on. On a single question that
    /// deselects the options, since the own answer counts only without one,
    /// and picks own text that `backspace` left unpicked.
    fn pick_own(&mut self) -> Effect {
        if self.kind() == Some(Kind::Single) {
            let working = &mut self.working[self.tab];
            working.selected.clear();
            working.custom_unpicked = false;
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
                self.message = Some(Notice::new(Tone::Warning, format!("at most {max} options")));
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

    /// Every question is optional, so a submit always
    /// goes through; unanswered questions come back as skipped. The session
    /// note is trimmed like the reject reason (ADR 29).
    fn submit(&mut self) -> Effect {
        Effect::Submit {
            answers: result_answers(&self.session, &self.answers()),
            note: trimmed(&self.session_note),
        }
    }

    /// A reject keeps the session note as a submit would send it, and the
    /// answers of the questions with an answer or a note.
    fn reject(&self) -> Effect {
        Effect::Reject {
            reason: trimmed(&self.reject_reason),
            note: trimmed(&self.session_note),
            answers: rejected_answers(&self.session, &self.answers()),
        }
    }

    /// Opens the note of the current question; every question and info
    /// item takes one (ADR 68). The review has no `n`.
    fn open_note(&mut self) -> Effect {
        if self.question().is_none() {
            return Effect::None;
        }
        self.field = Some(TextField::new(&self.working[self.tab].note));
        self.focus = Focus::Note;
        Effect::None
    }

    // -----------------------------------------------------------------
    // Keys inside fields
    // -----------------------------------------------------------------

    /// A key for the focused field or note. The field edits its text and
    /// says which keys it leaves to its context; what they mean depends on
    /// where the field is.
    fn typing(&mut self, key: KeyEvent) -> Effect {
        if is_ctrl_g(key) {
            return self.edit_request();
        }
        let note = self.focus == Focus::Note;
        // A field scrolled away from where it shows, into the lines above
        // the first row or below the last, goes back a line at a time with
        // the keys that move its text cursor, before they move it. The note
        // of a question belongs to no row, and the area keeps its text
        // cursor in view.
        if !note
            && let Some(cut) = self.row_cut()
            && let Some(down) = vertical(key)
            && (if down {
                cut.above < cut.home
            } else {
                cut.above > cut.home
            })
        {
            self.scroll_row(down);
            return Effect::None;
        }
        let field = self
            .field
            .as_mut()
            .expect("a focused field or note always has its editor");
        let result = field.key(key);
        // Leaving the field upwards or downwards may scroll on through the
        // row's range; any other key shows the field again first.
        if !matches!(result, FieldKey::Leave(Leave::Up | Leave::Down)) {
            self.reset_view();
        }
        match result {
            FieldKey::Changed => self.store_edit(),
            FieldKey::Unchanged => Effect::None,
            // A question's note is opened with `n` from a row and belongs
            // to no row of its own: `enter` and `esc` both return to the
            // row, and the arrows that leave a field find no row to go to.
            FieldKey::Enter | FieldKey::Escape if note => {
                self.arrive();
                Effect::None
            }
            FieldKey::Leave(_) if note => Effect::None,
            FieldKey::Enter => self.enter(),
            FieldKey::Escape => {
                self.focus = Focus::None;
                self.field = None;
                Effect::None
            }
            FieldKey::Leave(Leave::Up) => self.move_row(-1),
            FieldKey::Leave(Leave::Down) => self.move_row(1),
            // At the edges of the text the arrows leave a row's field for
            // the question before or after, so a field never traps the
            // person.
            FieldKey::Leave(Leave::Left) if self.tab > 0 => self.go_to_tab(self.tab - 1),
            FieldKey::Leave(Leave::Right) if self.tab + 1 < self.tab_count() => {
                self.go_to_tab(self.tab + 1)
            }
            FieldKey::Leave(Leave::Left | Leave::Right) => Effect::None,
        }
    }

    /// Inserts `text` the person pasted at the text cursor of the focused
    /// field or note (spec section 7.7). The terminal sends a paste as one
    /// event, so a line break in it never reaches `enter`, which would
    /// pick, submit or reject; every field keeps the line breaks (ADR 36).
    ///
    /// A field left with `esc` takes the paste as it would take typing
    /// again: it gets the keys back, with the text cursor at the end of
    /// its text. Where no field can take a paste, a message says where
    /// one goes, as a paste that vanishes reads like a lost one. Over the
    /// help, a paste only closes it, as any key does.
    pub fn paste(&mut self, text: &str) -> Effect {
        self.message = None;
        if self.help {
            self.help = false;
            return Effect::None;
        }
        if self.pane_focus {
            let leave = if self.full_screen.is_some() {
                "z/esc leave the full screen"
            } else {
                "esc leaves the pane"
            };
            self.message = Some(Notice::new(
                Tone::Warning,
                format!("a paste goes into a field; {leave}"),
            ));
            return Effect::None;
        }
        if self.focus == Focus::None {
            if !self.current_row().is_some_and(Row::is_field) {
                // Every question takes a note (ADR 68); the review has none.
                let message = if self.question().is_some() {
                    "a paste goes into a field; press n for a note"
                } else {
                    "a paste goes into a field"
                };
                self.message = Some(Notice::new(Tone::Warning, message));
                return Effect::None;
            }
            self.arrive();
        }
        // The pasted text shows where it goes.
        self.reset_view();
        let field = self
            .field
            .as_mut()
            .expect("a focused field or note always has its editor");
        if !field.paste(text) {
            return Effect::None;
        }
        self.store_edit()
    }

    /// Asks for the focused field or note in the person's editor, with
    /// what it belongs to (ADR 74). A text that holds the editor file's cut
    /// line would come back cut short there, so it is refused with a
    /// message.
    fn edit_request(&mut self) -> Effect {
        let text = self.field.as_ref().map(TextField::text).unwrap_or_default();
        if holds_scissors(&text) {
            self.message = Some(Notice::new(
                Tone::Warning,
                "the field holds the >8 line; remove it to edit in the editor",
            ));
            return Effect::None;
        }
        let field = match (self.focus, self.current_row()) {
            (Focus::Note, _) => FieldName::Note,
            (_, Some(Row::Note)) => FieldName::SessionNote,
            (_, Some(Row::Reject)) => FieldName::RejectReason,
            (_, Some(Row::Answer)) => FieldName::Answer,
            _ => FieldName::OwnAnswer,
        };
        let count = self.session.questions.len();
        let question = self.question().map(|question| QuestionAbout {
            number: self.tab + 1,
            count,
            label: question
                .header
                .clone()
                .unwrap_or_else(|| question.id.clone()),
            text: question.text.clone(),
        });
        Effect::Edit(EditRequest {
            text,
            about: FieldAbout {
                title: self.session.title.clone(),
                id: self.id.clone(),
                question,
                field,
            },
        })
    }

    /// Takes `text` back from the person's editor into the focused field
    /// or note, cleaned as a paste is (ADR 74). A text that differs from
    /// the field's only by the line breaks at its end, which the editor's
    /// file drops, leaves the field as it was.
    pub fn edited(&mut self, text: &str) -> Effect {
        let text = cleaned(text);
        let current = self.field.as_ref().map(TextField::text).unwrap_or_default();
        if text == current.trim_end_matches('\n') {
            self.message = Some(Notice::new(Tone::Info, "the field is unchanged"));
            return Effect::None;
        }
        self.field = Some(TextField::new(&text));
        self.reset_view();
        self.store_edit()
    }

    /// Keeps the field's text where the focused field or note belongs.
    fn store_edit(&mut self) -> Effect {
        let text = self.field.as_ref().map(TextField::text).unwrap_or_default();
        match (self.focus, self.current_row()) {
            (Focus::Field, Some(Row::Note)) => self.session_note = text,
            (Focus::Field, Some(Row::Reject)) => self.reject_reason = text,
            (Focus::Note, _) => {
                let working = &mut self.working[self.tab];
                working.note = text;
                working.touched = true;
            }
            _ => {
                let working = &mut self.working[self.tab];
                // With no option chosen, an edit of the own answer picks
                // it, as typing does on a fresh question; it never
                // deselects a chosen option (ADR 8). Text edited down to
                // nothing leaves nothing to show unpicked either.
                if working.selected.is_empty() || text.trim().is_empty() {
                    working.custom_unpicked = false;
                }
                working.custom = text;
                working.touched = true;
            }
        }
        Effect::DraftChanged
    }
}

/// Whether `key` moves a field's text cursor down (`true`) or up
/// (`false`), as [`TextField::key`] reads it, or neither.
fn vertical(key: KeyEvent) -> Option<bool> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Down => Some(true),
        KeyCode::Up => Some(false),
        KeyCode::Char('n') if control && !alt => Some(true),
        KeyCode::Char('p') if control && !alt => Some(false),
        _ => None,
    }
}

/// Whether `key` is `ctrl-g`, which opens a field in the person's editor.
fn is_ctrl_g(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('g') && key.modifiers == KeyModifiers::CONTROL
}

/// Typed text as a result carries it: trimmed, and absent when blank.
fn trimmed(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn option_ids(question: &Question) -> impl Iterator<Item = &str> {
    question
        .options
        .iter()
        .flatten()
        .map(|option| option.id.as_str())
}

/// The context entry `key` names in `session`, if it has one there.
fn entry_at(session: &Session, key: EntryKey) -> Option<&ContextEntry> {
    let question = session.questions.get(key.question)?;
    match key.option {
        Some(option) => question.options.as_ref()?.get(option)?.context.first(),
        None => question.context.first(),
    }
}

/// The ids of the options `question` marks as defaults, in option order.
fn defaults(question: &Question) -> Vec<String> {
    question
        .options
        .iter()
        .flatten()
        .filter(|option| option.default)
        .map(|option| option.id.clone())
        .collect()
}

/// A working answer from a draft, keeping only what fits the question.
///
/// An untouched default is the asker's preset, not the person's pick, so
/// it restores as the defaults the question sets now: a default the file
/// moved, added or removed since the draft was saved shows as the file
/// sets it. The person typed nothing into such a question, as every edit
/// and every pick marks it touched.
fn restored(question: &Question, answer: &Answer) -> Working {
    let mut selected: Vec<String> = if answer.defaulted {
        defaults(question)
    } else {
        option_ids(question)
            .filter(|id| answer.selected.iter().any(|selected| selected == id))
            .map(str::to_owned)
            .collect()
    };
    if question.kind == Kind::Single {
        selected.truncate(1);
    }
    // An info item, or a `single` or `multi` whose file turned `custom`
    // off, has no field, so typed text the draft kept for it would count as
    // an answer nobody can see or clear.
    let custom = answer
        .custom
        .as_deref()
        .filter(|_| takes_own_text(question));
    // Text in a draft reaches a field without being typed, like a paste,
    // and is cleaned the same way, so the field draws what it counts.
    let custom = cleaned(custom.unwrap_or_default());
    // Only a single's own text can be unpicked, and only text that shows.
    let custom_unpicked =
        question.kind == Kind::Single && answer.custom_unpicked && !custom.trim().is_empty();
    Working {
        selected,
        custom,
        custom_unpicked,
        note: cleaned(answer.note.as_deref().unwrap_or_default()),
        touched: !answer.defaulted,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::format::{Answer, SessionResult};
    use crate::tui::{Extent, SCISSORS};

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

    /// The message of a key or a paste the session refused.
    fn refused(text: &str) -> Notice {
        Notice::new(Tone::Warning, text)
    }

    #[test]
    fn a_session_is_named_by_its_title_else_by_its_id() {
        assert_eq!(state().name(), "batch");
        let titled = SESSION.replacen(r#"{"asqr": 2,"#, r#"{"asqr": 2, "title": "Batch 3","#, 1);
        let titled = SessionState::new(
            "batch",
            serde_json::from_str(&titled).expect("parses"),
            None,
        );
        assert_eq!(titled.name(), "Batch 3");
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
        assert_eq!(state.message(), Some(&refused("at most 2 options")));
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

    /// The answers a submit sends from where `state` is now.
    fn submitted(state: &mut SessionState) -> Vec<Answer> {
        go_to_tab(state, 4);
        let Effect::Submit { answers, .. } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on Submit submits");
        };
        answers
    }

    #[test]
    fn backspace_clears_a_pick_and_the_question_comes_back_skipped() {
        let mut state = state();
        go_to_tab(&mut state, 3);
        press(&mut state, "1h");
        assert_eq!(answer(&state, "plain").selected, ["only"]);

        assert_eq!(state.handle(code(KeyCode::Backspace)), Effect::DraftChanged);

        assert_eq!((state.tab(), state.row()), (3, 0), "the cursor stays");
        assert_eq!(answer(&state, "plain").selected, Vec::<String>::new());
        assert_eq!(submitted(&mut state)[3], Answer::skipped("plain"));
    }

    #[test]
    fn backspace_clears_an_untouched_default_and_its_flag() {
        let mut state = state();
        press(&mut state, "j");

        state.handle(code(KeyCode::Backspace));

        assert_eq!(answer(&state, "single"), Answer::new("single"));
        assert_eq!(
            state.counts(),
            Counts {
                answered: 0,
                skipped: 4,
                defaulted: 0,
                questions: 4,
            }
        );
        assert_eq!(submitted(&mut state)[0], Answer::skipped("single"));
    }

    #[test]
    fn a_cleared_default_keeps_its_note_and_comes_back_skipped_with_it() {
        let mut state = state();
        press(&mut state, "nnot sure");
        state.handle(code(KeyCode::Enter));
        press(&mut state, "j");

        state.handle(code(KeyCode::Backspace));

        assert_eq!(
            submitted(&mut state)[0],
            Answer {
                note: Some("not sure".into()),
                ..Answer::skipped("single")
            }
        );
    }

    #[test]
    fn a_cleared_pick_stays_cleared_through_the_draft() {
        let mut state = state();
        press(&mut state, "j");
        state.handle(code(KeyCode::Backspace));
        let session = || serde_json::from_str(SESSION).expect("parses");

        let restored = SessionState::new("batch", session(), Some(&state.to_draft()));
        state.replace_session(session());

        assert_eq!(answer(&restored, "single"), Answer::new("single"));
        assert_eq!(answer(&state, "single"), Answer::new("single"));
    }

    #[test]
    fn backspace_clears_only_on_the_picked_options_row() {
        // Moving around never clears a pick by accident.
        let mut state = state();

        assert_eq!(state.handle(code(KeyCode::Backspace)), Effect::None);

        assert_eq!(answer(&state, "single").selected, ["b"]);
        assert!(answer(&state, "single").defaulted);
    }

    #[test]
    fn enter_on_the_picked_option_keeps_it_picked() {
        // Going back and pressing enter on the same answer again is an
        // ordinary step, never a way to clear it.
        let mut state = state();
        press(&mut state, "2hj");

        state.handle(code(KeyCode::Enter));

        assert_eq!(answer(&state, "single").selected, ["b"]);
        assert_eq!(state.tab(), 1, "and moves on");
    }

    #[test]
    fn backspace_where_there_is_no_pick_to_clear_changes_nothing() {
        let backspace_changes_nothing = |state: &mut SessionState, place: &str| {
            let before = state.answers();
            assert_eq!(
                state.handle(code(KeyCode::Backspace)),
                Effect::None,
                "{place}"
            );
            assert_eq!(state.answers(), before, "{place}");
        };

        let mut state = state();
        press(&mut state, "jj");
        state.handle(code(KeyCode::Esc));
        backspace_changes_nothing(&mut state, "the own-answer row after esc");
        go_to_tab(&mut state, 2);
        state.handle(code(KeyCode::Esc));
        backspace_changes_nothing(&mut state, "a text question after esc");
        go_to_tab(&mut state, 3);
        backspace_changes_nothing(&mut state, "a single without a pick");
        go_to_tab(&mut state, 4);
        backspace_changes_nothing(&mut state, "the review");
        assert!(!state.on_pick(), "the review has no question to pick on");

        let mut info = with_info();
        backspace_changes_nothing(&mut info, "an info item");
    }

    #[test]
    fn backspace_in_the_own_answer_deletes_a_character() {
        let mut state = state();
        press(&mut state, "jjab");

        state.handle(code(KeyCode::Backspace));

        assert_eq!(state.field_text().as_deref(), Some("a"));
        assert_eq!(answer(&state, "single").selected, ["b"]);
    }

    // ---------------------------------------------------------------
    // The own answer of a single counts only while it is the pick
    // ---------------------------------------------------------------

    /// The state with `own` typed into the own answer of `single`, then
    /// option `a` picked and the pick cleared again on the first row.
    fn own_text_left_unpicked() -> SessionState {
        let mut state = state();
        press(&mut state, "jjown");
        state.handle(code(KeyCode::Esc));
        press(&mut state, "1h");
        state.handle(code(KeyCode::Backspace));
        state
    }

    #[test]
    fn backspace_unpicks_the_own_answer_and_keeps_its_text() {
        let mut state = own_text_left_unpicked();

        assert_eq!(answer(&state, "single").custom.as_deref(), Some("own"));
        assert_eq!(state.question_states()[0].answer, AnswerState::Skipped);
        assert_eq!(submitted(&mut state)[0], Answer::skipped("single"));
    }

    #[test]
    fn backspace_on_an_option_row_leaves_a_picked_own_answer() {
        // Emptying its text in the field is how an own answer goes.
        let mut state = state();
        press(&mut state, "j");
        state.handle(code(KeyCode::Backspace));
        press(&mut state, "jown");
        state.handle(code(KeyCode::Esc));
        press(&mut state, "k");

        assert_eq!(state.handle(code(KeyCode::Backspace)), Effect::None);

        assert_eq!(state.question_states()[0].answer, AnswerState::Answered);
    }

    #[test]
    fn typing_after_clearing_an_empty_own_answer_counts_as_on_a_fresh_question() {
        // A clear with nothing typed leaves nothing unpicked behind, so the
        // question reads exactly as a fresh one.
        let mut state = state();
        press(&mut state, "j");
        state.handle(code(KeyCode::Backspace));
        press(&mut state, "jx");
        state.handle(code(KeyCode::Up));

        assert_eq!(
            submitted(&mut state)[0],
            Answer {
                custom: Some("x".into()),
                ..Answer::new("single")
            }
        );
    }

    #[test]
    fn typing_into_an_unpicked_own_answer_picks_it() {
        // With no option chosen, an edit of the own answer picks it, as
        // typing does on a fresh question.
        let mut state = own_text_left_unpicked();
        press(&mut state, "jj!");
        state.handle(code(KeyCode::Up));

        assert_eq!(
            submitted(&mut state)[0],
            Answer {
                custom: Some("own!".into()),
                ..Answer::new("single")
            }
        );
    }

    #[test]
    fn enter_on_the_own_answer_picks_its_text_again() {
        let mut state = own_text_left_unpicked();
        press(&mut state, "jj");

        state.handle(code(KeyCode::Enter));

        assert_eq!(
            submitted(&mut state)[0],
            Answer {
                custom: Some("own".into()),
                ..Answer::new("single")
            }
        );
    }

    #[test]
    fn an_unpicked_own_answer_stays_unpicked_through_the_draft() {
        let mut state = own_text_left_unpicked();
        let session = || serde_json::from_str(SESSION).expect("parses");

        let mut restored = SessionState::new("batch", session(), Some(&state.to_draft()));
        state.replace_session(session());

        for (state, case) in [(&mut restored, "restored"), (&mut state, "replaced")] {
            assert_eq!(
                answer(state, "single").custom.as_deref(),
                Some("own"),
                "{case}"
            );
            assert_eq!(submitted(state)[0], Answer::skipped("single"), "{case}");
        }
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
    fn up_and_down_go_through_the_rows_of_a_wrapped_own_answer_first() {
        let mut state = state();
        press(&mut state, "jjaaaaa bbbbb ccccc ddddd");
        // Drawn after a head of 2 columns in 22: `ddddd` wraps onto a row
        // of its own.
        state.set_field_shape(FieldShape {
            head: vec![ratatui::text::Span::from("> ")],
            width: 22,
        });

        assert_eq!(state.handle(code(KeyCode::Up)), Effect::None);
        assert_eq!((state.row(), state.field_cursor()), (2, Some((0, 5))));
        state.handle(code(KeyCode::Up));
        assert_eq!(
            (state.row(), state.focus()),
            (1, Focus::None),
            "the first row leaves the field"
        );
    }

    #[test]
    fn a_field_entered_from_above_starts_at_the_end_of_its_first_row() {
        let mut state = state();
        press(&mut state, "jjone");
        state.handle(ctrl('j'));
        press(&mut state, "two");
        state.handle(code(KeyCode::Up));
        state.handle(code(KeyCode::Up));
        assert_eq!(state.current_row(), Some(Row::Option(1)), "left upwards");

        state.handle(code(KeyCode::Down));
        assert_eq!(state.field_cursor(), Some((0, 3)), "from above");

        // The session note from Submit above it and from Reject below it.
        review_row(&mut state, Row::Note);
        press(&mut state, "one");
        state.handle(ctrl('j'));
        press(&mut state, "two");
        state.handle(code(KeyCode::Down));
        state.handle(code(KeyCode::Up));
        assert_eq!(state.field_cursor(), Some((1, 3)), "from below");
        state.handle(code(KeyCode::Up));
        state.handle(code(KeyCode::Up));
        assert_eq!(state.current_row(), Some(Row::Submit));
        state.handle(code(KeyCode::Down));
        assert_eq!(state.field_cursor(), Some((0, 3)), "from above");
    }

    #[test]
    fn a_field_of_one_row_is_entered_at_the_end_of_its_text() {
        let mut state = state();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Up));

        state.handle(code(KeyCode::Down));

        assert_eq!(state.field_cursor(), Some((0, 2)));
    }

    #[test]
    fn the_first_draw_of_a_field_entered_from_above_moves_its_cursor() {
        let mut state = state();
        press(&mut state, "jj");
        press(&mut state, "aaaaa bbbbb ccccc ddddd");
        state.handle(code(KeyCode::Up));
        state.handle(code(KeyCode::Down));
        assert_eq!(
            state.field_cursor(),
            Some((0, 23)),
            "one line before a draw"
        );

        let shape = FieldShape {
            head: vec![ratatui::text::Span::from("> ")],
            width: 22,
        };
        assert!(state.set_field_shape(shape.clone()));
        assert_eq!(state.field_cursor(), Some((0, 17)));
        assert!(!state.set_field_shape(shape));
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
    fn ctrl_j_adds_a_line_to_the_own_answer_and_enter_still_picks_it() {
        let mut state = state();
        press(&mut state, "jja");

        assert_eq!(state.handle(ctrl('j')), Effect::DraftChanged);
        press(&mut state, "b");
        assert_eq!(state.field_text().as_deref(), Some("a\nb"));
        state.handle(code(KeyCode::Enter));

        let single = answer(&state, "single");
        assert_eq!(
            (single.selected.is_empty(), single.custom.as_deref()),
            (true, Some("a\nb"))
        );
        assert_eq!(state.tab(), 1, "enter picked the own answer and moved on");
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
    fn up_moves_to_the_line_above_in_a_text_answer() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        press(&mut state, "one");
        state.handle(ctrl('j'));
        press(&mut state, "two");

        state.handle(code(KeyCode::Up));
        assert_eq!(state.field_cursor(), Some((0, 3)));
        state.handle(code(KeyCode::Up));

        assert_eq!(
            (state.tab(), state.focus()),
            (2, Focus::Field),
            "the only row has nowhere to go"
        );
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
    fn up_and_down_move_between_the_lines_of_the_note() {
        let mut state = state();
        press(&mut state, "na");
        state.handle(ctrl('j'));
        press(&mut state, "b");

        assert_eq!(state.handle(code(KeyCode::Up)), Effect::None);
        assert_eq!(state.field_cursor(), Some((0, 1)));
        press(&mut state, "c");
        state.handle(code(KeyCode::Down));
        assert_eq!(state.field_cursor(), Some((1, 1)));

        assert_eq!(state.focus(), Focus::Note, "the note keeps the keys");
        assert_eq!(state.note_text().as_deref(), Some("ac\nb"));

        state.handle(code(KeyCode::Up));
        assert_eq!(state.handle(code(KeyCode::Up)), Effect::None);
        assert_eq!(
            (state.focus(), state.field_cursor()),
            (Focus::Note, Some((0, 2))),
            "back at column 2, and the first row of the note leads nowhere"
        );
    }

    #[test]
    fn a_note_is_counted_against_no_limits() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        state.handle(code(KeyCode::Esc));

        press(&mut state, "nlong note");

        assert_eq!((state.field_limits(), state.field_length()), (None, None));
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
    fn n_opens_the_note_on_a_question_that_sets_note_false() {
        // Every question takes a note (ADR 68); the field is ignored.
        let mut state = state();
        go_to_tab(&mut state, 3);

        press(&mut state, "n");

        assert_eq!(state.message(), None);
        assert_eq!(state.focus(), Focus::Note);
    }

    #[test]
    fn a_message_lasts_until_the_next_key() {
        let mut state = state();
        go_to_tab(&mut state, 3);
        state.paste("1");

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
    fn the_session_note_takes_new_lines_and_enter_still_submits() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        press(&mut state, "ship it");
        state.handle(ctrl('j'));
        press(&mut state, "after the tests ");

        let Effect::Submit { note, .. } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on the note submits");
        };

        assert_eq!(note.as_deref(), Some("ship it\nafter the tests"));
    }

    #[test]
    fn the_reject_reason_takes_new_lines_and_enter_still_rejects() {
        let mut state = state();
        review_row(&mut state, Row::Reject);
        press(&mut state, "stale");
        state.handle(ctrl('j'));
        press(&mut state, "ask again");

        let Effect::Reject { reason, .. } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on the reason rejects");
        };
        assert_eq!(reason.as_deref(), Some("stale\nask again"));
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
    fn rejecting_keeps_the_session_note_trimmed() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        press(&mut state, " approved ");
        state.handle(code(KeyCode::Down));

        let Effect::Reject { reason, note, .. } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on Reject rejects");
        };
        assert_eq!((reason, note.as_deref()), (None, Some("approved")));
    }

    #[test]
    fn rejecting_keeps_an_untouched_default_flagged() {
        let mut state = state();
        review_row(&mut state, Row::Reject);

        let Effect::Reject { answers, .. } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on Reject rejects");
        };
        assert_eq!(
            answers,
            [Answer {
                selected: vec!["b".into()],
                defaulted: true,
                ..Answer::new("single")
            }]
        );
    }

    #[test]
    fn rejecting_keeps_the_answered_and_the_noted_questions() {
        let mut state = state();
        // `1` picks a and moves on to `multi`, whose note is the only thing
        // entered there; `text` and `plain` stay untouched.
        press(&mut state, "1n");
        press(&mut state, "none fit");
        state.handle(code(KeyCode::Enter));
        review_row(&mut state, Row::Reject);

        let Effect::Reject { answers, .. } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on Reject rejects");
        };
        assert_eq!(
            answers,
            [
                Answer {
                    selected: vec!["a".into()],
                    ..Answer::new("single")
                },
                Answer {
                    note: Some("none fit".into()),
                    ..Answer::skipped("multi")
                },
            ]
        );
    }

    #[test]
    fn the_reject_row_takes_an_optional_reason() {
        let reason = |typed: &str| {
            let mut state = state();
            review_row(&mut state, Row::Reject);
            assert_eq!(state.focus(), Focus::Field);
            press(&mut state, typed);
            let Effect::Reject { reason, .. } = state.handle(code(KeyCode::Enter)) else {
                panic!("enter on Reject rejects");
            };
            reason
        };

        assert_eq!(reason("  out of date ").as_deref(), Some("out of date"));
        assert_eq!(reason(""), None);
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
                defaulted: 1,
                questions: 4,
            }
        );
    }

    // ---------------------------------------------------------------
    // Info items
    // ---------------------------------------------------------------

    /// An info item with context, a question, and an info item whose file
    /// sets `note: false`; the review follows them.
    const WITH_INFO: &str = r#"{"asqr": 2, "questions": [
        {"id": "checks", "text": "All checks passed.", "kind": "info",
         "context": [{"type": "markdown", "content": "| a | b |"}]},
        {"id": "go", "text": "Publish?", "kind": "single",
         "options": [{"id": "yes", "label": "Yes"}, {"id": "no", "label": "No"}]},
        {"id": "quiet", "text": "Nothing to add.", "kind": "info", "note": false}
    ]}"#;

    fn with_info() -> SessionState {
        SessionState::new(
            "info",
            serde_json::from_str(WITH_INFO).expect("test session parses"),
            None,
        )
    }

    #[test]
    fn an_info_item_has_the_next_row_only() {
        let state = with_info();

        assert_eq!(state.rows(), [Row::Next]);
        assert_eq!(state.focus(), Focus::None);
    }

    #[test]
    fn enter_on_the_next_row_moves_on_to_the_next_tab() {
        let mut state = with_info();

        let effect = state.handle(code(KeyCode::Enter));

        assert_eq!(
            (effect, state.tab(), state.row()),
            (Effect::DraftChanged, 1, 0)
        );
    }

    #[test]
    fn digits_and_space_do_nothing_on_an_info_item() {
        let mut state = with_info();

        press(&mut state, "1 ");

        assert_eq!((state.tab(), state.row()), (0, 0));
        assert_eq!(answer(&state, "checks"), Answer::new("checks"));
    }

    #[test]
    fn n_writes_a_note_on_an_info_item_and_the_submit_sends_it_alone() {
        let mut state = with_info();
        press(&mut state, "nlooks good");
        state.handle(code(KeyCode::Enter));
        go_to_tab(&mut state, 3);

        let Effect::Submit { answers, .. } = state.handle(code(KeyCode::Enter)) else {
            panic!("enter on Submit submits");
        };

        assert_eq!(
            answers,
            [
                Answer {
                    note: Some("looks good".into()),
                    ..Answer::new("checks")
                },
                Answer::skipped("go"),
            ],
            "the info item without a note is left out"
        );
    }

    #[test]
    fn an_info_item_counts_neither_answered_nor_skipped() {
        assert_eq!(
            with_info().counts(),
            Counts {
                answered: 0,
                skipped: 1,
                defaulted: 0,
                questions: 1,
            }
        );
    }

    #[test]
    fn n_opens_the_note_on_an_info_item_that_sets_note_false() {
        let mut state = with_info();
        go_to_tab(&mut state, 2);

        press(&mut state, "n");

        assert_eq!(state.message(), None);
        assert_eq!(state.focus(), Focus::Note);
    }

    #[test]
    fn the_draft_keeps_the_note_of_an_info_item() {
        let mut state = with_info();
        press(&mut state, "nlater");
        let draft = state.to_draft();

        let restored = SessionState::new(
            "info",
            serde_json::from_str(WITH_INFO).expect("parses"),
            Some(&draft),
        );

        assert_eq!(answer(&restored, "checks").note.as_deref(), Some("later"));
    }

    #[test]
    fn a_restored_draft_drops_typed_text_on_an_info_item() {
        // The text belonged to a question of the same id that a replaced
        // file turned into an info item, which has no field to show it.
        let draft = SessionResult::draft(
            "info",
            "checks",
            vec![Answer {
                custom: Some("typed before".into()),
                ..Answer::new("checks")
            }],
        );

        let restored = SessionState::new(
            "info",
            serde_json::from_str(WITH_INFO).expect("parses"),
            Some(&draft),
        );

        assert_eq!(answer(&restored, "checks").custom, None);
        assert!(!crate::queue::draft_has_answer(&restored.to_draft()));
    }

    #[test]
    fn an_info_item_is_neither_answered_nor_skipped_and_a_blank_note_is_none() {
        let mut state = with_info();
        press(&mut state, "n ");

        let states = state.question_states();

        assert_eq!(
            states[0],
            QuestionState {
                answer: AnswerState::Info,
                note: false
            }
        );
        assert_eq!(states[2].answer, AnswerState::Info);
    }

    #[test]
    fn the_window_loads_an_info_items_entry_ahead() {
        let mut state = with_info();
        go_to_tab(&mut state, 1);

        let keys: Vec<EntryKey> = state.window().into_iter().map(|(key, _)| key).collect();

        assert_eq!(
            keys,
            [EntryKey {
                question: 0,
                option: None
            }]
        );
    }

    #[test]
    fn a_session_of_info_items_only_submits_the_notes_alone() {
        let json = r#"{"asqr": 2, "questions": [
            {"id": "a", "text": "Done.", "kind": "info"},
            {"id": "b", "text": "Also done.", "kind": "info"}]}"#;
        let mut state =
            SessionState::new("report", serde_json::from_str(json).expect("parses"), None);
        go_to_tab(&mut state, 2);

        assert_eq!(
            state.counts(),
            Counts {
                answered: 0,
                skipped: 0,
                defaulted: 0,
                questions: 0,
            }
        );
        assert_eq!(
            state.handle(code(KeyCode::Enter)),
            Effect::Submit {
                answers: Vec::new(),
                note: None
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
        assert_eq!(state.message(), Some(&refused("nothing to open here")));

        let mut state = with_pane();
        let code_entry = "o opens images only; z shows this entry full screen";
        assert_eq!(press(&mut state, "o"), Effect::None);
        assert_eq!(state.message(), Some(&refused(code_entry)));
        state.handle(code(KeyCode::Tab));
        assert_eq!(press(&mut state, "o"), Effect::None);
        assert_eq!(
            state.message(),
            Some(&refused(code_entry)),
            "from the focused pane"
        );
        press(&mut state, "z");
        assert_eq!(press(&mut state, "o"), Effect::None);
        assert_eq!(
            state.message(),
            Some(&refused("o opens images only")),
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

    /// The keys of the entries `state` keeps loaded, in their order.
    fn window(state: &SessionState) -> Vec<EntryKey> {
        state.window().into_iter().map(|(key, _)| key).collect()
    }

    fn key(question: usize, option: Option<usize>) -> EntryKey {
        EntryKey { question, option }
    }

    #[test]
    fn the_window_goes_from_the_shown_entry_down_then_to_the_next_question_and_the_one_before() {
        let session = r#"{"asqr": 2, "questions": [
            {"id": "first", "text": "?", "kind": "single", "custom": true,
             "context": [{"type": "code", "content": "Q"}],
             "options": [
               {"id": "a", "label": "A", "context": [{"type": "code", "content": "A"}]},
               {"id": "b", "label": "B"},
               {"id": "c", "label": "C", "context": [{"type": "code", "content": "C"}]},
               {"id": "d", "label": "D", "context": [{"type": "code", "content": "D"}]}]},
            {"id": "second", "text": "?", "kind": "single",
             "context": [{"type": "code", "content": "R"}],
             "options": [
               {"id": "x", "label": "X", "context": [{"type": "code", "content": "X"}]},
               {"id": "y", "label": "Y"}]}
        ]}"#;
        let mut state = SessionState::new(
            "batch",
            serde_json::from_str(session).expect("test session parses"),
            None,
        );
        press(&mut state, "jj");

        assert_eq!(
            window(&state),
            [
                key(0, Some(2)),
                key(0, Some(3)),
                key(0, None),
                key(0, Some(0)),
                key(1, Some(0)),
                key(1, None)
            ],
            "the shown C, then D, the own answer's Q, A, and the next question"
        );

        go_to_tab(&mut state, 1);
        assert_eq!(
            window(&state),
            [
                key(1, Some(0)),
                key(1, None),
                key(0, Some(0)),
                key(0, None),
                key(0, Some(2)),
                key(0, Some(3))
            ],
            "then the question before"
        );

        go_to_tab(&mut state, 2);
        assert_eq!(window(&state), [], "the review keeps none");
    }

    #[test]
    fn a_row_without_context_keeps_the_others_in_the_window() {
        let session = r#"{"asqr": 2, "questions": [
            {"id": "q", "text": "?", "kind": "single",
             "options": [
               {"id": "a", "label": "A"},
               {"id": "b", "label": "B", "context": [{"type": "code", "content": "B"}]},
               {"id": "c", "label": "C", "context": [{"type": "code", "content": "C"}]}]}
        ]}"#;
        let state = SessionState::new(
            "batch",
            serde_json::from_str(session).expect("test session parses"),
            None,
        );

        assert_eq!(window(&state), [key(0, Some(1)), key(0, Some(2))]);
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
        state.set_pane_page(Some(PanePage::shown(PANE)));
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

    /// While the pane shows `loading…`, its one line says nothing of how
    /// far the entry scrolls: a scroll key is taken and moves nothing, so
    /// the entry shows where the person left it once it loaded.
    #[test]
    fn a_scroll_key_while_the_entry_loads_moves_nothing() {
        let mut state = with_pane();
        state.handle(code(KeyCode::PageDown));
        state.handle(code(KeyCode::PageDown));
        assert_eq!(top(&state), 16);
        let loading = Page {
            rows: Extent {
                shown: 10,
                total: 1,
            },
            columns: Extent::default(),
        };
        state.set_pane_page(Some(PanePage {
            page: loading,
            loading: true,
            diff: None,
        }));

        assert_eq!(state.handle(code(KeyCode::PageDown)), Effect::None);
        assert_eq!(state.handle(ctrl('u')), Effect::None);

        assert_eq!(top(&state), 16);
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
    fn shift_tab_acts_like_tab() {
        let back_tab = code(KeyCode::BackTab);
        let mut state = with_pane();
        for back in [back_tab, code(KeyCode::Tab)] {
            assert_eq!(state.handle(back_tab), Effect::None);
            assert!(state.pane_focused(), "into the pane");
            state.handle(back);
            assert!(!state.pane_focused(), "{back:?} gives the focus back");
        }

        press(&mut state, "z");
        state.handle(back_tab);
        assert!(!state.full_screen(), "it leaves the full-screen view");
    }

    #[test]
    fn shift_tab_never_types_into_a_field() {
        let back_tab = code(KeyCode::BackTab);
        // The own answer and a text answer's field, without a pane and
        // with one.
        for (mut state, question) in [(state(), "single"), (with_pane(), "q")] {
            press(&mut state, "jjab");
            assert_eq!(state.handle(back_tab), Effect::None);
            state.handle(code(KeyCode::Esc));
            assert_eq!(answer(&state, question).custom.as_deref(), Some("ab"));
        }
        let mut state = state();
        go_to_tab(&mut state, 2);
        press(&mut state, "ab");
        state.handle(back_tab);
        assert_eq!(state.field_text().as_deref(), Some("ab"), "a text answer");
        state.handle(code(KeyCode::Esc));
        press(&mut state, "nab");
        state.handle(back_tab);
        assert_eq!(state.note_text().as_deref(), Some("ab"), "a note");
        for row in [Row::Note, Row::Reject] {
            review_row(&mut state, row);
            press(&mut state, "ab");
            state.handle(back_tab);
            assert_eq!(state.field_text().as_deref(), Some("ab"), "{row:?}");
        }
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
        assert_eq!(scrolled(code(KeyCode::PageDown)).top, 9);
        assert_eq!(scrolled(code(KeyCode::Char('G'))).top, 30);
        assert_eq!(
            scrolled(code(KeyCode::Char('j'))).top,
            30,
            "stops at the end"
        );
        assert_eq!(scrolled(code(KeyCode::Home)).top, 0);
        assert_eq!(scrolled(code(KeyCode::End)).top, 30);
        assert_eq!(scrolled(code(KeyCode::Char('g'))).top, 0);
        assert_eq!(scrolled(code(KeyCode::Char('l'))).left, 4);
        assert_eq!(scrolled(code(KeyCode::Right)).left, 8);
        assert_eq!(scrolled(code(KeyCode::Left)).left, 4);
        assert_eq!(scrolled(code(KeyCode::Char('h'))).left, 0);
        assert_eq!((state.tab(), state.row()), (0, 0), "the question stays");
    }

    /// Where the pane's entry starts after each of `ctrl-d`, `ctrl-d` and
    /// `ctrl-u`.
    fn half_page_tops(state: &mut SessionState) -> Vec<usize> {
        [ctrl('d'), ctrl('d'), ctrl('u')]
            .into_iter()
            .map(|event| {
                assert_eq!(state.handle(event), Effect::None);
                top(state)
            })
            .collect()
    }

    /// `state`'s pane drawn as `PANE`, a diff in it in `layout`, its top
    /// translated to `translated` where the draw did.
    fn drawn_diff(
        state: &mut SessionState,
        layout: DiffLayout,
        translated: Option<(usize, usize)>,
    ) {
        state.set_pane_page(Some(PanePage {
            diff: Some(DrawnDiff {
                layout,
                fits_side_by_side: true,
                translated,
            }),
            ..PanePage::shown(PANE)
        }));
    }

    #[test]
    fn a_draw_in_another_layout_takes_the_top_it_translated_to() {
        let mut state = with_pane();
        state.handle(code(KeyCode::Tab));
        press(&mut state, "jjl");

        drawn_diff(&mut state, DiffLayout::SideBySide, Some((1, 2)));

        assert_eq!(state.pane_offset(), Offset { top: 1, left: 4 });
        assert_eq!(
            state.pane_layout_tag(),
            LayoutTag {
                layout: DiffLayout::SideBySide,
                back: Some((1, 2)),
            }
        );
    }

    /// A pin of the offsets a draw does not translate: they stay as the
    /// keys left them, also past what the draw could show.
    #[test]
    fn a_draw_in_the_same_layout_leaves_the_offset() {
        let mut state = with_pane();
        state.handle(code(KeyCode::Tab));
        press(&mut state, "G");

        drawn_diff(&mut state, DiffLayout::Unified, None);

        assert_eq!(top(&state), 30);
        assert_eq!(state.pane_layout_tag(), LayoutTag::default());
    }

    // -----------------------------------------------------------------
    // Side by side
    // -----------------------------------------------------------------

    /// A question whose first two options show a diff each, the third a
    /// code entry and the fourth nothing, and a text question after it.
    const DIFFS: &str = r#"{"asqr": 2, "questions": [
        {"id": "q", "text": "?", "kind": "single",
         "options": [
            {"id": "a", "label": "A", "context": [{"type": "diff", "old": {"content": "a\n"}, "new": {"content": "b\n"}}]},
            {"id": "b", "label": "B", "context": [{"type": "diff", "old": {"content": "c\n"}, "new": {"content": "d\n"}}]},
            {"id": "c", "label": "C", "context": [{"type": "code", "content": "x"}]},
            {"id": "d", "label": "D"}]},
        {"id": "t", "text": "?", "kind": "text"}
    ]}"#;

    /// The pane as it showed a diff unified, with room for two columns
    /// where `fits`.
    fn diff_page(fits: bool) -> PanePage {
        PanePage {
            diff: Some(DrawnDiff {
                layout: DiffLayout::Unified,
                fits_side_by_side: fits,
                translated: None,
            }),
            ..PanePage::shown(PANE)
        }
    }

    /// The session of [`DIFFS`], its pane drawn as `page`.
    fn diffs(page: Option<PanePage>) -> SessionState {
        let mut state = SessionState::new(
            "diffs",
            serde_json::from_str(DIFFS).expect("test session parses"),
            None,
        );
        state.set_pane_page(page);
        state
    }

    #[test]
    fn s_switches_a_diff_in_the_focused_pane_and_back() {
        let mut state = diffs(Some(diff_page(true)));
        state.handle(code(KeyCode::Tab));

        assert_eq!(press(&mut state, "s"), Effect::None);
        assert!(state.side_by_side());
        assert_eq!(state.message(), None);
        press(&mut state, "s");
        assert!(!state.side_by_side(), "s again shows it unified");
        assert!(state.pane_focused());
    }

    #[test]
    fn s_switches_a_diff_in_the_full_screen_view() {
        let mut state = diffs(Some(diff_page(true)));
        press(&mut state, "z");

        press(&mut state, "s");

        assert!(state.side_by_side());
        assert!(state.full_screen());
    }

    #[test]
    fn s_switches_a_diff_from_the_question_rows() {
        let mut state = diffs(Some(diff_page(true)));

        press(&mut state, "s");

        assert!(state.side_by_side());
        assert_eq!((state.row(), state.focus()), (0, Focus::None));
        assert!(!state.pane_focused());
    }

    #[test]
    fn s_in_a_field_or_a_note_types() {
        let mut state = diffs(Some(diff_page(true)));
        press(&mut state, "ns");
        assert_eq!(state.note_text().as_deref(), Some("s"), "a note");
        assert!(!state.side_by_side());

        state.handle(code(KeyCode::Esc));
        go_to_tab(&mut state, 1);
        press(&mut state, "s");
        assert_eq!(state.field_text().as_deref(), Some("s"), "a text answer");
    }

    #[test]
    fn s_without_an_entry_says_there_is_nothing_to_show() {
        let mut state = diffs(Some(diff_page(true)));
        press(&mut state, "jjj");

        press(&mut state, "s");

        assert_eq!(
            state.message(),
            Some(&refused("nothing to show side by side here"))
        );
    }

    #[test]
    fn s_on_an_entry_that_is_no_diff_says_so() {
        let mut state = diffs(Some(PanePage::shown(PANE)));
        press(&mut state, "jj");

        press(&mut state, "s");

        assert_eq!(
            state.message(),
            Some(&refused("only a diff shows side by side"))
        );
        assert!(!state.side_by_side());
    }

    #[test]
    fn s_on_a_diff_that_cannot_be_shown_says_so() {
        let mut state = diffs(Some(PanePage::shown(PANE)));

        press(&mut state, "s");

        assert_eq!(
            state.message(),
            Some(&refused("this diff cannot be shown side by side"))
        );
        assert!(!state.side_by_side());
    }

    #[test]
    fn s_where_two_columns_do_not_fit_points_to_the_full_screen_view() {
        let mut state = diffs(Some(diff_page(false)));

        press(&mut state, "s");

        assert_eq!(
            state.message(),
            Some(&refused(
                "too narrow for side by side; z shows it full screen"
            ))
        );
        assert!(!state.side_by_side());
    }

    #[test]
    fn s_in_a_full_screen_view_too_narrow_says_so() {
        let mut state = diffs(Some(diff_page(false)));
        press(&mut state, "z");

        press(&mut state, "s");

        assert_eq!(
            state.message(),
            Some(&refused("too narrow for side by side"))
        );
    }

    #[test]
    fn s_where_side_by_side_no_longer_fits_turns_it_off_and_says_why() {
        let mut state = diffs(Some(diff_page(true)));
        press(&mut state, "s");
        assert!(state.side_by_side());
        state.set_pane_page(Some(diff_page(false)));

        press(&mut state, "s");

        assert!(!state.side_by_side());
        assert_eq!(
            state.message(),
            Some(&refused(
                "too narrow for side by side, switched to unified; z shows it full screen"
            ))
        );
    }

    #[test]
    fn s_in_a_full_screen_view_where_side_by_side_no_longer_fits_says_why() {
        let mut state = diffs(Some(diff_page(true)));
        press(&mut state, "zs");
        assert!(state.side_by_side());
        state.set_pane_page(Some(diff_page(false)));

        press(&mut state, "s");

        assert!(!state.side_by_side());
        assert_eq!(
            state.message(),
            Some(&refused("too narrow for side by side, switched to unified"))
        );
    }

    #[test]
    fn s_twice_before_a_draw_switches_back_without_a_message() {
        let mut state = diffs(Some(diff_page(true)));

        press(&mut state, "ss");

        assert!(!state.side_by_side());
        assert_eq!(state.message(), None);
    }

    #[test]
    fn s_beside_the_hint_switches_and_points_to_z() {
        let mut state = diffs(None);

        press(&mut state, "s");

        assert!(state.side_by_side());
        assert_eq!(
            state.message(),
            Some(&Notice::new(Tone::Info, "side by side shows with z"))
        );
    }

    #[test]
    fn s_while_the_diff_loads_switches() {
        let mut state = diffs(Some(PanePage {
            loading: true,
            ..PanePage::shown(PANE)
        }));

        press(&mut state, "s");

        assert!(state.side_by_side());
        assert_eq!(state.message(), None);
    }

    #[test]
    fn another_entry_shows_unified_and_so_does_coming_back() {
        let mut state = diffs(Some(diff_page(true)));
        press(&mut state, "s");
        assert!(state.side_by_side());

        press(&mut state, "j");
        assert!(!state.side_by_side(), "option B's diff");
        press(&mut state, "k");
        assert!(!state.side_by_side(), "option A's diff again");
    }

    #[test]
    fn a_replaced_session_file_keeps_the_switch_of_the_entry_shown() {
        let mut state = diffs(Some(diff_page(true)));
        press(&mut state, "s");

        state.replace_session(serde_json::from_str(DIFFS).expect("test session parses"));

        assert!(state.side_by_side());
    }

    #[test]
    fn ctrl_u_and_ctrl_d_scroll_the_focused_pane_by_half_its_height() {
        let mut state = with_pane();
        state.handle(code(KeyCode::Tab));
        assert_eq!(half_page_tops(&mut state), [5, 10, 5]);
    }

    #[test]
    fn ctrl_u_and_ctrl_d_scroll_the_full_screen_view_by_half_its_height() {
        let mut state = with_pane();
        press(&mut state, "z");
        assert!(state.full_screen());
        assert_eq!(half_page_tops(&mut state), [5, 10, 5]);
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

    /// A question with an image entry, `caption` beside its path; no pane
    /// was drawn yet, as in the one-line hint.
    fn captioned(caption: &str) -> SessionState {
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "single",
                "context": [{{"type": "image", "path": "/pictures/p.png", "caption": {caption:?}}}],
                "options": [{{"id": "a", "label": "A"}}]}}]}}"#
        );
        SessionState::new(
            "caption",
            serde_json::from_str(&json).expect("parses"),
            None,
        )
    }

    #[test]
    fn z_from_a_pane_shows_a_captioned_image_alone_and_then_goes_back() {
        let mut state = captioned("Draft 3");
        state.set_pane_page(Some(PanePage::shown(PANE)));

        press(&mut state, "z");
        assert_eq!(
            (state.full_screen(), state.full_screen_alone()),
            (true, true)
        );

        press(&mut state, "z");
        assert!(!state.full_screen());
    }

    #[test]
    fn z_from_the_hint_shows_the_caption_then_the_image_alone_then_goes_back() {
        // Below the size a pane needs, `z` is the only way to the caption.
        let mut state = captioned("Draft 3");

        press(&mut state, "z");
        assert_eq!(
            (state.full_screen(), state.full_screen_alone()),
            (true, false)
        );
        // The full-screen view draws a pane, which the run loop hands on.
        state.set_pane_page(Some(PanePage::shown(PANE)));

        press(&mut state, "z");
        assert_eq!(
            (state.full_screen(), state.full_screen_alone()),
            (true, true)
        );

        press(&mut state, "z");
        assert!(!state.full_screen());
    }

    #[test]
    fn z_from_the_hint_shows_an_entry_without_a_caption_in_one_step() {
        for caption in ["", "  "] {
            let mut state = captioned(caption);

            press(&mut state, "z");
            assert_eq!(
                (state.full_screen(), state.full_screen_alone()),
                (true, true),
                "{caption:?}"
            );
            state.set_pane_page(Some(PanePage::shown(PANE)));

            press(&mut state, "z");
            assert!(!state.full_screen(), "{caption:?}");
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
        state.set_pane_page(Some(PanePage::shown(PANE)));
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
        let mut tops = Vec::new();

        for event in [code(KeyCode::PageDown), ctrl('d')] {
            state.handle(event);
            tops.push(top(&state));
        }
        assert_eq!(press(&mut state, "j"), Effect::None);
        assert_eq!(state.row(), 1, "j still moves the cursor");

        // The own answer shows the question's entry again.
        press(&mut state, "j");
        assert_eq!(state.focus(), Focus::Field);
        press(&mut state, "ab");
        state.handle(code(KeyCode::Left));
        for event in [ctrl('u'), ctrl('u'), code(KeyCode::PageUp)] {
            state.handle(event);
            tops.push(top(&state));
        }
        assert_eq!(tops, [8, 13, 8, 3, 0]);
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
        assert_eq!(state.handle(ctrl('v')), Effect::None);
        assert_eq!(state.field_cursor(), Some((0, 1)), "the text cursor stays");
    }

    #[test]
    fn each_entry_keeps_its_own_position() {
        let mut state = with_pane();
        state.handle(code(KeyCode::PageDown));
        assert_eq!(top(&state), 8);

        press(&mut state, "j");
        assert_eq!(top(&state), 0, "the option's entry starts at its top");
        state.handle(code(KeyCode::PageDown));
        state.handle(code(KeyCode::PageDown));
        assert_eq!(top(&state), 16);

        press(&mut state, "k");
        assert_eq!(top(&state), 8, "back on the question's entry");
        state.handle(code(KeyCode::Right));
        state.handle(code(KeyCode::Esc));
        state.handle(code(KeyCode::Left));
        assert_eq!(top(&state), 8, "kept while moving between questions");
        press(&mut state, "j");
        assert_eq!(top(&state), 16, "and the option's too");
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

    // ---------------------------------------------------------------
    // Pasting
    // ---------------------------------------------------------------

    #[test]
    fn a_paste_into_the_own_answer_keeps_its_lines() {
        let mut state = state();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Left));

        assert_eq!(state.paste("x\ny\r\nz\rw"), Effect::DraftChanged);

        assert_eq!(state.field_text().as_deref(), Some("ax\ny\nz\nwb"));
        let single = answer(&state, "single");
        assert_eq!(single.custom.as_deref(), Some("ax\ny\nz\nwb"));
        assert!(!single.defaulted, "a paste is an edit");
    }

    #[test]
    fn a_paste_into_the_session_note_or_the_reject_reason_keeps_its_lines() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        state.paste("only if\nthe tests pass");
        assert_eq!(state.session_note(), "only if\nthe tests pass");

        state.handle(code(KeyCode::Down));
        state.paste("out\r\nof date");
        assert_eq!(state.reject_reason(), "out\nof date");
    }

    #[test]
    fn a_paste_into_a_text_answer_or_a_note_keeps_its_lines() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        state.paste("one\r\ntwo\rthree");
        assert_eq!(
            answer(&state, "text").custom.as_deref(),
            Some("one\ntwo\nthree")
        );

        state.handle(code(KeyCode::Esc));
        press(&mut state, "n");
        assert_eq!(state.paste("a\nb"), Effect::DraftChanged);
        assert_eq!(answer(&state, "text").note.as_deref(), Some("a\nb"));
    }

    #[test]
    fn a_paste_on_a_field_left_with_esc_gives_it_the_keys_again() {
        // The own answer, a text answer, the session note and the reject
        // reason, each left with `esc` after typing `ab` and moving left.
        type Reach = fn(&mut SessionState);
        let rows: [(&str, Reach); 4] = [
            ("own answer", |state| {
                press(state, "jj");
            }),
            ("text answer", |state| go_to_tab(state, 2)),
            ("session note", |state| review_row(state, Row::Note)),
            ("reject reason", |state| review_row(state, Row::Reject)),
        ];
        for (name, reach) in rows {
            let mut state = state();
            reach(&mut state);
            press(&mut state, "ab");
            state.handle(code(KeyCode::Left));
            state.handle(code(KeyCode::Esc));
            assert_eq!(state.focus(), Focus::None, "{name}");

            assert_eq!(state.paste("c"), Effect::DraftChanged, "{name}");

            assert_eq!(state.focus(), Focus::Field, "{name}");
            assert_eq!(
                state.field_text().as_deref(),
                Some("abc"),
                "{name}: at the end of the text"
            );
            assert_eq!(state.message(), None, "{name}");
        }
    }

    #[test]
    fn a_paste_where_no_field_can_take_it_says_where_it_goes() {
        let mut state = state();
        assert_eq!(state.paste("1"), Effect::None);
        assert_eq!(
            state.message(),
            Some(&refused("a paste goes into a field; press n for a note"))
        );
        assert_eq!(state.answers()[0].selected, ["b"], "nothing picked");
        press(&mut state, "j");
        assert_eq!(
            state.message(),
            None,
            "the message lasts until the next key"
        );

        go_to_tab(&mut state, 3);
        state.paste("1");
        assert_eq!(
            state.message(),
            Some(&refused("a paste goes into a field; press n for a note")),
            "a question whose file sets note: false takes a note all the same"
        );
        review_row(&mut state, Row::Submit);
        state.paste("1");
        assert_eq!(state.message(), Some(&refused("a paste goes into a field")));
    }

    #[test]
    fn a_paste_into_the_pane_says_how_to_leave_it() {
        let mut state = with_pane();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Tab));

        assert_eq!(state.paste("c"), Effect::None);
        assert_eq!(
            state.message(),
            Some(&refused("a paste goes into a field; esc leaves the pane"))
        );
        press(&mut state, "z");
        state.paste("c");
        assert_eq!(
            state.message(),
            Some(&refused(
                "a paste goes into a field; z/esc leave the full screen"
            ))
        );

        state.handle(code(KeyCode::Esc));
        state.handle(code(KeyCode::Esc));
        assert_eq!(state.field_text().as_deref(), Some("ab"), "nothing went in");
    }

    #[test]
    fn a_paste_over_the_help_only_closes_it() {
        for mut state in [state(), with_pane()] {
            press(&mut state, "jjab");
            state.handle(code(KeyCode::Esc));
            press(&mut state, "?");

            assert_eq!(state.paste("c"), Effect::None);

            assert!(!state.help());
            assert_eq!(state.message(), None);
            assert_eq!(state.focus(), Focus::None, "the field keeps no keys");
            assert_eq!(
                answer(&state, state.question().expect("a question").id.as_str())
                    .custom
                    .as_deref(),
                Some("ab")
            );
        }
    }

    #[test]
    fn an_empty_paste_changes_nothing() {
        let mut state = state();
        press(&mut state, "jjab");

        assert_eq!(state.paste(""), Effect::None);
        assert_eq!(state.field_text().as_deref(), Some("ab"));
    }

    #[test]
    fn a_draft_is_cleaned_like_a_paste() {
        let mut draft = SessionResult::draft(
            "batch",
            "single",
            vec![Answer {
                custom: Some("a\tb".into()),
                note: Some("c\u{7}d".into()),
                ..Answer::new("single")
            }],
        );
        draft.note = Some("\tnote".into());
        draft.reason = Some("re\u{1b}ason".into());

        let state = SessionState::new(
            "batch",
            serde_json::from_str(SESSION).expect("parses"),
            Some(&draft),
        );

        let single = answer(&state, "single");
        assert_eq!(single.custom.as_deref(), Some("a   b"));
        assert_eq!(single.note.as_deref(), Some("cd"));
        assert_eq!(state.session_note(), "    note");
        assert_eq!(state.reject_reason(), "reason");
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

    /// The test session with the own answer of `single` switched off and
    /// that of `multi` left out.
    fn without_own_answers() -> Session {
        let switched_off = SESSION
            .replacen(
                r#""kind": "single", "custom": true"#,
                r#""kind": "single", "custom": false"#,
                1,
            )
            .replacen(r#""max": 2, "custom": true"#, r#""max": 2"#, 1);
        serde_json::from_str(&switched_off).expect("parses")
    }

    #[test]
    fn a_restored_draft_drops_own_text_on_a_single_without_an_own_answer() {
        // The question has no own-answer row any more, so the text could
        // neither be seen nor cleared, and a submit would send it.
        let draft = SessionResult::draft(
            "batch",
            "single",
            vec![Answer {
                custom: Some("typed before".into()),
                custom_unpicked: true,
                ..Answer::new("single")
            }],
        );

        let restored = SessionState::new("batch", without_own_answers(), Some(&draft));

        let single = answer(&restored, "single");
        assert_eq!(single.custom, None);
        assert!(!single.custom_unpicked);
    }

    #[test]
    fn a_restored_draft_drops_own_text_on_a_multi_without_an_own_answer() {
        let draft = SessionResult::draft(
            "batch",
            "multi",
            vec![Answer {
                selected: vec!["x".into()],
                custom: Some("typed before".into()),
                ..Answer::new("multi")
            }],
        );

        let restored = SessionState::new("batch", without_own_answers(), Some(&draft));

        let multi = answer(&restored, "multi");
        assert_eq!(multi.custom, None);
        assert_eq!(multi.selected, ["x"], "the options stay");
    }

    #[test]
    fn a_reload_drops_own_text_when_the_own_answer_is_turned_off() {
        let mut state = state();
        press(&mut state, "jjtyped");
        assert_eq!(answer(&state, "single").custom.as_deref(), Some("typed"));

        state.replace_session(without_own_answers());

        assert_eq!(answer(&state, "single").custom, None);
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

    /// The test session with the default of `single` on `a` instead of
    /// `b`.
    fn default_on_a() -> Session {
        let moved = SESSION
            .replace(r#""label": "B", "default": true"#, r#""label": "B""#)
            .replace(r#""label": "A""#, r#""label": "A", "default": true"#);
        serde_json::from_str(&moved).expect("parses")
    }

    #[test]
    fn a_reload_moves_an_untouched_default_with_the_file() {
        let mut state = state();

        state.replace_session(default_on_a());

        assert_eq!(
            answer(&state, "single"),
            Answer {
                selected: vec!["a".into()],
                defaulted: true,
                ..Answer::new("single")
            }
        );
    }

    #[test]
    fn a_reload_selects_a_default_added_to_an_untouched_question() {
        let mut state = state();
        let added = SESSION.replace(r#""label": "X""#, r#""label": "X", "default": true"#);

        state.replace_session(serde_json::from_str(&added).expect("parses"));

        assert_eq!(
            answer(&state, "multi"),
            Answer {
                selected: vec!["x".into()],
                defaulted: true,
                ..Answer::new("multi")
            }
        );
    }

    #[test]
    fn a_reload_drops_a_default_removed_from_an_untouched_question() {
        let mut state = state();
        let removed = SESSION.replace(r#""label": "B", "default": true"#, r#""label": "B""#);

        state.replace_session(serde_json::from_str(&removed).expect("parses"));

        assert_eq!(answer(&state, "single"), Answer::new("single"));
    }

    #[test]
    fn a_reload_keeps_the_persons_pick() {
        let mut state = state();
        press(&mut state, "1");
        let removed = SESSION.replace(r#""label": "B", "default": true"#, r#""label": "B""#);

        state.replace_session(serde_json::from_str(&removed).expect("parses"));

        assert_eq!(
            answer(&state, "single"),
            Answer {
                selected: vec!["a".into()],
                ..Answer::new("single")
            }
        );
    }

    #[test]
    fn a_restart_takes_the_current_default_of_an_untouched_question() {
        let draft = state().to_draft();

        let restored = SessionState::new("batch", default_on_a(), Some(&draft));

        assert_eq!(
            answer(&restored, "single"),
            Answer {
                selected: vec!["a".into()],
                defaulted: true,
                ..Answer::new("single")
            }
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
    fn a_session_not_yet_seen_is_reloaded_without_a_message() {
        let mut state = state();
        press(&mut state, "j");
        let titled = SESSION.replacen(r#"{"asqr": 2,"#, r#"{"asqr": 2, "title": "Batch 3","#, 1);

        state.replace_session(serde_json::from_str(&titled).expect("parses"));

        assert!(!state.seen());
        assert_eq!(state.message(), None);
        assert_eq!(
            (state.name(), state.revision(), state.row()),
            ("Batch 3", 1, 1),
            "the reload itself took effect"
        );
    }

    #[test]
    fn a_seen_session_stays_seen_across_reloads() {
        let mut state = state();
        state.mark_seen();

        state.replace_session(serde_json::from_str(SESSION).expect("parses"));
        press(&mut state, "j");
        assert_eq!(state.message(), None, "a key clears the message");
        state.replace_session(serde_json::from_str(SESSION).expect("parses"));

        assert!(state.seen());
        assert_eq!(
            state.message(),
            Some(&Notice::new(
                Tone::Info,
                "batch changed on disk and was reloaded"
            ))
        );
    }

    #[test]
    fn a_replaced_session_keeps_where_the_person_was_and_says_so() {
        let mut state = with_pane();
        state.mark_seen();
        state.handle(code(KeyCode::PageDown));
        press(&mut state, "j");
        state.handle(code(KeyCode::PageDown));
        state.handle(code(KeyCode::Tab));
        press(&mut state, "z");
        // The question's own entry changes; option b's stays as it was.
        let replacement = WITH_CONTEXT.replace("the question's", "the question's, reworded");

        state.replace_session(serde_json::from_str(&replacement).expect("parses"));

        assert_eq!((state.tab(), state.row()), (0, 1));
        assert_eq!((state.pane_focused(), state.full_screen()), (true, true));
        assert_eq!(top(&state), 8, "option b's entry is where it was");
        assert_eq!(
            state.message(),
            Some(&Notice::new(
                Tone::Info,
                "batch changed on disk and was reloaded"
            ))
        );
        state.handle(code(KeyCode::Esc));
        state.handle(code(KeyCode::Esc));
        press(&mut state, "k");
        assert_eq!(top(&state), 0, "the changed entry starts at its top");
    }

    #[test]
    fn a_replaced_session_ends_the_pane_focus_and_full_screen_when_their_entry_goes() {
        let mut state = with_pane();
        press(&mut state, "j");
        press(&mut state, "z");
        let replacement = WITH_CONTEXT.replace(
            r#", "context": [{"type": "markdown", "content": "the option's"}]"#,
            "",
        );

        state.replace_session(serde_json::from_str(&replacement).expect("parses"));

        assert_eq!(state.row(), 1, "the row stays");
        assert_eq!((state.pane_focused(), state.full_screen()), (false, false));
        press(&mut state, "k");
        assert_eq!(state.row(), 0, "keys move the cursor again");
    }

    #[test]
    fn a_replaced_session_keeps_the_review_and_the_field_typed_in() {
        let mut state = state();
        state.mark_seen();
        review_row(&mut state, Row::Reject);
        press(&mut state, "stale");
        let titled = SESSION.replacen(r#"{"asqr": 2,"#, r#"{"asqr": 2, "title": "Batch 3","#, 1);
        let fewer = titled.replace(
            r#"{"id": "text", "text": "?", "kind": "text", "length": {"warn": 5}},"#,
            "",
        );

        state.replace_session(serde_json::from_str(&fewer).expect("parses"));

        assert!(state.on_review());
        assert_eq!(state.current_row(), Some(Row::Reject));
        assert_eq!(state.field_text().as_deref(), Some("stale"), "typed on");
        assert_eq!(
            state.message(),
            Some(&Notice::new(
                Tone::Info,
                "Batch 3 changed on disk and was reloaded"
            ))
        );
    }

    #[test]
    fn a_replaced_session_clamps_the_row_to_the_rows_left() {
        let mut state = state();
        press(&mut state, "l");
        press(&mut state, "jj");
        assert_eq!(state.current_row(), Some(Row::Option(2)));
        let fewer = SESSION.replace(r#", {"id": "z", "label": "Z"}"#, "");

        state.replace_session(serde_json::from_str(&fewer).expect("parses"));

        assert_eq!(state.tab(), 1);
        assert_eq!(state.current_row(), Some(Row::Own), "the last row left");
        assert_eq!(state.focus(), Focus::Field, "a field row focuses its field");
    }

    #[test]
    fn a_replaced_session_keeps_a_field_left_with_esc_without_the_keys() {
        let mut state = state();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Esc));

        state.replace_session(serde_json::from_str(SESSION).expect("parses"));

        assert_eq!(state.current_row(), Some(Row::Own));
        assert_eq!(state.focus(), Focus::None);
        assert_eq!(press(&mut state, "q"), Effect::Quit, "keys are commands");
    }

    #[test]
    fn a_replaced_session_keeps_the_text_cursor_of_the_field_typed_in() {
        let mut state = state();
        press(&mut state, "jjabcd");
        state.handle(code(KeyCode::Left));
        state.handle(code(KeyCode::Left));

        state.replace_session(serde_json::from_str(SESSION).expect("parses"));

        assert_eq!(state.focus(), Focus::Field);
        assert_eq!(state.field_cursor(), Some((0, 2)));
        press(&mut state, "x");
        assert_eq!(state.field_text().as_deref(), Some("abxcd"));
    }

    #[test]
    fn a_replaced_session_keeps_an_open_note_open() {
        let mut state = state();
        press(&mut state, "nwhy");
        state.handle(code(KeyCode::Left));

        state.replace_session(serde_json::from_str(SESSION).expect("parses"));
        assert_eq!(state.focus(), Focus::Note);
        assert_eq!(state.field_cursor(), Some((0, 2)));
        assert_eq!(state.note_text().as_deref(), Some("why"));

        // A file that sets `note: false` changes nothing (ADR 68).
        let note_false = SESSION.replacen(
            r#""kind": "single", "custom": true,"#,
            r#""kind": "single", "custom": true, "note": false,"#,
            1,
        );
        state.replace_session(serde_json::from_str(&note_false).expect("parses"));
        assert_eq!(state.focus(), Focus::Note);
        assert_eq!(state.field_cursor(), Some((0, 2)));
        assert_eq!(state.note_text().as_deref(), Some("why"));
    }

    #[test]
    fn a_replaced_session_keeps_an_open_note_open_where_its_row_is_gone() {
        // The note belongs to the question, not to a row, so the own
        // answer turned off under it leaves it open; the cursor's row
        // falls back as it does without a note.
        let mut state = state();
        press(&mut state, "jj");
        state.handle(code(KeyCode::Esc));
        press(&mut state, "nwhy");
        assert_eq!(state.current_row(), Some(Row::Own));

        let no_custom = SESSION.replacen(r#""custom": true,"#, "", 1);
        state.replace_session(serde_json::from_str(&no_custom).expect("parses"));

        assert_eq!(state.focus(), Focus::Note);
        assert_eq!(state.note_text().as_deref(), Some("why"));
        assert_eq!(state.current_row(), Some(Row::Option(1)));
    }

    #[test]
    fn a_replaced_session_enters_the_row_afresh_when_the_field_is_gone() {
        // The own answer turned off: the cursor lands on the last option.
        let mut turned_off = state();
        press(&mut turned_off, "jjab");
        let no_custom = SESSION.replacen(r#""custom": true,"#, "", 1);
        turned_off.replace_session(serde_json::from_str(&no_custom).expect("parses"));
        assert_eq!(turned_off.current_row(), Some(Row::Option(1)));
        assert_eq!(turned_off.focus(), Focus::None);

        // The question turned into a text question: its answer field gets
        // the keys, the text cursor at the end.
        let mut state = state();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Left));
        let text = SESSION.replacen(
            r#""kind": "single", "custom": true"#,
            r#""kind": "text""#,
            1,
        );
        state.replace_session(serde_json::from_str(&text).expect("parses"));
        assert_eq!(state.current_row(), Some(Row::Answer));
        assert_eq!(state.focus(), Focus::Field);
        assert_eq!(state.field_cursor(), Some((0, 2)));
    }

    #[test]
    fn a_replaced_session_without_the_question_shown_starts_at_the_top() {
        let mut state = state();
        press(&mut state, "l");
        press(&mut state, "jj");
        let without = SESSION.replace(
            r#"{"id": "multi", "text": "?", "kind": "multi", "max": 2, "custom": true,"#,
            r#"{"id": "renamed", "text": "?", "kind": "multi", "max": 2, "custom": true,"#,
        );

        state.replace_session(serde_json::from_str(&without).expect("parses"));

        assert_eq!((state.tab(), state.row()), (0, 0));
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

    // ---------------------------------------------------------------
    // The view of the question's area
    // ---------------------------------------------------------------

    /// A draw that left `above` and `below` lines of the row's range
    /// around the view, the row at rest `home` lines from its upper end.
    fn cut(above: usize, below: usize, home: usize) -> Option<RowCut> {
        Some(RowCut { above, below, home })
    }

    #[test]
    fn down_and_up_scroll_through_a_tall_option_before_moving() {
        let mut state = state();
        state.set_row_cut(cut(0, 2, 0));

        press(&mut state, "j");
        assert_eq!((state.row(), state.row_top()), (0, RowTop::Line(1)));
        press(&mut state, "j");
        assert_eq!((state.row(), state.row_top()), (0, RowTop::Line(2)));
        press(&mut state, "j");
        assert_eq!(
            (state.row(), state.row_top()),
            (1, RowTop::Start),
            "the end of the range moves the cursor on"
        );

        press(&mut state, "k");
        assert_eq!(
            (state.row(), state.row_top()),
            (0, RowTop::End),
            "moving up shows the end"
        );
        state.set_row_cut(cut(2, 0, 0));
        state.handle(code(KeyCode::Up));
        state.handle(code(KeyCode::Up));
        assert_eq!((state.row(), state.row_top()), (0, RowTop::Line(0)));
        press(&mut state, "k");
        assert_eq!(
            (state.row(), state.row_top()),
            (0, RowTop::Line(0)),
            "nothing is above the first row's range"
        );
    }

    #[test]
    fn the_view_starts_afresh_on_every_other_way_onto_a_row() {
        let mut state = state();
        state.set_row_cut(cut(0, 2, 0));
        press(&mut state, "j");
        press(&mut state, "lh");
        assert_eq!((state.row(), state.row_top()), (0, RowTop::Start));

        // In a multi question the digits toggle and move the cursor; the
        // last `1` toggles the row the cursor is on.
        press(&mut state, "l");
        for (digit, row) in [("2", 1), ("1", 0), ("1", 0)] {
            state.set_row_cut(cut(0, 2, 0));
            press(&mut state, "j");
            assert_eq!(state.row_top(), RowTop::Line(1), "{digit}");
            press(&mut state, digit);
            assert_eq!((state.row(), state.row_top()), (row, RowTop::Start));
        }
    }

    #[test]
    fn a_field_scrolled_away_scrolls_back_before_its_cursor_moves() {
        // The answer of the text question is its first and last row; at
        // rest the area stands 3 lines into its range.
        let mut state = state();
        go_to_tab(&mut state, 2);
        press(&mut state, "ab");
        state.set_row_cut(cut(3, 0, 3));

        state.handle(code(KeyCode::Up));
        state.handle(ctrl('p'));
        assert_eq!(state.row_top(), RowTop::Line(1));
        state.handle(code(KeyCode::Down));
        assert_eq!(state.row_top(), RowTop::Line(2));
        assert_eq!(
            (state.focus(), state.field_cursor()),
            (Focus::Field, Some((0, 2))),
            "the text cursor stays"
        );
        press(&mut state, "c");
        assert_eq!(state.row_top(), RowTop::Start, "typing shows the field");
        assert_eq!(state.field_text().as_deref(), Some("abc"));

        state.set_row_cut(cut(3, 0, 3));
        state.handle(code(KeyCode::Up));
        state.paste("d");
        assert_eq!(state.row_top(), RowTop::Start, "a paste shows the field");

        // Scrolled below the field, as on a last row with lines under it.
        state.set_row_cut(cut(0, 2, 0));
        state.handle(code(KeyCode::Down));
        assert_eq!(state.row_top(), RowTop::Line(1));
        state.handle(ctrl('n'));
        state.handle(code(KeyCode::Up));
        assert_eq!(state.row_top(), RowTop::Line(1), "up goes back first");
    }

    /// `state` with the question's area scrolled a line up from where the
    /// row under the cursor rests.
    fn scrolled_up(state: &mut SessionState) {
        state.set_row_cut(cut(2, 0, 2));
        state.handle(code(KeyCode::Up));
        assert_eq!(state.row_top(), RowTop::Line(1));
    }

    #[test]
    fn keys_that_do_not_act_on_the_question_keep_its_view() {
        // The image question, where `z` shows the image full screen and
        // `o` opens it, so each key does what it is for.
        let moved: Vec<char> = ['?', 'o', 'z', 's', 'L', 'q']
            .into_iter()
            .filter(|&key| {
                let mut state = state();
                go_to_tab(&mut state, 3);
                scrolled_up(&mut state);
                press(&mut state, &key.to_string());
                state.row_top() != RowTop::Line(1)
            })
            .collect();

        assert_eq!(moved, [], "these keys moved the view");
    }

    #[test]
    fn keys_that_act_on_the_question_put_the_view_back() {
        // On the multi question `n`, `space` and `1` on the first row
        // leave the cursor on its row, so the view it shows is the one
        // the key left.
        for key in [
            code(KeyCode::Char('n')),
            code(KeyCode::Char(' ')),
            code(KeyCode::Char('1')),
        ] {
            let mut state = state();
            go_to_tab(&mut state, 1);
            scrolled_up(&mut state);
            state.handle(key);
            assert_eq!(
                (state.row(), state.row_top()),
                (0, RowTop::Start),
                "{key:?}"
            );
        }

        // `enter` on the review's Submit stays there.
        let mut review = state();
        go_to_tab(&mut review, 4);
        let submit = review.row();
        scrolled_up(&mut review);
        review.handle(code(KeyCode::Enter));
        assert_eq!(
            (review.row(), review.row_top()),
            (submit, RowTop::Start),
            "enter"
        );

        // `backspace` on the default the single question starts with.
        let mut single = state();
        press(&mut single, "j");
        assert!(single.on_pick());
        scrolled_up(&mut single);
        single.handle(code(KeyCode::Backspace));
        assert_eq!(
            (single.row(), single.row_top()),
            (1, RowTop::Start),
            "backspace"
        );
    }

    #[test]
    fn keys_the_context_pane_takes_keep_the_questions_view() {
        let mut state = with_pane();
        state.set_row_cut(cut(2, 0, 2));
        state.handle(code(KeyCode::Up));
        state.handle(code(KeyCode::PageDown));
        state.handle(code(KeyCode::Tab));
        assert_eq!(state.row_top(), RowTop::Line(1));
    }

    #[test]
    fn moving_onto_a_field_stores_no_view() {
        let mut state = state();
        press(&mut state, "jj");
        assert_eq!(state.current_row(), Some(Row::Own));
        assert_eq!(state.row_top, None);
    }

    #[test]
    fn a_replaced_session_keeps_the_view_of_its_row() {
        let mut state = state();
        state.set_row_cut(cut(0, 2, 0));
        press(&mut state, "j");

        state.replace_session(serde_json::from_str(SESSION).expect("parses"));
        assert_eq!((state.row(), state.row_top()), (0, RowTop::Line(1)));

        // With the row gone, its view goes too.
        press(&mut state, "l");
        press(&mut state, "jj");
        state.set_row_cut(cut(0, 2, 0));
        press(&mut state, "j");
        let fewer = SESSION.replace(r#", {"id": "z", "label": "Z"}"#, "");
        state.replace_session(serde_json::from_str(&fewer).expect("parses"));
        assert_eq!(state.row_top(), RowTop::Start);
    }

    // ---------------------------------------------------------------
    // ctrl-g: the field in the person's editor (ADR 74)
    // ---------------------------------------------------------------

    /// What `ctrl-g` asks for on question `index` of [`SESSION`] (each of
    /// whose questions is `?`), in its `field`.
    fn about_question(index: usize, field: FieldName) -> FieldAbout {
        let ids = ["single", "multi", "text", "plain"];
        FieldAbout {
            title: None,
            id: "batch".to_owned(),
            question: Some(QuestionAbout {
                number: index + 1,
                count: 4,
                label: ids[index].to_owned(),
                text: "?".to_owned(),
            }),
            field,
        }
    }

    fn edit_of(text: &str, about: FieldAbout) -> Effect {
        Effect::Edit(EditRequest {
            text: text.to_owned(),
            about,
        })
    }

    #[test]
    fn ctrl_g_asks_for_the_editor_in_the_own_answer() {
        let mut state = state();
        press(&mut state, "jjab");

        assert_eq!(
            state.handle(ctrl('g')),
            edit_of("ab", about_question(0, FieldName::OwnAnswer))
        );
    }

    #[test]
    fn ctrl_g_asks_for_the_editor_in_a_text_answer() {
        let mut state = state();
        go_to_tab(&mut state, 2);
        press(&mut state, "ab");

        assert_eq!(
            state.handle(ctrl('g')),
            edit_of("ab", about_question(2, FieldName::Answer))
        );
    }

    #[test]
    fn ctrl_g_asks_for_the_editor_in_a_note() {
        let mut state = state();
        press(&mut state, "nab");

        assert_eq!(
            state.handle(ctrl('g')),
            edit_of("ab", about_question(0, FieldName::Note))
        );
    }

    #[test]
    fn ctrl_g_asks_for_the_editor_in_the_session_note() {
        let mut state = state();
        review_row(&mut state, Row::Note);
        press(&mut state, "ab");

        assert_eq!(
            state.handle(ctrl('g')),
            edit_of(
                "ab",
                FieldAbout {
                    title: None,
                    id: "batch".to_owned(),
                    question: None,
                    field: FieldName::SessionNote,
                }
            )
        );
    }

    #[test]
    fn ctrl_g_asks_for_the_editor_in_the_reject_reason() {
        let mut state = state();
        review_row(&mut state, Row::Reject);
        press(&mut state, "ab");

        let Effect::Edit(request) = state.handle(ctrl('g')) else {
            panic!("ctrl-g asks for the editor");
        };
        assert_eq!(request.about.field, FieldName::RejectReason);
    }

    #[test]
    fn ctrl_g_on_a_field_left_with_esc_gives_it_the_keys_and_asks_for_the_editor() {
        let mut state = state();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Esc));
        assert_eq!(state.focus(), Focus::None);

        let effect = state.handle(ctrl('g'));

        assert_eq!(
            effect,
            edit_of("ab", about_question(0, FieldName::OwnAnswer))
        );
        assert_eq!(state.focus(), Focus::Field);
        assert_eq!(state.field_cursor(), Some((0, 2)), "at the end of the text");
    }

    #[test]
    fn ctrl_g_does_nothing_on_an_option_row() {
        let mut state = state();
        assert_eq!(state.handle(ctrl('g')), Effect::None);
        assert_eq!(state.focus(), Focus::None);
    }

    #[test]
    fn ctrl_g_does_nothing_in_the_focused_pane() {
        let mut state = with_pane();
        state.handle(code(KeyCode::Tab));

        assert_eq!(state.handle(ctrl('g')), Effect::None);
        assert!(state.pane_focused());
    }

    #[test]
    fn ctrl_g_does_nothing_in_the_full_screen_view() {
        let mut state = with_pane();
        press(&mut state, "jjab");
        state.handle(code(KeyCode::Tab));
        press(&mut state, "z");

        assert_eq!(state.handle(ctrl('g')), Effect::None);
        assert!(state.full_screen());
    }

    #[test]
    fn ctrl_g_does_nothing_over_the_help() {
        let mut state = state();
        press(&mut state, "?");

        assert_eq!(state.handle(ctrl('g')), Effect::None);
        assert!(!state.help(), "the key closed it, as any key does");
    }

    #[test]
    fn ctrl_g_refuses_a_field_that_holds_the_cut_line() {
        let mut state = state();
        press(&mut state, "jj");
        state.paste(&format!("pasted\n{SCISSORS}\nmore"));

        assert_eq!(state.handle(ctrl('g')), Effect::None);
        assert_eq!(
            state.message(),
            Some(&refused(
                "the field holds the >8 line; remove it to edit in the editor"
            ))
        );
    }

    #[test]
    fn the_edited_text_replaces_the_field_and_saves_the_draft() {
        let mut state = state();
        press(&mut state, "jjab");
        state.handle(ctrl('g'));

        assert_eq!(
            state.edited("from the editor\nline two"),
            Effect::DraftChanged
        );

        assert_eq!(
            state.field_text().as_deref(),
            Some("from the editor\nline two")
        );
        assert_eq!(state.field_cursor(), Some((1, 8)), "at the end of the text");
        assert_eq!(
            answer(&state, "single").custom.as_deref(),
            Some("from the editor\nline two")
        );
    }

    #[test]
    fn an_edited_note_goes_into_the_note() {
        let mut state = state();
        press(&mut state, "nab");
        state.handle(ctrl('g'));

        assert_eq!(state.edited("a note"), Effect::DraftChanged);

        assert_eq!(answer(&state, "single").note.as_deref(), Some("a note"));
    }

    #[test]
    fn an_unchanged_text_saves_nothing() {
        let mut state = state();
        press(&mut state, "jjab");
        state.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        assert_eq!(state.field_text().as_deref(), Some("ab\n"));
        state.handle(ctrl('g'));

        assert_eq!(state.edited("ab"), Effect::None);

        assert_eq!(
            state.field_text().as_deref(),
            Some("ab\n"),
            "kept as it was"
        );
        assert_eq!(
            state.message(),
            Some(&Notice::new(Tone::Info, "the field is unchanged"))
        );
    }

    #[test]
    fn edited_text_is_cleaned_like_a_paste() {
        let mut state = state();
        press(&mut state, "jj");
        state.handle(ctrl('g'));

        state.edited("a\tb\x1b[31mc");

        assert_eq!(state.field_text().as_deref(), Some("a   b[31mc"));
    }
}
