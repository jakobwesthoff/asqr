// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewer of diff entries (spec section 8.5, ADRs 33 and 60): the
//! change from the old to the new text as a unified diff, which asqr
//! computes itself. Changed lines show in a red or green fill, unchanged
//! lines highlighted in the diff's language.

use std::borrow::Cow;
use std::path::Path;
use std::time::{Duration, Instant};

use patine::StyledLine;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use similar::{Algorithm, Change, ChangeTag, DiffOp, DiffTag, TextDiff};

use super::code::{HIGHLIGHT_LIMIT, kind_of_file};
use super::patine_adapter::source_line;
use super::source_file::{Cut, LineBreaks, Source, Unreadable, between_cut_notes, read_source};
use super::source_line::{GutterRow, Note, SourceLine, rows_from};
use super::theme;
use super::viewer::{Content, ContentSize, DiffShape, Viewer};
use crate::format::{DiffEntry, DiffSide};
use crate::graphics::PlacedImage;
use crate::tui::{DiffLayout, Offset};

/// Unchanged lines shown before and after each change, as `diff -u` does.
const CONTEXT_LINES: usize = 3;

/// How long computing a diff may take. The viewer is built while the
/// person waits for the screen, so past this, similar settles for a diff that is correct
/// but larger than it needs to be.
const DIFF_TIME: Duration = Duration::from_secs(1);

/// What fills a separator line between two hunks.
const SEPARATOR: &str = "┈";

/// The viewer of `entry` as a unified diff: hunks with three lines of
/// context, both line numbers in a dim gutter, removed lines in a red fill
/// and added ones in a green fill, unchanged lines highlighted in the
/// entry's [`language`], or the card of the first side that cannot be
/// read. Reads both sides, computes the whole diff and highlights it, so
/// drawing only copies cells. The home folder `home` shows as `~` in the
/// path of a side that cannot be read.
pub(super) fn diff_content(entry: &DiffEntry, home: Option<&Path>) -> Content<Rows> {
    let sides =
        read("old", entry.old.as_ref()).and_then(|old| Ok((old, read("new", entry.new.as_ref())?)));
    match sides {
        Ok((old, new)) => {
            let cuts = [&old, &new]
                .into_iter()
                .flatten()
                .filter_map(|side| side.cut);
            Content::Body(Rows::of(
                text_of(old.as_ref()),
                text_of(new.as_ref()),
                cuts,
                language(entry).as_deref(),
            ))
        }
        Err((side, unreadable)) => Content::Problem(unreadable.card_on_side(side, home)),
    }
}

/// The language to highlight the unchanged lines of `entry` in: its
/// `language`, else what the file name `name` says, as for a code entry.
/// The `title` and the paths of the sides do not decide.
fn language(entry: &DiffEntry) -> Option<String> {
    entry
        .language
        .clone()
        .or_else(|| entry.name.as_deref().and_then(kind_of_file))
}

// ---------------------------------------------------------------------
// The diff as a file
// ---------------------------------------------------------------------

/// `entry` as the text of a unified diff, for the app `o` opens it in, with
/// `label` as the file's name in its header. The sides are read and
/// compared as the viewer does, so the hunks are the ones the pane shows,
/// and the notes the pane shows about cut sides and line ends come first,
/// as lines `patch` and `git apply` pass over before the header. The
/// error says which side cannot be read and why.
pub(crate) fn unified_text(entry: &DiffEntry, label: &str) -> Result<String, String> {
    let (old, new) = read("old", entry.old.as_ref())
        .and_then(|old| Ok((old, read("new", entry.new.as_ref())?)))
        .map_err(|(side, unreadable)| format!("{side} side: {}", unreadable.reason))?;
    let cuts = [&old, &new]
        .into_iter()
        .flatten()
        .filter_map(|side| side.cut);
    let mut text: String = between_cut_notes(cuts, [], |note| note)
        .map(|note| format!("{}\n", note.text()))
        .collect();
    let (old, new) = (text_of(old.as_ref()), text_of(new.as_ref()));
    if let (Some(old), Some(new)) = (old, new) {
        let (old, new) = (LineEnds::of(old), LineEnds::of(new));
        if old != new && old != LineEnds::NONE && new != LineEnds::NONE {
            text.push_str(endings_note(old, new).text());
            text.push('\n');
        }
    }
    let side = |given: bool, prefix: &str| {
        if given {
            format!("{prefix}/{label}")
        } else {
            "/dev/null".to_owned()
        }
    };
    text.push_str(&format!(
        "--- {}\n+++ {}\n",
        side(old.is_some(), "a"),
        side(new.is_some(), "b")
    ));
    let (old, new) = (
        unix_line_ends(old.unwrap_or_default()),
        unix_line_ends(new.unwrap_or_default()),
    );
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Histogram)
        .deadline(Instant::now() + DIFF_TIME)
        .diff_lines(old.as_ref(), new.as_ref());
    let mut unified = diff.unified_diff();
    unified
        .context_radius(CONTEXT_LINES)
        .missing_newline_hint(true);
    for hunk in unified.iter_hunks() {
        text.push_str(&hunk.to_string());
    }
    Ok(text)
}

// ---------------------------------------------------------------------
// Reading the sides
// ---------------------------------------------------------------------

/// The text of a side, or `None` for a side the entry leaves out. A side
/// that cannot be read comes with `name`, `old` or `new`, so the card says
/// which one failed.
fn read(
    name: &'static str,
    side: Option<&DiffSide>,
) -> Result<Option<Source>, (&'static str, Unreadable)> {
    side.map(|side| {
        read_source(
            side.path.as_deref(),
            side.content.as_deref(),
            LineBreaks::Any,
        )
    })
    .transpose()
    .map_err(|unreadable| (name, unreadable))
}

fn text_of(side: Option<&Source>) -> Option<&str> {
    side.map(|side| side.text.as_str())
}

// ---------------------------------------------------------------------
// The diff's rows
// ---------------------------------------------------------------------

/// The note under the last line of a side that ends without a newline.
const NO_NEWLINE: Note = Note::new("no newline at end of file");

/// The rows of a diff, unified and side by side, and how many digits its
/// line numbers take. Both layouts are made when the viewer is built, so
/// a switch between them only lays the viewer out again.
pub(super) struct Rows {
    /// The rows of the unified diff.
    rows: Vec<Row>,
    /// The lines the rows show, each once, so the unified rows and the
    /// pairs of side by side share them.
    lines: Vec<LineRow>,
    digits: usize,
    /// The rows side by side.
    pairs: Vec<Pair>,
    /// For each unified row, the pair that shows it.
    pair_of_row: Vec<usize>,
    /// For each pair, the first unified row it shows.
    first_row_of_pair: Vec<usize>,
    /// Whether the last lay-out was side by side.
    side_by_side: bool,
    /// The width of the body of the last lay-out.
    width: u16,
}

enum Row {
    /// A line of either side, or of both for an unchanged one, by its
    /// index into the lines.
    Line(usize),
    /// The gap between two hunks.
    Separator,
    /// Something to know about the diff that is no line of it.
    Note(Note),
    /// [`NO_NEWLINE`] under the line before it, which side by side shows in
    /// that line's column.
    NoNewline,
}

/// One line of the diff: a removed, an added or an unchanged one, with its
/// number on each side it is on.
struct LineRow {
    tag: ChangeTag,
    old: Option<usize>,
    new: Option<usize>,
    text: SourceLine,
}

/// A row of the diff side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pair {
    /// What the old and the new column show; `None` leaves a column
    /// blank, facing a line the other side has more of.
    Sides {
        old: Option<Half>,
        new: Option<Half>,
    },
    /// A separator or a note across both columns, by its unified row.
    Whole(usize),
}

/// What one column of a [`Pair`] shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Half {
    /// The line with this index into the lines.
    Line(usize),
    /// [`NO_NEWLINE`], under that side's last line.
    NoNewline,
}

impl Rows {
    /// The rows that show the change from `old` to `new`; a side left out
    /// is a file that did not exist before or no longer does. `cuts` are
    /// the limits that left a side only the start of a longer text, so the
    /// diff shows the rest of the other side as removed or added. Whether
    /// the file is new or deleted comes first, then the change between the
    /// notes of those limits. Unchanged lines are highlighted in
    /// `language`.
    fn of(
        old: Option<&str>,
        new: Option<&str>,
        cuts: impl IntoIterator<Item = Cut>,
        language: Option<&str>,
    ) -> Rows {
        let created_or_deleted = match (old, new) {
            (None, Some(_)) => Some(Note::new("new file")),
            (Some(_), None) => Some(Note::new("deleted file")),
            _ => None,
        };
        let (change, lines, digits) =
            Rows::change(old, new, created_or_deleted.is_some(), language);
        let rows: Vec<Row> = created_or_deleted
            .map(Row::Note)
            .into_iter()
            .chain(between_cut_notes(cuts, change, Row::Note))
            .collect();
        let SideBySide {
            pairs,
            pair_of_row,
            first_row_of_pair,
        } = pairs_of(&rows, &lines);
        Rows {
            rows,
            lines,
            digits,
            pairs,
            pair_of_row,
            first_row_of_pair,
            side_by_side: false,
            width: 0,
        }
    }

    /// The rows of the change from `old` to `new`, the lines they show,
    /// and how many digits their line numbers take. Text alike on both
    /// sides of a new or deleted file is an empty file.
    fn change(
        old: Option<&str>,
        new: Option<&str>,
        created_or_deleted: bool,
        language: Option<&str>,
    ) -> (Vec<Row>, Vec<LineRow>, usize) {
        let mut rows = Vec::new();
        let mut lines = Vec::new();
        // The two sides count together, as read, as the size warning counts
        // them, and a diff past the limit shows plain as a code entry does.
        let size = old.map_or(0, str::len) + new.map_or(0, str::len);
        let language = language.filter(|_| size <= HIGHLIGHT_LIMIT);
        // Lines compare without their line ends, so a file saved on
        // Windows does not show every line as removed and added again with
        // the same text. One note says the ends differ and names those of
        // each side, as nothing else would show them.
        let endings = match (old, new) {
            (Some(old), Some(new)) => {
                let (old, new) = (LineEnds::of(old), LineEnds::of(new));
                (old != new && old != LineEnds::NONE && new != LineEnds::NONE)
                    .then(|| endings_note(old, new))
            }
            _ => None,
        };
        let (old, new) = (
            unix_line_ends(old.unwrap_or_default()),
            unix_line_ends(new.unwrap_or_default()),
        );
        let (old, new) = (old.as_ref(), new.as_ref());
        if old == new {
            rows.push(Row::Note(Note::new(if created_or_deleted {
                "empty file"
            } else {
                "no changes"
            })));
            rows.extend(endings.map(Row::Note));
            return (rows, lines, 0);
        }
        rows.extend(endings.map(Row::Note));

        // Histogram, git's algorithm of that name, anchors the diff on
        // lines that occur rarely, such as a function's signature, rather
        // than on blank lines and lone braces.
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Histogram)
            .deadline(Instant::now() + DIFF_TIME)
            .diff_lines(old, new);
        // A last line without a newline differs from the same line with
        // one. Only where the two sides differ in that does a note say so,
        // or `- two` and `+ two` would look the same.
        let newline_differs = old.ends_with('\n') != new.ends_with('\n');

