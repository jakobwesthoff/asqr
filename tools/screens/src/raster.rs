// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A drawn ratatui buffer as a picture: every cell in the palette's
//! colours, its character from the fonts or painted, on the cell grid of
//! [`crate::fonts`].

use ab_glyph::{Font, PxScale, ScaleFont, point};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbImage};
use ratatui::buffer::{Buffer, Cell, CellWidth};
use ratatui::style::{Color, Modifier};

use crate::fonts::{
    BASELINE, CELL_HEIGHT, CELL_WIDTH, Face, STRIKE_THICKNESS, STRIKE_TOP, Source,
    UNDERLINE_THICKNESS, UNDERLINE_TOP, source_for,
};
use crate::paint::paint;
use crate::palette::{BACKGROUND, FOREGROUND, Rgb, rgb};

/// The border around the screen, in the background colour: the window
/// padding of 5 points that Ghostty draws at 10 pixels on a display of
/// content scale 2 (`floor(points * dpi / 72)`, `src/Surface.zig`).
pub const PADDING: u32 = 10;

/// How much of the text colour dim text shows over its background:
/// Ghostty's default `faint-opacity` (`src/config/Config.zig`).
const DIM_OPACITY: f32 = 0.5;

/// A character no committed font has, and the cell it stands in.
#[derive(Debug, PartialEq, Eq)]
pub struct MissingGlyph {
    pub ch: char,
    pub column: u16,
    pub row: u16,
}

impl std::fmt::Display for MissingGlyph {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "no font has U+{:04X} {:?} (column {}, row {})",
            u32::from(self.ch),
            self.ch,
            self.column,
            self.row
        )
    }
}

impl std::error::Error for MissingGlyph {}

/// A cell as the terminal shows it: where it stands and how many columns
/// its symbol takes.
struct Placed<'a> {
    column: u16,
    row: u16,
    columns: u16,
    cell: &'a Cell,
}

impl Placed<'_> {
    /// The pixel of the cell's top left corner.
    fn origin(&self) -> (u32, u32) {
        (
            PADDING + u32::from(self.column) * CELL_WIDTH,
            PADDING + u32::from(self.row) * CELL_HEIGHT,
        )
    }

    fn width(&self) -> u32 {
        u32::from(self.columns) * CELL_WIDTH
    }
}

/// The cells of `buf` as the terminal shows them. A symbol wider than one
/// column covers the cells after it, which ratatui resets and the terminal
/// does not show.
fn placed(buf: &Buffer) -> Vec<Placed<'_>> {
    let area = buf.area;
    let mut cells = Vec::new();
    for row in 0..area.height {
        let mut column = 0;
        while column < area.width {
            let cell = &buf[(area.x + column, area.y + row)];
            let columns = cell.cell_width().clamp(1, area.width - column);
            cells.push(Placed {
                column,
                row,
                columns,
                cell,
            });
            column += columns;
        }
    }
    cells
}

/// The text colour, the background and the opacity of the text of `cell`.
fn colours(cell: &Cell) -> (Rgb, Rgb, f32) {
    let mut text = rgb(cell.fg, FOREGROUND);
    let mut background = rgb(cell.bg, BACKGROUND);
    if cell.modifier.contains(Modifier::REVERSED) {
        std::mem::swap(&mut text, &mut background);
    }
    let opacity = if cell.modifier.contains(Modifier::DIM) {
        DIM_OPACITY
    } else {
        1.0
    };
    (text, background, opacity)
}

/// `buf` as a picture: its cells on the grid, inside the border.
///
/// Every background is painted before any text, as Ghostty draws them in a
/// pass of their own. Glyphs are not clipped to their cells: an italic's
/// overhang reaches into the next cell, as Ghostty places each glyph at its
/// cell plus its bearings (`src/renderer/shaders/shaders.metal`,
/// `cell_text_vertex`).
pub fn rasterize(buf: &Buffer) -> Result<RgbImage, MissingGlyph> {
    let width = u32::from(buf.area.width) * CELL_WIDTH + 2 * PADDING;
    let height = u32::from(buf.area.height) * CELL_HEIGHT + 2 * PADDING;
    let mut image = RgbImage::from_pixel(width, height, image::Rgb(BACKGROUND));
    let cells = placed(buf);
    for placed in &cells {
        let (_, background, _) = colours(placed.cell);
        let (x, y) = placed.origin();
        for py in y..y + CELL_HEIGHT {
            for px in x..x + placed.width() {
                image.put_pixel(px, py, image::Rgb(background));
            }
        }
    }
    for placed in &cells {
        draw_text(&mut image, placed)?;
    }
    Ok(image)
}

