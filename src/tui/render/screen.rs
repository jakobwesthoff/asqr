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
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::Images;
use super::markdown;
use super::parts::{Level, answer_summary, counter, cut, kind_hint};
use crate::format::{ContextEntry, Kind, Question};
use crate::tui::{AnswerState, App, Focus, Row, SessionState};

/// Below this size the layout does not fit; a message says so instead.
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;

/// From this width of the content on, the context goes beside the
/// question instead of below it.
const CONTEXT_BESIDE_WIDTH: u16 = 100;

/// Descriptions start on the label's line only while that leaves them at
/// least this much room; otherwise they move to their own lines.
const MIN_DESCRIPTION_WIDTH: usize = 30;

/// The text of a one-line field wraps under its own start only while that
/// leaves it at least this much room; otherwise its further lines start
/// at `NARROW_FIELD_INDENT`. Lower than `MIN_DESCRIPTION_WIDTH`, so the
/// reject reason, whose head leaves 27 columns in a 60-column terminal,
/// still wraps under its start there.
const MIN_FIELD_WIDTH: usize = 20;

/// The indent of a one-line field's further lines when its head leaves
/// less than `MIN_FIELD_WIDTH` columns.
const NARROW_FIELD_INDENT: usize = 4;

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

    let full_screen_image = state.shown_image().filter(|_| state.image_full_screen());
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