        let hunks = diff.grouped_ops(CONTEXT_LINES);
        let highlighted = highlighted_unchanged_lines(new, &hunks, language);
        for (index, hunk) in hunks.iter().enumerate() {
            if index > 0 {
                rows.push(Row::Separator);
            }
            for op in hunk {
                for change in diff.iter_changes(op) {
                    rows.push(Row::Line(lines.len()));
                    lines.push(line_row(&change, &highlighted));
                    if newline_differs && change.missing_newline() {
                        rows.push(Row::NoNewline);
                    }
                }
            }
        }
        let longest = diff.old_len().max(diff.new_len());
        (rows, lines, longest.to_string().len())
    }

    /// The columns before a line's text: both numbers, each followed by a
    /// blank, then the sign and a blank.
    fn gutter_width(&self) -> usize {
        2 * (self.digits + 1) + 2
    }

    /// Draws `row` of the unified diff into `area`, its text from column
    /// `left` on. Separators and notes show the same side by side, across
    /// both columns.
    fn draw_row(&self, row: &Row, area: Rect, buf: &mut Buffer, left: usize) {
        let width = usize::from(area.width);
        match row {
            Row::Separator => {
                let separator = SEPARATOR.repeat(width);
                buf.set_stringn(area.x, area.y, separator, width, theme::secondary());
            }
            Row::Note(note) => note.render(area, buf),
            Row::NoNewline => NO_NEWLINE.render(area, buf),
            Row::Line(index) => {
                let line = &self.lines[*index];
                let digits = self.digits;
                let numbers = format!(
                    "{:>digits$} {:>digits$} ",
                    number(line.old),
                    number(line.new)
                );
                draw_line(line, numbers, area, buf, left);
            }
        }
    }
}

/// `number` as the gutter shows it, or nothing for a side without the line.
fn number(number: Option<usize>) -> String {
    number.map_or_else(String::new, |number| number.to_string())
}

/// Draws `line` into `area` behind the gutter `numbers`, then its sign and
/// a blank, the text from column `left` on. A changed line fills the whole
/// of `area`, numbers, cut markers and the blanks after its text included,
/// so the change stands out at once. Dim is lifted in the fill, as under
/// the focus fill.
fn draw_line(line: &LineRow, numbers: String, area: Rect, buf: &mut Buffer, left: usize) {
    let sign = match line.tag {
        ChangeTag::Equal => " ",
        ChangeTag::Delete => "-",
        ChangeTag::Insert => "+",
    };
    let gutter = Line::from(vec![
        Span::styled(numbers, theme::secondary()),
        Span::styled(sign, line_style(line.tag)),
        Span::from(" "),
    ]);
    GutterRow {
        gutter,
        line: &line.text,
        left,
    }
    .render(area, buf);
    if line.tag != ChangeTag::Equal {
        buf.set_style(
            area,
            line_style(line.tag)
                .add_modifier(Modifier::REVERSED)
                .remove_modifier(Modifier::DIM),
        );
    }
}