/// Blends `colour` over the pixel at (`x`, `y`) with `alpha`, in sRGB
/// values, as Ghostty's macOS default `alpha-blending = native` does. A
/// pixel outside the picture is left out.
fn blend(image: &mut RgbImage, x: i64, y: i64, colour: Rgb, alpha: f32) {
    let inside =
        (0..i64::from(image.width())).contains(&x) && (0..i64::from(image.height())).contains(&y);
    if !inside || alpha <= 0.0 {
        return;
    }
    let alpha = alpha.min(1.0);
    let pixel = image.get_pixel_mut(x as u32, y as u32);
    for (channel, target) in pixel.0.iter_mut().zip(colour) {
        let from = f32::from(*channel);
        *channel = (from + (f32::from(target) - from) * alpha).round() as u8;
    }
}

/// Whether `ch` stands in a symbol without a glyph of its own: a
/// variation selector or a joiner.
fn draws_nothing(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x200C | 0x200D | 0xFE00..=0xFE0F | 0xE0100..=0xE01EF
    )
}

/// The text of one cell: its symbol, its underline and its
/// strikethrough. A combining mark is drawn over the character before it.
fn draw_text(image: &mut RgbImage, placed: &Placed<'_>) -> Result<(), MissingGlyph> {
    let cell = placed.cell;
    if cell.modifier.contains(Modifier::HIDDEN) {
        return Ok(());
    }
    let (text, _, opacity) = colours(cell);
    let (x, y) = placed.origin();
    let bold = cell.modifier.contains(Modifier::BOLD);
    let italic = cell.modifier.contains(Modifier::ITALIC);
    let mut base: Option<(f32, f32)> = None;
    for ch in cell.symbol().chars() {
        if ch.is_control() || draws_nothing(ch) {
            continue;
        }
        match source_for(ch, bold, italic) {
            Source::Painted => {
                let mask = paint(ch);
                for my in 0..CELL_HEIGHT {
                    for mx in 0..CELL_WIDTH {
                        let alpha = f32::from(mask.at(mx, my)) / 255.0;
                        blend(
                            image,
                            i64::from(x + mx),
                            i64::from(y + my),
                            text,
                            alpha * opacity,
                        );
                    }
                }
            }
            Source::Font(face) => {
                let placed_at = draw_glyph(image, face, ch, placed, base, text, opacity);
                base = base.or(Some(placed_at));
            }
            Source::Missing => {
                return Err(MissingGlyph {
                    ch,
                    column: placed.column,
                    row: placed.row,
                });
            }
        }
    }
    let lines = [
        (
            Modifier::UNDERLINED,
            UNDERLINE_TOP,
            UNDERLINE_THICKNESS,
            match cell.underline_color {
                Color::Reset => text,
                colour => rgb(colour, FOREGROUND),
            },
        ),
        (Modifier::CROSSED_OUT, STRIKE_TOP, STRIKE_THICKNESS, text),
    ];
    for (modifier, top, thickness, colour) in lines {
        if cell.modifier.contains(modifier) {
            for py in y + top..y + top + thickness {
                for px in x..x + placed.width() {
                    blend(image, i64::from(px), i64::from(py), colour, opacity);
                }
            }
        }
    }
    Ok(())
}

