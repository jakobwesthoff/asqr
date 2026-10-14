// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A typed field (spec section 7.3): the own answer, the answer of a
//! `text` question, a question's note, the session note and the reject
//! reason. What every field does the same lives here: editing its text,
//! measuring it and drawing it. Where a field's text is kept, and what
//! `enter` and leaving the field mean, depend on the field and stay with
//! [`super::SessionState`].

use std::ops::Range;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui_textarea::{CursorMove, TextArea};
use unicode_segmentation::UnicodeSegmentation;

use super::render::{Glyph, columns, styled_line, wrap_rows};
use crate::format::Length;

/// The text of a field wraps under its own start while that leaves it at
/// least this much room; otherwise its further lines start at
/// `NARROW_INDENT` (ADR 27). Lower than the 30 columns option
/// descriptions need beside their label, so the reject reason, whose
/// head leaves 27 columns in a 60-column terminal, still wraps under its
/// start there.
const MIN_TEXT_WIDTH: usize = 20;

/// The indent of a field's further lines when its head leaves less than
/// `MIN_TEXT_WIDTH` columns.
const NARROW_INDENT: usize = 4;

// ---------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------

/// What a key did in a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    /// The text changed.
    Changed,
    /// The key moved the text cursor or did nothing.
    Unchanged,
    /// `enter`: the field's action.
    Enter,
    /// `esc`: stop typing.
    Escape,
    /// An arrow that leaves the field instead of moving the text cursor.
    Leave(Leave),
}

/// The arrow a field is left by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// `←` with the text cursor at the start of the text.
    Left,
    /// `→` with the text cursor at the end of the text.
    Right,
    Up,
    Down,
}

/// Where a field is drawn: the head its text follows and the width its
/// rows wrap at.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldShape {
    pub head: Span<'static>,
    pub width: usize,
}

/// The field that has the keys: its text with the text cursor.
#[derive(Debug, Clone)]
pub struct TextField {
    editor: TextArea<'static>,
    /// Where the field was last drawn, which `↑`/`↓` move through, as a
    /// key acts on what is on screen.
    shape: Option<FieldShape>,
    /// The column `↑`/`↓` aim for while they follow each other, so the
    /// cursor comes back to it after passing a shorter row.
    wanted: Option<usize>,
}

impl TextField {
    /// A field holding `text`, its text cursor at the end, so typing
    /// goes on where the person left off.
    pub fn new(text: &str) -> Self {
        let mut editor = TextArea::new(text.split('\n').map(str::to_owned).collect());
        editor.set_hard_tab_indent(true);
        editor.move_cursor(CursorMove::Bottom);
        editor.move_cursor(CursorMove::End);
        TextField {
            editor,
            shape: None,
            wanted: None,
        }
    }

    pub fn text(&self) -> String {
        self.editor.lines().join("\n")
    }

    /// The text cursor: its line, and its column in characters.
    pub fn cursor(&self) -> (usize, usize) {
        let cursor = self.editor.cursor();
        (cursor.0, cursor.1)
    }

    /// Characters in the text, a line break counting as one.
    pub fn length(&self) -> usize {
        let lines = self.editor.lines();
        lines.iter().map(|line| line.chars().count()).sum::<usize>() + lines.len().saturating_sub(1)
    }

    /// Takes where the field was drawn, for `↑`/`↓`.
    pub fn set_shape(&mut self, shape: FieldShape) {
        self.shape = Some(shape);
    }

    /// Applies `key`. `enter`, `esc` and the arrows that would leave the
    /// field are left to the caller, as their meaning depends on the
    /// field; every other key edits.
    ///
    /// `↑`/`↓` move the text cursor to the row above or below as the field
    /// shows it, and leave only from its first or last row (ADR 36). `←`
    /// leaves only at the start of the text and `→` only at its end; inside
    /// the text, and with a modifier held, they move the text cursor.
    pub fn key(&mut self, key: KeyEvent) -> FieldKey {
        let plain = key.modifiers.is_empty();
        let wanted = self.wanted.take();
        match key.code {
            KeyCode::Enter => FieldKey::Enter,
            KeyCode::Esc => FieldKey::Escape,
            KeyCode::Up => self.vertical(Leave::Up, wanted),
            KeyCode::Down => self.vertical(Leave::Down, wanted),
            KeyCode::Left if plain && self.cursor() == (0, 0) => FieldKey::Leave(Leave::Left),
            KeyCode::Right if plain && self.at_end() => FieldKey::Leave(Leave::Right),
            _ if self.input(key) => FieldKey::Changed,
            _ => FieldKey::Unchanged,
        }
    }

