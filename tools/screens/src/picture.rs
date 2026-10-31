// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A picture painted where asqr shows an image entry, as a terminal with
//! graphics shows it.
//!
//! The tool draws images as half blocks, sized for its own cell. asqr
//! works out the cells an image takes from the picture's pixels, the cell
//! size and the room the viewer gives it, the same for every protocol
//! (`Image::lay_out` in `src/graphics/image.rs`), so the half blocks take
//! exactly the cells the image takes in a terminal with graphics. Those
//! cells are cleared to the terminal's background, and the picture is
//! painted into their pixels as ratatui-image sends it under the Kitty
//! protocol: scaled into them with its proportions kept, at their top
//! left, and padded with transparent pixels that show the background.

use anyhow::{Result, bail};
use image::imageops::FilterType;
use image::{DynamicImage, RgbImage};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::fonts::{CELL_HEIGHT, CELL_WIDTH};
use crate::raster::PADDING;

/// The cells half blocks drew an image into: every cell of its size gets
/// an RGB foreground and background from ratatui-image
/// (`protocol/halfblocks/primitive.rs`, 12.0.0-rc.3), while asqr draws its
/// own screen in the terminal's named colours. An `ansi` entry's RGB
/// colours and Talqi on the empty queue are RGB too, so a screen with
/// either has no single rectangle of them and is refused.
pub fn image_cells(buffer: &Buffer) -> Result<Rect> {
    let area = buffer.area;
    let cells: Vec<(u16, u16)> = (area.top()..area.bottom())
        .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| matches!(buffer[(x, y)].bg, Color::Rgb(..)))
        .collect();
    let (Some(left), Some(right), Some(top), Some(bottom)) = (
        cells.iter().map(|&(x, _)| x).min(),
        cells.iter().map(|&(x, _)| x).max(),
        cells.iter().map(|&(_, y)| y).min(),
        cells.iter().map(|&(_, y)| y).max(),
    ) else {
        bail!("the scene names an `image`, and no image shows on the screen");
    };
    let rectangle = Rect::new(left, top, right - left + 1, bottom - top + 1);
    if cells.len() != usize::from(rectangle.width) * usize::from(rectangle.height) {
        bail!(
            "the cells with an RGB background, which only an image has on asqr's screen, \
             do not form one rectangle: {} cells within {} by {}",
            cells.len(),
            rectangle.width,
            rectangle.height
        );
    }
    Ok(rectangle)
}

/// Clears `cells` of `buffer` to blanks in the terminal's own colours, which
/// a terminal shows under an image's transparent pixels: the context pane
/// has no fill of its own.
pub fn clear_cells(buffer: &mut Buffer, cells: Rect) {
    for y in cells.top()..cells.bottom() {
        for x in cells.left()..cells.right() {
            buffer[(x, y)].reset();
        }
    }
}

/// Paints `picture` into the pixels of `cells` of `image`, which the
/// raster drew from a buffer of those cells.
///
/// ratatui-image scales a Kitty image with `DynamicImage::resize` into its
/// cells' pixels, proportions kept, and overlays it at their top left on
/// transparent pixels (`Resize::resize`), with asqr's filter, `Triangle`
/// (`SCALING` in `src/graphics/image.rs`). A terminal composes the
/// result over its background, here as the raster blends glyphs: by the
/// alpha, in sRGB values.
///
/// asqr sends a picture it enlarges three times or more at a divisor of the
/// cell's pixels for the terminal to enlarge (ADR 80); this paints it
/// scaled to the cells' full pixels, as the repository does not name the
/// filter a terminal enlarges with.
pub fn paint(image: &mut RgbImage, cells: Rect, picture: &DynamicImage) {
    let scaled = picture
        .resize(
            u32::from(cells.width) * CELL_WIDTH,
            u32::from(cells.height) * CELL_HEIGHT,
            FilterType::Triangle,
        )
        .to_rgba8();
    let left = PADDING + u32::from(cells.x) * CELL_WIDTH;
    let top = PADDING + u32::from(cells.y) * CELL_HEIGHT;
    for (x, y, pixel) in scaled.enumerate_pixels() {
        let alpha = f32::from(pixel[3]) / 255.0;
        let under = image.get_pixel_mut(left + x, top + y);
        for channel in 0..3 {
            let over = f32::from(pixel[channel]);
            let below = f32::from(under[channel]);
            under[channel] = (over * alpha + below * (1.0 - alpha)).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_rgb_cells(cells: &[(u16, u16)]) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
        for &(x, y) in cells {
            buffer[(x, y)].set_bg(Color::Rgb(1, 2, 3));
        }
        buffer
    }

    fn error_of(buffer: &Buffer) -> String {
        match image_cells(buffer) {
            Ok(cells) => format!("found {cells:?}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn the_image_is_the_rectangle_of_cells_with_an_rgb_background() {
        let buffer = with_rgb_cells(&[(2, 1), (3, 1), (2, 2), (3, 2)]);

        assert_eq!(image_cells(&buffer).ok(), Some(Rect::new(2, 1, 2, 2)));
    }

    #[test]
    fn a_screen_without_an_image_is_refused() {
        let error = error_of(&with_rgb_cells(&[]));

        assert!(error.contains("no image shows"), "{error}");
    }

    #[test]
    fn rgb_cells_that_are_no_rectangle_are_refused() {
        let error = error_of(&with_rgb_cells(&[(2, 1), (3, 1), (2, 2)]));

        assert!(error.contains("do not form one rectangle"), "{error}");
    }

    #[test]
    fn cleared_cells_take_the_terminal_s_colours() {
        let mut buffer = with_rgb_cells(&[(2, 1)]);
        buffer[(2, 1)].set_symbol("▀").set_fg(Color::Rgb(9, 9, 9));

        clear_cells(&mut buffer, Rect::new(2, 1, 1, 1));

        let cell = &buffer[(2, 1)];
        assert_eq!(
            (cell.symbol(), cell.fg, cell.bg),
            (" ", Color::Reset, Color::Reset)
        );
    }

    #[test]
    fn a_half_transparent_pixel_blends_with_what_lies_below() {
        let mut image = RgbImage::from_pixel(100, 100, image::Rgb([0, 0, 0]));
        let picture = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            15,
            39,
            image::Rgba([200, 100, 50, 128]),
        ));

        paint(&mut image, Rect::new(0, 0, 1, 1), &picture);

        assert_eq!(image.get_pixel(PADDING + 7, PADDING + 20).0, [100, 50, 25]);
    }
}