/// Draws `ch` from `face` into the cells of `placed` and returns where its
/// pen started and how far it advanced.
///
/// Victor Mono's glyphs start at the cell's left edge, as a monospace
/// face's glyphs fill their cells. A fallback glyph is centred on its
/// cells, scaled down about the baseline when it is wider than them. A
/// combining mark after a `base` glyph goes over it: one without an
/// advance of its own is designed to follow its base, the others to share
/// its pen position.
fn draw_glyph(
    image: &mut RgbImage,
    face: &Face,
    ch: char,
    placed: &Placed<'_>,
    base: Option<(f32, f32)>,
    colour: Rgb,
    opacity: f32,
) -> (f32, f32) {
    let (x, y) = placed.origin();
    let id = face.font.glyph_id(ch);
    let mut scale = face.scale();
    let advance = face.font.as_scaled(scale).h_advance(id);
    let room = placed.width() as f32;
    let pen = match base {
        Some((base_pen, base_advance)) if advance == 0.0 => base_pen + base_advance,
        Some((base_pen, _)) => base_pen,
        None if face.text => x as f32,
        None => {
            let factor = (room / advance).min(1.0);
            scale = PxScale {
                x: scale.x * factor,
                y: scale.y * factor,
            };
            x as f32 + (room - advance * factor) / 2.0
        }
    };
    let glyph = id.with_scale_and_position(scale, point(pen, (y + BASELINE) as f32));
    if let Some(outline) = face.font.outline_glyph(glyph) {
        let bounds = outline.px_bounds();
        outline.draw(|gx, gy, cover| {
            blend(
                image,
                bounds.min.x as i64 + i64::from(gx),
                bounds.min.y as i64 + i64::from(gy),
                colour,
                cover * opacity,
            );
        });
    }
    (pen, face.font.as_scaled(scale).h_advance(id))
}

