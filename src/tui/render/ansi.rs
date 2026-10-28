// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewer of `ansi` entries (spec section 8.10, ADR 55): text in the
//! colours and attributes its own SGR escape sequences set, drawn cell by
//! cell. Every other escape sequence is dropped, so an entry cannot change
//! the screen outside its text. Lines never wrap and have no gutter; the
//! pane scrolls sideways, as for code.

use std::path::Path;

use anstyle_parse::{DefaultCharAccumulator, Params, ParamsIter, Parser, Perform};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use super::source_file::{Cut, Source, Unreadable, between_cut_notes, read_source};
use super::source_line::{Note, SourceLine, rows_from};
use super::viewer::{Content, ContentSize, Placeholder, Viewer};
use crate::format::AnsiEntry;
use crate::graphics::PlacedImage;
use crate::tui::Offset;

/// The viewer of `entry`: reads the text and parses it into styled lines
/// once, as nothing of it depends on the body's size. Without `colours`,
/// as under `NO_COLOR`, the text keeps its attributes and no colour at
/// all: the pass after drawing keeps RGB colours for images, so the
/// colours an entry sets go here. The home folder `home` shows as `~` in
/// the path of a file that cannot be read.
pub(super) fn ansi_content(
    entry: &AnsiEntry,
    colours: bool,
    home: Option<&Path>,
) -> Content<Painted> {
    let source = read_source(entry.path.as_deref(), entry.content.as_deref());
    content_of(source, colours, home)
}

/// What the viewer shows of `source`, as read for the entry: its lines,
/// a card when the file cannot be read, or a note when there is no line.
fn content_of(
    source: Result<Source, Unreadable>,
    colours: bool,
    home: Option<&Path>,
) -> Content<Painted> {
    match source {
        Err(unreadable) => Content::Problem(unreadable.card(home)),
        Ok(Source { text, cut }) => {
            let lines = styled_lines(&text, colours);
            if lines.is_empty() {
                Content::Note(Placeholder::note("empty"))
            } else {
                Content::Body(Painted::new(lines, cut))
            }
        }
    }
}

// ---------------------------------------------------------------------
// Reading the escape sequences
// ---------------------------------------------------------------------

/// What stands in for a character outside ASCII when it is fed to the
/// parser.
const DEL: u8 = 0x7f;

/// The lines of `text`, each in the styles its SGR sequences set, without
/// any colour unless `colours`.
///
/// anstyle-parse runs the DEC parser's state machine, which tells every
/// kind of sequence apart and ends each where a terminal does. It decodes
/// UTF-8 only between sequences, though: inside one, a byte of a
/// character outside ASCII can end a string early, as `0x9C` ends a DCS or
/// an APC string. So it gets the text character by character, ASCII as
/// itself and anything else as DEL, which prints between sequences and is
/// ignored inside one in every state, while the reader keeps the
/// character DEL stands for.
fn styled_lines(text: &str, colours: bool) -> Vec<SourceLine> {
    let mut parser = Parser::<DefaultCharAccumulator>::default();
    let mut reader = Reader {
        colours,
        pen: Pen::default(),
        current: '\0',
        line: Vec::new(),
        lines: Vec::new(),
    };
    for character in text.chars() {
        reader.current = character;
        let byte = u8::try_from(character)
            .ok()
            .filter(u8::is_ascii)
            .unwrap_or(DEL);
        parser.advance(&mut reader, byte);
    }
    reader.finish()
}

/// The colours and attributes SGR sequences set, which every character
/// printed takes until a sequence changes them.
#[derive(Debug, Clone, Copy, Default)]
struct Pen {
    fg: Option<Color>,
    bg: Option<Color>,
    modifiers: Modifier,
}

/// Builds the lines of a text from what the parser makes of it.
struct Reader {
    colours: bool,
    pen: Pen,
    /// The character the byte the parser has comes from.
    current: char,
    /// The pieces of the line being read, each text in one style.
    line: Vec<(String, Style)>,
    lines: Vec<SourceLine>,
}

impl Reader {
    /// The style a character printed now takes.
    fn style(&self) -> Style {
        let pen = if self.colours {
            self.pen
        } else {
            Pen {
                fg: None,
                bg: None,
                ..self.pen
            }
        };
        let mut style = Style::new().add_modifier(pen.modifiers);
        if let Some(fg) = pen.fg {
            style = style.fg(fg);
        }
        if let Some(bg) = pen.bg {
            style = style.bg(bg);
        }
        style
    }

