// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The screen (spec sections 7.1 to 7.4, ADR 22): a title bar, the session
//! header, the tab bar, the current tab, a status line and the key bar.
//!
//! The content of a tab is wrapped here rather than by the paragraph
//! widget, so the number of lines is known and the row under the cursor
//! can always be scrolled into view, whatever the size of the terminal.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::Images;
use super::markdown;
use super::parts::{Level, answer_summary, counter, kind_hint};
use crate::format::{Kind, Question};
use crate::tui::{AnswerState, App, Focus, Row, SessionState};

/// Below this size the layout does not fit; a message says so instead.
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;

/// From this width of the content on, an image goes beside the question
/// instead of below it.
const IMAGE_BESIDE_WIDTH: u16 = 100;

/// Descriptions start on the label's line only while that leaves them at
/// least this much room; otherwise they move to their own lines.
const MIN_DESCRIPTION_WIDTH: usize = 30;

/// `text` in at most `room` characters: whole as long as it fits,
/// otherwise its end behind `…`, cut at a `/` where one is in reach, since
/// the end of a queue's path is what tells queues apart.
fn shorten_from_left(text: &str, room: usize) -> String {
    let length = text.chars().count();
    if length <= room {
        return text.to_owned();
    }
    let keep = room.saturating_sub(1);
    let tail: String = text.chars().skip(length - keep).collect();
    let tail = match tail.find('/') {
        Some(slash) => &tail[slash..],
        None => &tail,
    };
    format!("…{tail}")
}

/// What the screen shows besides the state.
pub struct View<'a> {
    /// The queue name, or its directory for a queue given by `--dir`.
    pub queue: &'a str,
}

pub fn draw(frame: &mut Frame, app: &App, view: &View, images: &mut Images) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = format!(
            "asqr needs at least {MIN_WIDTH}×{MIN_HEIGHT}; this terminal is {}×{}.",
            area.width, area.height
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    }

    let header_lines = app
        .active()
        .map(|state| header_lines(state, usize::from(area.width), area.height))
        .unwrap_or_default();
    let header_height = u16::try_from(header_lines.len()).expect("at most four header lines");

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
    let room =
        usize::from(title.width).saturating_sub(prefix.chars().count() + count.chars().count());
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
        frame.render_widget(key_line(&[("q", "quit")]), keys);
        return;
    };

    frame.render_widget(Paragraph::new(header_lines), header);
    draw_tabs(frame, state, tabs);

    let full_screen_image = state
        .question()
        .and_then(|question| question.image.as_deref())
        .filter(|_| state.image_full_screen());
    if let Some(path) = full_screen_image {
        images.draw_full_screen(frame, path, main);
    } else {
        draw_tab(
            frame,
            state,
            main.inner(ratatui::layout::Margin::new(1, 0)),
            images,
        );
    }

    let feedback = app.current_notice().or(state.message()).unwrap_or_default();
    frame.render_widget(Line::from(feedback).fg(Color::Yellow), status);
    frame.render_widget(
        key_line(&key_hints(state, app.list_cursor().is_some())),
        keys,
    );

    // Overlays use the whole screen, so they fit the smallest one.
    if state.help() {
        draw_help(frame, area);
    }
    if let Some(cursor) = app.list_cursor() {
        draw_session_list(frame, app, cursor, area);
    }
}

/// The session header: the title line, then the intro. The intro takes
/// at most three lines, one on short terminals, so the question keeps its
/// room; a cut intro ends in `…`.
fn header_lines(state: &SessionState, width: usize, height: u16) -> Vec<Line<'static>> {
    let session = state.session();
    let title = session
        .title
        .clone()
        .unwrap_or_else(|| state.id().to_owned());
    let mut spans = vec![Span::from(format!(" {title}")).bold()];
    if let Some(from) = &session.from {
        spans.push(Span::from(format!("  ({from})")));
    }
    if let Some(follows) = &session.follows {
        spans.push(Span::from(format!("  follows: {follows}")).dim());
    }
    let mut lines = vec![Line::from(spans)];

    let Some(intro) = session.intro.as_deref() else {
        return lines;
    };
    let room = if height < 24 { 1 } else { 3 };
    let mut intro: Vec<Line<'static>> = markdown::render(intro)
        .into_iter()
        .flat_map(|line| wrap(line, width.saturating_sub(1), 1, 1))
        .map(|line| line.dim())
        .collect();
    if intro.len() > room {
        intro.truncate(room);
        let last = intro.last_mut().expect("room is at least one line");
        // Replace the end of the last line with the ellipsis, keeping it
        // inside the width.
        let mut text: String = last
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        while text.chars().count() + 1 > width.saturating_sub(1) {
            text.pop();
        }
        *last = Line::from(format!("{}…", text.trim_end())).dim();
    }
    lines.extend(intro);
    lines
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

    let widths: Vec<usize> = labels
        .iter()
        .map(|label| label.chars().count() + 2)
        .collect();
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
}

