// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The screen layout (spec section 7.1): a title bar, the session header,
//! the question list on the left and the current question on the right,
//! a status line and the key bar; overlays for editing, confirming, the
//! help and the session list.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::markdown;
use super::parts::{Level, answer_summary, counter, kind_hint};
use crate::format::{Kind, Question};
use crate::tui::{App, Counts, Field, Mode, SessionState};

/// Below this size the layout does not fit; a message says so instead.
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;

/// What the screen shows besides the state.
pub struct View<'a> {
    /// The queue name, or its directory for a queue given by `--dir`.
    pub queue: &'a str,
}

pub fn draw(frame: &mut Frame, app: &App, view: &View) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = format!(
            "asqr needs at least {MIN_WIDTH}×{MIN_HEIGHT}; this terminal is {}×{}.",
            area.width, area.height
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    }

    let [title, header, main, status, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let waiting = app.sessions().len();
    let title_text = format!(
        " asqr · queue: {} · {waiting} session{} waiting",
        view.queue,
        if waiting == 1 { "" } else { "s" }
    );
    frame.render_widget(Line::from(title_text).style(Style::new().reversed()), title);

    let Some(state) = app.active() else {
        let empty =
            Paragraph::new("Nothing to answer. New sessions show up here as soon as they arrive.")
                .centered()
                .wrap(Wrap { trim: true });
        frame.render_widget(empty, center(main, main.width.saturating_sub(4), 3));
        frame.render_widget(key_line(&[("q", "quit")]), keys);
        return;
    };

    draw_header(frame, state, header);
    let [list, question] = Layout::horizontal([
        Constraint::Length((main.width / 3).clamp(20, 32)),
        Constraint::Fill(1),
    ])
    .areas(main);
    draw_question_list(frame, state, list);
    draw_question(frame, state, question);

    let feedback = app.current_notice().or(state.message()).unwrap_or_default();
    frame.render_widget(
        Line::from(feedback).style(Style::new().fg(Color::Yellow)),
        status,
    );
    frame.render_widget(
        key_line(mode_keys(state.mode(), app.list_cursor().is_some())),
        keys,
    );

    match state.mode() {
        Mode::ConfirmSubmit(counts) => draw_confirm_submit(frame, *counts, main),
        Mode::ConfirmReject(editor) => {
            let area = center(main, 60, 7);
            frame.render_widget(Clear, area);
            let block = Block::bordered().title(" Reject this session? ");
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let [label, field, help] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .areas(inner);
            frame.render_widget(Line::from("Reason (optional):"), label);
            frame.render_widget(editor_widget(editor, Block::bordered()), field);
            frame.render_widget(Line::from("enter rejects · esc goes back").dim(), help);
        }
        Mode::Help => draw_help(frame, main),
        Mode::Browse | Mode::Editing { .. } => {}
    }
    if let Some(cursor) = app.list_cursor() {
        draw_session_list(frame, app, cursor, main);
    }
}

fn draw_header(frame: &mut Frame, state: &SessionState, area: Rect) {
    let session = state.session();
    let mut left = vec![
        Span::from(
            session
                .title
                .clone()
                .unwrap_or_else(|| state.id().to_owned()),
        )
        .bold(),
    ];
    if let Some(from) = &session.from {
        left.push(Span::from(format!("  ({from})")));
    }
    if let Some(follows) = &session.follows {
        left.push(Span::from(format!("  follows: {follows}")).dim());
    }
    let position = format!("{}/{}", state.current() + 1, session.questions.len());
    // The header's bottom border separates it from the list and the
    // question, as one continuous line.
    let block = Block::new().borders(Borders::BOTTOM);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let area = inner;
    let [text, count] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(position.len() as u16 + 1),
    ])
    .areas(area);
    frame.render_widget(Line::from(left), text);
    frame.render_widget(Line::from(position).right_aligned(), count);
}

