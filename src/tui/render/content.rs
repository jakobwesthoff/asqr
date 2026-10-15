// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The current tab (spec sections 7.1, 7.3 and 7.4): a question with its
//! options, fields and note, or the review, beside or above the context
//! pane.
//!
//! The content of a tab is wrapped here rather than by the paragraph
//! widget, so the number of lines is known and the row under the cursor
//! can always be scrolled into view, whatever the size of the terminal.

use std::collections::HashMap;
use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui_image::picker::Picker;

use super::context::{context_hint, draw_pane};
use super::panes::{self, Arrangement, PaneLayout, READING_WIDTH};
use super::parts::{answer_summary, kind_hint, question_key};
use super::source_line::{columns, fitting};
use super::text::field_lines;
use super::viewer::Viewers;
use super::wrap::{cut, indented, wrap};
use crate::format::{Kind, has_text};
use crate::tui::text_field::{FieldShape, TextField, counted_lines, resting_lines};
use crate::tui::{AnswerState, Focus, Page, Row, SessionState, has_context};

/// Descriptions start on the label's line only while that leaves them at
/// least this much room; otherwise they move to their own lines.
const MIN_DESCRIPTION_WIDTH: usize = 30;

/// Where descriptions start that go on the lines below their label.
const DESCRIPTION_INDENT: usize = 6;

/// Lines of a tab and where the row under the cursor sits among them.
struct Content {
    lines: Vec<Line<'static>>,
    /// The first and last line of the current row.
    cursor: (usize, usize),
    /// Where the field or note that has the keys was drawn.
    shape: Option<FieldShape>,
    /// The text cursor of the field or note that has the keys.
    text_cursor: Option<TextCursor>,
}

/// Where the text cursor of a field shows among the lines of its tab, or
/// of its row while the row is built.
#[derive(Debug, Clone, PartialEq)]
struct TextCursor {
    line: usize,
    /// The columns its cell takes.
    columns: Range<usize>,
    /// Whether it is drawn; it hides while the context pane has the keys.
    shown: bool,
    /// Whether it is on the last line of the field's text, the line before
    /// the counter of a field with a length.
    last_line: bool,
}

impl Content {
    /// Adds a row's `lines`, `text_cursor` counted from its first line.
    fn push_row(
        &mut self,
        lines: Vec<Line<'static>>,
        current: bool,
        text_cursor: Option<TextCursor>,
    ) {
        if current {
            self.cursor = (
                self.lines.len(),
                self.lines.len() + lines.len().saturating_sub(1),
            );
        }
        if let Some(cursor) = text_cursor {
            self.text_cursor = Some(TextCursor {
                line: self.lines.len() + cursor.line,
                ..cursor
            });
        }
        self.lines.extend(lines);
    }
}

/// What the current tab keeps from one draw to the next.
pub(super) struct TabState {
    layout: PaneLayout,
    /// The last arrangement with a context area shown, beside or below;
    /// [`panes::choose`] keeps it while a resize stays near a square
    /// shape. A hint leaves it as it is.
    shown: Option<Arrangement>,
    pub(super) question_height: QuestionHeight,
    /// The viewers of the context entries shown so far.
    pub(super) viewers: Viewers,
}

impl TabState {
    /// The state of a tab whose context goes where `layout` says, with
    /// images sent in the graphics protocol `picker` speaks.
    pub(super) fn new(layout: PaneLayout, picker: Picker) -> Self {
        TabState {
            layout,
            shown: None,
            question_height: QuestionHeight::default(),
            viewers: Viewers::new(picker),
        }
    }

    /// Forgets the viewers and rows of sessions that are no longer among
    /// `sessions`, and the rows of replaced files.
    pub(super) fn retain(&mut self, sessions: &[SessionState]) {
        self.viewers
            .retain_sessions(|id| sessions.iter().any(|waiting| waiting.id() == id));
        self.question_height.retain(sessions);
    }
}

/// What a question's rows are kept by: the session, the revision of its
/// file, the question's index and the width it was counted at.
type RowsKey = (String, u64, usize, u16);

