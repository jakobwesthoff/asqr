// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Text laid out in rows of a given width: word-wrapping a styled line,
//! indenting it and cutting it short. The screen's parts and the typed
//! fields wrap by these.

use std::ops::Range;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

use super::source_line::{columns, fitting};

/// Word-wraps `line` to `width` columns: the first line is indented by
/// `first`, the following ones by `rest`, as [`wrap_rows`] breaks it.
pub(super) fn wrap(
    line: Line<'static>,
    width: usize,
    first: usize,
    rest: usize,
) -> Vec<Line<'static>> {
    let glyphs: Vec<Glyph> = line
        .spans
        .iter()
        .flat_map(|span| {
            let style = line.style.patch(span.style);
            span.content
                .graphemes(true)
                .map(move |grapheme| Glyph::new(grapheme, style))
        })
        .collect();
    wrap_rows(&glyphs, width, first, rest)
        .into_iter()
        .map(|(indent, row)| styled_line(indent, &glyphs[row]))
        .collect()
}

/// Where `glyphs` break into rows of `width` columns: each row's indent
/// and the glyphs it takes, in order, the first row indented by `first`
/// and the following ones by `rest`. Every glyph lands on exactly one row.
///
/// A word is a run of non-blanks with the blanks after it, whatever
/// styles its glyphs carry, so a style change inside a word, such as the
/// text cursor, never splits it. A word longer than a row is broken where
/// the row ends.
pub(crate) fn wrap_rows(
    glyphs: &[Glyph],
    width: usize,
    first: usize,
    rest: usize,
) -> Vec<(usize, Range<usize>)> {
    let mut words: Vec<Range<usize>> = Vec::new();
    for (index, glyph) in glyphs.iter().enumerate() {
        let after_blank = words
            .last()
            .is_none_or(|word| glyphs[word.end - 1].is_blank());
        if words.is_empty() || (!glyph.is_blank() && after_blank) {
            words.push(index..index);
        }
        words.last_mut().expect("a word was started").end = index + 1;
    }

    let mut wrapped = Wrapped {
        rows: vec![(first, 0..0)],
        used: first,
        rest,
    };
    for word in words {
        let blanks = glyphs[word.clone()]
            .iter()
            .rev()
            .take_while(|glyph| glyph.is_blank())
            .count();
        let mut body = word.start..word.end - blanks;
        while !body.is_empty() {
            let indent = if wrapped.rows.len() == 1 { first } else { rest };
            if wrapped.used + Glyph::columns(&glyphs[body.clone()]) <= width {
                wrapped.push(glyphs, body.end);
                break;
            }
            if wrapped.used > indent {
                wrapped.break_row();
                continue;
            }
            // A word longer than a whole row: break it where the row ends.
            // A wide character that would cross the end starts the next
            // row whole, and every row takes at least one grapheme, so one
            // wider than the whole row still makes progress.
            let mut used = wrapped.used;
            let fits = glyphs[body.clone()]
                .iter()
                .take_while(|glyph| {
                    used += glyph.width;
                    used <= width
                })
                .count();
            let split = body.start + fits.max(1);
            wrapped.push(glyphs, split);
            if split < body.end {
                wrapped.break_row();
            }
            body = split..body.end;
        }
        // Blanks may hang past the end of a row, so the next row does not
        // start with them. A blank that shows is the exception: the text
        // cursor's cell or part of a filled head. It starts the next row
        // when this one is full.
        for blank in word.end - blanks..word.end {
            if glyphs[blank].visible && wrapped.used >= width {
                wrapped.break_row();
            }
            wrapped.push(glyphs, blank + 1);
        }
    }
    wrapped.rows
}

/// One grapheme of a line to wrap, with its style and the columns it
/// takes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Glyph<'a> {
    pub(crate) text: &'a str,
    pub(crate) style: Style,
    pub(crate) width: usize,
    /// Whether the glyph shows even where it is a blank, as the text
    /// cursor's cell and the blanks of a filled head do. Wrapping goes by
    /// this, never by the style, so a fill can take any look.
    pub(crate) visible: bool,
}

impl<'a> Glyph<'a> {
    pub(crate) fn new(text: &'a str, style: Style) -> Self {
        Glyph {
            text,
            style,
            width: columns(text),
            visible: false,
        }
    }

    /// A glyph that shows even where it is a blank: the text cursor's cell
    /// or part of a filled head.
    pub(crate) fn shown(text: &'a str, style: Style) -> Self {
        Glyph {
            visible: true,
            ..Glyph::new(text, style)
        }
    }

    fn is_blank(&self) -> bool {
        self.text == " "
    }

    /// The columns `glyphs` take together.
    fn columns(glyphs: &[Glyph]) -> usize {
        glyphs.iter().map(|glyph| glyph.width).sum()
    }
}

/// The rows `wrap_rows` builds: each one's indent and its glyphs.
struct Wrapped {
    rows: Vec<(usize, Range<usize>)>,
    /// The columns taken on the last row, its indent included.
    used: usize,
    rest: usize,
}

impl Wrapped {
    /// Extends the last row up to glyph `end`.
    fn push(&mut self, glyphs: &[Glyph], end: usize) {
        let (_, row) = self.rows.last_mut().expect("there is always a row");
        self.used += Glyph::columns(&glyphs[row.end..end]);
        row.end = end;
    }

    fn break_row(&mut self) {
        let end = self.rows.last().expect("there is always a row").1.end;
        self.rows.push((self.rest, end..end));
        self.used = self.rest;
    }
}