impl Content {
    fn push_row(&mut self, lines: Vec<Line<'static>>, current: bool) {
        if current {
            self.cursor = (
                self.lines.len(),
                self.lines.len() + lines.len().saturating_sub(1),
            );
        }
        self.lines.extend(lines);
    }
}

fn draw_tab(frame: &mut Frame, state: &SessionState, area: Rect, images: &mut Images) {
    let mut text_area = area;
    if let Some(path) = state
        .question()
        .and_then(|question| question.image.as_deref())
    {
        let image = if area.width >= IMAGE_BESIDE_WIDTH {
            let [text, image] = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)])
                .spacing(1)
                .areas(area);
            text_area = text;
            image
        } else {
            let [text, image] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length((area.height / 3).min(12)),
            ])
            .areas(area);
            text_area = text;
            image
        };
        images.draw(frame, path, image);
    }

    let width = text_area.width as usize;
    let content = match state.question() {
        Some(question) => question_content(state, question, width),
        None => review_content(state, width),
    };

    // Scroll so the whole current row is visible, its start winning when
    // it is taller than the area.
    let height = text_area.height as usize;
    let (start, end) = content.cursor;
    let scroll = end.saturating_sub(height.saturating_sub(1)).min(start);
    frame.render_widget(
        Paragraph::new(content.lines).scroll((scroll as u16, 0)),
        text_area,
    );
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

fn question_content(state: &SessionState, question: &Question, width: usize) -> Content {
    let mut content = Content {
        lines: Vec::new(),
        cursor: (0, 0),
    };
    for line in markdown::render(&question.text) {
        content.lines.extend(wrap(line, width, 0, 0));
    }
    content
        .lines
        .extend(wrap(Line::from(kind_hint(question)).dim(), width, 0, 0));
    content.lines.push(Line::default());

    let answer = &state.answers()[state.tab()];
    // Descriptions line up in one column after the widest label.
    let label_column = question
        .options
        .iter()
        .flatten()
        .enumerate()
        .map(|(index, option)| option_head_width(index, &option.label))
        .max()
        .unwrap_or(0);
    for (index, row) in state.rows().iter().enumerate() {
        let current = index == state.row();
        let pointer = if current { "❯ " } else { "  " };
        let lines = match *row {
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
                option_lines(
                    head,
                    label_column,
                    option.description.as_deref(),
                    width,
                    current,
                )
            }
            Row::Own => {
                let label = question
                    .custom
                    .as_ref()
                    .and_then(|custom| custom.label())
                    .unwrap_or("Own answer")
                    .to_owned();
                // On a single question the own answer is chosen when it
                // counts: it has text and no option is chosen.
                let chosen = answer.custom.is_some()
                    && (question.kind == Kind::Multi || answer.selected.is_empty());
                let head = format!("{pointer}{} ✎ {label}: ", mark(question.kind, chosen));
                let text = answer.custom.as_deref().unwrap_or_default();
                vec![one_line_field(state, head, text, width, current)]
            }
            Row::Answer => text_field(
                state,
                pointer,
                answer.custom.as_deref().unwrap_or_default(),
                width,
            ),
            Row::Question(_) | Row::Submit | Row::Reject => {
                unreachable!("review rows only appear on the review")
            }
        };
        content.push_row(lines, current);
    }

    if question.note {
        content.lines.push(Line::default());
        let editing = state.focus() == Focus::Note;
        let note = match state.note_text() {
            Some(text) => with_cursor(&text, state.field_cursor()),
            None => answer
                .note
                .as_deref()
                .map_or_else(|| vec![Line::from("—").dim()], plain_lines),
        };
        let mut note_lines = Vec::new();
        for (index, line) in note.into_iter().enumerate() {
            let label = if index == 0 { "Notes: " } else { "       " };
            let mut spans = vec![Span::from(label).bold()];
            spans.extend(line.spans);
            note_lines.extend(wrap(Line::from(spans), width, 0, 7));
        }
        content.push_row(note_lines, editing);
    }
    content
}

