// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewer of diff entries (spec section 7.7, ADR 33): the change from
//! the old to the new text as a unified diff, which asqr computes itself.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use similar::{Algorithm, ChangeTag, InlineChange, InlineChangeOptions, TextDiff};

use super::source_file::{CUT_NOTE, END_NOTE, Source, Unreadable, read_source};
use super::source_line::{SourceLine, columns};
use super::viewer::{ContentSize, Placeholder, Viewer};
use crate::format::{DiffEntry, DiffSide};
use crate::tui::Offset;

/// Unchanged lines shown before and after each change, as `diff -u` does.
const CONTEXT_LINES: usize = 3;

/// How long computing a diff may take. `load` runs while the person waits
/// for the screen, so past this, similar settles for a diff that is correct
/// but larger than it needs to be.
const DIFF_TIME: Duration = Duration::from_secs(1);

/// What fills a separator line between two hunks.
const SEPARATOR: &str = "┈";

/// A diff entry as a unified diff: hunks with three lines of context, both
/// line numbers in a dim gutter, removed lines red and added ones green,
/// with the changed words of a changed line reversed on top.
pub struct DiffViewer {
    old: Option<DiffSide>,
    new: Option<DiffSide>,
    content: Option<Content>,
}

/// What `load` made of the entry: the diff's rows, or why it cannot show.
enum Content {
    Rows(Rows),
    Unreadable(Placeholder),
}

impl DiffViewer {
    pub fn new(entry: &DiffEntry) -> Self {
        DiffViewer {
            old: entry.old.clone(),
            new: entry.new.clone(),
            content: None,
        }
    }

    fn content(&mut self) -> &mut Content {
        self.content
            .as_mut()
            .expect("the pane loads a viewer before laying it out")
    }
}

impl Viewer for DiffViewer {
    /// Reads both sides and computes the whole diff, so drawing only
    /// copies cells.
    fn load(&mut self) {
        let sides = read(self.old.as_ref()).and_then(|old| Ok((old, read(self.new.as_ref())?)));
        self.content = Some(match sides {
            Ok((old, new)) => {
                let cut = [&old, &new].into_iter().flatten().any(|side| side.cut);
                Content::Rows(Rows::of(text_of(old.as_ref()), text_of(new.as_ref()), cut))
            }
            Err(unreadable) => Content::Unreadable(unreadable.placeholder("diff not shown")),
        });
    }

    /// Lines never wrap, so only notes, which never scroll, depend on the
    /// width.
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        match self.content() {
            Content::Unreadable(placeholder) => placeholder.lay_out(width, height),
            Content::Rows(rows) => rows.size(usize::from(width)),
        }
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) {
        match self.content() {
            Content::Unreadable(placeholder) => placeholder.draw(area, buf, offset),
            Content::Rows(rows) => rows.draw(area, buf, offset),
        }
    }
}

// ---------------------------------------------------------------------
// Reading the sides
// ---------------------------------------------------------------------