/// How many rows the question takes above a context pane below it (spec
/// section 7.1, ADR 30). It is the most any question with context in the
/// session needs, so moving between questions never moves the pane, and it
/// follows what the person types only line by line.
#[derive(Default)]
pub(super) struct QuestionHeight {
    /// The rows each question takes, counted without a cursor. The current
    /// question is counted on every draw, as typing changes it, and the
    /// others once per width and revision.
    rows: HashMap<RowsKey, usize>,
    /// The tab area's size and the session with its revision that `mark`
    /// was taken for.
    measured: Option<(u16, u16, String, u64)>,
    /// The highest need since a field or note last gave up the keys.
    mark: usize,
}

impl QuestionHeight {
    /// The rows the question asks for above a context pane in `area`. It
    /// grows as soon as the tallest question with context grows, and
    /// shrinks only on a draw where no field or note has the keys, so
    /// deleting a line while typing leaves the pane where it is. A resize,
    /// another session or a replaced file start afresh.
    pub(super) fn need(&mut self, state: &SessionState, area: Rect) -> usize {
        let width = area.width.min(READING_WIDTH);
        let measured = (
            area.width,
            area.height,
            state.id().to_owned(),
            state.revision(),
        );
        if self.measured.as_ref() != Some(&measured) {
            let resized = self
                .measured
                .as_ref()
                .is_none_or(|(width, height, ..)| (*width, *height) != (area.width, area.height));
            if resized {
                self.rows.clear();
            }
            self.measured = Some(measured);
            self.mark = 0;
        }

        let typing = state.focus() != Focus::None && !state.pane_focused();
        let mut most = 0;
        for (index, question) in state.session().questions.iter().enumerate() {
            if !has_context(question) {
                continue;
            }
            let key = (state.id().to_owned(), state.revision(), index, width);
            let count = |drawn| {
                question_content(state, index, usize::from(width), drawn)
                    .lines
                    .len()
            };
            let rows = if index == state.tab() {
                let rows = count(Drawn::Counted);
                self.rows.insert(key, rows);
                // While a field or the note has the keys, the question
                // counts as drawn, its text cursor included: the cursor's
                // blank after a full line takes a row then, and counted
                // without it, that row would push the last one out of view.
                if typing {
                    rows.max(count(Drawn::Live))
                } else {
                    rows
                }
            } else {
                *self
                    .rows
                    .entry(key)
                    .or_insert_with(|| count(Drawn::Counted))
            };
            most = most.max(rows);
        }

        self.mark = if typing { self.mark.max(most) } else { most };
        self.mark
    }

    /// How many questions' rows are kept, so tests see what is forgotten.
    #[cfg(test)]
    pub(super) fn counted(&self) -> usize {
        self.rows.len()
    }

    /// Forgets the rows of sessions that are gone and of replaced files.
    fn retain(&mut self, sessions: &[SessionState]) {
        self.rows.retain(|(id, revision, ..), _| {
            sessions
                .iter()
                .any(|state| state.id() == id && state.revision() == *revision)
        });
    }
}

