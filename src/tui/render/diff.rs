// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewer of diff entries (spec section 7.7, ADR 33): the change from
//! the old to the new text as a unified diff, which asqr computes itself.

use std::borrow::Cow;
use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use similar::{Algorithm, Change, ChangeTag, TextDiff};

use super::source_file::{Source, Unreadable, between_cut_notes, read_source};
use super::source_line::{GutterRow, Note, SourceLine, rows_from};
use super::theme;
use super::viewer::{Content, ContentSize, Viewer};
use crate::format::{DiffEntry, DiffSide};
use crate::graphics::PlacedImage;
use crate::tui::Offset;

/// Unchanged lines shown before and after each change, as `diff -u` does.
const CONTEXT_LINES: usize = 3;

/// How long computing a diff may take. The viewer is built while the
/// person waits for the screen, so past this, similar settles for a diff that is correct
/// but larger than it needs to be.
const DIFF_TIME: Duration = Duration::from_secs(1);

/// What fills a separator line between two hunks.
const SEPARATOR: &str = "┈";

/// A diff entry as a unified diff: hunks with three lines of context, both
/// line numbers in a dim gutter, removed lines red and added ones green.
pub struct DiffViewer {
    /// The diff's rows, or why it cannot show.
    content: Content<Rows>,
}

impl DiffViewer {
    /// Reads both sides and computes the whole diff, so drawing only
    /// copies cells. The home folder `home` shows as `~` in the path of a
    /// side that cannot be read.
    pub fn new(entry: &DiffEntry, home: Option<&Path>) -> Self {
        let sides = read("old", entry.old.as_ref())
            .and_then(|old| Ok((old, read("new", entry.new.as_ref())?)));
        let content = match sides {
            Ok((old, new)) => {
                let cut = [&old, &new].into_iter().flatten().any(|side| side.cut);
                Content::Body(Rows::of(text_of(old.as_ref()), text_of(new.as_ref()), cut))
            }
            Err((side, unreadable)) => Content::Problem(unreadable.card_on_side(side, home)),
        };
        DiffViewer { content }
    }
}

impl Viewer for DiffViewer {
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        self.content.lay_out(width, height)
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        self.content.draw(area, buf, offset)
    }

    fn failed(&self) -> bool {
        self.content.failed()
    }
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
    side.map(|side| read_source(side.path.as_deref(), side.content.as_deref()))
        .transpose()
        .map_err(|unreadable| (name, unreadable))
}

fn text_of(side: Option<&Source>) -> Option<&str> {
    side.map(|side| side.text.as_str())
}

// ---------------------------------------------------------------------
// The diff's rows
// ---------------------------------------------------------------------

/// The rows of a diff, and how many digits its line numbers take.
struct Rows {
    rows: Vec<Row>,
    digits: usize,
}

enum Row {
    /// A line of either side, or of both for an unchanged one.
    Line {
        tag: ChangeTag,
        old: Option<usize>,
        new: Option<usize>,
        text: SourceLine,
    },
    /// The gap between two hunks.
    Separator,
    /// Something to know about the diff that is no line of it.
    Note(Note),
}

impl Rows {
    /// The rows that show the change from `old` to `new`; a side left out
    /// is a file that did not exist before or no longer does. `cut` says
    /// that a side is only the start of a longer file, so the diff shows
    /// the rest of the other side as removed or added. Whether the file is
    /// new or deleted comes first, then the change between the notes of a
    /// cut file.
    fn of(old: Option<&str>, new: Option<&str>, cut: bool) -> Rows {
        let created_or_deleted = match (old, new) {
            (None, Some(_)) => Some(Note::new("new file")),
            (Some(_), None) => Some(Note::new("deleted file")),
            _ => None,
        };
        let (change, digits) = Rows::change(old, new, created_or_deleted.is_some());
        let rows = created_or_deleted
            .map(Row::Note)
            .into_iter()
            .chain(between_cut_notes(cut, change, Row::Note))
            .collect();
        Rows { rows, digits }
    }

    /// The rows of the change from `old` to `new`, and how many digits
    /// their line numbers take. Text alike on both sides of a new or
    /// deleted file is an empty file.
    fn change(old: Option<&str>, new: Option<&str>, created_or_deleted: bool) -> (Vec<Row>, usize) {
        let mut rows = Vec::new();
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
            return (rows, 0);
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

        for (index, hunk) in diff.grouped_ops(CONTEXT_LINES).iter().enumerate() {
            if index > 0 {
                rows.push(Row::Separator);
            }
            for op in hunk {
                for change in diff.iter_changes(op) {
                    rows.push(line_row(&change));
                    if newline_differs && change.missing_newline() {
                        rows.push(Row::Note(Note::new("no newline at end of file")));
                    }
                }
            }
        }
        let longest = diff.old_len().max(diff.new_len());
        (rows, longest.to_string().len())
    }

