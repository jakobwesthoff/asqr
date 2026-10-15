// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Talqi, asqr's mascot, as the empty queue shows it (spec section 7.1,
//! ADR 39): the pictures built into asqr, the cells a picture takes, and
//! the picture scaled and encoded for the terminal's graphics protocol, or
//! drawn in half blocks where the terminal has none. Under `NO_COLOR`,
//! Talqi is a braille outline instead (ADR 41).

use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, RgbaImage};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Position, Rect, Size};
use ratatui::style::Color;
use ratatui::widgets::Widget;
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

/// The size in pixels of a half block: one wide and two tall, as a cell
/// holds two of them, one above the other. Laid out in cells of this size,
/// the picture has one pixel per half block.
const HALF_BLOCK_CELL: FontSize = FontSize::new(1, 2);

/// How the picture reaches the screen.
#[derive(Debug, Clone, Copy)]
enum Drawing {
    /// Encoded for the terminal's image protocol and laid out in its
    /// cells, of this size in pixels.
    Protocol(FontSize),
    /// In [`HalfBlocks`], laid out in cells of [`HALF_BLOCK_CELL`].
    HalfBlocks,
}

/// How the picture is drawn in a terminal that speaks `protocol` and has
/// cells of `font`'s size in pixels: in the image protocol only where
/// both sides of a cell have pixels. A terminal may report a side of zero,
/// and no picture can be laid out in such cells, so it gets half blocks
/// as a terminal without an image protocol does.
fn drawing(protocol: ProtocolType, font: FontSize) -> Drawing {
    let has_size = font.width > 0 && font.height > 0;
    match protocol {
        ProtocolType::Kitty | ProtocolType::Sixel | ProtocolType::Iterm2 if has_size => {
            Drawing::Protocol(font)
        }
        ProtocolType::Kitty
        | ProtocolType::Sixel
        | ProtocolType::Iterm2
        | ProtocolType::Halfblocks => Drawing::HalfBlocks,
    }
}

/// Talqi for one terminal: the picture last drawn, kept scaled, and
/// encoded where the terminal has an image protocol, until another
/// picture or another size is asked for. It is the state of
/// [`EmptyQueue`](super::empty::EmptyQueue).
pub(super) struct Mascot {
    picker: Picker,
    /// How the picture is drawn, decided once from `picker`.
    drawing: Drawing,
    /// The picture last drawn, the cells it was scaled for, and the
    /// picture ready to draw again.
    ready: Option<(Which, Size, Ready)>,
}

/// The picture scaled for its cells, in the form the terminal draws it.
enum Ready {
    /// Encoded for `picker`'s image protocol.
    Encoded(Image),
    /// One pixel per half block, for [`HalfBlocks`].
    HalfBlocks(RgbaImage),
}

impl Mascot {
    /// Talqi for a terminal whose graphics protocol `picker` speaks.
    pub(super) fn new(picker: Picker) -> Self {
        Mascot {
            drawing: drawing(picker.protocol_type(), picker.font_size()),
            picker,
            ready: None,
        }
    }

    /// The size in pixels of the cells the picture is laid out in: the
    /// terminal's own for an image protocol, a half block's without one.
    pub(super) fn font(&self) -> FontSize {
        match self.drawing {
            Drawing::Protocol(font) => font,
            Drawing::HalfBlocks => HALF_BLOCK_CELL,
        }
    }

    /// Draws `which` into `area` of `buf`, an area [`mascot_rect`] made
    /// for [`font`](Mascot::font).
    ///
    /// The picture is decoded, scaled and encoded only when `which` or the
    /// area's size differs from the last draw. Scaled to fill the area on
    /// one axis, it passes `Resize::Fit` unchanged (ratatui-image 11.1.0
    /// `lib.rs` l.470-480). `Resize::Scale` would resize it once more,
    /// without regard to alpha, and bring back the fringe [`scaled`]
    /// avoids. Without an image protocol, [`HalfBlocks`] draws the scaled
    /// picture as it is.
    pub(super) fn draw(&mut self, which: Which, area: Rect, buf: &mut Buffer) {
        let size = area.as_size();
        let kept =
            |&(shown, scaled_for, _): &(Which, Size, Ready)| shown == which && scaled_for == size;
        if !self.ready.as_ref().is_some_and(kept) {
            let picture = image::load_from_memory_with_format(which.webp(), ImageFormat::WebP)
                .expect("the pictures built into asqr decode");
            let side = picture_side(area.width, area.height, self.font());
            let picture = scaled(&picture, side, side);
            let ready = match self.drawing {
                Drawing::HalfBlocks => Ready::HalfBlocks(picture),
                Drawing::Protocol(_) => {
                    Ready::Encoded(Image::from_picture(&self.picker, picture.into()))
                }
            };
            self.ready = Some((which, size, ready));
        }
        match self.ready.as_mut().expect("made above when missing") {
            (.., Ready::Encoded(image)) => image.draw(area, buf, false),
            (.., Ready::HalfBlocks(picture)) => HalfBlocks(picture).render(area, buf),
        }
    }