/// Draws the current tab into `area`: the question, or the review, at no
/// more than the reading width, and the context pane where
/// [`panes::choose`] puts it. Says what the pane showed, if one did, and
/// where the field or note that has the keys was drawn, if one was.
pub(super) fn draw_tab(
    frame: &mut Frame,
    state: &SessionState,
    area: Rect,
    tab: &mut TabState,
) -> (Option<Page>, Option<FieldShape>) {
    let arrangement = state.has_context_area().then(|| {
        let arrangement = panes::choose(area.width, area.height, tab.layout, tab.shown);
        if arrangement != Arrangement::Hint {
            tab.shown = Some(arrangement);
        }
        arrangement
    });

    let content_at = |width: u16| match state.question() {
        Some(_) => question_content(state, state.tab(), usize::from(width), Drawn::Live),
        None => review_content(state, usize::from(width)),
    };
    let question_rows = match arrangement {
        Some(Arrangement::Below) => tab.question_height.need(state, area),
        _ => 0,
    };
    let (text_area, context_area) = panes::areas(area, arrangement, question_rows);
    let content = content_at(text_area.width);

    let pane = match context_area {
        Some(area) if arrangement == Some(Arrangement::Hint) => {
            let z = question_key(state, "z");
            frame.render_widget(context_hint(state.shown_context(), &z, area.width), area);
            None
        }
        Some(area) => draw_pane(frame, state, area, &mut tab.viewers),
        None => None,
    };

    // Scroll so the whole current row is visible. A row taller than the
    // area shows its start, unless it holds the text cursor: then the
    // cursor's line stays in view, on the area's last row while the start
    // of the row is further up, so `↑` scrolls back to it row by row. On
    // the text's last line, the row's end, a counter under it included,
    // takes the area's last row instead, so the length shows while the
    // person types at the end; the cursor wins where both do not fit.
    let height = text_area.height as usize;
    let (start, end) = content.cursor;
    let scroll = match &content.text_cursor {
        Some(cursor) if end - start >= height => {
            let kept = if cursor.last_line { end } else { cursor.line };
            kept.saturating_sub(height.saturating_sub(1))
                .max(start)
                .min(cursor.line)
        }
        _ => end.saturating_sub(height.saturating_sub(1)).min(start),
    };
    let total = content.lines.len();
    frame.render_widget(
        Paragraph::new(content.lines).scroll((scroll as u16, 0)),
        text_area,
    );

    // Rows cut off above or below the area are marked on its first and
    // last row, so the person knows to move on to see them.
    let cursor_on = |row: usize| {
        content
            .text_cursor
            .as_ref()
            .filter(|cursor| cursor.shown && cursor.line == scroll + row)
            .map(|cursor| {
                let x = |column: usize| {
                    text_area
                        .x
                        .saturating_add(u16::try_from(column).unwrap_or(u16::MAX))
                };
                x(cursor.columns.start)..x(cursor.columns.end)
            })
    };
    if height > 0 {
        if scroll > 0 {
            mark_cut(frame, text_area, 0, "↑ more", cursor_on(0));
        }
        if scroll + height < total {
            mark_cut(
                frame,
                text_area,
                height - 1,
                "↓ more",
                cursor_on(height - 1),
            );
        }
    }
    (pane, content.shape)
}

/// Draws `marker` dim over row `row` of `area`, which takes no row of its
/// own. It goes to the right edge where that part of the row is blank,
/// else to the left edge where that part is, else to the right edge
/// anyway: the start of a row holds the pointer and the option's number,
/// which matter more than the end of a line of text. A blank keeps it
/// apart from the text beside it.
///
/// The text cursor's cell on the row, the columns `cursor`, is never
/// covered: where the end chosen holds it, the marker goes to the other
/// end, and where both do, the row shows none.
fn mark_cut(frame: &mut Frame, area: Rect, row: usize, marker: &str, cursor: Option<Range<u16>>) {
    let width = u16::try_from(columns(marker) + 1)
        .expect("a short marker")
        .min(area.width);
    let y = area.y + u16::try_from(row).expect("a row inside the area");
    let at = |x: u16| Rect {
        x,
        y,
        width,
        height: 1,
    };
    let (left, right) = (at(area.x), at(area.right() - width));
    // A reversed blank is the text cursor, so it counts as text.
    let blank = |frame: &mut Frame, cells: Rect| {
        cells.positions().all(|position| {
            let cell = &frame.buffer_mut()[position];
            cell.symbol() == " " && !cell.modifier.contains(Modifier::REVERSED)
        })
    };
    let right_end = (right, format!(" {marker}"));
    let left_end = (left, format!("{marker} "));
    let (chosen, other) = if blank(frame, right) || !blank(frame, left) {
        (right_end, left_end)
    } else {
        (left_end, right_end)
    };
    let covers = |cells: Rect| {
        cursor
            .as_ref()
            .is_some_and(|cursor| cursor.start < cells.right() && cells.x < cursor.end)
    };
    let (marked, text) = if !covers(chosen.0) {
        chosen
    } else if !covers(other.0) {
        other
    } else {
        return;
    };
    // The cells lose the styles of the text under them first, so a marker
    // over a highlighted row never shows reversed.
    frame.render_widget(Clear, marked);
    frame.render_widget(Line::from(text).dim(), marked);
}