    /// Hands `key` to the editor as it is and says whether the text
    /// changed. `ctrl-j` adds a line, as `enter` is the field's action
    /// (ADR 36).
    fn input(&mut self, key: KeyEvent) -> bool {
        let before = self.editor.lines().to_vec();
        if key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.editor.insert_newline();
        } else {
            self.editor.input(key);
        }
        self.editor.lines() != before.as_slice()
    }

    /// Inserts pasted `text` at the text cursor and says whether the text
    /// changed. Terminals send a line break as `\r`, some as `\r\n`; both
    /// become `\n`, so a pasted text keeps its lines.
    pub fn paste(&mut self, text: &str) -> bool {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.editor.insert_str(text)
    }

    /// Moves the text cursor one row up or down, `to`, as the field was
    /// last drawn, or says that the field is left from its first or last
    /// row. Before the first draw the rows are the text's lines.
    ///
    /// The row comes from the layout with the cursor where it is. A
    /// position whose cursor would show on another row, such as the end of
    /// a row that fills the width, which would start a row of its own,
    /// gives way to the closest one before it that stays on the row.
    fn vertical(&mut self, to: Leave, wanted: Option<usize>) -> FieldKey {
        let unshaped = FieldShape {
            head: Span::default(),
            width: usize::MAX,
        };
        let shape = self.shape.as_ref().unwrap_or(&unshaped);
        let lines: Vec<&str> = self.editor.lines().iter().map(String::as_str).collect();
        let cursor = self.cursor();
        let rows = layout(&shape.head, &lines, Some(cursor), false, shape.width);
        let here = rows
            .iter()
            .position(|row| row.holds(cursor))
            .expect("the text cursor shows on a row");
        let column = wanted.unwrap_or_else(|| rows[here].column_of(cursor.1));
        self.wanted = Some(column);

        // The end of a line whose last row fills the width shows on a row
        // of its own, which is on screen only while the cursor is there.
        // `↓` from the row above it goes there first, so `↑` and `↓` from
        // that end come back to it; from further below, `↑` passes it.
        let row = &rows[here];
        if to == Leave::Down && row.last && cursor.1 != row.chars.end {
            let end = (row.line, row.chars.end);
            let own_row = layout(&shape.head, &lines, Some(end), false, shape.width)
                .iter()
                .find(|row| row.holds(end))
                .is_some_and(|row| !row.shows_text());
            if own_row {
                self.jump(end);
                return FieldKey::Unchanged;
            }
        }

        let target = match to {
            Leave::Up => rows[..here].iter().rev().find(|row| row.reachable()),
            _ => rows[here + 1..].iter().find(|row| row.reachable()),
        };
        let Some(target) = target else {
            return FieldKey::Leave(to);
        };
        let shows_on_target = |position: usize| {
            layout(
                &shape.head,
                &lines,
                Some((target.line, position)),
                false,
                shape.width,
            )
            .iter()
            .find(|row| row.holds((target.line, position)))
            .is_some_and(|row| row.line == target.line && row.chars.start == target.chars.start)
        };
        let aimed = target.position_at(column);
        let position = if shows_on_target(aimed) {
            aimed
        } else {
            target
                .positions()
                .filter(|position| *position < aimed)
                .rev()
                .find(|position| shows_on_target(*position))
                .unwrap_or(aimed)
        };
        let line = target.line;
        self.jump((line, position));
        FieldKey::Unchanged
    }

    /// Puts the text cursor at `cursor`, a line and a character in it.
    // TODO: the editor takes positions as `u16`, so a position past
    // 65535 lines or characters lands at that limit.
    fn jump(&mut self, (line, column): (usize, usize)) {
        let clamp = |value: usize| u16::try_from(value).unwrap_or(u16::MAX);
        self.editor
            .move_cursor(CursorMove::Jump(clamp(line), clamp(column)));
    }

    fn at_end(&self) -> bool {
        let (row, column) = self.cursor();
        let lines = self.editor.lines();
        row + 1 == lines.len() && column == lines[row].chars().count()
    }

    // -----------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------

    /// The lines of this field while it has the keys, drawn as `shape`
    /// says: its text after the head with the text cursor, which hides while `cursor` is false,
    /// and the counter against `limits` on a line of its own after the
    /// text. Without the text cursor an empty text shows nothing, not the
    /// `—` of a field without the keys, as the field still has them.
    pub fn lines(
        &self,
        shape: &FieldShape,
        cursor: bool,
        limits: Option<&Length>,
    ) -> Vec<Line<'static>> {
        let lines: Vec<&str> = self.editor.lines().iter().map(String::as_str).collect();
        let cursor = cursor.then(|| self.cursor());
        let mut lines = drawn(&layout(&shape.head, &lines, cursor, false, shape.width));
        if let Some(limits) = limits {
            let (text, level) = counter(self.length(), limits);
            lines.push(Line::from(Span::from(text).fg(level.colour())).right_aligned());
        }
        lines
    }
}