    /// Drops the picture kept for drawing when it is encoded for the Kitty
    /// protocol, so it is sent to the terminal again when it is next
    /// drawn. The Kitty protocol sends a picture's data only on its first
    /// draw, and tmux drops that transfer while its pane is hidden (ADR
    /// 24).
    ///
    /// Any other picture is kept. Sixel and iTerm2 carry the whole picture
    /// in one cell of each frame, and encoded again it gives the same
    /// cell, which ratatui does not write again. Half blocks are text,
    /// which tmux keeps for a hidden pane.
    pub(super) fn forget(&mut self) {
        let sent_once = self.picker.protocol_type() == ProtocolType::Kitty
            && matches!(self.ready, Some((.., Ready::Encoded(_))));
        if sent_once {
            self.ready = None;
        }
    }

    /// The picture last drawn and still kept.
    #[cfg(test)]
    pub(super) fn shown(&self) -> Option<Which> {
        self.ready.as_ref().map(|(which, ..)| *which)
    }
}

/// The least alpha of a pixel that counts as opaque in half blocks: half
/// of full. A half block is either drawn or not, so the soft edge
/// [`scaled`] leaves around Talqi falls to one side or the other.
const OPAQUE: u8 = 128;

/// A picture drawn as half blocks in its own RGB colours, one pixel per
/// half of a cell, from the area's top-left corner.
///
/// ratatui-image has half blocks of its own, but makes them from the
/// picture without its alpha channel (ratatui-image 11.1.0
/// `protocol/halfblocks/primitive.rs` l.29), which paints a transparent
/// background as coloured blocks. Here a transparent half shows the
/// cell's background, and a cell with two transparent halves stays as it
/// was. Under `NO_COLOR` the empty queue draws the [`Outline`] instead.
struct HalfBlocks<'a>(&'a RgbaImage);

impl Widget for HalfBlocks<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let HalfBlocks(picture) = self;
        let opaque = |x: u16, y: u32| {
            picture
                .get_pixel_checked(u32::from(x), y)
                .filter(|pixel| pixel[3] >= OPAQUE)
                .map(|pixel| Color::Rgb(pixel[0], pixel[1], pixel[2]))
        };
        for position in area.intersection(buf.area).positions() {
            let x = position.x - area.x;
            let upper = u32::from(position.y - area.y) * 2;
            let cell = &mut buf[position];
            match (opaque(x, upper), opaque(x, upper + 1)) {
                (Some(upper), Some(lower)) => {
                    cell.set_symbol("▀").set_fg(upper).set_bg(lower);
                }
                (Some(upper), None) => {
                    cell.set_symbol("▀").set_fg(upper);
                }
                (None, Some(lower)) => {
                    cell.set_symbol("▄").set_fg(lower);
                }
                (None, None) => {}
            }
        }
    }
}

/// Talqi under `NO_COLOR` (ADR 41): the outline of the terminal emblem
/// (`assets/emblem/talqi-terminal.svg`) in braille, one text for awake and
/// asleep and for light and dark terminals. asqr does not generate it;
/// ADR 41 says how it was made.
const OUTLINE: &str = include_str!("mascot/talqi-outline.txt");

/// The columns and rows of [`OUTLINE`], a braille character per cell.
pub(super) const OUTLINE_COLUMNS: u16 = 44;
pub(super) const OUTLINE_ROWS: u16 = 22;

/// The empty braille pattern, which [`OUTLINE`] has where it has no ink.
const NO_INK: char = '\u{2800}';

/// [`OUTLINE`] drawn from the area's top-left corner.
///
/// A cell with ink gets its braille character and no style at all, so
/// nothing of the outline carries a colour. A cell without ink stays as
/// it was, so the background shows between the lines.
pub(super) struct Outline;

