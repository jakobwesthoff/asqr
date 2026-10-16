// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frames drawn over the middle of the screen: the whole intro, the help
//! and the list of waiting sessions (spec section 7.2). They share the way
//! they take their place and their frame; what each shows inside is its
//! own widget.

use patine::QuoteStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, BorderType, Clear, HighlightSpacing, List, ListState, Padding, Paragraph,
    StatefulWidget, Widget,
};

use super::panes::READING_WIDTH;
use super::parts::POINTER;
use super::text::field_lines;
use super::theme;
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

/// The frame of an overlay: a rounded border in the primary colour,
/// `title` in bold at the top left and, where given, `footer` dim at the
/// bottom right, each with a blank on either side so it stands apart from
/// the border. A blank column inside each side keeps what the overlay shows
/// off the border. The title and the footer stand on the border and take
/// its colour.
pub(super) fn overlay_block(title: &str, footer: Option<&str>) -> Block<'static> {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::primary())
        .title(Line::styled(format!(" {title} "), theme::title()))
        .padding(Padding::horizontal(1));
    match footer {
        Some(footer) => block
            .title_bottom(Line::styled(format!(" {footer} "), theme::secondary()).right_aligned()),
        None => block,
    }
}

/// The whole intro in a frame over the screen, from line `scroll` on, and
/// what of it fits. An intro longer than the frame scrolls, and the frame's
/// bottom edge says which lines show. The intro shows in patine's styles,
/// with its quotes in `quote`.
pub(super) fn draw_intro(
    area: Rect,
    buf: &mut Buffer,
    intro: &str,
    scroll: usize,
    quote: QuoteStyle,
) -> Extent {
    // The text keeps to the reading width inside the frame.
    let width = area.width.saturating_sub(4).min(READING_WIDTH + 4);
    let inner = overlay_block("Intro", None)
        .inner(Rect::new(0, 0, width, 2))
        .width;
    let lines = field_lines(intro, usize::from(inner), quote);
    let wanted = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    // An intro taller than the screen gets the rows there are and scrolls
    // in them.
    let area = popup(area, width, wanted, buf);
    let page = Extent {
        shown: usize::from(area.height.saturating_sub(2)),
        total: lines.len(),
    };
    let scroll = scroll.min(page.last_start());

    let footer = page.position(scroll);
    Paragraph::new(lines)
        .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
        .block(overlay_block("Intro", footer.as_deref()))
        .render(area, buf);
    page
}

/// The waiting sessions in a frame over the screen, the one under `cursor`
/// filled. A list taller than the frame scrolls so the cursor's session
/// shows.
pub(super) fn draw_session_list(area: Rect, buf: &mut Buffer, app: &App, cursor: usize) {
    let area = popup(
        area,
        area.width.saturating_sub(4).min(80),
        app.sessions().len() as u16 + 2,
        buf,
    );
    let items = app.sessions().iter().map(|state| {
        format!(
            "{}  ({}/{} answered)",
            state.name(),
            state.counts().answered,
            state.session().questions.len()
        )
    });
    // Nothing keeps the list's scroll from one draw to the next: the list
    // scrolls just far enough for the cursor's session, which a fresh
    // state yields on every draw.
    let mut state = ListState::default().with_selected(Some(cursor));
    StatefulWidget::render(
        List::new(items)
            .highlight_symbol(POINTER)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(theme::focus_fill())
            .block(overlay_block("Waiting sessions", None)),
        area,
        buf,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Cell;
    use ratatui::style::{Color, Modifier};

    use super::super::patine_adapter::quote_style;
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
    fn an_overlay_frame_is_rounded_and_primary_with_a_bold_title_and_a_dim_footer() {
        let area = Rect::new(0, 0, 20, 4);
        let mut buf = Buffer::empty(area);
        let block = overlay_block("Keys", Some("esc"));
        assert_eq!(
            block.inner(area),
            Rect::new(2, 1, 16, 2),
            "a blank column inside"
        );
        block.render(area, &mut buf);

        assert_eq!(row(&buf, 0), "╭ Keys ────────────╮");
        assert_eq!(row(&buf, 1), "│                  │");
        assert_eq!(row(&buf, 3), "╰───────────── esc ╯");
        for corner in [(0, 0), (19, 0), (0, 3), (19, 3), (0, 1), (5, 3)] {
            assert_eq!(buf[corner].fg, Color::Cyan, "border at {corner:?}");
        }
        // The title and the footer stand on the border and keep its colour.
        let (x, y) = find(&buf, "Keys");
        for x in x - 1..x + 5 {
            let cell = &buf[(x, y)];
            assert_eq!(
                (cell.fg, cell.modifier),
                (Color::Cyan, Modifier::BOLD),
                "title column {x}"
            );
        }
        let (x, y) = find(&buf, "esc");
        for x in x - 1..x + 4 {
            let cell = &buf[(x, y)];
            assert_eq!(
                (cell.fg, cell.modifier),
                (Color::Cyan, Modifier::DIM),
                "footer column {x}"
            );
        }

        let mut buf = Buffer::empty(area);
        overlay_block("Keys", None).render(area, &mut buf);
        assert_eq!(row(&buf, 3), "╰──────────────────╯");
    }

    #[test]
    fn the_session_list_names_each_session_and_fills_the_one_under_the_cursor() {
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

        assert_eq!(
            row(&buf, 0),
            format!("  ╭ Waiting sessions {}╮", "─".repeat(16))
        );
        assert_eq!(row(&buf, 1), "  │   Release  (0/1 answered)        │");
        assert_eq!(row(&buf, 2), "  │ ❯ untitled  (0/1 answered)       │");
        // The fill takes the whole row inside the frame's blank columns.
        for x in 4..36 {
            let cell = &buf[(x, 2)];
            assert_eq!(
                (cell.fg, cell.modifier),
                (Color::Yellow, Modifier::REVERSED),
                "column {x}"
            );
        }
        assert_eq!(buf[(3, 2)].modifier, Modifier::empty(), "the blank column");
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

        assert_eq!(row(&buf, 1), "  │ ❯ batch  (1/3 answered)          │");
    }

    #[test]
    fn the_intro_frame_names_itself_and_dims_which_lines_show() {
        let intro = (1..=9)
            .map(|line| format!("Line {line}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let area = Rect::new(0, 0, 30, 7);
        let mut buf = Buffer::empty(area);
        let page = draw_intro(area, &mut buf, &intro, 99, quote_style(false));

        assert_eq!(
            page,
            Extent {
                shown: 5,
                total: 17
            }
        );
        assert_eq!(row(&buf, 0), "  ╭ Intro ─────────────────╮");
        assert_eq!(row(&buf, 1), "  │ Line 7                 │");
        assert_eq!(row(&buf, 6), "  ╰─────────── 13–17 of 17 ╯");
        assert_eq!(buf[find(&buf, "Intro")].modifier, Modifier::BOLD);
        let (x, y) = find(&buf, "13–17");
        for x in x - 1..x + 12 {
            assert_eq!(buf[(x, y)].modifier, Modifier::DIM, "column {x}");
        }
        assert_eq!(buf[(x - 2, y)].modifier, Modifier::empty());
    }
}