    /// Appends `character` in the pen's style. A piece opens only where
    /// the style changes, so sequences without text between them make
    /// none. A character that continues the last grapheme, such as an
    /// accent, a skin tone or what a ZWJ joins, stays in that grapheme's
    /// piece whatever its style: `SourceLine` measures and draws each
    /// piece's graphemes apart, and a grapheme split in two would take
    /// the columns of two.
    fn push(&mut self, character: char) {
        let style = self.style();
        if let Some((text, last)) = self.line.last_mut()
            && (*last == style || continues_grapheme(text, character))
        {
            text.push(character);
            return;
        }
        self.line.push((character.to_string(), style));
    }

    /// Ends the line being read. Its `\r` before the line end goes, so a
    /// `\r\n` shows no `␍`, whatever sequences came between the two.
    fn end_line(&mut self) {
        if let Some((text, _)) = self.line.last_mut()
            && text.ends_with('\r')
        {
            text.pop();
            if text.is_empty() {
                self.line.pop();
            }
        }
        let pieces = std::mem::take(&mut self.line);
        self.lines.push(SourceLine::new(
            pieces.iter().map(|(text, style)| (text.as_str(), *style)),
        ));
    }

    /// The lines read. The last one counts only when it holds a
    /// character, so text that ends in a line end, or in a reset after
    /// it, has no empty line at its end. A sequence still open is
    /// dropped with the parser.
    fn finish(mut self) -> Vec<SourceLine> {
        if !self.line.is_empty() {
            self.end_line();
        }
        self.lines
    }
}

/// Whether `character` continues the last grapheme of `text`.
fn continues_grapheme(text: &str, character: char) -> bool {
    text.graphemes(true).next_back().is_some_and(|last| {
        let mut joined = last.to_owned();
        joined.push(character);
        joined.graphemes(true).count() == 1
    })
}

impl Perform for Reader {
    fn print(&mut self, character: char) {
        let character = if character == char::from(DEL) {
            self.current
        } else {
            character
        };
        self.push(character);
    }

    /// Only control characters arrive here, as the parser gets no byte
    /// above ASCII. They act inside a sequence as outside one. Any other
    /// than the line end is kept, for `SourceLine` to show the tab's
    /// blanks and the pictures of the rest.
    fn execute(&mut self, byte: u8) {
        if byte == b'\n' {
            self.end_line();
        } else {
            self.push(char::from(byte));
        }
    }

    /// Only a plain SGR sequence sets the pen. A private marker or an
    /// intermediate byte makes another function of the same final byte,
    /// such as xterm's `ESC [ > 4 ; 2 m`, and with more than 32 numbers
    /// the parser kept only some of them.
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: u8) {
        if action == b'm' && intermediates.is_empty() && !ignore {
            apply_sgr(&mut self.pen, params);
        }
    }

    // OSC, DCS and the other escape sequences keep the trait's defaults,
    // which do nothing: they are dropped.
}

// ---------------------------------------------------------------------
// SGR parameters
// ---------------------------------------------------------------------

/// Sets `pen` by the parameters of one SGR sequence. Each parameter comes
/// with its subparameters, those written after a `:`; an empty one reads
/// as 0. Bold, dim, italic, underlined, reversed, crossed out and the
/// colours count, the rest is skipped, the arguments of the underline
/// colour `58` included.
fn apply_sgr(pen: &mut Pen, params: &Params) {
    let mut parameters = params.iter();
    while let Some(parameter) = parameters.next() {
        match *parameter {
            [code] => apply_code(pen, code, &mut parameters),
            // ratatui has one underline, so every style of it is that one.
            [4, 0] => pen.modifiers.remove(Modifier::UNDERLINED),
            [4, 1..=5] => pen.modifiers.insert(Modifier::UNDERLINED),
            [code @ (38 | 48 | 58), ref arguments @ ..] => {
                set_colour(pen, code, colon_colour(arguments));
            }
            _ => {}
        }
    }
}