/// The mark in front of an option or the own answer.
fn mark(kind: Kind, chosen: bool) -> &'static str {
    match (kind, chosen) {
        (Kind::Single, true) => "(•)",
        (Kind::Single, false) => "( )",
        (_, true) => "[x]",
        (_, false) => "[ ]",
    }
}

/// How [`question_content`] draws a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drawn {
    /// As the person sees the current tab: the row under the cursor
    /// highlighted, and the field or note that has the keys with its text
    /// cursor and counter.
    Live,
    /// Without a cursor, to count the rows any question takes (spec section
    /// 7.1): no row is current and every field shows unfocused, with the
    /// text typed so far. A field with a length keeps a row for its
    /// counter, so focusing it adds none, and the cursor's blank after a
    /// full line is left out.
    Counted,
}

/// The lines of question `index`, wrapped to `width`. Counting and drawing
/// share this function, so the rows counted are the rows drawn.
fn question_content(state: &SessionState, index: usize, width: usize, drawn: Drawn) -> Content {
    let question = &state.session().questions[index];
    let live = drawn == Drawn::Live;
    let mut content = Content {
        lines: field_lines(&question.text, width),
        cursor: (0, 0),
        shape: None,
        text_cursor: None,
    };
    content
        .lines
        .extend(wrap(Line::from(kind_hint(question)).dim(), width, 0, 0));
    content.lines.push(Line::default());

    let answer = &state.answers()[index];
    // Descriptions line up in one column after the widest label.
    let label_column = question
        .options
        .iter()
        .flatten()
        .enumerate()
        .map(|(index, option)| option_head_width(index, &option.label))
        .max()
        .unwrap_or(0);
    for (row_index, row) in state.rows_of(index).iter().enumerate() {
        let current = live && row_index == state.row();
        let pointer = if current { "❯ " } else { "  " };
        let (lines, text_cursor) = match *row {
            Row::Option(option_index) => {
                let option = &question
                    .options
                    .as_ref()
                    .expect("option rows come from options")[option_index];
                let chosen = answer.selected.contains(&option.id);
                let head = vec![
                    Span::from(format!(
                        "{pointer}{} {}. ",
                        mark(question.kind, chosen),
                        option_index + 1
                    )),
                    Span::from(option.label.clone()).bold(),
                ];
                let lines = option_lines(
                    head,
                    label_column,
                    option.description.as_deref(),
                    width,
                    current,
                );
                (lines, None)
            }
            Row::Own => {
                let label = question
                    .custom
                    .as_ref()
                    .and_then(|custom| custom.label())
                    .unwrap_or("Own answer")
                    .to_owned();
                // On a single question the own answer is chosen when it
                // counts: it has text and no option is chosen. Blanks and
                // line breaks alone are no text, as in the result.
                let chosen = has_text(answer)
                    && (question.kind == Kind::Multi || answer.selected.is_empty());
                let head = format!("{pointer}{} ✎ {label}: ", mark(question.kind, chosen));
                let text = answer.custom.as_deref().unwrap_or_default();
                if live {
                    live_field(state, head, text, width, current, true, &mut content.shape)
                } else {
                    let length = question.custom.as_ref().and_then(|custom| custom.length());
                    let lines = counted_lines(Span::from(head), text, width, length.is_some());
                    (lines, None)
                }
            }
            Row::Answer => {
                let head = format!("{pointer}✎ ");
                let text = answer.custom.as_deref().unwrap_or_default();
                if live {
                    live_field(state, head, text, width, current, false, &mut content.shape)
                } else {
                    let lines =
                        counted_lines(Span::from(head), text, width, question.length.is_some());
                    (lines, None)
                }
            }
            Row::Question(_) | Row::Submit | Row::Note | Row::Reject => {
                unreachable!("review rows only appear on the review")
            }
        };
        content.push_row(lines, current, text_cursor);
    }

    if question.note {
        content.lines.push(Line::default());
        let editing = live && state.focus() == Focus::Note;
        let head = Span::from("Notes: ").bold();
        let (note_lines, text_cursor) = match state.field().filter(|_| editing) {
            Some(field) => {
                let shape = FieldShape { head, width };
                let shown = !state.pane_focused();
                let lines = field.lines(&shape, shown, None);
                let cursor = field_cursor(field, &shape, shown);
                content.shape = Some(shape);
                (lines, Some(cursor))
            }
            None => {
                let text = answer.note.as_deref().unwrap_or_default();
                (resting_lines(head, text, width), None)
            }
        };
        content.push_row(note_lines, editing, text_cursor);
    }
    content
}

