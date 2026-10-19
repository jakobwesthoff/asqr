// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What tests need to make images without a terminal, and to read Kitty
//! images back off a drawn buffer: where the placeholders are, what the
//! transmission says and which images the cells name.

use std::collections::BTreeSet;

use ::image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;
use ratatui_image::protocol::Protocol as Encoded;
use ratatui_image::protocol::kitty::Kitty;

use super::placeholders::{DIACRITICS, PLACEHOLDER};
use super::{Graphics, Images, Passthrough, Protocol};

impl Graphics {
    /// The Kitty protocol outside tmux, with cells of 10×20 pixels.
    pub(crate) const KITTY: Graphics = Graphics {
        protocol: Protocol::Kitty,
        ..Graphics::HALF_BLOCKS
    };
}

impl Images {
    /// Images in half blocks.
    pub(crate) fn half_blocks() -> Self {
        Images::new(Graphics::HALF_BLOCKS)
    }

    /// Images under the Kitty protocol, whose ids are 1, 2, 3 and so on.
    pub(crate) fn kitty() -> Self {
        Images::counting(Graphics::KITTY)
    }

    /// Like [`Images::kitty`], where tmux passes on what `passthrough`
    /// says.
    pub(crate) fn kitty_through(passthrough: Passthrough) -> Self {
        Images::counting(Graphics {
            passthrough,
            ..Graphics::KITTY
        })
    }

    /// Images for `graphics` whose Kitty ids are 1, 2, 3 and so on.
    pub(crate) fn counting(graphics: Graphics) -> Self {
        let mut last = 0;
        Images::with_source(
            graphics,
            Box::new(move || {
                last += 1;
                last
            }),
        )
    }

    /// Images for `graphics` whose Kitty ids come from `ids` in turn.
    pub(crate) fn with_ids(graphics: Graphics, ids: impl IntoIterator<Item = u32>) -> Self {
        let mut ids = ids.into_iter().collect::<Vec<_>>().into_iter();
        Images::with_source(
            graphics,
            Box::new(move || ids.next().expect("the test gave enough ids")),
        )
    }

    /// The Kitty ids of the images that exist.
    pub(crate) fn held(&self) -> Vec<u32> {
        self.book.live()
    }

    /// The Kitty ids the deletes after a frame would name now.
    pub(crate) fn dropped(&self) -> Vec<u32> {
        self.book.after_frame().ids().to_vec()
    }

    /// The Kitty ids the deletes on quitting would name now.
    pub(crate) fn drained(&self) -> Vec<u32> {
        self.book.drain().ids().to_vec()
    }
}

/// A red picture of 20×40 pixels, which takes 2×2 cells of 10×20.
pub(crate) fn red() -> DynamicImage {
    ::image::RgbaImage::from_pixel(20, 40, ::image::Rgba([255, 0, 0, 255])).into()
}

/// Where `buf` holds a Kitty placeholder, row by row.
pub(crate) fn placeholder_cells(buf: &Buffer) -> Vec<(u16, u16)> {
    let area = buf.area;
    (area.top()..area.bottom())
        .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
        .filter(|&position| buf[position].symbol().contains(PLACEHOLDER))
        .collect()
}

/// The command that sends a Kitty image in `buf`, up to the placeholder
/// that follows it in the same cell.
pub(crate) fn transmission(buf: &Buffer) -> &str {
    let symbol = buf
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .find(|symbol| symbol.contains("\x1b_G") && symbol.contains(PLACEHOLDER))
        .expect("a Kitty image is sent");
    &symbol[..symbol.find(PLACEHOLDER).expect("a placeholder follows")]
}

/// The Kitty ids of the images `buf` shows: the low three bytes from the
/// foreground colour of the placeholders, the high byte from the third
/// mark, which the first placeholder of each row carries (ratatui-image
/// 12.0.0-rc.3 `protocol/kitty.rs` l.169-213).
pub(crate) fn kitty_ids_on_screen(buf: &Buffer) -> BTreeSet<u32> {
    buf.content()
        .iter()
        .filter_map(|cell| {
            let symbol = cell.symbol();
            let marks: Vec<char> = symbol[symbol.find(PLACEHOLDER)?..]
                .chars()
                .skip(1)
                .collect();
            let [_, _, high] = marks[..] else {
                return None;
            };
            Some(full_id(cell.fg, high))
        })
        .collect()
}

/// The number the diacritic `mark` stands for in a placeholder.
fn number(mark: char) -> u16 {
    let index = DIACRITICS
        .iter()
        .position(|known| *known == mark)
        .expect("a mark of the Kitty spec's table");
    u16::try_from(index).expect("the table has 297 marks")
}

/// The id of the image a placeholder of colour `fg` with the high byte
/// mark `high` names.
fn full_id(fg: Color, high: char) -> u32 {
    let Color::Rgb(red, green, blue) = fg else {
        panic!("a placeholder names its image in RGB: {fg:?}");
    };
    let high = u8::try_from(number(high)).expect("a high byte mark stands for a byte");
    u32::from_be_bytes([high, red, green, blue])
}

