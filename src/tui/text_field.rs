// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A typed field (spec section 7.3): the own answer, the answer of a
//! `text` question, a question's note, the session note and the reject
//! reason. What every field does the same lives here: editing its text,
//! measuring it and drawing it. Where a field's text is kept, and what
//! `enter` and leaving the field mean, depend on the field and stay with
//! [`super::SessionState`].

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Stylize};
use ratatui::text::{Line, Span};
use ratatui_textarea::{CursorMove, TextArea};
use unicode_segmentation::UnicodeSegmentation;

use super::render::{columns, wrap};
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

/// The field that has the keys: its text with the text cursor.
#[derive(Debug, Clone)]
pub struct TextField {
    editor: TextArea<'static>,
    /// Whether the text may hold line breaks.
    multiline: bool,
}

impl TextField {
    /// A field holding `text`, its text cursor at the end, so typing
    /// goes on where the person left off.
    pub fn new(text: &str, multiline: bool) -> Self {
        let mut editor = TextArea::new(text.split('\n').map(str::to_owned).collect());
        editor.set_hard_tab_indent(true);
        editor.move_cursor(CursorMove::Bottom);
        editor.move_cursor(CursorMove::End);
        TextField { editor, multiline }
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

    /// Applies `key`. `enter`, `esc` and the arrows that would leave the
    /// text are left to the caller, as their meaning depends on the
    /// field; every other key edits.
    ///
    /// `↑`/`↓` always leave, `←` only at the start of the text and `→`
    /// only at its end. Inside the text, and with a modifier held, `←`
    /// and `→` move the text cursor.
    pub fn key(&mut self, key: KeyEvent) -> FieldKey {
        let plain = key.modifiers.is_empty();
        match key.code {
            KeyCode::Enter => FieldKey::Enter,
            KeyCode::Esc => FieldKey::Escape,
            KeyCode::Up => FieldKey::Leave(Leave::Up),
            KeyCode::Down => FieldKey::Leave(Leave::Down),
            KeyCode::Left if plain && self.cursor() == (0, 0) => FieldKey::Leave(Leave::Left),
            KeyCode::Right if plain && self.at_end() => FieldKey::Leave(Leave::Right),
            _ if self.input(key) => FieldKey::Changed,
            _ => FieldKey::Unchanged,
        }
    }

    /// Hands `key` to the editor as it is, arrows included, and says
    /// whether the text changed. `ctrl-j` adds a line where the field is
    /// multi-line; any other way to a second line in a one-line field,
    /// such as `ctrl-m`, is undone.
    pub fn input(&mut self, key: KeyEvent) -> bool {
        let before = self.editor.lines().to_vec();
        if key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.multiline {
                self.editor.insert_newline();
            }
        } else {
            let previous = self.editor.clone();
            self.editor.input(key);
            if !self.multiline && self.editor.lines().len() > 1 {
                self.editor = previous;
            }
        }
        self.editor.lines() != before.as_slice()
    }

    /// Inserts pasted `text` at the text cursor and says whether the text
    /// changed. Terminals send a line break as `\r`, some as `\r\n`; both
    /// become `\n`, which a one-line field takes as a blank.
    pub fn paste(&mut self, text: &str) -> bool {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let text = if self.multiline {
            text
        } else {
            text.replace('\n', " ")
        };
        self.editor.insert_str(text)
    }

    fn at_end(&self) -> bool {
        let (row, column) = self.cursor();
        let lines = self.editor.lines();
        row + 1 == lines.len() && column == lines[row].chars().count()
    }

    // -----------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------