/// The text of a side, or `None` for a side the entry leaves out.
fn read(side: Option<&DiffSide>) -> Result<Option<Source>, Unreadable> {
    side.map(|side| read_source(side.path.as_deref(), side.content.as_deref()))
        .transpose()
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
    Note(&'static str),
}

impl Rows {
    /// The rows that show the change from `old` to `new`; a side left out
    /// is a file that did not exist before or no longer does. `cut` says
    /// that a side is only the start of a longer file, so the diff shows
    /// the rest of the other side as removed or added.
    fn of(old: Option<&str>, new: Option<&str>, cut: bool) -> Rows {
        let mut rows = Vec::new();
        let created_or_deleted = match (old, new) {
            (None, Some(_)) => Some("new file"),
            (Some(_), None) => Some("deleted file"),
            _ => None,
        };
        rows.extend(created_or_deleted.map(Row::Note));
        if cut {
            rows.push(Row::Note(CUT_NOTE));
        }
        // Lines compare without their line ends, so a file saved on
        // Windows does not show every line as removed and added again with
        // the same text. One note says the ends differ, as nothing else
        // would show it.
        let endings_differ = match (old, new) {
            (Some(old), Some(new)) => {
                let (old, new) = (LineEnds::of(old), LineEnds::of(new));
                old != new && old != LineEnds::NONE && new != LineEnds::NONE
            }
            _ => false,
        };
        let (old, new) = (
            unix_line_ends(old.unwrap_or_default()),
            unix_line_ends(new.unwrap_or_default()),
        );
        let (old, new) = (old.as_ref(), new.as_ref());
        if old == new {
            rows.push(Row::Note(if created_or_deleted.is_some() {
                "empty file"
            } else {
                "no changes"
            }));
            if endings_differ {
                rows.push(Row::Note(ENDINGS_DIFFER));
            }
            if cut {
                rows.push(Row::Note(END_NOTE));
            }
            return Rows { rows, digits: 0 };
        }
        if endings_differ {
            rows.push(Row::Note(ENDINGS_DIFFER));
        }

        let deadline = Instant::now() + DIFF_TIME;
        // Histogram, git's algorithm of that name, anchors the diff on
        // lines that occur rarely, such as a function's signature, rather
        // than on blank lines and lone braces.
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Histogram)
            .deadline(deadline)
            .diff_lines(old, new);
        let mut words = InlineChangeOptions::new();
        words.semantic_cleanup(true);
        // A last line without a newline differs from the same line with
        // one. Only where the two sides differ in that does a note say so,
        // or `- two` and `+ two` would look the same.
        let newline_differs = old.ends_with('\n') != new.ends_with('\n');

        for (index, hunk) in diff.grouped_ops(CONTEXT_LINES).iter().enumerate() {
            if index > 0 {
                rows.push(Row::Separator);
            }
            for op in hunk {
                for change in
                    diff.iter_inline_changes_with_options_deadline(op, words, Some(deadline))
                {
                    rows.push(line_row(&change));
                    if newline_differs && change.missing_newline() {
                        rows.push(Row::Note("no newline at end of file"));
                    }
                }
            }
        }
        // The last hunk ends where the cut side stops, which is not where
        // its file ends.
        if cut {
            rows.push(Row::Note(END_NOTE));
        }
        let longest = diff.old_len().max(diff.new_len());
        Rows {
            rows,
            digits: longest.to_string().len(),
        }
    }

    /// The columns before a line's text: both numbers, each followed by a
    /// blank, then the sign and a blank.
    fn gutter_width(&self) -> usize {
        2 * (self.digits + 1) + 2
    }

    fn size(&self, width: usize) -> ContentSize {
        let columns = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Line { text, .. } => self.gutter_width() + text.width(),
                // The separator fills whatever width the body has.
                Row::Separator => 0,
                Row::Note(note) => columns(note).min(width),
            })
            .max()
            .unwrap_or(0);
        ContentSize {
            lines: self.rows.len(),
            columns,
        }
    }

    /// Draws the rows from `offset.top` on. The gutter and the sign stay in
    /// place while the text scrolls sideways under `offset.left`, so every
    /// line keeps its numbers in sight.
    fn draw(&self, area: Rect, buf: &mut Buffer, offset: Offset) {
        let dim = Style::new().dim();
        let width = usize::from(area.width);
        for (y, row) in (area.top()..area.bottom()).zip(self.rows.iter().skip(offset.top)) {
            match row {
                Row::Separator => {
                    buf.set_stringn(area.x, y, SEPARATOR.repeat(width), width, dim);
                }
                Row::Note(note) => {
                    buf.set_stringn(area.x, y, note, width, dim);
                }
                Row::Line {
                    tag,
                    old,
                    new,
                    text,
                } => {
                    let number = |number: &Option<usize>| {
                        number.map_or_else(String::new, |number| number.to_string())
                    };
                    let digits = self.digits;
                    let gutter = format!("{:>digits$} {:>digits$} ", number(old), number(new));
                    let (x, _) = buf.set_stringn(area.x, y, gutter, width, dim);
                    let sign = match tag {
                        ChangeTag::Equal => " ",
                        ChangeTag::Delete => "-",
                        ChangeTag::Insert => "+",
                    };
                    let room = width.saturating_sub(usize::from(x - area.x));
                    buf.set_stringn(x, y, sign, room, line_style(*tag));
                    let fixed = u16::try_from(self.gutter_width()).unwrap_or(u16::MAX);
                    let text_area = Rect {
                        x: area.x.saturating_add(fixed).min(area.right()),
                        y,
                        width: area.width.saturating_sub(fixed),
                        height: 1,
                    };
                    text.draw(buf, text_area, offset.left);
                }
            }
        }
    }
}