impl Viewer for Rows {
    /// Lines never wrap, so only notes, which never scroll, depend on the
    /// width.
    fn lay_out(&mut self, width: u16, _height: u16) -> ContentSize {
        self.side_by_side = false;
        self.width = width;
        let columns = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Line(index) => self.gutter_width() + self.lines[*index].text.width(),
                // The separator fills whatever width the body has.
                Row::Separator => 0,
                Row::Note(note) => note.columns(usize::from(width)),
                Row::NoNewline => NO_NEWLINE.columns(usize::from(width)),
            })
            .max()
            .unwrap_or(0);
        ContentSize {
            lines: self.rows.len(),
            columns,
        }
    }

    /// Lays the pairs out in two columns where the body has room for them,
    /// and the unified rows where it has not. Both columns scroll sideways
    /// together, as far as the line that runs furthest past its column
    /// needs; separators and notes never scroll.
    fn lay_out_side_by_side(&mut self, width: u16, height: u16) -> ContentSize {
        if !self.fits_side_by_side(width) {
            return self.lay_out(width, height);
        }
        self.side_by_side = true;
        self.width = width;
        let (old, _, new) = columns_of(Rect::new(0, 0, width, 1));
        let gutter = self.half_gutter_width();
        let (old_room, new_room) = (
            usize::from(old.width) - gutter,
            usize::from(new.width) - gutter,
        );
        let overflow = self
            .pairs
            .iter()
            .map(|pair| match pair {
                Pair::Sides { old, new } => self
                    .overflow(*old, old_room)
                    .max(self.overflow(*new, new_room)),
                Pair::Whole(_) => 0,
            })
            .max()
            .unwrap_or(0);
        ContentSize {
            lines: self.pairs.len(),
            columns: usize::from(width) + overflow,
        }
    }

    fn diff_shape(&self) -> Option<DiffShape> {
        Some(DiffShape {
            layout: if self.side_by_side {
                DiffLayout::SideBySide
            } else {
                DiffLayout::Unified
            },
            fits_side_by_side: self.fits_side_by_side(self.width),
        })
    }

    /// A unified row shows side by side in the pair that holds it; a pair
    /// shows unified from the first row it holds, so a row facing a blank
    /// keeps its line on top. A top past the end counts as the last row.
    fn top_from(&self, top: usize, from: DiffLayout) -> usize {
        let last = |rows: &[usize]| rows.get(top.min(rows.len().saturating_sub(1))).copied();
        let translated = match (from, self.side_by_side) {
            (DiffLayout::Unified, true) => last(&self.pair_of_row),
            (DiffLayout::SideBySide, false) => last(&self.first_row_of_pair),
            _ => None,
        };
        translated.unwrap_or(top)
    }

    /// Draws the rows from `offset.top` on, each line with its numbers in a
    /// dim gutter and its sign after them. Side by side, each line shows in
    /// its side's column with its number there, and a dim divider stands
    /// between the columns.
    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        if !self.side_by_side {
            for (row_area, row) in rows_from(area, &self.rows, offset.top) {
                self.draw_row(row, row_area, buf, offset.left);
            }
            return None;
        }
        for (row_area, pair) in rows_from(area, &self.pairs, offset.top) {
            match pair {
                Pair::Whole(row) => self.draw_row(&self.rows[*row], row_area, buf, offset.left),
                Pair::Sides { old, new } => {
                    let (old_area, divider, new_area) = columns_of(row_area);
                    self.draw_half(*old, |line| line.old, old_area, buf, offset.left);
                    buf.set_string(divider.x, divider.y, "│", theme::secondary());
                    self.draw_half(*new, |line| line.new, new_area, buf, offset.left);
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------
// Side by side
// ---------------------------------------------------------------------

/// The fewest columns of text each side keeps beside its gutter. Below
/// that, two columns cannot show a line, and the diff shows unified.
const MIN_TEXT: usize = 8;

/// The rows of a diff side by side, and how they relate to the unified
/// rows.
struct SideBySide {
    pairs: Vec<Pair>,
    pair_of_row: Vec<usize>,
    first_row_of_pair: Vec<usize>,
}

/// One line of a run of changed lines, by its unified row and its index
/// into the lines, with the unified row of the note under it, if any.
struct Item {
    row: usize,
    line: usize,
    note: Option<usize>,
}

/// The pairs that show the unified `rows`, whose lines are `lines`, side
/// by side. An unchanged line shows on both sides. Within a run of changed
/// lines the k-th removed line faces the k-th added one, whatever order
/// the run has them in, and the side with more lines faces blanks with the
/// rest. The note of a line without a newline takes a row of its own
/// under that line, in its column, so the other side's lines after it
/// keep facing blanks. Separators and notes span both columns.
fn pairs_of(rows: &[Row], lines: &[LineRow]) -> SideBySide {
    let mut side_by_side = SideBySide {
        pairs: Vec::new(),
        pair_of_row: vec![0; rows.len()],
        first_row_of_pair: Vec::new(),
    };
    let (mut old, mut new): (Vec<Item>, Vec<Item>) = (Vec::new(), Vec::new());
    // Whether the changed line seen last was a removed one: a note under
    // it belongs to its side.
    let mut last_old = false;
    for (row, shown) in rows.iter().enumerate() {
        match shown {
            Row::Line(line) => {
                let item = Item {
                    row,
                    line: *line,
                    note: None,
                };
                match lines[*line].tag {
                    ChangeTag::Delete => {
                        old.push(item);
                        last_old = true;
                    }
                    ChangeTag::Insert => {
                        new.push(item);
                        last_old = false;
                    }
                    ChangeTag::Equal => {
                        side_by_side.flush(&mut old, &mut new);
                        let half = Some(Half::Line(*line));
                        side_by_side.push(Pair::Sides {
                            old: half,
                            new: half,
                        });
                        side_by_side.show(row, row);
                    }
                }
            }
            Row::NoNewline => {
                let side = if last_old { &mut old } else { &mut new };
                if let Some(item) = side.last_mut() {
                    item.note = Some(row);
                }
            }
            Row::Separator | Row::Note(_) => {
                side_by_side.flush(&mut old, &mut new);
                side_by_side.push(Pair::Whole(row));
                side_by_side.show(row, row);
            }
        }
    }
    side_by_side.flush(&mut old, &mut new);
    side_by_side
}

impl SideBySide {
    fn push(&mut self, pair: Pair) {
        self.pairs.push(pair);
    }

    /// Notes that the last pair shows the unified `row`, and that `first`
    /// is the first unified row it shows.
    fn show(&mut self, row: usize, first: usize) {
        let pair = self.pairs.len() - 1;
        self.pair_of_row[row] = pair;
        if self.first_row_of_pair.len() == pair {
            self.first_row_of_pair.push(first);
        }
    }

    /// Pairs the removed lines `old` with the added lines `new` of a run,
    /// and empties both for the next run.
    fn flush(&mut self, old: &mut Vec<Item>, new: &mut Vec<Item>) {
        for index in 0..old.len().max(new.len()) {
            let (old, new) = (old.get(index), new.get(index));
            self.push(Pair::Sides {
                old: old.map(|item| Half::Line(item.line)),
                new: new.map(|item| Half::Line(item.line)),
            });
            let first = [old, new]
                .into_iter()
                .flatten()
                .map(|item| item.row)
                .min()
                .expect("a pair of a run has a line on one side at least");
            for item in [old, new].into_iter().flatten() {
                self.show(item.row, first);
            }
            if let Some(note) = old.and_then(|item| item.note) {
                self.push(Pair::Sides {
                    old: Some(Half::NoNewline),
                    new: None,
                });
                self.show(note, note);
            }
            if let Some(note) = new.and_then(|item| item.note) {
                self.push(Pair::Sides {
                    old: None,
                    new: Some(Half::NoNewline),
                });
                self.show(note, note);
            }
        }
        old.clear();
        new.clear();
    }
}

impl Rows {
    /// The columns before a line's text in one column side by side: its
    /// number and a blank, then the sign and a blank.
    fn half_gutter_width(&self) -> usize {
        self.digits + 3
    }

    /// Whether a body `width` columns wide has room for two columns, each
    /// with its gutter and [`MIN_TEXT`] columns of text, and the divider.
    fn fits_side_by_side(&self, width: u16) -> bool {
        usize::from(width) >= 2 * (self.half_gutter_width() + MIN_TEXT) + usize::from(DIVIDER_WIDTH)
    }

    /// The width of the text of `half` beyond the `room` its column leaves
    /// for text, which sideways scrolling has to reach.
    fn overflow(&self, half: Option<Half>, room: usize) -> usize {
        match half {
            Some(Half::Line(line)) => self.lines[line].text.width().saturating_sub(room),
            Some(Half::NoNewline) | None => 0,
        }
    }

    /// Draws `half` into its column `area`, the line's number on that side
    /// taken by `number`; `None` leaves the column blank.
    fn draw_half(
        &self,
        half: Option<Half>,
        side: fn(&LineRow) -> Option<usize>,
        area: Rect,
        buf: &mut Buffer,
        left: usize,
    ) {
        match half {
            Some(Half::Line(index)) => {
                let line = &self.lines[index];
                let digits = self.digits;
                let numbers = format!("{:>digits$} ", number(side(line)));
                draw_line(line, numbers, area, buf, left);
            }
            Some(Half::NoNewline) => NO_NEWLINE.render(area, buf),
            None => {}
        }
    }
}

/// The columns a divider takes: the line and a blank after it, so a line
/// number of the new column never touches the line.
const DIVIDER_WIDTH: u16 = 2;

/// The old and the new column of a body `area` side by side, and the
/// divider between them: the old column takes the smaller half of what
/// the divider leaves.
fn columns_of(area: Rect) -> (Rect, Rect, Rect) {
    let old_width = (area.width - DIVIDER_WIDTH) / 2;
    let old = Rect {
        width: old_width,
        ..area
    };
    let divider = Rect {
        x: area.x + old_width,
        width: DIVIDER_WIDTH,
        ..area
    };
    let new = Rect {
        x: divider.x + DIVIDER_WIDTH,
        width: area.width - old_width - DIVIDER_WIDTH,
        ..area
    };
    (old, divider, new)
}

/// The row of one line of the diff, numbered from 1 on each side it is on.
/// An unchanged line takes its colours from `highlighted`, the new side's
/// lines highlighted from its start, by its number there. A changed line
/// takes its colour whole, with nothing marked inside it (ADR 33), and no
/// syntax colours, which its fill would make unreadable (ADR 60).
fn line_row(change: &Change<&str>, highlighted: &[StyledLine]) -> LineRow {
    let value = change.value();
    let text = value.strip_suffix('\n').unwrap_or(value);
    let unchanged = change
        .new_index()
        .filter(|_| change.tag() == ChangeTag::Equal)
        .and_then(|index| highlighted.get(index));
    LineRow {
        tag: change.tag(),
        old: change.old_index().map(|index| index + 1),
        new: change.new_index().map(|index| index + 1),
        text: unchanged.map_or_else(
            || SourceLine::new([(text, line_style(change.tag()))]),
            source_line,
        ),
    }
}

/// The lines of `new` highlighted in `language`, from its first line to the
/// last unchanged line that `hunks` show, or none without a language or
/// such a line. The text is highlighted as one, so a hunk that starts
/// inside a block comment or a string shows it as one: a hunk highlighted
/// alone would start in the grammar's first state and colour the inside of
/// a comment as code. Only unchanged lines take syntax colours, and the
/// state at a line depends only on the text above it, so nothing after the
/// last of them is highlighted.
fn highlighted_unchanged_lines(
    new: &str,
    hunks: &[Vec<DiffOp>],
    language: Option<&str>,
) -> Vec<StyledLine> {
    let end = hunks
        .iter()
        .flatten()
        .filter(|op| op.tag() == DiffTag::Equal)
        .map(|op| op.new_range().end)
        .max()
        .unwrap_or(0);
    match language {
        Some(language) if end > 0 => patine::highlight(first_lines(new, end), Some(language)),
        _ => Vec::new(),
    }
}

/// The first `count` lines of `text`, at least one, each with its `\n`:
/// the whole text when it has no more, so a last line without a newline
/// counts.
fn first_lines(text: &str, count: usize) -> &str {
    text.match_indices('\n')
        .nth(count - 1)
        .map_or(text, |(end, _)| &text[..=end])
}

/// The note shown when the two sides end their lines differently, which
/// names the ends of each side (ADR 33).
fn endings_note(old: LineEnds, new: LineEnds) -> Note {
    Note::from(format!(
        "line endings differ: old {}, new {}",
        old.name(),
        new.name()
    ))
}

/// Which line ends a text uses: `\n` as on Unix, `\r\n` as on Windows, and
/// a lone `\r` as on classic Mac OS. Notes name them `LF`, `CRLF` and `CR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineEnds {
    unix: bool,
    windows: bool,
    mac: bool,
}

impl LineEnds {
    /// A text of a single line without a line end, which can agree with
    /// any other.
    const NONE: LineEnds = LineEnds {
        unix: false,
        windows: false,
        mac: false,
    };

    fn of(text: &str) -> LineEnds {
        let mut ends = LineEnds::NONE;
        let mut bytes = text.bytes().peekable();
        while let Some(byte) = bytes.next() {
            match byte {
                b'\r' if bytes.peek() == Some(&b'\n') => {
                    bytes.next();
                    ends.windows = true;
                }
                b'\r' => ends.mac = true,
                b'\n' => ends.unix = true,
                _ => {}
            }
        }
        ends
    }

    /// How a note names these ends: `CRLF`, `LF` or `CR` for ends of one
    /// kind, and `mixed` with each kind for several, as in
    /// `mixed CRLF and LF`. [`LineEnds::NONE`] names no kind and comes out
    /// empty.
    fn name(self) -> String {
        let kinds: Vec<&str> = [(self.windows, "CRLF"), (self.unix, "LF"), (self.mac, "CR")]
            .into_iter()
            .filter_map(|(found, name)| found.then_some(name))
            .collect();
        match kinds.as_slice() {
            [rest @ .., last] if !rest.is_empty() => {
                format!("mixed {} and {last}", rest.join(", "))
            }
            one_or_none => one_or_none.concat(),
        }
    }
}

/// `text` with every `\r\n` and every lone `\r` turned into `\n`. A lone
/// `\r` counts as a line end, so a file with classic Mac OS line ends
/// shows its lines rather than one line with a `␍` between them.
fn unix_line_ends(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// Removed lines are red and added ones green, in the terminal's palette,
/// so its theme decides the exact colours. Drawn reversed, these are the
/// fills of changed rows.
fn line_style(tag: ChangeTag) -> Style {
    match tag {
        ChangeTag::Equal => Style::new(),
        ChangeTag::Delete => theme::error(),
        ChangeTag::Insert => theme::success(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ratatui::style::{Color, Modifier};

    use super::super::code::HIGHLIGHT_LIMIT;
    use super::super::source_file::{MAX_BYTES, MAX_LINES};
    use super::super::test_support::{drawn, row, rows, shown};
    use super::*;

    /// The viewer of the change from `old` to `new`, given inline.
    fn inline(old: Option<&str>, new: Option<&str>) -> Content<Rows> {
        let side = |text: Option<&str>| {
            text.map(|text| DiffSide {
                path: None,
                content: Some(text.to_owned()),
            })
        };
        diff_content(
            &DiffEntry {
                title: None,
                old: side(old),
                new: side(new),
                name: None,
                ..DiffEntry::default()
            },
            None,
        )
    }

    /// The viewer of the rows of `old` and `new`, where `cuts` are the
    /// limits that left a side only the start of a longer text.
    fn of_rows(old: &str, new: &str, cuts: impl IntoIterator<Item = Cut>) -> Content<Rows> {
        Content::Body(Rows::of(Some(old), Some(new), cuts, None))
    }

    fn file(path: &str) -> Option<DiffSide> {
        Some(DiffSide {
            path: Some(path.to_owned()),
            content: None,
        })
    }

    fn from_files(old: Option<DiffSide>, new: Option<DiffSide>) -> Content<Rows> {
        diff_content(
            &DiffEntry {
                title: None,
                old,
                new,
                name: None,
                ..DiffEntry::default()
            },
            None,
        )
    }

    /// The columns of row `y` whose cells carry `modifier`.
    fn with_modifier(buf: &Buffer, y: u16, modifier: Modifier) -> String {
        (0..buf.area.width)
            .filter(|&x| buf[(x, y)].modifier.contains(modifier))
            .map(|x| buf[(x, y)].symbol())
            .collect()
    }

    const OLD_FETCHER: &str = "\
use std::time::Duration;

pub struct Fetcher {
    client: Client,
}

impl Fetcher {
    pub fn new(client: Client) -> Self {
        Fetcher { client }
    }

    pub fn fetch(&self, url: &str) -> Result<Response> {
        self.client.get(url)
    }

    pub fn close(self) {
        drop(self.client);
    }
}
";

    /// [`OLD_FETCHER`] with a field added near the top and a call changed
    /// near the bottom, far enough apart for two hunks.
    fn new_fetcher() -> String {
        OLD_FETCHER
            .replace(
                "    client: Client,\n",
                "    client: Client,\n    retries: u32,\n",
            )
            .replace("get(url)", "get(url).retry(self.retries)")
    }

    // -----------------------------------------------------------------
    // Hunks
    // -----------------------------------------------------------------

    #[test]
    fn a_modified_file_shows_its_hunks_with_three_lines_of_context() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));

        insta::assert_snapshot!("two_hunks_60", shown(&mut viewer, 60, 0));
        insta::assert_snapshot!("two_hunks_40", shown(&mut viewer, 40, 0));
    }

    #[test]
    fn the_content_is_as_wide_as_the_gutter_and_the_widest_line() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));

        let size = viewer.lay_out(60, 10);

        // Two hunks of 7 and 8 lines and the separator between them.
        assert_eq!(size.lines, 16);
        // `NN NN + ` and the widest line of the hunks, a context line.
        let widest = "    pub fn fetch(&self, url: &str) -> Result<Response> {".len();
        assert_eq!(size.columns, 8 + widest);
        assert_eq!(viewer.lay_out(30, 10), size, "nothing wraps");
    }

    #[test]
    fn the_gutter_and_the_separator_are_dim_and_the_changes_coloured() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));
        let buf = drawn(&mut viewer, 60, 16, Offset::default());

        assert_eq!(row(&buf, 1), " 3  3   pub struct Fetcher {");
        assert_eq!(
            with_modifier(&buf, 1, Modifier::DIM),
            " 3  3 ",
            "the gutter"
        );
        assert_eq!(buf[(8, 1)].fg, Color::Reset, "context keeps the default");

        assert_eq!(row(&buf, 3), "    5 +     retries: u32,");
        assert_eq!(buf[(6, 3)].fg, Color::Green, "the sign");
        assert_eq!(buf[(12, 3)].fg, Color::Green, "the text");
        assert!(!buf[(6, 3)].modifier.contains(Modifier::DIM));

        assert_eq!(row(&buf, 7), "┈".repeat(60), "across the body");
        assert_eq!(with_modifier(&buf, 7, Modifier::DIM).chars().count(), 60);

        assert_eq!(row(&buf, 11), "13    -         self.client.get(url)");
        assert_eq!(buf[(6, 11)].fg, Color::Red);
        assert_eq!(buf[(20, 11)].fg, Color::Red);
    }

    #[test]
    fn it_draws_from_the_offsets_line_on() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));

        let buf = drawn(&mut viewer, 60, 3, Offset { top: 7, left: 0 });

        assert_eq!(row(&buf, 0), "┈".repeat(60));
        assert_eq!(row(&buf, 1), "10 11       }");
    }

    // -----------------------------------------------------------------
    // Changed lines
    // -----------------------------------------------------------------

    #[test]
    fn a_changed_line_shows_whole_in_its_colour_without_marked_words() {
        let mut viewer = inline(
            Some("let delay = 100;\nget(url)\n"),
            Some("let delay = 250;\nget(uri)\n"),
        );

        insta::assert_snapshot!("changed_line", shown(&mut viewer, 30, 0));
        let buf = drawn(&mut viewer, 30, 4, Offset::default());
        // Every cell of a row in one style: nothing inside the line is
        // marked apart from the rest of it.
        for (y, line) in [
            (0, Color::Red),
            (1, Color::Red),
            (2, Color::Green),
            (3, Color::Green),
        ] {
            assert_filled(&buf, y, line);
        }
    }

    // -----------------------------------------------------------------
    // Fills
    // -----------------------------------------------------------------

    /// Asserts that every cell of row `y` is reversed, without any other
    /// modifier, in `colour`.
    fn assert_filled(buf: &Buffer, y: u16, colour: Color) {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            assert_eq!(cell.modifier, Modifier::REVERSED, "row {y}, column {x}");
            assert_eq!(cell.fg, colour, "row {y}, column {x}");
        }
    }

    #[test]
    fn a_changed_row_is_filled_in_its_colour_to_the_edge() {
        let mut viewer = inline(Some("a\nb\n"), Some("a\nc\n"));

        let buf = drawn(&mut viewer, 20, 3, Offset::default());

        assert_eq!(row(&buf, 1), "2   - b");
        assert_filled(&buf, 1, Color::Red);
        assert_eq!(row(&buf, 2), "  2 + c");
        assert_filled(&buf, 2, Color::Green);
        assert_eq!(
            with_modifier(&buf, 0, Modifier::REVERSED),
            "",
            "an unchanged row"
        );
    }

    #[test]
    fn a_scrolled_changed_row_keeps_its_fill_under_the_cut_markers() {
        let long = format!("let message = \"{}\";\n", "a long text ".repeat(4));
        let mut viewer = inline(Some("let message = \"short\";\n"), Some(&long));

        let buf = drawn(&mut viewer, 40, 2, Offset { top: 0, left: 20 });

        assert_eq!(buf[(6, 1)].symbol(), "‹");
        assert_eq!(buf[(39, 1)].symbol(), "›");
        assert_filled(&buf, 1, Color::Green);
    }

    // -----------------------------------------------------------------
    // The diff as a file
    // -----------------------------------------------------------------

    /// The `.diff` text of the entry `json` describes, its `name` or
    /// `title` the header's label.
    fn diff_text(json: serde_json::Value) -> Result<String, String> {
        let entry: DiffEntry = serde_json::from_value(json).expect("test entry parses");
        let label = entry
            .name
            .clone()
            .or(entry.title.clone())
            .unwrap_or_default();
        unified_text(&entry, &label)
    }

    #[test]
    fn the_diff_file_names_the_file_and_holds_the_hunk_with_three_lines_of_context() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n";
        let new = "1\n2\n3\n4\nfive\n6\n7\n8\n";

        let text = diff_text(serde_json::json!({
            "name": "fetch.rs", "old": {"content": old}, "new": {"content": new}
        }));

        assert_eq!(
            text.as_deref(),
            Ok(
                "--- a/fetch.rs\n+++ b/fetch.rs\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n"
            )
        );
    }

    #[test]
    fn the_diff_file_of_a_new_file_comes_from_dev_null() {
        let text = diff_text(serde_json::json!({"name": "a.rs", "new": {"content": "x\n"}}));

        assert_eq!(
            text.as_deref(),
            Ok("--- /dev/null\n+++ b/a.rs\n@@ -0,0 +1 @@\n+x\n")
        );
    }

    #[test]
    fn the_diff_file_of_a_deleted_file_goes_to_dev_null() {
        let text = diff_text(serde_json::json!({"name": "a.rs", "old": {"content": "x\n"}}));

        assert_eq!(
            text.as_deref(),
            Ok("--- a/a.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n")
        );
    }

    #[test]
    fn the_diff_file_marks_a_missing_newline() {
        let text = diff_text(serde_json::json!({
            "title": "a.rs", "old": {"content": "x\n"}, "new": {"content": "x"}
        }))
        .expect("readable");

        assert!(
            text.ends_with("+x\n\\ No newline at end of file\n"),
            "{text}"
        );
    }

    #[test]
    fn the_diff_file_of_sides_apart_only_in_line_ends_says_so_above_its_header() {
        let text = diff_text(serde_json::json!({
            "name": "a.rs", "old": {"content": "x\r\ny\r\n"}, "new": {"content": "x\ny\n"}
        }));

        assert_eq!(
            text.as_deref(),
            Ok("line endings differ: old CRLF, new LF\n--- a/a.rs\n+++ b/a.rs\n")
        );
    }

    #[test]
    fn a_side_that_cannot_be_read_names_the_side() {
        let text = diff_text(serde_json::json!({
            "old": {"path": "/nowhere/old.rs"}, "new": {"content": "x\n"}
        }));

        let error = text.expect_err("the old side is missing");
        assert!(error.starts_with("old side: "), "{error}");
    }

    #[test]
    fn a_side_cut_after_its_lines_says_so_above_the_header() {
        let new = "x\n".repeat(MAX_LINES + 1);

        let text = diff_text(serde_json::json!({"name": "a.rs", "new": {"content": new}}))
            .expect("readable");

        assert!(
            text.starts_with(
                "first 500,000 lines shown\n… the rest is cut after 500,000 lines\n\
                 --- /dev/null\n+++ b/a.rs\n"
            ),
            "{}",
            &text[..120]
        );
    }

    // -----------------------------------------------------------------
    // Highlighting
    // -----------------------------------------------------------------

    /// The viewer of the diff entry `json` describes.
    fn entry(json: serde_json::Value) -> Content<Rows> {
        let entry: DiffEntry = serde_json::from_value(json).expect("test entry parses");
        diff_content(&entry, None)
    }

    /// A diff entry with `fields` that changes the second of three lines
    /// of Rust.
    fn rust_change(fields: serde_json::Value) -> Content<Rows> {
        let mut json = serde_json::json!({
            "old": {"content": "fn a() {}\nlet x = 1;\nfn b() {}\n"},
            "new": {"content": "fn a() {}\nlet x = 2;\nfn b() {}\n"},
        });
        json.as_object_mut()
            .expect("an object")
            .extend(fields.as_object().expect("an object").clone());
        entry(json)
    }

    /// The foreground of the first cell of `text` in row `y`.
    fn fg_in_row(buf: &Buffer, y: u16, text: &str) -> Color {
        let shown = row(buf, y);
        let x = shown
            .find(text)
            .unwrap_or_else(|| panic!("{text:?} in row {y}: {shown:?}"));
        buf[(u16::try_from(x).expect("a short row"), y)].fg
    }

    #[test]
    fn a_diff_is_highlighted_in_the_language_of_its_name() {
        let mut viewer = rust_change(serde_json::json!({"name": "src/fetch.rs"}));

        let buf = drawn(&mut viewer, 30, 4, Offset::default());

        assert_eq!(row(&buf, 0), "1 1   fn a() {}");
        assert_eq!(fg_in_row(&buf, 0, "fn"), Color::Indexed(5), "a keyword");
        assert_eq!(fg_in_row(&buf, 3, "fn"), Color::Indexed(5));
    }

    #[test]
    fn the_language_key_takes_precedence_over_the_name() {
        let mut viewer = rust_change(serde_json::json!({"name": "notes.txt", "language": "rust"}));

        let buf = drawn(&mut viewer, 30, 4, Offset::default());

        assert_eq!(fg_in_row(&buf, 0, "fn"), Color::Indexed(5));
    }

    #[test]
    fn a_name_without_an_extension_picks_its_grammar() {
        let mut viewer = entry(serde_json::json!({
            "name": "Dockerfile",
            "old": {"content": "FROM rust:1.97\nRUN make\n"},
            "new": {"content": "FROM rust:1.97\nRUN make test\n"},
        }));

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(fg_in_row(&buf, 0, "FROM"), Color::Indexed(5));
    }

    #[test]
    fn the_title_picks_no_language() {
        let mut viewer = rust_change(serde_json::json!({"title": "fetch.rs"}));

        let buf = drawn(&mut viewer, 30, 4, Offset::default());

        assert_eq!(fg_in_row(&buf, 0, "fn"), Color::Reset);
    }

    #[test]
    fn an_unknown_language_shows_plain() {
        let mut unknown = rust_change(serde_json::json!({"language": "no-such-language"}));
        let mut none = rust_change(serde_json::json!({}));

        assert_eq!(
            drawn(&mut unknown, 30, 4, Offset::default()),
            drawn(&mut none, 30, 4, Offset::default())
        );
    }

    #[test]
    fn a_changed_line_shows_no_syntax_colours() {
        let mut viewer = rust_change(serde_json::json!({"name": "fetch.rs"}));

        let buf = drawn(&mut viewer, 30, 4, Offset::default());

        assert_eq!(row(&buf, 1), "2   - let x = 1;");
        assert_filled(&buf, 1, Color::Red);
        assert_filled(&buf, 2, Color::Green);
    }

    #[test]
    fn a_hunk_inside_a_block_comment_is_coloured_as_a_comment() {
        let old = "/*\none\ntwo\nthree\nfour\nold line\nfn helper() {}\n*/\n";
        let mut viewer = entry(serde_json::json!({
            "name": "lib.rs",
            "old": {"content": old},
            "new": {"content": old.replace("old line", "new line")},
        }));

        let buf = drawn(&mut viewer, 30, 7, Offset::default());

        assert_eq!(row(&buf, 0), "3 3   two", "the hunk starts in the comment");
        assert_eq!(fg_in_row(&buf, 0, "two"), Color::Indexed(2));
        assert_filled(&buf, 3, Color::Red);
        assert_eq!(row(&buf, 5), "7 7   fn helper() {}");
        assert_eq!(fg_in_row(&buf, 5, "fn"), Color::Indexed(2), "not a keyword");
    }

    #[test]
    fn an_unchanged_line_takes_the_colours_of_the_new_side() {
        let mut viewer = entry(serde_json::json!({
            "name": "lib.rs",
            "old": {"content": "fn a() {}\nfn b() {}\n"},
            "new": {"content": "/*\nfn a() {}\nfn b() {}\n"},
        }));

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 1), "1 2   fn a() {}");
        assert_eq!(
            fg_in_row(&buf, 1, "fn"),
            Color::Indexed(2),
            "inside the comment"
        );
    }

    #[test]
    fn a_last_line_without_a_newline_is_highlighted() {
        let mut viewer = entry(serde_json::json!({
            "name": "a.rs",
            "old": {"content": "fn a() {}\nfn b() {}"},
            "new": {"content": "fn x() {}\nfn b() {}"},
        }));

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 2), "2 2   fn b() {}");
        assert_eq!(fg_in_row(&buf, 2, "fn"), Color::Indexed(5));
    }

    #[test]
    fn a_new_file_with_a_language_shows_its_rows_filled() {
        let mut viewer = entry(serde_json::json!({
            "name": "a.rs",
            "new": {"content": "fn a() {}\nfn b() {}\n"},
        }));

        let buf = drawn(&mut viewer, 20, 3, Offset::default());

        assert_eq!(row(&buf, 1), "  1 + fn a() {}");
        assert_filled(&buf, 1, Color::Green);
        assert_filled(&buf, 2, Color::Green);
    }

    #[test]
    fn a_deleted_file_with_a_language_shows_its_rows_filled() {
        let mut viewer = entry(serde_json::json!({
            "name": "a.rs",
            "old": {"content": "fn a() {}\nfn b() {}\n"},
        }));

        let buf = drawn(&mut viewer, 20, 3, Offset::default());

        assert_eq!(row(&buf, 1), "1   - fn a() {}");
        assert_filled(&buf, 1, Color::Red);
        assert_filled(&buf, 2, Color::Red);
    }

    #[test]
    fn sides_with_windows_line_ends_are_highlighted_line_by_line() {
        let mut viewer = entry(serde_json::json!({
            "name": "a.rs",
            "old": {"content": "fn a() {}\r\nlet x = 1;\r\nfn b() {}\r\n"},
            "new": {"content": "fn a() {}\r\nlet x = 2;\r\nfn b() {}\r\n"},
        }));

        let buf = drawn(&mut viewer, 30, 4, Offset::default());

        assert_eq!(fg_in_row(&buf, 0, "fn"), Color::Indexed(5));
        assert_eq!(row(&buf, 3), "3 3   fn b() {}");
        assert_eq!(fg_in_row(&buf, 3, "fn"), Color::Indexed(5));
        assert_eq!(
            fg_in_row(&buf, 3, "b"),
            Color::Indexed(4),
            "a function's name"
        );
    }

    /// The viewer of a Rust diff that changes its first line, after
    /// `old_head` and `new_head`, and leaves the lines after it alike:
    /// so many of them that both sides together take `bytes` bytes.
    fn rust_change_of(old_head: &str, new_head: &str, bytes: usize) -> Content<Rows> {
        let tail = "fn a() {}\n";
        let filler = (bytes - old_head.len() - new_head.len()) / 2 - tail.len();
        let line = "// filler\n";
        let lines = filler / line.len() - 1;
        let last = filler - lines * line.len();
        let rest = format!("{tail}{}{}\n", line.repeat(lines), "x".repeat(last - 1));
        let (old, new) = (format!("{old_head}{rest}"), format!("{new_head}{rest}"));
        assert_eq!(old.len() + new.len(), bytes);
        entry(serde_json::json!({
            "name": "a.rs",
            "old": {"content": old},
            "new": {"content": new},
        }))
    }

    #[test]
    fn a_diff_of_one_mib_is_highlighted() {
        let mut viewer = rust_change_of("let x = 1;\n", "let x = 2;\n", HIGHLIGHT_LIMIT);

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert!(
            row(&buf, 2).ends_with(" 2   fn a() {}"),
            "{:?}",
            row(&buf, 2)
        );
        assert_eq!(fg_in_row(&buf, 2, "fn"), Color::Indexed(5));
    }

    #[test]
    fn a_diff_above_one_mib_shows_plain() {
        let mut viewer = rust_change_of("let x = 1;\n", "let x = 22;\n", HIGHLIGHT_LIMIT + 1);

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert!(
            row(&buf, 2).ends_with(" 2   fn a() {}"),
            "{:?}",
            row(&buf, 2)
        );
        assert_eq!(fg_in_row(&buf, 2, "fn"), Color::Reset);
    }

    // -----------------------------------------------------------------
    // New, deleted and unchanged files
    // -----------------------------------------------------------------

    #[test]
    fn without_old_text_it_is_a_new_file() {
        let mut viewer = inline(None, Some("fn main() {\n    run();\n}\n"));

        insta::assert_snapshot!("new_file", shown(&mut viewer, 30, 0));
        let buf = drawn(&mut viewer, 30, 4, Offset::default());
        assert_eq!(with_modifier(&buf, 0, Modifier::DIM), "new file");
        assert_eq!(buf[(4, 1)].fg, Color::Green);
    }

    #[test]
    fn without_new_text_it_is_a_deleted_file() {
        let mut viewer = inline(Some("fn main() {\n    run();\n}\n"), None);

        insta::assert_snapshot!("deleted_file", shown(&mut viewer, 30, 0));
        let buf = drawn(&mut viewer, 30, 4, Offset::default());
        assert_eq!(with_modifier(&buf, 0, Modifier::DIM), "deleted file");
        assert_eq!(buf[(4, 1)].fg, Color::Red);
    }

    #[test]
    fn the_same_text_on_both_sides_says_there_are_no_changes() {
        let mut viewer = inline(Some("same\n"), Some("same\n"));

        insta::assert_snapshot!("no_changes", shown(&mut viewer, 30, 0));
        let buf = drawn(&mut viewer, 30, 1, Offset::default());
        assert_eq!(with_modifier(&buf, 0, Modifier::DIM), "no changes");
        assert_eq!(
            viewer.lay_out(30, 5),
            ContentSize {
                lines: 1,
                columns: 10
            }
        );
    }

    #[test]
    fn an_empty_new_or_deleted_file_says_so() {
        let mut new = inline(None, Some(""));
        assert_eq!(
            shown(&mut new, 20, 0),
            "\"new file            \"\n\"empty file          \""
        );

        let mut deleted = inline(Some(""), None);
        assert_eq!(
            shown(&mut deleted, 20, 0),
            "\"deleted file        \"\n\"empty file          \""
        );
    }

    #[test]
    fn a_note_wider_than_the_body_counts_only_the_body() {
        let mut viewer = inline(Some("same\n"), Some("same\n"));

        assert_eq!(viewer.lay_out(6, 5).columns, 6, "never scrolls sideways");
        assert_eq!(
            row(&drawn(&mut viewer, 6, 1, Offset::default()), 0),
            "no cha"
        );
    }

    // -----------------------------------------------------------------
    // Line ends
    // -----------------------------------------------------------------

    #[test]
    fn a_missing_newline_shows_where_it_is_the_only_difference() {
        let mut viewer = inline(Some("one\ntwo"), Some("one\ntwo\n"));

        insta::assert_snapshot!("no_newline_old", shown(&mut viewer, 40, 0));

        let mut viewer = inline(Some("one\ntwo\n"), Some("one\ntwo"));
        let buf = drawn(&mut viewer, 40, 4, Offset::default());
        assert_eq!(row(&buf, 2), "  2 + two");
        assert_eq!(
            row(&buf, 3),
            "no newline at end of file",
            "after the added line"
        );
    }

    #[test]
    fn a_missing_newline_on_both_sides_goes_unmentioned() {
        let mut viewer = inline(Some("one\ntwo"), Some("one\nthree"));

        let text = shown(&mut viewer, 40, 0);

        assert!(!text.contains("newline"), "{text}");
        assert_eq!(viewer.lay_out(40, 5).lines, 3);
    }

    #[test]
    fn windows_line_ends_do_not_show() {
        let mut viewer = inline(Some("a\r\nb\r\n"), Some("a\r\nc\r\n"));

        assert_eq!(
            shown(&mut viewer, 20, 0),
            "\"1 1   a             \"\n\"2   - b             \"\n\"  2 + c             \"",
            "the same line ends on both sides need no note"
        );
    }

    #[test]
    fn sides_that_differ_only_in_their_line_ends_have_no_changes() {
        for (old, new, note) in [
            (
                "a\r\nb\r\n",
                "a\nb\n",
                "line endings differ: old CRLF, new LF",
            ),
            (
                "a\nb\n",
                "a\r\nb\r\n",
                "line endings differ: old LF, new CRLF",
            ),
            ("a\rb\r", "a\nb\n", "line endings differ: old CR, new LF"),
            (
                "a\r\nb\n",
                "a\nb\n",
                "line endings differ: old mixed CRLF and LF, new LF",
            ),
            (
                "a\r\nb\nc\r",
                "a\r\nb\r\nc\r\n",
                "line endings differ: old mixed CRLF, LF and CR, new CRLF",
            ),
        ] {
            let mut viewer = inline(Some(old), Some(new));

            let buf = drawn(&mut viewer, 60, 2, Offset::default());

            assert_eq!(row(&buf, 0), "no changes", "{old:?} {new:?}");
            assert_eq!(row(&buf, 1), note, "{old:?} {new:?}");
            assert_eq!(with_modifier(&buf, 1, Modifier::DIM), note);
        }
    }

    #[test]
    fn a_change_between_line_ends_of_two_kinds_shows_as_one_note_above_the_hunks() {
        let mut viewer = inline(Some("a\r\nb\r\nc\r\n"), Some("a\nB\nc\n"));

        assert_eq!(
            shown(&mut viewer, 40, 0),
            [
                "\"line endings differ: old CRLF, new LF   \"",
                "\"1 1   a                                 \"",
                "\"2   - b                                 \"",
                "\"  2 + B                                 \"",
                "\"3 3   c                                 \"",
            ]
            .join("\n")
        );
    }

    #[test]
    fn a_narrow_pane_cuts_the_endings_note_at_its_edge() {
        let mut viewer = inline(Some("a\r\nb\r\n"), Some("a\nc\n"));

        assert_eq!(viewer.lay_out(60, 4).columns, 37, "the note's own width");
        assert_eq!(viewer.lay_out(20, 4).columns, 20, "never scrolls sideways");
        let buf = drawn(&mut viewer, 20, 1, Offset::default());
        assert_eq!(row(&buf, 0), "line endings differ:");
    }

    #[test]
    fn a_lone_carriage_return_ends_a_line() {
        let mut viewer = inline(Some("one\rtwo\r"), Some("one\rthree\r"));

        let buf = drawn(&mut viewer, 20, 3, Offset::default());

        assert_eq!(row(&buf, 0), "1 1   one");
        assert_eq!(row(&buf, 1), "2   - two");
        assert_eq!(row(&buf, 2), "  2 + three");
    }

    #[test]
    fn a_side_without_any_line_end_differs_in_no_line_ends() {
        let mut viewer = inline(Some("one"), Some("one\r\n"));

        let text = shown(&mut viewer, 30, 0);

        assert!(!text.contains("line endings"), "{text}");
        assert!(text.contains("no newline at end of file"), "{text}");
    }

    #[test]
    fn a_new_file_with_windows_line_ends_has_no_note_about_them() {
        let mut viewer = inline(None, Some("a\r\n"));

        assert_eq!(
            shown(&mut viewer, 12, 0),
            "\"new file    \"\n\"  1 + a     \""
        );
    }

    // -----------------------------------------------------------------
    // Long lines, tabs and control characters
    // -----------------------------------------------------------------

    #[test]
    fn a_long_line_scrolls_sideways_under_a_gutter_that_stays() {
        let long = format!("let message = \"{}\";\n", "a long text ".repeat(4));
        let mut viewer = inline(Some("let message = \"short\";\n"), Some(&long));

        assert_eq!(viewer.lay_out(40, 5).columns, 6 + long.len() - 1);
        insta::assert_snapshot!("long_line", shown(&mut viewer, 40, 0));
        insta::assert_snapshot!("long_line_scrolled", shown(&mut viewer, 40, 20));
        insta::assert_snapshot!("long_line_at_its_end", shown(&mut viewer, 40, 31));
    }

    #[test]
    fn tabs_line_up_at_stops_of_four_and_controls_show_their_pictures() {
        let mut viewer = inline(Some("\tx = 1;\n"), Some("\tx = 2;\u{1b}\n"));

        let buf = drawn(&mut viewer, 20, 2, Offset::default());

        assert_eq!(row(&buf, 0), "1   -     x = 1;");
        assert_eq!(row(&buf, 1), "  1 +     x = 2;␛");
        assert_eq!(viewer.lay_out(20, 2).columns, 6 + 11);
    }

    // -----------------------------------------------------------------
    // Files
    // -----------------------------------------------------------------

    #[test]
    fn the_sides_can_come_from_files() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let old = directory.path().join("old.rs");
        let new = directory.path().join("new.rs");
        fs::write(&old, "a\nb\n").expect("old file is writable");
        fs::write(&new, "a\nc\n").expect("new file is writable");

        let mut viewer = from_files(
            file(old.to_str().expect("a UTF-8 path")),
            file(new.to_str().expect("a UTF-8 path")),
        );

        let buf = drawn(&mut viewer, 20, 3, Offset::default());
        assert_eq!(row(&buf, 1), "2   - b");
        assert_eq!(row(&buf, 2), "  2 + c");
    }

    #[test]
    fn a_file_with_invalid_utf8_shows_the_replacement_character() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let latin1 = directory.path().join("latin1.txt");
        fs::write(&latin1, b"gr\xfc\xdf\n").expect("file is writable");

        let mut viewer = from_files(None, file(latin1.to_str().expect("a UTF-8 path")));

        let buf = drawn(&mut viewer, 20, 2, Offset::default());
        assert_eq!(row(&buf, 1), "  1 + gr\u{fffd}\u{fffd}");
    }

    #[test]
    fn a_file_that_cannot_be_read_shows_why_as_the_content() {
        let mut viewer = from_files(file("/nowhere/old.rs"), file("/nowhere/new.rs"));

        insta::assert_snapshot!("unreadable", shown(&mut viewer, 30, 0));
        // The reason wraps over three rows inside the 26 columns of a card
        // at 30, and the card is as wide as the widest of them shows.
        let buf = drawn(&mut viewer, 30, 8, Offset::default());
        assert_eq!(row(&buf, 0), format!(" ╭{}╮", "─".repeat(26)));
        assert!(
            row(&buf, 1).starts_with(" │ ✗ old side: No such file"),
            "the first side that fails: {buf:?}"
        );
        assert!(row(&buf, 5).starts_with(" │ old.rs "), "{buf:?}");
        assert!(row(&buf, 6).starts_with(" │ /nowhere/ "), "{buf:?}");
        assert!(viewer.failed());
    }

    #[test]
    fn a_side_too_long_to_read_whole_says_so_above_and_below_the_hunks() {
        let mut viewer = of_rows("a\n", "b\n", [Cut::Bytes]);

        let buf = drawn(&mut viewer, 30, 4, Offset::default());

        assert_eq!(row(&buf, 0), "first 8 MiB shown");
        assert_eq!(with_modifier(&buf, 0, Modifier::DIM), "first 8 MiB shown");
        assert_eq!(row(&buf, 1), "1   - a");
        assert_eq!(row(&buf, 3), "… the rest is cut after 8 MiB");
        assert_eq!(
            with_modifier(&buf, 3, Modifier::DIM),
            "… the rest is cut after 8 MiB"
        );

        assert_eq!(viewer.lay_out(10, 4).columns, 10);
        let buf = drawn(&mut viewer, 10, 4, Offset::default());
        assert_eq!(row(&buf, 0), "first 8 Mi", "cut at the width");
        assert_eq!(row(&buf, 3), "… the rest");
    }

    #[test]
    fn a_body_narrower_than_the_gutter_shows_what_fits_of_it() {
        let mut viewer = inline(Some("a\n"), Some("b\n"));

        let buf = drawn(&mut viewer, 5, 2, Offset::default());
        assert_eq!(row(&buf, 0), "1   -");
        assert_eq!(row(&buf, 1), "  1 +");
        assert_eq!(buf[(4, 1)].fg, Color::Green, "the sign keeps its colour");
        assert_filled(&buf, 0, Color::Red);
        assert_filled(&buf, 1, Color::Green);

        let buf = drawn(&mut viewer, 3, 1, Offset::default());
        assert_eq!(row(&buf, 0), "1", "the numbers alone");
        assert_filled(&buf, 0, Color::Red);
    }

    #[test]
    fn sides_cut_alike_say_so_after_no_changes() {
        let mut viewer = of_rows("a\n", "a\n", [Cut::Bytes]);

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 1), "no changes");
        assert_eq!(row(&buf, 2), "… the rest is cut after 8 MiB");
    }

    #[test]
    fn a_side_above_the_line_limit_shows_the_line_notes() {
        let limit = super::super::source_file::MAX_LINES;
        let mut viewer = entry(serde_json::json!({"new": {"content": "x\n".repeat(limit + 1)}}));

        let lines = viewer.lay_out(40, 3).lines;
        assert_eq!(lines, limit + 3, "new file, two notes and the lines kept");
        let buf = drawn(&mut viewer, 40, 2, Offset::default());
        assert_eq!(row(&buf, 0), "new file");
        assert_eq!(row(&buf, 1), "first 500,000 lines shown");
        let buf = drawn(
            &mut viewer,
            40,
            1,
            Offset {
                top: lines - 1,
                left: 0,
            },
        );
        assert_eq!(row(&buf, 0), "… the rest is cut after 500,000 lines");
    }

    /// Each side is cut by its own limit, and the notes do not say which
    /// side a limit cut.
    #[test]
    fn sides_cut_by_different_limits_show_both_notes() {
        let mut viewer = of_rows("a\n", "a\n", [Cut::Lines, Cut::Bytes]);

        let buf = drawn(&mut viewer, 40, 5, Offset::default());

        assert_eq!(
            (0..5).map(|y| row(&buf, y)).collect::<Vec<_>>(),
            [
                "first 8 MiB shown",
                "first 500,000 lines shown",
                "no changes",
                "… the rest is cut after 8 MiB",
                "… the rest is cut after 500,000 lines",
            ]
        );
    }

    /// A line `a` and lines of distinct numbers, each followed by `end`,
    /// and the last without one when `last_end` is false. With `\r\n` and the last end,
    /// they fill [`MAX_BYTES`] and one byte more, so the last `\r` is the
    /// last byte under the limit and its `\n` the first past it. Lines of
    /// 1 KiB keep a diff of a few of them cheap where a single line of
    /// 8 MiB would not be.
    fn lines_past_the_limit(end: &str, last_end: bool) -> String {
        let numbered = (0..8191).map(|number| format!("{number:0>1022}"));
        let last = format!("{:0>1020}", 8192);
        let lines: Vec<String> = std::iter::once("a".to_owned())
            .chain(numbered)
            .chain([last])
            .collect();
        let text = lines.join(end);
        if last_end { text + end } else { text }
    }

    /// The first rows of the diff between a file of
    /// [`lines_past_the_limit`] with `\r\n`, cut between the `\r` and the
    /// `\n` of its last line end, and the lines it keeps, given inline with
    /// `new_end`.
    fn cut_inside_a_crlf_against(new_end: &str) -> Vec<String> {
        let old_text = lines_past_the_limit("\r\n", true);
        assert_eq!(old_text.len() as u64, MAX_BYTES + 1);
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let old = directory.path().join("old.txt");
        fs::write(&old, old_text).expect("old file is writable");
        let new = DiffSide {
            path: None,
            content: Some(lines_past_the_limit(new_end, false)),
        };
        let mut viewer = from_files(file(old.to_str().expect("a UTF-8 path")), Some(new));

        let buf = drawn(&mut viewer, 60, 4, Offset::default());

        (0..4).map(|y| row(&buf, y)).collect()
    }

    #[test]
    fn a_side_cut_inside_a_crlf_names_its_line_ends_crlf() {
        assert_eq!(
            cut_inside_a_crlf_against("\n"),
            [
                "first 8 MiB shown",
                "no changes",
                "line endings differ: old CRLF, new LF",
                "… the rest is cut after 8 MiB",
            ]
        );
    }

    #[test]
    fn a_side_cut_inside_a_crlf_has_the_line_ends_of_a_crlf_side() {
        assert_eq!(
            cut_inside_a_crlf_against("\r\n"),
            [
                "first 8 MiB shown",
                "no changes",
                "… the rest is cut after 8 MiB",
                "",
            ]
        );
    }

    #[test]
    fn a_directory_is_not_read_as_a_side() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().to_str().expect("a UTF-8 path");

        let mut viewer = from_files(None, file(path));

        let buf = drawn(&mut viewer, 60, 8, Offset::default());
        assert!(
            row(&buf, 2).contains("│ ✗ new side: not a regular file "),
            "{buf:?}"
        );
    }

    #[test]
    fn a_relative_path_is_never_read() {
        let mut viewer = from_files(None, file("Cargo.toml"));

        let buf = drawn(&mut viewer, 40, 7, Offset::default());

        assert!(
            row(&buf, 2).contains("│ ✗ new side: relative path │"),
            "{buf:?}"
        );
        assert!(row(&buf, 4).contains("│ Cargo.toml "), "{buf:?}");
    }

    #[test]
    fn a_side_with_neither_path_nor_content_counts_as_empty() {
        let empty = Some(DiffSide {
            path: None,
            content: None,
        });

        let mut viewer = from_files(
            empty,
            Some(DiffSide {
                path: None,
                content: Some("x\n".into()),
            }),
        );

        let buf = drawn(&mut viewer, 20, 1, Offset::default());
        assert_eq!(row(&buf, 0), "  1 + x", "added to nothing, not a new file");
    }

    #[test]
    fn a_binary_file_is_not_shown_as_text() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let binary = directory.path().join("logo.png");
        fs::write(&binary, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").expect("file is writable");
        let path = binary.to_str().expect("a UTF-8 path");

        let mut viewer = from_files(
            Some(DiffSide {
                path: None,
                content: Some("x".into()),
            }),
            file(path),
        );

        let buf = drawn(&mut viewer, 80, 8, Offset::default());
        let rows: Vec<String> = (0..8).map(|y| row(&buf, y)).collect();
        assert!(
            rows.iter()
                .any(|row| row.contains("│ ✗ new side: binary file ")),
            "{rows:#?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("│ logo.png ")),
            "{rows:#?}"
        );
    }

    // -----------------------------------------------------------------
    // Side by side
    // -----------------------------------------------------------------

    /// `viewer` laid out side by side for `width`×`height` cells and drawn
    /// into them from `offset` on, as the pane draws a diff switched to
    /// side by side.
    fn drawn_side_by_side(
        viewer: &mut Content<Rows>,
        width: u16,
        height: u16,
        offset: Offset,
    ) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        viewer.lay_out_side_by_side(width, height);
        viewer.draw(area, &mut buf, offset);
        buf
    }

    /// Every row of `viewer` side by side, `width` columns wide, each in
    /// quotes so its width shows.
    fn shown_side_by_side(viewer: &mut Content<Rows>, width: u16) -> String {
        let lines = viewer.lay_out_side_by_side(width, 1).lines;
        let height = u16::try_from(lines).expect("a test's content is short");
        rows(&drawn_side_by_side(
            viewer,
            width,
            height,
            Offset::default(),
        ))
        .iter()
        .map(|row| format!("\"{row}\""))
        .collect::<Vec<_>>()
        .join("\n")
    }

    /// Where the divider stands in a body as wide as `buf`.
    fn divider_at(buf: &Buffer) -> u16 {
        (buf.area.width - 2) / 2
    }

    /// Row `y` of `buf` split at the divider: the old column and the new
    /// one, each without the blanks after it.
    fn halves(buf: &Buffer, y: u16) -> (String, String) {
        let divider = divider_at(buf);
        let text = |columns: std::ops::Range<u16>| {
            columns
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        };
        (text(0..divider), text(divider + 2..buf.area.width))
    }

    fn pair(old: &str, new: &str) -> (String, String) {
        (old.to_owned(), new.to_owned())
    }

    #[test]
    fn side_by_side_shows_old_left_and_new_right() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));

        let buf = drawn_side_by_side(&mut viewer, 100, 15, Offset::default());

        assert_eq!(
            halves(&buf, 1),
            pair(" 3   pub struct Fetcher {", " 3   pub struct Fetcher {")
        );
        assert_eq!(buf[(divider_at(&buf), 1)].symbol(), "│");
        insta::assert_snapshot!(
            "two_hunks_side_by_side",
            shown_side_by_side(&mut viewer, 100)
        );
    }

    #[test]
    fn a_longer_run_faces_blank_rows() {
        let mut viewer = inline(Some("x\na\ny\n"), Some("x\nb\nc\nd\ny\n"));

        let buf = drawn_side_by_side(&mut viewer, 30, 5, Offset::default());

        assert_eq!(halves(&buf, 1), pair("2 - a", "2 + b"));
        assert_eq!(halves(&buf, 2), pair("", "3 + c"));
        assert_eq!(halves(&buf, 3), pair("", "4 + d"));
        assert_eq!(halves(&buf, 4), pair("3   y", "5   y"));
        assert!(
            (0..divider_at(&buf)).all(|x| buf[(x, 2)].modifier.is_empty()),
            "a blank row has no fill"
        );
    }

    #[test]
    fn a_changed_half_is_filled_up_to_the_divider() {
        let mut viewer = inline(Some("a\nb\n"), Some("a\nc\n"));

        let buf = drawn_side_by_side(&mut viewer, 30, 2, Offset::default());

        assert_eq!(halves(&buf, 1), pair("2 - b", "2 + c"));
        let divider = divider_at(&buf);
        for x in 0..divider {
            assert_eq!(buf[(x, 1)].modifier, Modifier::REVERSED, "column {x}");
            assert_eq!(buf[(x, 1)].fg, Color::Red, "column {x}");
        }
        assert_eq!(buf[(divider, 1)].symbol(), "│");
        assert_eq!(buf[(divider, 1)].modifier, Modifier::DIM, "the divider");
        assert_eq!(
            buf[(divider + 1, 1)].modifier,
            Modifier::empty(),
            "its blank"
        );
        for x in divider + 2..30 {
            assert_eq!(buf[(x, 1)].modifier, Modifier::REVERSED, "column {x}");
            assert_eq!(buf[(x, 1)].fg, Color::Green, "column {x}");
        }
    }

    #[test]
    fn an_unchanged_line_is_highlighted_on_both_sides() {
        let mut viewer = rust_change(serde_json::json!({"name": "fetch.rs"}));

        let buf = drawn_side_by_side(&mut viewer, 40, 4, Offset::default());

        assert_eq!(halves(&buf, 0), pair("1   fn a() {}", "1   fn a() {}"));
        assert_eq!(buf[(4, 0)].fg, Color::Indexed(5), "the old side's keyword");
        assert_eq!(buf[(25, 0)].fg, Color::Indexed(5), "the new side's keyword");
    }

    #[test]
    fn the_separator_spans_both_columns() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));

        let buf = drawn_side_by_side(&mut viewer, 100, 15, Offset::default());

        assert_eq!(buf[(divider_at(&buf), 6)].symbol(), "│", "the row above");
        assert_eq!(row(&buf, 7), "┈".repeat(100));
    }

    #[test]
    fn a_file_note_spans_both_columns() {
        let mut viewer = inline(None, Some("fn main() {\n}\n"));

        let buf = drawn_side_by_side(&mut viewer, 32, 3, Offset::default());

        assert_eq!(row(&buf, 0), "new file");
        assert_eq!(halves(&buf, 1), pair("", "1 + fn main() {"));
    }

    #[test]
    fn the_cut_notes_span_both_columns() {
        let mut viewer = of_rows("a\n", "b\n", [Cut::Bytes]);

        let buf = drawn_side_by_side(&mut viewer, 40, 3, Offset::default());

        assert_eq!(row(&buf, 0), "first 8 MiB shown");
        assert_eq!(halves(&buf, 1), pair("1 - a", "1 + b"));
        assert_eq!(row(&buf, 2), "… the rest is cut after 8 MiB");
    }

    #[test]
    fn a_missing_newline_note_stands_in_its_sides_column() {
        let mut viewer = inline(Some("a\nb"), Some("a\nb\n"));

        let buf = drawn_side_by_side(&mut viewer, 60, 3, Offset::default());

        assert_eq!(halves(&buf, 1), pair("2 - b", "2 + b"));
        assert_eq!(halves(&buf, 2), pair("no newline at end of file", ""));
        assert_eq!(buf[(divider_at(&buf), 2)].symbol(), "│");
    }

    #[test]
    fn a_missing_newline_note_faces_a_blank_before_the_rest() {
        let mut viewer = inline(Some("a\nb"), Some("a\nc\nd\n"));

        let buf = drawn_side_by_side(&mut viewer, 60, 4, Offset::default());

        assert_eq!(halves(&buf, 1), pair("2 - b", "2 + c"));
        assert_eq!(halves(&buf, 2), pair("no newline at end of file", ""));
        assert_eq!(halves(&buf, 3), pair("", "3 + d"));
    }

    #[test]
    fn a_missing_newline_note_on_the_new_side_starts_its_column() {
        let mut viewer = inline(Some("a\nb\n"), Some("a\nc"));

        let buf = drawn_side_by_side(&mut viewer, 60, 3, Offset::default());

        assert_eq!(halves(&buf, 2), pair("", "no newline at end of file"));
        assert_eq!(buf[(divider_at(&buf) + 2, 2)].symbol(), "n");
    }

    #[test]
    fn side_by_side_fits_a_short_diff_at_its_minimum_and_not_below() {
        let mut viewer = inline(Some("a\n"), Some("b\n"));

        // One digit: two columns of a number, a blank, the sign, a blank
        // and 8 columns of text, and the divider with its blank.
        let buf = drawn_side_by_side(&mut viewer, 26, 1, Offset::default());
        assert_eq!(halves(&buf, 0), pair("1 - a", "1 + b"));

        let narrow = drawn_side_by_side(&mut viewer, 25, 2, Offset::default());
        let unified = drawn(&mut viewer, 25, 2, Offset::default());
        assert_eq!(narrow, unified, "unified below the minimum");
    }

    #[test]
    fn side_by_side_fits_a_long_diff_at_its_minimum_and_not_below() {
        let filler = "x\n".repeat(10_000);
        let mut viewer = inline(Some(&format!("{filler}a\n")), Some(&format!("{filler}b\n")));

        // Five digits: 2 × (5 + 3 + 8) + 2.
        let buf = drawn_side_by_side(&mut viewer, 34, 4, Offset::default());
        assert_eq!(halves(&buf, 3), pair("10001 - a", "10001 + b"));

        let narrow = drawn_side_by_side(&mut viewer, 33, 4, Offset::default());
        let unified = drawn(&mut viewer, 33, 4, Offset::default());
        assert_eq!(narrow, unified, "unified below the minimum");
    }

    #[test]
    fn side_by_side_counts_its_pair_rows() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));

        // The unified 16 rows less the removed line, which stands beside
        // its replacement.
        assert_eq!(viewer.lay_out_side_by_side(100, 10).lines, 15);
    }

    #[test]
    fn side_by_side_reports_the_overflow_of_the_widest_side() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));

        let size = viewer.lay_out_side_by_side(100, 10);

        // The old column is 49 wide, 44 of them for text behind its gutter
        // of `NN - `; its widest line is a context line.
        let widest = "    pub fn fetch(&self, url: &str) -> Result<Response> {".len();
        assert_eq!(size.columns, 100 + widest - 44);
    }

    /// A long line changed into another long line.
    fn long_change() -> Content<Rows> {
        let line = |text: &str| format!("let message = \"{}\";\n", text.repeat(4));
        inline(Some(&line("an old text ")), Some(&line("a new text ")))
    }

    #[test]
    fn both_texts_scroll_together_under_gutters_that_stay() {
        let mut viewer = long_change();

        let buf = drawn_side_by_side(&mut viewer, 60, 2, Offset { top: 0, left: 8 });

        let (old, new) = halves(&buf, 0);
        assert!(old.starts_with("1 - ‹ge = \"an old"), "{old:?}");
        assert!(new.starts_with("1 + ‹ge = \"a new"), "{new:?}");
    }

    #[test]
    fn each_column_marks_a_hidden_start() {
        let mut viewer = long_change();

        let buf = drawn_side_by_side(&mut viewer, 60, 2, Offset { top: 0, left: 8 });

        let divider = divider_at(&buf);
        for x in [4, divider + 2 + 4] {
            assert_eq!(buf[(x, 0)].symbol(), "‹", "column {x}");
        }
    }

    #[test]
    fn each_column_marks_a_cut_end() {
        let mut viewer = long_change();

        let buf = drawn_side_by_side(&mut viewer, 60, 2, Offset::default());

        let divider = divider_at(&buf);
        for x in [divider - 1, 59] {
            assert_eq!(buf[(x, 0)].symbol(), "›", "column {x}");
        }
        assert_eq!(buf[(divider, 0)].symbol(), "│");
    }

    #[test]
    fn a_blank_keeps_the_new_columns_numbers_off_the_divider() {
        let new = new_fetcher();
        let mut viewer = inline(Some(OLD_FETCHER), Some(&new));

        let buf = drawn_side_by_side(&mut viewer, 100, 15, Offset::default());

        // The first row after the separator, the first with numbers of
        // two digits on both sides.
        assert!(row(&buf, 8).contains("│ 11       }"), "{:?}", row(&buf, 8));
    }

    #[test]
    fn an_even_and_an_odd_body_split_around_the_divider() {
        let mut viewer = inline(Some("a\n"), Some("b\n"));

        for (width, divider, new) in [(30, 14, 16), (31, 14, 16)] {
            let buf = drawn_side_by_side(&mut viewer, width, 1, Offset::default());

            assert_eq!(buf[(divider, 0)].symbol(), "│", "{width} columns");
            assert_eq!(buf[(new, 0)].symbol(), "1", "{width} columns");
        }
    }

    #[test]
    fn a_wide_character_at_the_old_columns_edge_shows_as_blanks() {
        let mut viewer = inline(Some("abcdefghi世\n"), Some("x\n"));

        let buf = drawn_side_by_side(&mut viewer, 30, 1, Offset::default());

        assert_eq!(halves(&buf, 0).0, "1 - abcdefghi›");
        assert_eq!(buf[(14, 0)].symbol(), "│");
    }

    #[test]
    fn a_new_file_side_by_side_leaves_the_old_column_blank() {
        let mut viewer = inline(None, Some("a\nb\n"));

        let buf = drawn_side_by_side(&mut viewer, 30, 3, Offset::default());

        for y in 1..3 {
            assert_eq!(halves(&buf, y).0, "", "row {y}");
            assert!(
                (0..divider_at(&buf)).all(|x| buf[(x, y)].modifier.is_empty()),
                "row {y} has no fill on the old side"
            );
        }
        assert_eq!(halves(&buf, 2).1, "2 + b");
    }

    /// A line of the diff with `tag`, for hand-built rows.
    fn line_of(tag: ChangeTag) -> LineRow {
        LineRow {
            tag,
            old: None,
            new: None,
            text: SourceLine::new([("x", Style::new())]),
        }
    }

    #[test]
    fn pairs_of_pairs_a_run_whatever_its_order() {
        // An added line before a removed one, and the note of the removed
        // one after them.
        let lines = [line_of(ChangeTag::Insert), line_of(ChangeTag::Delete)];
        let rows = [Row::Line(0), Row::Line(1), Row::NoNewline];

        let side_by_side = pairs_of(&rows, &lines);

        assert_eq!(
            side_by_side.pairs,
            [
                Pair::Sides {
                    old: Some(Half::Line(1)),
                    new: Some(Half::Line(0)),
                },
                Pair::Sides {
                    old: Some(Half::NoNewline),
                    new: None,
                },
            ]
        );
        assert_eq!(side_by_side.pair_of_row, [0, 0, 1]);
        assert_eq!(side_by_side.first_row_of_pair, [0, 2]);
    }

    #[test]
    fn a_top_maps_from_unified_to_side_by_side() {
        let new = new_fetcher();
        let mut fetcher = inline(Some(OLD_FETCHER), Some(&new));
        fetcher.lay_out_side_by_side(100, 10);
        let mut note = inline(Some("a\nb"), Some("a\nc\nd\n"));
        note.lay_out_side_by_side(60, 10);

        // The added field, the separator, the removed call, its
        // replacement and the line after them.
        for (unified, paired) in [(3, 3), (7, 7), (11, 11), (12, 11), (13, 12)] {
            assert_eq!(fetcher.top_from(unified, DiffLayout::Unified), paired);
        }
        // The note, the line beside the removed one and the one after.
        for (unified, paired) in [(2, 2), (3, 1), (4, 3)] {
            assert_eq!(note.top_from(unified, DiffLayout::Unified), paired);
        }
        assert_eq!(note.top_from(99, DiffLayout::Unified), 3, "past the end");
    }

    #[test]
    fn a_top_maps_from_side_by_side_to_unified() {
        let mut note = inline(Some("a\nb"), Some("a\nc\nd\n"));
        note.lay_out(60, 10);

        // The removed line beside its replacement, the note, and the added
        // line beside a blank.
        for (paired, unified) in [(1, 1), (2, 2), (3, 4)] {
            assert_eq!(note.top_from(paired, DiffLayout::SideBySide), unified);
        }
        assert_eq!(note.top_from(99, DiffLayout::SideBySide), 4, "past the end");
        assert_eq!(note.top_from(3, DiffLayout::Unified), 3, "the same layout");
    }

    #[test]
    fn diff_shape_says_the_layout_used_and_whether_it_fits() {
        let mut viewer = inline(Some("a\n"), Some("b\n"));
        let shape = |layout, fits_side_by_side| {
            Some(DiffShape {
                layout,
                fits_side_by_side,
            })
        };

        viewer.lay_out(30, 5);
        assert_eq!(viewer.diff_shape(), shape(DiffLayout::Unified, true));
        viewer.lay_out_side_by_side(30, 5);
        assert_eq!(viewer.diff_shape(), shape(DiffLayout::SideBySide, true));
        viewer.lay_out_side_by_side(25, 5);
        assert_eq!(viewer.diff_shape(), shape(DiffLayout::Unified, false));
    }
}