/// The width of an option's pointer, mark, number and label.
fn option_head_width(index: usize, label: &str) -> usize {
    format!("❯ ( ) {}. ", index + 1).chars().count() + label.chars().count()
}

/// An option: pointer, mark, number and label, then the description from
/// `label_column` on, wrapped under its own start. When that leaves too
/// little room, the descriptions go on the lines below instead.
fn option_lines(
    head: Vec<Span<'static>>,
    label_column: usize,
    description: Option<&str>,
    width: usize,
    current: bool,
) -> Vec<Line<'static>> {
    let head_width: usize = head.iter().map(|span| span.content.chars().count()).sum();
    let head = if current {
        head.into_iter()
            .map(|span| span.add_modifier(Modifier::REVERSED))
            .collect()
    } else {
        head
    };
    let Some(description) = description else {
        return vec![Line::from(head)];
    };
    let description = markdown::render(description);
    let mut lines = Vec::new();
    if width.saturating_sub(label_column + 2) >= MIN_DESCRIPTION_WIDTH {
        let indent = label_column + 2;
        for (index, line) in description.into_iter().enumerate() {
            for (part, wrapped) in wrap(line, width, indent, indent).into_iter().enumerate() {
                if index == 0 && part == 0 {
                    // The first line starts after the label, padded to
                    // the description column.
                    let mut spans = head.clone();
                    spans.push(Span::from(" ".repeat(indent - head_width)));
                    spans.extend(wrapped.spans.into_iter().skip(1));
                    lines.push(Line::from(spans));
                } else {
                    lines.push(wrapped);
                }
            }
        }
    } else {
        lines.push(Line::from(head));
        for line in description {
            lines.extend(wrap(line, width, 6, 6));
        }
    }
    lines
}

/// A one-line field after `head`: the text scrolls sideways so the text
/// cursor stays visible, and the counter sits at the end of the line.
fn one_line_field(
    state: &SessionState,
    head: String,
    text: &str,
    width: usize,
    current: bool,
) -> Line<'static> {
    let focused = current && state.focus() == Focus::Field;
    let counter = focused
        .then(|| state.field_length())
        .flatten()
        .map(|(length, limits)| counter(length, &limits));
    let counter_width = counter
        .as_ref()
        .map_or(0, |(text, _)| text.chars().count() + 1);
    let room = width
        .saturating_sub(head.chars().count() + counter_width)
        .max(2);

    let mut spans = vec![if current && !focused {
        Span::from(head).reversed()
    } else {
        Span::from(head)
    }];
    let chars: Vec<char> = text.chars().collect();
    if focused {
        let cursor = state
            .field_cursor()
            .map_or(chars.len(), |(_, column)| column);
        // The window of `room` columns ending just after the cursor; `…`
        // marks text scrolled out on the left.
        let start = (cursor + 1).saturating_sub(room);
        let visible: String = chars[start..chars.len().min(start + room)].iter().collect();
        let mut line = with_cursor(&visible, Some((0, cursor - start))).remove(0);
        if start > 0 {
            line.spans.insert(0, Span::from("…").dim());
        }
        spans.extend(line.spans);
    } else if chars.is_empty() {
        spans.push(Span::from("—").dim());
    } else if chars.len() > room {
        let shown: String = chars[..room - 1].iter().collect();
        spans.push(Span::from(format!("{shown}…")));
    } else {
        spans.push(Span::from(text.to_owned()));
    }

    if let Some((text, level)) = counter {
        let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        spans.push(Span::from(
            " ".repeat(width.saturating_sub(used + text.chars().count())),
        ));
        spans.push(Span::from(text).fg(level_colour(level)));
    }
    Line::from(spans)
}