    /// The lines of this field while it has the keys: its text after
    /// `head` with the text cursor, which hides while `cursor` is false,
    /// and the counter against `limits` on a line of its own after the
    /// text. Without the text cursor an empty text shows nothing, not the
    /// `—` of a field without the keys, as the field still has them.
    pub fn lines(
        &self,
        head: Span<'static>,
        width: usize,
        cursor: bool,
        limits: Option<&Length>,
    ) -> Vec<Line<'static>> {
        let cursor = cursor.then(|| self.cursor());
        let mut lines = after_head(head, with_cursor(&self.text(), cursor), width);
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
    let text = if text.is_empty() {
        vec![Line::from(Span::from("—").dim())]
    } else {
        text.split('\n')
            .map(|line| Line::from(line.to_owned()))
            .collect()
    };
    after_head(head, text, width)
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

/// `text`, one line per line of the field, after `head`, wrapped to
/// `width`. Every further line, whether the text breaks there or it is
/// wrapped, starts under the start of the text, or `NARROW_INDENT`
/// columns in when the head leaves the text less than `MIN_TEXT_WIDTH`
/// (ADR 27).
fn after_head(head: Span<'static>, text: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let head_width = columns(&head.content);
    let indent = if width.saturating_sub(head_width) >= MIN_TEXT_WIDTH {
        head_width
    } else {
        NARROW_INDENT
    };
    let mut lines = Vec::new();
    for (index, line) in text.into_iter().enumerate() {
        let lead = if index == 0 {
            head.clone()
        } else {
            Span::from(" ".repeat(indent))
        };
        let mut spans = vec![lead];
        spans.extend(line.spans);
        lines.extend(wrap(Line::from(spans), width, 0, indent));
    }
    lines
}

/// `text` as lines with the grapheme under the text cursor inverted, or
/// an inverted blank after the end of its line. The cursor's column is a
/// character index, as ratatui-textarea counts it, and can lie inside a
/// grapheme, such as on the skin tone of an emoji: the whole grapheme
/// shows as the cursor then, as no terminal cell holds a part of one.
fn with_cursor(text: &str, cursor: Option<(usize, usize)>) -> Vec<Line<'static>> {
    text.split('\n')
        .enumerate()
        .map(|(index, line)| match cursor {
            Some((row, column)) if row == index => {
                let mut before = String::new();
                let mut under = None;
                let mut after = String::new();
                let mut start = 0;
                for grapheme in line.graphemes(true) {
                    let end = start + grapheme.chars().count();
                    if under.is_none() && column < end {
                        under = Some(grapheme);
                    } else if under.is_none() {
                        before.push_str(grapheme);
                    } else {
                        after.push_str(grapheme);
                    }
                    start = end;
                }
                Line::from(vec![
                    Span::from(before),
                    Span::from(under.unwrap_or(" ").to_owned()).reversed(),
                    Span::from(after),
                ])
            }
            _ => Line::from(line.to_owned()),
        })
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
        let field = TextField::new("one\ntwo", true);