/// The row of one line of the diff, numbered from 1 on each side it is on.
fn line_row(change: &InlineChange<'_, str>) -> Row {
    let style = line_style(change.tag());
    let emphasis = style.reversed();
    let values = change.values();
    let last = values.len().saturating_sub(1);
    let pieces = values
        .iter()
        .enumerate()
        .map(|(index, (emphasised, value))| {
            // The line break belongs to the last piece.
            let value = if index == last {
                value.strip_suffix('\n').unwrap_or(value)
            } else {
                value
            };
            (value, if *emphasised { emphasis } else { style })
        });
    Row::Line {
        tag: change.tag(),
        old: change.old_index().map(|index| index + 1),
        new: change.new_index().map(|index| index + 1),
        text: SourceLine::new(pieces),
    }
}

/// The note shown when the two sides end their lines differently.
const ENDINGS_DIFFER: &str = "line endings differ";

/// Which line ends a text uses: `\n` as on Unix, `\r\n` as on Windows, and
/// a lone `\r` as on classic Mac OS.
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
        ChangeTag::Delete => Style::new().fg(Color::Red),
        ChangeTag::Insert => Style::new().fg(Color::Green),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{drawn, row, shown};
    use super::*;

    /// A loaded viewer of the change from `old` to `new`, given inline.
    fn inline(old: Option<&str>, new: Option<&str>) -> DiffViewer {
        let side = |text: Option<&str>| {
            text.map(|text| DiffSide {
                path: None,
                content: Some(text.to_owned()),
            })
        };
        loaded(DiffEntry {
            title: None,
            old: side(old),
            new: side(new),
            name: None,
            ..DiffEntry::default()
        })
    }

    fn loaded(entry: DiffEntry) -> DiffViewer {
        let mut viewer = DiffViewer::new(&entry);
        viewer.load();
        viewer
    }

    fn file(path: &str) -> Option<DiffSide> {
        Some(DiffSide {
            path: Some(path.to_owned()),
            content: None,
        })
    }

    fn from_files(old: Option<DiffSide>, new: Option<DiffSide>) -> DiffViewer {
        loaded(DiffEntry {
            title: None,
            old,
            new,
            name: None,
            ..DiffEntry::default()
        })
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
    // Changed words
    // -----------------------------------------------------------------

    #[test]
    fn the_changed_words_of_a_changed_line_stand_out() {
        let mut viewer = inline(
            Some("let delay = 100;\nlet tries = 3;\n"),
            Some("let delay = 250;\nlet tries = 3;\n"),
        );

        insta::assert_snapshot!("changed_word", shown(&mut viewer, 30, 0));
        let buf = drawn(&mut viewer, 30, 3, Offset::default());
        assert_eq!(with_modifier(&buf, 0, Modifier::REVERSED), "100");
        assert_eq!(with_modifier(&buf, 1, Modifier::REVERSED), "250");
        assert_eq!(buf[(14, 0)].fg, Color::Red, "emphasis on top of the colour");
        assert_eq!(buf[(14, 1)].fg, Color::Green);
        assert_eq!(with_modifier(&buf, 2, Modifier::REVERSED), "", "context");
    }

    #[test]
    fn words_split_at_punctuation_so_code_shows_the_changed_name() {
        let mut viewer = inline(Some("get(url)\n"), Some("get(uri)\n"));

        let buf = drawn(&mut viewer, 30, 2, Offset::default());

        assert_eq!(with_modifier(&buf, 0, Modifier::REVERSED), "url");
        assert_eq!(with_modifier(&buf, 1, Modifier::REVERSED), "uri");
    }

    #[test]
    fn a_line_rewritten_entirely_has_no_emphasis() {
        let mut viewer = inline(Some("alpha beta\n"), Some("gamma delta epsilon\n"));

        let buf = drawn(&mut viewer, 30, 2, Offset::default());

        assert_eq!(with_modifier(&buf, 0, Modifier::REVERSED), "");
        assert_eq!(with_modifier(&buf, 1, Modifier::REVERSED), "");
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
        for old in ["a\r\nb\r\n", "a\rb\r", "a\r\nb\n"] {
            let mut viewer = inline(Some(old), Some("a\nb\n"));

            let buf = drawn(&mut viewer, 30, 2, Offset::default());

            assert_eq!(row(&buf, 0), "no changes", "{old:?}");
            assert_eq!(row(&buf, 1), "line endings differ", "{old:?}");
            assert_eq!(with_modifier(&buf, 1, Modifier::DIM), "line endings differ");
        }
    }

    #[test]
    fn a_change_between_line_ends_of_two_kinds_shows_as_one_note_above_the_hunks() {
        let mut viewer = inline(Some("a\r\nb\r\nc\r\n"), Some("a\nB\nc\n"));

        assert_eq!(
            shown(&mut viewer, 22, 0),
            [
                "\"line endings differ   \"",
                "\"1 1   a               \"",
                "\"2   - b               \"",
                "\"  2 + B               \"",
                "\"3 3   c               \"",
            ]
            .join("\n")
        );
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
        let buf = drawn(&mut viewer, 30, 4, Offset::default());
        assert_eq!(row(&buf, 0), "diff not shown");
        assert_eq!(row(&buf, 1), "/nowhere/old.rs", "the first side that fails");
    }

    #[test]
    fn a_side_too_long_to_read_whole_says_so_above_and_below_the_hunks() {
        let mut viewer = inline(None, None);
        viewer.content = Some(Content::Rows(Rows::of(Some("a\n"), Some("b\n"), true)));

        let buf = drawn(&mut viewer, 30, 4, Offset::default());

        assert_eq!(row(&buf, 0), "first 8 MiB shown");
        assert_eq!(with_modifier(&buf, 0, Modifier::DIM), "first 8 MiB shown");
        assert_eq!(row(&buf, 1), "1   - a");
        assert_eq!(row(&buf, 3), "… the rest is cut after 8 MiB");
        assert_eq!(
            with_modifier(&buf, 3, Modifier::DIM),
            "… the rest is cut after 8 MiB"
        );
    }

    #[test]
    fn sides_cut_alike_say_so_after_no_changes() {
        let rows = Rows::of(Some("a\n"), Some("a\n"), true);
        let mut viewer = inline(None, None);
        viewer.content = Some(Content::Rows(rows));

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 1), "no changes");
        assert_eq!(row(&buf, 2), "… the rest is cut after 8 MiB");
    }

    #[test]
    fn a_directory_is_not_read_as_a_side() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().to_str().expect("a UTF-8 path");

        let mut viewer = from_files(None, file(path));

        let buf = drawn(&mut viewer, 60, 3, Offset::default());
        assert_eq!(row(&buf, 0), "diff not shown");
        assert_eq!(row(&buf, 2), "not a regular file");
    }

    #[test]
    fn a_relative_path_is_never_read() {
        let mut viewer = from_files(None, file("Cargo.toml"));

        let buf = drawn(&mut viewer, 30, 3, Offset::default());

        assert_eq!(row(&buf, 1), "Cargo.toml");
        assert_eq!(row(&buf, 2), "relative path");
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

        let buf = drawn(&mut viewer, 80, 3, Offset::default());
        assert_eq!(row(&buf, 1), path);
        assert_eq!(row(&buf, 2), "binary file");
    }
}
