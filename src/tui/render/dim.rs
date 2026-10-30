// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The screen behind an overlay (ADR 84): dim, and without the focus
//! fill, so the overlay's own focus fill is the one on the screen.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};

use super::theme;

/// Dims every cell of `area` in `buf` and takes the focus fill off the
/// cells that have it. The cells of `image`, the place of a Kitty image,
/// keep their style: a placeholder's colour names its image, and a
/// terminal may draw a dim colour as another one.
pub(super) fn dim_behind(buf: &mut Buffer, area: Rect, image: Option<Rect>) {
    let fill = theme::focus_fill();
    let focus_colour = fill.fg.unwrap_or(Color::Reset);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if image.is_some_and(|image| image.contains(Position { x, y })) {
                continue;
            }
            let cell = &mut buf[(x, y)];
            // The focus fill is reversed text in the focus colour. The
            // submit button and a diff's changed lines are fills of their
            // own and stay, dim like the rest.
            if cell.modifier.contains(Modifier::REVERSED) && cell.fg == focus_colour {
                cell.modifier.remove(Modifier::REVERSED);
                cell.fg = Color::Reset;
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

    #[test]
    fn the_focus_fill_goes_and_other_fills_stay() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf.set_style(Rect::new(0, 0, 1, 1), theme::focus_fill());
        buf.set_style(Rect::new(1, 0, 1, 1), theme::button());
        buf.set_style(Rect::new(2, 0, 1, 1), Style::new().fg(Color::Yellow));

        let area = buf.area;
        dim_behind(&mut buf, area, None);

        assert_eq!(cell(&buf, 0), (Color::Reset, Modifier::DIM));
        assert_eq!(
            cell(&buf, 1),
            (
                Color::Cyan,
                Modifier::BOLD | Modifier::REVERSED | Modifier::DIM
            )
        );
        assert_eq!(cell(&buf, 2), (Color::Yellow, Modifier::DIM), "yellow text");
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
