// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Talqi's emblem as a fixed block of text cells: the four
//! templates built into asqr, read from the text chafa wrote for them,
//! and drawn into a buffer. `scripts/emblem-cells.py` makes the templates.

use std::sync::LazyLock;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget;

/// The glyphs Talqi's emblem is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyphs {
    /// Sextants (U+1FB00..1FB3B) with `█▌▐`, for terminals that draw
    /// sextants themselves.
    Sextant,
    /// Quadrants (U+2596..259F) with `▀▄█▌▐`, which the fonts of every
    /// other terminal have.
    Quadrant,
}

impl Glyphs {
    /// The template of the emblem in these glyphs: in colour, or the
    /// outline for `NO_COLOR`.
    pub(super) fn template(self, outline: bool) -> &'static Template {
        static COLOUR_SEXTANT: LazyLock<Template> =
            LazyLock::new(|| built_in(include_str!("talqi-colour-sextant.ansi")));
        static COLOUR_QUADRANT: LazyLock<Template> =
            LazyLock::new(|| built_in(include_str!("talqi-colour-quad.ansi")));
        static OUTLINE_SEXTANT: LazyLock<Template> =
            LazyLock::new(|| built_in(include_str!("talqi-outline-sextant.txt")));
        static OUTLINE_QUADRANT: LazyLock<Template> =
            LazyLock::new(|| built_in(include_str!("talqi-outline-quad.txt")));
        match (self, outline) {
            (Glyphs::Sextant, false) => &COLOUR_SEXTANT,
            (Glyphs::Quadrant, false) => &COLOUR_QUADRANT,
            (Glyphs::Sextant, true) => &OUTLINE_SEXTANT,
            (Glyphs::Quadrant, true) => &OUTLINE_QUADRANT,
        }
    }
}

/// A template built into asqr, which `scripts/emblem-cells.py` made and
/// the tests check.
fn built_in(text: &str) -> Template {
    Template::parse(text).expect("the templates built into asqr parse")
}

/// The columns and rows of every template.
pub(super) const EMBLEM_COLUMNS: u16 = 44;
pub(super) const EMBLEM_ROWS: u16 = 22;

/// One cell of a template: its glyph and the colours chafa gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TemplateCell {
    symbol: char,
    fg: Option<Color>,
    bg: Option<Color>,
    /// Foreground and background swapped. chafa writes a cell whose glyph
    /// shows the terminal's background, and whose rest is a colour, as
    /// that glyph reversed over that colour as its foreground.
    reversed: bool,
}

impl TemplateCell {
    /// Whether the terminal's background shows through the whole cell.
    /// chafa writes the colour of such a space only when the next cell
    /// needs it, so its foreground means nothing.
    fn transparent(self) -> bool {
        self.symbol == ' ' && self.bg.is_none() && !self.reversed
    }

    /// Whether the cell has no style of its own, as every cell of an
    /// outline.
    fn plain(self) -> bool {
        self.fg.is_none() && self.bg.is_none() && !self.reversed
    }
}

/// The emblem as rows of cells, read from the text chafa writes.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Template {
    rows: Vec<Vec<TemplateCell>>,
}

impl Template {
    /// Reads `text`, chafa's output: one line per row, a glyph per cell,
    /// and between them SGR sequences that set the colours of the cells
    /// that follow, until the next sequence changes them.
    ///
    /// Only what chafa 1.18.2 writes for the templates is understood:
    /// `0` (reset), `7` (reverse), `38;2;r;g;b` and `48;2;r;g;b` (24-bit
    /// foreground and background), `39` and `49` (default foreground and
    /// background), alone or several in one sequence. Anything else is an
    /// error, so a template made by another chafa that writes more fails
    /// the tests instead of being drawn wrongly.
    fn parse(text: &str) -> Result<Template, String> {
        let mut rows = Vec::new();
        for (number, line) in text.lines().enumerate() {
            rows.push(parse_row(line).map_err(|error| format!("row {}: {error}", number + 1))?);
        }
        Ok(Template { rows })
    }
}

/// The cells of one line of a template. chafa ends every line in a
/// reset, so each row starts from the default colours.
fn parse_row(line: &str) -> Result<Vec<TemplateCell>, String> {
    let mut cells = Vec::new();
    let mut pen = TemplateCell {
        symbol: ' ',
        fg: None,
        bg: None,
        reversed: false,
    };
    let mut chars = line.chars();
    while let Some(symbol) = chars.next() {
        if symbol != '\x1b' {
            cells.push(TemplateCell { symbol, ..pen });
            continue;
        }
        if chars.next() != Some('[') {
            return Err("an escape that starts no CSI sequence".to_owned());
        }
        let mut parameters = String::new();
        loop {
            match chars.next() {
                Some('m') => break,
                Some(digit @ ('0'..='9' | ';')) => parameters.push(digit),
                Some(other) => return Err(format!("a sequence other than SGR, at {other:?}")),
                None => return Err("a sequence cut short by the line's end".to_owned()),
            }
        }
        apply_sgr(&parameters, &mut pen)?;
    }
    Ok(cells)
}

