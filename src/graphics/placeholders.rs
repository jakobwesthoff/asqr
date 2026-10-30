// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Kitty images that something was drawn over (ADR 48).
//!
//! ratatui-image 12.0.0-rc.3 draws a Kitty image as cells of the
//! placeholder character, and only the first cell of each image row
//! carries the diacritics for its row, its column and the high byte of the
//! image's id. The cells after it carry none: by the Kitty spec they take
//! their place from the placeholder on their left that has the same
//! colour, and where there is none, the terminal does not have to draw
//! anything. An overlay drawn over the first cells of image rows leaves
//! the cells right of it without such a neighbour, and Ghostty draws
//! nothing there.
//!
//! ratatui-image also puts the image's transmission in front of the first
//! image cell, once per encoding, so an overlay over that cell takes the
//! image's data with it.
//!
//! The screen notes the Kitty image it draws as a [`PlacedImage`] and,
//! once everything over it is drawn, mends the frame: each placeholder
//! after a gap gets its full position again, and a transmission an
//! overlay covered moves to a cell that is still written.

use std::num::NonZeroU16;

use ratatui::buffer::{Buffer, CellDiffOption, CellWidth};
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;

/// The character the Kitty protocol reserves for image placeholders.
pub(super) const PLACEHOLDER: char = '\u{10EEEE}';