/// What stands in for a text or diff entry, which this asqr has no viewer
/// for yet. It names the entry the way an image placeholder names its file.
// TODO: replace with the text and diff viewers (context pane plan, steps 4
// and 5).
fn viewerless_placeholder(
    heading: &'static str,
    name: Option<&String>,
    reason: &'static str,
) -> Paragraph<'static> {
    let mut lines = vec![Line::from(heading).dim()];
    lines.extend(name.map(|name| Line::from(name.clone())));
    lines.push(Line::from(reason).dim());
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn draw_tab(frame: &mut Frame, state: &SessionState, area: Rect, images: &mut Images) {
    let mut text_area = area;
    if state.has_context_area() {
        let context = if area.width >= CONTEXT_BESIDE_WIDTH {
            let [text, context] = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)])
                .spacing(1)
                .areas(area);
            text_area = text;
            context
        } else {
            let [text, context] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length((area.height / 3).min(12)),
            ])
            .areas(area);
            text_area = text;
            context
        };
        match state.shown_context() {
            Some(ContextEntry::Image(image)) => images.draw(frame, &image.path, context),
            Some(ContextEntry::Text(text)) => frame.render_widget(
                viewerless_placeholder(
                    "text not shown",
                    text.title.as_ref().or(text.path.as_ref()),
                    "this asqr cannot show text entries yet",
                ),
                context,
            ),
            Some(ContextEntry::Diff(diff)) => frame.render_widget(
                viewerless_placeholder(
                    "diff not shown",
                    diff.title.as_ref().or(diff.name.as_ref()),
                    "this asqr cannot show diffs yet",
                ),
                context,
            ),
            None => {}
        }
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
                one_line_field(state, head, text, width, current)
            }
            Row::Answer => text_field(
                state,
                pointer,
                answer.custom.as_deref().unwrap_or_default(),
                width,
            ),
            Row::Question(_) | Row::Submit | Row::Note | Row::Reject => {
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
    // An empty or blank description renders to no lines at all. The head
    // then stands alone, or the beside layout below would drop it with
    // the missing first line.
    let description = description.map(markdown::render).unwrap_or_default();
    if description.is_empty() {
        return vec![Line::from(head)];
    }
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

/// A one-line field after `head`. The text wraps like a note: under its
/// own start while that leaves `MIN_FIELD_WIDTH` columns, otherwise from
/// `NARROW_FIELD_INDENT` on. The value itself stays one line (ADR 27);
/// only its display spans several. The counter of a focused field follows
/// on a line of its own, as in `text_field`.
fn one_line_field(
    state: &SessionState,
    head: String,
    text: &str,
    width: usize,
    current: bool,
) -> Vec<Line<'static>> {
    let focused = current && state.focus() == Focus::Field;
    let head_width = head.chars().count();
    let indent = if width.saturating_sub(head_width) >= MIN_FIELD_WIDTH {
        head_width
    } else {
        NARROW_FIELD_INDENT
    };

    let mut spans = vec![if current && !focused {
        Span::from(head).reversed()
    } else {
        Span::from(head)
    }];
    if focused {
        let cursor = state
            .field_cursor()
            .map_or(text.chars().count(), |(_, column)| column);
        spans.extend(with_cursor(text, Some((0, cursor))).remove(0).spans);
    } else if text.is_empty() {
        spans.push(Span::from("—").dim());
    } else {
        spans.push(Span::from(text.to_owned()));
    }
    let mut lines = wrap(Line::from(spans), width, 0, indent);

    if let Some((length, limits)) = focused.then(|| state.field_length()).flatten() {
        let (text, level) = counter(length, &limits);
        lines.push(Line::from(Span::from(text).fg(level_colour(level))).right_aligned());
    }
    lines
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
                let lead = format!(
                    "{pointer}{} {label:<label_width$}  ",
                    if skipped { "☐" } else { "☒" }
                );
                // One line per question: the summary gets what is left of
                // the width and ends in `…` when it is longer.
                let room = width.saturating_sub(lead.chars().count());
                let summary = cut(&answer_summary(question, &answers[question_index]), room);
                let spans = vec![Span::from(lead), Span::from(summary).dim()];
                let line = Line::from(spans);
                vec![if current { line.reversed() } else { line }]
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
            Row::Note => one_line_field(
                state,
                format!("{pointer}✎ Note to the asker (optional): "),
                state.session_note(),
                width,
                current,
            ),
            Row::Reject => one_line_field(
                state,
                format!("{pointer}✗ Reject, reason (optional): "),
                state.reject_reason(),
                width,
                current,
            ),
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
/// the following ones by `rest`. A word is a run of non-blanks with the
/// blanks after it, whatever styles its characters carry, so a style
/// change inside a word, such as the text cursor or Markdown emphasis,
/// never splits it. Every character keeps its style, and a word longer
/// than a line is broken where the line ends.
fn wrap(line: Line<'static>, width: usize, first: usize, rest: usize) -> Vec<Line<'static>> {
    let mut words: Vec<Vec<(char, Style)>> = Vec::new();
    for span in &line.spans {
        let style = line.style.patch(span.style);
        for c in span.content.chars() {
            let after_blank = words
                .last()
                .and_then(|word| word.last())
                .is_none_or(|(last, _)| *last == ' ');
            if words.is_empty() || (c != ' ' && after_blank) {
                words.push(Vec::new());
            }
            words
                .last_mut()
                .expect("a word was started")
                .push((c, style));
        }
    }

    let mut wrapped = Wrapped {
        lines: vec![(first, Vec::new())],
        used: first,
        rest,
    };
    for word in words {
        let blanks = word.iter().rev().take_while(|(c, _)| *c == ' ').count();
        let (mut body, blanks) = word.split_at(word.len() - blanks);
        while !body.is_empty() {
            let indent = if wrapped.lines.len() == 1 {
                first
            } else {
                rest
            };
            if wrapped.used + body.len() <= width {
                wrapped.push(body);
                break;
            }
            if wrapped.used > indent {
                wrapped.break_line();
                continue;
            }
            // A word longer than a whole line: break it where the line
            // ends.
            let room = width.saturating_sub(indent).max(1);
            let (head, tail) = body.split_at(room.min(body.len()));
            wrapped.push(head);
            wrapped.break_line();
            body = tail;
        }
        // Blanks may hang past the end of a line, so the next line does not
        // start with them. A reversed blank is the exception, since it is
        // visible: the text cursor's cell (`with_cursor`) or part of a
        // highlighted head. It starts the next line when this one is full.
        for blank in blanks {
            let visible = blank.1.add_modifier.contains(Modifier::REVERSED);
            if visible && wrapped.used >= width {
                wrapped.break_line();
            }
            wrapped.push(std::slice::from_ref(blank));
        }
    }
    wrapped.into_lines()
}

/// The lines `wrap` builds: each one's indent and its styled characters.
struct Wrapped {
    lines: Vec<(usize, Vec<(char, Style)>)>,
    /// The columns taken on the last line, its indent included.
    used: usize,
    rest: usize,
}

impl Wrapped {
    fn push(&mut self, cells: &[(char, Style)]) {
        let (_, line) = self.lines.last_mut().expect("there is always a line");
        line.extend_from_slice(cells);
        self.used += cells.len();
    }

    fn break_line(&mut self) {
        self.lines.push((self.rest, Vec::new()));
        self.used = self.rest;
    }

    /// The lines as spans: the indent first, as a span of its own that
    /// `option_lines` replaces, then one span per run of a style.
    fn into_lines(self) -> Vec<Line<'static>> {
        self.lines
            .into_iter()
            .map(|(indent, cells)| {
                let mut runs: Vec<(String, Style)> = Vec::new();
                for (c, style) in cells {
                    match runs.last_mut() {
                        Some((text, run_style)) if *run_style == style => text.push(c),
                        _ => runs.push((c.to_string(), style)),
                    }
                }
                let mut spans = vec![Span::from(" ".repeat(indent))];
                spans.extend(
                    runs.into_iter()
                        .map(|(text, style)| Span::styled(text, style)),
                );
                Line::from(spans)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------
// Overlays and the key bar
// ---------------------------------------------------------------------

const HELP: &[(&str, &str)] = &[
    ("↑/↓, k/j", "move between the rows"),
    ("←/→, h/l", "previous and next question"),
    ("", "in a field: at the start or end of its text"),
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
        (Focus::Field, Some(Row::Note)) => {
            vec![
                ("enter", "submit"),
                ("↑/↓", "leave"),
                ("esc", "stop typing"),
            ]
        }
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
    use std::collections::BTreeSet;

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

    /// With the Kitty protocol the terminal draws the image into the cells
    /// its placement names. They must be the cells asqr fills with
    /// placeholders: left to the terminal, it derives them from its own cell
    /// size, which differs from the one asqr resized the image with after a
    /// font size change or in a second tmux client, and crops the image.
    #[test]
    fn a_kitty_image_is_placed_over_exactly_its_placeholder_cells() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));
        let mut picker = ratatui_image::picker::Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
        let mut images = Images::new(picker);
        let mut terminal = Terminal::new(TestBackend::new(140, 24)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app, &View { queue: "q" }, &mut images))
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

        assert!(*rows.start() > 10, "below the question text: {rows:?}");
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
        assert!(*columns.start() > 80, "beside the options: {columns:?}");

        keys(&mut app, "j");
        let backend = screen(&app, 140, 20);
        let buffer = backend.buffer();
        let right_side_used = (4..buffer.area.height)
            .flat_map(|y| (85..buffer.area.width).map(move |x| (x, y)))
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
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Name it", "kind": "single",
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
    fn text_and_diff_entries_show_a_placeholder_naming_them() {
        let review = include_str!("../../../examples/sessions/retry-review.json");
        let mut titled = app(&[("review", review)]);

        insta::assert_snapshot!("diff_placeholder", screen(&titled, 120, 20));
        keys(&mut titled, "l");
        insta::assert_snapshot!("text_placeholder", screen(&titled, 80, 24));

        // Without a title the placeholder names the file, or nothing for
        // inline text.
        let without_titles = review
            .replace(r#""title": "Retry with backoff","#, "")
            .replace(r#""title": "short.md", "#, "");
        let mut untitled = app(&[("review", &without_titles)]);
        let text = screen(&untitled, 120, 20).to_string();
        assert!(text.contains("src/fetch.rs"), "{text}");
        keys(&mut untitled, "l");
        let text = screen(&untitled, 120, 20).to_string();
        assert!(text.contains("text not shown"), "{text}");
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
                    column += span.content.chars().count();
                }
                (text, cursor)
            })
            .collect()
    }

    #[test]
    fn the_text_cursor_after_a_full_line_starts_the_next_one() {
        let line = with_cursor("ab cd", Some((0, 5))).remove(0);

        let lines = wrap(line, 5, 0, 1);

        assert_eq!(
            cursor_columns(&lines),
            [("ab cd".to_owned(), None), ("  ".to_owned(), Some(1))]
        );
    }

    #[test]
    fn the_text_cursor_on_the_space_at_a_line_break_starts_the_next_line() {
        let line = with_cursor("abcd efg", Some((0, 4))).remove(0);

        let lines = wrap(line, 4, 0, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("abcd".to_owned(), None), (" efg".to_owned(), Some(0))]
        );
    }

    #[test]
    fn the_text_cursor_inside_a_word_keeps_the_word_whole() {
        let line = with_cursor("ab channel", Some((0, 3))).remove(0);

        let lines = wrap(line, 8, 0, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("ab ".to_owned(), None), ("channel".to_owned(), Some(0))]
        );
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
}
