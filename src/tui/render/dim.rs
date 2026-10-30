// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The screen behind an overlay (ADR 84, ADR 88): dim, and without the
//! fills of asqr's own, so the overlay's own focus fill is the one on the
//! screen.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};

use super::theme;

/// Dims every cell of `area` in `buf`. The focus fill goes with its
/// colour, and the other fills of asqr's own, the submit button and a
/// diff's changed lines, lose their reversal and stay as dim text in
/// their colour. Reversed cells that content brings keep their shape. The
/// cells of `image`, the place of a Kitty image, keep their style: a
/// placeholder's colour names its image, and a terminal may draw a dim
/// colour as another one.
pub(super) fn dim_behind(buf: &mut Buffer, area: Rect, image: Option<Rect>) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if image.is_some_and(|image| image.contains(Position { x, y })) {
                continue;
            }
            let cell = &mut buf[(x, y)];
            if theme::has_focus_fill(cell) {
                cell.modifier.remove(Modifier::REVERSED);
                cell.fg = Color::Reset;
            } else if theme::has_own_fill(cell) {
                cell.modifier.remove(Modifier::REVERSED);
            }
            cell.modifier.insert(Modifier::DIM);
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Style;

    use super::*;

    fn cell(buf: &Buffer, x: u16) -> (Color, Modifier) {
        let cell = &buf[(x, 0)];
        (cell.fg, cell.modifier)
    }

    #[test]
    fn dims_the_area_and_leaves_the_rest() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));

        dim_behind(&mut buf, Rect::new(0, 0, 4, 1), None);

        assert!(buf[(3, 0)].modifier.contains(Modifier::DIM));
        assert!(!buf[(0, 1)].modifier.contains(Modifier::DIM));
    }

    /// One cell in `style`, dimmed: its colour and modifiers.
    fn dimmed(style: Style) -> (Color, Modifier) {
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        buf.set_style(buf.area, style);
        let area = buf.area;
        dim_behind(&mut buf, area, None);
        cell(&buf, 0)
    }

    #[test]
    fn the_focus_fill_goes_with_its_colour() {
        assert_eq!(dimmed(theme::focus_fill()), (Color::Reset, Modifier::DIM));
    }

    #[test]
    fn the_submit_button_is_dim_text_in_its_colour() {
        assert_eq!(
            dimmed(theme::button()),
            (Color::Cyan, Modifier::BOLD | Modifier::DIM)
        );
    }

    #[test]
    fn the_changed_lines_of_a_diff_are_dim_text_in_their_colour() {
        let removed = dimmed(theme::error().reversed());
        let added = dimmed(theme::success().reversed());

        assert_eq!(removed, (Color::Red, Modifier::DIM));
        assert_eq!(added, (Color::Green, Modifier::DIM));
    }

    #[test]
    fn text_is_only_dimmed() {
        assert_eq!(
            dimmed(Style::new().fg(Color::Yellow)),
            (Color::Yellow, Modifier::DIM)
        );
    }

    /// Content an entry brings, such as ANSI art written as a glyph
    /// reversed over a colour, keeps its shape, as does the text cursor:
    /// none of them is reversed in the colour of one of asqr's fills.
    #[test]
    fn reversed_cells_that_are_not_asqrs_fills_keep_their_reversal() {
        for (what, style) in [
            ("an ANSI palette slot", Style::new().fg(Color::Indexed(1))),
            ("an RGB colour", Style::new().fg(Color::Rgb(9, 8, 7))),
            ("the text cursor", theme::cursor()),
        ] {
            let (_, modifier) = dimmed(style.reversed());

            assert!(modifier.contains(Modifier::REVERSED), "{what}");
        }
    }

    #[test]
    fn a_fill_in_the_cells_of_an_image_keeps_its_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        buf.set_style(buf.area, theme::button());

        let area = buf.area;
        dim_behind(&mut buf, area, Some(area));

        assert_eq!(
            cell(&buf, 0),
            (Color::Cyan, Modifier::BOLD | Modifier::REVERSED)
        );
    }

    #[test]
    fn the_cells_of_an_image_keep_their_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));

        let area = buf.area;
        dim_behind(&mut buf, area, Some(Rect::new(1, 0, 2, 1)));

        let dimmed: Vec<bool> = (0..4)
            .map(|x| buf[(x, 0)].modifier.contains(Modifier::DIM))
            .collect();
        assert_eq!(dimmed, [true, false, false, true]);
    }
}