/// The answer field of a text question: every line of the answer, wrapped,
/// with the counter after the last one.
fn text_field(state: &SessionState, pointer: &str, text: &str, width: usize) -> Vec<Line<'static>> {
    let lines = match state.field_text() {
        Some(typed) => with_cursor(&typed, state.field_cursor()),
        None if text.is_empty() => vec![Line::from("—").dim()],
        None => plain_lines(text),
    };
    let mut out = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        let lead = if index == 0 {
            format!("{pointer}✎ ")
        } else {
            "    ".to_owned()
        };
        let mut spans = vec![Span::from(lead)];
        spans.extend(line.spans);
        out.extend(wrap(Line::from(spans), width, 0, 4));
    }
    if let Some((length, limits)) = state.field_length() {
        let (text, level) = counter(length, &limits);
        out.push(Line::from(Span::from(text).fg(level_colour(level))).right_aligned());
    }
    out
}

fn review_content(state: &SessionState, width: usize) -> Content {
    let mut content = Content {
        lines: vec![
            Line::from("Review your answers, then submit or reject.").dim(),
            Line::default(),
        ],
        cursor: (0, 0),
    };
    let answers = state.answers();
    let states = state.question_states();
    let questions = &state.session().questions;
    let label_width = questions
        .iter()
        .map(|question| {
            question
                .header
                .as_deref()
                .unwrap_or(&question.id)
                .chars()
                .count()
        })
        .max()
        .unwrap_or(0)
        .min(24);

    for (index, row) in state.rows().iter().enumerate() {
        let current = index == state.row();
        let pointer = if current { "❯ " } else { "  " };
        let lines = match *row {
            Row::Question(question_index) => {
                let question = &questions[question_index];
                let skipped = states[question_index].answer == AnswerState::Skipped;
                let label: String = question
                    .header
                    .as_deref()
                    .unwrap_or(&question.id)
                    .chars()
                    .take(label_width)
                    .collect();
                let mut spans = vec![
                    Span::from(format!(
                        "{pointer}{} {label:<label_width$}  ",
                        if skipped { "☐" } else { "☒" }
                    )),
                    Span::from(answer_summary(question, &answers[question_index])).dim(),
                ];
                if question.required && skipped {
                    spans.push(Span::from("  required").fg(Color::Red));
                }
                let line = Line::from(spans);
                wrap(
                    if current { line.reversed() } else { line },
                    width,
                    0,
                    label_width + 6,
                )
            }
            Row::Submit => {
                let counts = state.counts();
                let button = Line::from(format!("{pointer}[ Submit ]")).bold();
                vec![
                    Line::default(),
                    Line::from(format!(
                        "answered {} · skipped {} · defaulted {}",
                        counts.answered, counts.skipped, counts.defaulted
                    )),
                    Line::default(),
                    if current { button.reversed() } else { button },
                ]
            }
            Row::Reject => vec![one_line_field(
                state,
                format!("{pointer}✗ Reject, reason (optional): "),
                state.reject_reason(),
                width,
                current,
            )],
            Row::Option(_) | Row::Own | Row::Answer => {
                unreachable!("question rows never appear on the review")
            }
        };
        content.push_row(lines, current);
    }
    content
}

fn level_colour(level: Level) -> Color {
    match level {
        Level::Fine => Color::Green,
        Level::Warn => Color::Yellow,
        Level::Over => Color::Red,
    }
}

// ---------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------

fn plain_lines(text: &str) -> Vec<Line<'static>> {
    text.split('\n')
        .map(|line| Line::from(line.to_owned()))
        .collect()
}

/// `text` as lines with the cell under the text cursor inverted, or an
/// inverted blank after the end of its line.
fn with_cursor(text: &str, cursor: Option<(usize, usize)>) -> Vec<Line<'static>> {
    text.split('\n')
        .enumerate()
        .map(|(index, line)| match cursor {
            Some((row, column)) if row == index => {
                let chars: Vec<char> = line.chars().collect();
                let column = column.min(chars.len());
                let before: String = chars[..column].iter().collect();
                let under = chars
                    .get(column)
                    .map_or_else(|| " ".to_owned(), char::to_string);
                let after: String = chars.iter().skip(column + 1).collect();
                Line::from(vec![
                    Span::from(before),
                    Span::from(under).reversed(),
                    Span::from(after),
                ])
            }
            _ => Line::from(line.to_owned()),
        })
        .collect()
}

