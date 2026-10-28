// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The current tab (spec sections 7.2, 7.7 and 7.8): a question with its
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

use super::context::{DrawnPane, Ledge, context_hint, draw_pane, pane_body};
use super::panes::{self, Arrangement, PaneLayout, READING_WIDTH};
use super::parts::{POINTER, answer_mark, answer_summary, kind_hint, pointer, question_key};
use super::patine_adapter::WIDEST_GRAPHEME;
use super::source_line::{columns, fitting};
use super::text::field_lines;
use super::theme;
use super::viewer::Viewers;
use super::wrap::{cut_spans, indented, trim_end, wrap};
use crate::format::{Kind, Length, has_text};
use crate::graphics::Images;
use crate::tui::text_field::{FieldShape, TextField, counted_lines, resting_lines};
use crate::tui::{Focus, Row, RowCut, RowTop, SessionState, has_context};

/// Descriptions start on the label's line only while that leaves them at
/// least this much room; otherwise they move to their own lines.
const MIN_DESCRIPTION_WIDTH: usize = 30;

/// Where descriptions start that go on the lines below their label, and
/// labels that go on the lines below their number.
const DESCRIPTION_INDENT: usize = 6;

/// Labels start on their option's row after the number only while that
/// leaves them at least this many columns; otherwise the whole label goes
/// on the lines below the option's row.
const MIN_LABEL_WIDTH: usize = 20;

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

    /// The state of a tab as tests need it (see [`Viewers::for_tests`]).
    #[cfg(test)]
    pub(super) fn for_tests(layout: PaneLayout, images: Images) -> Self {
        TabState {
            layout,
            shown: None,
            question_height: QuestionHeight::default(),
            viewers: Viewers::for_tests(images),
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
/// section 7.4, ADR 30). It is the most any question with context in the
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
/// the pane showed, if one did, where the field or note that has the keys
/// was drawn, if one was, and what the area showed of the range of the row
/// under the cursor, if it has one.
pub(super) fn draw_tab(
    area: Rect,
    buf: &mut Buffer,
    state: &SessionState,
    tab: &mut TabState,
    quote: QuoteStyle,
) -> (Option<DrawnPane>, Option<FieldShape>, Option<RowCut>) {
    let arrangement = state.has_context_area().then(|| {
        let arrangement = panes::choose(area.width, area.height, tab.layout, tab.shown);
        if arrangement != Arrangement::Hint {
            tab.shown = Some(arrangement);
        }
        arrangement
    });

    let content_at = |width: u16| match state.question() {
        Some(_) => question_content(state, state.tab(), usize::from(width), Pass::Live, quote),
        None => review_content(state, usize::from(width), quote),
    };
    let question_rows = match arrangement {
        Some(Arrangement::Below) => tab.question_height.need(state, area, quote),
        _ => 0,
    };
    let (text_area, context_area) = panes::areas(area, arrangement, question_rows);
    let content = content_at(text_area.width);

    let ledge = if arrangement == Some(Arrangement::Below) {
        Ledge::Below
    } else {
        Ledge::Beside
    };
    let pane = match context_area {
        Some(area) if arrangement == Some(Arrangement::Hint) => {
            let z = question_key(state, "z");
            context_hint(state.shown_context(), &z, area.width).render(area, buf);
            None
        }
        Some(area) => draw_pane(area, buf, state, &mut tab.viewers, quote, ledge),
        None => None,
    };
    // The viewers keep the window of the question shown, and load what it
    // lacks ahead for the body of a pane that shows, whatever its row
    // shows (ADR 46).
    if state.question().is_some() {
        let body = context_area
            .filter(|_| arrangement != Some(Arrangement::Hint))
            .map(|area| {
                let body = pane_body(area, ledge);
                (body.width, body.height)
            });
        tab.viewers
            .load_window(state.id(), &state.window(), body, quote);
    }

    // At rest the area shows the whole current row. A row taller than the
    // area shows its start, unless it holds the text cursor: then the
    // cursor's line stays in view, on the area's last row while the start
    // of the row is further up, so `↑` scrolls back to it row by row. On
    // the text's last line, the row's end, a counter under it included,
    // takes the area's last row instead, so the length shows while the
    // person types at the end; the cursor wins where both do not fit. The
    // keys that move the cursor move the area from there (`row_view`).
    let height = text_area.height as usize;
    let (start, end) = content.cursor;
    let at_rest = match &content.text_cursor {
        Some(cursor) if end - start >= height => {
            let kept = if cursor.last_line { end } else { cursor.line };
            kept.saturating_sub(height.saturating_sub(1))
                .max(start)
                .min(cursor.line)
        }
        _ => end.saturating_sub(height.saturating_sub(1)).min(start),
    };
    let total = content.lines.len();
    let (scroll, row_cut) = row_view(state, content.cursor, total, height, at_rest);
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
    (pane, content.shape, row_cut)
}

/// Where the question's area of `height` rows starts among the `total`
/// lines of its tab, and what that leaves of the range of the row under
/// the cursor, whose lines are `start` to `end` (spec section 7.4).
/// `at_rest` is where the area shows the row without the person moving
/// the view: the row whole with its end on the area's last row, a row
/// taller than the area from its start, and a field taller than it with
/// its text cursor in view.
///
/// The keys reach every line of the area through the range: on the first
/// row it runs up to the area's first line, on the last row down to its
/// last line. A row other than a field that does not fit the area with the
/// line above it and the line below it scrolls through all of them, so the
/// markers on the area's first and last rows cover those neighbours rather
/// than the ends of its own first and last lines.
fn row_view(
    state: &SessionState,
    (start, end): (usize, usize),
    total: usize,
    height: usize,
    at_rest: usize,
) -> (usize, Option<RowCut>) {
    // While a question's note has the keys, its lines are the ones the
    // cursor marks, and the area keeps its text cursor in view.
    let rows = state.rows();
    let row = rows.get(state.row());
    if state.focus() == Focus::Note || height == 0 || row.is_none() {
        return (at_rest, None);
    }

    let above = start.saturating_sub(1);
    let below = (end + 1).min(total.saturating_sub(1));
    let tall = row.is_some_and(|row| !row.is_field()) && below - above + 1 > height;
    // Where the row starts as the cursor reaches it, where it ends as the
    // cursor moving up reaches it, and the row's own range between them.
    let (first_view, last_view) = if tall {
        (above, below + 1 - height)
    } else {
        (at_rest, at_rest)
    };
    let low = if state.row() == 0 { 0 } else { first_view };
    let high = if state.row() + 1 == rows.len() {
        total.saturating_sub(height).max(last_view)
    } else {
        last_view
    };

    let scroll = match state.row_top() {
        RowTop::Start => first_view,
        RowTop::End => last_view,
        RowTop::Line(line) => (low + line).min(high),
    };
    let cut = (high > low).then(|| RowCut {
        above: scroll - low,
        below: high - scroll,
        home: first_view - low,
    });
    (scroll, cut)
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

/// The mark in front of an option or the own answer, bold in the primary
/// colour when chosen, so a chosen option and an own answer that counts
/// look alike.
fn mark(kind: Kind, chosen: bool) -> Span<'static> {
    let mark = match (kind, chosen) {
        (Kind::Single, true) => "(•)",
        (Kind::Single, false) => "( )",
        (_, true) => "[x]",
        (_, false) => "[ ]",
    };
    if chosen {
        Span::styled(mark, theme::chosen())
    } else {
        Span::from(mark)
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
    /// 7.4): no row is current and every field shows unfocused, with the
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
    let labels: Vec<LabelRows> = question
        .options
        .iter()
        .flatten()
        .enumerate()
        .map(|(index, option)| label_rows(&option.label, option_prefix_width(index), width, quote))
        .collect();
    // Descriptions line up in one column after the widest label while
    // every label takes a single line beside its number and the column
    // leaves the descriptions enough room; otherwise each goes below its
    // label.
    let description_column = labels
        .iter()
        .map(LabelRows::beside_width)
        .collect::<Option<Vec<_>>>()
        .and_then(|widths| widths.into_iter().max())
        .map(|widest| widest + 2)
        .filter(|column| width.saturating_sub(*column) >= MIN_DESCRIPTION_WIDTH);
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
                let head = vec![
                    pointer(current),
                    mark(question.kind, chosen),
                    Span::styled(format!(" {}. ", option_index + 1), theme::secondary()),
                ];
                let lines = option_lines(
                    head,
                    &labels[option_index],
                    description_column,
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
                // counts: it has text, no option is chosen and `backspace`
                // did not leave it unpicked. Blanks and line breaks alone
                // are no text, as in the result.
                let chosen = has_text(answer)
                    && (question.kind == Kind::Multi
                        || (answer.selected.is_empty() && !answer.custom_unpicked));
                let head = vec![
                    pointer(current),
                    mark(question.kind, chosen),
                    Span::from(format!(" ✎ {label}: ")),
                ];
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
                let head = vec![Span::from(format!("{}✎ ", pointer(current)))];
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
            // An info item's row reads like an option's, so the item never
            // looks empty and the pointer shows where `enter` moves on.
            Row::Next => {
                let spans = vec![pointer(current), Span::from("next")];
                let spans = if filled {
                    spans.into_iter().map(theme::focus_span).collect()
                } else {
                    spans
                };
                (vec![Line::from(spans)], None)
            }
            Row::Question(_) | Row::Submit | Row::Note | Row::Reject => {
                unreachable!("review rows only appear on the review")
            }
        };
        content.push_row(lines, current, text_cursor);
    }

    // Every question and info item takes a note (ADR 68).
    content.lines.push(Line::default());
    let editing = live && state.focus() == Focus::Note;
    // A note has no length to count against, and its head is never filled.
    let row = FieldRow {
        head: vec![Span::styled("Notes: ", theme::strong())],
        text: answer.note.as_deref().unwrap_or_default(),
        editing,
        fill: false,
        limits: None,
    };
    let (note_lines, text_cursor) = live_field(state, row, width, &mut content.shape);
    content.push_row(note_lines, editing, text_cursor);
    content
}

/// The width of an option's pointer, mark and number.
fn option_prefix_width(index: usize) -> usize {
    columns(POINTER) + columns(&format!("( ) {}. ", index + 1))
}

/// An option's label rendered for its option, in bold over its Markdown
/// styles.
struct LabelRows {
    /// The label's lines, without blanks at their ends. An empty or blank
    /// label has none.
    rows: Vec<Line<'static>>,
    /// Whether the first line follows the number on the option's row.
    beside: bool,
    /// The column the label's lines start in, the first one's too when it
    /// is beside the number.
    indent: usize,
}

impl LabelRows {
    /// The columns from the start of the option's row to the end of its
    /// label, when the label takes at most the one line beside its number;
    /// a label without lines ends where the number does.
    fn beside_width(&self) -> Option<usize> {
        match self.rows.as_slice() {
            _ if !self.beside => None,
            [] => Some(self.indent),
            [row] => Some(self.indent + line_columns(row)),
            _ => None,
        }
    }
}

/// `label` rendered as the label of an option whose pointer, mark and
/// number take `prefix` of `width` columns, with Markdown quotes in
/// `quote`. It follows the number and wraps under its own start while that
/// leaves it `MIN_LABEL_WIDTH` columns; otherwise the whole label goes on
/// the lines below, from `DESCRIPTION_INDENT`, or from the left edge
/// where not even a wide character would fit after that indent.
fn label_rows(label: &str, prefix: usize, width: usize, quote: QuoteStyle) -> LabelRows {
    let (beside, indent) = if width.saturating_sub(prefix) >= MIN_LABEL_WIDTH {
        (true, prefix)
    } else if width >= DESCRIPTION_INDENT + WIDEST_GRAPHEME {
        (false, DESCRIPTION_INDENT)
    } else {
        (false, 0)
    };
    let rows = field_lines(label, width - indent, quote)
        .into_iter()
        .map(|row| {
            let spans = trim_end(row.spans)
                .into_iter()
                .map(|span| span.patch_style(theme::strong()))
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect();
    LabelRows {
        rows,
        beside,
        indent,
    }
}

/// The columns `line` takes.
fn line_columns(line: &Line) -> usize {
    line.spans.iter().map(|span| columns(&span.content)).sum()
}

/// An option: `head`, its pointer, mark and number, then `label`, all of
/// it under the focus fill where `filled`, then the description. With a
/// `description_column`, the description starts there on the option's row,
/// rendered for the columns from there to the end and so wrapped under its
/// own start; without one, it goes on the lines below the label's, from
/// `DESCRIPTION_INDENT` on. The description's quotes show in `quote`.
fn option_lines(
    head: Vec<Span<'static>>,
    label: &LabelRows,
    description_column: Option<usize>,
    description: Option<&str>,
    width: usize,
    filled: bool,
    quote: QuoteStyle,
) -> Vec<Line<'static>> {
    let fill = |spans: Vec<Span<'static>>| -> Vec<Span<'static>> {
        if filled {
            spans.into_iter().map(theme::focus_span).collect()
        } else {
            spans
        }
    };
    let mut label_lines = label.rows.iter().map(|row| fill(row.spans.clone()));
    let mut first = fill(head);
    if label.beside {
        first.extend(label_lines.next().unwrap_or_default());
    }

    let indent = description_column.unwrap_or(DESCRIPTION_INDENT);
    // An empty or blank description renders to no lines at all.
    let mut descriptions = description
        .map(|description| field_lines(description, width.saturating_sub(indent), quote))
        .unwrap_or_default()
        .into_iter();
    // A description beside its label starts on the option's row, padded
    // to the description column.
    if let Some(column) = description_column
        && let Some(row) = descriptions.next()
    {
        let used: usize = first.iter().map(|span| columns(&span.content)).sum();
        first.push(Span::from(" ".repeat(column - used)));
        first.extend(row.spans);
    }

    let mut lines = vec![Line::from(first)];
    lines.extend(label_lines.map(|spans| indented(Line::from(spans), label.indent)));
    lines.extend(descriptions.map(|row| indented(row, indent)));
    lines
}

/// A row that is a field: an own answer, a text answer, a note, the
/// session note or the reject reason.
struct FieldRow<'a> {
    /// What comes before the text: the pointer, marks and a label.
    head: Vec<Span<'static>>,
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
        None if row.fill => {
            let head = row.head.into_iter().map(theme::focus_span).collect();
            (resting_lines(head, true, row.text, width), None)
        }
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

fn review_content(state: &SessionState, width: usize, quote: QuoteStyle) -> Content {
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
            head: vec![Span::from(head)],
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
                let summary = cut_spans(
                    answer_summary(question, &answers[question_index], room, quote),
                    room,
                    theme::secondary(),
                );
                let mut spans = lead;
                spans.extend(summary);
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
            Row::Option(_) | Row::Own | Row::Answer | Row::Next => {
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

    /// The session after pressing `keys`, `E` standing for `esc`, `B` for
    /// `backspace` and `\n` for `enter`.
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
                'B' => KeyCode::Backspace,
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
            None => review_content(state, 40, quote_style(false)),
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
        assert_eq!(shape.head, [Span::from("Notes: ").bold()]);
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
    fn an_info_item_shows_the_next_row_under_the_pointer_like_an_option() {
        let json = r#"{"asqr": 2, "questions": [{"id": "i", "text": "All checks passed.", "kind": "info"}]}"#;
        let state = SessionState::new("info", serde_json::from_str(json).expect("parses"), None);

        let (content, buf) = tab_of(&state);

        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| test_support::row(&buf, y))
            .collect();
        assert_eq!(
            rows,
            [
                "All checks passed.",
                "nothing to answer",
                "",
                "❯ next",
                "",
                "Notes: —",
            ]
        );
        // The row is the current one, so a text taller than the area is
        // cut above it, as on every question.
        assert_eq!(content.cursor, (3, 3));
        let next = test_support::find(&buf, "❯ next");
        assert_eq!(
            (buf[next].fg, buf[next].modifier),
            (Color::Yellow, Modifier::REVERSED)
        );
    }

    #[test]
    fn the_notes_line_shows_where_the_file_sets_note_false() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "note": false, "options": [{"id": "a", "label": "A"}]}]}"#;
        let state = SessionState::new("notes", serde_json::from_str(json).expect("parses"), None);

        let (_, buf) = tab_of(&state);

        assert_eq!(
            test_support::row(&buf, buf.area.height - 1),
            "Notes: —",
            "every question takes a note (ADR 68)"
        );
    }

    #[test]
    fn only_a_field_counts_its_length_while_it_has_the_keys() {
        // The text question takes the keys on arrival.
        let (content, buf) = tab_of(&after("labcd"));
        let shape = content.shape.expect("the field has the keys");
        assert_eq!(shape.head, [Span::from("❯ ✎ ")]);
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

    /// A multi question of two options after `keys`.
    fn multi_after(keys: &str) -> (Content, Buffer) {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let json = r#"{"asqr": 2, "questions": [{"id": "m", "text": "?", "kind": "multi",
            "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}]}"#;
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

    /// The text colour and the modifiers of a cell.
    type Look = (Color, Modifier);

    /// The look of each of the first `width` cells where `text` starts.
    fn looks_from(buf: &Buffer, text: &str, width: u16) -> Vec<Look> {
        let (x, y) = test_support::find(buf, text);
        (x..x + width)
            .map(|x| (buf[(x, y)].fg, buf[(x, y)].modifier))
            .collect()
    }

    /// The look of the own answer's three-cell mark in `buf`, then of the
    /// blank and the `✎` after it, starting where `mark` shows before `✎`.
    fn own_mark_looks(buf: &Buffer, mark: &str) -> (Vec<Look>, Vec<Look>) {
        let looks = looks_from(buf, &format!("{mark} ✎"), 5);
        (looks[..3].to_vec(), looks[3..].to_vec())
    }

    #[test]
    fn a_counting_own_answer_shows_its_mark_bold_primary_off_the_fill() {
        let cases = [
            (after("jabEk"), "the cursor on another row"),
            (after("jab"), "the field has the keys"),
            (with_pane_focused("jabE"), "the pane has the focus"),
            (
                with_pane_focused("jab"),
                "the pane has the focus while the field keeps the keys",
            ),
        ];

        // Compared all at once, so a failure shows every case.
        let looks: Vec<_> = cases
            .iter()
            .map(|(state, case)| (*case, own_mark_looks(&tab_of(state).1, "(•)")))
            .collect();
        let bold_primary = (Color::Cyan, Modifier::BOLD);
        let plain = (Color::Reset, Modifier::empty());
        let expected: Vec<_> = cases
            .iter()
            .map(|(_, case)| (*case, (vec![bold_primary; 3], vec![plain; 2])))
            .collect();
        assert_eq!(looks, expected, "the mark and then the blank and `✎`");
    }

    #[test]
    fn an_own_answer_that_does_not_count_keeps_its_mark_plain() {
        let plain = (Color::Reset, Modifier::empty());
        for (keys, case) in [
            ("", "empty"),
            ("j  Ek", "blanks only"),
            // Enter picks A and moves on to the text question, which takes
            // the keys; `esc` and `h` come back.
            ("jabEk\nEh", "an option chosen besides the text"),
            ("jabEk\nEhB", "the pick cleared, the text left unpicked"),
        ] {
            let (_, buf) = tab_of(&after(keys));
            let (mark, _) = own_mark_looks(&buf, "( )");
            assert_eq!(mark, [plain; 3], "{case}: {:?}", test_support::rows(&buf));
        }
    }

    #[test]
    fn a_filled_own_answer_is_one_colour_with_its_chosen_mark() {
        let (_, buf) = tab_of(&after("jabE"));
        let looks = looks_from(&buf, "❯ (•) ✎ Own answer: ", 20);
        for (x, (colour, modifier)) in looks.iter().enumerate() {
            assert_eq!((*colour, *modifier & !Modifier::BOLD), FILLED, "column {x}");
        }
        let (mark, _) = own_mark_looks(&buf, "(•)");
        assert!(
            mark.iter()
                .all(|(_, modifier)| modifier.contains(Modifier::BOLD)),
            "the mark keeps its bold under the fill: {mark:?}"
        );
    }

    #[test]
    fn a_multi_own_answer_with_text_shows_its_mark_bold_primary() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // On a multi question the own answer counts with an option ticked
        // too. The cursor ends on B, so neither A nor the own answer is
        // under the fill.
        let json = r#"{"asqr": 2, "questions": [{"id": "m", "text": "?", "kind": "multi",
            "custom": true,
            "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}]}"#;
        let mut state =
            SessionState::new("multi", serde_json::from_str(json).expect("parses"), None);
        for key in " jjabEk".chars() {
            let code = if key == 'E' {
                KeyCode::Esc
            } else {
                KeyCode::Char(key)
            };
            state.handle(KeyEvent::new(code, KeyModifiers::NONE));
        }
        let (_, buf) = tab_of(&state);

        let bold_primary = (Color::Cyan, Modifier::BOLD);
        assert_eq!(looks_from(&buf, "[x] 1. A", 3), [bold_primary; 3], "A");
        let (mark, _) = own_mark_looks(&buf, "[x]");
        assert_eq!(mark, [bold_primary; 3], "the own answer");
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
        let mut tab = TabState::for_tests(PaneLayout::Auto, Images::half_blocks());
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
        let mut tab = TabState::for_tests(PaneLayout::Beside, Images::half_blocks());
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

    // -----------------------------------------------------------------
    // Option labels
    // -----------------------------------------------------------------

    /// A single question whose options carry `options`, a JSON list.
    fn labelled(options: &str) -> SessionState {
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "single",
                "options": {options}}}]}}"#
        );
        SessionState::new("labels", serde_json::from_str(&json).expect("parses"), None)
    }

    /// The lines of the options of the question of `state`, `width`
    /// columns wide, counted without a cursor: from the first option's row
    /// to the blank row before the notes line every question ends with.
    fn option_rows(state: &SessionState, width: usize) -> Vec<Line<'static>> {
        let lines = question_content(state, 0, width, Pass::Counted, quote_style(false)).lines;
        let first = lines
            .iter()
            .position(|line| {
                test_support::line_texts(std::slice::from_ref(line))[0].starts_with("  ( ) 1.")
            })
            .expect("the first option shows");
        // The empty notes line wraps in a narrow pane like any field.
        let notes = resting_lines(
            vec![Span::styled("Notes: ", theme::strong())],
            false,
            "",
            width,
        )
        .len();
        lines[first..lines.len() - notes - 1].to_vec()
    }

    #[test]
    fn a_label_without_room_beside_its_number_goes_below_it() {
        // The number takes 9 columns, which leaves 16 of 25.
        let state = labelled(r#"[{"id": "a", "label": "Rewrite the parser from scratch"}]"#);

        assert_eq!(
            test_support::line_texts(&option_rows(&state, 25)),
            [
                "  ( ) 1. ",
                "      Rewrite the parser",
                "      from scratch"
            ]
        );
    }

    #[test]
    fn a_label_in_a_pane_narrower_than_its_indent_starts_at_the_left_edge() {
        let state = labelled(r#"[{"id": "a", "label": "Rewrite the parser"}]"#);

        assert_eq!(
            test_support::line_texts(&option_rows(&state, 7)),
            ["  ( ) 1. ", "Rewrite", "the", "parser"]
        );
    }

    #[test]
    fn a_label_is_never_cut_at_any_width() {
        let token = "x".repeat(60);
        let label = format!("Keep `retry` and {token} 日本語の説明 🎉");
        let state = labelled(&serde_json::json!([{"id": "a", "label": label}]).to_string());
        let expected = format!("Keepretryand{token}日本語の説明🎉");

        for width in 2..=60 {
            let rows = option_rows(&state, width);
            let mut shown = String::new();
            for (index, row) in rows.iter().enumerate() {
                // The option's row starts with the pointer, the mark and
                // the number; a label beside them fits the width as well.
                let label_spans = if index == 0 {
                    &row.spans[3..]
                } else {
                    &row.spans[..]
                };
                if label_spans
                    .iter()
                    .any(|span| !span.content.trim().is_empty())
                {
                    let row_width: usize =
                        row.spans.iter().map(|span| columns(&span.content)).sum();
                    assert!(row_width <= width, "row {index} at {width}: {row:?}");
                }
                for span in label_spans {
                    // A word wider than the room goes on behind a dim marker.
                    let text = match span.content.strip_prefix("↪ ") {
                        Some(rest) if span.style.add_modifier.contains(Modifier::DIM) => rest,
                        _ => &span.content,
                    };
                    shown.extend(text.chars().filter(|c| !c.is_whitespace()));
                }
            }
            assert_eq!(shown, expected, "at {width}");
        }
    }

    #[test]
    fn a_filled_option_fills_every_line_of_its_label() {
        let state =
            labelled(r#"[{"id": "a", "label": "A label long enough to wrap onto a second line"}]"#);
        let (_, buf) = tab_of(&state);

        let (_, y) = test_support::find(&buf, "❯ ( ) 1. A label");
        assert!(
            y + 1 < buf.area.height,
            "the label takes a second line: {:?}",
            test_support::rows(&buf)
        );
        let second = test_support::row(&buf, y + 1);
        assert!(second.contains("line"), "{:?}", test_support::rows(&buf));
        for x in 0..40 {
            let cell = &buf[(x, y + 1)];
            if cell.symbol() != " " {
                assert_eq!(
                    (cell.fg, cell.modifier & !Modifier::BOLD),
                    FILLED,
                    "column {x} of {second:?}"
                );
            }
        }
    }

    #[test]
    fn descriptions_stand_beside_short_labels_and_below_long_ones() {
        let short = labelled(
            r#"[{"id": "a", "label": "Alpha", "description": "First."},
                {"id": "b", "label": "", "description": "Second."},
                {"id": "c", "label": "Gamma", "description": "Third."}]"#,
        );
        assert_eq!(
            test_support::line_texts(&option_rows(&short, 80)),
            [
                "  ( ) 1. Alpha  First.",
                "  ( ) 2.        Second.",
                "  ( ) 3. Gamma  Third."
            ]
        );

        // 9 columns of number and 45 of label leave 24 beside it.
        let long = labelled(
            r#"[{"id": "a", "label": "Alpha", "description": "First."},
                {"id": "b", "label": "A label of forty-five columns, too wide here", "description": "Second."}]"#,
        );
        assert_eq!(
            test_support::line_texts(&option_rows(&long, 80)),
            [
                "  ( ) 1. Alpha",
                "      First.",
                "  ( ) 2. A label of forty-five columns, too wide here",
                "      Second."
            ]
        );
    }

    #[test]
    fn an_empty_label_keeps_its_head_and_description_where_they_are() {
        let state = labelled(r#"[{"id": "a", "label": "", "description": "Described."}]"#);

        assert_eq!(
            test_support::line_texts(&option_rows(&state, 60)),
            ["  ( ) 1.   Described."]
        );
    }

    #[test]
    fn a_blank_label_takes_no_columns() {
        let state = labelled(r#"[{"id": "a", "label": "   ", "description": "Described."}]"#);

        assert_eq!(
            test_support::line_texts(&option_rows(&state, 60)),
            ["  ( ) 1.   Described."]
        );
    }

    #[test]
    fn a_wrapped_label_counts_the_lines_it_draws() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "context": [{"type": "code", "content": "x"}],
            "options": [{"id": "a", "label": "A label long enough to wrap onto a second line and a third one too"},
                        {"id": "b", "label": "B"}]}]}"#;
        let mut state =
            SessionState::new("count", serde_json::from_str(json).expect("parses"), None);
        let count = |state: &SessionState, pass| {
            question_content(state, 0, 40, pass, quote_style(false))
                .lines
                .len()
        };

        assert_eq!(
            count(&state, Pass::Live),
            count(&state, Pass::Counted),
            "on the label"
        );
        state.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            count(&state, Pass::Live),
            count(&state, Pass::Counted),
            "off it"
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

    // -----------------------------------------------------------------
    // The view of the area for the row under the cursor
    // -----------------------------------------------------------------

    /// `state` drawn as its tab into an area of 30×6 without context: its
    /// rows, each cut to its text, and the cut of the row under the cursor.
    fn viewed(state: &SessionState) -> (Vec<String>, Option<RowCut>) {
        let area = Rect::new(0, 0, 30, 6);
        let mut buf = Buffer::empty(area);
        let mut tab = TabState::for_tests(PaneLayout::Auto, Images::half_blocks());
        let (_, _, cut) = draw_tab(area, &mut buf, state, &mut tab, quote_style(false));
        let rows = (0..6).map(|y| test_support::row(&buf, y)).collect();
        (rows, cut)
    }

    /// `count` words of three letters from `a01` on, which take 21
    /// columns, five words, to a row beside an option's number.
    fn three_letter_words(count: usize) -> String {
        (1..=count)
            .map(|n| format!("a{n:02}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn key(state: &mut SessionState, code: ratatui::crossterm::event::KeyCode) {
        use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

        state.handle(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn the_view_of_a_tall_option_runs_from_the_line_above_it_to_the_line_below_it() {
        use ratatui::crossterm::event::KeyCode;

        // The text, the hint and a blank take lines 0 to 2, the tall option
        // lines 3 to 10 and the short one line 11.
        let mut state = labelled(&format!(
            r#"[{{"id": "a", "label": "{}"}}, {{"id": "b", "label": "Other"}}]"#,
            three_letter_words(40)
        ));

        let (rows, cut) = viewed(&state);
        assert_eq!(
            cut,
            Some(RowCut {
                above: 2,
                below: 4,
                home: 2
            }),
            "on the first row the range reaches the text: {rows:#?}"
        );
        assert!(rows[0].ends_with("↑ more"), "{rows:#?}");
        assert_eq!(rows[1], "❯ ( ) 1. a01 a02 a03 a04 a05");

        state.set_row_cut(cut);
        for _ in 0..4 {
            key(&mut state, KeyCode::Down);
        }
        let (rows, cut) = viewed(&state);
        assert_eq!(state.row(), 0);
        assert_eq!(rows[4], "         a36 a37 a38 a39 a40");
        assert!(rows[5].starts_with("  ( ) 2. Other"), "{rows:#?}");
        assert_eq!(cut.map(|cut| cut.below), Some(0));

        state.set_row_cut(cut);
        key(&mut state, KeyCode::Down);
        assert_eq!(state.row(), 1, "the end of the range moves on");
        key(&mut state, KeyCode::Up);
        let (rows, _) = viewed(&state);
        assert_eq!(
            rows[4], "         a36 a37 a38 a39 a40",
            "moving up shows the end: {rows:#?}"
        );
    }

    #[test]
    fn an_option_as_tall_as_the_area_or_one_line_less_scrolls_too() {
        // The middle option of 6 lines, or of 5, does not fit 6 rows with
        // the lines around it; one of 4 does.
        for (words, tall) in [(30, true), (25, true), (20, false)] {
            let mut state = labelled(&format!(
                r#"[{{"id": "a", "label": "A"}}, {{"id": "b", "label": "{}"}},
                    {{"id": "c", "label": "C"}}]"#,
                three_letter_words(words)
            ));
            key(&mut state, ratatui::crossterm::event::KeyCode::Down);

            let (rows, cut) = viewed(&state);
            assert_eq!(cut.is_some(), tall, "{words} words: {rows:#?}");
            if tall {
                assert_eq!(rows[1], "❯ ( ) 2. a01 a02 a03 a04 a05", "{words}");
            }
        }
    }

    #[test]
    fn the_first_and_last_rows_reach_the_ends_of_the_question() {
        use ratatui::crossterm::event::KeyCode;

        // A text of 8 lines above two options and a note of 3 lines below.
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "{}", "kind": "single",
                "options": [{{"id": "a", "label": "A"}}, {{"id": "b", "label": "B"}}]}}]}}"#,
            three_letter_words(56)
        );
        let draft = crate::format::SessionResult::draft(
            "ends",
            "q",
            vec![crate::format::Answer {
                note: Some("n1\nn2\nn3".into()),
                ..crate::format::Answer::new("q")
            }],
        );
        let mut state = SessionState::new(
            "ends",
            serde_json::from_str(&json).expect("parses"),
            Some(&draft),
        );

        let (rows, cut) = viewed(&state);
        assert!(rows[5].contains("( ) 1. A"), "{rows:#?}");
        let cut = cut.expect("the text above can be reached");
        assert_eq!((cut.below, cut.above), (0, cut.home));

        key(&mut state, KeyCode::Down);
        let (rows, cut) = viewed(&state);
        let cut = cut.expect("the note below can be reached");
        assert_eq!((cut.above, cut.home), (0, 0), "{rows:#?}");
        assert!(cut.below > 0, "{rows:#?}");
    }
}