impl Widget for Outline {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (y, row) in (area.top()..area.bottom()).zip(OUTLINE.lines()) {
            for (x, symbol) in (area.left()..area.right()).zip(row.chars()) {
                if symbol != NO_INK && buf.area.contains(Position { x, y }) {
                    buf[(x, y)].reset();
                    buf[(x, y)].set_char(symbol);
                }
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use image::Rgba;
    use ratatui::buffer::Cell;
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
    fn an_image_protocol_lays_the_picture_out_in_the_terminal_s_cells() {
        let font = Mascot::new(kitty()).font();
        assert_eq!((font.width, font.height), (10, 20));
    }

    /// The cells `drawing` lays the picture out in, or `None` for half
    /// blocks. [`FontSize`] cannot be compared.
    fn protocol_cells(drawing: Drawing) -> Option<(u16, u16)> {
        match drawing {
            Drawing::Protocol(font) => Some((font.width, font.height)),
            Drawing::HalfBlocks => None,
        }
    }

    #[test]
    fn an_image_protocol_with_a_cell_size_draws_in_the_protocol() {
        for protocol in [
            ProtocolType::Kitty,
            ProtocolType::Sixel,
            ProtocolType::Iterm2,
        ] {
            let drawn = drawing(protocol, FontSize::new(10, 20));
            assert_eq!(protocol_cells(drawn), Some((10, 20)), "{protocol:?}");
        }
    }

    /// A terminal may report a cell side of zero, in its answer to the
    /// cell size query or as a window fewer pixels wide than it has
    /// columns, which ratatui-image's fallback divides down to zero
    /// (ratatui-image 11.1.0 `picker.rs` l.468-472, l.557-561). No picture
    /// can be laid out in such cells, so it is drawn in half blocks.
    #[test]
    fn a_cell_side_of_zero_draws_in_half_blocks() {
        for font in [
            FontSize::new(0, 20),
            FontSize::new(8, 0),
            FontSize::new(0, 0),
        ] {
            let drawn = drawing(ProtocolType::Kitty, font);
            assert_eq!(protocol_cells(drawn), None, "{font:?}");
        }
    }

    #[test]
    fn without_an_image_protocol_talqi_is_drawn_in_half_blocks() {
        let drawn = drawing(ProtocolType::Halfblocks, FontSize::new(10, 20));
        assert_eq!(protocol_cells(drawn), None);
    }

    /// The Kitty protocol with a cell side of zero lays the picture out
    /// in half blocks, at the largest size, and sends nothing.
    #[test]
    fn a_kitty_terminal_without_a_cell_size_gets_talqi_in_half_blocks() {
        let mut mascot = Mascot {
            drawing: drawing(ProtocolType::Kitty, FontSize::new(0, 20)),
            ..Mascot::new(kitty())
        };
        let rect = mascot_rect(Rect::new(0, 0, 100, 40), mascot.font());
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 40));

        mascot.draw(Which::Base, rect, &mut buf);

        assert_eq!(rect, Rect::new(28, 0, 44, 22));
        assert_eq!(transmission(&buf), None);
        let half_blocks: Vec<_> = buf
            .area
            .positions()
            .filter(|&position| matches!(buf[position].symbol(), "▀" | "▄"))
            .collect();
        assert!(!half_blocks.is_empty(), "Talqi is drawn");
        assert!(half_blocks.iter().all(|&position| rect.contains(position)));
        mascot.forget();
        assert_eq!(mascot.shown(), Some(Which::Base), "half blocks are kept");
    }

    // -----------------------------------------------------------------
    // Half blocks
    // -----------------------------------------------------------------

    /// Opaque, if only just, and transparent, if only just: alpha 128 is
    /// the least that counts as opaque. The transparent pixel stores a
    /// colour, as lossy WebP may.
    const RED: Rgba<u8> = Rgba([200, 30, 10, 255]);
    const GREEN: Rgba<u8> = Rgba([10, 180, 40, 128]);
    const CLEAR: Rgba<u8> = Rgba([90, 90, 250, 127]);

    /// A picture of `columns`, each given from top to bottom.
    fn picture(columns: &[&[Rgba<u8>]]) -> RgbaImage {
        let width = u32::try_from(columns.len()).expect("a small picture");
        let height = u32::try_from(columns[0].len()).expect("a small picture");
        RgbaImage::from_fn(width, height, |x, y| columns[x as usize][y as usize])
    }

