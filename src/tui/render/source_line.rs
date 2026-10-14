// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One line of source text as the viewers show it (ADR 33): tabs expanded,
//! control characters made visible, measured in terminal columns, and drawn
//! through a window of columns, so a line longer than the pane scrolls
//! sideways instead of wrapping. Rendered Markdown, which never scrolls
//! sideways, breaks such a line into rows instead (ADR 34). Its measure of
//! columns, [`columns`], is the one the whole screen lays text out by.

use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;

/// Columns from one tab stop to the next. Panes are often 40 to 60 columns
/// wide, where the terminal's 8 would push indented code out of sight.
pub const TAB_STOP: usize = 4;

/// What marks a line that goes on past the right edge.
const CUT_MARKER: &str = "›";

/// A line of text ready to draw: every grapheme in it takes the columns
/// its width says, so the width measured here is the width drawn.
pub struct SourceLine {
    pieces: Vec<Piece>,
    width: usize,
}

/// A run of the line in one style, such as a changed word in a diff.
struct Piece {
    text: String,
    style: Style,
}

impl SourceLine {
    /// The line made of `pieces` of text, each with its style, without
    /// the line break.
    ///
    /// A tab becomes the blanks up to the next tab stop, counted from the
    /// start of the line. Any other control character becomes its picture
    /// from the Control Pictures block, such as `␛` for escape, or `�` for
    /// the C1 controls, which have none. ratatui leaves control characters
    /// out of the cells it draws, so the columns would no longer add up,
    /// and an escape sent to the terminal could change what it shows.
    pub fn new<'a>(pieces: impl IntoIterator<Item = (&'a str, Style)>) -> Self {
        let mut column = 0;
        let pieces = pieces
            .into_iter()
            .map(|(text, style)| {
                let mut shown = String::with_capacity(text.len());
                for grapheme in text.graphemes(true) {
                    if grapheme == "\t" {
                        let blanks = TAB_STOP - column % TAB_STOP;
                        shown.extend(std::iter::repeat_n(' ', blanks));
                        column += blanks;
                    } else if grapheme.contains(char::is_control) {
                        // Control characters are graphemes of their own,
                        // `\r\n` aside, so the whole grapheme is controls.
                        for control in grapheme.chars() {
                            shown.push(picture(control));
                            column += 1;
                        }
                    } else {
                        shown.push_str(grapheme);
                        column += grapheme_columns(grapheme);
                    }
                }
                Piece { text: shown, style }
            })
            .collect();
        SourceLine {
            pieces,
            width: column,
        }
    }

    /// How many columns the line takes.
    pub fn width(&self) -> usize {
        self.width
    }

    /// The runs of the line with their styles, tabs already expanded and
    /// control characters already pictured.
    pub fn pieces(&self) -> impl Iterator<Item = (&str, Style)> {
        self.pieces
            .iter()
            .map(|piece| (piece.text.as_str(), piece.style))
    }

    /// The line broken into rows, the first at most `first` columns wide
    /// and every further one at most `rest`, for text that wraps instead
    /// of scrolling sideways. Graphemes stay whole: a wide character that
    /// would cross the end of a row starts the next one, and the cell it
    /// leaves shows a blank in its style, as at the edge of a cut line. A
    /// row always takes at least one grapheme, so a character wider than
    /// the room still makes progress.
    pub fn wrap(&self, first: usize, rest: usize) -> Vec<SourceLine> {
        let mut rows = Vec::new();
        let mut row = SourceLine {
            pieces: Vec::new(),
            width: 0,
        };
        let mut room = first;
        for piece in &self.pieces {
            for grapheme in piece.text.graphemes(true) {
                let width = grapheme_columns(grapheme);
                if row.width > 0 && row.width + width > room {
                    let blanks = room.saturating_sub(row.width);
                    row.push(&" ".repeat(blanks), piece.style);
                    rows.push(std::mem::replace(
                        &mut row,
                        SourceLine {
                            pieces: Vec::new(),
                            width: 0,
                        },
                    ));
                    room = rest;
                }
                row.push(grapheme, piece.style);
            }
        }
        rows.push(row);
        rows
    }

    /// Appends `text`, which holds no tab or control character, in
    /// `style`, extending the last piece when it has the same style.
    fn push(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        self.width += columns(text);
        match self.pieces.last_mut() {
            Some(last) if last.style == style => last.text.push_str(text),
            _ => self.pieces.push(Piece {
                text: text.to_owned(),
                style,
            }),
        }
    }

    /// Draws the columns `left..left + area.width` of the line into the
    /// first row of `area`. A wide character that an edge cuts in two
    /// shows as blanks in its style. When the line goes on past the right
    /// edge, a dim `›` in the last cell says so.
    pub fn draw(&self, buf: &mut Buffer, area: Rect, left: usize) {
        let room = usize::from(area.width);
        if room == 0 {
            return;
        }
        let right = left + room;
        // The cell of column `column`, which lies inside the window.
        let x_of = |column: usize| {
            area.x + u16::try_from(column - left).expect("inside the window, as wide as the area")
        };

        let mut column = 0;
        'pieces: for piece in &self.pieces {
            for grapheme in piece.text.graphemes(true) {
                let width = grapheme_columns(grapheme);
                let start = column;
                column += width;
                if width == 0 || column <= left {
                    continue;
                }
                if start >= right {
                    break 'pieces;
                }
                if start < left || column > right {
                    for cut in start.max(left)..column.min(right) {
                        buf[(x_of(cut), area.y)]
                            .set_symbol(" ")
                            .set_style(piece.style);
                    }
                } else {
                    buf.set_stringn(x_of(start), area.y, grapheme, width, piece.style);
                }
            }
        }

        if self.width > right {
            let last = &mut buf[(x_of(right - 1), area.y)];
            last.reset();
            last.set_symbol(CUT_MARKER).set_style(Style::new().dim());
        }
    }
}

