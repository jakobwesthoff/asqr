// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frames drawn over the middle of the screen: the whole intro, the help
//! and the list of waiting sessions (spec section 7.2). They share the way
//! they take their place and their frame; what each shows inside is its
//! own widget.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

use super::header::intro_lines;
use super::panes::READING_WIDTH;
use super::parts::pointer;
use crate::tui::{App, Extent};

/// Clears the place of an overlay `width` columns wide and `height` rows
/// tall in the middle of `area`, and returns it. The layout never makes
/// it larger than `area`, so an overlay on a small screen gets what there
/// is.
pub(super) fn popup(area: Rect, width: u16, height: u16, buf: &mut Buffer) -> Rect {
    let area = area.centered(Constraint::Length(width), Constraint::Length(height));
    Clear.render(area, buf);
    area
}

/// The frame of an overlay: `title` at the top left and, where given,
/// `footer` at the bottom right, each with a blank on either side so it
/// stands apart from the border.
pub(super) fn overlay_block(title: &str, footer: Option<Line<'static>>) -> Block<'static> {
    let block = Block::bordered().title(format!(" {title} "));
    match footer {
        Some(footer) => {
            let mut spans = vec![Span::from(" ")];
            spans.extend(footer.spans);
            spans.push(Span::from(" "));
            // The footer's own style spans the blanks too, as it does the
            // whole line.
            block.title_bottom(Line::from(spans).style(footer.style).right_aligned())
        }
        None => block,
    }
}

/// The whole intro in a frame over the screen, from line `scroll` on, and
/// what of it fits. An intro longer than the frame scrolls, and the frame's
/// bottom edge says which lines show. The intro shows in patine's styles
/// without the header's dim.
pub(super) fn draw_intro(area: Rect, buf: &mut Buffer, intro: &str, scroll: usize) -> Extent {
    // The text keeps to the reading width, with a blank column inside each
    // border.
    let width = area.width.saturating_sub(4).min(READING_WIDTH + 4);
    let inner = usize::from(width.saturating_sub(2));
    let lines = intro_lines(intro, inner.saturating_sub(1));
    let wanted = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    // An intro taller than the screen gets the rows there are and scrolls
    // in them.
    let area = popup(area, width, wanted, buf);
    let page = Extent {
        shown: usize::from(area.height.saturating_sub(2)),
        total: lines.len(),
    };
    let scroll = scroll.min(page.last_start());

    let footer = page
        .position(scroll)
        .map(|position| Line::from(position).dim());
    Paragraph::new(lines)
        .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
        .block(overlay_block("Intro", footer))
        .render(area, buf);
    page
}

pub(super) fn draw_session_list(area: Rect, buf: &mut Buffer, app: &App, cursor: usize) {
    let area = popup(
        area,
        area.width.saturating_sub(4).min(80),
        app.sessions().len() as u16 + 2,
        buf,
    );
    let lines: Vec<Line> = app
        .sessions()
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let line = Line::from(vec![
                pointer(index == cursor),
                Span::from(format!(
                    "{}  ({}/{} answered)",
                    state.name(),
                    state.counts().answered,
                    state.session().questions.len()
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
        .block(overlay_block("Waiting sessions", None))
        .render(area, buf);
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Cell;
    use ratatui::style::Modifier;

    use super::super::test_support::{find, row, rows};
    use super::*;
    use crate::tui::SessionState;

    #[test]
    fn a_popup_clears_its_place_in_the_middle_and_keeps_within_the_area() {
        let area = Rect::new(0, 0, 10, 5);
        let mut buf = Buffer::filled(area, Cell::new("x"));

        assert_eq!(popup(area, 4, 3, &mut buf), Rect::new(3, 1, 4, 3));
        assert_eq!(
            rows(&buf),
            [
                "xxxxxxxxxx",
                "xxx    xxx",
                "xxx    xxx",
                "xxx    xxx",
                "xxxxxxxxxx"
            ]
        );
        assert_eq!(popup(area, 20, 9, &mut buf), area, "larger than the area");
    }

    #[test]
    fn an_overlay_frame_sets_its_title_and_footer_apart_by_a_blank() {
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        overlay_block("Keys", Some(Line::from("esc").dim())).render(area, &mut buf);

        assert_eq!(row(&buf, 0), "┌ Keys ────────────┐");
        assert_eq!(row(&buf, 2), "└───────────── esc ┘");
        let (x, y) = find(&buf, "esc");
        for x in x - 1..x + 4 {
            assert_eq!(buf[(x, y)].modifier, Modifier::DIM, "column {x}");
        }

        let mut buf = Buffer::empty(area);
        overlay_block("Keys", None).render(area, &mut buf);
        assert_eq!(row(&buf, 2), "└──────────────────┘");
    }

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

    #[test]
    fn the_session_list_counts_a_default_answer_and_not_one_short_of_its_minimum() {
        let mut app = App::default();
        let json = r#"{"asqr": 2, "questions": [
            {"id": "s", "text": "?", "kind": "single",
             "options": [{"id": "a", "label": "A", "default": true}]},
            {"id": "m", "text": "?", "kind": "multi", "min": 2,
             "options": [{"id": "a", "label": "A", "default": true},
                         {"id": "b", "label": "B"}]},
            {"id": "t", "text": "?", "kind": "text"}
        ]}"#;
        let session = serde_json::from_str(json).expect("parses");
        app.add(SessionState::new("batch", session, None));
        let area = Rect::new(0, 0, 40, 3);
        let mut buf = Buffer::empty(area);
        draw_session_list(area, &mut buf, &app, 0);

        assert_eq!(row(&buf, 1), "  │❯ batch  (1/3 answered)           │");
    }

    #[test]
    fn the_intro_frame_names_itself_and_dims_which_lines_show() {
        let intro = (1..=9)
            .map(|line| format!("Line {line}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let area = Rect::new(0, 0, 30, 7);
        let mut buf = Buffer::empty(area);
        let page = draw_intro(area, &mut buf, &intro, 99);

        assert_eq!(
            page,
            Extent {
                shown: 5,
                total: 17
            }
        );
        assert_eq!(row(&buf, 0), "  ┌ Intro ─────────────────┐");
        assert_eq!(row(&buf, 1), "  │ Line 7                 │");
        assert_eq!(row(&buf, 6), "  └─────────── 13–17 of 17 ┘");
        assert_eq!(buf[find(&buf, "Intro")].modifier, Modifier::empty());
        let (x, y) = find(&buf, "13–17");
        for x in x - 1..x + 12 {
            assert_eq!(buf[(x, y)].modifier, Modifier::DIM, "column {x}");
        }
        assert_eq!(buf[(x - 2, y)].modifier, Modifier::empty());
    }
}