/// The diacritics that stand for the numbers 0 to 296 in a placeholder:
/// its row, its column and the high byte of its image's id. Copied in
/// order from `rowcolumn-diacritics.txt`, which the "Unicode
/// placeholders" section of the Kitty graphics protocol links:
/// <https://sw.kovidgoyal.net/kitty/graphics-protocol/#unicode-placeholders>
#[rustfmt::skip]
pub(super) static DIACRITICS: [char; 297] = [
    '\u{305}', '\u{30D}', '\u{30E}', '\u{310}', '\u{312}', '\u{33D}', '\u{33E}',
    '\u{33F}', '\u{346}', '\u{34A}', '\u{34B}', '\u{34C}', '\u{350}', '\u{351}',
    '\u{352}', '\u{357}', '\u{35B}', '\u{363}', '\u{364}', '\u{365}', '\u{366}',
    '\u{367}', '\u{368}', '\u{369}', '\u{36A}', '\u{36B}', '\u{36C}', '\u{36D}',
    '\u{36E}', '\u{36F}', '\u{483}', '\u{484}', '\u{485}', '\u{486}', '\u{487}',
    '\u{592}', '\u{593}', '\u{594}', '\u{595}', '\u{597}', '\u{598}', '\u{599}',
    '\u{59C}', '\u{59D}', '\u{59E}', '\u{59F}', '\u{5A0}', '\u{5A1}', '\u{5A8}',
    '\u{5A9}', '\u{5AB}', '\u{5AC}', '\u{5AF}', '\u{5C4}', '\u{610}', '\u{611}',
    '\u{612}', '\u{613}', '\u{614}', '\u{615}', '\u{616}', '\u{617}', '\u{657}',
    '\u{658}', '\u{659}', '\u{65A}', '\u{65B}', '\u{65D}', '\u{65E}', '\u{6D6}',
    '\u{6D7}', '\u{6D8}', '\u{6D9}', '\u{6DA}', '\u{6DB}', '\u{6DC}', '\u{6DF}',
    '\u{6E0}', '\u{6E1}', '\u{6E2}', '\u{6E4}', '\u{6E7}', '\u{6E8}', '\u{6EB}',
    '\u{6EC}', '\u{730}', '\u{732}', '\u{733}', '\u{735}', '\u{736}', '\u{73A}',
    '\u{73D}', '\u{73F}', '\u{740}', '\u{741}', '\u{743}', '\u{745}', '\u{747}',
    '\u{749}', '\u{74A}', '\u{7EB}', '\u{7EC}', '\u{7ED}', '\u{7EE}', '\u{7EF}',
    '\u{7F0}', '\u{7F1}', '\u{7F3}', '\u{816}', '\u{817}', '\u{818}', '\u{819}',
    '\u{81B}', '\u{81C}', '\u{81D}', '\u{81E}', '\u{81F}', '\u{820}', '\u{821}',
    '\u{822}', '\u{823}', '\u{825}', '\u{826}', '\u{827}', '\u{829}', '\u{82A}',
    '\u{82B}', '\u{82C}', '\u{82D}', '\u{951}', '\u{953}', '\u{954}', '\u{F82}',
    '\u{F83}', '\u{F86}', '\u{F87}', '\u{135D}', '\u{135E}', '\u{135F}',
    '\u{17DD}', '\u{193A}', '\u{1A17}', '\u{1A75}', '\u{1A76}', '\u{1A77}',
    '\u{1A78}', '\u{1A79}', '\u{1A7A}', '\u{1A7B}', '\u{1A7C}', '\u{1B6B}',
    '\u{1B6D}', '\u{1B6E}', '\u{1B6F}', '\u{1B70}', '\u{1B71}', '\u{1B72}',
    '\u{1B73}', '\u{1CD0}', '\u{1CD1}', '\u{1CD2}', '\u{1CDA}', '\u{1CDB}',
    '\u{1CE0}', '\u{1DC0}', '\u{1DC1}', '\u{1DC3}', '\u{1DC4}', '\u{1DC5}',
    '\u{1DC6}', '\u{1DC7}', '\u{1DC8}', '\u{1DC9}', '\u{1DCB}', '\u{1DCC}',
    '\u{1DD1}', '\u{1DD2}', '\u{1DD3}', '\u{1DD4}', '\u{1DD5}', '\u{1DD6}',
    '\u{1DD7}', '\u{1DD8}', '\u{1DD9}', '\u{1DDA}', '\u{1DDB}', '\u{1DDC}',
    '\u{1DDD}', '\u{1DDE}', '\u{1DDF}', '\u{1DE0}', '\u{1DE1}', '\u{1DE2}',
    '\u{1DE3}', '\u{1DE4}', '\u{1DE5}', '\u{1DE6}', '\u{1DFE}', '\u{20D0}',
    '\u{20D1}', '\u{20D4}', '\u{20D5}', '\u{20D6}', '\u{20D7}', '\u{20DB}',
    '\u{20DC}', '\u{20E1}', '\u{20E7}', '\u{20E9}', '\u{20F0}', '\u{2CEF}',
    '\u{2CF0}', '\u{2CF1}', '\u{2DE0}', '\u{2DE1}', '\u{2DE2}', '\u{2DE3}',
    '\u{2DE4}', '\u{2DE5}', '\u{2DE6}', '\u{2DE7}', '\u{2DE8}', '\u{2DE9}',
    '\u{2DEA}', '\u{2DEB}', '\u{2DEC}', '\u{2DED}', '\u{2DEE}', '\u{2DEF}',
    '\u{2DF0}', '\u{2DF1}', '\u{2DF2}', '\u{2DF3}', '\u{2DF4}', '\u{2DF5}',
    '\u{2DF6}', '\u{2DF7}', '\u{2DF8}', '\u{2DF9}', '\u{2DFA}', '\u{2DFB}',
    '\u{2DFC}', '\u{2DFD}', '\u{2DFE}', '\u{2DFF}', '\u{A66F}', '\u{A67C}',
    '\u{A67D}', '\u{A6F0}', '\u{A6F1}', '\u{A8E0}', '\u{A8E1}', '\u{A8E2}',
    '\u{A8E3}', '\u{A8E4}', '\u{A8E5}', '\u{A8E6}', '\u{A8E7}', '\u{A8E8}',
    '\u{A8E9}', '\u{A8EA}', '\u{A8EB}', '\u{A8EC}', '\u{A8ED}', '\u{A8EE}',
    '\u{A8EF}', '\u{A8F0}', '\u{A8F1}', '\u{AAB0}', '\u{AAB2}', '\u{AAB3}',
    '\u{AAB7}', '\u{AAB8}', '\u{AABE}', '\u{AABF}', '\u{AAC1}', '\u{FE20}',
    '\u{FE21}', '\u{FE22}', '\u{FE23}', '\u{FE24}', '\u{FE25}', '\u{FE26}',
    '\u{10A0F}', '\u{10A38}', '\u{1D185}', '\u{1D186}', '\u{1D187}',
    '\u{1D188}', '\u{1D189}', '\u{1D1AA}', '\u{1D1AB}', '\u{1D1AC}',
    '\u{1D1AD}', '\u{1D242}', '\u{1D243}', '\u{1D244}',
];