        assert_eq!(field.text(), "one\ntwo");
        assert_eq!(field.cursor(), (1, 3));
        assert_eq!(field.length(), 7, "a line break counts as one");
        assert_eq!(TextField::new("", false).length(), 0);
    }

    #[test]
    fn keys_that_type_change_the_text_and_others_do_not() {
        let mut field = TextField::new("ab", false);

        assert_eq!(field.key(code(KeyCode::Char('c'))), FieldKey::Changed);
        assert_eq!(field.key(code(KeyCode::Backspace)), FieldKey::Changed);
        assert_eq!(field.key(code(KeyCode::F(5))), FieldKey::Unchanged);
        assert_eq!(field.text(), "ab");
    }

    #[test]
    fn enter_and_esc_are_left_to_the_fields_context() {
        let mut field = TextField::new("ab", true);

        assert_eq!(field.key(code(KeyCode::Enter)), FieldKey::Enter);
        assert_eq!(field.key(code(KeyCode::Esc)), FieldKey::Escape);
        assert_eq!(field.text(), "ab", "enter adds no line");
    }

    #[test]
    fn up_and_down_leave_from_any_line() {
        let mut field = TextField::new("one\ntwo\nthree", true);
        field.input(code(KeyCode::Up));

        assert_eq!(field.key(code(KeyCode::Up)), FieldKey::Leave(Leave::Up));
        assert_eq!(
            field.key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT)),
            FieldKey::Leave(Leave::Down)
        );
        assert_eq!(field.cursor().0, 1, "the text cursor stays");
    }

    #[test]
    fn input_moves_the_text_cursor_between_lines() {
        let mut field = TextField::new("one\ntwo", true);

        assert!(!field.input(code(KeyCode::Up)), "the text stays");

        assert_eq!(field.cursor(), (0, 3));
    }

    #[test]
    fn left_and_right_leave_only_at_the_edges_of_the_text() {
        let mut field = TextField::new("ab", false);

        assert_eq!(
            field.key(code(KeyCode::Right)),
            FieldKey::Leave(Leave::Right)
        );
        assert_eq!(field.key(code(KeyCode::Left)), FieldKey::Unchanged);
        assert_eq!(field.cursor(), (0, 1));
        assert_eq!(field.key(code(KeyCode::Right)), FieldKey::Unchanged);
        field.key(code(KeyCode::Home));
        assert_eq!(field.key(code(KeyCode::Left)), FieldKey::Leave(Leave::Left));

        let mut empty = TextField::new("", false);
        assert_eq!(empty.key(code(KeyCode::Left)), FieldKey::Leave(Leave::Left));
        assert_eq!(
            empty.key(code(KeyCode::Right)),
            FieldKey::Leave(Leave::Right)
        );
    }

    #[test]
    fn an_arrow_with_a_modifier_never_leaves() {
        let mut field = TextField::new("ab cd", false);

        let word_left = KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(field.key(word_left), FieldKey::Unchanged);
        assert_eq!(field.cursor(), (0, 3));
        let word_right = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL);
        field.key(word_right);
        assert_eq!(field.key(word_right), FieldKey::Unchanged, "at the end");
    }

    #[test]
    fn the_end_of_a_multi_line_text_is_the_end_of_its_last_line() {
        let mut field = TextField::new("one\ntwo", true);
        field.input(code(KeyCode::Up));

        assert_eq!(field.key(code(KeyCode::Right)), FieldKey::Unchanged);
        assert_eq!(field.cursor(), (1, 0), "on to the next line");
    }

    #[test]
    fn ctrl_j_adds_a_line_only_in_a_multi_line_field() {
        let mut multi = TextField::new("a", true);
        assert_eq!(multi.key(ctrl('j')), FieldKey::Changed);
        typed(&mut multi, "b");
        assert_eq!(multi.text(), "a\nb");

        let mut one = TextField::new("a", false);
        assert_eq!(one.key(ctrl('j')), FieldKey::Unchanged);
        assert_eq!(one.key(ctrl('m')), FieldKey::Unchanged, "nor ctrl-m");
        typed(&mut one, "b");
        assert_eq!(one.text(), "ab");
    }

    #[test]
    fn a_paste_turns_every_line_break_into_a_newline() {
        let mut field = TextField::new("", true);

        assert!(field.paste("one\r\ntwo\rthree\nfour"));

        assert_eq!(field.text(), "one\ntwo\nthree\nfour");
        assert_eq!(field.cursor(), (3, 4));
    }

    #[test]
    fn a_paste_into_a_one_line_field_puts_blanks_for_its_line_breaks() {
        let mut field = TextField::new("ab", false);
        field.key(code(KeyCode::Left));

        assert!(field.paste("x\ny\r\nz\rw"));

        assert_eq!(field.text(), "ax y z wb");
        assert!(!field.paste(""), "an empty paste changes nothing");
    }

    // ---------------------------------------------------------------
    // Drawing
    // ---------------------------------------------------------------

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
        let mut field = TextField::new("ab", true);
        field.key(code(KeyCode::Left));
        let warn = limits(None, Some(10));

        let lines = field.lines(Span::from("> "), 40, true, Some(&warn));

        assert_eq!(
            cursor_columns(&lines),
            [("> ab".to_owned(), Some(3)), ("2/10".to_owned(), None)]
        );
        assert_eq!(lines[1].alignment, Some(ratatui::layout::Alignment::Right));
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Green));
    }

    #[test]
    fn the_counter_takes_the_colour_of_its_level() {
        let field = TextField::new(&"x".repeat(12), false);

        let lines = field.lines(Span::from("> "), 40, true, Some(&limits(None, Some(10))));

        assert_eq!(texts(&lines[1..]), ["12/10 !!"]);
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Red));
        assert_eq!(Level::Warn.colour(), Color::Yellow);
    }

    #[test]
    fn a_field_with_the_keys_but_without_its_cursor_shows_no_dash() {
        let field = TextField::new("", false);

        let lines = field.lines(Span::from("> "), 40, false, None);

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
        let field = TextField::new("abcd", false);
        assert_eq!(field.lines(head(), 6, true, None).len(), 2);
    }

    #[test]
    fn the_text_cursor_after_a_full_line_starts_the_next_one() {
        let line = with_cursor("ab cd", Some((0, 5))).remove(0);

        let lines = wrap(line, 5, 0, 1);

        assert_eq!(
            cursor_columns(&lines),
            [("ab cd".to_owned(), None), ("  ".to_owned(), Some(1))]
        );
    }

    #[test]
    fn the_text_cursor_on_the_space_at_a_line_break_starts_the_next_line() {
        let line = with_cursor("abcd efg", Some((0, 4))).remove(0);

        let lines = wrap(line, 4, 0, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("abcd".to_owned(), None), (" efg".to_owned(), Some(0))]
        );
    }

    #[test]
    fn the_text_cursor_inside_a_word_keeps_the_word_whole() {
        let line = with_cursor("ab channel", Some((0, 3))).remove(0);

        let lines = wrap(line, 8, 0, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("ab ".to_owned(), None), ("channel".to_owned(), Some(0))]
        );
    }

    #[test]
    fn the_text_cursor_on_a_wide_character_moves_with_it_to_the_next_line() {
        let line = with_cursor("日本語", Some((0, 2))).remove(0);
        assert_eq!(
            cursor_columns(&wrap(line, 5, 0, 1)),
            [("日本".to_owned(), None), (" 語".to_owned(), Some(1))]
        );

        let line = with_cursor("日本", Some((0, 2))).remove(0);
        assert_eq!(
            cursor_columns(&wrap(line, 4, 0, 1)),
            [("日本".to_owned(), None), ("  ".to_owned(), Some(1))],
            "after a line full of wide characters"
        );
    }

    #[test]
    fn the_text_cursor_covers_the_whole_grapheme_its_character_is_in() {
        // ratatui-textarea moves its cursor by characters, so it can stop
        // on a combining accent or on the skin tone of an emoji.
        let accent = with_cursor("ce\u{301}x", Some((0, 2))).remove(0);
        assert_eq!(
            accent.spans,
            [
                Span::from("c"),
                Span::from("e\u{301}").reversed(),
                Span::from("x")
            ]
        );

        let emoji = with_cursor("👍🏽!", Some((0, 1))).remove(0);
        assert_eq!(
            emoji.spans,
            [Span::from(""), Span::from("👍🏽").reversed(), Span::from("!")]
        );
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