/// Where the terminal puts a Kitty placeholder: its row and column within
/// the image, and the image's full id.
pub(crate) type Place = (u16, u16, u32);

/// Where the terminal places each Kitty placeholder of `buf`, row by row,
/// or `None` where it cannot tell and draws nothing.
///
/// It follows the rules of the Kitty spec for what asqr writes, and no
/// further: a placeholder with the marks for its row, its column and the
/// id's high byte stands on its own, and one without marks takes its row,
/// the column after and the high byte from a placeholder of the same
/// colour directly on its left. asqr never sets an underline colour, so
/// the placement id it would carry is left out.
/// <https://sw.kovidgoyal.net/kitty/graphics-protocol/#unicode-placeholders>
pub(crate) fn kitty_positions(buf: &Buffer) -> Vec<((u16, u16), Option<Place>)> {
    let area = buf.area;
    let mut placed = Vec::new();
    for y in area.top()..area.bottom() {
        // The placeholder left of the current cell: its colour, and where
        // the terminal placed it.
        let mut left: Option<(Color, Option<Place>)> = None;
        for x in area.left()..area.right() {
            let cell = &buf[(x, y)];
            let symbol = cell.symbol();
            let Some(start) = symbol.find(PLACEHOLDER) else {
                left = None;
                continue;
            };
            let marks: Vec<char> = symbol[start..].chars().skip(1).collect();
            let position = match marks[..] {
                [row, column, high] => Some((number(row), number(column), full_id(cell.fg, high))),
                [] => left
                    .filter(|(colour, _)| *colour == cell.fg)
                    .and_then(|(_, position)| position)
                    .map(|(row, column, id)| (row, column + 1, id)),
                _ => panic!("asqr writes no such cell: {symbol:?}"),
            };
            left = Some((cell.fg, position));
            placed.push(((x, y), position));
        }
    }
    placed
}

/// Draws a transparent image under the Kitty `id` into `area` of `buf`,
/// through ratatui-image alone.
pub(crate) fn draw_kitty(buf: &mut Buffer, id: u32, area: Rect) {
    let picture = DynamicImage::new_rgba8(1, 1);
    let kitty = Kitty::new(picture, area.as_size(), id, false, false, false)
        .expect("a picture of one pixel encodes");
    ratatui_image::Image::new(&Encoded::Kitty(kitty)).render(area, buf);
}

#[test]
fn the_ids_of_kitty_images_are_read_from_the_screen() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
    draw_kitty(&mut buf, 0x0102_0304, Rect::new(0, 0, 2, 2));
    draw_kitty(&mut buf, 0xFF00_00FF, Rect::new(3, 0, 1, 2));

    assert_eq!(
        kitty_ids_on_screen(&buf),
        BTreeSet::from([0x0102_0304, 0xFF00_00FF])
    );
    assert_eq!(
        placeholder_cells(&buf),
        [(0, 0), (1, 0), (3, 0), (0, 1), (1, 1), (3, 1)]
    );
}

/// A row of placeholders written by hand: `(fg, marks)` per cell, where
/// `None` is a cell of text.
fn row_of(cells: &[Option<(Color, &[u16])>]) -> Buffer {
    let width = u16::try_from(cells.len()).expect("a short row");
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    for (x, cell) in (0..).zip(cells) {
        let Some((fg, marks)) = cell else {
            buf[(x, 0)].set_symbol("x");
            continue;
        };
        let mut symbol = PLACEHOLDER.to_string();
        symbol.extend(marks.iter().map(|&number| DIACRITICS[usize::from(number)]));
        buf[(x, 0)].set_symbol(&symbol).set_fg(*fg);
    }
    buf
}

/// The terminal places a placeholder with all three marks by them, and a
/// bare one after a placeholder of its colour on its left. A bare one
/// after text or after another image has no place.
#[test]
fn the_terminal_s_rules_place_the_placeholders() {
    let ours = Color::Rgb(0, 0, 1);
    let theirs = Color::Rgb(0, 0, 2);
    let buf = row_of(&[
        Some((ours, &[0, 0, 42])),
        Some((ours, &[])),
        Some((ours, &[])),
        None,
        Some((ours, &[])),
        Some((theirs, &[0, 5, 0])),
        Some((ours, &[])),
    ]);

    let id = 0x2A00_0001;
    assert_eq!(
        kitty_positions(&buf),
        [
            ((0, 0), Some((0, 0, id))),
            ((1, 0), Some((0, 1, id))),
            ((2, 0), Some((0, 2, id))),
            ((4, 0), None),
            ((5, 0), Some((0, 5, 2))),
            ((6, 0), None),
        ]
    );
}

/// ratatui-image and the mending write placeholders with none or all
/// three marks; any other cell would need rules the helper leaves out.
#[test]
#[should_panic(expected = "asqr writes no such cell")]
fn a_placeholder_with_one_or_two_marks_is_refused() {
    let buf = row_of(&[Some((Color::Rgb(0, 0, 1), &[0]))]);

    kitty_positions(&buf);
}