/// What the placeholder character shows as where text holds it.
const STAND_IN: char = '\u{FFFD}';

/// Replaces the placeholder character with `�` in every cell of `buf`
/// that is not one of `image`'s placeholders (ADR 67). Kitty draws an
/// image's cells wherever a cell holds the character in a colour that
/// names an image, and the text of a session can hold it, in any colour a
/// code entry's highlighting or an `ansi` entry gives it. Only the image
/// the frame drew keeps it. The pass runs under every protocol, as the
/// terminal can be Kitty while asqr draws its images in blocks.
pub fn disarm_placeholders(buf: &mut Buffer, image: Option<&PlacedImage>) {
    let area = buf.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let held = buf[(x, y)].symbol().contains(PLACEHOLDER);
            if held && !image.is_some_and(|image| image.holds(buf, x, y)) {
                let symbol = buf[(x, y)]
                    .symbol()
                    .replace(PLACEHOLDER, &STAND_IN.to_string());
                buf[(x, y)].set_symbol(&symbol);
            }
        }
    }
}

/// One Kitty image as a frame drew it: the cells its placeholders cover,
/// its id, and the transmission its first cell carried, which only the
/// first draw of an encoding has.
#[derive(Debug)]
pub struct PlacedImage {
    area: Rect,
    id: u32,
    transmission: Option<String>,
}

impl PlacedImage {
    /// The image with `id` whose `size` cells ratatui-image just drew into
    /// `area` of `buf`, read back from the first of them.
    pub(super) fn read(buf: &Buffer, area: Rect, size: Size, id: u32) -> PlacedImage {
        // ratatui-image draws no more cells than the image has, and none
        // past the table, whose marks could not name them.
        let most = u16::try_from(DIACRITICS.len()).expect("the table has 297 marks");
        let area = Rect {
            width: area.width.min(size.width).min(most),
            height: area.height.min(size.height).min(most),
            ..area
        }
        .intersection(buf.area);
        let transmission = (!area.is_empty())
            .then(|| {
                let first = buf[(area.x, area.y)].symbol();
                let sent = &first[..first.find(PLACEHOLDER)?];
                (!sent.is_empty()).then(|| sent.to_owned())
            })
            .flatten();
        PlacedImage {
            area,
            id,
            transmission,
        }
    }

