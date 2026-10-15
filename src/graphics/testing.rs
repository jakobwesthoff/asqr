// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What tests need to make images without a terminal, and to read Kitty
//! images back off a drawn buffer: where the placeholders are, what the
//! transmission says and which images the cells name.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use ::image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;
use ratatui_image::protocol::Protocol as Encoded;
use ratatui_image::protocol::kitty::Kitty;

use super::{Graphics, Images, Protocol};

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
        self.book.borrow().held()
    }
}

/// A red picture of 20×40 pixels, which takes 2×2 cells of 10×20.
pub(crate) fn red() -> DynamicImage {
    ::image::RgbaImage::from_pixel(20, 40, ::image::Rgba([255, 0, 0, 255])).into()
}

/// The character the Kitty protocol reserves for image placeholders.
pub(crate) const PLACEHOLDER: char = '\u{10EEEE}';

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
/// 12.0.0-rc.2 `protocol/kitty.rs` l.169-213).
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
            let Color::Rgb(red, green, blue) = cell.fg else {
                panic!("a placeholder names its image in RGB: {:?}", cell.fg);
            };
            let high = high_bytes()[&high];
            Some(u32::from_be_bytes([high, red, green, blue]))
        })
        .collect()
}

/// The byte each diacritic stands for as the third mark of a
/// placeholder, which carries the high byte of the image's id.
/// ratatui-image keeps its table private, so this draws one cell of an
/// image with each high byte and reads the mark back.
fn high_bytes() -> &'static HashMap<char, u8> {
    static TABLE: OnceLock<HashMap<char, u8>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..=u8::MAX)
            .map(|high| {
                let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
                draw_kitty(&mut buf, u32::from(high) << 24, Rect::new(0, 0, 1, 1));
                let mark = buf[(0, 0)].symbol().chars().last().expect("marks");
                (mark, high)
            })
            .collect()
    })
}

/// Draws a transparent image under the Kitty `id` into `area` of `buf`,
/// through ratatui-image alone.
fn draw_kitty(buf: &mut Buffer, id: u32, area: Rect) {
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
