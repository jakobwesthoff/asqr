// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One line of source text as the viewers show it (ADR 33): tabs expanded,
//! control characters made visible, measured in terminal columns, and drawn
//! through a window of columns, so a line longer than the pane scrolls
//! sideways instead of wrapping. Rendered Markdown, which never scrolls
//! sideways, breaks such a line into rows instead (ADR 34). Its measure of
//! columns, [`columns`], is the one the whole screen lays text out by.
//!
//! The rows the text viewers draw are made of such lines: a [`GutterRow`]
//! puts one right of a gutter that stays in place, a [`Note`] says
//! something about the content that is no line of it, and [`rows_from`]
//! walks the rows of a viewer from its offset.

use std::borrow::Cow;

use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use super::theme;

/// Columns from one tab stop to the next. Panes are often 40 to 60 columns
/// wide, where the terminal's 8 would push indented code out of sight.
pub(crate) const TAB_STOP: usize = 4;

/// What marks a line that goes on past the right edge.
const CUT_MARKER: &str = "›";

/// What marks a line scrolled past the start of its text.
const LEFT_CUT_MARKER: &str = "‹";

/// A line of text ready to draw: every grapheme in it takes the columns
/// its width says, so the width measured here is the width drawn.
pub(super) struct SourceLine {
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
    pub(super) fn new<'a>(pieces: impl IntoIterator<Item = (&'a str, Style)>) -> Self {
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
    pub(super) fn width(&self) -> usize {
        self.width
    }

    /// The runs of the line with their styles, tabs already expanded and
    /// control characters already pictured.
    pub(super) fn pieces(&self) -> impl Iterator<Item = (&str, Style)> {
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
    pub(super) fn wrap(&self, first: usize, rest: usize) -> Vec<SourceLine> {
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
    /// first row of `area`.
    ///
    /// When text that is not blank lies left of `left`, a dim `‹` takes the
    /// first cell; when the line goes on past the right edge, a dim `›`
    /// takes the last. Each marker covers the column it sits on, so the
    /// text runs between them, and a wide character that crosses into a
    /// marker's cell or past an edge shows as blanks in its style. A
    /// marker never shares its cell with the trailing half of a wide
    /// character, where the terminal would not show it. Blanks alone
    /// left of `left`, such as an indent, get no marker, as nothing of
    /// the line is hidden.
    pub(super) fn draw(&self, buf: &mut Buffer, area: Rect, left: usize) {
        let room = usize::from(area.width);
        if room == 0 {
            return;
        }
        let right = left + room;
        let hidden_left = left > 0 && self.has_text_before(left);
        let hidden_right = self.width > right;
        // The columns the text itself may take, between the markers.
        let first = left + usize::from(hidden_left);
        let end = right - usize::from(hidden_right);
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
                if width == 0 || column <= first {
                    continue;
                }
                if start >= end {
                    break 'pieces;
                }
                if start < first || column > end {
                    for cut in start.max(first)..column.min(end) {
                        buf[(x_of(cut), area.y)]
                            .set_symbol(" ")
                            .set_style(piece.style);
                    }
                } else {
                    buf.set_stringn(x_of(start), area.y, grapheme, width, piece.style);
                }
            }
        }

        // In a window of a single cell both markers want it; the right one
        // is drawn last and takes it.
        for (shown, column, marker) in [
            (hidden_left, left, LEFT_CUT_MARKER),
            (hidden_right, right - 1, CUT_MARKER),
        ] {
            if shown {
                let cell = &mut buf[(x_of(column), area.y)];
                cell.reset();
                cell.set_symbol(marker).set_style(theme::secondary());
            }
        }
    }

    /// Whether any grapheme other than a blank starts before column
    /// `column`, wholly or partly hidden by a window that starts there.
    fn has_text_before(&self, column: usize) -> bool {
        let mut start = 0;
        for piece in &self.pieces {
            for grapheme in piece.text.graphemes(true) {
                if start >= column {
                    return false;
                }
                if grapheme != " " && grapheme_columns(grapheme) > 0 {
                    return true;
                }
                start += grapheme_columns(grapheme);
            }
        }
        false
    }
}

// ---------------------------------------------------------------------
// The rows of a viewer
// ---------------------------------------------------------------------