    /// Mends this image in `buf` where something was drawn over it: each
    /// placeholder of it with no placeholder of it on its left gets the
    /// marks for its row, its column and the id's high byte, and a
    /// transmission whose cell was drawn over moves to the first cell of
    /// the image still drawn, or to the first cell of `buf` where none is.
    pub fn mend(&self, buf: &mut Buffer) {
        let [high, ..] = self.id.to_be_bytes();

        let mut first_drawn = None;
        for y in self.area.top()..self.area.bottom() {
            for x in self.area.left()..self.area.right() {
                if !self.holds(buf, x, y) {
                    continue;
                }
                first_drawn.get_or_insert((x, y));
                let after_gap = x == self.area.left() || !self.holds(buf, x - 1, y);
                let cell = &mut buf[(x, y)];
                // The first cell of each row carries its marks already.
                if after_gap && cell.symbol().ends_with(PLACEHOLDER) {
                    let mut symbol = cell.symbol().to_owned();
                    symbol.push(DIACRITICS[usize::from(y - self.area.top())]);
                    symbol.push(DIACRITICS[usize::from(x - self.area.left())]);
                    symbol.push(DIACRITICS[usize::from(high)]);
                    cell.set_symbol(&symbol);
                }
            }
        }

        let Some(transmission) = &self.transmission else {
            return;
        };
        if buf[(self.area.x, self.area.y)]
            .symbol()
            .starts_with(transmission.as_str())
        {
            return;
        }
        // A placeholder keeps the width ratatui-image forced on it. Any
        // other cell is forced to the width it has, as ratatui's diff would
        // measure the escape sequence as columns and skip the cells after
        // it.
        let (x, y) = first_drawn.unwrap_or((buf.area.x, buf.area.y));
        let cell = &mut buf[(x, y)];
        let width = NonZeroU16::new(cell.cell_width().max(1)).expect("at least 1");
        let symbol = format!("{transmission}{}", cell.symbol());
        cell.set_symbol(&symbol)
            .set_diff_option(CellDiffOption::ForcedWidth(width));
    }

    /// Whether the cell at `x`, `y` of `buf` is one of this image's
    /// placeholders: inside its area, in the colour that names its id,
    /// holding the placeholder character.
    fn holds(&self, buf: &Buffer, x: u16, y: u16) -> bool {
        let [_, red, green, blue] = self.id.to_be_bytes();
        let cell = &buf[(x, y)];
        self.area.contains((x, y).into())
            && cell.fg == Color::Rgb(red, green, blue)
            && cell.symbol().contains(PLACEHOLDER)
    }

    #[cfg(test)]
    pub(crate) fn area(&self) -> Rect {
        self.area
    }

