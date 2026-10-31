// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! patine's style-neutral lines (ADR 34) as asqr draws them: its styles as
//! ratatui styles, its lines as [`SourceLine`]s, and the one rule for
//! laying out Markdown that patine rendered, wherever asqr shows it. A
//! line wider than the space it gets wraps, and each further row starts
//! with a dim `↪`, so nothing is cut off.

use patine::{AnsiColor, QuoteStyle, StyledLine, TextStyle, Underline};
use ratatui::style::{Color, Modifier, Style};

use super::source_line::{SourceLine, columns};
use super::theme;

/// What starts a row that continues a line too wide for its space.
const CONTINUATION: &str = "↪ ";

/// The most columns one grapheme takes, as a CJK character or most emoji
/// do.
pub(super) const WIDEST_GRAPHEME: usize = 2;

/// How Markdown quotes look wherever asqr renders Markdown: a blue bar,
/// apart from the cyan that marks what can be acted on, and a dim one
/// under `NO_COLOR`. The colours come off the finished screen there and
/// dim stays (spec section 7.3), so a blue bar would be left in the text
/// colour, like the quoted text beside it.
pub(super) const fn quote_style(no_color: bool) -> QuoteStyle {
    if no_color {
        QuoteStyle::DimBar
    } else {
        QuoteStyle::Bar(AnsiColor::Blue)
    }
}

/// Whether the screen draws colours, read off the quote style it asks
/// for: the inverse of how [`quote_style`] is chosen.
pub(super) fn colours_on(quote: QuoteStyle) -> bool {
    quote != quote_style(true)
}

/// What patine draws in place of a bullet for a task list item, open or
/// done, and after the number of an ordered one.
const TASK_BOXES: [&str; 2] = ["☐ ", "☑ "];

/// The ratatui style of a patine style.
///
/// ratatui has a single underline only, so a double one, which patine
/// gives level-one headings, becomes single; patine makes those headings
/// italic as well, so they stay apart from level-two ones. A colour stays
/// a palette slot, `Color::Indexed`, so the terminal's theme decides it
/// and index 7 stays patine's white, which ratatui would call gray.
///
/// These styles take no role from [`super::theme`]: code and Markdown keep
/// the colours their highlighting gives them (ADR 34).
// `TextStyle` may gain attributes in a minor release of patine; this
// function shows only the ones it names and ignores any others.
fn style(style: TextStyle) -> Style {
    let flags = [
        (style.bold, Modifier::BOLD),
        (style.italic, Modifier::ITALIC),
        (style.dim, Modifier::DIM),
        (style.strikethrough, Modifier::CROSSED_OUT),
        (style.underline != Underline::None, Modifier::UNDERLINED),
    ];
    let modifier = flags
        .into_iter()
        .filter(|(set, _)| *set)
        .fold(Modifier::empty(), |all, (_, one)| all | one);
    let base = Style::new().add_modifier(modifier);
    match style.foreground {
        Some(color) => base.fg(Color::Indexed(color.index())),
        None => base,
    }
}

/// Whether any of `lines` has a span with a style, which tells highlighted
/// code from lines patine left plain, as it does for a language it does not
/// know.
pub(super) fn is_coloured(lines: &[StyledLine]) -> bool {
    lines
        .iter()
        .flat_map(|line| &line.spans)
        .any(|span| span.style != TextStyle::default())
}

/// A patine line as a [`SourceLine`], which expands its tabs, shows its
/// control characters and measures it in columns (ADR 33).
pub(super) fn source_line(line: &StyledLine) -> SourceLine {
    SourceLine::new(
        line.spans
            .iter()
            .map(|span| (span.text.as_str(), style(span.style))),
    )
}