/// Word-wraps `line` to `width`: the first line is indented by `first`,
/// the following ones by `rest`. Styles are kept per word, and a word
/// longer than a line is broken where the line ends.
fn wrap(line: Line<'static>, width: usize, first: usize, rest: usize) -> Vec<Line<'static>> {
    let line_style = line.style;
    let mut lines: Vec<Vec<Span<'static>>> = vec![vec![Span::from(" ".repeat(first))]];
    let mut used = first;
    for span in line.spans {
        let style = line_style.patch(span.style);
        for word in span.content.split_inclusive(' ') {
            let mut word: Vec<char> = word.chars().collect();
            loop {
                let indent = if lines.len() == 1 { first } else { rest };
                let fits = used + word.len() <= width || word.iter().all(|c| *c == ' ');
                if fits {
                    let text: String = word.iter().collect();
                    lines
                        .last_mut()
                        .expect("there is always a line")
                        .push(Span::styled(text, style));
                    used += word.len();
                    break;
                }
                if used > indent {
                    lines.push(vec![Span::from(" ".repeat(rest))]);
                    used = rest;
                    continue;
                }
                // A word longer than a whole line: break it where the line
                // ends.
                let room = width.saturating_sub(indent).max(1);
                let (head, tail) = word.split_at(room.min(word.len()));
                let text: String = head.iter().collect();
                lines
                    .last_mut()
                    .expect("there is always a line")
                    .push(Span::styled(text, style));
                lines.push(vec![Span::from(" ".repeat(rest))]);
                used = rest;
                word = tail.to_vec();
            }
        }
    }
    lines.into_iter().map(Line::from).collect()
}

// ---------------------------------------------------------------------
// Overlays and the key bar
// ---------------------------------------------------------------------

const HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "move between the rows"),
    ("←/→, h/l", "previous and next question"),
    ("enter", "pick and go on; open or submit on the review"),
    ("space", "toggle an option of a multi question"),
    ("1-9", "pick or toggle an option directly"),
    ("n", "write a note"),
    ("ctrl-j", "new line in an answer or a note"),
    ("esc", "stop typing in a field"),
    ("L", "the list of waiting sessions"),
    ("o, z", "open the image; show it full screen"),
    ("q, ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

fn draw_help(frame: &mut Frame, area: Rect) {
    let area = center(area, 70, HELP.len() as u16 + 2);
    frame.render_widget(Clear, area);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::from(format!("{keys:<11}")).bold(),
                Span::from(*what),
            ])
        })
        .collect();
    let block = Block::bordered()
        .title(" Keys ")
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
fn key_hints(state: &SessionState, list: bool) -> Vec<(&'static str, &'static str)> {
    let mut hints = all_key_hints(state, list);
    if state.question().is_some_and(|question| !question.note) {
        hints.retain(|(key, _)| *key != "n");
    }
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
        (Focus::Field, Some(Row::Reject)) => {
            vec![("enter", "reject"), ("↑", "back"), ("esc", "stop typing")]
        }
        (Focus::Field, _) => vec![("enter", "pick"), ("↑/↓", "leave"), ("esc", "stop typing")],
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

fn key_line(keys: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (key, what) in keys {
        spans.push(Span::from(format!(" {key} ")).add_modifier(Modifier::REVERSED));
        spans.push(Span::from(format!(" {what}  ")));
    }
    Line::from(spans)
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::format::Session;

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
        let mut images = Images::new(ratatui_image::picker::Picker::halfblocks());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, app, &View { queue }, &mut images))
            .expect("draws");
        terminal.backend().clone()
    }

    fn title_line(backend: &TestBackend) -> String {
        let buffer = backend.buffer();
        (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    // -----------------------------------------------------------------
    // Sizes
    // -----------------------------------------------------------------

    #[test]
    fn the_intro_shows_under_the_title_as_far_as_there_is_room() {
        let long_intro = RELEASE.replace(
            "Three decisions before the release. `required` questions block submit.",
            "Three decisions before the release. `required` questions block submit. \
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
            r#"{{"asqr": 1, "questions": [{{"id": "q", "text": "Which one?", "kind": "single", "custom": true,
                "options": [{}]}}]}}"#,
            options.join(",")
        );
        let mut app = app(&[("many", &json)]);
        for _ in 0..12 {
            special(&mut app, KeyCode::Down);
        }
        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    #[test]
    fn a_long_session_scrolls_the_tab_bar_to_the_current_tab() {
        let questions: Vec<String> = (1..=40)
            .map(|n| format!(r#"{{"id": "q{n}", "text": "Question {n}?", "kind": "single", "options": [{{"id": "a", "label": "A"}}]}}"#))
            .collect();
        let json = format!(r#"{{"asqr": 1, "questions": [{}]}}"#, questions.join(","));
        let mut app = app(&[("long", &json)]);
        for _ in 0..30 {
            special(&mut app, KeyCode::Right);
        }
        insta::assert_snapshot!(screen(&app, 80, 16));
    }

    #[test]
    fn narrow_screens_put_descriptions_below_the_label() {
        let json = r#"{"asqr": 1, "questions": [{"id": "q", "text": "Which approach?", "kind": "single",
            "options": [
                {"id": "a", "label": "Rewrite the parser from scratch", "description": "Cleanest result, takes the longest."},
                {"id": "b", "label": "Patch the tokenizer", "description": "Quick, but the grammar stays odd."}]}]}"#;
        let app = app(&[("approach", json)]);

        insta::assert_snapshot!("descriptions_below_60x15", screen(&app, 60, 15));
        insta::assert_snapshot!("descriptions_beside_120x15", screen(&app, 120, 15));
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

    #[test]
    fn the_own_answer_is_typed_in_its_line_and_scrolls_sideways() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ljjj");
        keys(
            &mut app,
            "a mailing list, the release channel and the team chat",
        );
        insta::assert_snapshot!(screen(&app, 60, 15));
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
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "ljjj");
        special(&mut app, KeyCode::Enter);
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
                r#"{{"asqr": 1, "questions": [{{"id": "q", "text": "Which one?", "kind": "single",
                    "image": {image:?}, "options": [{{"id": "a", "label": "This one"}}]}}]}}"#
            ),
        )])
    }

    /// A two-colour PNG, so the halfblock rendering is visible.
    fn png() -> tempfile::NamedTempFile {
        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        let image = image::RgbImage::from_fn(160, 160, |x, _| {
            if x < 80 {
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

        assert!(
            *columns.start() > 80,
            "right of the question text: {columns:?}"
        );
        assert_eq!(*rows.start(), 4, "from the top of the tab: {rows:?}");
    }

    #[test]
    fn a_narrow_screen_shows_the_image_below_the_question() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        let (_, rows) = image_cells(&screen(&app, 80, 24));

        assert!(*rows.start() > 10, "below the question text: {rows:?}");
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
            screen(&with_image("/nonexistent/x.png"), 80, 20)
        );

        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        std::fs::write(file.path(), "not a picture").expect("written");
        let text = screen(&with_image(file.path().to_str().expect("UTF-8")), 80, 20).to_string();
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
        let json = r#"{"asqr": 1, "questions": [{"id": "q", "text": "Name it", "kind": "single",
            "note": false, "custom": true, "options": [{"id": "a", "label": "asqr"}]}]}"#;
        let mut app = app(&[("name", json)]);
        keys(&mut app, "jmine");
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    #[test]
    fn an_empty_text_answer_without_focus_shows_a_dash() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_one_line_field_with_a_length_shows_its_counter() {
        let mut app = app(&[("batch", EXAMPLE)]);
        keys(&mut app, "jjjjSnappy in holly");
        insta::assert_snapshot!(screen(&app, 100, 24));
    }

    #[test]
    fn the_reject_field_has_its_own_keys() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "ljjjjout of date");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_relative_image_path_shows_a_placeholder() {
        let text = screen(&with_image("images/x.png"), 80, 20).to_string();
        assert!(text.contains("relative path"), "{text}");
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
}