/// Sets `pen` by the SGR `parameters`, such as `38;2;9;23;26;48;2;0;0;0`.
/// An empty parameter counts as `0`, as in every terminal.
fn apply_sgr(parameters: &str, pen: &mut TemplateCell) -> Result<(), String> {
    let numbers: Vec<u16> = parameters
        .split(';')
        .map(|number| {
            if number.is_empty() {
                Ok(0)
            } else {
                number.parse()
            }
        })
        .collect::<Result<_, _>>()
        .map_err(|_| format!("a parameter out of range in {parameters:?}"))?;
    let mut numbers = numbers.into_iter();
    while let Some(parameter) = numbers.next() {
        match parameter {
            0 => {
                pen.fg = None;
                pen.bg = None;
                pen.reversed = false;
            }
            7 => pen.reversed = true,
            39 => pen.fg = None,
            49 => pen.bg = None,
            38 | 48 => {
                let mut channel = || numbers.next().and_then(|value| u8::try_from(value).ok());
                let colour = match (channel(), channel(), channel(), channel()) {
                    (Some(2), Some(red), Some(green), Some(blue)) => Color::Rgb(red, green, blue),
                    _ => return Err(format!("no 24-bit colour in {parameters:?}")),
                };
                if parameter == 38 {
                    pen.fg = Some(colour);
                } else {
                    pen.bg = Some(colour);
                }
            }
            other => return Err(format!("SGR {other} in {parameters:?}")),
        }
    }
    Ok(())
}

/// The template drawn from the area's top-left corner, as far as the area
/// and the buffer reach.
///
/// A transparent cell leaves the buffer's cell as it was, so the
/// terminal's background shows there, on a light terminal as on a dark
/// one. A cell without a style of its own, as every cell of an outline,
/// gets its glyph and no style at all. Any other cell gets its glyph and
/// the colours it has; a colour it lacks stays as the buffer had it.
impl Widget for &Template {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (y, row) in (area.top()..area.bottom()).zip(&self.rows) {
            for (x, cell) in (area.left()..area.right()).zip(row) {
                if cell.transparent() || !buf.area.contains(Position { x, y }) {
                    continue;
                }
                let drawn = &mut buf[(x, y)];
                if cell.plain() {
                    drawn.reset();
                }
                drawn.set_char(cell.symbol);
                if let Some(fg) = cell.fg {
                    drawn.set_fg(fg);
                }
                if let Some(bg) = cell.bg {
                    drawn.set_bg(bg);
                }
                if cell.reversed {
                    drawn.modifier.insert(Modifier::REVERSED);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Cell;

    use super::*;

    /// Every template built into asqr, with its name.
    fn templates() -> [(&'static str, &'static str); 4] {
        [
            ("colour sextant", include_str!("talqi-colour-sextant.ansi")),
            ("colour quad", include_str!("talqi-colour-quad.ansi")),
            ("outline sextant", include_str!("talqi-outline-sextant.txt")),
            ("outline quad", include_str!("talqi-outline-quad.txt")),
        ]
    }

    #[test]
    fn every_template_parses_into_22_rows_of_44_cells() {
        for (name, text) in templates() {
            let template = Template::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));

            assert_eq!(template.rows.len(), usize::from(EMBLEM_ROWS), "{name}");
            for row in &template.rows {
                assert_eq!(row.len(), usize::from(EMBLEM_COLUMNS), "{name}");
            }
        }
    }

    /// The outlines carry no escape sequence at all, and so no cell of
    /// theirs has a colour or is reversed.
    #[test]
    fn the_outlines_are_plain_text() {
        for glyphs in [Glyphs::Sextant, Glyphs::Quadrant] {
            let outline = glyphs.template(true);
            assert!(
                outline.rows.iter().flatten().all(|cell| cell.plain()),
                "{glyphs:?}"
            );
            assert!(
                outline.rows.iter().flatten().any(|cell| cell.symbol != ' '),
                "{glyphs:?} has ink"
            );
        }
        for (name, text) in &templates()[2..] {
            assert!(!text.contains('\x1b'), "{name}");
        }
    }