/// Applies the parameter `code`, written without subparameters. A colour
/// written with `;` takes its arguments from `parameters`.
fn apply_code(pen: &mut Pen, code: u16, parameters: &mut ParamsIter<'_>) {
    let modifiers = &mut pen.modifiers;
    match code {
        0 => *pen = Pen::default(),
        1 => modifiers.insert(Modifier::BOLD),
        2 => modifiers.insert(Modifier::DIM),
        3 => modifiers.insert(Modifier::ITALIC),
        4 | 21 => modifiers.insert(Modifier::UNDERLINED),
        7 => modifiers.insert(Modifier::REVERSED),
        9 => modifiers.insert(Modifier::CROSSED_OUT),
        22 => modifiers.remove(Modifier::BOLD | Modifier::DIM),
        23 => modifiers.remove(Modifier::ITALIC),
        24 => modifiers.remove(Modifier::UNDERLINED),
        27 => modifiers.remove(Modifier::REVERSED),
        29 => modifiers.remove(Modifier::CROSSED_OUT),
        30..=37 => pen.fg = Some(slot(code - 30)),
        39 => pen.fg = None,
        40..=47 => pen.bg = Some(slot(code - 40)),
        49 => pen.bg = None,
        90..=97 => pen.fg = Some(slot(code - 90 + 8)),
        100..=107 => pen.bg = Some(slot(code - 100 + 8)),
        38 | 48 | 58 => set_colour(pen, code, semicolon_colour(parameters)),
        _ => {}
    }
}

/// Slot `index` of the terminal's palette, below 16 for the named
/// colours.
fn slot(index: u16) -> Color {
    Color::Indexed(u8::try_from(index).expect("the named colours are the first 16 slots"))
}

/// Sets the foreground for `38`, the background for `48`, and nothing for
/// the underline colour `58`, whose arguments are read all the same.
fn set_colour(pen: &mut Pen, code: u16, colour: Option<Color>) {
    match (code, colour) {
        (38, Some(colour)) => pen.fg = Some(colour),
        (48, Some(colour)) => pen.bg = Some(colour),
        _ => {}
    }
}

/// The colour of `38;5;n` or `38;2;r;g;b`: the mode, then one or three
/// arguments, taken from `parameters`. Another mode takes no argument.
/// Arguments are consumed as the mode counts them even when the colour is
/// invalid, so they are not read as parameters of their own.
fn semicolon_colour(parameters: &mut ParamsIter<'_>) -> Option<Color> {
    let mode = parameters.next()?;
    let wanted = match mode.first() {
        Some(5) => 1,
        Some(2) => 3,
        _ => 0,
    };
    let arguments: Vec<&[u16]> = parameters.by_ref().take(wanted).collect();
    if mode.len() > 1
        || arguments.len() < wanted
        || arguments.iter().any(|argument| argument.len() > 1)
    {
        return None;
    }
    let values: Vec<u16> = arguments.iter().map(|argument| argument[0]).collect();
    match (mode[0], values.as_slice()) {
        (5, &[index]) => indexed(index),
        (2, &[red, green, blue]) => rgb(red, green, blue),
        _ => None,
    }
}

/// The colour of `38:5:n`, `38:2:r:g:b` or `38:2:cs:r:g:b` (with `cs`, a
/// colour space, ignored), from the subparameters after `38`.
fn colon_colour(arguments: &[u16]) -> Option<Color> {
    match *arguments {
        [5, index] => indexed(index),
        [2, _, red, green, blue] | [2, red, green, blue] => rgb(red, green, blue),
        _ => None,
    }
}

fn indexed(index: u16) -> Option<Color> {
    u8::try_from(index).ok().map(Color::Indexed)
}

fn rgb(red: u16, green: u16, blue: u16) -> Option<Color> {
    Some(Color::Rgb(
        u8::try_from(red).ok()?,
        u8::try_from(green).ok()?,
        u8::try_from(blue).ok()?,
    ))
}

// ---------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------

/// The styled lines with the notes of a cut file around them.
pub(super) struct Painted {
    rows: Vec<Row>,
}

enum Row {
    Line(SourceLine),
    Note(Note),
}

impl Painted {
    /// `lines`, the whole text or the start of a longer one that the limit
    /// `cut` ended.
    fn new(lines: Vec<SourceLine>, cut: Option<Cut>) -> Self {
        Painted {
            rows: between_cut_notes(cut, lines.into_iter().map(Row::Line), Row::Note).collect(),
        }
    }
}

