// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The screen (spec sections 7.1 to 7.4, ADR 22): a title bar, the session
//! header, the tab bar, the current tab, a status line and the key bar.
//!
//! The content of a tab is wrapped here rather than by the paragraph
//! widget, so the number of lines is known and the row under the cursor
//! can always be scrolled into view, whatever the size of the terminal.

use std::collections::HashMap;
use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui_image::picker::Picker;
use unicode_segmentation::UnicodeSegmentation;

use super::context::{context_hint, draw_pane};
use super::panes::{self, Arrangement, PaneLayout, READING_WIDTH};
use super::parts::{answer_summary, cut, kind_hint, question_key};
use super::source_line::{columns, fitting};
use super::text::field_lines;
use super::viewer::Viewers;
use crate::format::{ContextEntry, Kind, has_text};
use crate::tui::text_field::{FieldShape, TextField, counted_lines, resting_lines};
use crate::tui::{AnswerState, App, Extent, Focus, Page, Row, SessionState, has_context};

/// Below this size the layout does not fit; a message says so instead.
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;

/// Descriptions start on the label's line only while that leaves them at
/// least this much room; otherwise they move to their own lines.
const MIN_DESCRIPTION_WIDTH: usize = 30;

/// Where descriptions start that go on the lines below their label.
const DESCRIPTION_INDENT: usize = 6;

/// `text` in at most `room` columns: whole as long as it fits, otherwise
/// its end behind `…`, cut at a `/` where one is in reach, since the end
/// of a queue's path is what tells queues apart.
fn shorten_from_left(text: &str, room: usize) -> String {
    if columns(text) <= room {
        return text.to_owned();
    }
    // The end that fits behind the `…`, taken grapheme by grapheme from the
    // last one, so a wide character that would not fit stays out whole.
    let keep = room.saturating_sub(1);
    let mut start = text.len();
    let mut used = 0;
    for (index, grapheme) in text.grapheme_indices(true).rev() {
        used += columns(grapheme);
        if used > keep {
            break;
        }
        start = index;
    }
    let tail = &text[start..];
    let tail = match tail.find('/') {
        Some(slash) => &tail[slash..],
        None => tail,
    };
    format!("…{tail}")
}

/// What the screen shows besides the state, and what it keeps from one
/// draw to the next.
pub struct View<'a> {
    /// The queue name, or its directory for a queue given by `--dir`.
    queue: &'a str,
    layout: PaneLayout,
    /// The last arrangement with a context area shown, beside or below;
    /// [`panes::choose`] keeps it while a resize stays near a square
    /// shape. A hint leaves it as it is.
    shown: Option<Arrangement>,
    /// What the intro overlay showed in the last draw.
    intro_page: Extent,
    /// The viewers of the context entries shown so far.
    viewers: Viewers,
    /// What the context pane showed in the last draw; see
    /// [`View::pane_page`].
    pane_page: Option<Option<Page>>,
    /// Where the field or note that has the keys was last drawn.
    field_shape: Option<FieldShape>,
    question_height: QuestionHeight,
    /// Whether the last draw showed the help, the intro or the session
    /// list over the screen.
    overlay_shown: bool,
    /// Whether the last draw showed an image entry in the context pane.
    image_shown: bool,
}

/// What a question's rows are kept by: the session, the revision of its
/// file, the question's index and the width it was counted at.
type RowsKey = (String, u64, usize, u16);

/// How many rows the question takes above a context pane below it (spec
/// section 7.1, ADR 30). It is the most any question with context in the
/// session needs, so moving between questions never moves the pane, and it
/// follows what the person types only line by line.
#[derive(Default)]
struct QuestionHeight {
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
    fn need(&mut self, state: &SessionState, area: Rect) -> usize {
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

    /// Forgets the rows of sessions that are gone and of replaced files.
    fn retain(&mut self, sessions: &[SessionState]) {
        self.rows.retain(|(id, revision, ..), _| {
            sessions
                .iter()
                .any(|state| state.id() == id && state.revision() == *revision)
        });
    }
}

impl<'a> View<'a> {
    /// The view of `queue`, with images sent in the graphics protocol
    /// `picker` speaks.
    pub fn new(queue: &'a str, layout: PaneLayout, picker: Picker) -> Self {
        View {
            queue,
            layout,
            shown: None,
            intro_page: Extent::default(),
            viewers: Viewers::new(picker),
            pane_page: None,
            field_shape: None,
            question_height: QuestionHeight::default(),
            overlay_shown: false,
            image_shown: false,
        }
    }

    /// Whether the terminal must be cleared before `app` is drawn: an
    /// overlay that the last draw showed over an image entry is closed.
    ///
    /// The Sixel and iTerm2 protocols of `ratatui-image` mark the cells an
    /// image covers as skipped, so ratatui never writes them again, also
    /// not where an overlay's text stood over the image. A cleared terminal
    /// is drawn whole, image included. The image entry is the one the last
    /// draw showed under the overlay; a session picked in the list may
    /// have none, and then the clear only costs a full redraw.
    pub fn must_clear(&self, app: &App) -> bool {
        self.overlay_shown && self.image_shown && !overlay_open(app)
    }

    /// What the intro overlay showed in the last draw, for
    /// [`App::set_intro_page`]; nothing while it is closed.
    pub fn intro_page(&self) -> Extent {
        self.intro_page
    }

    /// What the context pane showed in the last draw, for
    /// [`App::set_pane_page`]: `Some(None)` when the draw showed a tab
    /// without a pane, and `None` when it showed no tab at all, as below
    /// the minimum size or with an empty queue. Such a frame says nothing
    /// about the pane, so it is not handed on, and a moment at a small size
    /// keeps the pane's focus and the full-screen view.
    pub fn pane_page(&self) -> Option<Option<Page>> {
        self.pane_page
    }

    /// Where the field or note that has the keys was drawn in the last
    /// draw, for [`App::set_field_shape`]; nothing when none was.
    pub fn field_shape(&self) -> Option<FieldShape> {
        self.field_shape.clone()
    }

    /// Reads and sends every image again as it is next shown (ADR 24).
    pub fn forget_images(&mut self) {
        self.viewers.forget_images();
    }
}

pub fn draw(frame: &mut Frame, app: &App, view: &mut View) {
    // Whatever returns early draws no tab, no pane and no overlay.
    view.pane_page = None;
    view.field_shape = None;
    view.overlay_shown = false;
    view.image_shown = false;
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = format!(
            "asqr needs at least {MIN_WIDTH}×{MIN_HEIGHT}; this terminal is {}×{}.",
            area.width, area.height
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    }

    // The header text keeps to the reading width the question has, after
    // the margin on the left and before the one on the right.
    let header_width = usize::from(area.width.min(READING_WIDTH + 2));
    let header_lines = app
        .active()
        .map(|state| header_lines(state, header_width, area.height))
        .unwrap_or_default();
    let header_height = u16::try_from(header_lines.len()).expect("at most five header lines");

    let [title, header, tabs, main, status, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(header_height.max(1)),
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let waiting = app.sessions().len();
    let count = format!(
        " · {waiting} session{} waiting",
        if waiting == 1 { "" } else { "s" }
    );
    let prefix = " asqr · queue: ";
    let room = usize::from(title.width).saturating_sub(columns(prefix) + columns(&count));
    let title_text = format!("{prefix}{}{count}", shorten_from_left(view.queue, room));
    frame.render_widget(Line::from(title_text).reversed(), title);

    let Some(state) = app.active() else {
        let empty =
            Paragraph::new("Nothing to answer. New sessions show up here as soon as they arrive.")
                .centered()
                .wrap(Wrap { trim: true });
        frame.render_widget(empty, center(main, main.width.saturating_sub(4), 3));
        // A notice matters most here: an invalid session that was the only
        // one leaves the queue empty, and the notice is all that says why.
        let notice = app.current_notice().unwrap_or_default();
        frame.render_widget(Line::from(notice).fg(Color::Yellow), status);
        frame.render_widget(key_line(&[("q", "quit")], keys.width), keys);
        return;
    };

    frame.render_widget(Paragraph::new(header_lines), header);
    draw_tabs(frame, state, tabs);

    view.viewers
        .retain_sessions(|id| app.sessions().iter().any(|waiting| waiting.id() == id));
    view.question_height.retain(app.sessions());
    let main = main.inner(ratatui::layout::Margin::new(1, 0));
    let pane_page = if state.full_screen() && state.shown_context().is_some() {
        draw_pane(frame, state, main, &mut view.viewers)
    } else {
        draw_tab(frame, state, main, view)
    };
    view.pane_page = Some(pane_page);
    view.image_shown =
        pane_page.is_some() && matches!(state.shown_entry(), Some((_, ContextEntry::Image(_))));

    let feedback = app.current_notice().or(state.message()).unwrap_or_default();
    frame.render_widget(Line::from(feedback).fg(Color::Yellow), status);

    // The intro overlay leaves the key bar free, which says how to scroll
    // and close it. It never shows together with the other overlays: while
    // it is open it takes every key, and any key closes the help.
    view.intro_page = match (app.intro_scroll(), session_intro(state)) {
        (Some(scroll), Some(intro)) => {
            let above_keys = Rect {
                height: area.height - 1,
                ..area
            };
            draw_intro(frame, intro, scroll, above_keys)
        }
        _ => Extent::default(),
    };

    let hints = match app.intro_scroll() {
        Some(_) if view.intro_page.total > view.intro_page.shown => vec![
            ("↑/↓", "scroll"),
            ("pgup/pgdn", "page"),
            ("esc/q/i", "close"),
        ],
        Some(_) => vec![("esc/q/i", "close")],
        None => key_hints(state, app.list_cursor().is_some(), pane_page),
    };
    frame.render_widget(key_line(&hints, keys.width), keys);

    // These overlays use the whole screen, so they fit the smallest one.
    if state.help() {
        draw_help(frame, area, state);
    }
    if let Some(cursor) = app.list_cursor() {
        draw_session_list(frame, app, cursor, area);
    }
    view.overlay_shown = overlay_open(app);
}

/// Whether `app` shows an overlay over the screen: the help, the intro or
/// the session list. A screen too small for asqr shows none of them, and
/// neither does an empty queue.
fn overlay_open(app: &App) -> bool {
    app.active().is_some_and(|state| {
        state.help()
            || app.list_cursor().is_some()
            || (app.intro_scroll().is_some() && session_intro(state).is_some())
    })
}

fn session_intro(state: &SessionState) -> Option<&str> {
    state.session().intro.as_deref()
}

/// The whole intro in a frame over the screen, from line `scroll` on, and
/// what of it fits. An intro longer than the frame scrolls, and the frame's
/// bottom edge says which lines show. The intro shows in patine's styles
/// without the header's dim.
fn draw_intro(frame: &mut Frame, intro: &str, scroll: usize, area: Rect) -> Extent {
    // The text keeps to the reading width, with a blank column inside each
    // border.
    let width = area.width.saturating_sub(4).min(READING_WIDTH + 4);
    let inner = usize::from(width.saturating_sub(2));
    let lines: Vec<Line<'static>> = field_lines(intro, inner.saturating_sub(2))
        .into_iter()
        .map(|line| indented(line, 1))
        .collect();
    let wanted = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let area = center(area, width, wanted);
    let page = Extent {
        shown: usize::from(area.height.saturating_sub(2)),
        total: lines.len(),
    };
    let scroll = scroll.min(page.last_start());

    let mut block = Block::bordered().title(" Intro ");
    if page.total > page.shown {
        let position = format!(" {}–{} of {} ", scroll + 1, scroll + page.shown, page.total);
        block = block.title_bottom(Line::from(position).dim().right_aligned());
    }
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
            .block(block),
        area,
    );
    page
}

/// The session header in `width` columns: the title line, then the intro,
/// both wrapped. The title takes at most two lines and the intro three,
/// one each on short terminals, so the question keeps its room. A cut
/// title ends in `…`; a cut intro ends in `… i: more`, as `i` shows all
/// of it, or in `… esc, i: more` where `i` would be typed or taken by the
/// pane.
fn header_lines(state: &SessionState, width: usize, height: u16) -> Vec<Line<'static>> {
    let short = height < 24;
    let session = state.session();
    let title = session
        .title
        .clone()
        .unwrap_or_else(|| state.id().to_owned());
    let mut spans = vec![Span::from(title).bold()];
    if let Some(from) = &session.from {
        spans.push(Span::from(format!("  ({from})")));
    }
    if let Some(follows) = &session.follows {
        spans.push(Span::from(format!("  follows: {follows}")).dim());
    }
    // Lines end a column before the edge, as the question's text does.
    let room = width.saturating_sub(1);
    let mut lines = at_most(
        wrap(Line::from(spans), room, 1, 1),
        if short { 1 } else { 2 },
        "…",
        room,
    );

    let Some(intro) = session.intro.as_deref() else {
        return lines;
    };
    // Dim comes on top of patine's styles, which still show underneath.
    let intro: Vec<Line<'static>> = field_lines(intro, room.saturating_sub(1))
        .into_iter()
        .map(|line| indented(line, 1).dim())
        .collect();
    let more = format!("… {}: more", question_key(state, "i"));
    lines.extend(at_most(intro, if short { 1 } else { 3 }, &more, room));
    lines
}

/// The first `count` of `lines`; when that cuts some off, the last one
/// kept ends in `marker`, within `width`.
fn at_most(
    mut lines: Vec<Line<'static>>,
    count: usize,
    marker: &str,
    width: usize,
) -> Vec<Line<'static>> {
    if lines.len() <= count {
        return lines;
    }
    lines.truncate(count);
    let last = lines.pop().expect("at least one line is kept");
    lines.push(ending_in(last, marker, width));
    lines
}

/// `line` shortened so that `marker` fits after it within `width`
/// columns. What stays keeps its styles, and blanks before the marker go.
fn ending_in(line: Line<'static>, marker: &str, width: usize) -> Line<'static> {
    let mut room = width.saturating_sub(columns(marker));
    let mut spans: Vec<Span<'static>> = Vec::new();
    for span in line.spans {
        let kept = fitting(&span.content, room);
        room -= columns(kept);
        if !kept.is_empty() {
            spans.push(Span::styled(kept.to_owned(), span.style));
        }
        // A wide character that did not fit ends the line, even where a
        // narrower one after it would fill the column it leaves.
        if kept.len() < span.content.len() {
            break;
        }
    }
    // The indent span stays, so a line of nothing but blanks keeps its
    // place before the marker.
    while spans.len() > 1 {
        let last = spans.last_mut().expect("more than one span");
        let trimmed = last.content.trim_end().to_owned();
        if trimmed.is_empty() {
            spans.pop();
        } else {
            last.content = trimmed.into();
            break;
        }
    }
    spans.push(Span::from(marker.to_owned()));
    Line::from(spans).style(line.style)
}

// ---------------------------------------------------------------------
// The tab bar
// ---------------------------------------------------------------------

fn draw_tabs(frame: &mut Frame, state: &SessionState, area: Rect) {
    let block = Block::new().borders(Borders::BOTTOM);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(tab_line(state, inner.width as usize), inner);
}

/// The tabs as one line of `width` columns: every question with its mark,
/// then the review. When they do not fit, the tabs around the current one
/// are shown, with `←` and `→` where more follow.
fn tab_line(state: &SessionState, width: usize) -> Line<'static> {
    let states = state.question_states();
    let mut labels: Vec<String> = state
        .session()
        .questions
        .iter()
        .zip(&states)
        .map(|(question, question_state)| {
            let mark = if question_state.answer == AnswerState::Skipped {
                "☐"
            } else {
                "☒"
            };
            format!(
                "{mark} {}",
                question.header.as_deref().unwrap_or(&question.id)
            )
        })
        .collect();
    labels.push("✔ Review".to_owned());

    let widths: Vec<usize> = labels.iter().map(|label| columns(label) + 2).collect();
    let current = state.tab();
    // Grow the visible window around the current tab while it fits,
    // keeping two columns on each side for the arrows.
    let room = width.saturating_sub(4);
    let (mut first, mut last) = (current, current);
    let mut used = widths[current];
    loop {
        let grow_right = last + 1 < labels.len() && used + widths[last + 1] <= room;
        if grow_right {
            last += 1;
            used += widths[last];
        }
        let grow_left = first > 0 && used + widths[first - 1] <= room;
        if grow_left {
            first -= 1;
            used += widths[first];
        }
        if !grow_left && !grow_right {
            break;
        }
    }

    let mut spans = vec![Span::from(if first > 0 { "← " } else { "  " })];
    for (index, label) in labels.iter().enumerate().take(last + 1).skip(first) {
        let span = Span::from(format!(" {label} "));
        spans.push(if index == current {
            span.reversed().bold()
        } else {
            span
        });
    }
    if last + 1 < labels.len() {
        spans.push(Span::from(" →"));
    }
    Line::from(spans)
}

