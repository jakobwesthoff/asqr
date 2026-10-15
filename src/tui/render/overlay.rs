// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frames drawn over the middle of the screen: the whole intro and the list
//! of waiting sessions (spec section 7.2).

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

use super::panes::READING_WIDTH;
use super::parts::pointer;
use super::text::field_lines;
use super::wrap::indented;
use crate::tui::{AnswerState, App, Extent};

/// The whole intro in a frame over the screen, from line `scroll` on, and
/// what of it fits. An intro longer than the frame scrolls, and the frame's
/// bottom edge says which lines show. The intro shows in patine's styles
/// without the header's dim.
pub(super) fn draw_intro(area: Rect, buf: &mut Buffer, intro: &str, scroll: usize) -> Extent {
    // The text keeps to the reading width, with a blank column inside each
    // border.
    let width = area.width.saturating_sub(4).min(READING_WIDTH + 4);
    let inner = usize::from(width.saturating_sub(2));
    let lines: Vec<Line<'static>> = field_lines(intro, inner.saturating_sub(2))
        .into_iter()
        .map(|line| indented(line, 1))
        .collect();
    let wanted = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    // The layout never makes the frame larger than `area`, so an intro
    // taller than the screen gets the rows there are and scrolls in them.
    let area = area.centered(Constraint::Length(width), Constraint::Length(wanted));
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
    Clear.render(area, buf);
    Paragraph::new(lines)
        .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
        .block(block)
        .render(area, buf);
    page
}

pub(super) fn draw_session_list(area: Rect, buf: &mut Buffer, app: &App, cursor: usize) {
    let area = area.centered(
        Constraint::Length(area.width.saturating_sub(4).min(80)),
        Constraint::Length(app.sessions().len() as u16 + 2),
    );
    Clear.render(area, buf);
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
            let line = Line::from(vec![
                pointer(index == cursor),
                Span::from(format!(
                    "{}  ({answered}/{} answered)",
                    state.name(),
                    session.questions.len()
                )),
            ]);
            if index == cursor {
                line.reversed()
            } else {
                line
            }
        })
        .collect();
    let visible = area.height.saturating_sub(2) as usize;
    let scroll = (cursor + 1).saturating_sub(visible);
    Paragraph::new(lines)
        .scroll((scroll as u16, 0))
        .block(Block::bordered().title(" Waiting sessions "))
        .render(area, buf);
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::super::test_support::{find, row};
    use super::*;
    use crate::tui::SessionState;

    #[test]
    fn the_session_list_names_each_session_and_reverses_the_one_under_the_cursor() {
        let mut app = App::default();
        for (id, title) in [("titled", r#""title": "Release","#), ("untitled", "")] {
            let json = format!(
                r#"{{"asqr": 2, {title} "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#
            );
            let session = serde_json::from_str(&json).expect("parses");
            app.add(SessionState::new(id, session, None));
        }
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        draw_session_list(area, &mut buf, &app, 1);

        assert_eq!(row(&buf, 1), "  │  Release  (0/1 answered)         │");
        assert_eq!(row(&buf, 2), "  │❯ untitled  (0/1 answered)        │");
        assert!(
            buf[find(&buf, "❯ untitled")]
                .modifier
                .contains(Modifier::REVERSED)
        );
        assert_eq!(buf[find(&buf, "Release")].modifier, Modifier::empty());
    }
}