    /// Each glyph set's templates hold the glyphs of that set and blanks:
    /// sextants or quadrants, and the half and full blocks chafa adds to
    /// both.
    #[test]
    fn each_template_holds_the_glyphs_of_its_set() {
        let sextant = |c: char| ('\u{1FB00}'..='\u{1FB3B}').contains(&c);
        let quadrant = |c: char| ('\u{2596}'..='\u{259F}').contains(&c);
        for (glyphs, own) in [
            (Glyphs::Sextant, &sextant as &dyn Fn(char) -> bool),
            (Glyphs::Quadrant, &quadrant),
        ] {
            for outline in [false, true] {
                let template = glyphs.template(outline);
                for cell in template.rows.iter().flatten() {
                    assert!(
                        own(cell.symbol) || " ▀▄█▌▐".contains(cell.symbol),
                        "{glyphs:?} outline {outline}: {:?}",
                        cell.symbol
                    );
                }
                assert!(
                    template.rows.iter().flatten().any(|cell| own(cell.symbol)),
                    "{glyphs:?} outline {outline}"
                );
            }
        }
    }

    /// The colour templates paint with 24-bit colours, and chafa's
    /// reversed cells come through.
    #[test]
    fn the_colour_templates_have_rgb_colours_and_reversed_cells() {
        for glyphs in [Glyphs::Sextant, Glyphs::Quadrant] {
            let cells: Vec<_> = glyphs.template(false).rows.iter().flatten().collect();
            assert!(
                cells.iter().all(|cell| cell
                    .fg
                    .into_iter()
                    .chain(cell.bg)
                    .all(|colour| matches!(colour, Color::Rgb(..)))),
                "{glyphs:?}"
            );
            assert!(cells.iter().any(|cell| cell.reversed), "{glyphs:?}");
            assert!(cells.iter().any(|cell| cell.bg.is_some()), "{glyphs:?}");
        }
    }

    fn rgb_cell(symbol: char, fg: Option<(u8, u8, u8)>, bg: Option<(u8, u8, u8)>) -> TemplateCell {
        let colour = |(red, green, blue)| Color::Rgb(red, green, blue);
        TemplateCell {
            symbol,
            fg: fg.map(colour),
            bg: bg.map(colour),
            reversed: false,
        }
    }

    /// One row parsed from `line`.
    fn row_of(line: &str) -> Vec<TemplateCell> {
        let template = Template::parse(line).expect("parses");
        assert_eq!(template.rows.len(), 1);
        template.rows.into_iter().next().expect("one row")
    }

    /// chafa writes a colour only when it changes, so it carries over to
    /// the cells that follow, also when fore- and background come in one
    /// sequence.
    #[test]
    fn colours_carry_over_until_a_sequence_changes_them() {
        let row = row_of("\x1b[38;2;1;2;3ma\x1b[48;2;4;5;6mbc\x1b[38;2;7;8;9;48;2;10;11;12md");

        assert_eq!(
            row,
            [
                rgb_cell('a', Some((1, 2, 3)), None),
                rgb_cell('b', Some((1, 2, 3)), Some((4, 5, 6))),
                rgb_cell('c', Some((1, 2, 3)), Some((4, 5, 6))),
                rgb_cell('d', Some((7, 8, 9)), Some((10, 11, 12))),
            ]
        );
    }

    #[test]
    fn reset_and_the_defaults_take_colours_away() {
        let row = row_of(
            "\x1b[7;38;2;1;2;3;48;2;4;5;6ma\x1b[39mb\x1b[49mc\x1b[38;2;1;2;3;48;2;4;5;6md\x1b[0me\x1b[7mf\x1b[mg",
        );

        let reversed = |cell: TemplateCell| TemplateCell {
            reversed: true,
            ..cell
        };
        assert_eq!(
            row,
            [
                reversed(rgb_cell('a', Some((1, 2, 3)), Some((4, 5, 6)))),
                reversed(rgb_cell('b', None, Some((4, 5, 6)))),
                reversed(rgb_cell('c', None, None)),
                reversed(rgb_cell('d', Some((1, 2, 3)), Some((4, 5, 6)))),
                rgb_cell('e', None, None),
                reversed(rgb_cell('f', None, None)),
                rgb_cell('g', None, None),
            ]
        );
    }

    /// Every line starts from the default colours, as chafa ends each in
    /// a reset.
    #[test]
    fn each_line_is_a_row_that_starts_without_colours() {
        let template = Template::parse("\x1b[38;2;1;2;3mab\nc\n").expect("parses");

        assert_eq!(
            template.rows,
            [
                vec![
                    rgb_cell('a', Some((1, 2, 3)), None),
                    rgb_cell('b', Some((1, 2, 3)), None)
                ],
                vec![rgb_cell('c', None, None)],
            ]
        );
    }