// ---------------------------------------------------------------------
// The current tab
// ---------------------------------------------------------------------

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

/// Draws the current tab into `area`: the question, or the review, at no
/// more than the reading width, and the context pane where
/// [`panes::choose`] puts it. Says what the pane showed, if one did.
fn draw_tab(frame: &mut Frame, state: &SessionState, area: Rect, view: &mut View) -> Option<Page> {
    let arrangement = state.has_context_area().then(|| {
        let arrangement = panes::choose(area.width, area.height, view.layout, view.shown);
        if arrangement != Arrangement::Hint {
            view.shown = Some(arrangement);
        }
        arrangement
    });

    let content_at = |width: u16| match state.question() {
        Some(_) => question_content(state, state.tab(), usize::from(width), Drawn::Live),
        None => review_content(state, usize::from(width)),
    };
    let question_rows = match arrangement {
        Some(Arrangement::Below) => view.question_height.need(state, area),
        _ => 0,
    };
    let (text_area, context_area) = panes::areas(area, arrangement, question_rows);
    let content = content_at(text_area.width);
    view.field_shape.clone_from(&content.shape);

    let pane = match context_area {
        Some(area) if arrangement == Some(Arrangement::Hint) => {
            let z = question_key(state, "z");
            frame.render_widget(context_hint(state.shown_context(), &z, area.width), area);
            None
        }
        Some(area) => draw_pane(frame, state, area, &mut view.viewers),
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
    pane
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

/// `line` behind `columns` blanks.
fn indented(line: Line<'static>, columns: usize) -> Line<'static> {
    let mut spans = vec![Span::from(" ".repeat(columns))];
    spans.extend(line.spans);
    Line::from(spans).style(line.style)
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

// ---------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------

/// Word-wraps `line` to `width` columns: the first line is indented by
/// `first`, the following ones by `rest`, as [`wrap_rows`] breaks it.
pub(super) fn wrap(
    line: Line<'static>,
    width: usize,
    first: usize,
    rest: usize,
) -> Vec<Line<'static>> {
    let glyphs: Vec<Glyph> = line
        .spans
        .iter()
        .flat_map(|span| {
            let style = line.style.patch(span.style);
            span.content
                .graphemes(true)
                .map(move |grapheme| Glyph::new(grapheme, style))
        })
        .collect();
    wrap_rows(&glyphs, width, first, rest)
        .into_iter()
        .map(|(indent, row)| styled_line(indent, &glyphs[row]))
        .collect()
}

/// Where `glyphs` break into rows of `width` columns: each row's indent
/// and the glyphs it takes, in order, the first row indented by `first`
/// and the following ones by `rest`. Every glyph lands on exactly one row.
///
/// A word is a run of non-blanks with the blanks after it, whatever
/// styles its glyphs carry, so a style change inside a word, such as the
/// text cursor, never splits it. A word longer than a row is broken where
/// the row ends.
pub(crate) fn wrap_rows(
    glyphs: &[Glyph],
    width: usize,
    first: usize,
    rest: usize,
) -> Vec<(usize, Range<usize>)> {
    let mut words: Vec<Range<usize>> = Vec::new();
    for (index, glyph) in glyphs.iter().enumerate() {
        let after_blank = words
            .last()
            .is_none_or(|word| glyphs[word.end - 1].is_blank());
        if words.is_empty() || (!glyph.is_blank() && after_blank) {
            words.push(index..index);
        }
        words.last_mut().expect("a word was started").end = index + 1;
    }

    let mut wrapped = Wrapped {
        rows: vec![(first, 0..0)],
        used: first,
        rest,
    };
    for word in words {
        let blanks = glyphs[word.clone()]
            .iter()
            .rev()
            .take_while(|glyph| glyph.is_blank())
            .count();
        let mut body = word.start..word.end - blanks;
        while !body.is_empty() {
            let indent = if wrapped.rows.len() == 1 { first } else { rest };
            if wrapped.used + Glyph::columns(&glyphs[body.clone()]) <= width {
                wrapped.push(glyphs, body.end);
                break;
            }
            if wrapped.used > indent {
                wrapped.break_row();
                continue;
            }
            // A word longer than a whole row: break it where the row ends.
            // A wide character that would cross the end starts the next
            // row whole, and every row takes at least one grapheme, so one
            // wider than the whole row still makes progress.
            let mut used = wrapped.used;
            let fits = glyphs[body.clone()]
                .iter()
                .take_while(|glyph| {
                    used += glyph.width;
                    used <= width
                })
                .count();
            let split = body.start + fits.max(1);
            wrapped.push(glyphs, split);
            if split < body.end {
                wrapped.break_row();
            }
            body = split..body.end;
        }
        // Blanks may hang past the end of a row, so the next row does not
        // start with them. A reversed blank is the exception, since it is
        // visible: the text cursor's cell or part of a highlighted head. It
        // starts the next row when this one is full.
        for blank in word.end - blanks..word.end {
            let visible = glyphs[blank]
                .style
                .add_modifier
                .contains(Modifier::REVERSED);
            if visible && wrapped.used >= width {
                wrapped.break_row();
            }
            wrapped.push(glyphs, blank + 1);
        }
    }
    wrapped.rows
}

/// One grapheme of a line to wrap, with its style and the columns it
/// takes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Glyph<'a> {
    pub(crate) text: &'a str,
    pub(crate) style: Style,
    pub(crate) width: usize,
}

impl<'a> Glyph<'a> {
    pub(crate) fn new(text: &'a str, style: Style) -> Self {
        Glyph {
            text,
            style,
            width: columns(text),
        }
    }

    fn is_blank(&self) -> bool {
        self.text == " "
    }

    /// The columns `glyphs` take together.
    fn columns(glyphs: &[Glyph]) -> usize {
        glyphs.iter().map(|glyph| glyph.width).sum()
    }
}

/// The rows `wrap_rows` builds: each one's indent and its glyphs.
struct Wrapped {
    rows: Vec<(usize, Range<usize>)>,
    /// The columns taken on the last row, its indent included.
    used: usize,
    rest: usize,
}

impl Wrapped {
    /// Extends the last row up to glyph `end`.
    fn push(&mut self, glyphs: &[Glyph], end: usize) {
        let (_, row) = self.rows.last_mut().expect("there is always a row");
        self.used += Glyph::columns(&glyphs[row.end..end]);
        row.end = end;
    }

    fn break_row(&mut self) {
        let end = self.rows.last().expect("there is always a row").1.end;
        self.rows.push((self.rest, end..end));
        self.used = self.rest;
    }
}

/// A row of `glyphs` behind `indent` blanks: the indent as a span of its
/// own, which `ending_in` keeps, then one span per run of a style.
pub(crate) fn styled_line(indent: usize, glyphs: &[Glyph]) -> Line<'static> {
    let mut runs: Vec<(String, Style)> = Vec::new();
    for glyph in glyphs {
        match runs.last_mut() {
            Some((text, style)) if *style == glyph.style => text.push_str(glyph.text),
            _ => runs.push((glyph.text.to_owned(), glyph.style)),
        }
    }
    let mut spans = vec![Span::from(" ".repeat(indent))];
    spans.extend(
        runs.into_iter()
            .map(|(text, style)| Span::styled(text, style)),
    );
    Line::from(spans)
}

// ---------------------------------------------------------------------
// Overlays and the key bar
// ---------------------------------------------------------------------

/// Every key, in the 13 lines the help has on the smallest screen.
const HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "move between the rows"),
    ("←/→, h/l", "previous and next question"),
    ("", "in a field, arrows leave it at its edges"),
    ("enter", "pick and go on; open or submit on the review"),
    (
        "space, 1-9",
        "toggle an option; pick or toggle one directly",
    ),
    ("n, ctrl-j", "write a note; a new line in any field"),
    ("esc", "stop typing in a field"),
    ("tab", "move into the context pane and back"),
    ("pgup/pgdn", "scroll the context pane, also ctrl-u/ctrl-d"),
    ("o, z", "open the image; show the context full screen"),
    ("L", "the list of waiting sessions"),
    ("q, ctrl-c", "quit, keeping the draft"),
    ("?, i", "this help; the whole intro"),
];

/// The keys of the focused context pane.
const PANE_HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "scroll by line"),
    ("←/→, h/l", "scroll sideways where lines are cut"),
    ("pgup/pgdn", "scroll by page, also ctrl-u/ctrl-d"),
    ("g/G", "to the top and the bottom, also home/end"),
    ("z", "show the context full screen and back"),
    ("o", "open an image in the system viewer"),
    ("tab/esc/q", "back to the question"),
    ("ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

/// The keys of the full-screen view. It goes back to where it was entered
/// from, the pane or the question, so its way out names no place.
const FULL_SCREEN_HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "scroll by line"),
    ("←/→, h/l", "scroll sideways where lines are cut"),
    ("pgup/pgdn", "scroll by page, also ctrl-u/ctrl-d"),
    ("g/G", "to the top and the bottom, also home/end"),
    ("o", "open an image in the system viewer"),
    ("z/tab/esc/q", "leave the full screen"),
    ("ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

/// The keys that work where the focus is: the full-screen view's, the
/// pane's while it has the focus, every key otherwise.
fn draw_help(frame: &mut Frame, area: Rect, state: &SessionState) {
    let (keys, title) = if state.full_screen() {
        (FULL_SCREEN_HELP, " Keys of the full-screen view ")
    } else if state.pane_focused() {
        (PANE_HELP, " Keys of the context pane ")
    } else {
        (HELP, " Keys ")
    };
    let height = u16::try_from(keys.len() + 2).expect("a short list");
    let area = center(area, 70, height);
    frame.render_widget(Clear, area);
    // `format!` pads by characters, and every key is one column per
    // character, so the longest key plus one blank sets the column.
    let key_column = keys
        .iter()
        .map(|(keys, _)| keys.chars().count() + 1)
        .max()
        .unwrap_or_default()
        .max(11);
    let lines: Vec<Line> = keys
        .iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::from(format!("{keys:<key_column$}")).bold(),
                Span::from(*what),
            ])
        })
        .collect();
    let block = Block::bordered()
        .title(title)
        .title_bottom(Line::from(" any key closes ").right_aligned());
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn draw_session_list(frame: &mut Frame, app: &App, cursor: usize, area: Rect) {
    let area = center(
        area,
        area.width.saturating_sub(4).min(80),
        app.sessions().len() as u16 + 2,
    );
    frame.render_widget(Clear, area);
    let lines: Vec<Line> = app
        .sessions()
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let session = state.session();
            let answered = state
                .question_states()
                .iter()
                .filter(|question| question.answer != AnswerState::Skipped)
                .count();
            let title = session
                .title
                .clone()
                .unwrap_or_else(|| state.id().to_owned());
            let pointer = if index == cursor { "❯ " } else { "  " };
            let line = Line::from(format!(
                "{pointer}{title}  ({answered}/{} answered)",
                session.questions.len()
            ));
            if index == cursor {
                line.reversed()
            } else {
                line
            }
        })
        .collect();
    let visible = area.height.saturating_sub(2) as usize;
    let scroll = (cursor + 1).saturating_sub(visible);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((scroll as u16, 0))
            .block(Block::bordered().title(" Waiting sessions ")),
        area,
    );
}

/// The keys worth showing right now, without `n` where no note is taken.
/// `pane` is what the context pane showed in this draw: where one shows,
/// `tab` leads into it, and while it has the focus its own keys show.
fn key_hints(
    state: &SessionState,
    list: bool,
    pane: Option<Page>,
) -> Vec<(&'static str, &'static str)> {
    if let Some(page) = pane.filter(|_| state.pane_focused() && !list && !state.help()) {
        return pane_key_hints(state, page);
    }
    let mut hints = all_key_hints(state, list);
    if state.question().is_some_and(|question| !question.note) {
        hints.retain(|(key, _)| *key != "n");
    }
    if pane.is_some() && !list && !state.help() {
        // Before the help, or last in a field, whose keys have no help.
        let at = hints
            .iter()
            .position(|(key, _)| *key == "?")
            .unwrap_or(hints.len());
        hints.insert(at, ("tab", "context"));
    }
    hints
}

/// The keys of the focused pane and the full-screen view, where `z` is
/// one of the keys that go back. Sideways and `o` only show where they do
/// something.
fn pane_key_hints(state: &SessionState, page: Page) -> Vec<(&'static str, &'static str)> {
    let mut hints = vec![("↑/↓", "scroll")];
    if page.columns.total > page.columns.shown {
        hints.push(("←/→", "sideways"));
    }
    hints.push(("pgup/pgdn", "page"));
    if !state.full_screen() {
        hints.push(("z", "full screen"));
    }
    if state.shown_image().is_some() {
        hints.push(("o", "open"));
    }
    hints.push(("?", "help"));
    hints.push(if state.full_screen() {
        ("z/esc/q", "back")
    } else {
        ("tab/esc/q", "back")
    });
    hints
}