/// The width of an option's pointer, mark, number and label.
fn option_head_width(index: usize, label: &str) -> usize {
    columns(&format!("❯ ( ) {}. ", index + 1)) + columns(label)
}

/// An option: pointer, mark, number and label, then the description from
/// `label_column` on, rendered for the columns from there to the end and
/// so wrapped under its own start. When that leaves too little room, the
/// descriptions go on the lines below instead, from `DESCRIPTION_INDENT`
/// on.
fn option_lines(
    head: Vec<Span<'static>>,
    label_column: usize,
    description: Option<&str>,
    width: usize,
    current: bool,
) -> Vec<Line<'static>> {
    let head_width: usize = head.iter().map(|span| columns(&span.content)).sum();
    let head = if current {
        head.into_iter()
            .map(|span| span.add_modifier(Modifier::REVERSED))
            .collect()
    } else {
        head
    };
    let beside = width.saturating_sub(label_column + 2) >= MIN_DESCRIPTION_WIDTH;
    let indent = if beside {
        label_column + 2
    } else {
        DESCRIPTION_INDENT
    };
    let mut rows = description
        .map(|description| field_lines(description, width.saturating_sub(indent)))
        .unwrap_or_default()
        .into_iter();
    let mut lines = Vec::new();
    match rows.next() {
        // An empty or blank description renders to no lines at all, and
        // the head stands alone.
        None => lines.push(Line::from(head)),
        // The first row starts after the label, padded to the description
        // column.
        Some(first) if beside => {
            let mut spans = head;
            spans.push(Span::from(" ".repeat(indent - head_width)));
            spans.extend(first.spans);
            lines.push(Line::from(spans));
        }
        Some(first) => {
            lines.push(Line::from(head));
            lines.push(indented(first, indent));
        }
    }
    lines.extend(rows.map(|row| indented(row, indent)));
    lines
}

/// The field of a row after `head` as the person sees it: with its text
/// cursor and counter while it has the keys, where it is drawn then going
/// to `shape`, otherwise with `text` as kept, its head highlighted when
/// `highlight` holds and the cursor is on the row. Says where its text
/// cursor shows, if it has the keys.
fn live_field(
    state: &SessionState,
    head: String,
    text: &str,
    width: usize,
    current: bool,
    highlight: bool,
    shape: &mut Option<FieldShape>,
) -> (Vec<Line<'static>>, Option<TextCursor>) {
    match state
        .field()
        .filter(|_| current && state.focus() == Focus::Field)
    {
        // The text cursor hides while the context pane has the focus, so
        // only one place on the screen looks like it takes the keys.
        Some(field) => {
            let drawn = FieldShape {
                head: Span::from(head),
                width,
            };
            let shown = !state.pane_focused();
            let lines = field.lines(&drawn, shown, state.field_limits().as_ref());
            let cursor = field_cursor(field, &drawn, shown);
            *shape = Some(drawn);
            (lines, Some(cursor))
        }
        None if current && highlight => {
            let lines = resting_lines(Span::from(head).reversed(), text, width);
            (lines, None)
        }
        None => (resting_lines(Span::from(head), text, width), None),
    }
}