    #[test]
    fn anything_but_the_sgr_chafa_writes_fails() {
        for (text, why) in [
            ("\x1b[1ma", "bold"),
            ("\x1b[27ma", "reverse off"),
            ("\x1b[38;5;3ma", "an indexed colour"),
            ("\x1b[48;2;1;2ma", "a colour cut short"),
            ("\x1b[38;2;1;2;256ma", "a channel out of range"),
            ("\x1b[38;2;1;2;99999ma", "a parameter out of range"),
            ("\x1b[?25la", "hiding the cursor"),
            ("\x1b[2Ja", "erasing the screen"),
            ("\x1b]0;title\x07a", "an OSC"),
            ("a\x1b[38;2", "a sequence cut short"),
            ("a\x1b", "an escape at the end"),
        ] {
            assert!(Template::parse(text).is_err(), "{why}: {text:?}");
        }
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

    /// The template of `line` drawn into a [`marked`] buffer of one row.
    fn drawn_line(line: &str) -> Buffer {
        let template = Template::parse(line).expect("parses");
        let width = u16::try_from(template.rows[0].len()).expect("a short line");
        let mut buf = marked(width, 1);
        template.render(buf.area, &mut buf);
        buf
    }

    #[test]
    fn a_transparent_space_leaves_the_cell_as_it_was() {
        let buf = drawn_line("\x1b[38;2;0;0;0m \x1b[0m ");

        assert_eq!(buf, marked(2, 1));
    }

    /// A space with a background is a cell of that colour, and a glyph
    /// whose background is the terminal's keeps the buffer's.
    #[test]
    fn a_cell_takes_the_colours_it_has_and_keeps_the_others() {
        let buf = drawn_line("\x1b[48;2;4;5;6m \x1b[0m\x1b[38;2;1;2;3m▀");

        let mut space = Cell::default();
        space
            .set_symbol(" ")
            .set_fg(Color::Blue)
            .set_bg(Color::Rgb(4, 5, 6));
        let mut glyph = Cell::default();
        glyph
            .set_symbol("▀")
            .set_fg(Color::Rgb(1, 2, 3))
            .set_bg(Color::White);
        assert_eq!(buf[(0, 0)], space);
        assert_eq!(buf[(1, 0)], glyph);
    }

    /// A reversed cell keeps its foreground and is drawn reversed, so its
    /// ink shows the background, even a reversed space.
    #[test]
    fn a_reversed_cell_is_drawn_reversed() {
        let buf = drawn_line("\x1b[7m\x1b[38;2;1;2;3m🬆 ");

        for x in 0..2 {
            let cell = &buf[(x, 0)];
            assert!(cell.modifier.contains(Modifier::REVERSED), "{cell:?}");
            assert_eq!((cell.fg, cell.bg), (Color::Rgb(1, 2, 3), Color::White));
        }
        assert_eq!(buf[(0, 0)].symbol(), "🬆");
    }

    /// A cell of an outline is its glyph and nothing else, no colour and
    /// no modifier, and a blank leaves the buffer as it was.
    #[test]
    fn an_outline_draws_its_ink_without_a_style() {
        let buf = drawn_line("🬞▐ ");

        for (x, symbol) in [(0, "🬞"), (1, "▐")] {
            let mut plain = Cell::default();
            plain.set_symbol(symbol);
            assert_eq!(buf[(x, 0)], plain);
        }
        assert_eq!(buf[(2, 0)], marked(1, 1)[(0, 0)]);
    }

    /// Drawn at an offset into a smaller buffer, the template holds its
    /// top-left part there, and nothing is drawn past the buffer's edge.
    #[test]
    fn a_template_ends_where_the_area_and_the_buffer_end() {
        let outline = Glyphs::Quadrant.template(true);
        let mut whole = Buffer::empty(Rect::new(0, 0, EMBLEM_COLUMNS + 2, EMBLEM_ROWS + 1));
        outline.render(Rect::new(2, 1, EMBLEM_COLUMNS, EMBLEM_ROWS), &mut whole);

        let mut part = Buffer::empty(Rect::new(0, 0, 20, 10));
        outline.render(Rect::new(2, 1, EMBLEM_COLUMNS, EMBLEM_ROWS), &mut part);
        let mut cut = Buffer::empty(Rect::new(0, 0, EMBLEM_COLUMNS + 2, EMBLEM_ROWS + 1));
        outline.render(Rect::new(2, 1, 10, 5), &mut cut);

        for position in part.area.positions() {
            assert_eq!(part[position], whole[position], "{position:?}");
        }
        for position in cut.area.positions() {
            let inside = Rect::new(2, 1, 10, 5).contains(position);
            let expected = if inside {
                whole[position].clone()
            } else {
                Cell::default()
            };
            assert_eq!(cut[position], expected, "{position:?}");
        }
        assert_eq!(whole[(2 + 18, 1)].symbol(), "▄", "the first row's ink");
    }
}