    /// A buffer of `width`×`height` cells that each hold a blue `x` on
    /// white, so that a test sees which cells a draw left alone.
    fn marked(width: u16, height: u16) -> Buffer {
        let mut cell = Cell::default();
        cell.set_symbol("x")
            .set_fg(Color::Blue)
            .set_bg(Color::White);
        Buffer::filled(Rect::new(0, 0, width, height), cell)
    }

    /// The symbol and colours of the cell at `x`, `y`.
    fn drawn_at(buf: &Buffer, x: u16, y: u16) -> (&str, Color, Color) {
        let cell = &buf[(x, y)];
        (cell.symbol(), cell.fg, cell.bg)
    }

    fn rgb(pixel: Rgba<u8>) -> Color {
        let [red, green, blue, _] = pixel.0;
        Color::Rgb(red, green, blue)
    }

    #[test]
    fn half_blocks_leave_the_cells_of_transparent_pixels_alone() {
        let clear = picture(&[&[CLEAR, Rgba([0; 4])], &[Rgba([255, 0, 0, 0]), CLEAR]]);
        let mut buf = marked(2, 1);

        HalfBlocks(&clear).render(buf.area, &mut buf);

        assert_eq!(buf, marked(2, 1));
    }

    #[test]
    fn two_opaque_pixels_take_an_upper_half_block_in_both_their_colours() {
        let both = picture(&[&[RED, GREEN]]);
        let mut buf = marked(1, 1);

        HalfBlocks(&both).render(buf.area, &mut buf);

        assert_eq!(drawn_at(&buf, 0, 0), ("▀", rgb(RED), rgb(GREEN)));
    }

    /// One opaque pixel takes the half block on its side, in its colour
    /// over the cell's background, so the terminal's background shows in
    /// the transparent half.
    #[test]
    fn one_opaque_pixel_takes_its_half_of_the_cell() {
        let halves = picture(&[&[RED, CLEAR], &[CLEAR, GREEN]]);
        let mut buf = marked(2, 1);

        HalfBlocks(&halves).render(buf.area, &mut buf);

        assert_eq!(drawn_at(&buf, 0, 0), ("▀", rgb(RED), Color::White));
        assert_eq!(drawn_at(&buf, 1, 0), ("▄", rgb(GREEN), Color::White));
    }

    /// A picture with an odd number of rows ends in a row of upper
    /// halves, and an area larger than the picture or the buffer keeps
    /// the rest of the buffer as it is.
    #[test]
    fn half_blocks_end_where_the_picture_and_the_buffer_end() {
        let column = picture(&[&[RED, RED, RED]]);
        let mut buf = marked(2, 3);

        HalfBlocks(&column).render(Rect::new(0, 0, 5, 5), &mut buf);

        assert_eq!(drawn_at(&buf, 0, 0), ("▀", rgb(RED), rgb(RED)));
        assert_eq!(drawn_at(&buf, 0, 1), ("▀", rgb(RED), Color::White));
        for (x, y) in [(0, 2), (1, 0), (1, 1), (1, 2)] {
            assert_eq!(drawn_at(&buf, x, y), ("x", Color::Blue, Color::White));
        }
    }

