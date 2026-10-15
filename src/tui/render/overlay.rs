// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frames drawn over the middle of the screen: the whole intro and the list
//! of waiting sessions (spec section 7.2).

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

use super::panes::READING_WIDTH;
use super::text::field_lines;
use super::wrap::indented;
use crate::tui::{AnswerState, App, Extent};

/// The whole intro in a frame over the screen, from line `scroll` on, and
/// what of it fits. An intro longer than the frame scrolls, and the frame's
/// bottom edge says which lines show. The intro shows in patine's styles
/// without the header's dim.
pub(super) fn draw_intro(frame: &mut Frame, intro: &str, scroll: usize, area: Rect) -> Extent {
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

pub(super) fn draw_session_list(frame: &mut Frame, app: &App, cursor: usize, area: Rect) {
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

/// A rectangle of `width`×`height` in the middle of `area`, never larger
/// than `area`.
pub(super) fn center(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    area
}