/// The lines of a field without the keys that holds `text`: the text
/// after `head`, or a dim `—` when it is empty.
pub fn resting_lines(head: Span<'static>, text: &str, width: usize) -> Vec<Line<'static>> {
    let lines: Vec<&str> = text.split('\n').collect();
    drawn(&layout(&head, &lines, None, text.is_empty(), width))
}

/// The lines a field holding `text` takes however it shows, so the layout
/// can count them (spec section 7.1): the lines without the keys, and a
/// row for the counter when the field has `limits`, so giving it the keys
/// adds none. Without a cursor, the cursor's blank after a full line is
/// left out.
pub fn counted_lines(
    head: Span<'static>,
    text: &str,
    width: usize,
    limits: bool,
) -> Vec<Line<'static>> {
    let mut lines = resting_lines(head, text, width);
    if limits {
        lines.push(Line::default());
    }
    lines
}

// ---------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------

/// What a glyph of a field's layout shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// The head, the indent of a further line, or the `—` of an empty
    /// field: no text.
    Lead,
    /// The grapheme at character `start` of its line, `chars` characters
    /// long.
    Text { start: usize, chars: usize },
    /// The text cursor's blank after the end of its line.
    End,
}

/// One row of a field as the person sees it.
#[derive(Debug)]
struct VisualRow<'a> {
    /// The line of the text the row shows a part of.
    line: usize,
    indent: usize,
    glyphs: Vec<Glyph<'a>>,
    /// What each of `glyphs` shows.
    sources: Vec<Source>,
    /// The characters of `line` on the row.
    chars: Range<usize>,
    /// Whether the row ends its line, so the end of the line is on it.
    last: bool,
}

impl VisualRow<'_> {
    /// Whether the text cursor at `cursor` shows on this row.
    fn holds(&self, (line, column): (usize, usize)) -> bool {
        self.line == line
            && self.sources.iter().any(|source| match *source {
                Source::Text { start, chars } => (start..start + chars).contains(&column),
                Source::End => true,
                Source::Lead => false,
            })
    }

    /// Whether the text cursor can be put on this row: it shows text, or
    /// the end of its line, as on an empty line. A row with only the head,
    /// where a long head wraps, cannot hold it.
    fn reachable(&self) -> bool {
        self.last || self.shows_text()
    }

    /// Whether the row shows text. A row with only the cursor's blank
    /// after its line shows none.
    fn shows_text(&self) -> bool {
        self.sources
            .iter()
            .any(|source| matches!(source, Source::Text { .. }))
    }

    /// The display column at which the character `column` of the row's
    /// line shows.
    fn column_of(&self, column: usize) -> usize {
        let before = self
            .sources
            .iter()
            .take_while(|source| match **source {
                Source::Text { start, chars } => start + chars <= column,
                Source::Lead => true,
                Source::End => false,
            })
            .count();
        self.indent
            + self.glyphs[..before]
                .iter()
                .map(|glyph| glyph.width)
                .sum::<usize>()
    }

    /// The positions the text cursor can take on the row, in order: each
    /// grapheme's start, and the end of the line on its last row.
    fn positions(&self) -> impl DoubleEndedIterator<Item = usize> {
        let starts: Vec<usize> = self
            .sources
            .iter()
            .filter_map(|source| match source {
                Source::Text { start, .. } => Some(*start),
                _ => None,
            })
            .collect();
        starts
            .into_iter()
            .chain(self.last.then_some(self.chars.end))
    }

    /// The position on the row closest to display column `column`: the
    /// grapheme that covers it, the end of the line past the last one, or
    /// the row's first or last grapheme where the column lies before or
    /// past the text.
    fn position_at(&self, column: usize) -> usize {
        let mut at = self.indent;
        let mut position = None;
        for (glyph, source) in self.glyphs.iter().zip(&self.sources) {
            if let Source::Text { start, .. } = *source
                && (position.is_none() || at <= column)
            {
                position = Some(start);
            }
            at += glyph.width;
        }
        if self.last && column >= at {
            return self.chars.end;
        }
        position.unwrap_or(self.chars.end)
    }
}

/// What a field shows, laid out in rows of `width` columns: `lines` of its
/// text after `head`, the grapheme under the text cursor inverted, and a
/// dim `—` for an empty text where `placeholder` holds. Drawing and
/// counting rows both go through this one function, so they agree on
/// where a row breaks.
///
/// Every line after the first starts under the start of the text, or
/// `NARROW_INDENT` columns in when the head leaves the text less than
/// `MIN_TEXT_WIDTH` (ADR 27), and so does every row a line wraps onto.
fn layout<'a>(
    head: &'a Span<'a>,
    lines: &[&'a str],
    cursor: Option<(usize, usize)>,
    placeholder: bool,
    width: usize,
) -> Vec<VisualRow<'a>> {
    let head_width = columns(&head.content);
    let indent = if width.saturating_sub(head_width) >= MIN_TEXT_WIDTH {
        head_width
    } else {
        NARROW_INDENT
    };
    let mut rows = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let lead: Vec<Glyph> = if index == 0 {
            head.content
                .graphemes(true)
                .map(|grapheme| Glyph::new(grapheme, head.style))
                .collect()
        } else {
            vec![Glyph::new(" ", Style::new()); indent]
        };
        let cursor = cursor
            .filter(|(row, _)| *row == index)
            .map(|(_, column)| column);
        rows.extend(line_rows(
            index,
            lead,
            line,
            cursor,
            placeholder,
            width,
            indent,
        ));
    }
    rows
}