/// How many columns `text` takes in a span ratatui draws. ratatui lays a
/// span out grapheme by grapheme, each as wide as its cell width, so this
/// sums those: a CJK character or most emoji take two columns, a
/// combining accent none, and a grapheme with a control character none
/// either, as ratatui leaves it out. Every width the screen lays out is
/// measured here, so what it measures is what ratatui draws.
pub fn columns(text: &str) -> usize {
    text.graphemes(true).map(grapheme_columns).sum()
}

/// The longest start of `text` that fits into `room` columns, measured as
/// [`columns`] does. A fit never splits a grapheme: a wide character that
/// would cross the end stays out whole, and a combining accent stays with
/// its letter.
pub fn fitting(text: &str, room: usize) -> &str {
    let mut used = 0;
    for (start, grapheme) in text.grapheme_indices(true) {
        used += grapheme_columns(grapheme);
        if used > room {
            return &text[..start];
        }
    }
    text
}

/// The columns ratatui gives one grapheme. Its cell width is the grapheme's
/// Unicode width, except that a halfwidth sound mark such as `ﾞ` takes a
/// cell of its own, as terminals draw it.
fn grapheme_columns(grapheme: &str) -> usize {
    if grapheme.contains(char::is_control) {
        0
    } else {
        usize::from(grapheme.cell_width())
    }
}

