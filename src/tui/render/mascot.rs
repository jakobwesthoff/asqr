// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Talqi, asqr's mascot, as the empty queue shows it (spec section 7.1,
//! ADR 39): the pictures built into asqr, the cells a picture takes, and
//! the picture scaled and encoded for the terminal's graphics protocol.

use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, RgbaImage};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect, Size};
use ratatui_image::FontSize;
use ratatui_image::picker::{Picker, ProtocolType};

use super::images::Image;

/// The most columns and rows the picture takes. 44×22 cells are what a
/// square picture takes in half blocks, one pixel wide and two tall each,
/// and Jakob chose to draw the real image at about that size rather than
/// stretch it over the empty screen.
const MAX_COLUMNS: u16 = 44;
const MAX_ROWS: u16 = 22;

/// Which picture of Talqi the empty queue shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Which {
    /// Talqi awake: asqr started and no session came in yet.
    Base,
    /// Talqi asleep: sessions came in and the queue is empty again.
    Sleeping,
}

impl Which {
    /// The picture as built into asqr: 768 px lossy WebP, made with
    /// `cwebp` from the 1536 px originals under `assets/`, which the
    /// published crate leaves out.
    fn webp(self) -> &'static [u8] {
        match self {
            Which::Base => include_bytes!("mascot/talqi-awake.webp"),
            Which::Sleeping => include_bytes!("mascot/talqi-asleep.webp"),
        }
    }
}

/// The cells of `room` the picture takes for cells of `font`'s size in
/// pixels: as large as a square fits into at most [`MAX_COLUMNS`] by
/// [`MAX_ROWS`] cells of `room`, centred horizontally, at `room`'s top.
///
/// ratatui-image neither enlarges a picture with `Resize::Fit` nor centres
/// it, and draws from its area's top-left corner (ratatui-image 11.1.0
/// `lib.rs` l.526-533), so asqr works out the cells itself. A square of
/// that side rarely fills whole cells on both axes; the axis it does not
/// fill is rounded up to whole cells, and the image protocol pads the
/// rest transparently.
pub(super) fn mascot_rect(room: Rect, font: FontSize) -> Rect {
    let side = picture_side(room.width.min(MAX_COLUMNS), room.height.min(MAX_ROWS), font);
    let cells = |pixels: u16| {
        u16::try_from(side.div_ceil(u32::from(pixels)))
            .expect("no more cells than the box the side was taken from")
    };
    Rect {
        height: cells(font.height),
        ..room.centered_horizontally(Constraint::Length(cells(font.width)))
    }
}

/// The side in pixels of the largest square that fits into `columns` by
/// `rows` cells of `font`'s size.
///
/// For a rect [`mascot_rect`] made, this is the side it was made from: one
/// of its axes holds the side exactly, and the other was rounded up.
fn picture_side(columns: u16, rows: u16, font: FontSize) -> u32 {
    let width = u32::from(columns) * u32::from(font.width);
    let height = u32::from(rows) * u32::from(font.height);
    width.min(height)
}

/// `picture` scaled to `width`×`height` pixels with Lanczos3, its colours
/// weighted by their alpha.
///
/// The image crate resizes each channel on its own, so the colour a
/// transparent pixel stores would bleed into the edge of its opaque
/// neighbours. Talqi's transparent pixels store black, and lossy WebP may
/// store any colour there, either of which leaves a fringe. With every
/// colour multiplied by its alpha before the resize and divided by the new
/// alpha after it, a transparent pixel contributes nothing.
pub(super) fn scaled(picture: &DynamicImage, width: u32, height: u32) -> RgbaImage {
    let mut picture = picture.to_rgba32f();
    for pixel in picture.pixels_mut() {
        let [red, green, blue, alpha] = pixel.0;
        pixel.0 = [red * alpha, green * alpha, blue * alpha, alpha];
    }
    let mut resized = imageops::resize(&picture, width, height, FilterType::Lanczos3);
    for pixel in resized.pixels_mut() {
        // Lanczos3 overshoots at sharp edges, so alpha can leave 0..=1.
        let [red, green, blue, alpha] = pixel.0;
        let alpha = alpha.clamp(0.0, 1.0);
        pixel.0 = if alpha <= 0.0 {
            [0.0; 4]
        } else {
            let colour = |premultiplied: f32| (premultiplied / alpha).clamp(0.0, 1.0);
            [colour(red), colour(green), colour(blue), alpha]
        };
    }
    DynamicImage::ImageRgba32F(resized).into_rgba8()
}