/// The rows of line `index` of a field's text after `lead`, the head or
/// the indent, the further rows behind `indent` blanks.
///
/// The text cursor's column is a character index, as ratatui-textarea
/// counts it, and can lie inside a grapheme, such as on the skin tone of
/// an emoji: the whole grapheme shows as the cursor then, as no terminal
/// cell holds a part of one. At the end of the line the cursor is an
/// inverted blank.
fn line_rows<'a>(
    index: usize,
    lead: Vec<Glyph<'a>>,
    line: &'a str,
    cursor: Option<usize>,
    placeholder: bool,
    width: usize,
    indent: usize,
) -> Vec<VisualRow<'a>> {
    let mut sources = vec![Source::Lead; lead.len()];
    let mut glyphs = lead;
    if placeholder {
        glyphs.push(Glyph::new("—", Style::new().dim()));
        sources.push(Source::Lead);
    }
    let mut start = 0;
    let mut covered = false;
    for grapheme in line.graphemes(true) {
        let chars = grapheme.chars().count();
        let under = !covered && cursor.is_some_and(|column| column < start + chars);
        covered |= under;
        let style = if under {
            Style::new().reversed()
        } else {
            Style::new()
        };
        glyphs.push(Glyph::new(grapheme, style));
        sources.push(Source::Text { start, chars });
        start += chars;
    }
    if cursor.is_some() && !covered {
        glyphs.push(Glyph::new(" ", Style::new().reversed()));
        sources.push(Source::End);
    }

    let breaks = wrap_rows(&glyphs, width, 0, indent);
    let count = breaks.len();
    // Each row's characters run from where the row before ended to the
    // end of its last grapheme.
    let mut position = 0;
    breaks
        .into_iter()
        .enumerate()
        .map(|(number, (indent, row))| {
            let from = position;
            for source in &sources[row.clone()] {
                if let Source::Text { start, chars } = *source {
                    position = start + chars;
                }
            }
            VisualRow {
                line: index,
                indent,
                glyphs: glyphs[row.clone()].to_vec(),
                sources: sources[row].to_vec(),
                chars: from..position,
                last: number + 1 == count,
            }
        })
        .collect()
}

/// `rows` as lines to draw.
fn drawn(rows: &[VisualRow]) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| styled_line(row.indent, &row.glyphs))
        .collect()
}

// ---------------------------------------------------------------------
// The counter
// ---------------------------------------------------------------------

/// How a length counter is coloured (spec section 7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Within the target.
    Fine,
    /// Above the target, up to `warn`.
    Warn,
    /// Above `warn`.
    Over,
}

impl Level {
    fn colour(self) -> Color {
        match self {
            Level::Fine => Color::Green,
            Level::Warn => Color::Yellow,
            Level::Over => Color::Red,
        }
    }
}