/// The rows that show Markdown rendered by patine in `width` columns.
///
/// patine wraps prose and lays out a table that does not fit as records,
/// but leaves some lines wider: code blocks and frontmatter, a word or URL
/// in running text longer than the width, a record's line where one
/// grapheme, such as a wide character, is wider than the room it gets,
/// and any line where quote bars and indents leave no room. Such a line
/// wraps here: each further row repeats the line's indent, the
/// blanks and blockquote bars it starts with and a list item's marker as
/// blanks (see [`indent`]), so a code block's lines stay inside the block
/// and an item's text inside the item, then a dim `↪` and the next part.
/// Where the indent leaves no room for a wide character after the marker,
/// the rows start with the marker alone, and where not even that fits,
/// the line breaks without one. So every row fits `width` from 2 columns
/// on.
pub(super) fn markdown_rows(lines: &[StyledLine], width: usize) -> Vec<SourceLine> {
    markdown_rows_from(lines, width, width)
}

/// The rows that show Markdown rendered by patine with its first line at
/// most `first` columns wide and every later line `width` (ADR 79), laid
/// out as [`markdown_rows`] lays them out. The first row fits `first`,
/// every later one `width`, both from 2 columns on: a first line patine
/// leaves wider than `first`, such as a line of a code block or an
/// overlong first word, breaks there, and its further rows take the full
/// width behind the `↪`.
pub(super) fn markdown_rows_from(
    lines: &[StyledLine],
    first: usize,
    width: usize,
) -> Vec<SourceLine> {
    wrapped_rows(lines, first, width, indent)
}

/// The rows that show `lines`, text that is no Markdown, in `width`
/// columns. A line wider than `width` wraps behind a dim `↪` as in
/// [`markdown_rows`], but its further rows repeat only the blanks the line
/// starts with: a `1. ` or a `• ` in plain text is no list item to line
/// the rest up under.
pub(super) fn plain_rows(lines: &[StyledLine], width: usize) -> Vec<SourceLine> {
    plain_rows_from(lines, width, width)
}

/// The rows that show `lines` as [`plain_rows`] does, with the first line
/// at most `first` columns wide and every later one `width` (ADR 79).
pub(super) fn plain_rows_from(lines: &[StyledLine], first: usize, width: usize) -> Vec<SourceLine> {
    wrapped_rows(lines, first, width, leading_blanks)
}

/// What the continuation rows of a line repeat before their marker, in
/// their styles, and its width.
type Indent = (Vec<(String, Style)>, usize);

/// The rows of `lines` with the first line at most `first` columns wide and
/// every later one `width`, a wider line wrapping behind a `↪` after the
/// indent `indent_of` finds for it.
fn wrapped_rows(
    lines: &[StyledLine],
    first: usize,
    width: usize,
    indent_of: fn(&SourceLine) -> Indent,
) -> Vec<SourceLine> {
    let marker = theme::secondary();
    let marker_width = 2;
    let mut rows = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let line = source_line(line);
        let first = if index == 0 { first } else { width };
        if line.width() <= first {
            rows.push(line);
            continue;
        }
        let (indent, indent_width) = indent_of(&line);
        // A row behind the prefix holds at least one grapheme, so a
        // prefix leaving less than a wide character's columns would push
        // that character past the edge.
        let (indent, prefix_width) = if indent_width + marker_width + WIDEST_GRAPHEME <= width {
            (indent, indent_width + marker_width)
        } else if marker_width + WIDEST_GRAPHEME <= width {
            (Vec::new(), marker_width)
        } else {
            rows.extend(line.wrap(first, width));
            continue;
        };
        let mut parts = line.wrap(first, width - prefix_width).into_iter();
        rows.extend(parts.next());
        for part in parts {
            let prefix = indent
                .iter()
                .map(|(text, style)| (text.as_str(), *style))
                .chain([(CONTINUATION, marker)]);
            rows.push(SourceLine::new(prefix.chain(part.pieces())));
        }
    }
    rows
}

