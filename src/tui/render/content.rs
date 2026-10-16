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

use patine::QuoteStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Widget};

use super::context::{DrawnPane, context_hint, draw_pane, pane_body};
use super::panes::{self, Arrangement, PaneLayout, READING_WIDTH};
use super::parts::{POINTER, answer_mark, answer_summary, kind_hint, pointer, question_key};
use super::source_line::{columns, fitting};
use super::text::field_lines;
use super::theme;
use super::viewer::Viewers;
use super::wrap::{cut, indented, wrap};
use crate::format::{Kind, Length, has_text};
use crate::graphics::Images;
use crate::tui::text_field::{FieldShape, TextField, counted_lines, resting_lines};
use crate::tui::{Focus, Row, SessionState, has_context};

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
    /// images that `images` makes.
    pub(super) fn new(layout: PaneLayout, images: Images) -> Self {
        TabState {
            layout,
            shown: None,
            question_height: QuestionHeight::default(),
            viewers: Viewers::new(images),
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
    /// The rows each question takes, counted without a cursor and with
    /// the own answer's head filled (`Pass::Counted`). The current
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
    ///
    /// The questions are counted with their quotes in `quote`. A quote's
    /// bar takes the same columns in every style, so the rows kept from
    /// another frame stay right.
    pub(super) fn need(&mut self, state: &SessionState, area: Rect, quote: QuoteStyle) -> usize {
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
            let count = |pass| {
                question_content(state, index, usize::from(width), pass, quote)
                    .lines
                    .len()
            };
            let rows = if index == state.tab() {
                let rows = count(Pass::Counted);
                self.rows.insert(key, rows);
                // While a field or the note has the keys, the question
                // counts as drawn, its text cursor included: the cursor's
                // blank after a full line takes a row then, and counted
                // without it, that row would push the last one out of view.
                if typing {
                    rows.max(count(Pass::Live))
                } else {
                    rows
                }
            } else {
                *self.rows.entry(key).or_insert_with(|| count(Pass::Counted))
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
/// [`panes::choose`] puts it, with Markdown quotes in `quote`. Says what
/// the pane showed, if one did, and where the field or note that has the
/// keys was drawn, if one was.
pub(super) fn draw_tab(
    area: Rect,
    buf: &mut Buffer,
    state: &SessionState,
    tab: &mut TabState,
    quote: QuoteStyle,
) -> (Option<DrawnPane>, Option<FieldShape>) {
    let arrangement = state.has_context_area().then(|| {
        let arrangement = panes::choose(area.width, area.height, tab.layout, tab.shown);
        if arrangement != Arrangement::Hint {
            tab.shown = Some(arrangement);
        }
        arrangement
    });

    let content_at = |width: u16| match state.question() {
        Some(_) => question_content(state, state.tab(), usize::from(width), Pass::Live, quote),
        None => review_content(state, usize::from(width)),
    };
    let question_rows = match arrangement {
        Some(Arrangement::Below) => tab.question_height.need(state, area, quote),
        _ => 0,
    };
    let (text_area, context_area) = panes::areas(area, arrangement, question_rows);
    let content = content_at(text_area.width);

    let pane = match context_area {
        Some(area) if arrangement == Some(Arrangement::Hint) => {
            let z = question_key(state, "z");
            context_hint(state.shown_context(), &z, area.width).render(area, buf);
            None
        }
        Some(area) => draw_pane(area, buf, state, &mut tab.viewers, quote),
        None => None,
    };
    // The viewers keep the window of the question shown, and load what it
    // lacks ahead for the body of a pane that shows, whatever its row
    // shows (ADR 46).
    if state.question().is_some() {
        let body = context_area
            .filter(|_| arrangement != Some(Arrangement::Hint))
            .map(|area| {
                let body = pane_body(area);
                (body.width, body.height)
            });
        tab.viewers
            .load_window(state.id(), &state.window(), body, quote);
    }

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
    Paragraph::new(content.lines)
        .scroll((scroll as u16, 0))
        .render(text_area, buf);

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
            mark_cut(text_area, buf, 0, "↑ more", cursor_on(0));
        }
        if scroll + height < total {
            mark_cut(text_area, buf, height - 1, "↓ more", cursor_on(height - 1));
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
fn mark_cut(area: Rect, buf: &mut Buffer, row: usize, marker: &str, cursor: Option<Range<u16>>) {
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
    let covers = |cells: Rect| {
        cursor
            .as_ref()
            .is_some_and(|cursor| cursor.start < cells.right() && cells.x < cursor.end)
    };
    // The text cursor's cell counts as text even where it is a blank. The
    // blanks of a fill do not: a fill is no text, and the marker may cover
    // it as it covers the fill's text.
    let blank = |buf: &Buffer, cells: Rect| {
        !covers(cells)
            && cells
                .positions()
                .all(|position| buf[position].symbol() == " ")
    };
    let right_end = (right, format!(" {marker}"));
    let left_end = (left, format!("{marker} "));
    let (chosen, other) = if blank(buf, right) || !blank(buf, left) {
        (right_end, left_end)
    } else {
        (left_end, right_end)
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
    Clear.render(marked, buf);
    Line::styled(text, theme::secondary()).render(marked, buf);
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
enum Pass {
    /// As the person sees the current tab: the row under the cursor
    /// highlighted, and the field or note that has the keys with its text
    /// cursor and counter.
    Live,
    /// Without a cursor, to count the rows any question takes (spec section
    /// 7.1): no row is current and every field shows unfocused, with the
    /// text typed so far. A field with a length keeps a row for its
    /// counter, so focusing it adds none, and the cursor's blank after a
    /// full line is left out.
    ///
    /// The own answer's head, the one field head of a question that takes
    /// the focus fill, is laid out filled, whichever row is current.
    /// A filled head's blanks show and wrap like text, so a head ending at
    /// the edge starts the next row with its blank and moves the text
    /// right. A shown blank only ever moves text right, so the filled
    /// layout is the most rows the head can take, and the count stays the
    /// same as the cursor moves onto the own answer and off it.
    Counted,
}

/// The lines of question `index`, wrapped to `width`, with Markdown quotes
/// in `quote`. Counting and drawing share this function, so the rows
/// counted are the rows drawn.
fn question_content(
    state: &SessionState,
    index: usize,
    width: usize,
    pass: Pass,
    quote: QuoteStyle,
) -> Content {
    let question = &state.session().questions[index];
    let live = pass == Pass::Live;
    let mut content = Content {
        lines: field_lines(&question.text, width, quote),
        cursor: (0, 0),
        shape: None,
        text_cursor: None,
    };
    content.lines.extend(wrap(
        Line::styled(kind_hint(question), theme::secondary()),
        width,
        0,
        0,
    ));
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
        // One focus fill shows at a time: while the context pane has the
        // focus, its header has the fill, and the current row keeps only
        // its pointer.
        let filled = current && !state.pane_focused();
        let editing = current && state.focus() == Focus::Field;
        let (lines, text_cursor) = match *row {
            Row::Option(option_index) => {
                let option = &question
                    .options
                    .as_ref()
                    .expect("option rows come from options")[option_index];
                let chosen = answer.selected.contains(&option.id);
                let mark = Span::from(mark(question.kind, chosen));
                let head = vec![
                    pointer(current),
                    if chosen {
                        mark.patch_style(theme::chosen())
                    } else {
                        mark
                    },
                    Span::styled(format!(" {}. ", option_index + 1), theme::secondary()),
                    Span::styled(option.label.clone(), theme::strong()),
                ];
                let lines = option_lines(
                    head,
                    label_column,
                    option.description.as_deref(),
                    width,
                    filled,
                    quote,
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
                let head = Span::from(format!(
                    "{}{} ✎ {label}: ",
                    pointer(current),
                    mark(question.kind, chosen)
                ));
                let text = answer.custom.as_deref().unwrap_or_default();
                if live {
                    let row = FieldRow {
                        head,
                        text,
                        editing,
                        fill: filled,
                        limits: state.field_limits(),
                    };
                    live_field(state, row, width, &mut content.shape)
                } else {
                    let length = question.custom.as_ref().and_then(|custom| custom.length());
                    let lines = counted_lines(head, true, text, width, length.is_some());
                    (lines, None)
                }
            }
            Row::Answer => {
                let head = Span::from(format!("{}✎ ", pointer(current)));
                let text = answer.custom.as_deref().unwrap_or_default();
                if live {
                    let row = FieldRow {
                        head,
                        text,
                        editing,
                        fill: false,
                        limits: state.field_limits(),
                    };
                    live_field(state, row, width, &mut content.shape)
                } else {
                    let lines = counted_lines(head, false, text, width, question.length.is_some());
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
        // A note has no length to count against, and its head is never
        // filled.
        let row = FieldRow {
            head: Span::styled("Notes: ", theme::strong()),
            text: answer.note.as_deref().unwrap_or_default(),
            editing,
            fill: false,
            limits: None,
        };
        let (note_lines, text_cursor) = live_field(state, row, width, &mut content.shape);
        content.push_row(note_lines, editing, text_cursor);
    }
    content
}

/// The width of an option's pointer, mark, number and label.
fn option_head_width(index: usize, label: &str) -> usize {
    columns(POINTER) + columns(&format!("( ) {}. ", index + 1)) + columns(label)
}

/// An option: pointer, mark, number and label, under the focus fill where
/// `filled`, then the description from `label_column` on, rendered for the
/// columns from there to the end and so wrapped under its own start. When
/// that leaves too little room, the descriptions go on the lines below
/// instead, from `DESCRIPTION_INDENT` on. The description's quotes show in
/// `quote`.
fn option_lines(
    head: Vec<Span<'static>>,
    label_column: usize,
    description: Option<&str>,
    width: usize,
    filled: bool,
    quote: QuoteStyle,
) -> Vec<Line<'static>> {
    let head_width: usize = head.iter().map(|span| columns(&span.content)).sum();
    let head = if filled {
        head.into_iter().map(theme::focus_span).collect()
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
        .map(|description| field_lines(description, width.saturating_sub(indent), quote))
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

/// A row that is a field: an own answer, a text answer, a note, the
/// session note or the reject reason.
struct FieldRow<'a> {
    /// What comes before the text: the pointer, marks and a label.
    head: Span<'static>,
    /// The text as kept, shown while the field does not have the keys.
    text: &'a str,
    /// Whether the field has the keys, so the session's field is drawn
    /// here with its text cursor.
    editing: bool,
    /// Whether the head is under the focus fill while the field does not
    /// have the keys.
    fill: bool,
    /// What the counter under the field counts against while it has the
    /// keys; without limits there is no counter.
    limits: Option<Length>,
}

/// The lines of field `row`, wrapped to `width`, and where its text cursor
/// shows if it has the keys. Where a field that has the keys is drawn goes
/// to `shape`.
fn live_field(
    state: &SessionState,
    row: FieldRow,
    width: usize,
    shape: &mut Option<FieldShape>,
) -> (Vec<Line<'static>>, Option<TextCursor>) {
    match state.field().filter(|_| row.editing) {
        // The text cursor hides while the context pane has the focus, so
        // only one place on the screen looks like it takes the keys.
        Some(field) => {
            let drawn = FieldShape {
                head: row.head,
                width,
            };
            let shown = !state.pane_focused();
            let lines = field.lines(&drawn, shown, row.limits.as_ref());
            let cursor = field_cursor(field, &drawn, shown);
            *shape = Some(drawn);
            (lines, Some(cursor))
        }
        None if row.fill => (
            resting_lines(theme::focus_span(row.head), true, row.text, width),
            None,
        ),
        None => (resting_lines(row.head, false, row.text, width), None),
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
            Line::styled(
                "Review your answers, then submit or reject.",
                theme::secondary(),
            ),
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
        let filled = current && !state.pane_focused();
        // The session note and the reject reason are fields.
        let field_row = |head: String, text| FieldRow {
            head: Span::from(head),
            text,
            editing: current && state.focus() == Focus::Field,
            fill: filled,
            limits: state.field_limits(),
        };
        let (lines, text_cursor) = match *row {
            Row::Question(question_index) => {
                let question = &questions[question_index];
                let label = fitting(
                    question.header.as_deref().unwrap_or(&question.id),
                    label_width,
                );
                // The padding is counted in columns: `format!` would pad a
                // label of wide characters by its characters.
                let padding = " ".repeat(label_width - columns(label));
                let lead = vec![
                    pointer(current),
                    answer_mark(states[question_index].answer),
                    Span::from(format!(" {label}{padding}  ")),
                ];
                // One line per question: the summary gets what is left of
                // the width and ends in `…` when it is longer.
                let lead_width: usize = lead.iter().map(|span| columns(&span.content)).sum();
                let room = width.saturating_sub(lead_width);
                let summary = cut(&answer_summary(question, &answers[question_index]), room);
                let mut spans = lead;
                spans.push(Span::styled(summary, theme::secondary()));
                let line = Line::from(spans);
                let line = if filled { theme::focus_row(line) } else { line };
                (vec![line], None)
            }
            Row::Submit => {
                let counts = state.counts();
                let words = |text: &str| Span::styled(text.to_owned(), theme::secondary());
                let number = |count: usize| Span::styled(count.to_string(), theme::strong());
                let tally = Line::from(vec![
                    words("answered "),
                    number(counts.answered),
                    words(" · skipped "),
                    number(counts.skipped),
                    words(" · defaulted "),
                    number(counts.defaulted),
                ]);
                // The button is always filled, so the cursor on it shows
                // in its pointer alone, in the focus colour.
                let pointer = if current {
                    pointer(true).patch_style(theme::focus_pointer())
                } else {
                    pointer(false)
                };
                let button = Line::from(vec![pointer, Span::styled("[ Submit ]", theme::button())]);
                let lines = vec![Line::default(), tally, Line::default(), button];
                (lines, None)
            }
            Row::Note => {
                let head = format!("{}✎ Note to the asker (optional): ", pointer(current));
                let row = field_row(head, state.session_note());
                live_field(state, row, width, &mut content.shape)
            }
            Row::Reject => {
                let head = format!("{}✗ Reject, reason (optional): ", pointer(current));
                let row = field_row(head, state.reject_reason());
                live_field(state, row, width, &mut content.shape)
            }
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
    use std::collections::BTreeSet;

    use ratatui::style::{Color, Modifier, Stylize};

    use super::super::patine_adapter::quote_style;
    use super::super::test_support;
    use super::*;
    use crate::tui::Page;

    /// A one-row buffer as wide as `row`, holding it, with `↓ more` put
    /// over it by [`mark_cut`], the text cursor at `cursor`.
    fn marked(row: Line<'static>, cursor: Option<Range<u16>>) -> Buffer {
        let area = Rect::new(0, 0, u16::try_from(row.width()).expect("a short row"), 1);
        let mut buf = Buffer::empty(area);
        row.render(area, &mut buf);
        mark_cut(area, &mut buf, 0, "↓ more", cursor);
        buf
    }

    #[test]
    fn a_marker_takes_the_blank_end_of_its_row() {
        let shown = |row: &str| test_support::rows(&marked(Line::from(row.to_owned()), None));

        assert_eq!(shown(&" ".repeat(12)), ["      ↓ more"], "a blank row");
        assert_eq!(shown("text        "), ["text  ↓ more"], "text at the start");
        assert_eq!(shown("        text"), ["↓ more  text"], "text at the end");
        assert_eq!(shown("text at both"), ["text  ↓ more"], "text at both ends");
    }

    #[test]
    fn a_reversed_blank_counts_as_text_and_the_marker_drops_the_styles_under_it() {
        // The text cursor, a reversed blank, at the end of the row.
        let row = Line::from(vec![Span::from(" ".repeat(11)), Span::from(" ").reversed()]);
        let buf = marked(row, Some(11..12));
        assert_eq!(test_support::rows(&buf), ["↓ more      "]);

        let buf = marked(Line::from("highlighted ").reversed(), None);
        assert_eq!(test_support::rows(&buf), ["highl ↓ more"]);
        assert_eq!(
            buf[test_support::find(&buf, "↓")].modifier,
            Modifier::DIM,
            "dim, not reversed"
        );
    }

    #[test]
    fn the_text_cursor_counts_as_text_by_its_columns_not_by_its_style() {
        let blank = || Line::from(" ".repeat(12));
        assert_eq!(
            test_support::rows(&marked(blank(), Some(11..12))),
            ["↓ more      "],
            "the cursor's cell, though not reversed, keeps the marker away"
        );

        let row = Line::from(vec![Span::from(" ".repeat(11)), Span::from(" ").reversed()]);
        assert_eq!(
            test_support::rows(&marked(row, None)),
            ["      ↓ more"],
            "a reversed blank that is not the cursor is a blank"
        );
    }

    #[test]
    fn a_marker_may_cover_the_blanks_of_a_fill() {
        // A filled head wrapped onto a row of its own, ending in blanks of
        // its fill: they are no text, so the marker takes that end.
        let row = Line::from(vec![
            Span::from(" ".repeat(7)),
            Span::from(format!("ab{}", " ".repeat(7))).reversed(),
        ]);

        assert_eq!(test_support::rows(&marked(row, None)), ["       ab ↓ more"]);
    }

    /// A single with an own answer and a note, then a text question with a
    /// length; the review follows them.
    const SESSION: &str = r#"{"asqr": 2, "questions": [
        {"id": "pick", "header": "Pick", "text": "?", "kind": "single", "custom": true,
         "options": [{"id": "a", "label": "A"}]},
        {"id": "words", "text": "?", "kind": "text", "length": {"warn": 3}}]}"#;

    /// The session after pressing `keys`, `E` standing for `esc` and `\n`
    /// for `enter`.
    fn after(keys: &str) -> SessionState {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut state = SessionState::new(
            "parts",
            serde_json::from_str(SESSION).expect("test session parses"),
            None,
        );
        for key in keys.chars() {
            let code = match key {
                'E' => KeyCode::Esc,
                '\n' => KeyCode::Enter,
                key => KeyCode::Char(key),
            };
            state.handle(KeyEvent::new(code, KeyModifiers::NONE));
        }
        state
    }

    /// The current tab of `state` drawn 40 columns wide.
    fn tab_of(state: &SessionState) -> (Content, Buffer) {
        let content = match state.question() {
            Some(_) => question_content(state, state.tab(), 40, Pass::Live, quote_style(false)),
            None => review_content(state, 40),
        };
        let height = u16::try_from(content.lines.len()).expect("a short tab");
        let area = Rect::new(0, 0, 40, height);
        let mut buf = Buffer::empty(area);
        Paragraph::new(content.lines.clone()).render(area, &mut buf);
        (content, buf)
    }

    fn modifier_at(buf: &Buffer, text: &str) -> Modifier {
        test_support::style_at(buf, text).add_modifier
    }

    #[test]
    fn a_note_shows_its_head_bold_and_never_reversed() {
        let (content, buf) = tab_of(&after("n"));
        let shape = content.shape.expect("the note has the keys");
        assert_eq!(shape.head, Span::from("Notes: ").bold());
        assert_eq!(shape.width, 40);
        let notes = test_support::find(&buf, "Notes: ").1;
        assert_eq!(content.cursor, (usize::from(notes), usize::from(notes)));
        assert_eq!(
            content
                .text_cursor
                .map(|cursor| (cursor.line, cursor.shown)),
            Some((usize::from(notes), true))
        );
        assert_eq!(modifier_at(&buf, "Notes: "), Modifier::BOLD);

        let (content, buf) = tab_of(&after("nabE"));
        assert_eq!(content.shape, None);
        assert_eq!(content.text_cursor, None);
        assert_eq!(
            test_support::row(&buf, test_support::find(&buf, "Notes: ").1),
            "Notes: ab"
        );
        assert_eq!(modifier_at(&buf, "Notes: "), Modifier::BOLD);
    }

    #[test]
    fn only_a_field_counts_its_length_while_it_has_the_keys() {
        // The text question takes the keys on arrival.
        let (content, buf) = tab_of(&after("labcd"));
        let shape = content.shape.expect("the field has the keys");
        assert_eq!(shape.head, Span::from("❯ ✎ "));
        assert_eq!(test_support::row(&buf, 3), "❯ ✎ abcd");
        assert!(
            test_support::row(&buf, 4).contains("4/3"),
            "{:?}",
            test_support::rows(&buf)
        );

        let (_, buf) = tab_of(&after("labcdEn"));
        assert!(
            !test_support::rows(&buf)
                .iter()
                .any(|row| row.contains("/3")),
            "the note shows no counter: {:?}",
            test_support::rows(&buf)
        );
    }

    /// The colour and modifiers of a cell under the focus fill.
    const FILLED: (Color, Modifier) = (Color::Yellow, Modifier::REVERSED);

    #[test]
    fn a_resting_field_head_under_the_cursor_is_filled_where_it_highlights() {
        let (_, buf) = tab_of(&after("jE"));
        let (start, y) = test_support::find(&buf, "❯ ( ) ✎ Own answer: ");
        for x in start..start + 20 {
            let cell = &buf[(x, y)];
            assert_eq!(
                (cell.fg, cell.modifier),
                FILLED,
                "the own answer, column {x}"
            );
        }
        let (_, buf) = tab_of(&after("lE"));
        assert_eq!(
            modifier_at(&buf, "❯ ✎ "),
            Modifier::empty(),
            "a text answer"
        );
        let (_, buf) = tab_of(&after("lEljE"));
        assert_eq!(
            test_support::look_at(&buf, "❯ ✎ Note to the asker"),
            FILLED,
            "the session note"
        );
    }

    /// A multi question of two options, without notes, after `keys`.
    fn multi_after(keys: &str) -> (Content, Buffer) {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let json = r#"{"asqr": 2, "questions": [{"id": "m", "text": "?", "kind": "multi",
            "note": false, "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}]}"#;
        let mut state =
            SessionState::new("marks", serde_json::from_str(json).expect("parses"), None);
        for key in keys.chars() {
            state.handle(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
        }
        tab_of(&state)
    }

    #[test]
    fn a_chosen_mark_is_bold_primary_and_an_option_number_dim() {
        // Ticked, then the cursor moves on to the second option.
        let (_, buf) = multi_after(" j");
        assert_eq!(test_support::row(&buf, 3), "  [x] 1. A");
        assert_eq!(
            test_support::look_at(&buf, "[x]"),
            (Color::Cyan, Modifier::BOLD)
        );
        assert_eq!(
            test_support::look_at(&buf, "x] 1"),
            (Color::Cyan, Modifier::BOLD),
            "the whole mark"
        );
        assert_eq!(
            test_support::look_at(&buf, " 1. A"),
            (Color::Reset, Modifier::DIM)
        );
        assert_eq!(
            test_support::look_at(&buf, "A"),
            (Color::Reset, Modifier::BOLD)
        );

        let (_, buf) = multi_after(" ");
        assert_eq!(test_support::row(&buf, 4), "  [ ] 2. B");
        assert_eq!(
            test_support::look_at(&buf, "[ ] 2"),
            (Color::Reset, Modifier::empty()),
            "a mark not chosen"
        );
    }

    #[test]
    fn a_filled_option_is_one_colour_with_its_chosen_mark_and_number() {
        let (_, buf) = multi_after(" ");
        assert_eq!(test_support::row(&buf, 3), "❯ [x] 1. A");
        for x in 0..10 {
            let cell = &buf[(x, 3)];
            assert_eq!(
                (cell.fg, cell.modifier & !Modifier::BOLD),
                FILLED,
                "column {x}"
            );
        }
    }

    #[test]
    fn the_review_marks_answered_questions_and_fills_the_row_under_the_cursor() {
        let (_, buf) = tab_of(&after("lElkk"));
        let pick = test_support::find(&buf, "❯ ☐ Pick").1;
        assert_eq!(test_support::row(&buf, pick), "❯ ☐ Pick   -");
        assert_eq!(test_support::row(&buf, pick + 1), "  ☐ words  -");
        for x in 0..12 {
            let cell = &buf[(x, pick)];
            assert_eq!((cell.fg, cell.modifier), FILLED, "column {x}");
        }
        assert_eq!(modifier_at(&buf, "  ☐ words"), Modifier::empty());
        assert_eq!(
            buf[(2, pick + 1)].modifier,
            Modifier::DIM,
            "an unanswered mark"
        );

        // Enter picks the option and goes on to the text question.
        let (_, buf) = tab_of(&after("\nElkk"));
        assert_eq!(test_support::row(&buf, pick), "❯ ☒ Pick   A");
    }

    #[test]
    fn the_submit_button_is_always_filled_in_the_primary_colour() {
        let button = (Color::Cyan, Modifier::REVERSED | Modifier::BOLD);
        // The review opens on the submit button.
        let (_, buf) = tab_of(&after("lEl"));
        let (x, y) = test_support::find(&buf, "❯ [ Submit ]");
        assert_eq!(
            test_support::look_at(&buf, "❯ [ Submit ]"),
            (Color::Yellow, Modifier::BOLD),
            "the pointer"
        );
        for x in x + 2..x + 12 {
            let cell = &buf[(x, y)];
            assert_eq!((cell.fg, cell.modifier), button, "column {x}");
        }
        assert_eq!(buf[(x + 12, y)].modifier, Modifier::empty());

        let (_, buf) = tab_of(&after("lElk"));
        assert_eq!(test_support::look_at(&buf, "[ Submit ]"), button);
        let (x, y) = test_support::find(&buf, "[ Submit ]");
        assert_eq!(buf[(x - 2, y)].symbol(), " ", "no pointer");
    }

    #[test]
    fn the_review_shows_one_focus_fill_besides_the_button() {
        let (_, buf) = tab_of(&after("lElkk"));
        let reversed: Vec<(u16, u16, Color)> = buf
            .area
            .positions()
            .filter(|&position| buf[position].modifier.contains(Modifier::REVERSED))
            .map(|position| (position.x, position.y, buf[position].fg))
            .collect();
        let rows: BTreeSet<u16> = reversed.iter().map(|&(_, y, _)| y).collect();
        let (pick, submit) = (
            test_support::find(&buf, "❯ ☐ Pick").1,
            test_support::find(&buf, "[ Submit ]").1,
        );
        assert_eq!(rows, BTreeSet::from([pick, submit]));
        for (x, y, colour) in reversed {
            let expected = if y == pick {
                Color::Yellow
            } else {
                Color::Cyan
            };
            assert_eq!(colour, expected, "column {x} of row {y}");
        }
        let button: String = (0..40)
            .filter(|&x| buf[(x, submit)].modifier.contains(Modifier::REVERSED))
            .map(|x| buf[(x, submit)].symbol())
            .collect();
        assert_eq!(
            button, "[ Submit ]",
            "the brackets tell it from a focus fill"
        );
    }

    #[test]
    fn the_review_counts_its_numbers_bold_and_its_words_dim() {
        let (_, buf) = tab_of(&after("lEl"));
        let (x, y) = test_support::find(&buf, "answered 0 · skipped 2 · defaulted 0");
        let words = (Color::Reset, Modifier::DIM);
        let number = (Color::Reset, Modifier::BOLD);
        let looks: Vec<(Color, Modifier)> = (x..x + 36)
            .map(|x| (buf[(x, y)].fg, buf[(x, y)].modifier))
            .collect();
        for (offset, look) in looks.iter().enumerate() {
            let expected = if [9, 21, 35].contains(&offset) {
                number
            } else {
                words
            };
            assert_eq!(*look, expected, "column {}", x + offset as u16);
        }
    }

    /// The single question of [`SESSION`] with a context entry, so it has
    /// a pane, after `keys`, then with the pane focused.
    fn with_pane_focused(keys: &str) -> SessionState {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        use crate::tui::Extent;

        let session = SESSION.replace(
            r#""custom": true,"#,
            r#""custom": true, "context": [{"type": "code", "content": "x"}],"#,
        );
        let mut state = SessionState::new(
            "pane",
            serde_json::from_str(&session).expect("parses"),
            None,
        );
        state.set_pane_page(Some(crate::tui::PanePage::shown(Page {
            rows: Extent { shown: 5, total: 5 },
            columns: Extent::default(),
        })));
        for key in keys.chars() {
            let code = if key == 'E' {
                KeyCode::Esc
            } else {
                KeyCode::Char(key)
            };
            state.handle(KeyEvent::new(code, KeyModifiers::NONE));
        }
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(state.pane_focused());
        state
    }

    #[test]
    fn while_the_pane_has_the_focus_the_current_row_keeps_its_pointer_and_loses_its_fill() {
        for (keys, row) in [("", "❯ ( ) 1. A"), ("jE", "❯ ( ) ✎ Own answer: —")] {
            let (_, buf) = tab_of(&with_pane_focused(keys));
            test_support::find(&buf, row);
            assert!(
                buf.content()
                    .iter()
                    .all(|cell| !cell.modifier.contains(Modifier::REVERSED)),
                "{row}: {:?}",
                test_support::rows(&buf)
            );
        }
    }

    #[test]
    fn an_unfilled_head_at_the_row_width_takes_the_rows_it_was_counted_with() {
        // The own answer's head takes 20 columns, its closing blank the
        // 20th. While the pane has the focus the head is not filled, so its
        // blank hangs past the edge as in the count.
        let state = with_pane_focused("jE");
        let lines = |pass| {
            test_support::line_texts(
                &question_content(&state, state.tab(), 19, pass, quote_style(false)).lines,
            )
        };

        let drawn = lines(Pass::Live);
        assert_eq!(drawn.len(), lines(Pass::Counted).len());
        let head = drawn
            .iter()
            .position(|line| line.contains("Own answer"))
            .expect("the own answer shows");
        assert_eq!(drawn[head..head + 2], ["❯ ( ) ✎ Own answer: ", "    —"]);
    }

    #[test]
    fn a_filled_head_ending_at_the_edge_starts_the_next_row_with_its_blank() {
        // The own answer's head takes 20 columns, its closing blank the
        // 20th, so in 19 that blank is one past the end of the first row.
        let own_rows = |state: &SessionState| {
            let lines = test_support::line_texts(
                &question_content(state, state.tab(), 19, Pass::Live, quote_style(false)).lines,
            );
            let head = lines
                .iter()
                .position(|line| line.contains("Own answer"))
                .expect("the own answer shows");
            lines[head..head + 2].to_vec()
        };

        assert_eq!(
            own_rows(&after("jE")),
            ["❯ ( ) ✎ Own answer:", "     —"],
            "the fill's blank shows, so it moves on"
        );
        assert_eq!(
            own_rows(&after("")),
            ["  ( ) ✎ Own answer: ", "    —"],
            "a plain blank hangs past the edge"
        );
    }

    #[test]
    fn a_filled_head_ending_at_the_edge_leaves_room_for_its_last_line_above_the_pane() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // The own answer's head ends at the edge of 19 columns, and its
        // text fills the row under it unless the fill's blank goes first.
        let session = SESSION.replace(
            r#""custom": true,"#,
            r#""custom": true, "context": [{"type": "code", "content": "x"}],"#,
        );
        let mut state = SessionState::new(
            "edge",
            serde_json::from_str(&session).expect("parses"),
            None,
        );
        for key in "jabcdefghijklmno".chars() {
            state.handle(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
        }
        state.handle(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let drawn = test_support::line_texts(
            &question_content(&state, state.tab(), 19, Pass::Live, quote_style(false)).lines,
        );
        assert!(
            drawn.ends_with(&[
                "❯ (•) ✎ Own answer:".to_owned(),
                "     ".to_owned(),
                "    abcdefghijklmno".to_owned(),
                String::new(),
                "Notes: —".to_owned(),
            ]),
            "{drawn:?}"
        );

        let area = Rect::new(0, 0, 19, 30);
        let mut buf = Buffer::empty(area);
        let mut tab = TabState::new(PaneLayout::Auto, Images::half_blocks());
        draw_tab(area, &mut buf, &state, &mut tab, quote_style(false));

        let shown = test_support::rows(&buf);
        assert_eq!(
            shown[..drawn.len()]
                .iter()
                .map(|row| row.trim_end())
                .collect::<Vec<_>>(),
            drawn.iter().map(|line| line.trim_end()).collect::<Vec<_>>(),
            "the whole question shows above the pane: {shown:#?}"
        );
    }

    /// A row without context shows an empty pane, and the other options'
    /// entries still load ahead, for the pane's body.
    #[test]
    fn a_row_without_context_still_loads_the_others_ahead() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "options": [
              {"id": "a", "label": "A"},
              {"id": "b", "label": "B", "context": [{"type": "code", "content": "b"}]}]}]}"#;
        let state = SessionState::new("batch", serde_json::from_str(json).expect("parses"), None);
        let mut tab = TabState::new(PaneLayout::Beside, Images::half_blocks());
        tab.viewers.loading_ahead();
        let area = Rect::new(0, 0, 140, 30);
        let mut buf = Buffer::empty(area);

        draw_tab(area, &mut buf, &state, &mut tab, quote_style(false));

        assert_eq!(
            tab.viewers.pending(),
            [crate::tui::EntryKey {
                question: 0,
                option: Some(1)
            }],
            "b loads ahead"
        );
    }

    #[test]
    fn a_marker_that_would_cover_the_cursor_at_both_ends_is_left_out() {
        // In 8 columns both ends of the row hold the cell at column 3.
        let blank = || Line::from(" ".repeat(8));
        assert_eq!(
            test_support::rows(&marked(blank(), Some(3..4))),
            [" ".repeat(8)]
        );
        assert_eq!(
            test_support::rows(&marked(blank(), Some(7..8))),
            ["↓ more  "],
            "the other end"
        );
    }
}
