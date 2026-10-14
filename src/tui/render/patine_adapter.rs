// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! patine's style-neutral lines (ADR 34) as asqr draws them: its styles as
//! ratatui styles, its lines as [`SourceLine`]s, and the one rule for
//! laying out Markdown that patine rendered, wherever asqr shows it. A
//! line wider than the space it gets wraps, and each further row starts
//! with a dim `↪`, so nothing is cut off.

use patine::{StyledLine, TextStyle, Underline};
use ratatui::style::{Color, Modifier, Style};

use super::source_line::{SourceLine, columns};

/// What starts a row that continues a line too wide for its space.
const CONTINUATION: &str = "↪ ";

/// The ratatui style of a patine style.
///
/// ratatui has a single underline only, so a double one, which patine
/// gives level-one headings, becomes single; patine makes those headings
/// italic as well, so they stay apart from level-two ones. A colour stays
/// a palette slot, `Color::Indexed`, so the terminal's theme decides it
/// and index 7 stays patine's white, which ratatui would call gray.
// `TextStyle` may gain attributes in a minor release of patine; this
// function shows only the ones it names and ignores any others.
pub fn style(style: TextStyle) -> Style {
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

/// A patine line as a [`SourceLine`], which expands its tabs, shows its
/// control characters and measures it in columns (ADR 33).
pub fn source_line(line: &StyledLine) -> SourceLine {
    SourceLine::new(
        line.spans
            .iter()
            .map(|span| (span.text.as_str(), style(span.style))),
    )
}

/// The rows that show Markdown rendered by patine in `width` columns.
///
/// patine wraps prose, but leaves some lines wider: code blocks and
/// frontmatter, a word or URL longer than the width, a table that cannot
/// shrink further. Such a line wraps here: each further row repeats the
/// line's indent, the blanks and blockquote bars it starts with and a list
/// item's marker as blanks (see [`indent`]), so a code block's lines stay
/// inside the block and an item's text inside the item, then a dim `↪`
/// and the next part. Where the indent leaves no room, the rows start with the marker
/// alone, and where not even that fits, the line breaks without one.
pub fn markdown_rows(lines: &[StyledLine], width: usize) -> Vec<SourceLine> {
    let marker = Style::new().dim();
    let marker_width = 2;
    let mut rows = Vec::new();
    for line in lines {
        let line = source_line(line);
        if line.width() <= width {
            rows.push(line);
            continue;
        }
        let (indent, indent_width) = indent(&line);
        let (indent, prefix_width) = if indent_width + marker_width < width {
            (indent, indent_width + marker_width)
        } else if marker_width < width {
            (Vec::new(), marker_width)
        } else {
            rows.extend(line.wrap(width, width));
            continue;
        };
        let mut parts = line.wrap(width, width - prefix_width).into_iter();
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
/// A bullet, `• `, counts after any blanks and bars, as patine indents a
/// nested item with blanks. A number, such as `1. ` or `10. `, counts
/// only where the line starts, quote bars aside: there patine starts an
/// ordered item, while a line of a code block starts with blanks, and
/// its `1. ` is code. A nested numbered item therefore continues under
/// its number.
fn indent(line: &SourceLine) -> (Vec<(String, Style)>, usize) {
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

    let marker = if rest.starts_with("• ") {
        columns("• ")
    } else if only_quote_bars(&lead) {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(". ") {
            digits + 2
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
        let bar = text_style(|s| s.dim = true);
        let lines = [line(&[
            ("│ ", bar),
            ("https://example.com/long", TextStyle::default()),
        ])];

        let rows = markdown_rows(&lines, 16);

        assert_eq!(texts(&rows), ["│ https://exampl", "│ ↪ e.com/long"]);
        assert_eq!(
            rows[1].pieces().next(),
            Some(("│ ", Style::new().add_modifier(Modifier::DIM)))
        );
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