/// Talqi for one terminal: the picture last drawn, kept encoded until
/// another picture or another size is asked for. It is the state of
/// [`EmptyQueue`](super::empty::EmptyQueue).
pub(super) struct Mascot {
    picker: Picker,
    /// The picture last drawn, the cells it was scaled for, and the
    /// picture encoded for `picker`'s protocol.
    ready: Option<(Which, Size, Image)>,
}

impl Mascot {
    /// Talqi for a terminal whose graphics protocol `picker` speaks.
    pub(super) fn new(picker: Picker) -> Self {
        Mascot {
            picker,
            ready: None,
        }
    }

    /// The size in pixels of the cells the picture is laid out in, or
    /// `None` where it cannot be drawn.
    pub(super) fn font(&self) -> Option<FontSize> {
        match self.picker.protocol_type() {
            // TODO: Draw Talqi in half blocks of its own colours where no
            // image protocol is available (todo
            // 01M507H3TNBASG8PKRN0C7YW2Q). ratatui-image's half blocks drop
            // the alpha channel and would draw the transparent background
            // as coloured blocks, so the sentence stands alone until then.
            ProtocolType::Halfblocks => None,
            ProtocolType::Kitty | ProtocolType::Sixel | ProtocolType::Iterm2 => {
                Some(self.picker.font_size())
            }
        }
    }

    /// Draws `which` into `area` of `buf`, an area [`mascot_rect`] made.
    ///
    /// The picture is decoded, scaled and encoded only when `which` or the
    /// area's size differs from the last draw. Scaled to fill the area on
    /// one axis, it passes `Resize::Fit` unchanged (ratatui-image 11.1.0
    /// `lib.rs` l.470-480). `Resize::Scale` would resize it once more,
    /// without regard to alpha, and bring back the fringe [`scaled`]
    /// avoids.
    pub(super) fn draw(&mut self, which: Which, area: Rect, buf: &mut Buffer) {
        let size = area.as_size();
        let kept =
            |&(shown, scaled_for, _): &(Which, Size, Image)| shown == which && scaled_for == size;
        if !self.ready.as_ref().is_some_and(kept) {
            let picture = image::load_from_memory_with_format(which.webp(), ImageFormat::WebP)
                .expect("the pictures built into asqr decode");
            let side = picture_side(area.width, area.height, self.picker.font_size());
            let picture = scaled(&picture, side, side);
            let image = Image::from_picture(&self.picker, picture.into());
            self.ready = Some((which, size, image));
        }
        let (.., image) = self.ready.as_mut().expect("made above when missing");
        image.draw(area, buf, false);
    }

    /// Drops the encoded picture, so it is sent to the terminal again when
    /// it is next drawn. The Kitty protocol sends a picture's data only on
    /// its first draw, and tmux drops that transfer while its pane is
    /// hidden (ADR 24).
    pub(super) fn forget(&mut self) {
        self.ready = None;
    }