impl Viewer for Painted {
    /// Lines never wrap, so only the notes depend on the width.
    fn lay_out(&mut self, width: u16, _height: u16) -> ContentSize {
        let columns = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Line(line) => line.width(),
                Row::Note(note) => note.columns(usize::from(width)),
            })
            .max()
            .unwrap_or(0);
        ContentSize {
            lines: self.rows.len(),
            columns,
        }
    }

    /// Draws the rows from `offset.top` on, each line from column
    /// `offset.left`.
    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        for (row_area, row) in rows_from(area, &self.rows, offset.top) {
            match row {
                Row::Line(line) => line.draw(buf, row_area, offset.left),
                Row::Note(note) => note.render(row_area, buf),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{drawn, row};
    use super::*;

    // -----------------------------------------------------------------
    // Reading the text
    // -----------------------------------------------------------------

    /// The pieces of each line `text` makes, with colours, each as its
    /// text and its style.
    fn painted(text: &str) -> Vec<Vec<(String, Style)>> {
        painted_with(text, true)
    }

    fn painted_with(text: &str, colours: bool) -> Vec<Vec<(String, Style)>> {
        styled_lines(text, colours)
            .iter()
            .map(|line| {
                line.pieces()
                    .map(|(text, style)| (text.to_owned(), style))
                    .collect()
            })
            .collect()
    }

    /// The text of each line `text` makes.
    fn texts(text: &str) -> Vec<String> {
        styled_lines(text, true)
            .iter()
            .map(|line| line.pieces().map(|(text, _)| text).collect())
            .collect()
    }

    /// An `x` after the SGR sequence of `parameters`.
    fn sgr(parameters: &str) -> String {
        format!("\x1b[{parameters}mx")
    }

    /// One line of one piece, `text` in `style`.
    fn one(text: &str, style: Style) -> Vec<Vec<(String, Style)>> {
        vec![vec![(text.to_owned(), style)]]
    }

    fn x_in(style: Style) -> Vec<Vec<(String, Style)>> {
        one("x", style)
    }

    /// Checks every case at once, so a run lists each case that fails
    /// with what it got, not only the first.
    fn assert_cases<T: PartialEq + Debug>(cases: Vec<(&str, T, T)>) {
        let failures: Vec<String> = cases
            .iter()
            .filter(|(_, got, expected)| got != expected)
            .map(|(name, got, expected)| format!("{name}: got {got:?}, expected {expected:?}"))
            .collect();
        assert!(
            failures.is_empty(),
            "{} of {} cases fail:\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }

    fn fg(index: u8) -> Style {
        Style::new().fg(Color::Indexed(index))
    }

    fn bg(index: u8) -> Style {
        Style::new().bg(Color::Indexed(index))
    }

    fn with(modifier: Modifier) -> Style {
        Style::new().add_modifier(modifier)
    }

    #[test]
    fn plain_text_shows_line_by_line_as_written() {
        assert_cases(vec![
            (
                "lines",
                texts("one\ntwo\r\n"),
                vec!["one".to_owned(), "two".to_owned()],
            ),
            (
                "a reset between the CR and the LF",
                texts("a\r\x1b[0m\n"),
                vec!["a".to_owned()],
            ),
            (
                "a CR in a style of its own",
                texts("a\x1b[31m\r\n"),
                vec!["a".to_owned()],
            ),
        ]);
    }

    #[test]
    fn named_colours_are_slots_of_the_terminals_palette() {
        assert_cases(vec![
            ("30", painted(&sgr("30")), x_in(fg(0))),
            ("31", painted(&sgr("31")), x_in(fg(1))),
            ("37", painted(&sgr("37")), x_in(fg(7))),
            ("90", painted(&sgr("90")), x_in(fg(8))),
            ("92", painted(&sgr("92")), x_in(fg(10))),
            ("97", painted(&sgr("97")), x_in(fg(15))),
            ("40", painted(&sgr("40")), x_in(bg(0))),
            ("44", painted(&sgr("44")), x_in(bg(4))),
            ("47", painted(&sgr("47")), x_in(bg(7))),
            ("100", painted(&sgr("100")), x_in(bg(8))),
            ("103", painted(&sgr("103")), x_in(bg(11))),
            ("107", painted(&sgr("107")), x_in(bg(15))),
        ]);
    }

    #[test]
    fn indexed_and_rgb_colours_in_both_notations() {
        let rgb = |style: Style| style;
        assert_cases(vec![
            ("38;5;208", painted(&sgr("38;5;208")), x_in(fg(208))),
            ("48;5;9", painted(&sgr("48;5;9")), x_in(bg(9))),
            (
                "48;2;1;2;3",
                painted(&sgr("48;2;1;2;3")),
                x_in(rgb(Style::new().bg(Color::Rgb(1, 2, 3)))),
            ),
            ("38:5:208", painted(&sgr("38:5:208")), x_in(fg(208))),
            (
                "38:2::1:2:3",
                painted(&sgr("38:2::1:2:3")),
                x_in(Style::new().fg(Color::Rgb(1, 2, 3))),
            ),
            (
                "38:2:7:1:2:3",
                painted(&sgr("38:2:7:1:2:3")),
                x_in(Style::new().fg(Color::Rgb(1, 2, 3))),
            ),
            (
                "38:2:1:2:3",
                painted(&sgr("38:2:1:2:3")),
                x_in(Style::new().fg(Color::Rgb(1, 2, 3))),
            ),
        ]);
    }

    #[test]
    fn attributes_switch_on_and_off() {
        assert_cases(vec![
            ("1", painted(&sgr("1")), x_in(with(Modifier::BOLD))),
            ("2", painted(&sgr("2")), x_in(with(Modifier::DIM))),
            ("3", painted(&sgr("3")), x_in(with(Modifier::ITALIC))),
            ("4", painted(&sgr("4")), x_in(with(Modifier::UNDERLINED))),
            ("7", painted(&sgr("7")), x_in(with(Modifier::REVERSED))),
            ("9", painted(&sgr("9")), x_in(with(Modifier::CROSSED_OUT))),
            ("21", painted(&sgr("21")), x_in(with(Modifier::UNDERLINED))),
            (
                "4:1",
                painted(&sgr("4:1")),
                x_in(with(Modifier::UNDERLINED)),
            ),
            (
                "4:3",
                painted(&sgr("4:3")),
                x_in(with(Modifier::UNDERLINED)),
            ),
            (
                "4:5",
                painted(&sgr("4:5")),
                x_in(with(Modifier::UNDERLINED)),
            ),
            ("1;2;22", painted(&sgr("1;2;22")), x_in(Style::new())),
            ("3;23", painted(&sgr("3;23")), x_in(Style::new())),
            ("4;24", painted(&sgr("4;24")), x_in(Style::new())),
            ("7;27", painted(&sgr("7;27")), x_in(Style::new())),
            ("9;29", painted(&sgr("9;29")), x_in(Style::new())),
            ("4;4:0", painted(&sgr("4;4:0")), x_in(Style::new())),
            (
                "4:6 is skipped",
                painted(&sgr("1;4:6")),
                x_in(with(Modifier::BOLD)),
            ),
        ]);
    }

    #[test]
    fn a_reset_and_the_defaults() {
        let red_on_blue = Style::new()
            .fg(Color::Indexed(1))
            .bg(Color::Indexed(4))
            .add_modifier(Modifier::BOLD);
        assert_cases(vec![
            ("0", painted("\x1b[31;44;1m\x1b[0mx"), x_in(Style::new())),
            ("empty", painted("\x1b[31;44;1m\x1b[mx"), x_in(Style::new())),
            (
                "39",
                painted("\x1b[31;44;1m\x1b[39mx"),
                x_in(Style {
                    fg: None,
                    ..red_on_blue
                }),
            ),
            (
                "49",
                painted("\x1b[31;44;1m\x1b[49mx"),
                x_in(Style {
                    bg: None,
                    ..red_on_blue
                }),
            ),
        ]);
    }

    #[test]
    fn a_style_holds_across_a_line_end() {
        assert_eq!(
            painted("\x1b[31ma\nb"),
            vec![vec![("a".to_owned(), fg(1))], vec![("b".to_owned(), fg(1))]]
        );
    }

    /// Text around `sequence`, which shows none of it.
    fn around(sequence: &str) -> Vec<Vec<(String, Style)>> {
        painted(&format!("a{sequence}b"))
    }

    #[test]
    fn cursor_movement_and_erasing_are_dropped() {
        let ab = one("ab", Style::new());
        assert_cases(vec![
            ("erase the screen", around("\x1b[2J"), ab.clone()),
            ("home", around("\x1b[H"), ab.clone()),
            ("move to", around("\x1b[5;10H"), ab.clone()),
            ("erase the line", around("\x1b[K"), ab.clone()),
            ("up", around("\x1b[3A"), ab.clone()),
            ("hide the cursor", around("\x1b[?25l"), ab.clone()),
            ("save the cursor", around("\x1b7"), ab.clone()),
            ("restore the cursor", around("\x1b8"), ab.clone()),
            ("a character set", around("\x1b(B"), ab.clone()),
            ("the cursor's shape", around("\x1b[1 q"), ab),
        ]);
    }

    #[test]
    fn strings_are_dropped_with_their_contents() {
        let ab = one("ab", Style::new());
        assert_cases(vec![
            (
                "a title ended by BEL",
                around("\x1b]0;title\x07"),
                ab.clone(),
            ),
            (
                "a title ended by ST",
                around("\x1b]0;title\x1b\\"),
                ab.clone(),
            ),
            ("the clipboard", around("\x1b]52;c;aGk=\x07"), ab.clone()),
            ("DCS", around("\x1bPqsixel\x1b\\"), ab.clone()),
            ("APC", around("\x1b_Gi=1;AAAA\x1b\\"), ab.clone()),
            ("SOS", around("\x1bXsos\x1b\\"), ab.clone()),
            ("PM", around("\x1b^pm\x1b\\"), ab.clone()),
            ("APC holding ✓", around("\x1b_✓ hidden\x1b\\"), ab.clone()),
            ("APC holding “", around("\x1b_a“b\x1b\\"), ab.clone()),
            ("DCS holding ✓", around("\x1bP✓ hidden\x1b\\"), ab.clone()),
            ("OSC holding é", around("\x1b]0;café\x07"), ab),
            (
                "a hyperlink",
                around("\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\"),
                one("alinkb", Style::new()),
            ),
        ]);
    }

    #[test]
    fn only_a_plain_sgr_sets_a_style() {
        assert_cases(vec![
            (
                "a private marker >",
                painted("\x1b[>4;2mx"),
                x_in(Style::new()),
            ),
            (
                "a private marker ?",
                painted("\x1b[?1mx"),
                x_in(Style::new()),
            ),
            ("an intermediate", painted("\x1b[1$mx"), x_in(Style::new())),
            (
                "32 numbers",
                painted(&sgr(&format!("{}1", "0;".repeat(31)))),
                x_in(with(Modifier::BOLD)),
            ),
            (
                "33 numbers",
                painted(&sgr(&format!("{}1", "0;".repeat(32)))),
                x_in(Style::new()),
            ),
            (
                "36 numbers in colons",
                painted(&sgr(&["38:2::1:2:3"; 6].join(";"))),
                x_in(Style::new()),
            ),
        ]);
    }

    #[test]
    fn unknown_sgr_parameters_are_skipped_with_their_arguments() {
        let bold = x_in(with(Modifier::BOLD));
        assert_cases(vec![
            ("blink", painted(&sgr("5")), x_in(Style::new())),
            ("hidden", painted(&sgr("8")), x_in(Style::new())),
            ("overline", painted(&sgr("53")), x_in(Style::new())),
            (
                "an RGB underline colour",
                painted(&sgr("1;58;2;255;0;0")),
                bold.clone(),
            ),
            (
                "an indexed underline colour",
                painted(&sgr("1;58;5;2")),
                bold.clone(),
            ),
            (
                "an underline colour in colons",
                painted(&sgr("1;58:2::1:2:3")),
                bold.clone(),
            ),
            (
                "the default underline colour",
                painted(&sgr("59;1")),
                bold.clone(),
            ),
            (
                "an unknown colour mode",
                painted(&sgr("38;7;1")),
                bold.clone(),
            ),
            ("1:2", painted(&sgr("1:2")), x_in(Style::new())),
            ("31:5", painted(&sgr("31:5")), x_in(Style::new())),
            ("0:1", painted(&sgr("1;0:1")), bold),
            (
                "a mode in colons",
                painted(&sgr("38;5:208")),
                x_in(Style::new()),
            ),
            (
                "an argument in colons",
                painted(&sgr("38;2;1:2;3;4")),
                x_in(Style::new()),
            ),
        ]);
    }

    #[test]
    fn a_colour_out_of_range_or_cut_short_is_ignored() {
        let plain = x_in(Style::new());
        assert_cases(vec![
            ("slot 256", painted(&sgr("38;5;256")), plain.clone()),
            ("red 300", painted(&sgr("38;2;300;0;0")), plain.clone()),
            ("no slot", painted(&sgr("38;5")), plain.clone()),
            ("only red", painted(&sgr("38;2;1")), plain.clone()),
            ("no blue", painted(&sgr("48;2;1;2")), plain.clone()),
            ("no slot in colons", painted(&sgr("38:5")), plain.clone()),
            (
                "no blue in colons",
                painted(&sgr("38:2:1:2")),
                plain.clone(),
            ),
            ("an unknown mode in colons", painted(&sgr("38:9:1")), plain),
        ]);
    }

    #[test]
    fn a_sequence_cut_short_at_the_end_is_dropped() {
        let ok = one("ok", Style::new());
        assert_cases(vec![
            ("a CSI", painted("ok\x1b[31"), ok.clone()),
            ("an escape", painted("ok\x1b"), ok),
        ]);
    }

    #[test]
    fn an_escape_ends_at_its_final_byte() {
        assert_cases(vec![
            (
                "the next character",
                texts("a\x1bxb"),
                vec!["ab".to_owned()],
            ),
            ("a line end inside", texts("a\x1b\nb"), vec!["a".to_owned()]),
            (
                "é inside an escape",
                texts("a\x1b\u{e9}b"),
                vec!["a".to_owned()],
            ),
            ("日 inside an escape", texts("\x1b日"), Vec::new()),
            ("日 inside a CSI", texts("\x1b[31日"), Vec::new()),
        ]);
        assert_eq!(painted("\x1b[31\u{e9}mx"), x_in(fg(1)), "é inside a CSI");
    }

    #[test]
    fn cancel_and_substitute_end_a_sequence_and_show() {
        assert_eq!(painted("\x1b[31\x18x"), one("␘x", Style::new()));
        assert_eq!(painted("\x1b[31\x1ax"), one("␚x", Style::new()));
    }

    #[test]
    fn a_line_end_inside_a_sequence_ends_the_line_and_the_sequence_goes_on() {
        assert_eq!(
            painted("\x1b[3\n1mx"),
            vec![Vec::new(), vec![("x".to_owned(), fg(1))]]
        );
    }

    #[test]
    fn an_unterminated_osc_hides_the_text_up_to_the_next_escape() {
        assert_eq!(
            painted("a\x1b]0;t\nb\x1b[1mc"),
            vec![vec![
                ("a".to_owned(), Style::new()),
                ("c".to_owned(), with(Modifier::BOLD))
            ]]
        );
    }

    #[test]
    fn a_tab_counts_the_columns_of_coloured_text_before_it() {
        assert_eq!(
            painted("\x1b[31mab\x1b[0m\tc"),
            vec![vec![
                ("ab".to_owned(), fg(1)),
                ("  c".to_owned(), Style::new())
            ]]
        );
    }

    #[test]
    fn control_characters_show_as_their_pictures() {
        assert_eq!(texts("a\x07b\rc\x08d\x7f"), ["a␇b␍c␈d␡"]);
    }

    #[test]
    fn a_c1_character_is_no_sequence() {
        assert_eq!(painted("\u{9b}31mx"), one("\u{fffd}31mx", Style::new()));
    }

    #[test]
    fn wide_characters_take_two_columns() {
        let lines = styled_lines("日\x1b[31m本", true);

        assert_eq!(lines[0].width(), 4);
        assert_eq!(
            painted("日\x1b[31m本"),
            vec![vec![
                ("日".to_owned(), Style::new()),
                ("本".to_owned(), fg(1))
            ]]
        );
    }

    #[test]
    fn a_grapheme_split_by_a_style_change_stays_whole() {
        assert_cases(vec![
            (
                "an accent",
                painted("e\x1b[31m\u{301}x"),
                vec![vec![
                    ("e\u{301}".to_owned(), Style::new()),
                    ("x".to_owned(), fg(1)),
                ]],
            ),
            (
                "a skin tone",
                painted("👍\x1b[31m🏽"),
                one("👍🏽", Style::new()),
            ),
            (
                "a joined emoji",
                painted("👨\x1b[31m\u{200d}👩"),
                one("👨\u{200d}👩", Style::new()),
            ),
        ]);
        assert_eq!(styled_lines("👍\x1b[31m🏽", true)[0].width(), 2);
    }

    #[test]
    fn pieces_open_only_where_a_style_changes_and_text_follows() {
        assert_eq!(
            painted("\x1b[1m\x1b[0ma\x1b[31mb\x1b[31mc\x1b[1m"),
            vec![vec![
                ("a".to_owned(), Style::new()),
                ("bc".to_owned(), fg(1))
            ]]
        );
    }

    #[test]
    fn a_last_line_with_nothing_but_sequences_is_no_line() {
        assert_cases(vec![
            (
                "a reset after the last line end",
                texts("a\n\x1b[0m"),
                vec!["a".to_owned()],
            ),
            ("only a reset", texts("\x1b[0m"), Vec::new()),
            ("nothing", texts(""), Vec::new()),
            ("a line end", texts("\n"), vec![String::new()]),
        ]);
    }

    #[test]
    fn without_colours_every_colour_goes_and_the_attributes_stay() {
        assert_eq!(
            painted_with("\x1b[1;7;38;2;1;2;3;48;5;2mx", false),
            x_in(with(Modifier::BOLD | Modifier::REVERSED))
        );
    }

    // -----------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------

    fn built(text: &str) -> Content<Painted> {
        let entry: AnsiEntry = serde_json::from_value(serde_json::json!({"content": text}))
            .expect("test entry parses");
        ansi_content(&entry, true, None)
    }

    #[test]
    fn lines_do_not_wrap_and_scroll_sideways_without_a_gutter() {
        let mut viewer = built("short\n\x1b[32mthe longest line\x1b[0m here\n");

        assert_eq!(
            viewer.lay_out(10, 5),
            ContentSize {
                lines: 2,
                columns: 21
            }
        );
        let buf = drawn(&mut viewer, 10, 2, Offset::default());
        assert_eq!(row(&buf, 0), "short", "from the first column");
        assert_eq!(row(&buf, 1), "the longe›");
        assert_eq!(buf[(0, 1)].fg, Color::Indexed(2));
        let buf = drawn(&mut viewer, 10, 2, Offset { top: 0, left: 4 });
        assert_eq!(row(&buf, 1), "‹ongest l›");
    }

    #[test]
    fn it_draws_from_the_offsets_line_on() {
        let mut viewer = built("one\ntwo\nthree\n");

        let buf = drawn(&mut viewer, 10, 2, Offset { top: 1, left: 0 });

        assert_eq!(row(&buf, 0), "two");
        assert_eq!(row(&buf, 1), "three");
    }

    #[test]
    fn a_file_is_read_with_its_escape_sequences() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("run.ansi");
        std::fs::write(&path, "\x1b[32mok\x1b[0m 3 passed\n").expect("file is writable");
        let entry = AnsiEntry {
            title: None,
            path: Some(path.to_str().expect("a UTF-8 path").to_owned()),
            content: None,
        };

        let mut viewer = ansi_content(&entry, true, None);

        let buf = drawn(&mut viewer, 20, 1, Offset::default());
        assert_eq!(row(&buf, 0), "ok 3 passed");
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(2));
    }

    #[test]
    fn a_file_too_long_to_read_whole_shows_its_cut_notes() {
        let source = Ok(Source {
            text: "one\n\x1b[31mtwo\n".into(),
            cut: Some(Cut::Bytes),
        });
        let mut viewer = content_of(source, true, None);

        let buf = drawn(&mut viewer, 40, 4, Offset::default());

        assert_eq!(row(&buf, 0), "first 8 MiB shown");
        assert_eq!(row(&buf, 1), "one");
        assert_eq!(row(&buf, 2), "two");
        assert_eq!(row(&buf, 3), "… the rest is cut after 8 MiB");
        assert!(buf[(0, 3)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_file_that_cannot_be_read_shows_why_as_the_content() {
        let entry = AnsiEntry {
            title: None,
            path: Some("/nowhere/run.ansi".into()),
            content: None,
        };
        let mut viewer = ansi_content(&entry, true, None);

        let buf = drawn(&mut viewer, 50, 8, Offset::default());

        assert!(row(&buf, 2).contains("│ ✗ No such file"), "{buf:?}");
        assert!(viewer.failed());
    }

    #[test]
    fn text_without_a_line_says_empty() {
        for text in ["", "\x1b[0m"] {
            let mut viewer = built(text);

            let buf = drawn(&mut viewer, 10, 1, Offset::default());

            assert_eq!(row(&buf, 0), "empty", "{text:?}");
            assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        }
    }
}