/// How a control character shows: U+2400 to U+241F picture the C0 controls
/// in order, and U+2421 pictures delete.
fn picture(control: char) -> char {
    match u32::from(control) {
        code @ 0..0x20 => char::from_u32(0x2400 + code).expect("the Control Pictures block"),
        0x7f => '\u{2421}',
        _ => char::REPLACEMENT_CHARACTER,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::*;

    fn plain(text: &str) -> SourceLine {
        SourceLine::new([(text, Style::new())])
    }

    /// The text of the line, pieces joined.
    fn text(line: &SourceLine) -> String {
        line.pieces
            .iter()
            .map(|piece| piece.text.as_str())
            .collect()
    }

    /// The row `line` draws into `width` cells from column `left`, as the
    /// terminal shows it: a wide character hides the cell after it.
    fn drawn(line: &SourceLine, width: u16, left: usize) -> (String, Buffer) {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        line.draw(&mut buf, area, left);
        let mut row = String::new();
        let mut x = 0;
        while x < width {
            let symbol = buf[(x, 0)].symbol();
            row.push_str(symbol);
            x += u16::try_from(columns(symbol).max(1)).expect("a cell is narrow");
        }
        (row, buf)
    }

    #[test]
    fn tabs_expand_to_the_next_stop_of_four_columns() {
        assert_eq!(text(&plain("\tx")), "    x");
        assert_eq!(text(&plain("ab\tc")), "ab  c");
        assert_eq!(text(&plain("abcd\te")), "abcd    e", "a full stop goes on");
        assert_eq!(
            text(&plain("日\tx")),
            "日  x",
            "a wide character counts twice"
        );
        assert_eq!(plain("ab\tc").width(), 5);
    }

    #[test]
    fn a_tab_counts_the_columns_of_the_pieces_before_it() {
        let line = SourceLine::new([("ab", Style::new()), ("\tc", Style::new().red())]);

        assert_eq!(text(&line), "ab  c");
        assert_eq!(line.pieces[1].text, "  c", "the tab stays in its piece");
    }

    #[test]
    fn control_characters_show_as_their_pictures() {
        assert_eq!(text(&plain("\u{1b}[31m")), "␛[31m");
        assert_eq!(text(&plain("a\0b\rc\u{7f}")), "a␀b␍c␡");
        assert_eq!(
            text(&plain("\u{85}")),
            "\u{fffd}",
            "C1 controls have no picture"
        );
        assert_eq!(plain("\u{1b}[31m").width(), 5, "one column each");
    }

    #[test]
    fn the_width_is_the_columns_on_screen() {
        assert_eq!(plain("").width(), 0);
        assert_eq!(plain("fetch").width(), 5);
        assert_eq!(plain("日本語").width(), 6);
        assert_eq!(
            plain("ｶﾞ").width(),
            2,
            "as ratatui draws a halfwidth sound mark"
        );
        assert_eq!(
            plain("e\u{301}").width(),
            1,
            "a combining accent takes no column"
        );
    }

    #[test]
    fn a_line_that_fits_draws_whole_with_its_styles() {
        let line = SourceLine::new([
            ("let ", Style::new().green()),
            ("x", Style::new().green().reversed()),
        ]);

        let (row, buf) = drawn(&line, 8, 0);

        assert_eq!(row, "let x   ");
        assert_eq!(buf[(0, 0)].fg, Color::Green);
        assert!(!buf[(3, 0)].modifier.contains(Modifier::REVERSED));
        assert!(buf[(4, 0)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buf[(5, 0)].fg, Color::Reset, "nothing past the end");
    }

    #[test]
    fn a_line_cut_at_the_right_edge_ends_in_a_dim_marker() {
        let line = plain("0123456789");

        let (row, buf) = drawn(&line, 6, 0);
        assert_eq!(row, "01234›");
        assert!(buf[(5, 0)].modifier.contains(Modifier::DIM));

        assert_eq!(drawn(&line, 6, 2).0, "23456›", "scrolled on");
        assert_eq!(drawn(&line, 6, 4).0, "456789", "the end shows whole");
        assert_eq!(drawn(&line, 6, 7).0, "789   ");
        assert_eq!(drawn(&line, 6, 12).0, "      ", "past the end");
    }

    #[test]
    fn a_wide_character_cut_by_an_edge_becomes_blanks_in_its_style() {
        let line = SourceLine::new([("a日本b", Style::new().red())]);

        let (row, buf) = drawn(&line, 4, 2);
        assert_eq!(row, " 本b", "the right half of 日 is a blank");
        assert_eq!(buf[(0, 0)].fg, Color::Red);

        let (row, _) = drawn(&line, 3, 0);
        assert_eq!(row, "a日", "日 ends at the edge");
        let (row, _) = drawn(&line, 2, 0);
        assert_eq!(row, "a›", "the marker takes the cut half");

        let tight = SourceLine::new([("日本", Style::new())]);
        let (row, _) = drawn(&tight, 3, 1);
        assert_eq!(row, " 本", "cut on the left, whole on the right");
    }

    /// The text of each row `line` wraps into.
    fn wrapped(line: &SourceLine, first: usize, rest: usize) -> Vec<String> {
        line.wrap(first, rest).iter().map(text).collect()
    }

    #[test]
    fn wrapping_breaks_a_line_into_rows_of_the_rooms_given() {
        let line = plain("0123456789");

        assert_eq!(wrapped(&line, 4, 3), ["0123", "456", "789"]);
        assert_eq!(wrapped(&line, 10, 3), ["0123456789"], "it fits");
        assert_eq!(wrapped(&plain(""), 4, 3), [""], "an empty line is one row");
        let rows = line.wrap(4, 3);
        assert_eq!(
            rows.iter().map(SourceLine::width).collect::<Vec<_>>(),
            [4, 3, 3]
        );
    }

    #[test]
    fn wrapping_keeps_the_styles_and_the_expanded_tabs() {
        let line = SourceLine::new([("\tab", Style::new()), ("cdef", Style::new().red())]);

        let rows = line.wrap(6, 6);

        assert_eq!(
            rows.iter().map(text).collect::<Vec<_>>(),
            ["    ab", "cdef"]
        );
        assert_eq!(
            rows[1].pieces().collect::<Vec<_>>(),
            [("cdef", Style::new().red())]
        );
    }

    #[test]
    fn a_wide_character_at_the_break_moves_whole_and_leaves_a_blank_in_its_style() {
        let line = SourceLine::new([("ab", Style::new()), ("日本", Style::new().green())]);

        let rows = line.wrap(3, 3);

        assert_eq!(
            rows.iter().map(text).collect::<Vec<_>>(),
            ["ab ", "日 ", "本"]
        );
        assert_eq!(
            rows[0].pieces().collect::<Vec<_>>(),
            [("ab", Style::new()), (" ", Style::new().green())]
        );
        assert_eq!(rows[0].width(), 3);
        assert_eq!(
            wrapped(&plain("日本"), 1, 1),
            ["日", "本"],
            "a character wider than the room still goes on"
        );
    }

    #[test]
    fn columns_count_what_each_grapheme_takes_on_screen() {
        assert_eq!(columns(""), 0);
        assert_eq!(columns("fetch"), 5);
        assert_eq!(columns("日本語"), 6);
        assert_eq!(columns("🎉ok"), 4);
        assert_eq!(columns("e\u{301}"), 1, "a combining accent takes no column");
        assert_eq!(
            columns("👍🏽"),
            2,
            "an emoji with a skin tone is one grapheme of two columns"
        );
        assert_eq!(columns("a\u{1b}b"), 2, "ratatui draws no control character");
        assert_eq!(
            columns("ｶﾞ"),
            2,
            "terminals give a halfwidth sound mark a cell of its own, and so does ratatui"
        );
    }

    #[test]
    fn fitting_keeps_whole_graphemes_within_the_room() {
        assert_eq!(fitting("fetch", 3), "fet");
        assert_eq!(fitting("fetch", 9), "fetch", "it fits");
        assert_eq!(fitting("日本語", 4), "日本", "本 ends at the edge");
        assert_eq!(fitting("日本語", 5), "日本", "語 would cross it");
        assert_eq!(fitting("a🎉", 2), "a");
        assert_eq!(fitting("e\u{301}x", 1), "e\u{301}", "the accent stays");
        assert_eq!(fitting("日", 0), "");
    }

    #[test]
    fn a_row_without_room_stays_empty() {
        let area = Rect::new(0, 0, 4, 1);
        let mut buf = Buffer::empty(area);

        plain("text").draw(&mut buf, Rect::new(0, 0, 0, 1), 0);

        assert_eq!(buf, Buffer::empty(area));
    }

    #[test]
    fn it_draws_at_the_areas_position() {
        let area = Rect::new(0, 0, 8, 2);
        let mut buf = Buffer::empty(area);

        plain("abc").draw(&mut buf, Rect::new(3, 1, 4, 1), 0);

        assert_eq!(buf[(3, 1)].symbol(), "a");
        assert_eq!(buf[(5, 1)].symbol(), "c");
        assert_eq!(buf[(3, 0)].symbol(), " ");
    }
}