    /// The picture last drawn and still kept.
    #[cfg(test)]
    pub(super) fn shown(&self) -> Option<Which> {
        self.ready.as_ref().map(|(which, ..)| *which)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use image::Rgba;
    use ratatui::style::Color;
    use ratatui_image::Resize;

    use super::*;

    /// The cells of Jakob's Ghostty (asqr.log, 2026-10-15) and those
    /// `Picker::halfblocks` assumes.
    const GHOSTTY: FontSize = FontSize::new(15, 39);
    const HALFBLOCKS: FontSize = FontSize::new(10, 20);

    /// A picker that speaks the Kitty protocol, with cells of 10×20 pixels.
    pub(in super::super) fn kitty() -> Picker {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        picker
    }

    /// The cell that carries the Kitty command which sends an image, if a
    /// draw into `buf` sent one.
    pub(in super::super) fn transmission(buf: &Buffer) -> Option<&str> {
        buf.content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .find(|symbol| symbol.contains("\x1b_G"))
    }

    #[test]
    fn the_picture_takes_at_most_44_by_22_cells_of_a_square() {
        let room = Rect::new(0, 3, 100, 40);

        // 44 columns of 15 px are 660 px, which take 17 rows of 39 px.
        assert_eq!(mascot_rect(room, GHOSTTY), Rect::new(28, 3, 44, 17));
        // 44 columns of 10 px are 440 px, as are 22 rows of 20 px.
        assert_eq!(mascot_rect(room, HALFBLOCKS), Rect::new(28, 3, 44, 22));
    }

    #[test]
    fn a_small_room_makes_the_picture_smaller() {
        // 8 rows of 20 px are 160 px, which take 16 columns of 10 px.
        assert_eq!(
            mascot_rect(Rect::new(5, 2, 30, 8), HALFBLOCKS),
            Rect::new(12, 2, 16, 8)
        );
        // 30 columns of 15 px are 450 px, which take 12 rows of 39 px
        // (11.54, rounded up).
        assert_eq!(
            mascot_rect(Rect::new(0, 0, 30, 20), GHOSTTY),
            Rect::new(0, 0, 30, 12)
        );
    }

    #[test]
    fn both_pictures_decode_as_squares_of_768_pixels() {
        for which in [Which::Base, Which::Sleeping] {
            let picture = image::load_from_memory_with_format(which.webp(), ImageFormat::WebP)
                .expect("decodes");
            assert_eq!((picture.width(), picture.height()), (768, 768), "{which:?}");
        }
    }

    /// ratatui-image resizes a picture again unless it fits its cells
    /// exactly on one axis, and it resizes without taking alpha into
    /// account (ratatui-image 11.1.0 `lib.rs` l.470-480). The scaled
    /// picture must therefore fit the rect so that it does not.
    #[test]
    fn the_scaled_picture_fits_its_cells_so_ratatui_image_keeps_it() {
        let picture = DynamicImage::new_rgba8(768, 768);
        let room = Rect::new(0, 0, 80, 30);
        for font in [GHOSTTY, HALFBLOCKS, FontSize::new(7, 16)] {
            let rect = mascot_rect(room, font);
            let side = picture_side(rect.width, rect.height, font);
            let picture = DynamicImage::from(scaled(&picture, side, side));

            let width = u32::from(rect.width) * u32::from(font.width);
            let height = u32::from(rect.height) * u32::from(font.height);
            assert!(
                picture.width() == width || picture.height() == height,
                "{font:?}: {}×{} px in {width}×{height}",
                picture.width(),
                picture.height()
            );
            assert_eq!(Resize::natural_size(&picture, font), rect.as_size());
        }
    }

    /// Lossy WebP may store any colour under alpha 0, and the mascot's
    /// transparent pixels store black. Neither may bleed into the edge of
    /// what is opaque.
    #[test]
    fn scaling_keeps_the_colour_of_an_edge_next_to_transparency() {
        // The left half opaque red, the right half transparent green.
        let picture = RgbaImage::from_fn(8, 8, |x, _| {
            if x < 4 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 255, 0, 0])
            }
        });

        let small = scaled(&picture.into(), 3, 3);

        assert_eq!(small.dimensions(), (3, 3));
        let edge = small.get_pixel(1, 1).0;
        assert!(0 < edge[3] && edge[3] < 255, "half covered: {edge:?}");
        assert!(edge[0] >= 250 && edge[1] <= 5, "still red: {edge:?}");
    }

    #[test]
    fn only_an_image_protocol_draws_the_picture() {
        assert!(Mascot::new(Picker::halfblocks()).font().is_none());
        let font = Mascot::new(kitty()).font().expect("Kitty draws it");
        assert_eq!((font.width, font.height), (10, 20));
    }

    /// Draws `which` with `mascot` into a 60×30 buffer and says whether
    /// the draw sent the picture to the terminal.
    fn sent(mascot: &mut Mascot, which: Which, area: Rect) -> bool {
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 30));
        mascot.draw(which, area, &mut buf);
        let placeholders = buf
            .content()
            .iter()
            .filter(|cell| matches!(cell.fg, Color::Rgb(..)))
            .count();
        assert_eq!(
            placeholders,
            usize::from(area.width) * usize::from(area.height),
            "the picture covers its area"
        );
        transmission(&buf).is_some()
    }

    #[test]
    fn the_picture_is_sent_once_until_it_changes_or_is_forgotten() {
        let mut mascot = Mascot::new(kitty());
        let area = mascot_rect(Rect::new(0, 0, 60, 26), HALFBLOCKS);

        assert!(sent(&mut mascot, Which::Base, area), "first draw");
        assert!(!sent(&mut mascot, Which::Base, area), "kept");
        assert_eq!(mascot.shown(), Some(Which::Base));

        assert!(sent(&mut mascot, Which::Sleeping, area), "another picture");
        assert_eq!(mascot.shown(), Some(Which::Sleeping));

        let smaller = mascot_rect(Rect::new(0, 0, 60, 10), HALFBLOCKS);
        assert!(sent(&mut mascot, Which::Sleeping, smaller), "another size");

        mascot.forget();
        assert_eq!(mascot.shown(), None);
        assert!(sent(&mut mascot, Which::Sleeping, smaller), "forgotten");
    }
}