/// `image` as PNG bytes. The pictures use `Best`, which takes longer; the
/// tests use `Fast`.
pub fn encode_png(image: &RgbImage, compression: CompressionType) -> Vec<u8> {
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, compression, FilterType::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgb8,
        )
        .expect("encoding a whole RGB image into memory cannot fail");
    bytes
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};

    use super::*;
    use crate::fonts::{CELL_HEIGHT, CELL_WIDTH};
    use crate::palette::{BACKGROUND, FOREGROUND, PALETTE, Rgb};

    fn buffer(columns: u16, rows: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, columns, rows))
    }

    fn drawn(buf: &Buffer) -> RgbImage {
        rasterize(buf).expect("the test's characters are in the fonts")
    }

    /// The pixel at (`x`, `y`) of cell (`column`, `row`), if the picture
    /// has it.
    fn pixel(image: &RgbImage, column: u32, row: u32, x: u32, y: u32) -> Option<Rgb> {
        let x = PADDING + column * CELL_WIDTH + x;
        let y = PADDING + row * CELL_HEIGHT + y;
        (x < image.width() && y < image.height()).then(|| image.get_pixel(x, y).0)
    }

    /// The pixels of cell (`column`, `row`) that are not `background`.
    fn ink(image: &RgbImage, column: u32, row: u32, background: Rgb) -> Vec<(u32, u32)> {
        (0..CELL_HEIGHT)
            .flat_map(|y| (0..CELL_WIDTH).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(image, column, row, x, y).is_some_and(|rgb| rgb != background))
            .collect()
    }

    fn half_way(a: Rgb, b: Rgb) -> Rgb {
        [0, 1, 2].map(|i| (u16::from(a[i]) + u16::from(b[i])).div_ceil(2) as u8)
    }

    #[test]
    fn the_picture_has_the_padding_and_the_cell_grid() {
        let image = drawn(&buffer(2, 1));

        assert_eq!((image.width(), image.height()), (2 * 15 + 20, 39 + 20));
    }

    #[test]
    fn the_border_and_an_empty_cell_take_the_background() {
        let image = drawn(&buffer(1, 1));

        assert_eq!(image.get_pixel(0, 0).0, BACKGROUND);
        assert_eq!(pixel(&image, 0, 0, 7, 19), Some(BACKGROUND));
    }

    #[test]
    fn reversed_swaps_text_and_background() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)].set_style(Style::new().fg(Color::Red).bg(Color::Blue).reversed());

        assert_eq!(pixel(&drawn(&buf), 0, 0, 7, 19), Some(PALETTE[1]));
    }

    #[test]
    fn reversed_without_colours_shows_the_foreground_as_background() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)].set_style(Style::new().reversed());

        assert_eq!(pixel(&drawn(&buf), 0, 0, 7, 19), Some(FOREGROUND));
    }

    #[test]
    fn dim_draws_the_text_at_half_opacity() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)].set_char('█').set_style(Style::new().dim());

        assert_eq!(
            pixel(&drawn(&buf), 0, 0, 7, 19),
            Some(half_way(FOREGROUND, BACKGROUND))
        );
    }

    #[test]
    fn a_painted_character_takes_the_text_colour() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)]
            .set_char('█')
            .set_style(Style::new().fg(Color::Green));

        assert_eq!(pixel(&drawn(&buf), 0, 0, 0, 0), Some(PALETTE[2]));
        assert_eq!(pixel(&drawn(&buf), 0, 0, 14, 38), Some(PALETTE[2]));
    }

    #[test]
    fn hidden_text_draws_only_its_background() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)]
            .set_char('█')
            .set_style(Style::new().bg(Color::Blue).add_modifier(Modifier::HIDDEN));

        assert_eq!(pixel(&drawn(&buf), 0, 0, 7, 19), Some(PALETTE[4]));
    }

    #[test]
    fn a_glyph_stands_on_the_baseline() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)].set_char('x');

        let ink = ink(&drawn(&buf), 0, 0, BACKGROUND);
        let lowest = ink.iter().map(|&(_, y)| y).max();
        assert!(matches!(lowest, Some(29..=30)), "lowest ink row {lowest:?}");
    }

    #[test]
    fn bold_draws_with_the_bold_face() {
        let mut regular = buffer(1, 1);
        regular[(0, 0)].set_char('M');
        let mut bold = buffer(1, 1);
        bold[(0, 0)].set_char('M').set_style(Style::new().bold());

        let regular = ink(&drawn(&regular), 0, 0, BACKGROUND).len();
        let bold = ink(&drawn(&bold), 0, 0, BACKGROUND).len();
        assert!(
            regular > 0 && bold > regular,
            "regular {regular}, bold {bold}"
        );
    }

    #[test]
    fn an_italic_glyph_may_reach_into_its_neighbours() {
        let mut buf = buffer(3, 1);
        buf[(1, 0)].set_char('/').set_style(Style::new().italic());

        let image = drawn(&buf);
        let outside = ink(&image, 0, 0, BACKGROUND).len() + ink(&image, 2, 0, BACKGROUND).len();
        assert!(outside > 0, "the slash keeps inside its cell");
    }

    #[test]
    fn a_fallback_glyph_is_centred_on_its_cell_and_stands_on_the_baseline() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)].set_char('☐');

        let ink = ink(&drawn(&buf), 0, 0, BACKGROUND);
        let left = ink.iter().map(|&(x, _)| x).min().expect("the box has ink");
        let right = CELL_WIDTH - 1 - ink.iter().map(|&(x, _)| x).max().expect("ink");
        let lowest = ink.iter().map(|&(_, y)| y).max().expect("ink");
        assert!(left.abs_diff(right) <= 1, "margins {left} and {right}");
        assert!((26..=32).contains(&lowest), "lowest ink row {lowest}");
    }

    #[test]
    fn a_wide_character_takes_two_cells_with_its_background() {
        let mut buf = buffer(3, 1);
        buf.set_string(0, 0, "⚡", Style::new().bg(Color::Red));

        let image = drawn(&buf);
        assert_eq!(pixel(&image, 0, 0, 0, 0), Some(PALETTE[1]));
        assert_eq!(pixel(&image, 1, 0, 14, 0), Some(PALETTE[1]));
        assert_eq!(pixel(&image, 2, 0, 7, 19), Some(BACKGROUND));
        let left = ink(&image, 0, 0, PALETTE[1]).len();
        let right = ink(&image, 1, 0, PALETTE[1]).len();
        assert!(left > 0 && right > 0, "ink {left} and {right}");
    }

    #[test]
    fn an_underline_takes_rows_32_and_33_in_its_colour() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)].set_style(Style::new().underlined().underline_color(Color::Green));

        let image = drawn(&buf);
        assert_eq!(pixel(&image, 0, 0, 7, 32), Some(PALETTE[2]));
        assert_eq!(pixel(&image, 0, 0, 7, 33), Some(PALETTE[2]));
        assert_eq!(pixel(&image, 0, 0, 7, 31), Some(BACKGROUND));
        assert_eq!(pixel(&image, 0, 0, 7, 34), Some(BACKGROUND));
    }

    #[test]
    fn an_underline_without_its_own_colour_takes_the_text_colour() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)].set_style(Style::new().underlined().fg(Color::Red));

        assert_eq!(pixel(&drawn(&buf), 0, 0, 7, 32), Some(PALETTE[1]));
    }

    #[test]
    fn a_strikethrough_takes_rows_21_and_22() {
        let mut buf = buffer(1, 1);
        buf[(0, 0)].set_style(Style::new().crossed_out());

        let image = drawn(&buf);
        assert_eq!(pixel(&image, 0, 0, 7, 21), Some(FOREGROUND));
        assert_eq!(pixel(&image, 0, 0, 7, 22), Some(FOREGROUND));
        assert_eq!(pixel(&image, 0, 0, 7, 20), Some(BACKGROUND));
    }

    #[test]
    fn a_variation_selector_in_a_symbol_draws_nothing() {
        let mut plain = buffer(1, 1);
        plain[(0, 0)].set_symbol("✔");
        let mut selected = buffer(1, 1);
        selected[(0, 0)].set_symbol("✔\u{FE0F}");

        let plain = drawn(&plain);
        assert!(
            !ink(&plain, 0, 0, BACKGROUND).is_empty(),
            "the check has ink"
        );
        assert_eq!(plain, drawn(&selected));
    }

    #[test]
    fn a_combining_mark_is_drawn_over_its_base() {
        let mut plain = buffer(1, 1);
        plain[(0, 0)].set_symbol("e");
        let mut accented = buffer(1, 1);
        accented[(0, 0)].set_symbol("e\u{301}");

        let above = |image: &RgbImage| {
            ink(image, 0, 0, BACKGROUND)
                .into_iter()
                .filter(|&(_, y)| y < 10)
                .count()
        };
        assert_eq!(
            above(&drawn(&plain)),
            0,
            "e alone has no ink in the top ten rows"
        );
        assert!(above(&drawn(&accented)) > 0, "the accent sits above the e");
    }

    /// Victor Mono's marks have no advance and are drawn after their base;
    /// DejaVu Sans Mono's have one and share their base's pen. U+030D, a
    /// vertical line above, is only in DejaVu Sans Mono and is drawn at
    /// the middle of the cell.
    #[test]
    fn a_combining_mark_with_an_advance_shares_its_base_s_pen() {
        let mut marked = buffer(1, 1);
        marked[(0, 0)].set_symbol("a\u{30D}");

        let above: Vec<u32> = ink(&drawn(&marked), 0, 0, BACKGROUND)
            .into_iter()
            .filter(|&(_, y)| y < 10)
            .map(|(x, _)| x)
            .collect();
        assert!(!above.is_empty(), "the line stands above the a");
        assert!(
            above.iter().all(|x| (5..=10).contains(x)),
            "the line is in the middle of the cell: {above:?}"
        );
    }

    #[test]
    fn a_missing_glyph_names_its_code_point_and_cell() {
        let mut buf = buffer(3, 1);
        buf[(2, 0)].set_char('\u{10FFFD}');

        let error = rasterize(&buf).expect_err("no font has U+10FFFD");
        assert_eq!(
            error.to_string(),
            "no font has U+10FFFD '\\u{10fffd}' (column 2, row 0)"
        );
    }

    #[test]
    fn a_png_decodes_to_the_picture() {
        let mut buf = buffer(2, 1);
        buf[(0, 0)].set_char('a');
        let image = drawn(&buf);

        let bytes = encode_png(&image, CompressionType::Best);
        let decoded = image::load_from_memory(&bytes)
            .map(|decoded| decoded.to_rgb8())
            .ok();
        assert_eq!(decoded.as_ref(), Some(&image));
    }

    /// Guard against drawing that depends on anything but the buffer; its
    /// first assertion fails on an empty picture.
    #[test]
    fn the_same_buffer_gives_the_same_png_bytes() {
        let mut buf = buffer(4, 2);
        buf.set_string(0, 0, "a☐╭█", Style::new().fg(Color::Cyan).bold());
        let first = encode_png(&drawn(&buf), CompressionType::Fast);

        assert!(!first.is_empty());
        assert_eq!(first, encode_png(&drawn(&buf), CompressionType::Fast));
    }
}