fn draw_question_list(frame: &mut Frame, state: &SessionState, area: Rect) {
    let block = Block::new().borders(Borders::RIGHT);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let answers = state.answers();
    let lines: Vec<Line> = state
        .session()
        .questions
        .iter()
        .zip(&answers)
        .enumerate()
        .map(|(index, (question, answer))| {
            let current = index == state.current();
            let label = question
                .header
                .clone()
                .unwrap_or_else(|| question.id.clone());
            let line = Line::from(vec![
                Span::from(if current { "> " } else { "  " }),
                Span::from(label),
                Span::from("  "),
                Span::from(answer_summary(question, answer)).dim(),
            ]);
            if current { line.bold() } else { line }
        })
        .collect();
    // Keep the current question in view on long sessions.
    let scroll = (state.current() as u16).saturating_sub(inner.height.saturating_sub(1));
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

fn draw_question(frame: &mut Frame, state: &SessionState, area: Rect) {
    let inner = area.inner(ratatui::layout::Margin::new(1, 0));

    let (content, editor_area) = match state.mode() {
        Mode::Editing { .. } => {
            let [content, editor] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(6)]).areas(inner);
            (content, Some(editor))
        }
        _ => (inner, None),
    };

    let question = &state.session().questions[state.current()];
    let answer = &state.answers()[state.current()];
    let mut lines: Vec<Line> = markdown::render(&question.text);
    lines.push(Line::from(kind_hint(question)).dim());
    lines.push(Line::default());

    for (index, option) in question.options.iter().flatten().enumerate() {
        let chosen = answer.selected.contains(&option.id);
        let mark = match (question.kind, chosen) {
            (Kind::Single, true) => "(•)",
            (Kind::Single, false) => "( )",
            (_, true) => "[x]",
            (_, false) => "[ ]",
        };
        let cursor = index == state.cursor();
        let head = Line::from(vec![
            Span::from(if cursor { "› " } else { "  " }),
            Span::from(format!("{mark} {} ", index + 1)),
            Span::from(option.label.clone()).bold(),
        ]);
        lines.push(if cursor { head.reversed() } else { head });
        if let Some(description) = &option.description {
            for line in markdown::render(description) {
                lines.extend(wrap_indented(
                    line,
                    content.width as usize,
                    DESCRIPTION_INDENT,
                ));
            }
        }
    }

    let typed = question.kind == Kind::Text
        || question
            .custom
            .as_ref()
            .is_some_and(|custom| custom.is_enabled());
    if typed || question.note {
        lines.push(Line::default());
    }
    if typed {
        let label = match (
            question.kind,
            question.custom.as_ref().and_then(|custom| custom.label()),
        ) {
            (Kind::Text, _) => "Answer",
            (_, Some(label)) => label,
            (_, None) => "Own answer",
        };
        lines.push(labelled(label, answer.custom.as_deref()));
    }
    if question.note {
        lines.push(labelled("Note", answer.note.as_deref()));
    }
    if let Some(image) = &question.image {
        lines.push(Line::default());
        lines.push(Line::from(format!("[image: {image}]")).dim());
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), content);

    if let (Some(area), Mode::Editing { field, editor }) = (editor_area, state.mode()) {
        draw_editor(frame, state, question, *field, editor, area);
    }
}

/// How far option descriptions are indented, to sit under the label.
const DESCRIPTION_INDENT: usize = 8;

/// Word-wraps `line` to `width`, indenting every resulting line by
/// `indent`, so wrapped descriptions stay under their option. Styles are
/// kept per word. A word longer than a line is left to the paragraph's own
/// wrapping.
fn wrap_indented(line: Line<'static>, width: usize, indent: usize) -> Vec<Line<'static>> {
    let room = width.saturating_sub(indent).max(1);
    let mut lines = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in line.spans {
        for word in span.content.split_inclusive(' ') {
            let length = word.trim_end().chars().count();
            if used > 0 && used + length > room {
                lines.push(std::mem::take(&mut current));
                used = 0;
            }
            current.push(Span::styled(word.to_owned(), span.style));
            used += word.chars().count();
        }
    }
    lines.push(current);
    lines
        .into_iter()
        .map(|spans| {
            let mut indented = vec![Span::from(" ".repeat(indent))];
            indented.extend(spans);
            Line::from(indented)
        })
        .collect()
}

fn labelled<'a>(label: &str, value: Option<&'a str>) -> Line<'a> {
    Line::from(vec![
        Span::from(format!("{label}: ")).bold(),
        match value {
            Some(value) => Span::from(value),
            None => Span::from("—").dim(),
        },
    ])
}