/// Where `field`'s text cursor shows among its lines drawn as `shape`.
fn field_cursor(field: &TextField, shape: &FieldShape, shown: bool) -> TextCursor {
    let (line, columns, last_line) = field.cursor_cell(shape);
    TextCursor {
        line,
        columns,
        shown,
        last_line,
    }
}

fn review_content(state: &SessionState, width: usize) -> Content {
    let mut content = Content {
        lines: vec![
            Line::from("Review your answers, then submit or reject.").dim(),
            Line::default(),
        ],
        cursor: (0, 0),
        shape: None,
        text_cursor: None,
    };
    let answers = state.answers();
    let states = state.question_states();
    let questions = &state.session().questions;
    let label_width = questions
        .iter()
        .map(|question| columns(question.header.as_deref().unwrap_or(&question.id)))
        .max()
        .unwrap_or(0)
        .min(24);

    for (index, row) in state.rows().iter().enumerate() {
        let current = index == state.row();
        let pointer = if current { "❯ " } else { "  " };
        let (lines, text_cursor) = match *row {
            Row::Question(question_index) => {
                let question = &questions[question_index];
                let skipped = states[question_index].answer == AnswerState::Skipped;
                let label = fitting(
                    question.header.as_deref().unwrap_or(&question.id),
                    label_width,
                );
                // The padding is counted in columns: `format!` would pad a
                // label of wide characters by its characters.
                let padding = " ".repeat(label_width - columns(label));
                let lead = format!(
                    "{pointer}{} {label}{padding}  ",
                    if skipped { "☐" } else { "☒" }
                );
                // One line per question: the summary gets what is left of
                // the width and ends in `…` when it is longer.
                let room = width.saturating_sub(columns(&lead));
                let summary = cut(&answer_summary(question, &answers[question_index]), room);
                let spans = vec![Span::from(lead), Span::from(summary).dim()];
                let line = Line::from(spans);
                (vec![if current { line.reversed() } else { line }], None)
            }
            Row::Submit => {
                let counts = state.counts();
                let button = Line::from(format!("{pointer}[ Submit ]")).bold();
                let lines = vec![
                    Line::default(),
                    Line::from(format!(
                        "answered {} · skipped {} · defaulted {}",
                        counts.answered, counts.skipped, counts.defaulted
                    )),
                    Line::default(),
                    if current { button.reversed() } else { button },
                ];
                (lines, None)
            }
            Row::Note => live_field(
                state,
                format!("{pointer}✎ Note to the asker (optional): "),
                state.session_note(),
                width,
                current,
                true,
                &mut content.shape,
            ),
            Row::Reject => live_field(
                state,
                format!("{pointer}✗ Reject, reason (optional): "),
                state.reject_reason(),
                width,
                current,
                true,
                &mut content.shape,
            ),
            Row::Option(_) | Row::Own | Row::Answer => {
                unreachable!("question rows never appear on the review")
            }
        };
        content.push_row(lines, current, text_cursor);
    }
    content
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::super::test_support;
    use super::*;

    #[test]
    fn a_marker_that_would_cover_the_cursor_at_both_ends_is_left_out() {
        // In 8 columns both ends of the row hold the cell at column 3.
        let mut terminal = Terminal::new(TestBackend::new(8, 1)).expect("test terminal");
        terminal
            .draw(|frame| mark_cut(frame, frame.area(), 0, "↓ more", Some(3..4)))
            .expect("draws");
        assert_eq!(
            test_support::rows(terminal.backend().buffer())[0],
            " ".repeat(8)
        );

        terminal
            .draw(|frame| mark_cut(frame, frame.area(), 0, "↓ more", Some(7..8)))
            .expect("draws");
        assert_eq!(
            test_support::rows(terminal.backend().buffer())[0],
            "↓ more  ",
            "the other end"
        );
    }
}