/// A row of `glyphs` behind `indent` blanks: the indent as a span of its
/// own, which `ending_in` keeps, then one span per run of a style.
pub(crate) fn styled_line(indent: usize, glyphs: &[Glyph]) -> Line<'static> {
    let mut runs: Vec<(String, Style)> = Vec::new();
    for glyph in glyphs {
        match runs.last_mut() {
            Some((text, style)) if *style == glyph.style => text.push_str(glyph.text),
            _ => runs.push((glyph.text.to_owned(), glyph.style)),
        }
    }
    let mut spans = vec![Span::from(" ".repeat(indent))];
    spans.extend(
        runs.into_iter()
            .map(|(text, style)| Span::styled(text, style)),
    );
    Line::from(spans)
}

/// `line` behind `columns` blanks.
pub(super) fn indented(line: Line<'static>, columns: usize) -> Line<'static> {
    let mut spans = vec![Span::from(" ".repeat(columns))];
    spans.extend(line.spans);
    Line::from(spans).style(line.style)
}

/// `text` in at most `room` columns, ending in `…` when it had to be cut.
pub(super) fn cut(text: &str, room: usize) -> String {
    if columns(text) <= room {
        return text.to_owned();
    }
    if room == 0 {
        return String::new();
    }
    format!("{}…", fitting(text, room - 1))
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Modifier, Stylize};

    use super::super::test_support::line_texts;
    use super::*;

    #[test]
    fn wrapping_breaks_words_longer_than_a_line() {
        let lines = wrap(Line::from("abcdefghij"), 4, 0, 1);
        let text: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();

        assert_eq!(text, ["abcd", " efg", " hij"]);
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
    fn a_word_styled_in_parts_wraps_as_one_word() {
        let line = Line::from(vec![
            Span::from("ab "),
            Span::from("cha").bold(),
            Span::from("nnel"),
        ]);

        let lines = wrap(line, 8, 0, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("ab ".to_owned(), None), ("channel".to_owned(), None)]
        );
    }

    #[test]
    fn a_word_ending_at_the_end_of_a_line_stays_on_it() {
        assert_eq!(
            cursor_columns(&wrap(Line::from("ab cd ef"), 5, 0, 0)),
            [("ab cd ".to_owned(), None), ("ef".to_owned(), None)]
        );
        assert_eq!(
            cursor_columns(&wrap(Line::from("abcd efg"), 4, 0, 0)),
            [("abcd ".to_owned(), None), ("efg".to_owned(), None)],
            "the next line does not start with the blank"
        );
    }

    #[test]
    fn plain_spaces_still_hang_past_the_end_of_a_line() {
        let lines = wrap(Line::from("ab  cd"), 3, 0, 0);

        assert_eq!(
            cursor_columns(&lines),
            [("ab  ".to_owned(), None), ("cd".to_owned(), None)]
        );
    }

    #[test]
    fn a_blank_starts_the_next_row_by_whether_it_shows_not_by_its_style() {
        // "abcd" fills a row of 4, so the blank after it lies past the end.
        let rows = |blank: Glyph<'static>| {
            let mut glyphs: Vec<Glyph> = "abcd"
                .graphemes(true)
                .map(|grapheme| Glyph::new(grapheme, Style::new()))
                .collect();
            glyphs.extend([blank, Glyph::new("e", Style::new())]);
            wrap_rows(&glyphs, 4, 0, 0)
        };

        assert_eq!(
            rows(Glyph::shown(" ", Style::new())),
            [(0, 0..4), (0, 4..6)],
            "a blank that shows starts the next row, whatever its style"
        );
        assert_eq!(
            rows(Glyph::new(" ", Style::new().reversed())),
            [(0, 0..5), (0, 5..6)],
            "a reversed blank that does not show hangs past the end"
        );
    }

    #[test]
    fn wrapping_measures_wide_characters_in_columns() {
        let wrapped =
            |text: &'static str, width, rest| line_texts(&wrap(Line::from(text), width, 0, rest));

        assert_eq!(
            wrapped("日本語🎉です", 6, 0),
            ["日本語", "🎉です"],
            "語 ends at the edge"
        );
        assert_eq!(
            wrapped("日本語🎉です", 5, 0),
            ["日本", "語🎉", "です"],
            "語 would cross the edge and moves on whole"
        );
        assert_eq!(wrapped("ab 日本 語", 5, 1), ["ab ", " 日本 ", " 語"]);
        assert_eq!(
            wrapped("日本", 1, 0),
            ["日", "本"],
            "a character wider than the line still goes on"
        );
    }

    #[test]
    fn cutting_keeps_text_within_its_room() {
        assert_eq!(cut("Changelog, Blog post", 30), "Changelog, Blog post");
        assert_eq!(cut("Changelog, Blog post", 12), "Changelog, …");
        assert_eq!(cut("Changelog", 1), "…");
        assert_eq!(cut("Changelog", 0), "");
    }

    #[test]
    fn cutting_counts_columns_and_keeps_wide_characters_whole() {
        assert_eq!(cut("日本語の説明", 12), "日本語の説明", "it fits exactly");
        assert_eq!(
            cut("日本語の説明", 11),
            "日本語の説…",
            "説 ends where `…` starts"
        );
        assert_eq!(
            cut("日本語の説明", 10),
            "日本語の…",
            "説 would cross into it"
        );
        assert_eq!(cut("🎉🎉🎉", 4), "🎉…");
    }
}