/// What the continuation rows of `line` repeat before their marker, in
/// its styles, and its width: the blanks and blockquote bars the line
/// starts with, then the marker of a list item turned into blanks, so the
/// rest of an item lines up under its text rather than under its marker.
///
/// A bullet, `• `, or the box of a task, `☐ ` or `☑ `, counts after any
/// blanks and bars, as patine indents a nested item with blanks. A number,
/// such as `1. ` or `10. `, counts only where the line starts, quote bars
/// aside: there patine starts an ordered item, while a line of a code
/// block starts with blanks, and its `1. ` is code. A nested numbered item
/// therefore continues under its number. The box of an ordered task comes
/// after its number, `1. ☐ `, and counts with it.
fn indent(line: &SourceLine) -> Indent {
    let mut indent = Vec::new();
    let mut lead = String::new();
    let mut rest = String::new();
    for (text, style) in line.pieces() {
        if !rest.is_empty() {
            rest.push_str(text);
            continue;
        }
        let blanks_and_bars: String = text
            .chars()
            .take_while(|c| matches!(c, ' ' | '│'))
            .collect();
        rest.push_str(&text[blanks_and_bars.len()..]);
        lead.push_str(&blanks_and_bars);
        if !blanks_and_bars.is_empty() {
            indent.push((blanks_and_bars, style));
        }
    }
    let mut width = lead.chars().count();

    // The width of the task box `text` starts with, if it starts with one.
    let task_box = |text: &str| {
        TASK_BOXES
            .into_iter()
            .find(|task_box| text.starts_with(task_box))
            .map(columns)
    };
    let marker = if rest.starts_with("• ") {
        columns("• ")
    } else if let Some(width) = task_box(&rest) {
        width
    } else if only_quote_bars(&lead) {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(". ") {
            digits + 2 + task_box(&rest[digits + 2..]).unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };
    if marker > 0 {
        indent.push((" ".repeat(marker), Style::new()));
        width += marker;
    }
    (indent, width)
}

/// What the continuation rows of plain text repeat: the blanks `line`
/// starts with, in their style, and their width. Plain text is one span a
/// line, so its blanks sit at the start of the first piece.
fn leading_blanks(line: &SourceLine) -> Indent {
    let blanks = line.pieces().next().map(|(text, style)| {
        let run = text.chars().take_while(|c| *c == ' ').count();
        (run, style)
    });
    match blanks {
        Some((run, style)) if run > 0 => (vec![(" ".repeat(run), style)], run),
        _ => (Vec::new(), 0),
    }
}

/// Whether `lead` is nothing but blockquote bars, each followed by at most
/// the one blank patine puts after it.
fn only_quote_bars(lead: &str) -> bool {
    let mut previous = None;
    lead.chars().all(|c| {
        let bar_or_its_blank = c == '│' || previous == Some('│');
        previous = Some(c);
        bar_or_its_blank
    })
}

#[cfg(test)]
mod tests {
    use patine::{AnsiColor, StyledSpan};

    use super::*;

    #[test]
    fn colours_on_is_the_inverse_of_the_quote_style() {
        assert!(colours_on(quote_style(false)));
        assert!(!colours_on(quote_style(true)));
    }

    fn text_style(change: impl FnOnce(&mut TextStyle)) -> TextStyle {
        let mut style = TextStyle::default();
        change(&mut style);
        style
    }

    /// The text of each row.
    fn texts(rows: &[SourceLine]) -> Vec<String> {
        rows.iter()
            .map(|row| row.pieces().map(|(text, _)| text).collect())
            .collect()
    }

    fn line(spans: &[(&str, TextStyle)]) -> StyledLine {
        StyledLine::new(
            spans
                .iter()
                .map(|(text, style)| StyledSpan::new(*text, *style))
                .collect(),
        )
    }

    #[test]
    fn every_attribute_maps_to_its_modifier() {
        assert_eq!(style(TextStyle::default()), Style::new());
        assert_eq!(
            style(text_style(|s| s.bold = true)),
            Style::new().add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            style(text_style(|s| s.italic = true)),
            Style::new().add_modifier(Modifier::ITALIC)
        );
        assert_eq!(
            style(text_style(|s| s.dim = true)),
            Style::new().add_modifier(Modifier::DIM)
        );
        assert_eq!(
            style(text_style(|s| s.strikethrough = true)),
            Style::new().add_modifier(Modifier::CROSSED_OUT)
        );
        assert_eq!(
            style(text_style(|s| {
                s.bold = true;
                s.dim = true;
            })),
            Style::new().add_modifier(Modifier::BOLD | Modifier::DIM),
            "they combine"
        );
    }

    #[test]
    fn single_and_double_underlines_are_both_underlined() {
        for underline in [Underline::Single, Underline::Double] {
            assert_eq!(
                style(text_style(|s| s.underline = underline)),
                Style::new().add_modifier(Modifier::UNDERLINED),
                "{underline:?}"
            );
        }
    }

    #[test]
    fn colours_stay_palette_slots() {
        assert_eq!(
            style(text_style(|s| s.foreground = Some(AnsiColor::Yellow))),
            Style::new().fg(Color::Indexed(3))
        );
        assert_eq!(
            style(text_style(|s| s.foreground = Some(AnsiColor::White))).fg,
            Some(Color::Indexed(7)),
            "patine's white, not ratatui's gray"
        );
    }

    #[test]
    fn a_line_keeps_its_spans_styles_and_expands_its_tabs() {
        let keyword = text_style(|s| s.foreground = Some(AnsiColor::Magenta));
        let line = source_line(&line(&[("\tlet", keyword), (" x", TextStyle::default())]));

        assert_eq!(
            line.pieces().collect::<Vec<_>>(),
            [
                ("    let", Style::new().fg(Color::Indexed(5))),
                (" x", Style::new())
            ]
        );
        assert_eq!(line.width(), 9);
    }

    /// Asserts that asqr measures `text` as patine laid it out: through
    /// [`columns`], and as the [`SourceLine`] of a patine line, which is
    /// what [`markdown_rows`] compares with the width.
    fn assert_measured_alike(text: &str) {
        let span = StyledSpan::new(text, TextStyle::default());
        assert_eq!(columns(text), span.width(), "columns");
        let line = StyledLine::new(vec![span]);
        assert_eq!(source_line(&line).width(), line.width(), "a source line");
    }

    /// `ل` and `ا` are graphemes of their own, each drawn in a column.
    #[test]
    fn widths_agree_with_patine_for_a_ligature() {
        assert_measured_alike("لا");
    }

    #[test]
    fn widths_agree_with_patine_for_a_halfwidth_sound_mark() {
        assert_measured_alike("ｶﾞ");
    }

    /// [`columns`] counts no column for an escape, as ratatui draws none,
    /// but a source line shows it as its picture `␛` in the one column
    /// patine counts.
    #[test]
    fn a_control_character_measures_alike_in_patine_and_a_source_line() {
        let line = StyledLine::new(vec![StyledSpan::new("\u{1b}", TextStyle::default())]);

        assert_eq!(source_line(&line).width(), line.width());
    }

    /// The rows of one patine line of plain spans at `width` columns.
    fn rows_of(spans: &[&str], width: usize) -> Vec<String> {
        let spans: Vec<_> = spans
            .iter()
            .map(|text| (*text, TextStyle::default()))
            .collect();
        texts(&markdown_rows(&[line(&spans)], width))
    }

    #[test]
    fn a_continuation_of_a_bullet_item_lines_up_under_its_text() {
        let word = "a".repeat(50);

        assert_eq!(
            rows_of(&["• ", &word], 40),
            [
                format!("• {}", "a".repeat(38)),
                format!("  ↪ {}", "a".repeat(12))
            ]
        );
        assert_eq!(
            rows_of(&["  • ", &word], 40),
            [
                format!("  • {}", "a".repeat(36)),
                format!("    ↪ {}", "a".repeat(14))
            ],
            "a nested item"
        );
        assert_eq!(
            rows_of(&["│ ", "• ", &word], 40),
            [
                format!("│ • {}", "a".repeat(36)),
                format!("│   ↪ {}", "a".repeat(14))
            ],
            "an item in a quote"
        );
    }

    #[test]
    fn a_continuation_of_a_task_item_lines_up_under_its_text() {
        let word = "a".repeat(50);

        assert_eq!(
            rows_of(&["☐ ", &word], 40),
            [
                format!("☐ {}", "a".repeat(38)),
                format!("  ↪ {}", "a".repeat(12))
            ],
            "an open task"
        );
        assert_eq!(
            rows_of(&["☑ ", &word], 40),
            [
                format!("☑ {}", "a".repeat(38)),
                format!("  ↪ {}", "a".repeat(12))
            ],
            "a done task"
        );
        assert_eq!(
            rows_of(&["  ☐ ", &word], 40),
            [
                format!("  ☐ {}", "a".repeat(36)),
                format!("    ↪ {}", "a".repeat(14))
            ],
            "a nested task"
        );
        assert_eq!(
            rows_of(&["│ ", "☑ ", &word], 40),
            [
                format!("│ ☑ {}", "a".repeat(36)),
                format!("│   ↪ {}", "a".repeat(14))
            ],
            "a task in a quote"
        );
        assert_eq!(
            rows_of(&["1. ☐ ", &word], 40),
            [
                format!("1. ☐ {}", "a".repeat(35)),
                format!("     ↪ {}", "a".repeat(15))
            ],
            "a numbered task, its box after the number"
        );
        assert_eq!(
            rows_of(&["10. ☑ ", &word], 40),
            [
                format!("10. ☑ {}", "a".repeat(34)),
                format!("      ↪ {}", "a".repeat(16))
            ]
        );
    }

    #[test]
    fn quotes_have_a_blue_bar_and_a_dim_one_without_colours() {
        assert_eq!(quote_style(false), QuoteStyle::Bar(AnsiColor::Blue));
        assert_eq!(quote_style(true), QuoteStyle::DimBar);
    }

    #[test]
    fn a_continuation_of_a_numbered_item_lines_up_under_its_text() {
        let word = "a".repeat(50);

        assert_eq!(
            rows_of(&["1. ", &word], 40),
            [
                format!("1. {}", "a".repeat(37)),
                format!("   ↪ {}", "a".repeat(13))
            ]
        );
        assert_eq!(
            rows_of(&["10. ", &word], 40),
            [
                format!("10. {}", "a".repeat(36)),
                format!("    ↪ {}", "a".repeat(14))
            ]
        );
        assert_eq!(
            rows_of(&["│ ", "3. ", &word], 40),
            [
                format!("│ 3. {}", "a".repeat(35)),
                format!("│    ↪ {}", "a".repeat(15))
            ],
            "an item in a quote"
        );
    }

    #[test]
    fn a_number_after_blanks_is_no_item() {
        let word = "a".repeat(50);
        let lines = patine::render_lines(&format!("```\n1. {word}\n```\n"), 40);

        assert_eq!(
            texts(&markdown_rows(&lines, 40)),
            [
                format!("  1. {}", "a".repeat(35)),
                format!("  ↪ {}", "a".repeat(15))
            ],
            "a line of a code block that starts with a number"
        );
        assert_eq!(
            rows_of(&["1.", &word], 40)[1],
            format!("↪ {}", "a".repeat(12)),
            "a number without its blank is no marker either"
        );
    }

    /// One patine line of plain `text`.
    fn plain(text: &str) -> StyledLine {
        line(&[(text, TextStyle::default())])
    }

    #[test]
    fn a_first_line_wider_than_its_first_width_wraps_there_then_at_the_width() {
        let rows = markdown_rows_from(&[plain("  abcdefghijklmnopqrstuvwxyz")], 10, 16);

        assert_eq!(
            texts(&rows),
            ["  abcdefgh", "  ↪ ijklmnopqrst", "  ↪ uvwxyz"]
        );
    }

    #[test]
    fn a_first_line_within_the_width_but_not_its_first_width_wraps() {
        let rows = markdown_rows_from(&[plain("abcdefghijklmn")], 10, 16);

        assert_eq!(texts(&rows), ["abcdefghij", "↪ klmn"]);
    }

    /// Guard: the lines after the first keep the full width.
    #[test]
    fn only_the_first_line_gets_the_first_width() {
        let lines = [plain("abcdefghijklmn"), plain("opqrstuvwxyzab")];

        assert_eq!(
            texts(&markdown_rows_from(&lines, 10, 16)).last(),
            Some(&"opqrstuvwxyzab".to_owned())
        );
    }

    #[test]
    fn lines_that_fit_stay_as_they_are() {
        let lines = patine::render_lines("Some text that wraps around nicely here.", 24);

        assert_eq!(
            texts(&markdown_rows(&lines, 24)),
            ["Some text that wraps", "around nicely here."]
        );
    }

    #[test]
    fn a_wide_code_line_wraps_inside_its_block_behind_a_dim_marker() {
        let lines = patine::render_lines("```\nlet total = first + second + third;\n```\n", 20);

        let rows = markdown_rows(&lines, 20);

        assert_eq!(
            texts(&rows),
            ["  let total = first ", "  ↪ + second + third", "  ↪ ;"]
        );
        let marker: Vec<_> = rows[1].pieces().collect();
        assert_eq!(marker[1], ("↪ ", Style::new().dim()));
        assert_eq!(
            marker[2].1,
            Style::new().fg(Color::Indexed(3)),
            "the code keeps its colour"
        );
        assert!(rows.iter().all(|row| row.width() <= 20));
    }

    #[test]
    fn a_continuation_repeats_blockquote_bars_in_their_style() {
        let bar = text_style(|s| s.foreground = Some(AnsiColor::Blue));
        let lines = [line(&[
            ("│ ", bar),
            ("https://example.com/long", TextStyle::default()),
        ])];

        let rows = markdown_rows(&lines, 16);

        assert_eq!(texts(&rows), ["│ https://exampl", "│ ↪ e.com/long"]);
        assert_eq!(
            rows[1].pieces().next(),
            Some(("│ ", Style::new().fg(Color::Indexed(4))))
        );
    }

    /// Asserts that the rows of one patine line of plain `spans` stay
    /// within each of `widths`.
    fn assert_within(spans: &[&str], widths: std::ops::RangeInclusive<usize>) {
        let spans: Vec<_> = spans
            .iter()
            .map(|text| (*text, TextStyle::default()))
            .collect();
        for width in widths {
            let rows = markdown_rows(&[line(&spans)], width);
            assert!(
                rows.iter().all(|row| row.width() <= width),
                "at {width}: {:?}",
                texts(&rows)
            );
        }
    }

    #[test]
    fn a_wide_character_after_the_marker_stays_within_the_width() {
        assert_within(&["日本語の説明"], 2..=8);
    }

    #[test]
    fn a_wide_character_after_the_indent_and_the_marker_stays_within_the_width() {
        assert_within(&["  • ", "日本語の説明"], 2..=10);
    }

    #[test]
    fn without_room_for_the_indent_the_marker_stands_alone_or_goes() {
        let lines = [line(&[("      abcdefghij", TextStyle::default())])];

        assert_eq!(
            texts(&markdown_rows(&lines, 8)),
            ["      ab", "↪ cdefgh", "↪ ij"],
            "the indent would leave no room"
        );
        assert_eq!(
            texts(&markdown_rows(&lines, 2)),
            ["  ", "  ", "  ", "ab", "cd", "ef", "gh", "ij"],
            "neither fits"
        );
    }
}