    #[cfg(test)]
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    /// The transmission this frame put in front of the image, which only
    /// the first draw of an encoding carries.
    pub fn transmission(&self) -> Option<&str> {
        self.transmission.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use ::image::DynamicImage;
    use ratatui::widgets::Widget;
    use ratatui_image::protocol::Protocol as Encoded;
    use ratatui_image::protocol::kitty::Kitty;

    use super::super::testing::{draw_kitty, kitty_positions};
    use super::*;

    /// An id with a high byte, which a placeholder can only name with its
    /// third mark.
    const ID: u32 = 0x2A00_0001;

    /// Where the tests draw their image: 6×3 cells, two columns in.
    const IMAGE: Rect = Rect::new(2, 0, 6, 3);

    /// A transparent picture encoded for `IMAGE` under `ID`, which hands
    /// out its transmission on its first draw only.
    fn encoded() -> Encoded {
        let kitty = Kitty::new(
            DynamicImage::new_rgba8(1, 1),
            IMAGE.as_size(),
            ID,
            false,
            false,
            false,
        )
        .expect("a picture of one pixel encodes");
        Encoded::Kitty(kitty)
    }

    /// `encoded` drawn into `IMAGE` of `buf` as the screen draws it, and
    /// read back.
    fn drawn(buf: &mut Buffer, encoded: &Encoded) -> PlacedImage {
        ratatui_image::Image::new(encoded).render(IMAGE, buf);
        PlacedImage::read(buf, IMAGE, IMAGE.as_size(), ID)
    }

    /// A buffer of 10×3 cells with the image's first draw in it.
    fn first_draw() -> (Buffer, PlacedImage) {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        let placed = drawn(&mut buf, &encoded());
        (buf, placed)
    }

    /// Writes text over the cells `columns` of rows `rows`, as an overlay
    /// clears its place and draws into it.
    fn cover(buf: &mut Buffer, columns: std::ops::Range<u16>, rows: std::ops::Range<u16>) {
        for y in rows {
            for x in columns.clone() {
                buf[(x, y)].reset();
                buf[(x, y)].set_symbol("x");
            }
        }
    }

    /// The cells of `buf` that hold a Kitty command.
    fn sending(buf: &Buffer) -> Vec<(u16, u16)> {
        let area = buf.area;
        (area.top()..area.bottom())
            .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
            .filter(|&cell| buf[cell].symbol().contains("\x1b_G"))
            .collect()
    }

    /// Asserts that the terminal places every placeholder of `buf` at its
    /// own cell of the image.
    fn assert_all_placed(buf: &Buffer) {
        for ((x, y), position) in kitty_positions(buf) {
            assert_eq!(
                position,
                Some((y - IMAGE.y, x - IMAGE.x, ID)),
                "the cell at {x},{y}"
            );
        }
    }

    /// The table is the spec's, as ratatui-image draws it: the row marks of
    /// a column as tall as the table, and the high byte marks of all 256
    /// ids that differ in it.
    #[test]
    fn the_diacritics_are_those_ratatui_image_draws() {
        let column = Rect::new(0, 0, 1, 297);
        let mut buf = Buffer::empty(column);
        draw_kitty(&mut buf, 0, column);
        for (y, mark) in (0..).zip(DIACRITICS) {
            // The first cell carries the transmission before its marks.
            let symbol = buf[(0, y)].symbol();
            let marks = &symbol[symbol.find(PLACEHOLDER).expect("a placeholder")..];
            assert_eq!(marks.chars().nth(1), Some(mark), "row {y}");
        }

        let cell = Rect::new(0, 0, 1, 1);
        for (high, mark) in (0..=u8::MAX).zip(DIACRITICS) {
            let mut buf = Buffer::empty(cell);
            draw_kitty(&mut buf, u32::from(high) << 24, cell);
            assert_eq!(buf[(0, 0)].symbol().chars().last(), Some(mark), "{high}");
        }
    }

    /// Without anything drawn over it, the image keeps its cells and its
    /// transmission where ratatui-image put them.
    #[test]
    fn an_image_nothing_was_drawn_over_is_left_as_it_is() {
        let (mut buf, placed) = first_draw();
        let before = buf.clone();

        placed.mend(&mut buf);

        assert_eq!(buf, before);
    }

    /// Text over the first cells of each image row leaves the cells right
    /// of it with nothing on their left; the first of them gets its whole
    /// place, and those after it take theirs from it.
    #[test]
    fn a_placeholder_after_a_gap_gets_its_whole_place() {
        let (mut buf, placed) = first_draw();
        cover(&mut buf, 3..5, 0..3);
        let before = buf.clone();

        placed.mend(&mut buf);

        assert_all_placed(&buf);
        assert_eq!(
            kitty_positions(&buf).len(),
            12,
            "two of six columns covered"
        );
        let changed: Vec<_> = before
            .diff(&buf)
            .into_iter()
            .map(|(x, y, _)| (x, y))
            .collect();
        assert_eq!(changed, [(5, 0), (5, 1), (5, 2)], "one cell per row");
    }

    /// Text over the image's first cell takes the transmission with it; it
    /// moves to the first cell of the image still drawn.
    #[test]
    fn a_covered_transmission_moves_to_the_first_cell_still_drawn() {
        let (mut buf, placed) = first_draw();
        let transmission = placed.transmission().expect("a first draw").to_owned();
        cover(&mut buf, 2..4, 0..1);

        placed.mend(&mut buf);

        assert_eq!(sending(&buf), [(4, 0)]);
        assert!(buf[(4, 0)].symbol().starts_with(&transmission));
        assert_all_placed(&buf);
    }

    /// With the whole image covered, the transmission moves to the first
    /// cell of the screen, which keeps the width its own text has.
    /// Measured from the escape sequence, the cell would take many columns,
    /// and ratatui would skip the cells after it.
    #[test]
    fn a_transmission_of_an_image_covered_whole_moves_to_the_first_cell_of_the_screen() {
        let width = |columns| CellDiffOption::ForcedWidth(NonZeroU16::new(columns).expect("not 0"));
        for (text, columns) in [("a", 1), ("日", 2)] {
            let (mut buf, placed) = first_draw();
            let transmission = placed.transmission().expect("a first draw").to_owned();
            cover(&mut buf, 2..8, 0..3);
            buf[(0, 0)].set_symbol(text);
            if columns == 1 {
                buf[(1, 0)].set_symbol("b");
            }

            placed.mend(&mut buf);

            assert_eq!(sending(&buf), [(0, 0)], "{text}");
            let first = &buf[(0, 0)];
            assert_eq!(first.symbol(), format!("{transmission}{text}"));
            assert_eq!(first.diff_option, width(columns), "{text}");
            if columns == 1 {
                let sent: Vec<_> = Buffer::empty(buf.area)
                    .diff(&buf)
                    .into_iter()
                    .map(|(x, y, _)| (x, y))
                    .collect();
                assert!(
                    sent.contains(&(1, 0)),
                    "the cell after it is sent: {sent:?}"
                );
            }
        }
    }

    /// Text holding the placeholder character in its cells, one of them
    /// with the marks of a position.
    fn text_with_placeholders(buf: &mut Buffer) {
        let marked = format!("{PLACEHOLDER}{}{}", DIACRITICS[0], DIACRITICS[1]);
        buf[(0, 0)].set_symbol(&marked);
        buf[(1, 1)].set_symbol(&PLACEHOLDER.to_string());
    }

    #[test]
    fn the_placeholder_character_in_text_turns_into_a_replacement_character() {
        let (mut buf, placed) = first_draw();
        let image: Vec<_> = IMAGE.positions().map(|cell| buf[cell].clone()).collect();
        text_with_placeholders(&mut buf);
        // A cell inside the image's area in another colour is no cell of it.
        buf[(3, 1)].set_fg(Color::Indexed(1));

        disarm_placeholders(&mut buf, Some(&placed));

        assert_eq!(
            buf[(0, 0)].symbol(),
            format!("\u{FFFD}{}{}", DIACRITICS[0], DIACRITICS[1]),
            "its marks stay"
        );
        assert_eq!(buf[(1, 1)].symbol(), "\u{FFFD}");
        assert!(buf[(3, 1)].symbol().starts_with('\u{FFFD}'));
        for (cell, before) in IMAGE.positions().zip(&image) {
            if cell != (3, 1).into() {
                assert_eq!(&buf[cell], before, "{cell:?}");
            }
        }
        assert!(
            buf[(IMAGE.x, IMAGE.y)].symbol().contains("\x1b_G"),
            "the first cell keeps its transmission"
        );
    }

    #[test]
    fn without_an_image_every_placeholder_character_is_replaced() {
        let (mut buf, _) = first_draw();
        text_with_placeholders(&mut buf);

        disarm_placeholders(&mut buf, None);

        assert!(
            buf.content()
                .iter()
                .all(|cell| !cell.symbol().contains(PLACEHOLDER))
        );
    }

    /// Either character takes one column, so replacing one keeps every
    /// cell after it where it was.
    #[test]
    fn the_replacement_takes_the_columns_of_the_placeholder() {
        use ratatui::text::Span;

        let columns = |character: char| Span::raw(character.to_string()).width();
        assert_eq!((columns(PLACEHOLDER), columns(STAND_IN)), (1, 1));
    }

    /// The second draw of an encoding carries no transmission, and nothing
    /// moves when its first cell is covered.
    #[test]
    fn an_image_drawn_again_carries_no_transmission() {
        let encoded = encoded();
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        drawn(&mut buf, &encoded);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));

        let placed = drawn(&mut buf, &encoded);
        cover(&mut buf, 2..4, 0..1);
        placed.mend(&mut buf);

        assert_eq!(placed.transmission(), None);
        assert_eq!(sending(&buf), Vec::<(u16, u16)>::new());
    }
}