/// The counter text for `length` characters against `limits`: the length
/// over the end of the target range (or over `warn` without a target), `!`
/// above the target and `!!` above `warn`. The text says everything the
/// colour says, so it works without colour.
pub fn counter(length: usize, limits: &Length) -> (String, Level) {
    let length32 = u32::try_from(length).unwrap_or(u32::MAX);
    let target_end = limits.target.map(|(_, end)| end);
    let above_warn = limits.warn.is_some_and(|warn| length32 > warn);
    let above_target = target_end.is_some_and(|end| length32 > end);

    let mut text = match target_end.or(limits.warn) {
        Some(reference) => format!("{length}/{reference}"),
        None => length.to_string(),
    };
    let level = if above_warn {
        text.push_str(" !!");
        Level::Over
    } else if above_target {
        text.push_str(" !");
        Level::Warn
    } else {
        Level::Fine
    };
    (text, level)
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Modifier, Style};

    use super::*;

    fn code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn typed(field: &mut TextField, text: &str) {
        for c in text.chars() {
            field.key(code(KeyCode::Char(c)));
        }
    }

    fn limits(target: Option<(u32, u32)>, warn: Option<u32>) -> Length {
        Length { target, warn }
    }

    // ---------------------------------------------------------------
    // Editing
    // ---------------------------------------------------------------

    #[test]
    fn a_field_starts_with_its_text_and_the_cursor_at_the_end() {
        let field = TextField::new("one\ntwo");

        assert_eq!(field.text(), "one\ntwo");
        assert_eq!(field.cursor(), (1, 3));
        assert_eq!(field.length(), 7, "a line break counts as one");
        assert_eq!(TextField::new("").length(), 0);
    }

    #[test]
    fn keys_that_type_change_the_text_and_others_do_not() {
        let mut field = TextField::new("ab");

        assert_eq!(field.key(code(KeyCode::Char('c'))), FieldKey::Changed);
        assert_eq!(field.key(code(KeyCode::Backspace)), FieldKey::Changed);
        assert_eq!(field.key(code(KeyCode::F(5))), FieldKey::Unchanged);
        assert_eq!(field.text(), "ab");
    }

    #[test]
    fn enter_and_esc_are_left_to_the_fields_context() {
        let mut field = TextField::new("ab");

        assert_eq!(field.key(code(KeyCode::Enter)), FieldKey::Enter);
        assert_eq!(field.key(code(KeyCode::Esc)), FieldKey::Escape);
        assert_eq!(field.text(), "ab", "enter adds no line");
    }

    /// `text` in a field drawn after the head `> ` in `width` columns, the
    /// text cursor at `cursor`.
    fn shaped(text: &str, width: usize, cursor: (usize, usize)) -> TextField {
        let mut field = TextField::new(text);
        field.set_shape(FieldShape {
            head: Span::from("> "),
            width,
        });
        field.jump(cursor);
        field
    }

    /// Presses `key` and returns where the text cursor went, or how the
    /// field was left.
    fn step(field: &mut TextField, key: KeyCode) -> Result<(usize, usize), FieldKey> {
        match field.key(code(key)) {
            FieldKey::Unchanged => Ok(field.cursor()),
            other => Err(other),
        }
    }

    /// Eight words of 5 letters, three to a row after `> ` in 22 columns:
    ///
    /// ```text
    /// > aaaaa bbbbb ccccc
    ///   ddddd eeeee fffff
    ///   ggggg hhhhh
    /// ```
    const THREE_ROWS: &str = "aaaaa bbbbb ccccc ddddd eeeee fffff ggggg hhhhh";

    #[test]
    fn up_and_down_move_through_the_rows_the_field_shows() {
        let mut field = shaped(THREE_ROWS, 22, (0, 47));

        assert_eq!(step(&mut field, KeyCode::Up), Ok((0, 29)), "under the end");
        assert_eq!(step(&mut field, KeyCode::Up), Ok((0, 11)));
        assert_eq!(
            step(&mut field, KeyCode::Up),
            Err(FieldKey::Leave(Leave::Up)),
            "from the first row"
        );
        assert_eq!(field.cursor(), (0, 11), "leaving moves nothing");
        assert_eq!(step(&mut field, KeyCode::Down), Ok((0, 29)));
        assert_eq!(step(&mut field, KeyCode::Down), Ok((0, 47)));
        assert_eq!(
            step(&mut field, KeyCode::Down),
            Err(FieldKey::Leave(Leave::Down)),
            "from the last row"
        );
    }

    #[test]
    fn up_and_down_keep_the_column_they_started_from() {
        // The blank after `fffff`, at column 19 of the second row.
        let mut field = shaped(THREE_ROWS, 22, (0, 35));

        assert_eq!(step(&mut field, KeyCode::Down), Ok((0, 47)), "at the end");
        assert_eq!(
            step(&mut field, KeyCode::Up),
            Ok((0, 35)),
            "column 19 again"
        );
        field.key(code(KeyCode::Left));
        assert_eq!(
            step(&mut field, KeyCode::Up),
            Ok((0, 16)),
            "another key starts afresh from column 18"
        );
    }

    #[test]
    fn up_and_down_go_by_the_columns_of_wide_characters() {
        // One word wider than a row: it starts on the row below the head,
        // ten characters of two columns to a row.
        let mut field = shaped("日本語の説明です日本語の説明です", 22, (0, 16));

        assert_eq!(step(&mut field, KeyCode::Up), Ok((0, 6)));
        assert_eq!(
            step(&mut field, KeyCode::Up),
            Err(FieldKey::Leave(Leave::Up)),
            "the head's row holds no text"
        );
        field.key(code(KeyCode::Right));
        assert_eq!(step(&mut field, KeyCode::Down), Ok((0, 16)), "the end");
    }

    #[test]
    fn up_and_down_cross_line_breaks_and_wrapped_rows_alike() {
        let mut field = shaped("aaaaa bbbbb ccccc ddddd\nee\n\nfff", 22, (3, 3));

        assert_eq!(step(&mut field, KeyCode::Up), Ok((2, 0)), "an empty line");
        assert_eq!(step(&mut field, KeyCode::Up), Ok((1, 2)));
        assert_eq!(
            step(&mut field, KeyCode::Up),
            Ok((0, 21)),
            "the wrapped row"
        );
        assert_eq!(step(&mut field, KeyCode::Up), Ok((0, 3)));
        assert_eq!(step(&mut field, KeyCode::Down), Ok((0, 21)));
        assert_eq!(step(&mut field, KeyCode::Down), Ok((1, 2)));
    }

    #[test]
    fn a_row_ending_at_the_width_is_reached_in_both_directions() {
        // `> ` and 20 letters fill the first row exactly. The cursor after
        // its last letter shows as a blank on a row of its own below.
        let full = "abcdefghijklmnopqrst\nxy";
        let table = [
            ((1, 2), KeyCode::Up, Ok((0, 2))),
            ((0, 2), KeyCode::Down, Ok((0, 20))),
            ((0, 19), KeyCode::Down, Ok((0, 20))),
            ((0, 20), KeyCode::Up, Ok((0, 0))),
            ((0, 20), KeyCode::Down, Ok((1, 0))),
            ((0, 19), KeyCode::Up, Err(FieldKey::Leave(Leave::Up))),
        ];
        for (from, key, to) in table {
            let mut field = shaped(full, 22, from);
            assert_eq!(step(&mut field, key), to, "{key:?} from {from:?}");
        }

        // Back up from the line below at column 21 stops on the last
        // letter: the end's own row is entered only from directly above.
        let mut field = shaped(full, 22, (0, 19));
        assert_eq!(step(&mut field, KeyCode::Down), Ok((0, 20)));
        assert_eq!(step(&mut field, KeyCode::Down), Ok((1, 2)));
        assert_eq!(step(&mut field, KeyCode::Up), Ok((0, 19)));
    }

    #[test]
    fn a_column_kept_from_a_wider_draw_still_lands_on_the_row() {
        let mut field = shaped(
            "abcdefghijklmnopqrstuvwxyz\nabcdefghijklmnopqrst",
            40,
            (0, 26),
        );
        // Column 28, after the first line, is kept for the next `↓`.
        step(&mut field, KeyCode::Up).expect_err("leaves from the first row");
        field.set_shape(shape_of(22));

        // At 22 columns the second line fills its row, and column 28 lies
        // past it: its end would show on a row of its own, so the cursor
        // stops on its last letter.
        assert_eq!(step(&mut field, KeyCode::Down), Ok((1, 19)));
    }

    #[test]
    fn up_and_down_from_the_end_of_a_full_row_come_back_to_it() {
        // The end of a full last row shows on a row of its own, in the
        // last line and in a line with another after it.
        for (text, end) in [
            ("abcdefghijklmnopqrst", (0, 20)),
            ("abcdefghijklmnopqrst\nxy", (0, 20)),
        ] {
            let mut field = shaped(text, 22, end);
            assert_eq!(step(&mut field, KeyCode::Up), Ok((0, 0)), "{text:?}");
            assert_eq!(step(&mut field, KeyCode::Down), Ok(end), "{text:?}");
        }

        let mut field = shaped("abcdefghijklmnopqrst", 22, (0, 20));
        assert_eq!(
            step(&mut field, KeyCode::Down),
            Err(FieldKey::Leave(Leave::Down)),
            "only from the end's own row"
        );
    }

    #[test]
    fn before_the_first_draw_up_and_down_go_by_the_texts_lines() {
        let mut field = TextField::new("one\n\nthree");

        assert_eq!(step(&mut field, KeyCode::Up), Ok((1, 0)));
        assert_eq!(
            step(&mut field, KeyCode::Up),
            Ok((0, 3)),
            "column 5 clamped"
        );
        assert_eq!(
            step(&mut field, KeyCode::Up),
            Err(FieldKey::Leave(Leave::Up))
        );
        assert_eq!(step(&mut field, KeyCode::Down), Ok((1, 0)));
        assert_eq!(
            step(&mut field, KeyCode::Down),
            Ok((2, 5)),
            "back to column 5"
        );
    }

    #[test]
    fn up_and_down_with_a_modifier_move_the_same_way() {
        let mut field = shaped("one\ntwo", 40, (1, 3));

        assert_eq!(
            field.key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)),
            FieldKey::Unchanged
        );
        assert_eq!(field.cursor(), (0, 3));
    }

    #[test]
    fn left_and_right_leave_only_at_the_edges_of_the_text() {
        let mut field = TextField::new("ab");

        assert_eq!(
            field.key(code(KeyCode::Right)),
            FieldKey::Leave(Leave::Right)
        );
        assert_eq!(field.key(code(KeyCode::Left)), FieldKey::Unchanged);
        assert_eq!(field.cursor(), (0, 1));
        assert_eq!(field.key(code(KeyCode::Right)), FieldKey::Unchanged);
        field.key(code(KeyCode::Home));
        assert_eq!(field.key(code(KeyCode::Left)), FieldKey::Leave(Leave::Left));

        let mut empty = TextField::new("");
        assert_eq!(empty.key(code(KeyCode::Left)), FieldKey::Leave(Leave::Left));
        assert_eq!(
            empty.key(code(KeyCode::Right)),
            FieldKey::Leave(Leave::Right)
        );
    }

    #[test]
    fn an_arrow_with_a_modifier_never_leaves() {
        let mut field = TextField::new("ab cd");

        let word_left = KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(field.key(word_left), FieldKey::Unchanged);
        assert_eq!(field.cursor(), (0, 3));
        let word_right = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL);
        field.key(word_right);
        assert_eq!(field.key(word_right), FieldKey::Unchanged, "at the end");
    }

    #[test]
    fn the_end_of_a_multi_line_text_is_the_end_of_its_last_line() {
        let mut field = TextField::new("one\ntwo");
        field.jump((0, 3));

        assert_eq!(field.key(code(KeyCode::Right)), FieldKey::Unchanged);
        assert_eq!(field.cursor(), (1, 0), "on to the next line");
    }

    #[test]
    fn ctrl_j_adds_a_line() {
        let mut field = TextField::new("a");

        assert_eq!(field.key(ctrl('j')), FieldKey::Changed);
        typed(&mut field, "b");

        assert_eq!(field.text(), "a\nb");
    }

    #[test]
    fn a_paste_turns_every_line_break_into_a_newline() {
        let mut field = TextField::new("");

        assert!(field.paste("one\r\ntwo\rthree\nfour"));

        assert_eq!(field.text(), "one\ntwo\nthree\nfour");
        assert_eq!(field.cursor(), (3, 4));
    }

    #[test]
    fn a_paste_goes_in_at_the_text_cursor() {
        let mut field = TextField::new("ab");
        field.key(code(KeyCode::Left));

        assert!(field.paste("x\ny"));

        assert_eq!(field.text(), "ax\nyb");
        assert!(!field.paste(""), "an empty paste changes nothing");
    }

    // ---------------------------------------------------------------
    // Drawing
    // ---------------------------------------------------------------

    /// A field drawn after `> ` in `width` columns.
    fn shape_of(width: usize) -> FieldShape {
        FieldShape {
            head: Span::from("> "),
            width,
        }
    }

    /// The text of each line, spans joined.
    fn texts(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    /// The text of each line and the column of its reversed cell, if any.
    fn cursor_columns(lines: &[Line]) -> Vec<(String, Option<usize>)> {
        lines
            .iter()
            .map(|line| {
                let text: String = line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                let mut column = 0;
                let mut cursor = None;
                for span in &line.spans {
                    if span.style.add_modifier.contains(Modifier::REVERSED) {
                        cursor = Some(column);
                    }
                    column += columns(&span.content);
                }
                (text, cursor)
            })
            .collect()
    }

    #[test]
    fn every_line_of_the_text_starts_under_its_start() {
        let lines = resting_lines(Span::from("Notes: "), "first\nsecond", 40);

        assert_eq!(texts(&lines), ["Notes: first", "       second"]);
    }

    #[test]
    fn a_long_line_wraps_under_the_start_of_the_text() {
        let lines = resting_lines(Span::from("H: "), "alpha beta gamma delta", 24);

        assert_eq!(texts(&lines), ["H: alpha beta gamma ", "   delta"]);
    }

    #[test]
    fn a_head_leaving_less_than_20_columns_wraps_the_text_4_columns_in() {
        let lines = resting_lines(Span::from("Head: "), "alpha beta gamma delta\nepsilon", 25);

        assert_eq!(
            texts(&lines),
            ["Head: alpha beta gamma ", "    delta", "    epsilon"]
        );
    }

    #[test]
    fn an_empty_field_without_the_keys_shows_a_dim_dash_after_its_head() {
        let head = Span::from("Head: ").reversed();
        let lines = resting_lines(head.clone(), "", 40);

        assert_eq!(lines.len(), 1);
        assert_eq!(texts(&lines), ["Head: —"]);
        let spans = &lines[0].spans;
        assert_eq!(spans[1].content, "Head: ");
        assert_eq!(spans[1].style, head.style, "the head keeps its style");
        assert_eq!(spans[2].style, Style::new().dim());
    }

    #[test]
    fn a_field_with_the_keys_shows_its_cursor_and_its_counter() {
        let mut field = TextField::new("ab");
        field.key(code(KeyCode::Left));
        let warn = limits(None, Some(10));

        let lines = field.lines(&shape_of(40), true, Some(&warn));

        assert_eq!(
            cursor_columns(&lines),
            [("> ab".to_owned(), Some(3)), ("2/10".to_owned(), None)]
        );
        assert_eq!(lines[1].alignment, Some(ratatui::layout::Alignment::Right));
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Green));
    }

    #[test]
    fn the_counter_takes_the_colour_of_its_level() {
        let field = TextField::new(&"x".repeat(12));

        let lines = field.lines(&shape_of(40), true, Some(&limits(None, Some(10))));

        assert_eq!(texts(&lines[1..]), ["12/10 !!"]);
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Red));
        assert_eq!(Level::Warn.colour(), Color::Yellow);
    }

    #[test]
    fn a_field_with_the_keys_but_without_its_cursor_shows_no_dash() {
        let field = TextField::new("");

        let lines = field.lines(&shape_of(40), false, None);

        assert_eq!(cursor_columns(&lines), [("> ".to_owned(), None)]);
    }

    #[test]
    fn counting_keeps_a_row_for_the_counter_of_a_field_with_limits() {
        let head = || Span::from("> ");

        assert_eq!(counted_lines(head(), "a\nb", 40, false).len(), 2);
        assert_eq!(counted_lines(head(), "a\nb", 40, true).len(), 3);
        // A full line of 4 columns: the cursor's blank would take a row of
        // its own, which counting leaves out.
        assert_eq!(counted_lines(head(), "abcd", 6, false).len(), 1);
        let field = TextField::new("abcd");
        assert_eq!(
            field
                .lines(
                    &FieldShape {
                        head: head(),
                        width: 6
                    },
                    true,
                    None
                )
                .len(),
            2
        );
    }

    /// `line` with the text cursor at `column`, wrapped to `width` with
    /// further rows `rest` columns in.
    fn line_with_cursor(
        line: &str,
        column: usize,
        width: usize,
        rest: usize,
    ) -> Vec<Line<'static>> {
        drawn(&line_rows(
            0,
            Vec::new(),
            line,
            Some(column),
            false,
            width,
            rest,
        ))
    }

    #[test]
    fn the_text_cursor_after_a_full_line_starts_the_next_one() {
        let lines = line_with_cursor("ab cd", 5, 5, 1);

        assert_eq!(
            cursor_columns(&lines),
            [("ab cd".to_owned(), None), ("  ".to_owned(), Some(1))]
        );
    }

    #[test]
    fn the_text_cursor_on_the_space_at_a_line_break_starts_the_next_line() {
        let lines = line_with_cursor("abcd efg", 4, 4, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("abcd".to_owned(), None), (" efg".to_owned(), Some(0))]
        );
    }

    #[test]
    fn the_text_cursor_inside_a_word_keeps_the_word_whole() {
        let lines = line_with_cursor("ab channel", 3, 8, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("ab ".to_owned(), None), ("channel".to_owned(), Some(0))]
        );
    }

    #[test]
    fn the_text_cursor_on_a_wide_character_moves_with_it_to_the_next_line() {
        assert_eq!(
            cursor_columns(&line_with_cursor("日本語", 2, 5, 1)),
            [("日本".to_owned(), None), (" 語".to_owned(), Some(1))]
        );

        assert_eq!(
            cursor_columns(&line_with_cursor("日本", 2, 4, 1)),
            [("日本".to_owned(), None), ("  ".to_owned(), Some(1))],
            "after a line full of wide characters"
        );
    }

    /// The text of the inverted span of `lines`.
    fn under_cursor(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .filter(|span| span.style.add_modifier.contains(Modifier::REVERSED))
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn the_text_cursor_covers_the_whole_grapheme_its_character_is_in() {
        // ratatui-textarea moves its cursor by characters, so it can stop
        // on a combining accent or on the skin tone of an emoji.
        let accent = line_with_cursor("ce\u{301}x", 2, 10, 0);
        assert_eq!(under_cursor(&accent), "e\u{301}");
        assert_eq!(
            cursor_columns(&accent),
            [("ce\u{301}x".to_owned(), Some(1))]
        );

        let emoji = line_with_cursor("👍🏽!", 1, 10, 0);
        assert_eq!(under_cursor(&emoji), "👍🏽");
        assert_eq!(cursor_columns(&emoji), [("👍🏽!".to_owned(), Some(0))]);
    }

    // ---------------------------------------------------------------
    // The counter
    // ---------------------------------------------------------------

    #[test]
    fn the_counter_reads_against_the_target_end() {
        let length = limits(Some((80, 125)), Some(145));

        assert_eq!(counter(112, &length), ("112/125".to_owned(), Level::Fine));
        assert_eq!(counter(125, &length), ("125/125".to_owned(), Level::Fine));
        assert_eq!(counter(140, &length), ("140/125 !".to_owned(), Level::Warn));
        assert_eq!(counter(145, &length), ("145/125 !".to_owned(), Level::Warn));
        assert_eq!(
            counter(182, &length),
            ("182/125 !!".to_owned(), Level::Over)
        );
    }

    #[test]
    fn the_counter_falls_back_to_warn() {
        assert_eq!(
            counter(3, &limits(None, Some(10))),
            ("3/10".to_owned(), Level::Fine)
        );
        assert_eq!(
            counter(12, &limits(None, Some(10))),
            ("12/10 !!".to_owned(), Level::Over)
        );
        assert_eq!(
            counter(200, &limits(Some((1, 5)), None)),
            ("200/5 !".to_owned(), Level::Warn)
        );
        assert_eq!(
            counter(2, &limits(None, None)),
            ("2".to_owned(), Level::Fine)
        );
    }
}