/// The rows of `area` from the top, each as an area one row high, paired
/// with the viewer's `rows` from row `top` on, as far as both go.
pub(super) fn rows_from<T>(area: Rect, rows: &[T], top: usize) -> impl Iterator<Item = (Rect, &T)> {
    area.rows().zip(rows.iter().skip(top))
}

/// A row that keeps a gutter, such as a line's number, at its left edge.
/// The gutter stays in place while the line right of it scrolls sideways
/// under `left`, so every line keeps its gutter in sight. A row narrower
/// than the gutter shows only what fits of the gutter.
pub(super) struct GutterRow<'a> {
    /// The gutter in its own styles, as wide as the columns it keeps.
    pub gutter: Line<'a>,
    pub line: &'a SourceLine,
    pub left: usize,
}

impl Widget for GutterRow<'_> {
    /// Draws into the first row of `area`.
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_line(area.x, area.y, &self.gutter, area.width);
        let fixed = u16::try_from(self.gutter.width()).unwrap_or(u16::MAX);
        let text = Rect {
            x: area.x.saturating_add(fixed).min(area.right()),
            width: area.width.saturating_sub(fixed),
            height: 1,
            ..area
        };
        self.line.draw(buf, text, self.left);
    }
}

/// A dim row about the content that is no line of it, such as "new file"
/// or the note of a file cut at 8 MiB. It never scrolls sideways: a body
/// narrower than the note cuts it at the edge, without a marker, so the
/// note counts no wider than the body. Most notes are fixed text; one built
/// for the content, such as the line ends of a diff's two sides, owns its
/// text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Note(Cow<'static, str>);

impl Note {
    /// A note of fixed text, usable in a `const`.
    pub(super) const fn new(text: &'static str) -> Note {
        Note(Cow::Borrowed(text))
    }

    pub(super) fn text(&self) -> &str {
        &self.0
    }

    /// The columns the note takes in a body `width` columns wide.
    pub(super) fn columns(&self, width: usize) -> usize {
        columns(self.text()).min(width)
    }
}

impl From<String> for Note {
    fn from(text: String) -> Note {
        Note(Cow::Owned(text))
    }
}

impl Widget for &Note {
    /// Draws into the first row of `area`.
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_stringn(
            area.x,
            area.y,
            self.text(),
            usize::from(area.width),
            theme::secondary(),
        );
    }
}

/// How many columns `text` takes in a span ratatui draws. ratatui lays a
/// span out grapheme by grapheme, each as wide as its cell width, so this
/// sums those: a CJK character or most emoji take two columns, a
/// combining accent none, and a grapheme with a control character none
/// either, as ratatui leaves it out. Every width the screen lays out is
/// measured here, so what it measures is what ratatui draws.
pub(crate) fn columns(text: &str) -> usize {
    text.graphemes(true).map(grapheme_columns).sum()
}