fn draw_editor(
    frame: &mut Frame,
    state: &SessionState,
    question: &Question,
    field: Field,
    editor: &ratatui_textarea::TextArea<'static>,
    area: Rect,
) {
    let title = match (field, question.kind) {
        (Field::Note, _) => "Note",
        (Field::Custom, Kind::Text) => "Answer",
        (Field::Custom, _) => question
            .custom
            .as_ref()
            .and_then(|custom| custom.label())
            .unwrap_or("Own answer"),
    };
    let mut block = Block::bordered().title(format!(" {title} "));
    if let Some((length, limits)) = state.editor_length() {
        let (text, level) = counter(length, &limits);
        let colour = match level {
            Level::Fine => Color::Green,
            Level::Warn => Color::Yellow,
            Level::Over => Color::Red,
        };
        block = block.title_top(Line::from(format!(" {text} ")).fg(colour).right_aligned());
    }
    frame.render_widget(editor_widget(editor, block), area);
}

fn editor_widget<'a>(
    editor: &'a ratatui_textarea::TextArea<'static>,
    block: Block<'static>,
) -> impl ratatui::widgets::Widget + 'a {
    let mut editor = editor.clone();
    editor.set_block(block);
    editor.set_cursor_line_style(Style::default());
    EditorWidget(editor)
}

/// Owns a styled copy of the editor for one frame.
struct EditorWidget(ratatui_textarea::TextArea<'static>);

impl ratatui::widgets::Widget for EditorWidget {
    fn render(self, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
        (&self.0).render(area, buffer);
    }
}

fn draw_confirm_submit(frame: &mut Frame, counts: Counts, area: Rect) {
    let area = center(area, 50, 7);
    frame.render_widget(Clear, area);
    let text = Text::from(vec![
        Line::default(),
        Line::from(format!(
            "answered {} · skipped {} · defaulted {}",
            counts.answered, counts.skipped, counts.defaulted
        )),
        Line::default(),
        Line::from("enter submits · esc goes back").dim(),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .centered()
            .block(Block::bordered().title(" Submit this session? ")),
        area,
    );
}

const HELP: &[(&str, &str)] = &[
    ("j/k, ↓/↑", "move within the options"),
    ("tab, shift-tab / J, K", "next and previous question"),
    ("space, enter", "select or toggle the option"),
    ("1-9", "pick an option directly"),
    ("c", "type your own answer"),
    ("n", "add a note"),
    ("esc", "leave a text field, keeping the text"),
    ("L", "the list of waiting sessions"),
    ("S", "submit the session"),
    ("X", "reject the session"),
    ("o, z", "open the image; show it full screen"),
    ("q, ctrl-c", "quit, keeping the draft"),
    ("?", "this help"),
];

fn draw_help(frame: &mut Frame, area: Rect) {
    let area = center(area, 62, HELP.len() as u16 + 2);
    frame.render_widget(Clear, area);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::from(format!("{keys:<22}")).bold(),
                Span::from(*what),
            ])
        })
        .collect();
    let block = Block::bordered()
        .title(" Keys ")
        .title_bottom(Line::from(" any key closes ").right_aligned());
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_session_list(frame: &mut Frame, app: &App, cursor: usize, area: Rect) {
    let height = (app.sessions().len() as u16 + 2).min(area.height);
    let area = center(area, area.width.saturating_sub(10).min(70), height);
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
                .filter(|question| question.answer != crate::tui::AnswerState::Skipped)
                .count();
            let title = session
                .title
                .clone()
                .unwrap_or_else(|| state.id().to_owned());
            let line = Line::from(format!(
                "{}{title}  ({answered}/{} answered)",
                if index == cursor { "> " } else { "  " },
                session.questions.len()
            ));
            if index == cursor {
                line.reversed()
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Waiting sessions ")),
        area,
    );
}