    /// The columns before a line's text: both numbers, each followed by a
    /// blank, then the sign and a blank.
    fn gutter_width(&self) -> usize {
        2 * (self.digits + 1) + 2
    }
}

impl Viewer for Rows {
    /// Lines never wrap, so only notes, which never scroll, depend on the
    /// width.
    fn lay_out(&mut self, width: u16, _height: u16) -> ContentSize {
        let columns = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Line { text, .. } => self.gutter_width() + text.width(),
                // The separator fills whatever width the body has.
                Row::Separator => 0,
                Row::Note(note) => note.columns(usize::from(width)),
            })
            .max()
            .unwrap_or(0);
        ContentSize {
            lines: self.rows.len(),
            columns,
        }
    }

    /// Draws the rows from `offset.top` on, each line with its numbers in a
    /// dim gutter and its sign after them.
    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        let width = usize::from(area.width);
        let digits = self.digits;
        for (row_area, row) in rows_from(area, &self.rows, offset.top) {
            match row {
                Row::Separator => {
                    let separator = SEPARATOR.repeat(width);
                    buf.set_stringn(row_area.x, row_area.y, separator, width, theme::secondary());
                }
                Row::Note(note) => note.render(row_area, buf),
                Row::Line {
                    tag,
                    old,
                    new,
                    text,
                } => {
                    let number = |number: &Option<usize>| {
                        number.map_or_else(String::new, |number| number.to_string())
                    };
                    let numbers = format!("{:>digits$} {:>digits$} ", number(old), number(new));
                    let sign = match tag {
                        ChangeTag::Equal => " ",
                        ChangeTag::Delete => "-",
                        ChangeTag::Insert => "+",
                    };
                    let gutter = Line::from(vec![
                        Span::styled(numbers, theme::secondary()),
                        Span::styled(sign, line_style(*tag)),
                        Span::from(" "),
                    ]);
                    GutterRow {
                        gutter,
                        line: text,
                        left: offset.left,
                    }
                    .render(row_area, buf);
                }
            }
        }
        None
    }
}

/// The row of one line of the diff, numbered from 1 on each side it is on.
/// The whole line takes its colour, with nothing marked inside it (ADR 33).
fn line_row(change: &Change<&str>) -> Row {
    let value = change.value();
    let text = value.strip_suffix('\n').unwrap_or(value);
    Row::Line {
        tag: change.tag(),
        old: change.old_index().map(|index| index + 1),
        new: change.new_index().map(|index| index + 1),
        text: SourceLine::new([(text, line_style(change.tag()))]),
    }
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
/// so its theme decides the exact colours.
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

    use super::super::test_support::{drawn, row, shown};
    use super::*;

    /// The viewer of the change from `old` to `new`, given inline.
    fn inline(old: Option<&str>, new: Option<&str>) -> DiffViewer {
        let side = |text: Option<&str>| {
            text.map(|text| DiffSide {
                path: None,
                content: Some(text.to_owned()),
            })
        };
        DiffViewer::new(
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

    /// The viewer of the rows of `old` and `new`, where `cut` says a side
    /// is only the start of a longer file.
    fn of_rows(old: &str, new: &str, cut: bool) -> DiffViewer {
        DiffViewer {
            content: Content::Body(Rows::of(Some(old), Some(new), cut)),
        }
    }

    fn file(path: &str) -> Option<DiffSide> {
        Some(DiffSide {
            path: Some(path.to_owned()),
            content: None,
        })
    }

    fn from_files(old: Option<DiffSide>, new: Option<DiffSide>) -> DiffViewer {
        DiffViewer::new(
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
        for y in 0..4 {
            assert_eq!(with_modifier(&buf, y, Modifier::REVERSED), "", "row {y}");
        }
        let colour = |y: u16| -> Vec<Color> {
            let text = row(&buf, y);
            (6..u16::try_from(text.chars().count()).expect("a short row"))
                .map(|x| buf[(x, y)].fg)
                .collect()
        };
        for (y, line) in [
            (0, Color::Red),
            (1, Color::Red),
            (2, Color::Green),
            (3, Color::Green),
        ] {
            assert!(colour(y).iter().all(|&fg| fg == line), "row {y}");
        }
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
        let mut viewer = of_rows("a\n", "b\n", true);

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

        let buf = drawn(&mut viewer, 3, 1, Offset::default());
        assert_eq!(row(&buf, 0), "1", "the numbers alone");
        assert_eq!(with_modifier(&buf, 0, Modifier::DIM), "1  ");
    }

    #[test]
    fn sides_cut_alike_say_so_after_no_changes() {
        let mut viewer = of_rows("a\n", "a\n", true);

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 1), "no changes");
        assert_eq!(row(&buf, 2), "… the rest is cut after 8 MiB");
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
}