/// The longest start of `text` that fits into `room` columns, measured as
/// [`columns`] does. A fit never splits a grapheme: a wide character that
/// would cross the end stays out whole, and a combining accent stays with
/// its letter.
pub(super) fn fitting(text: &str, room: usize) -> &str {
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
pub(crate) fn picture(control: char) -> char {
    match u32::from(control) {
        code @ 0..0x20 => char::from_u32(0x2400 + code).expect("the Control Pictures block"),
        0x7f => '\u{2421}',
        _ => char::REPLACEMENT_CHARACTER,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier, Stylize};
    use ratatui::text::Span;

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

        assert_eq!(drawn(&line, 6, 4).0, "‹56789", "the end shows whole");
    }

    #[test]
    fn a_line_scrolled_past_its_start_begins_with_a_dim_marker() {
        let line = plain("0123456789");

        let (row, buf) = drawn(&line, 6, 2);
        assert_eq!(row, "‹3456›", "the marker hides the column at the offset");
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));

        assert_eq!(drawn(&line, 6, 7).0, "‹89   ");
        assert_eq!(
            drawn(&line, 6, 12).0,
            "‹     ",
            "a line scrolled out of sight still has text to the left"
        );
        assert_eq!(
            drawn(&plain(""), 6, 2).0,
            "      ",
            "an empty line has none"
        );
    }

    #[test]
    fn blanks_alone_left_of_the_offset_get_no_marker() {
        let indented = SourceLine::new([("    ", Style::new()), ("x = 1", Style::new().red())]);

        assert_eq!(drawn(&indented, 6, 2).0, "  x =›");
        assert_eq!(drawn(&indented, 6, 4).0, "x = 1 ");
        assert_eq!(drawn(&indented, 6, 5).0, "‹= 1  ", "the x is hidden");
    }

    #[test]
    fn a_wide_character_cut_by_an_edge_becomes_blanks_in_its_style() {
        let line = SourceLine::new([("a日本b", Style::new().red())]);

        let (row, buf) = drawn(&line, 5, 1);
        assert_eq!(
            row, "‹ 本b",
            "the left marker hides 日's left half, its right half is a blank"
        );
        assert_eq!(buf[(1, 0)].fg, Color::Red);

        let (row, buf) = drawn(&line, 3, 0);
        assert_eq!(row, "a ›", "the marker's column cuts 日");
        assert_eq!(buf[(1, 0)].fg, Color::Red);
        let (row, _) = drawn(&line, 2, 0);
        assert_eq!(row, "a›");
        assert_eq!(drawn(&plain("日本語"), 4, 0).0, "日 ›");

        let tight = SourceLine::new([("日本", Style::new())]);
        let (row, _) = drawn(&tight, 3, 1);
        assert_eq!(row, "‹本", "the marker takes the cut half of 日");
    }

    #[test]
    fn a_single_column_shows_the_right_marker_over_the_left_one() {
        assert_eq!(drawn(&plain("0123"), 1, 1).0, "›");
        assert_eq!(drawn(&plain("0123"), 1, 3).0, "‹");
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

    // -----------------------------------------------------------------
    // The rows of a viewer
    // -----------------------------------------------------------------

    #[test]
    fn rows_pair_the_areas_rows_with_the_viewers_from_the_offset() {
        let area = Rect::new(2, 5, 8, 3);

        let walked: Vec<(Rect, &&str)> = rows_from(area, &["a", "b", "c", "d"], 2).collect();
        assert_eq!(
            walked,
            [(Rect::new(2, 5, 8, 1), &"c"), (Rect::new(2, 6, 8, 1), &"d")],
            "as far as the rows go"
        );

        assert_eq!(
            rows_from(area, &["a", "b", "c", "d"], 0).count(),
            3,
            "as far as the area goes"
        );
        assert_eq!(rows_from(area, &["a"], 4).count(), 0, "past the end");
    }

    /// The row of `width` cells a `GutterRow` of `gutter` and `line` draws
    /// from column `left` on.
    fn gutter_row(gutter: Line<'_>, line: &SourceLine, width: u16, left: usize) -> Buffer {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        GutterRow { gutter, line, left }.render(area, &mut buf);
        buf
    }

    /// The symbols of the first row of `buf`.
    fn symbols(buf: &Buffer) -> String {
        (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn a_gutter_row_keeps_the_gutter_in_place_and_scrolls_the_line() {
        let gutter = || Line::from(vec![Span::from("12 ").dim(), Span::from("+").green()]);
        let line = plain("0123456789");

        let buf = gutter_row(gutter(), &line, 10, 0);
        assert_eq!(symbols(&buf), "12 +01234›");
        assert!(
            buf[(1, 0)].modifier.contains(Modifier::DIM),
            "the gutter's style"
        );
        assert_eq!(buf[(3, 0)].fg, Color::Green);
        assert!(!buf[(3, 0)].modifier.contains(Modifier::DIM));

        assert_eq!(symbols(&gutter_row(gutter(), &line, 10, 4)), "12 +‹56789");
    }

    #[test]
    fn a_gutter_row_narrower_than_its_gutter_shows_what_fits_of_it() {
        let line = plain("text");

        assert_eq!(symbols(&gutter_row(Line::from("12 "), &line, 2, 0)), "12");
        assert_eq!(
            symbols(&gutter_row(Line::from("12 "), &line, 3, 0)),
            "12 ",
            "no room is left for the line"
        );
    }

    #[test]
    fn a_note_is_dim_and_cut_at_the_edge_without_a_marker() {
        let note = Note::new("first 8 MiB shown");
        let area = Rect::new(0, 0, 8, 1);
        let mut buf = Buffer::empty(area);

        note.render(area, &mut buf);

        assert_eq!(symbols(&buf), "first 8 ");
        assert!(buf[(7, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(note.columns(8), 8, "no wider than the body");
        assert_eq!(note.columns(40), 17);
    }
}