fn all_key_hints(state: &SessionState, list: bool) -> Vec<(&'static str, &'static str)> {
    if list {
        return vec![("↑/↓", "move"), ("enter", "open"), ("esc", "close")];
    }
    if state.help() {
        return vec![("any key", "close")];
    }
    let row = state.rows().get(state.row()).copied();
    let kind = state.question().map(|question| question.kind);
    match (state.focus(), row) {
        (Focus::Note, _) => vec![("ctrl-j", "new line"), ("enter/esc", "done")],
        (Focus::Field, Some(Row::Answer)) => vec![
            ("ctrl-j", "new line"),
            ("enter", "next"),
            ("esc", "stop typing"),
        ],
        (Focus::Field, Some(Row::Note)) => vec![
            ("ctrl-j", "new line"),
            ("enter", "submit"),
            ("esc", "stop typing"),
        ],
        (Focus::Field, Some(Row::Reject)) => vec![
            ("ctrl-j", "new line"),
            ("enter", "reject"),
            ("esc", "stop typing"),
        ],
        (Focus::Field, _) => vec![
            ("ctrl-j", "new line"),
            ("enter", "pick"),
            ("esc", "stop typing"),
        ],
        (Focus::None, _) if state.on_review() => vec![
            ("↑/↓", "move"),
            ("enter", "open/submit"),
            ("←/→", "question"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (Focus::None, _) if kind == Some(Kind::Text) => vec![
            ("↑/↓", "type"),
            ("enter", "next"),
            ("←/→", "question"),
            ("n", "note"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (Focus::None, _) if kind == Some(Kind::Multi) => vec![
            ("↑/↓", "move"),
            ("space", "toggle"),
            ("enter", "next"),
            ("←/→", "question"),
            ("n", "note"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (Focus::None, _) => vec![
            ("↑/↓", "move"),
            ("enter", "pick"),
            ("←/→", "question"),
            ("n", "note"),
            ("?", "help"),
            ("q", "quit"),
        ],
    }
}

/// The key bar in `width` columns. Hints that do not fit whole are left
/// out rather than cut, the least important first (`hint_rank`), and the
/// rest keep their order; the help lists every key anyway.
fn key_line(keys: &[(&str, &str)], width: u16) -> Line<'static> {
    // Each hint is ` key ` and ` what  `; the blanks after the last one may
    // run past the edge.
    let hint_width = |(key, what): &(&str, &str)| columns(key) + columns(what) + 5;
    let mut kept: Vec<&(&str, &str)> = keys.iter().collect();
    loop {
        let used: usize = kept.iter().map(|hint| hint_width(hint)).sum();
        if used.saturating_sub(2) <= usize::from(width) {
            break;
        }
        // Among hints of the same rank, the one further right goes first.
        let least = (0..kept.len())
            .rev()
            .min_by_key(|&index| hint_rank(kept[index].0))
            .expect("no hints at all always fit");
        kept.remove(least);
    }

    let mut spans = Vec::new();
    for (key, what) in kept {
        spans.push(Span::from(format!(" {key} ")).add_modifier(Modifier::REVERSED));
        spans.push(Span::from(format!(" {what}  ")));
    }
    Line::from(spans)
}

/// How long a hint stays in a key bar too narrow for all of them; higher
/// stays longer. `?` and `q` stay longest, as the help names every other
/// key and quitting, or going back from the context pane, is what a lost
/// person looks for. The keys that act where the cursor is come next, then
/// switching questions, then the context pane's `tab`, and a note last,
/// which is optional on every question.
fn hint_rank(key: &str) -> u8 {
    match key {
        "?" => 6,
        "q" | "tab/esc/q" | "z/esc/q" => 5,
        "←/→" => 3,
        "tab" => 2,
        "n" => 1,
        _ => 4,
    }
}

/// A rectangle of `width`×`height` in the middle of `area`, never larger
/// than `area`.
fn center(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    area
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::format::{Session, result_answers};

    const EXAMPLE: &str = include_str!("../../../examples/sessions/alt-rework-batch.json");
    const RELEASE: &str = include_str!("../../../examples/sessions/release-checklist.json");

    fn app(sessions: &[(&str, &str)]) -> App {
        let mut app = App::default();
        for (id, json) in sessions {
            let session: Session = serde_json::from_str(json).expect("example parses");
            app.add(SessionState::new(*id, session, None));
        }
        app
    }

    fn keys(app: &mut App, keys: &str) {
        for c in keys.chars() {
            app.handle(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    fn special(app: &mut App, code: KeyCode) {
        app.handle(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn screen(app: &App, width: u16, height: u16) -> TestBackend {
        screen_of_queue(app, "default", width, height)
    }

    fn screen_of_queue(app: &App, queue: &str, width: u16, height: u16) -> TestBackend {
        screen_in(
            app,
            &mut View::new(queue, PaneLayout::Auto, Picker::halfblocks()),
            width,
            height,
        )
    }

    /// Hands `app` what the last draw with `view` showed of the pane, as the
    /// run loop does after each frame.
    fn hand_on_pane(app: &mut App, view: &View) {
        if let Some(page) = view.pane_page() {
            app.set_pane_page(page);
        }
    }

    /// The screen drawn with `view`, which keeps what it remembers for the
    /// next draw.
    fn screen_in(app: &App, view: &mut View, width: u16, height: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, app, view))
            .expect("draws");
        terminal.backend().clone()
    }

    fn title_line(backend: &TestBackend) -> String {
        visible_rows(backend)[0].trim_end().to_owned()
    }

    /// Each row of the screen as the terminal shows it: a wide character
    /// hides the cell after it.
    fn visible_rows(backend: &TestBackend) -> Vec<String> {
        let buffer = backend.buffer();
        (0..buffer.area.height)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < buffer.area.width {
                    let symbol = buffer[(x, y)].symbol();
                    row.push_str(symbol);
                    x += u16::try_from(columns(symbol).max(1)).expect("a cell is narrow");
                }
                row
            })
            .collect()
    }

    // -----------------------------------------------------------------
    // Sizes
    // -----------------------------------------------------------------

    #[test]
    fn the_intro_shows_under_the_title_as_far_as_there_is_room() {
        let long_intro = RELEASE.replace(
            "Three decisions before the release. Skip what you cannot decide `yet`.",
            "Three decisions before the release. Skip what you cannot decide `yet`. \
             The **channels** question takes several answers, and the highlights \
             line goes into the release notes as it is typed, so write it the way \
             it should read on the blog. Nothing is published before you submit.",
        );
        let app = app(&[("release", &long_intro)]);

        for (width, height) in [(60, 15), (80, 24)] {
            insta::assert_snapshot!(
                format!("intro_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    /// The release session with a title and an intro that run far past the
    /// reading width.
    fn long_header() -> App {
        let session = RELEASE
            .replace(
                r#""title": "Release 0.2.0""#,
                r#""title": "Release 0.2.0 of the queue watcher, the new session format and the reworked review tab with its note field and the layout that keeps questions readable""#,
            )
            .replace(
                "Three decisions before the release. Skip what you cannot decide `yet`.",
                &"Three decisions before the release, each with its own question. ".repeat(4),
            );
        app(&[("release", &session)])
    }

    #[test]
    fn the_header_wraps_at_the_reading_width_and_the_tab_bar_does_not() {
        let backend = screen(&long_header(), 160, 30);
        let lines = screen_lines(&backend);

        for (row, line) in lines.iter().enumerate().take(6).skip(1) {
            let used = line.trim_end().chars().count();
            assert!(used <= 101, "header row {row} ends at {used}:\n{backend}");
        }
        insta::assert_snapshot!("header_wrapped_160x30", backend);

        let questions: Vec<String> = (1..=20)
            .map(|n| format!(r#"{{"id": "question-{n}", "text": "?", "kind": "text"}}"#))
            .collect();
        let many_tabs = app(&[(
            "tabs",
            &format!(r#"{{"asqr": 2, "questions": [{}]}}"#, questions.join(",")),
        )]);
        let tab_bar = screen_lines(&screen(&many_tabs, 160, 30))[2].clone();
        assert!(
            tab_bar.trim_end().chars().count() > 150,
            "the tab bar uses the full width: {tab_bar}"
        );
    }

    #[test]
    fn a_long_title_takes_two_lines_or_one_on_short_terminals() {
        let app = long_header();
        let lines = screen_lines(&screen(&app, 80, 24));
        assert!(lines[2].trim_end().ends_with('…'), "{lines:#?}");
        assert!(lines[5].trim_end().ends_with("… i: more"), "{lines:#?}");

        let lines = screen_lines(&screen(&app, 80, 20));
        assert!(lines[1].trim_end().ends_with('…'), "{lines:#?}");
        assert!(lines[2].trim_end().ends_with("… i: more"), "{lines:#?}");
    }

    #[test]
    fn a_cut_line_keeps_its_styles_and_drops_the_blanks_before_the_marker() {
        let line = Line::from(vec![
            Span::from(" "),
            Span::from("bold").bold(),
            Span::from(" words   "),
        ]);
        let cut = ending_in(line, "…", 13);
        assert_eq!(
            cut.spans,
            [
                Span::from(" "),
                Span::from("bold").bold(),
                Span::from(" words"),
                Span::from("…"),
            ]
        );

        let blank = ending_in(Line::from(vec![Span::from(" "), Span::from("   ")]), "…", 4);
        assert_eq!(blank.spans, [Span::from(" "), Span::from("…")]);
    }

    /// A session whose intro takes far more lines than a 60×15 screen.
    fn longer_intro() -> App {
        let intro = (1..=8)
            .map(|round| {
                format!(
                    "- Round {round} asked about the **queue watcher** and the review tab, \
                     and the answers went into the plan."
                )
            })
            .collect::<Vec<_>>()
            .join("\\n");
        app(&[(
            "release",
            &RELEASE.replace(
                "Three decisions before the release. Skip what you cannot decide `yet`.",
                &intro,
            ),
        )])
    }

    /// Draws `app` with `view`, then hands the app what the intro overlay
    /// showed, as the running app does after every draw.
    fn drawn(app: &mut App, view: &mut View, width: u16, height: u16) -> TestBackend {
        let backend = screen_in(app, view, width, height);
        app.set_intro_page(view.intro_page());
        app.set_field_shape(view.field_shape());
        backend
    }

    #[test]
    fn i_shows_the_whole_intro_and_scrolls_it_on_a_small_screen() {
        let mut app = longer_intro();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        keys(&mut app, "i");
        insta::assert_snapshot!("intro_overlay_top", drawn(&mut app, &mut view, 60, 15));

        keys(&mut app, "jjj");
        insta::assert_snapshot!("intro_overlay_scrolled", drawn(&mut app, &mut view, 60, 15));

        keys(&mut app, "G");
        insta::assert_snapshot!("intro_overlay_end", drawn(&mut app, &mut view, 60, 15));

        keys(&mut app, "i");
        let backend = drawn(&mut app, &mut view, 60, 15);
        assert!(!backend.to_string().contains("Intro"), "{backend}");
        assert_eq!(view.intro_page(), Extent::default());
    }

    #[test]
    fn an_intro_that_fits_shows_without_a_position() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "i");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_question_adapts_to_every_terminal_size() {
        let app = app(&[("release", RELEASE)]);
        for (width, height) in [(60, 15), (80, 24), (120, 40), (200, 50)] {
            insta::assert_snapshot!(
                format!("question_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    #[test]
    fn the_review_shows_labels_and_text_cut_to_the_line() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "223");
        special(&mut app, KeyCode::Enter);
        keys(
            &mut app,
            "Queue scans are faster and the review shows what you picked",
        );
        special(&mut app, KeyCode::Enter);

        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    #[test]
    fn the_review_adapts_to_every_terminal_size() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");
        for (width, height) in [(60, 15), (100, 30)] {
            insta::assert_snapshot!(
                format!("review_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    #[test]
    fn a_small_screen_keeps_the_cursor_row_in_view() {
        let options: Vec<String> = (1..=12)
            .map(|n| format!(r#"{{"id": "o{n}", "label": "Option {n}", "description": "What option {n} means."}}"#))
            .collect();
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Which one?", "kind": "single", "custom": true,
                "options": [{}]}}]}}"#,
            options.join(",")
        );
        let mut app = app(&[("many", &json)]);
        for _ in 0..12 {
            special(&mut app, KeyCode::Down);
        }
        insta::assert_snapshot!(screen(&app, 60, 15));

        // A wrapped own answer counts with all its lines.
        keys(&mut app, LONG_OWN_ANSWER);
        insta::assert_snapshot!("cursor_row_wrapped_typed", screen(&app, 60, 15));
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!("cursor_row_wrapped_left", screen(&app, 60, 15));
    }

    #[test]
    fn a_long_session_scrolls_the_tab_bar_to_the_current_tab() {
        let questions: Vec<String> = (1..=40)
            .map(|n| format!(r#"{{"id": "q{n}", "text": "Question {n}?", "kind": "single", "options": [{{"id": "a", "label": "A"}}]}}"#))
            .collect();
        let json = format!(r#"{{"asqr": 2, "questions": [{}]}}"#, questions.join(","));
        let mut app = app(&[("long", &json)]);
        for _ in 0..30 {
            special(&mut app, KeyCode::Right);
        }
        insta::assert_snapshot!(screen(&app, 80, 16));
    }

    #[test]
    fn narrow_screens_put_descriptions_below_the_label() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Which approach?", "kind": "single",
            "options": [
                {"id": "a", "label": "Rewrite the parser from scratch", "description": "Cleanest result, takes the longest."},
                {"id": "b", "label": "Patch the tokenizer", "description": "Quick, but the grammar stays odd."}]}]}"#;
        let app = app(&[("approach", json)]);

        insta::assert_snapshot!("descriptions_below_60x15", screen(&app, 60, 15));
        insta::assert_snapshot!("descriptions_beside_120x15", screen(&app, 120, 15));
    }

    #[test]
    fn an_option_with_an_empty_description_still_shows() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Which one?", "kind": "single",
            "options": [
                {"id": "a", "label": "Alpha option", "description": ""},
                {"id": "b", "label": "Beta option", "description": "   "},
                {"id": "c", "label": "Gamma option", "description": "Has a description."}]}]}"#;
        let app = app(&[("probe", json)]);

        let text = format!("{:?}", screen(&app, 120, 20));
        assert!(text.contains("Gamma option"), "control option shows");
        assert!(
            text.contains("Alpha option"),
            "option with empty description shows"
        );
        assert!(
            text.contains("Beta option"),
            "option with blank description shows"
        );
    }

    // -----------------------------------------------------------------
    // Markdown in the intro, question texts and option descriptions
    // -----------------------------------------------------------------

    /// A session with `intro` and one question of `text`, whose first
    /// option is labelled `label` and described by `description`.
    fn markdown_session(intro: &str, text: &str, label: &str, description: &str) -> App {
        let json = serde_json::json!({
            "asqr": 2,
            "title": "Markdown",
            "intro": intro,
            "questions": [{"id": "q", "text": text, "kind": "single", "options": [
                {"id": "a", "label": label, "description": description},
                {"id": "b", "label": "Beta"}
            ]}]
        });
        app(&[("markdown", &json.to_string())])
    }

    /// The column and row of the first cell where `text` starts, searching
    /// the rows from `from` on. Every cell in these tests holds one
    /// character, so a character's index in a row is its column.
    fn find_from(backend: &TestBackend, from: u16, text: &str) -> (u16, u16) {
        let lines = screen_lines(backend);
        for (y, line) in lines.iter().enumerate().skip(usize::from(from)) {
            if let Some(start) = line.find(text) {
                let x = line[..start].chars().count();
                return (
                    u16::try_from(x).expect("a column on the screen"),
                    u16::try_from(y).expect("a row on the screen"),
                );
            }
        }
        panic!("{text:?} is not on the screen:\n{backend}");
    }

    fn find(backend: &TestBackend, text: &str) -> (u16, u16) {
        find_from(backend, 0, text)
    }

    const PLAN: &str = "## Plan\n\nRun `cargo test` **now**, see [docs](https://example.com).\n\n> Quoted\n\n```rust\nlet x = 1;\n```";

    #[test]
    fn a_question_text_renders_full_markdown_in_patines_styles() {
        let backend = screen(&markdown_session("Intro", PLAN, "Alpha", ""), 100, 30);
        let buffer = backend.buffer();
        let text = backend.to_string();

        for source in ["##", "**", "```", "[docs]", "> Quoted"] {
            assert!(!text.contains(source), "{source} shows:\n{backend}");
        }
        let heading = find(&backend, "Plan");
        assert!(
            buffer[heading]
                .modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED),
            "a level-two heading"
        );
        let code = find(&backend, "cargo test");
        assert_eq!(buffer[code].fg, Color::Indexed(3), "inline code in yellow");
        assert!(
            !buffer[code].modifier.contains(Modifier::REVERSED),
            "inline code is not reversed"
        );
        assert!(
            buffer[find(&backend, "now")]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            buffer[find(&backend, "docs")]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        assert!(
            buffer[find(&backend, "(https://example.com)")]
                .modifier
                .contains(Modifier::DIM)
        );
        let (x, y) = find(&backend, "│ Quoted");
        assert!(buffer[(x, y)].modifier.contains(Modifier::DIM), "the bar");
        assert!(buffer[(x + 2, y)].modifier.contains(Modifier::ITALIC));
        let (x, y) = find(&backend, "let x = 1;");
        assert_eq!(x, 3, "the block is indented by two after the margin");
        assert_eq!(
            buffer[(x, y)].fg,
            Color::Indexed(5),
            "a highlighted keyword"
        );
    }

    #[test]
    fn a_wide_code_line_in_a_question_wraps_behind_a_marker() {
        let code = "let total = first + second + third + fourth + fifth + sixth + seventh;";
        let backend = screen(
            &markdown_session("Intro", &format!("```\n{code}\n```"), "Alpha", ""),
            60,
            24,
        );
        let buffer = backend.buffer();

        let (x, y) = find(&backend, "let total");
        assert_eq!(x, 3);
        let first = &screen_lines(&backend)[usize::from(y)];
        let second = &screen_lines(&backend)[usize::from(y) + 1];
        assert!(second.starts_with("   ↪ "), "{backend}");
        assert!(
            buffer[(3, y + 1)].modifier.contains(Modifier::DIM),
            "the marker"
        );
        let shown = format!(
            "{}{}",
            first[3..].trim_end(),
            second["   ↪ ".len()..].trim_end()
        );
        assert_eq!(shown, code, "nothing is cut:\n{backend}");
        assert!(!backend.to_string().contains('›'), "{backend}");
    }

    #[test]
    fn the_intro_is_dim_over_its_markdown_styles() {
        let app = markdown_session("Round **two** of `layout`.", "Which?", "Alpha", "");
        let backend = screen(&app, 80, 24);
        let buffer = backend.buffer();

        let two = find(&backend, "two");
        assert!(
            buffer[two]
                .modifier
                .contains(Modifier::BOLD | Modifier::DIM),
            "{backend}"
        );
        let layout = find(&backend, "layout");
        assert_eq!(buffer[layout].fg, Color::Indexed(3));
        assert!(buffer[layout].modifier.contains(Modifier::DIM));
        assert!(!backend.to_string().contains('`'), "{backend}");
    }

    #[test]
    fn the_intro_overlay_shows_the_markdown_styles_without_dim() {
        let mut app = markdown_session("Round **two** of `layout`.", "Which?", "Alpha", "");
        keys(&mut app, "i");
        let backend = screen(&app, 80, 24);
        let buffer = backend.buffer();

        let (_, y) = find(&backend, "│ Round two");
        let two = find_from(&backend, y, "two");
        assert!(buffer[two].modifier.contains(Modifier::BOLD), "{backend}");
        assert!(!buffer[two].modifier.contains(Modifier::DIM), "{backend}");
        let (layout, _) = find_from(&backend, y, "layout");
        assert_eq!(buffer[(layout, y)].fg, Color::Indexed(3));
    }

    const STEPS: &str = "Keeps **all** of it:\n\n- `first` step\n- second step";

    #[test]
    fn a_description_beside_its_label_renders_markdown_under_its_own_start() {
        let backend = screen(
            &markdown_session("Intro", "Which?", "Alpha", STEPS),
            120,
            20,
        );
        let buffer = backend.buffer();
        let lines = screen_lines(&backend);

        // The margin, then "❯ ( ) 1. Alpha" and two blanks.
        let (x, y) = find(&backend, "Keeps all of it:");
        assert_eq!(x, 17, "{backend}");
        assert!(
            lines[usize::from(y)].contains("Alpha"),
            "on the label's row"
        );
        assert!(
            buffer[find(&backend, "all")]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(find(&backend, "• first step"), (17, y + 2), "{backend}");
        assert_eq!(find(&backend, "• second step"), (17, y + 3), "{backend}");
        assert_eq!(buffer[(19, y + 2)].fg, Color::Indexed(3), "inline code");
        assert!(lines[usize::from(y) + 4].contains("Beta"), "{backend}");
    }

    #[test]
    fn a_description_below_its_label_renders_markdown_from_column_six() {
        let label = "Rewrite the parser from scratch, every part of it";
        let backend = screen(&markdown_session("Intro", "Which?", label, STEPS), 60, 24);

        let (_, label_row) = find(&backend, label);
        assert_eq!(
            find(&backend, "Keeps all of it:"),
            (7, label_row + 1),
            "{backend}"
        );
        assert_eq!(
            find(&backend, "• first step"),
            (7, label_row + 3),
            "{backend}"
        );
    }

    #[test]
    fn a_wide_code_line_in_a_description_wraps_inside_the_description_column() {
        let description = format!("```\n{}\n```", "word ".repeat(30));
        let backend = screen(
            &markdown_session("Intro", "Which?", "Alpha", &description),
            120,
            20,
        );

        let (x, y) = find(&backend, "word word");
        assert_eq!(x, 19, "the block's indent inside the description column");
        assert_eq!(find_from(&backend, y + 1, "↪"), (19, y + 1), "{backend}");
        assert!(!backend.to_string().contains('›'), "{backend}");
    }

    #[test]
    fn single_newlines_break_and_raw_html_shows_as_written() {
        let text = "First line\nsecond line, which returns a Vec<String>\n\n<details>\nmore\n</details>\n\n- an item\n  that goes on";
        let description = "Short.\nKeeps <b>tags</b>.";
        let app = markdown_session("One\ntwo", text, "Alpha", description);

        insta::assert_snapshot!("newlines_and_html_80x24", screen(&app, 80, 24));
    }

    #[test]
    fn a_question_with_an_image_placeholder_at_60_columns() {
        insta::assert_snapshot!(screen(&app(&[("batch", EXAMPLE)]), 60, 24));
    }

    #[test]
    fn a_small_terminal_says_what_it_needs() {
        insta::assert_snapshot!(screen(&App::default(), 40, 10));
    }

    #[test]
    fn an_empty_queue_says_so() {
        insta::assert_snapshot!(screen(&App::default(), 80, 20));
    }

    #[test]
    fn a_long_queue_path_gives_way_to_the_count() {
        let path = "/private/tmp/some/deeply/nested/scratch/directory/of/a/session/queue";
        let app = App::default();

        assert_eq!(
            title_line(&screen_of_queue(&app, path, 60, 15)),
            " asqr · queue: …/of/a/session/queue · 0 sessions waiting"
        );
        assert_eq!(
            title_line(&screen_of_queue(&app, &"x".repeat(80), 60, 15)),
            format!(" asqr · queue: …{} · 0 sessions waiting", "x".repeat(23)),
            "a name without separators is cut anywhere"
        );
        assert_eq!(
            title_line(&screen_of_queue(&app, "default", 60, 15)),
            " asqr · queue: default · 0 sessions waiting",
            "a title that fits stays as it is"
        );
    }

    #[test]
    fn an_empty_queue_still_shows_notices() {
        let mut app = App::default();
        app.notice("broken is invalid and was answered with an error: questions[0].min");

        insta::assert_snapshot!(screen(&app, 80, 20));
    }

    // -----------------------------------------------------------------
    // Fields
    // -----------------------------------------------------------------

    /// An own answer for the release session's channels question that
    /// needs several lines in every supported width.
    const LONG_OWN_ANSWER: &str = "a mailing list, the release channel, the team chat \
        and a short post on the project page, written by whoever has the time on the day";

    /// The release session with `LONG_OWN_ANSWER` typed into the own answer
    /// of its channels question, the field still focused.
    fn typed_own_answer() -> App {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ljjj");
        keys(&mut app, LONG_OWN_ANSWER);
        app
    }

    #[test]
    fn the_own_answer_wraps_under_its_start_while_typed() {
        let app = typed_own_answer();
        for (width, height) in [(60, 15), (80, 24), (120, 24)] {
            insta::assert_snapshot!(
                format!("own_answer_typed_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    #[test]
    fn a_left_own_answer_shows_completely_on_wrapped_lines() {
        let mut app = typed_own_answer();
        special(&mut app, KeyCode::Up);
        for (width, height) in [(60, 15), (80, 24), (120, 24)] {
            insta::assert_snapshot!(
                format!("own_answer_left_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    /// The reversed cells in the tab's area, which lies between the tab bar
    /// and the status line: the text cursor, or a highlighted row.
    fn reversed_cells_in_tab(backend: &TestBackend) -> Vec<(u16, u16, String)> {
        let buffer = backend.buffer();
        (5..buffer.area.height - 2)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].modifier.contains(Modifier::REVERSED))
            .map(|(x, y)| (x, y, buffer[(x, y)].symbol().to_owned()))
            .collect()
    }

    #[test]
    fn the_text_cursor_shows_on_its_wrapped_line() {
        let mut app = typed_own_answer();
        // Back to the `c` of "channel", the first word that does not fit
        // on the first line at 60 columns.
        let channel = LONG_OWN_ANSWER.find("channel").expect("the word is there");
        for _ in channel..LONG_OWN_ANSWER.chars().count() {
            special(&mut app, KeyCode::Left);
        }
        let backend = screen(&app, 60, 24);
        let buffer = backend.buffer();

        let cursor = reversed_cells_in_tab(&backend);
        assert_eq!(cursor.len(), 1, "only the cursor is reversed:\n{backend}");
        let (x, y, symbol) = &cursor[0];
        assert_eq!(symbol, "c", "{backend}");
        // The text starts after the margin and the 24 columns of
        // "❯ [x] ✎ Somewhere else: ", and the second line under it.
        assert_eq!(*x, 25, "{backend}");
        assert_eq!(buffer[(*x, y - 1)].symbol(), "a", "{backend}");
    }

    #[test]
    fn up_and_down_move_the_text_cursor_through_the_rows_on_screen() {
        // The text fills its last row at 60 columns, so the cursor after it
        // shows on a row of its own.
        let mut app = typed_own_answer();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        let mut cursor = |app: &mut App| {
            let backend = drawn(app, &mut view, 60, 24);
            let cells = reversed_cells_in_tab(&backend);
            assert_eq!(cells.len(), 1, "only the cursor is reversed:\n{backend}");
            (cells[0].0, cells[0].1)
        };
        let (x, y) = cursor(&mut app);

        special(&mut app, KeyCode::Up);
        let (above, row_above) = cursor(&mut app);
        assert_eq!(row_above, y - 1, "one row up, still in the field");
        assert!(above <= x, "at the column it was in, or before");
        special(&mut app, KeyCode::Down);

        assert_eq!(cursor(&mut app), (x, y), "back where it was");
    }

    /// A single question whose own answer has `label` and holds the
    /// typed `LONG_OWN_ANSWER`, the field still focused.
    fn own_answer_labelled(label: &str) -> App {
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Name it", "kind": "single",
                "custom": {{"label": {label:?}}}, "options": [{{"id": "a", "label": "asqr"}}]}}]}}"#
        );
        let mut app = app(&[("name", &json)]);
        keys(&mut app, "j");
        keys(&mut app, LONG_OWN_ANSWER);
        app
    }

    fn screen_lines(backend: &TestBackend) -> Vec<String> {
        let buffer = backend.buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_head_leaving_less_than_20_columns_wraps_the_text_under_the_pointer() {
        // The head is the pointer, the mark and `✎ ` (8 columns), the
        // label and `: `; the text area of a 60-column screen is 58 wide.
        for (label_length, indent) in [(28, 38), (29, 4)] {
            let backend = screen(&own_answer_labelled(&"x".repeat(label_length)), 60, 24);
            let lines = screen_lines(&backend);

            let head = lines
                .iter()
                .position(|line| line.contains('✎'))
                .expect("the own answer is on screen");
            let wrapped = &lines[head + 1];
            let leading = wrapped.chars().take_while(|c| *c == ' ').count();
            assert_eq!(
                leading,
                1 + indent,
                "a label of {label_length} wraps after {indent}:\n{backend}"
            );
        }
    }

    #[test]
    fn a_long_own_answer_label_keeps_the_text_readable() {
        let app = own_answer_labelled("Your own suggestion for the name of it");
        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    /// The release session on its review, the cursor on `row` below
    /// Submit: 1 for the session note, 2 for Reject.
    fn review_row_below_submit(row: usize) -> App {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");
        for _ in 0..row {
            special(&mut app, KeyCode::Down);
        }
        app
    }

    #[test]
    fn a_long_session_note_wraps_under_its_start() {
        let mut app = review_row_below_submit(1);
        keys(
            &mut app,
            "fine to ship once the changelog names the new format and the skill is updated",
        );
        insta::assert_snapshot!("session_note_typed_60x15", screen(&app, 60, 15));

        special(&mut app, KeyCode::Up);
        insta::assert_snapshot!("session_note_left_60x24", screen(&app, 60, 24));
    }

    #[test]
    fn a_long_reject_reason_wraps_under_its_start() {
        let mut app = review_row_below_submit(2);
        keys(
            &mut app,
            "the release waits for the security fix, which lands next week at the earliest",
        );
        insta::assert_snapshot!("reject_reason_typed_60x15", screen(&app, 60, 15));

        // Up past the session note, which would take the focus, to Submit.
        special(&mut app, KeyCode::Up);
        special(&mut app, KeyCode::Up);
        insta::assert_snapshot!("reject_reason_left_60x24", screen(&app, 60, 24));
    }

    #[test]
    fn the_session_note_and_the_reject_reason_show_their_lines() {
        let mut app = review_row_below_submit(1);
        keys(&mut app, "fine to ship");
        ctrl_j(&mut app);
        keys(&mut app, "once the changelog names the new format");
        special(&mut app, KeyCode::Down);
        keys(&mut app, "stale");
        ctrl_j(&mut app);
        keys(&mut app, "ask again");
        insta::assert_snapshot!(screen(&app, 60, 24));
    }

    #[test]
    fn line_breaks_in_an_own_answer_show_as_marks_in_the_review() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ljjjblog");
        ctrl_j(&mut app);
        keys(&mut app, "post");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");

        let backend = screen(&app, 80, 24);

        find(&backend, "\"blog ⏎ post\"");
    }

    #[test]
    fn a_text_answer_shows_its_lines_and_the_counter() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        keys(&mut app, "Ship the queue watcher");
        app.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        keys(&mut app, "and the new format.");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_note_is_written_in_its_line() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "nafter the weekend");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    /// The colour of the counter: the last coloured cell of the screen.
    fn counter_colour(app: &App) -> Color {
        let backend = screen(app, 80, 24);
        let buffer = backend.buffer();
        let area = buffer.area;
        (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| (x, y)))
            .map(|position| buffer[position].fg)
            .rfind(|colour| matches!(colour, Color::Green | Color::Yellow | Color::Red))
            .expect("the counter is on screen")
    }

    #[test]
    fn the_counter_colour_follows_the_length() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "lla");
        assert_eq!(counter_colour(&app), Color::Green);

        keys(&mut app, &"b".repeat(125));
        assert_eq!(
            counter_colour(&app),
            Color::Yellow,
            "above the target of 120"
        );

        keys(&mut app, &"c".repeat(20));
        assert_eq!(counter_colour(&app), Color::Red, "above warn at 140");
    }

    // -----------------------------------------------------------------
    // Overlays and feedback
    // -----------------------------------------------------------------

    #[test]
    fn the_help_fits_the_smallest_screen() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "?");
        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    #[test]
    fn the_session_list_shows_every_waiting_session() {
        let mut app = app(&[("batch", EXAMPLE), ("release", RELEASE)]);
        keys(&mut app, "Lj");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn messages_and_notices_show_in_the_status_line() {
        // With `max` 2 and the default ticked, the third pick is refused.
        let mut app = app(&[("release", &RELEASE.replace(r#""max": 3"#, r#""max": 2"#))]);
        keys(&mut app, "l23");
        insta::assert_snapshot!("message", screen(&app, 80, 24));

        app.notice("batch-01 collided with an unread result and was archived");
        insta::assert_snapshot!("notice", screen(&app, 80, 24));
    }

    // -----------------------------------------------------------------
    // Images
    // -----------------------------------------------------------------

    /// A session with one question showing `image`.
    fn with_image(image: &str) -> App {
        app(&[(
            "pictured",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Which one?", "kind": "single",
                    "context": [{{"type": "image", "path": {image:?}}}], "options": [{{"id": "a", "label": "This one"}}]}}]}}"#
            ),
        )])
    }

    /// A two-colour PNG, so the halfblock rendering is visible.
    fn png() -> tempfile::NamedTempFile {
        png_of(160)
    }

    /// A square two-colour PNG of `size` pixels.
    fn png_of(size: u32) -> tempfile::NamedTempFile {
        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        let image = image::RgbImage::from_fn(size, size, |x, _| {
            if x < size / 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        image.save(file.path()).expect("png is written");
        file
    }

    /// The columns and rows of cells the test image painted: its red and
    /// blue halves show as background colours.
    fn image_cells(
        backend: &TestBackend,
    ) -> (std::ops::RangeInclusive<u16>, std::ops::RangeInclusive<u16>) {
        let buffer = backend.buffer();
        let painted: Vec<(u16, u16)> = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| {
                let cell = &buffer[position];
                [cell.bg, cell.fg]
                    .iter()
                    .any(|colour| matches!(colour, Color::Rgb(255, 0, 0) | Color::Rgb(0, 0, 255)))
            })
            .collect();
        assert!(!painted.is_empty(), "the image is drawn");
        let xs = painted.iter().map(|(x, _)| *x);
        let ys = painted.iter().map(|(_, y)| *y);
        (
            xs.clone().min().expect("cells")..=xs.max().expect("cells"),
            ys.clone().min().expect("cells")..=ys.max().expect("cells"),
        )
    }

    #[test]
    fn a_wide_screen_shows_the_image_beside_the_question() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        let (columns, rows) = image_cells(&screen(&app, 140, 20));

        // The 138 columns inside the margins split into 68 for the
        // question, the separator and the rest for the image.
        assert_eq!(
            *columns.start(),
            70,
            "right of the question text: {columns:?}"
        );
        assert_eq!(*rows.start(), 5, "under the pane's header: {rows:?}");
    }

    #[test]
    fn a_small_image_grows_to_fill_the_pane() {
        // The 160 pixels of the test image take 16 columns at the half
        // blocks' cell size, far less than the pane has.
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        let (columns, rows) = image_cells(&screen(&app, 140, 20));

        assert_eq!(rows, 5..=17, "every row of the pane's body");
        assert!(columns.end() - columns.start() > 16, "{columns:?}");
    }

    /// A session with one question showing `image` at its natural size.
    fn with_image_kept_small(image: &str) -> App {
        app(&[(
            "pictured",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Which one?", "kind": "single",
                    "context": [{{"type": "image", "path": {image:?}, "enlarge": false}}],
                    "options": [{{"id": "a", "label": "This one"}}]}}]}}"#
            ),
        )])
    }

    #[test]
    fn an_image_that_does_not_enlarge_keeps_its_size_in_the_pane_and_full_screen() {
        // The 160 pixels take 16 columns and 8 rows at the half blocks'
        // cell size of 10×20 pixels.
        let file = png();
        let mut app = with_image_kept_small(file.path().to_str().expect("UTF-8 path"));

        let (columns, rows) = image_cells(&screen(&app, 140, 20));
        assert_eq!((columns, rows), (70..=85, 5..=12), "in the pane");

        keys(&mut app, "z");
        let (columns, rows) = image_cells(&screen(&app, 140, 20));
        assert_eq!(
            (columns.end() - columns.start(), rows.end() - rows.start()),
            (15, 7),
            "full screen too"
        );
    }

    #[test]
    fn an_image_that_does_not_enlarge_still_shrinks_to_fit() {
        let file = png_of(2000);
        let app = with_image_kept_small(file.path().to_str().expect("UTF-8 path"));

        let (_, rows) = image_cells(&screen(&app, 140, 20));

        assert_eq!(rows, 5..=17, "the pane's body, no more");
    }

    /// With the Kitty protocol the terminal draws the image into the cells
    /// its placement names. They must be the cells asqr fills with
    /// placeholders: left to the terminal, it derives them from its own cell
    /// size, which differs from the one asqr resized the image with after a
    /// font size change or in a second tmux client, and crops the image.
    #[test]
    fn a_kitty_image_is_placed_over_exactly_its_placeholder_cells() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
        let mut view = View::new("q", PaneLayout::Auto, picker);
        let mut terminal = Terminal::new(TestBackend::new(140, 24)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app, &mut view))
            .expect("draws");

        let buffer = terminal.backend().buffer();
        let placeholders: Vec<(u16, u16)> = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].symbol().contains('\u{10EEEE}'))
            .collect();
        let columns = placeholders
            .iter()
            .map(|&(x, _)| x)
            .collect::<BTreeSet<_>>();
        let rows = placeholders
            .iter()
            .map(|&(_, y)| y)
            .collect::<BTreeSet<_>>();
        let transmission = placeholders
            .iter()
            .map(|&position| buffer[position].symbol())
            .find(|symbol| symbol.contains("\x1b_G"))
            .expect("the first draw sends the image");

        let placement = format!("c={},r={}", columns.len(), rows.len());
        assert!(
            transmission.contains(&placement),
            "the placement names {placement}"
        );
    }

    #[test]
    fn a_narrow_screen_shows_the_image_below_the_question() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        let (_, rows) = image_cells(&screen(&app, 80, 24));

        // The question takes the six rows it needs, from row 4 on, and the
        // pane's header the next one.
        assert_eq!(*rows.start(), 11, "below the question text: {rows:?}");
    }

    #[test]
    fn the_option_under_the_cursor_shows_its_image_and_keeps_the_room() {
        let file = png();
        let long =
            "a description long enough to reach the right edge of a wide terminal ".repeat(3);
        let mut app = app(&[(
            "browsing",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Which one?", "kind": "single",
                    "options": [{{"id": "a", "label": "Pictured", "context": [{{"type": "image", "path": {image:?}}}]}},
                                {{"id": "b", "label": "Plain", "description": {long:?}}}]}}]}}"#,
                image = file.path().to_str().expect("UTF-8 path"),
            ),
        )]);

        let (columns, _) = image_cells(&screen(&app, 140, 20));
        assert_eq!(*columns.start(), 70, "beside the options: {columns:?}");

        keys(&mut app, "j");
        let backend = screen(&app, 140, 20);
        let buffer = backend.buffer();
        let right_side_used = (4..buffer.area.height - 2)
            .flat_map(|y| (69..buffer.area.width).map(move |x| (x, y)))
            .any(|position| buffer[position].symbol() != " ");
        assert!(
            !right_side_used,
            "the text keeps its width while the area stays empty:\n{}",
            backend
        );
    }

    #[test]
    fn z_shows_the_image_full_screen() {
        let file = png();
        let mut app = with_image(file.path().to_str().expect("UTF-8 path"));
        keys(&mut app, "z");

        let (columns, rows) = image_cells(&screen(&app, 80, 20));

        assert!(
            *columns.start() < 30,
            "the whole width is used: {columns:?}"
        );
        assert!(
            rows.end() - rows.start() >= 10,
            "it fills the height: {rows:?}"
        );
    }

    #[test]
    fn a_missing_or_unreadable_image_shows_a_placeholder() {
        insta::assert_snapshot!(
            "missing_image",
            screen(&with_image("/nonexistent/x.png"), 80, 24)
        );

        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        std::fs::write(file.path(), "not a picture").expect("written");
        let text = screen(&with_image(file.path().to_str().expect("UTF-8")), 80, 24).to_string();
        assert!(text.contains("image not shown"), "{text}");
    }

    #[test]
    fn answers_show_on_their_questions_when_coming_back() {
        let mut app = app(&[("release", RELEASE)]);
        // Pick "Publish now", toggle nothing, type an own answer that does
        // not fit its line, then write a text answer and a note.
        keys(
            &mut app,
            "1jjjan own answer much longer than the rest of this line has room",
        );
        special(&mut app, KeyCode::Up);
        insta::assert_snapshot!("own_answer_unfocused_60x15", screen(&app, 60, 15));

        keys(&mut app, "l");
        keys(&mut app, "done");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "nfirst");
        app.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        keys(&mut app, "second");
        special(&mut app, KeyCode::Esc);
        // Leaving the note returns to the answer field; esc leaves it too.
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!("text_answer_unfocused", screen(&app, 80, 24));

        keys(&mut app, "hh");
        insta::assert_snapshot!("single_picked", screen(&app, 80, 24));
    }

    #[test]
    fn a_left_own_answer_row_stays_highlighted_without_notes() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Name it", "kind": "single",
            "note": false, "custom": true, "options": [{"id": "a", "label": "asqr"}]}]}"#;
        let mut app = app(&[("name", json)]);
        keys(&mut app, "jmine");
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    #[test]
    fn an_own_answer_of_blanks_is_no_answer_anywhere() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Name it", "kind": "single",
            "custom": true, "options": [{"id": "a", "label": "asqr"}]}]}"#;
        for (name, blank) in [("blanks", "  "), ("a line break", "\n")] {
            let mut app = app(&[("name", json)]);
            keys(&mut app, "j");
            app.paste(blank);

            let state = app.active().expect("the session");
            assert_eq!(
                state.answers()[0].custom.as_deref(),
                Some(blank),
                "{name}: the draft keeps the text as typed"
            );
            let backend = screen(&app, 60, 15);
            let rows = visible_rows(&backend);
            let own = rows
                .iter()
                .find(|row| row.contains('✎'))
                .expect("the own answer shows");
            assert!(own.contains("( ) ✎"), "{name}: not chosen\n{backend}");
            assert!(rows[2].contains("☐ q"), "{name}: the tab\n{backend}");
            assert_eq!(state.counts().answered, 0, "{name}");
            assert_eq!(state.counts().skipped, 1, "{name}");
            assert!(
                result_answers(state.session(), &state.answers())[0].skipped,
                "{name}"
            );
        }
    }

    #[test]
    fn an_empty_text_answer_without_focus_shows_a_dash() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn the_dash_of_an_empty_field_or_note_is_dim() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        let backend = screen(&app, 80, 24);

        for head in ["❯ ✎ ", "Notes: "] {
            let (x, y) = find(&backend, &format!("{head}—"));
            let dash = x + u16::try_from(columns(head)).expect("a short head");
            assert!(
                backend.buffer()[(dash, y)].modifier.contains(Modifier::DIM),
                "after {head:?}:\n{backend}"
            );
        }
    }

    #[test]
    fn a_one_line_field_with_a_length_shows_its_counter() {
        let mut app = app(&[("batch", EXAMPLE)]);
        keys(&mut app, "jjjjSnappy in holly");
        let backend = screen(&app, 100, 24);
        insta::assert_snapshot!(backend);

        // The counter ends the question's rows above the image and the
        // note is cut off. The marker keeps clear of the counter on the
        // right and goes to the blank start of its line.
        let (counter, row) = find(&backend, "15/125");
        assert_eq!(counter, 93, "{backend}");
        assert_eq!(find(&backend, "↓ more"), (1, row), "{backend}");
    }

    #[test]
    fn the_reject_field_has_its_own_keys() {
        let mut app = review_row_below_submit(2);
        keys(&mut app, "out of date");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn the_session_note_field_has_its_own_keys() {
        let mut app = review_row_below_submit(1);
        keys(&mut app, "approved");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_diff_entry_shows_its_diff_under_its_title() {
        let review = include_str!("../../../examples/sessions/retry-review.json");
        let titled = app(&[("review", review)]);

        insta::assert_snapshot!("diff_in_pane", screen(&titled, 120, 20));

        // Without a title the header names the file.
        let without_title = review.replace(r#""title": "Retry with backoff","#, "");
        let untitled = app(&[("review", &without_title)]);
        let text = screen(&untitled, 120, 20).to_string();
        assert!(text.contains("fetch.rs"), "{text}");
    }

    #[test]
    fn a_markdown_entry_shows_rendered_under_its_title() {
        let review = include_str!("../../../examples/sessions/retry-review.json");
        let mut titled = app(&[("review", review)]);
        keys(&mut titled, "l");
        insta::assert_snapshot!("markdown_in_pane", screen(&titled, 80, 24));

        // Inline Markdown without a title is named by its kind.
        let without_title = review.replace(r#""title": "short.md", "#, "");
        let mut untitled = app(&[("review", &without_title)]);
        keys(&mut untitled, "l");
        let text = screen(&untitled, 80, 24).to_string();
        assert!(text.contains(" markdown "), "{text}");
    }

    #[test]
    fn a_code_entry_shows_highlighted_with_line_numbers() {
        let app = app(&[(
            "code",
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "Keep the retry?", "kind": "single",
                "context": [{"type": "code", "title": "fetch.rs",
                             "content": "pub fn fetch(url: &str) -> Response {\n    retry(|| get(url))\n}\n"}],
                "options": [{"id": "a", "label": "Yes"}]}]}"#,
        )]);

        let backend = screen(&app, 120, 20);

        insta::assert_snapshot!("code_in_pane", backend);
        let row = row_starting_with(&backend, "Keep the retry?");
        let line = screen_lines(&backend)[usize::from(row) + 1].clone();
        assert!(line.contains("1 pub fn fetch"), "{backend}");
        let start = line.find("pub fn").expect("the code line");
        let x = line[..start].chars().count() + "pub ".len();
        let x = u16::try_from(x).expect("a narrow screen");
        assert_eq!(
            backend.buffer()[(x, row + 1)].fg,
            Color::Indexed(5),
            "a highlighted keyword:\n{backend}"
        );
    }

    #[test]
    fn z_shows_any_entry_full_screen_with_its_header() {
        let review = include_str!("../../../examples/sessions/retry-review.json");
        let mut app = app(&[("review", review)]);
        keys(&mut app, "lz");
        insta::assert_snapshot!("markdown_full_screen", screen(&app, 80, 24));

        keys(&mut app, "z");
        let text = screen(&app, 80, 24).to_string();
        assert!(text.contains("pick one"), "z goes back:\n{text}");
    }

    /// A question with an own answer and a new file of more lines than
    /// the pane below the question has at 80×24.
    fn long_context() -> App {
        let lines: String = (1..=30).map(|line| format!("line {line}\\n")).collect();
        app(&[(
            "long",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Merge it?", "kind": "single", "custom": true,
                    "context": [{{"type": "diff", "title": "Thirty lines", "new": {{"content": "{lines}"}}}}],
                    "options": [{{"id": "a", "label": "Yes"}}]}}]}}"#
            ),
        )])
    }

    /// The row of the screen whose text starts with `start` after the
    /// margin.
    fn row_starting_with(backend: &TestBackend, start: &str) -> u16 {
        let row = screen_lines(backend)
            .iter()
            .position(|line| line.chars().skip(1).collect::<String>().starts_with(start))
            .unwrap_or_else(|| panic!("no row starts with {start}:\n{backend}"));
        u16::try_from(row).expect("a short screen")
    }

    #[test]
    fn the_focused_pane_scrolls_and_shows_its_keys() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        let mut drawn = |app: &mut App| {
            let backend = screen_in(app, &mut view, 80, 24);
            hand_on_pane(app, &view);
            backend
        };
        insta::assert_snapshot!("pane_unfocused", drawn(&mut app));

        special(&mut app, KeyCode::Tab);
        special(&mut app, KeyCode::PageDown);
        let backend = drawn(&mut app);
        insta::assert_snapshot!("pane_focused_scrolled", backend);
        let header = row_starting_with(&backend, "Thirty lines");
        assert!(
            backend.buffer()[(1, header)]
                .modifier
                .contains(Modifier::REVERSED),
            "the focused pane's header stands out:\n{backend}"
        );

        keys(&mut app, "?");
        insta::assert_snapshot!("pane_help", drawn(&mut app));
    }

    #[test]
    fn a_frame_too_small_for_asqr_keeps_the_pane_focus_and_the_full_screen() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        // What the run loop does after each frame.
        let mut drawn = |app: &mut App, width: u16, height: u16| {
            screen_in(app, &mut view, width, height);
            hand_on_pane(app, &view);
        };
        drawn(&mut app, 80, 24);
        special(&mut app, KeyCode::Tab);
        drawn(&mut app, 80, 24);

        drawn(&mut app, 40, 10);
        drawn(&mut app, 80, 24);
        let state = app.active().expect("a session");
        assert!(state.pane_focused(), "the pane keeps the focus");

        keys(&mut app, "z");
        drawn(&mut app, 80, 24);
        drawn(&mut app, 40, 10);
        drawn(&mut app, 80, 24);
        let state = app.active().expect("a session");
        assert!(state.full_screen(), "the full-screen view stays");
        assert!(state.pane_focused());
    }

    #[test]
    fn the_full_screen_help_says_how_to_leave_the_full_screen() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        screen_in(&app, &mut view, 80, 24);
        hand_on_pane(&mut app, &view);
        // Entered from the pane, where "back to the question" would be wrong.
        special(&mut app, KeyCode::Tab);
        keys(&mut app, "z?");
        insta::assert_snapshot!("full_screen_help", screen_in(&app, &mut view, 80, 24));
    }

    #[test]
    fn the_text_cursor_hides_while_the_pane_has_the_focus() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        screen_in(&app, &mut view, 80, 24);
        hand_on_pane(&mut app, &view);
        keys(&mut app, "jmine");

        let typing = screen_in(&app, &mut view, 80, 24);
        let own = row_starting_with(&typing, "❯ (•) ✎");
        let reversed_in = |backend: &TestBackend, row: u16| {
            (0..80)
                .filter(|&x| {
                    backend.buffer()[(x, row)]
                        .modifier
                        .contains(Modifier::REVERSED)
                })
                .count()
        };
        assert_eq!(reversed_in(&typing, own), 1, "the cursor:\n{typing}");

        special(&mut app, KeyCode::Tab);
        let focused = screen_in(&app, &mut view, 80, 24);
        assert_eq!(reversed_in(&focused, own), 0, "no cursor:\n{focused}");
        assert!(focused.to_string().contains("mine"), "{focused}");
    }

    #[test]
    fn the_focused_pane_names_sideways_and_open_only_where_they_work() {
        let mut state = SessionState::new(
            "pictured",
            serde_json::from_str(
                r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
                    "context": [{"type": "image", "path": "/pictures/cat.png"}], "options": [{"id": "a", "label": "A"}]}]}"#,
            )
            .expect("parses"),
            None,
        );
        let page = |columns: usize| Page {
            rows: Extent {
                shown: 10,
                total: 10,
            },
            columns: Extent {
                shown: 20,
                total: columns,
            },
        };
        state.set_pane_page(Some(page(20)));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let keys_of = |hints: Vec<(&str, &str)>| {
            hints
                .iter()
                .map(|(key, _)| key.to_string())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(20)))),
            ["↑/↓", "pgup/pgdn", "z", "o", "?", "tab/esc/q"]
        );
        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(50)))),
            ["↑/↓", "←/→", "pgup/pgdn", "z", "o", "?", "tab/esc/q"]
        );
        state.handle(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!(
            key_hints(&state, false, Some(page(20))),
            [
                ("↑/↓", "scroll"),
                ("pgup/pgdn", "page"),
                ("o", "open"),
                ("?", "help"),
                ("z/esc/q", "back")
            ],
            "the full-screen view"
        );

        let text = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "context": [{"type": "code", "content": "x"}], "options": [{"id": "a", "label": "A"}]}]}"#;
        let mut state =
            SessionState::new("text", serde_json::from_str(text).expect("parses"), None);
        state.set_pane_page(Some(page(20)));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            keys_of(key_hints(&state, false, Some(page(20)))),
            ["↑/↓", "pgup/pgdn", "z", "?", "tab/esc/q"],
            "o opens images only"
        );
    }

    #[test]
    fn a_relative_image_path_shows_a_placeholder() {
        let text = screen(&with_image("images/x.png"), 80, 24).to_string();
        assert!(text.contains("relative path"), "{text}");
    }

    // -----------------------------------------------------------------
    // The reading width and the context layout
    // -----------------------------------------------------------------

    /// A question whose text and first option's description run far past
    /// the reading width, with `context` (a JSON member and its comma, or
    /// nothing) and an own answer.
    fn long_question(context: &str) -> App {
        let text = "A question text that goes on and on, ".repeat(8);
        let description = "a description that would fill a very wide line ".repeat(5);
        app(&[(
            "long",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": {text:?}, "kind": "single", "custom": true, {context}
                    "options": [{{"id": "a", "label": "First", "description": {description:?}}}, {{"id": "b", "label": "Second"}}]}}]}}"#
            ),
        )])
    }

    /// The rightmost column with text in the rows of the tab, between the
    /// tab bar and the status line.
    fn rightmost_text_column(backend: &TestBackend) -> u16 {
        let buffer = backend.buffer();
        (4..buffer.area.height - 2)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].symbol() != " ")
            .map(|(x, _)| x)
            .max()
            .expect("the tab has text")
    }

    #[test]
    fn a_question_without_context_stays_within_the_reading_width() {
        let app = long_question("");
        // The text starts after the one-column margin, so its last column
        // is the reading width itself.
        for width in [104, 120, 160, 230] {
            let backend = screen(&app, width, 30);
            let rightmost = rightmost_text_column(&backend);
            assert!(
                (95..=READING_WIDTH).contains(&rightmost),
                "{width} columns end at {rightmost}:\n{backend}"
            );
        }
        insta::assert_snapshot!("reading_width_160x30", screen(&app, 160, 30));
    }

    #[test]
    fn the_review_stays_within_the_reading_width() {
        let mut app = long_question("");
        keys(&mut app, "jj");
        keys(
            &mut app,
            &"an own answer much longer than any line ".repeat(6),
        );
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");

        let backend = screen(&app, 200, 24);
        assert_eq!(rightmost_text_column(&backend), READING_WIDTH, "{backend}");
    }

    #[test]
    fn a_question_with_context_beside_is_narrower_than_one_without() {
        let file = png();
        let image = file.path().to_str().expect("UTF-8 path");
        let with_context = long_question(&format!(
            r#""context": [{{"type": "image", "path": {image:?}}}],"#
        ));

        // 160 columns leave 158 inside the margins: 78 for the question,
        // the separator and 79 for the image.
        let backend = screen(&with_context, 160, 30);
        let (columns, _) = image_cells(&backend);
        assert_eq!(*columns.start(), 80, "{backend}");
        let text_columns = (4..28)
            .flat_map(|y| (0..79).map(move |x| (x, y)))
            .filter(|&position| backend.buffer()[position].symbol() != " ")
            .map(|(x, _)| x)
            .max();
        assert!(
            text_columns.is_some_and(|column| (70..=78).contains(&column)),
            "the question wraps within its 78 columns: {text_columns:?}"
        );
    }

    #[test]
    fn a_terminal_too_small_for_the_context_shows_a_hint() {
        // 80×20 leaves 78×14 for the tab: beside needs 101 columns, below
        // 8 rows for the context with the question at half the height.
        insta::assert_snapshot!(
            "context_hint_80x20",
            screen(&with_image("/pictures/x.png"), 80, 20)
        );
    }

    #[test]
    fn the_context_hint_and_the_cut_intro_put_esc_first_while_a_field_has_the_keys() {
        let mut app = app(&[(
            "pictured",
            r#"{"asqr": 2, "intro": "First line of the intro\nsecond line\nthird line\nfourth line",
                "questions": [{"id": "q", "text": "Which one?", "kind": "single", "custom": true,
                "context": [{"type": "image", "path": "/pictures/x.png"}], "options": [{"id": "a", "label": "This one"}]}]}"#,
        )]);
        let rows = |app: &App| screen_lines(&screen(app, 80, 20));
        let unfocused = rows(&app);
        assert!(
            unfocused.iter().any(|row| row.contains("… i: more")),
            "{unfocused:#?}"
        );
        assert!(
            unfocused.iter().any(|row| row.contains("· z: full screen")),
            "{unfocused:#?}"
        );

        keys(&mut app, "j");

        let backend = screen(&app, 80, 20);
        let typing = screen_lines(&backend);
        assert!(
            typing.iter().any(|row| row.contains("… esc, i: more")),
            "{typing:#?}"
        );
        assert!(
            typing
                .iter()
                .any(|row| row.contains("· esc, z: full screen")),
            "{typing:#?}"
        );
        insta::assert_snapshot!("context_hint_in_a_field_80x20", backend);
    }

    #[test]
    fn the_cut_intro_puts_esc_first_in_a_text_answer() {
        let mut app = long_header();
        keys(&mut app, "ll");

        insta::assert_snapshot!("intro_cut_in_a_field_80x24", screen(&app, 80, 24));
    }

    #[test]
    fn at_80_columns_the_pane_header_offers_tab_where_the_key_bar_has_no_room() {
        let lines: String = (1..=30).map(|line| format!("line {line}\\n")).collect();
        let app = app(&[(
            "long",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Merge which?", "kind": "multi", "custom": true,
                    "context": [{{"type": "diff", "title": "Thirty lines", "new": {{"content": "{lines}"}}}}],
                    "options": [{{"id": "a", "label": "This"}}, {{"id": "b", "label": "That"}}]}}]}}"#
            ),
        )]);

        let backend = screen(&app, 80, 24);
        let rows = screen_lines(&backend);

        let key_bar = rows.last().expect("a key bar");
        assert!(!key_bar.contains("tab"), "no room for tab: {key_bar}");
        let header = &rows[usize::from(row_starting_with(&backend, "Thirty lines"))];
        assert!(header.trim_end().ends_with(" · tab"), "{backend}");
    }

    #[test]
    fn a_layout_asked_for_wins_while_it_fits() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        // 140×40 is landscape, where auto puts the image beside.
        let backend = screen_in(
            &app,
            &mut View::new("q", PaneLayout::Below, Picker::halfblocks()),
            140,
            40,
        );
        let (columns, rows) = image_cells(&backend);
        assert_eq!(
            (*columns.start(), *rows.start()),
            (1, 11),
            "under the question's six rows and the header:\n{backend}"
        );

        // 110×70 is portrait, where auto puts the image below; 108 columns
        // inside the margins give the question its 60 and the image the
        // rest after the separator.
        let backend = screen_in(
            &app,
            &mut View::new("q", PaneLayout::Beside, Picker::halfblocks()),
            110,
            70,
        );
        let (columns, rows) = image_cells(&backend);
        assert_eq!((*columns.start(), *rows.start()), (62, 5), "{backend}");
    }

    #[test]
    fn a_resize_to_a_nearly_square_shape_keeps_the_arrangement_shown() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));
        // 192×106 leaves 190×100 for the tab, a shape of 0.95.
        let (columns, rows) = image_cells(&screen(&app, 192, 106));
        assert_eq!(
            (*columns.start(), *rows.start()),
            (1, 11),
            "shown fresh, it is below"
        );

        let mut view = View::new("q", PaneLayout::Auto, Picker::halfblocks());
        screen_in(&app, &mut view, 230, 60);
        let (columns, rows) = image_cells(&screen_in(&app, &mut view, 192, 106));
        assert_eq!(
            (*columns.start(), *rows.start()),
            (96, 5),
            "after beside, it stays beside"
        );
    }

    // -----------------------------------------------------------------
    // The question's height above its context
    // -----------------------------------------------------------------

    /// The entry of every question with context in these tests.
    fn code_entry() -> serde_json::Value {
        serde_json::json!([{"type": "code", "title": "ctx.rs", "content": "fn main() {}\n"}])
    }

    /// A single question with `options` options without descriptions.
    /// Its text, hint and blank line, the options, and the blank line and
    /// `Notes` take `options + 5` rows.
    fn choice(id: &str, options: usize, context: bool) -> serde_json::Value {
        let options: Vec<_> = (1..=options)
            .map(|index| serde_json::json!({"id": format!("o{index}"), "label": format!("Option {index}")}))
            .collect();
        let mut question =
            serde_json::json!({"id": id, "text": "?", "kind": "single", "options": options});
        if context {
            question["context"] = code_entry();
        }
        question
    }

    /// A text question with context and a length. Its text, hint and blank
    /// line, the answer and the row for its counter, and the blank line
    /// and `Notes` take 7 rows while the answer fits one line.
    fn typed(id: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "text": "?", "kind": "text",
                           "length": {"warn": 500}, "context": code_entry()})
    }

    fn shaped(questions: &[serde_json::Value]) -> String {
        serde_json::json!({"asqr": 2, "questions": questions}).to_string()
    }

    /// The rows the question asks for above its context, measured in the
    /// tab area of a 60×30 terminal, or `width` columns of it.
    fn need(app: &App, view: &mut View, width: u16) -> usize {
        let state = app.active().expect("a session is waiting");
        view.question_height.need(state, Rect::new(1, 5, width, 24))
    }

    fn ctrl_j(app: &mut App) {
        app.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    }

    #[test]
    fn below_the_question_takes_what_the_tallest_question_with_context_needs() {
        // name, questions, rows on every tab
        let cases = [
            (
                "equal questions",
                vec![
                    choice("a", 3, true),
                    choice("b", 3, true),
                    choice("c", 3, true),
                ],
                8,
            ),
            (
                "one tall question",
                vec![
                    choice("a", 2, true),
                    choice("b", 9, true),
                    choice("c", 2, true),
                ],
                14,
            ),
            ("a text question", vec![typed("t"), choice("a", 1, true)], 7),
            (
                "a taller question without context",
                vec![choice("a", 2, true), choice("b", 12, false)],
                7,
            ),
        ];
        for (name, questions, rows) in cases {
            let mut app = app(&[("shape", &shaped(&questions))]);
            let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
            for tab in 0..questions.len() {
                assert_eq!(need(&app, &mut view, 58), rows, "{name}, tab {tab}");
                special(&mut app, KeyCode::Right);
            }
        }
    }

    #[test]
    fn typing_grows_the_need_at_once_and_it_shrinks_only_after_leaving_the_field() {
        let mut app = app(&[("shape", &shaped(&[typed("t"), choice("a", 1, true)]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        assert_eq!(need(&app, &mut view, 58), 7);

        keys(&mut app, "abc");
        assert_eq!(need(&app, &mut view, 58), 7, "the line is not full yet");
        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 58), 8, "a new line grows it at once");
        keys(&mut app, "d");
        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 58), 9);

        special(&mut app, KeyCode::Backspace);
        special(&mut app, KeyCode::Backspace);
        assert_eq!(
            need(&app, &mut view, 58),
            9,
            "deleting lines keeps the height while the field has the keys"
        );
        special(&mut app, KeyCode::Esc);
        assert_eq!(need(&app, &mut view, 58), 8, "leaving the field lowers it");
    }

    #[test]
    fn the_cursor_after_a_full_line_counts_only_while_typing() {
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());

        // `❯ ✎ ` and 54 characters fill the 58 columns, so the cursor's
        // blank starts a line of its own on the screen.
        keys(&mut app, &"x".repeat(54));
        let backend = screen_in(&app, &mut view, 60, 30);
        let rows = visible_rows(&backend);
        let answer = rows
            .iter()
            .position(|row| row.contains('✎'))
            .expect("the answer is on screen");
        assert_eq!(rows[answer + 1].trim(), "", "{backend}");
        assert_eq!(need(&app, &mut view, 58), 8);
        special(&mut app, KeyCode::Esc);
        assert_eq!(need(&app, &mut view, 58), 7, "without the cursor");
    }

    #[test]
    fn moving_the_focus_into_the_pane_counts_as_leaving_the_field() {
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        ctrl_j(&mut app);
        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 58), 9);
        special(&mut app, KeyCode::Backspace);
        assert_eq!(need(&app, &mut view, 58), 9);

        screen_in(&app, &mut view, 60, 30);
        hand_on_pane(&mut app, &view);
        special(&mut app, KeyCode::Tab);
        assert!(app.active().is_some_and(SessionState::pane_focused));
        assert_eq!(need(&app, &mut view, 58), 8);
    }

    #[test]
    fn typing_to_an_exact_fill_keeps_the_last_row_in_view() {
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        let notes = |backend: &TestBackend| {
            visible_rows(backend)
                .iter()
                .any(|row| row.starts_with(" Notes: —"))
        };
        let top = pane_row(&screen_in(&app, &mut view, 80, 24));

        // `❯ ✎ ` and 74 letters fill the 78 columns, so the cursor's blank
        // takes a row of its own, which counts while the field has the
        // keys.
        keys(&mut app, &"x".repeat(74));
        let backend = screen_in(&app, &mut view, 80, 24);
        assert!(notes(&backend), "{backend}");
        assert!(!backend.to_string().contains("↓ more"), "{backend}");
        assert_eq!(pane_row(&backend), top + 1, "{backend}");

        special(&mut app, KeyCode::Backspace);
        let backend = screen_in(&app, &mut view, 80, 24);
        assert_eq!(pane_row(&backend), top + 1, "kept while typing:\n{backend}");
        special(&mut app, KeyCode::Esc);
        let backend = screen_in(&app, &mut view, 80, 24);
        assert_eq!(pane_row(&backend), top, "{backend}");
        assert!(notes(&backend), "{backend}");
    }

    /// A text question typed into until it took 9 rows, then cut back to
    /// 8 while its field keeps the keys.
    fn typed_and_cut_back(id: &str) -> App {
        let mut app = app(&[(id, &shaped(&[typed("t")]))]);
        ctrl_j(&mut app);
        ctrl_j(&mut app);
        special(&mut app, KeyCode::Backspace);
        app
    }

    #[test]
    fn a_resize_a_replaced_file_or_another_session_measure_afresh() {
        let mut app = typed_and_cut_back("shape");
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 58), 9);
        special(&mut app, KeyCode::Backspace);
        assert_eq!(need(&app, &mut view, 58), 9, "kept while typing");
        assert_eq!(need(&app, &mut view, 78), 8, "after a resize");

        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 78), 9);
        special(&mut app, KeyCode::Backspace);
        app.add(SessionState::new(
            "shape",
            serde_json::from_str(&shaped(&[typed("t")])).expect("parses"),
            None,
        ));
        assert_eq!(
            app.active().map(SessionState::focus),
            Some(Focus::Field),
            "the replacement keeps the typed answer, and its field has the keys"
        );
        assert_eq!(need(&app, &mut view, 78), 8, "after the file was replaced");

        let mut other = typed_and_cut_back("other");
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        ctrl_j(&mut other);
        assert_eq!(need(&other, &mut view, 58), 9);
        other.add(SessionState::new(
            "next",
            serde_json::from_str(&shaped(&[typed("t")])).expect("parses"),
            None,
        ));
        other.remove("other");
        assert_eq!(
            other.active().map(SessionState::focus),
            Some(Focus::Field),
            "the next session's field has the keys"
        );
        assert_eq!(need(&other, &mut view, 58), 7, "in another session");
    }

    #[test]
    fn the_rows_of_the_other_questions_are_kept_per_width_and_revision() {
        let questions = [
            choice("a", 2, true),
            choice("b", 2, true),
            choice("c", 9, false),
        ];
        let mut app = app(&[("shape", &shaped(&questions))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());

        assert_eq!(need(&app, &mut view, 58), 7);
        assert_eq!(
            view.question_height.rows.len(),
            2,
            "only questions with context"
        );
        assert_eq!(need(&app, &mut view, 78), 7);
        assert_eq!(
            view.question_height.rows.len(),
            2,
            "a resize forgets the old width"
        );

        // The second question grows in the file while the first is shown.
        let taller = [
            choice("a", 2, true),
            choice("b", 6, true),
            choice("c", 9, false),
        ];
        app.add(SessionState::new(
            "shape",
            serde_json::from_str(&shaped(&taller)).expect("parses"),
            None,
        ));
        assert_eq!(need(&app, &mut view, 78), 11);
        screen_in(&app, &mut view, 80, 30);
        assert_eq!(
            view.question_height.rows.len(),
            2,
            "a draw forgets the replaced revision"
        );
    }

    /// The screen row of the context pane's header, which names `ctx.rs`.
    fn pane_row(backend: &TestBackend) -> usize {
        visible_rows(backend)
            .iter()
            .position(|row| row.contains("ctx.rs"))
            .unwrap_or_else(|| panic!("the pane shows:\n{backend}"))
    }

    #[test]
    fn moving_between_questions_keeps_the_split_and_typing_moves_it_by_lines() {
        let questions = [choice("a", 2, true), choice("b", 6, true), typed("t")];
        let mut app = app(&[("shape", &shaped(&questions))]);
        let mut view = View::new("q", PaneLayout::Auto, Picker::halfblocks());

        // A 60×40 terminal is portrait, so the context goes below. The
        // tallest question takes 11 rows under the tab bar's 4.
        let top = pane_row(&screen_in(&app, &mut view, 60, 40));
        assert_eq!(top, 4 + 11);
        for tab in 1..=2 {
            special(&mut app, KeyCode::Right);
            let backend = screen_in(&app, &mut view, 60, 40);
            assert_eq!(pane_row(&backend), top, "tab {tab}:\n{backend}");
        }

        // The text question needs 7 rows; at 5 lines it needs 11, and
        // only its sixth line moves the split.
        for _ in 0..4 {
            ctrl_j(&mut app);
        }
        assert_eq!(pane_row(&screen_in(&app, &mut view, 60, 40)), top);
        ctrl_j(&mut app);
        assert_eq!(pane_row(&screen_in(&app, &mut view, 60, 40)), top + 1);
    }

    // -----------------------------------------------------------------
    // Marks of rows cut off
    // -----------------------------------------------------------------

    /// A session with an intro and one question with context, five
    /// options, an own answer and a note: 11 rows, more than the 8 the
    /// question gets above its context on a 60×24 or 80×24 terminal.
    fn five_options() -> App {
        let options: Vec<_> = ["Holly", "Snow", "Bells", "Candles", "Stars"]
            .iter()
            .enumerate()
            .map(|(index, label)| {
                serde_json::json!({"id": format!("o{index}"), "label": label,
                                   "description": format!("Snappy in {} again.", label.to_lowercase())})
            })
            .collect();
        let json = serde_json::json!({
            "asqr": 2,
            "title": "Mascot lines",
            "intro": "Pick the line the mascot says.",
            "questions": [{"id": "line", "text": "Which line fits the winter mascot?",
                           "kind": "single", "custom": {"label": "Own line"},
                           "context": code_entry(), "options": options}]
        });
        app(&[("mascot", &json.to_string())])
    }

    /// Whether every cell of `text` on the screen is dim and none reversed,
    /// so a marker never looks like the cursor.
    fn dim_only(backend: &TestBackend, text: &str) -> bool {
        let (x, y) = find(backend, text);
        let width = u16::try_from(columns(text)).expect("a short text");
        (x..x + width).all(|x| {
            let modifier = backend.buffer()[(x, y)].modifier;
            modifier.contains(Modifier::DIM) && !modifier.contains(Modifier::REVERSED)
        })
    }

    #[test]
    fn rows_cut_off_above_or_below_the_question_are_marked() {
        for width in [60, 80] {
            let mut app = five_options();
            // The title, the intro and the tab bar leave the question rows
            // 5 to 12, the context starting below them.
            let backend = screen(&app, width, 24);
            insta::assert_snapshot!(format!("cut_below_{width}x24"), backend);
            // The marker ends at the right edge of the question's area,
            // which is the screen's but for the margin.
            let right = width - 1 - 6;
            assert_eq!(find(&backend, "ctx.rs").1, 13, "{backend}");
            assert_eq!(
                find(&backend, "↓ more"),
                (right, 12),
                "over the blank end of the last row:\n{backend}"
            );
            assert!(dim_only(&backend, "↓ more"), "{backend}");
            assert!(!backend.to_string().contains("↑ more"), "{backend}");

            // On the own answer the question scrolls by a row.
            keys(&mut app, "jjjjj");
            let backend = screen(&app, width, 24);
            insta::assert_snapshot!(format!("cut_both_{width}x24"), backend);
            assert_eq!(find(&backend, "↑ more"), (right, 5), "{backend}");
            assert_eq!(find(&backend, "↓ more"), (right, 12), "{backend}");
            assert!(find(&backend, "❯ ( ) ✎ Own line:").1 == 12, "{backend}");
            assert!(dim_only(&backend, "↑ more") && dim_only(&backend, "↓ more"));
        }
    }

    #[test]
    fn a_row_full_of_text_keeps_its_start_and_the_marker_covers_its_end() {
        let json = serde_json::json!({
            "asqr": 2,
            "questions": [{"id": "q", "text": "x".repeat(600), "kind": "single",
                           "options": [{"id": "a", "label": "A"}]}]
        });
        let app = app(&[("full", &json.to_string())]);

        // The text fills whole lines of 58 columns, each further one
        // behind `↪`. The option under the cursor ends the 9 rows of the
        // tab on a 60×15 terminal, so the text's first lines are cut off
        // above, under a full line.
        let backend = screen(&app, 60, 15);
        let rows = visible_rows(&backend);
        assert_eq!(find(&backend, "↑ more"), (53, 4), "{backend}");
        assert_eq!(rows[4], format!(" ↪ {} ↑ more ", "x".repeat(49)));
        assert_eq!(find(&backend, "↓ more"), (53, 12), "{backend}");
    }

    #[test]
    fn the_review_marks_its_cut_rows_too() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");

        // The review opens on Submit, whose button ends the 8 rows a 60×15
        // terminal leaves it: its first line and the note and reject rows
        // are cut off.
        let backend = screen(&app, 60, 15);
        assert_eq!(find(&backend, "[ Submit ]").1, 12, "{backend}");
        assert_eq!(find(&backend, "↑ more"), (53, 5), "{backend}");
        assert_eq!(
            find(&backend, "↓ more"),
            (53, 12),
            "beside the button under the cursor:\n{backend}"
        );

        special(&mut app, KeyCode::Down);
        special(&mut app, KeyCode::Down);
        let backend = screen(&app, 60, 15);
        assert_eq!(find(&backend, "↑ more"), (53, 5), "{backend}");
        assert!(!backend.to_string().contains("↓ more"), "{backend}");
    }

    #[test]
    fn a_question_that_fits_has_no_marks() {
        let backend = screen(&five_options(), 120, 40);
        let text = backend.to_string();
        assert!(
            !text.contains("↑ more") && !text.contains("↓ more"),
            "{text}"
        );
    }

    #[test]
    fn a_marker_that_would_cover_the_cursor_at_both_ends_is_left_out() {
        // In 8 columns both ends of the row hold the cell at column 3.
        let mut terminal = Terminal::new(TestBackend::new(8, 1)).expect("test terminal");
        terminal
            .draw(|frame| mark_cut(frame, frame.area(), 0, "↓ more", Some(3..4)))
            .expect("draws");
        assert_eq!(visible_rows(terminal.backend())[0], " ".repeat(8));

        terminal
            .draw(|frame| mark_cut(frame, frame.area(), 0, "↓ more", Some(7..8)))
            .expect("draws");
        assert_eq!(
            visible_rows(terminal.backend())[0],
            "↓ more  ",
            "the other end"
        );
    }

    // -----------------------------------------------------------------
    // A field taller than the question's area
    // -----------------------------------------------------------------

    /// Types `count` lines, `line 1` to `line N`, drawing after each key
    /// so `↑`/`↓` go by the rows on screen.
    fn type_lines(app: &mut App, view: &mut View, width: u16, height: u16, count: usize) {
        for line in 1..=count {
            if line > 1 {
                ctrl_j(app);
            }
            keys(app, &format!("line {line}"));
            drawn(app, view, width, height);
        }
    }

    /// The reversed cells under the tab bar of a screen whose header takes
    /// one row, above the status line: the text cursor while a field has
    /// the keys.
    fn cursor_cells(backend: &TestBackend) -> Vec<(u16, u16, String)> {
        let buffer = backend.buffer();
        (4..buffer.area.height - 2)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].modifier.contains(Modifier::REVERSED))
            .map(|(x, y)| (x, y, buffer[(x, y)].symbol().to_owned()))
            .collect()
    }

    /// A single question with context, an own answer and a note.
    fn own_and_note(id: &str) -> serde_json::Value {
        let mut question = choice(id, 2, true);
        question["custom"] = serde_json::json!(true);
        question
    }

    #[test]
    fn a_field_taller_than_the_area_keeps_its_text_cursor_in_view() {
        // At 80×24 the question gets 9 rows above its context, 4 to 12.
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        type_lines(&mut app, &mut view, 80, 24, 30);

        // On the last line, the counter takes the area's last row.
        let backend = drawn(&mut app, &mut view, 80, 24);
        let rows = visible_rows(&backend);
        assert_eq!(
            cursor_cells(&backend),
            [(12, 11, " ".to_owned())],
            "{backend}"
        );
        assert!(rows[11].starts_with("     line 30"), "{backend}");
        assert!(rows[4].contains("↑ more"), "{backend}");

        // `↑` keeps the cursor on the last row while the view scrolls up,
        // until the start of the field shows.
        for line in (1..30).rev() {
            special(&mut app, KeyCode::Up);
            let backend = drawn(&mut app, &mut view, 80, 24);
            let y = if line <= 9 { 3 + line } else { 12 };
            let y16 = u16::try_from(y).expect("a row");
            assert_eq!(
                cursor_cells(&backend).first().map(|cell| cell.1),
                Some(y16),
                "line {line}:\n{backend}"
            );
            let rows = visible_rows(&backend);
            assert!(
                rows[y].contains(&format!("line {line}")),
                "line {line}:\n{backend}"
            );
        }
        let backend = drawn(&mut app, &mut view, 80, 24);
        let rows = visible_rows(&backend);
        assert!(rows[4].starts_with(" ❯ ✎ line 1 "), "{backend}");
        assert!(
            rows[4].contains("↑ more"),
            "the question is above:\n{backend}"
        );
    }

    #[test]
    fn a_tall_note_or_own_answer_keeps_its_text_cursor_in_view() {
        for (name, start) in [("note", "n"), ("own answer", "jj")] {
            let mut app = app(&[("shape", &shaped(&[own_and_note("q")]))]);
            let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
            keys(&mut app, start);
            type_lines(&mut app, &mut view, 80, 24, 30);

            let backend = drawn(&mut app, &mut view, 80, 24);
            let cursor = cursor_cells(&backend);
            assert_eq!(cursor.len(), 1, "{name}:\n{backend}");
            let (_, y, symbol) = &cursor[0];
            assert_eq!(symbol, " ", "{name}");
            let rows = visible_rows(&backend);
            assert!(
                rows[usize::from(*y)].contains("line 30"),
                "{name}:\n{backend}"
            );
        }
    }

    #[test]
    fn typing_at_the_end_of_a_tall_field_keeps_its_counter_in_view() {
        let mut own = own_and_note("q");
        own["custom"] = serde_json::json!({"length": {"warn": 500}});
        for (name, question, start) in [("text answer", typed("t"), ""), ("own answer", own, "jj")]
        {
            let mut app = app(&[("shape", &shaped(&[question]))]);
            let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
            keys(&mut app, start);
            type_lines(&mut app, &mut view, 80, 24, 30);

            // The question's area takes rows 4 to 12; the counter takes the
            // last of them, and the cursor stays on the row above.
            let backend = drawn(&mut app, &mut view, 80, 24);
            let rows = visible_rows(&backend);
            let cursor = cursor_cells(&backend);
            assert_eq!(cursor.len(), 1, "{name}:\n{backend}");
            assert_eq!(cursor[0].1, 11, "{name}:\n{backend}");
            assert!(rows[11].contains("line 30"), "{name}:\n{backend}");
            // The notes below are cut off, and the marker keeps clear of the
            // counter at the row's right end.
            assert!(rows[12].trim_end().ends_with("/500"), "{name}:\n{backend}");
            assert!(rows[12].contains("↓ more"), "{name}:\n{backend}");
        }
    }

    #[test]
    fn a_text_answer_of_twelve_lines_keeps_the_cursor_on_a_small_screen() {
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Auto, Picker::halfblocks());
        type_lines(&mut app, &mut view, 60, 24, 12);

        let backend = drawn(&mut app, &mut view, 60, 24);
        let cursor = cursor_cells(&backend);
        assert_eq!(
            cursor.len(),
            1,
            "the reversed blank is on screen:\n{backend}"
        );
        assert_eq!(cursor[0].2, " ");
    }

    #[test]
    fn a_marker_never_covers_the_text_cursor() {
        // 74 letters fill the 78 columns after `❯ ✎ `; every line after
        // the first starts 4 columns in.
        let full = "x".repeat(74);
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        keys(&mut app, &full);
        ctrl_j(&mut app);
        type_lines(&mut app, &mut view, 80, 24, 29);
        ctrl_j(&mut app);
        keys(&mut app, &full);
        // A line after the full one keeps the counter out of the way: on
        // the text's last line the area's last row would hold the counter.
        ctrl_j(&mut app);
        keys(&mut app, "end");
        drawn(&mut app, &mut view, 80, 24);
        special(&mut app, KeyCode::Up);
        special(&mut app, KeyCode::End);

        // The cursor's blank after the full line shows on a row of its own
        // at the bottom, where `↓ more` takes the blank right end.
        let backend = drawn(&mut app, &mut view, 80, 24);
        assert_eq!(
            cursor_cells(&backend),
            [(5, 12, " ".to_owned())],
            "{backend}"
        );
        assert_eq!(find(&backend, "↓ more"), (73, 12), "{backend}");

        // On the last letter, the right end holds the cursor, so the marker
        // goes over the start of the row.
        special(&mut app, KeyCode::Left);
        let backend = drawn(&mut app, &mut view, 80, 24);
        assert_eq!(
            cursor_cells(&backend),
            [(78, 12, "x".to_owned())],
            "{backend}"
        );
        assert_eq!(find(&backend, "↓ more"), (1, 12), "{backend}");

        // The same for `↑ more` on the first row, on the first line's last
        // letter.
        for _ in 0..40 {
            special(&mut app, KeyCode::Up);
            drawn(&mut app, &mut view, 80, 24);
        }
        special(&mut app, KeyCode::End);
        special(&mut app, KeyCode::Left);
        let backend = drawn(&mut app, &mut view, 80, 24);
        assert_eq!(
            cursor_cells(&backend),
            [(78, 4, "x".to_owned())],
            "{backend}"
        );
        assert_eq!(find(&backend, "↑ more"), (1, 4), "{backend}");
    }

    #[test]
    fn the_key_bar_leaves_out_what_does_not_fit_whole() {
        let text = |width: u16| -> String {
            let hints = [
                ("↑/↓", "move"),
                ("enter", "pick"),
                ("←/→", "question"),
                ("n", "note"),
                ("tab", "context"),
                ("?", "help"),
                ("q", "quit"),
            ];
            let line = key_line(&hints, width);
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_owned()
        };

        assert_eq!(
            text(100),
            " ↑/↓  move   enter  pick   ←/→  question   n  note   tab  context   ?  help   q  quit"
        );
        assert_eq!(
            text(85),
            " ↑/↓  move   enter  pick   ←/→  question   n  note   tab  context   ?  help   q  quit",
            "the blanks after the last hint may run past the edge"
        );
        assert_eq!(
            text(80),
            " ↑/↓  move   enter  pick   ←/→  question   tab  context   ?  help   q  quit",
            "the note goes first"
        );
        assert_eq!(
            text(60),
            " ↑/↓  move   enter  pick   ←/→  question   ?  help   q  quit",
            "then the pane's tab"
        );
        assert_eq!(text(40), " ↑/↓  move   ?  help   q  quit");
        assert_eq!(text(20), " ?  help   q  quit");
        assert_eq!(text(5), "");
    }

    #[test]
    fn wrapping_breaks_words_longer_than_a_line() {
        let lines = wrap(Line::from("abcdefghij"), 4, 0, 1);
        let text: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();

        assert_eq!(text, ["abcd", " efg", " hij"]);
    }

    /// The text of each line and the column of its reversed cell, if any.
    fn cursor_columns(lines: &[Line]) -> Vec<(String, Option<usize>)> {
        lines
            .iter()
            .map(|line| {
                let text: String = line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                let mut column = 0;
                let mut cursor = None;
                for span in &line.spans {
                    if span.style.add_modifier.contains(Modifier::REVERSED) {
                        cursor = Some(column);
                    }
                    column += columns(&span.content);
                }
                (text, cursor)
            })
            .collect()
    }

    #[test]
    fn a_word_styled_in_parts_wraps_as_one_word() {
        let line = Line::from(vec![
            Span::from("ab "),
            Span::from("cha").bold(),
            Span::from("nnel"),
        ]);

        let lines = wrap(line, 8, 0, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("ab ".to_owned(), None), ("channel".to_owned(), None)]
        );
    }

    #[test]
    fn a_word_ending_at_the_end_of_a_line_stays_on_it() {
        assert_eq!(
            cursor_columns(&wrap(Line::from("ab cd ef"), 5, 0, 0)),
            [("ab cd ".to_owned(), None), ("ef".to_owned(), None)]
        );
        assert_eq!(
            cursor_columns(&wrap(Line::from("abcd efg"), 4, 0, 0)),
            [("abcd ".to_owned(), None), ("efg".to_owned(), None)],
            "the next line does not start with the blank"
        );
    }

    #[test]
    fn plain_spaces_still_hang_past_the_end_of_a_line() {
        let lines = wrap(Line::from("ab  cd"), 3, 0, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("ab  ".to_owned(), None), ("cd".to_owned(), None)]
        );
    }

    // -----------------------------------------------------------------
    // Wide characters
    // -----------------------------------------------------------------

    /// The text of each line, spans joined.
    fn line_texts(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    /// Thirty characters of two columns each.
    const WIDE_TEXT: &str = "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほ";

    /// A session in Japanese with an emoji in its title, whose intro runs
    /// past the three lines the header gives it at 60×24.
    fn wide_session() -> App {
        let intro = "変更点を確認してから公開してください。 ".repeat(6);
        let json = serde_json::json!({
            "asqr": 2,
            "title": "リリース 🎉 の確認",
            "intro": intro,
            "questions": [
                {"id": "go", "header": "公開", "text": "今すぐ**公開**しますか？", "kind": "single",
                 "options": [
                    {"id": "yes", "label": "今すぐ", "description": "タグを作って公開する"},
                    {"id": "later", "label": "Later", "description": "週末の後で公開する"}],
                 "custom": {"label": "別の答え"}},
                {"id": "notes", "header": "メモ", "text": "リリースノートの一文:", "kind": "text"}
            ]
        });
        app(&[("wide", &json.to_string())])
    }

    #[test]
    fn a_cut_line_measures_wide_characters_in_columns() {
        let line = Line::from(vec![Span::from(" "), Span::from("日本語の説明").dim()]);
        let kept = [
            Span::from(" "),
            Span::from("日本").dim(),
            Span::from("… i: more"),
        ];

        // The marker takes 9 of the 14 columns, which leaves 5: the blank,
        // then 日本, which ends at the edge.
        let cut = ending_in(line.clone(), "… i: more", 14);
        assert_eq!(cut.spans, kept);
        assert_eq!(columns(&line_texts(&[cut])[0]), 14);
        assert_eq!(
            ending_in(line, "… i: more", 15).spans,
            kept,
            "語 would cross the edge"
        );
        assert_eq!(
            line_texts(&[ending_in(Line::from("ab🎉cd"), "…", 4)]),
            ["ab…"]
        );
    }

    #[test]
    fn wrapping_measures_wide_characters_in_columns() {
        let wrapped =
            |text: &'static str, width, rest| line_texts(&wrap(Line::from(text), width, 0, rest));

        assert_eq!(
            wrapped("日本語🎉です", 6, 0),
            ["日本語", "🎉です"],
            "語 ends at the edge"
        );
        assert_eq!(
            wrapped("日本語🎉です", 5, 0),
            ["日本", "語🎉", "です"],
            "語 would cross the edge and moves on whole"
        );
        assert_eq!(wrapped("ab 日本 語", 5, 1), ["ab ", " 日本 ", " 語"]);
        assert_eq!(
            wrapped("日本", 1, 0),
            ["日", "本"],
            "a character wider than the line still goes on"
        );
    }

    #[test]
    fn the_tab_bar_measures_wide_headers_in_columns() {
        let questions: Vec<String> = ["質問一", "質問二", "質問三", "質問四", "質問五"]
            .iter()
            .enumerate()
            .map(|(index, header)| {
                format!(
                    r#"{{"id": "q{index}", "header": "{header}", "text": "?", "kind": "text"}}"#
                )
            })
            .collect();
        let app = app(&[(
            "wide",
            &format!(r#"{{"asqr": 2, "questions": [{}]}}"#, questions.join(",")),
        )]);

        // Each tab takes 10 columns, so four fill the 40 between the
        // arrows exactly.
        let line = tab_line(app.active().expect("the session"), 44);

        assert_eq!(
            line_texts(&[line])[0],
            "   ☐ 質問一  ☐ 質問二  ☐ 質問三  ☐ 質問四  →"
        );
    }

    #[test]
    fn descriptions_line_up_after_a_label_of_wide_characters() {
        let backend = screen(&wide_session(), 100, 24);
        let rows = visible_rows(&backend);

        // The widest head is `❯ ( ) 1. ` and 今すぐ, 15 columns, and the
        // descriptions start 2 after it, behind the margin of one.
        for description in ["タグを作って公開する", "週末の後で公開する"] {
            let row = rows
                .iter()
                .find(|row| row.contains(description))
                .expect("the description is on screen");
            let start = row.find(description).expect("it is in the row");
            assert_eq!(columns(&row[..start]), 18, "{description}:\n{backend}");
        }
    }

    #[test]
    fn a_head_of_wide_characters_counts_its_columns() {
        // As with 28 and 29 `x`: 14 wide characters leave the text 20
        // columns, and it wraps under its start; 15 leave it 18.
        for (count, indent) in [(14, 38), (15, 4)] {
            let backend = screen(&own_answer_labelled(&"日".repeat(count)), 60, 24);
            let rows = visible_rows(&backend);

            let head = rows
                .iter()
                .position(|row| row.contains('✎'))
                .expect("the own answer is on screen");
            let leading = rows[head + 1].chars().take_while(|c| *c == ' ').count();
            assert_eq!(
                leading,
                1 + indent,
                "a label of {count} wraps after {indent}:\n{backend}"
            );
        }
    }

    #[test]
    fn an_own_answer_of_wide_characters_wraps_in_columns() {
        let mut app = wide_session();
        keys(&mut app, "jj");
        keys(&mut app, WIDE_TEXT);

        let backend = screen(&app, 60, 24);
        insta::assert_snapshot!("wide_own_answer_60x24", backend);
        let rows = visible_rows(&backend);
        assert!(
            rows[4].trim_end().ends_with("… esc, i: more"),
            "the cut intro keeps its marker:\n{backend}"
        );
        // Without blanks, the text is one word longer than the room after
        // the head `❯ (•) ✎ 別の答え: `, so it starts under its start on
        // the next line. Its 18 columns leave 40 of the 58 for 20
        // characters, the last ending at the edge.
        let head = rows
            .iter()
            .position(|row| row.contains('✎'))
            .expect("the own answer is on screen");
        assert_eq!(rows[head].trim_end(), " ❯ (•) ✎ 別の答え:");
        assert_eq!(
            rows[head + 1],
            format!("{}{} ", " ".repeat(19), &WIDE_TEXT[..60]),
            "only the margin follows"
        );
        assert_eq!(rows[head + 2].trim(), &WIDE_TEXT[60..]);

        // The cursor goes back to な, which starts the third line.
        for _ in 0..10 {
            special(&mut app, KeyCode::Left);
        }
        let backend = screen(&app, 60, 24);
        // The header takes four rows here, so the tab bar's current tab is
        // among the reversed cells too.
        let head = u16::try_from(head).expect("a row on the screen");
        let cursor: Vec<_> = reversed_cells_in_tab(&backend)
            .into_iter()
            .filter(|&(_, y, _)| y >= head)
            .collect();
        assert_eq!(cursor, [(19, head + 2, "な".to_owned())], "{backend}");
    }

    #[test]
    fn a_text_answer_of_wide_characters_wraps_in_columns() {
        let mut app = wide_session();
        keys(&mut app, "l");
        keys(&mut app, WIDE_TEXT);

        let backend = screen(&app, 60, 24);
        insta::assert_snapshot!("wide_text_answer_60x24", backend);
        // Without blanks, the text is one word longer than the room after
        // `❯ ✎ `, so it starts on the next line, 4 columns in, which leaves
        // 54 of the 58 for 27 characters, the last ending at the edge.
        let rows = visible_rows(&backend);
        let first = rows
            .iter()
            .position(|row| row.contains('✎'))
            .expect("the answer is on screen");
        assert_eq!(rows[first].trim_end(), " ❯ ✎");
        assert_eq!(
            rows[first + 1],
            format!("     {} ", &WIDE_TEXT[..81]),
            "only the margin follows"
        );
        assert_eq!(rows[first + 2].trim(), &WIDE_TEXT[81..]);
    }

    #[test]
    fn the_review_lines_up_and_cuts_wide_labels_and_answers_in_columns() {
        let json = serde_json::json!({
            "asqr": 2,
            "questions": [
                {"id": "a", "header": "Go", "text": "?", "kind": "text"},
                {"id": "b", "header": "a日本語のとても長い見出し", "text": "?", "kind": "text"}
            ]
        });
        let mut app = app(&[("wide", &json.to_string())]);
        keys(&mut app, "ok");
        special(&mut app, KeyCode::Enter);
        keys(&mut app, WIDE_TEXT);
        special(&mut app, KeyCode::Enter);

        let backend = screen(&app, 60, 24);
        insta::assert_snapshot!("wide_review_60x24", backend);
        let rows = visible_rows(&backend);
        let row = |start: &str| {
            rows.iter()
                .find(|row| row.contains(start))
                .unwrap_or_else(|| panic!("{start} is on screen:\n{backend}"))
        };
        // The second header takes 25 columns and is cut to 23, as its
        // last character would cross the 24 labels get. Its summary then
        // starts at column 30 of the tab and has 28 columns: the quote,
        // 13 characters and `…`, the last ending at the edge.
        let summary = format!("\"{}…", &WIDE_TEXT[..39]);
        assert_eq!(
            row("見出 ").trim_end(),
            format!("   ☒ a日本語のとても長い見出   {summary}")
        );
        let go = row("\"ok\"");
        let start = go.find("\"ok\"").expect("the first summary");
        assert_eq!(columns(&go[..start]), 31, "{backend}");
    }

    #[test]
    fn a_queue_path_of_wide_characters_is_cut_in_columns() {
        let app = App::default();

        // The path gets 24 columns: `…` and 23 for its end, which hold 11
        // wide characters, as a 12th would cross the edge.
        assert_eq!(
            title_line(&screen_of_queue(
                &app,
                &format!("/q/{}", "日".repeat(20)),
                60,
                15
            )),
            format!(" asqr · queue: …{} · 0 sessions waiting", "日".repeat(11))
        );
        assert_eq!(
            title_line(&screen_of_queue(
                &app,
                &format!("/{}/{}", "日".repeat(15), "本".repeat(8)),
                60,
                15
            )),
            format!(" asqr · queue: …/{} · 0 sessions waiting", "本".repeat(8)),
            "cut at the separator in reach"
        );
    }

    #[test]
    fn the_key_bar_measures_wide_hints_in_columns() {
        let text = |width: u16| {
            line_texts(&[key_line(&[("日本", "表示"), ("q", "quit")], width)])[0]
                .trim_end()
                .to_owned()
        };

        // ` 日本 ` and ` 表示  ` take 13 columns, ` q ` and ` quit  ` 10,
        // of which the last two blanks may run past the edge.
        assert_eq!(text(21), " 日本  表示   q  quit");
        assert_eq!(text(20), " q  quit");
    }
}