fn mode_keys(mode: &Mode, list: bool) -> &'static [(&'static str, &'static str)] {
    if list {
        return &[("j/k", "move"), ("enter", "open"), ("esc", "close")];
    }
    match mode {
        Mode::Browse => &[
            ("j/k", "move"),
            ("space", "select"),
            ("c", "own"),
            ("n", "note"),
            ("tab", "next"),
            ("S", "submit"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Mode::Editing { .. } => &[("esc", "done")],
        Mode::ConfirmSubmit(_) | Mode::ConfirmReject(_) => &[("enter", "confirm"), ("esc", "back")],
        Mode::Help => &[("any key", "close")],
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

/// A rectangle of `width`×`height` in the middle of `area`.
fn center(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
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
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, app, &View { queue: "default" }))
            .expect("draws");
        terminal.backend().clone()
    }

    #[test]
    fn browsing_a_question_with_long_options() {
        insta::assert_snapshot!(screen(&app(&[("batch", EXAMPLE)]), 100, 30));
    }

    #[test]
    fn a_narrow_terminal_wraps_the_descriptions() {
        insta::assert_snapshot!(screen(&app(&[("batch", EXAMPLE)]), 64, 30));
    }

    #[test]
    fn the_list_shows_answer_states_and_the_markdown_is_styled() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "1Jnwhy");
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!(screen(&app, 90, 24));
    }

    #[test]
    fn editing_shows_the_field_and_its_counter() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "JJ");
        special(&mut app, KeyCode::Enter);
        keys(&mut app, "Ship the queue watcher and the new format.");
        insta::assert_snapshot!(screen(&app, 90, 24));
    }

    #[test]
    fn a_note_and_an_unlabelled_custom_entry_have_their_titles() {
        let mut app = app(&[(
            "plain",
            r#"{"asqr": 1, "questions": [{"id": "q", "text": "?", "kind": "single", "custom": true,
                "options": [{"id": "a", "label": "A"}]}]}"#,
        )]);
        keys(&mut app, "c");
        insta::assert_snapshot!("custom_entry", screen(&app, 70, 16));
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "n");
        insta::assert_snapshot!("note", screen(&app, 70, 16));
    }

    /// The colour of the counter, read from the editor's top border: the
    /// row where the "Answer" title starts.
    fn counter_colour(app: &App, marker: &str) -> Color {
        let buffer = screen(app, 90, 24).buffer().clone();
        let area = buffer.area;
        let row = |y: u16| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let border = (0..area.height)
            .find(|&y| row(y).contains("┌ Answer"))
            .expect("the editor is on screen");
        (0..area.width)
            .find(|&x| buffer[(x, border)].symbol() == marker)
            .map(|x| buffer[(x, border)].fg)
            .expect("the counter is in the border")
    }

    #[test]
    fn the_counter_colour_follows_the_length() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "JJ");
        special(&mut app, KeyCode::Enter);
        keys(&mut app, "a");
        assert_eq!(counter_colour(&app, "/"), Color::Green);

        keys(&mut app, &"b".repeat(125));
        assert_eq!(
            counter_colour(&app, "!"),
            Color::Yellow,
            "above the target of 120"
        );

        keys(&mut app, &"c".repeat(20));
        assert_eq!(counter_colour(&app, "!"), Color::Red, "above warn at 140");
    }

    #[test]
    fn submitting_asks_with_the_counts() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "1S");
        insta::assert_snapshot!(screen(&app, 90, 24));
    }

    #[test]
    fn a_message_shows_in_the_status_line() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "S");
        insta::assert_snapshot!(screen(&app, 90, 24));
    }

    #[test]
    fn rejecting_asks_for_a_reason() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "Xout of date");
        insta::assert_snapshot!(screen(&app, 90, 24));
    }

    #[test]
    fn the_help_lists_every_key() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "?");
        insta::assert_snapshot!(screen(&app, 90, 24));
    }

    #[test]
    fn the_session_list_shows_every_waiting_session() {
        let mut app = app(&[("batch", EXAMPLE), ("release", RELEASE)]);
        keys(&mut app, "Lj");
        insta::assert_snapshot!(screen(&app, 90, 24));
    }

    #[test]
    fn a_notice_shows_in_the_status_line() {
        let mut app = app(&[("release", RELEASE)]);
        app.notice("batch-01 collided with an unread result and was archived");
        insta::assert_snapshot!(screen(&app, 90, 24));
    }

    #[test]
    fn an_empty_queue_says_so() {
        insta::assert_snapshot!(screen(&App::default(), 80, 20));
    }

    #[test]
    fn a_small_terminal_says_what_it_needs() {
        insta::assert_snapshot!(screen(&App::default(), 40, 10));
    }

    #[test]
    fn a_long_session_scrolls_the_list_to_the_current_question() {
        let questions: Vec<String> = (1..=40)
            .map(|n| format!(r#"{{"id": "q{n}", "text": "Question {n}?", "kind": "text"}}"#))
            .collect();
        let json = format!(r#"{{"asqr": 1, "questions": [{}]}}"#, questions.join(","));
        let mut app = app(&[("long", &json)]);
        for _ in 0..30 {
            special(&mut app, KeyCode::Tab);
        }
        insta::assert_snapshot!(screen(&app, 80, 20));
    }
}