    /// A half block is one pixel wide and two tall, so the same rule
    /// that sizes the picture for an image protocol gives a square of 44
    /// pixels in the 44×22 cells of the largest box.
    #[test]
    fn a_square_in_half_blocks_takes_44_by_22_cells() {
        let font = Mascot::new(Picker::halfblocks()).font();
        assert_eq!((font.width, font.height), (1, 2));
        let rect = mascot_rect(Rect::new(0, 0, 100, 40), font);
        assert_eq!(rect, Rect::new(28, 0, 44, 22));

        let side = picture_side(rect.width, rect.height, font);
        let opaque = RgbaImage::from_pixel(768, 768, RED);
        let square = scaled(&opaque.into(), side, side);
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 40));
        HalfBlocks(&square).render(rect, &mut buf);

        let painted: Vec<_> = buf
            .area
            .positions()
            .filter(|&position| buf[position].symbol() == "▀")
            .collect();
        assert_eq!(painted.len(), 44 * 22);
        assert!(painted.iter().all(|&position| rect.contains(position)));
    }

    #[test]
    fn without_an_image_protocol_talqi_is_drawn_in_half_blocks_inside_its_rect() {
        let mut mascot = Mascot::new(Picker::halfblocks());
        let rect = mascot_rect(Rect::new(0, 0, 60, 30), mascot.font());
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 30));

        mascot.draw(Which::Base, rect, &mut buf);

        // A cell of Talqi's transparent background stays blank, without
        // colours.
        let painted: Vec<_> = buf
            .area
            .positions()
            .filter(|&position| buf[position] != Cell::default())
            .collect();
        assert!(!painted.is_empty(), "Talqi is drawn");
        for position in painted {
            let cell = &buf[position];
            assert!(rect.contains(position), "{position:?} outside {rect:?}");
            assert!(matches!(cell.symbol(), "▀" | "▄"), "{:?}", cell.symbol());
            assert!(matches!(cell.fg, Color::Rgb(..)), "{:?}", cell.fg);
        }
        assert_eq!(mascot.shown(), Some(Which::Base));
        mascot.forget();
        assert_eq!(mascot.shown(), Some(Which::Base), "half blocks are kept");
    }

    // -----------------------------------------------------------------
    // The outline under NO_COLOR
    // -----------------------------------------------------------------

    #[test]
    fn the_outline_is_22_rows_of_44_braille_cells() {
        let rows: Vec<&str> = OUTLINE.lines().collect();

        assert_eq!(rows.len(), usize::from(OUTLINE_ROWS));
        for row in rows {
            assert_eq!(row.chars().count(), usize::from(OUTLINE_COLUMNS), "{row}");
            assert!(
                row.chars()
                    .all(|symbol| ('\u{2800}'..='\u{28FF}').contains(&symbol)),
                "only braille: {row}"
            );
        }
    }

    /// The outline drawn into a [`marked`] buffer of `width`×`height`
    /// cells, at `area`.
    fn outlined(width: u16, height: u16, area: Rect) -> Buffer {
        let mut buf = marked(width, height);
        Outline.render(area, &mut buf);
        buf
    }

    /// A cell with ink is the template's character and nothing else, no
    /// colour and no modifier; a cell without ink, U+2800, keeps what the
    /// buffer held there.
    #[test]
    fn the_outline_draws_its_ink_without_a_style_and_leaves_the_rest_alone() {
        let area = Rect::new(3, 2, OUTLINE_COLUMNS, OUTLINE_ROWS);
        let buf = outlined(50, 26, area);

        let rows: Vec<Vec<char>> = OUTLINE.lines().map(|row| row.chars().collect()).collect();
        let mut ink = 0;
        for position in buf.area.positions() {
            let template = area
                .contains(position)
                .then(|| rows[usize::from(position.y - area.y)][usize::from(position.x - area.x)]);
            match template {
                Some(symbol) if symbol != '\u{2800}' => {
                    ink += 1;
                    let mut expected = Cell::default();
                    expected.set_char(symbol);
                    assert_eq!(buf[position], expected, "{position:?}");
                }
                _ => assert_eq!(
                    drawn_at(&buf, position.x, position.y),
                    ("x", Color::Blue, Color::White),
                    "{position:?}"
                ),
            }
        }
        assert!(ink > 0, "the outline has ink");
    }

    /// A buffer that ends inside the outline holds the outline's top-left
    /// part, and nothing is drawn past its edge.
    #[test]
    fn the_outline_ends_where_the_buffer_ends() {
        let area = Rect::new(0, 0, OUTLINE_COLUMNS, OUTLINE_ROWS);
        let whole = outlined(OUTLINE_COLUMNS, OUTLINE_ROWS, area);

        let part = outlined(20, 10, area);

        assert_ne!(whole[(1, 21)].symbol(), "x", "ink in the bottom row");
        for position in part.area.positions() {
            assert_eq!(part[position], whole[position], "{position:?}");
        }
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

    /// Sixel and iTerm2 carry the whole picture in one cell of each frame.
    /// Encoded again, it gives the same cell, which ratatui does not write
    /// again, so forgetting it would only cost a new encoding.
    #[test]
    fn sixel_and_iterm2_keep_their_picture() {
        for protocol in [ProtocolType::Sixel, ProtocolType::Iterm2] {
            let mut picker = Picker::halfblocks();
            picker.set_protocol_type(protocol);
            let mut mascot = Mascot::new(picker);
            let area = mascot_rect(Rect::new(0, 0, 60, 10), mascot.font());
            let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
            mascot.draw(Which::Sleeping, area, &mut buf);

            mascot.forget();

            assert_eq!(mascot.shown(), Some(Which::Sleeping), "{protocol:?}");
        }
    }
}
