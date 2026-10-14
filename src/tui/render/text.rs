// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Markdown of the intro, question texts and option descriptions (spec
//! section 7.6, ADR 35): rendered by patine for the width the text gets,
//! with every newline a line break and raw HTML as written, laid out by
//! the adapter's one rule for Markdown, so a line patine leaves wider
//! wraps behind a dim `↪`, and kept from one frame to the next.

use std::collections::HashMap;

use patine::{Html, RenderOptions, SoftBreaks};
use ratatui::text::{Line, Span};

use super::patine_adapter::markdown_rows;

/// How the text fields differ from Markdown documents (ADR 35): a single
/// newline breaks the line, and raw HTML shows as written.
const FIELD_OPTIONS: RenderOptions = RenderOptions::new()
    .with_soft_breaks(SoftBreaks::Break)
    .with_html(Html::Literal);

/// `source` rendered for `width` columns, as lines of at most that width.
/// Text without any content renders to no lines at all.
fn render(source: &str, width: usize) -> Vec<Line<'static>> {
    markdown_rows(
        &patine::render_lines_with(source, width, &FIELD_OPTIONS),
        width,
    )
    .iter()
    .map(|row| {
        Line::from(
            row.pieces()
                .map(|(text, style)| Span::styled(text.to_owned(), style))
                .collect::<Vec<_>>(),
        )
    })
    .collect()
}

/// Rendered texts by their source and width.
type Rendered = HashMap<String, Vec<(usize, Vec<Line<'static>>)>>;

/// The texts rendered for the frames being drawn. The screen draws after
/// every key, and rendering is the costly part of a frame: patine parses
/// the Markdown and highlights its code blocks. A text keeps its lines
/// while each frame shows it at the same width; whatever a frame leaves
/// out is gone after the next one, so the cache holds what is on screen
/// and nothing that a resize or another session left behind.
#[derive(Default)]
pub struct MarkdownCache {
    /// What the frame being drawn has used so far.
    current: Rendered,
    /// What the frame before used and this one has not asked for yet.
    previous: Rendered,
    #[cfg(test)]
    renders: usize,
}

impl MarkdownCache {
    /// Starts the next frame: what the last one used stays at hand, and
    /// what it did not use is dropped.
    pub fn begin_frame(&mut self) {
        self.previous = std::mem::take(&mut self.current);
    }

    /// `source` rendered for `width` columns, as [`render`] does, rendered
    /// only when no earlier frame or this one has done so already.
    pub fn lines(&mut self, source: &str, width: usize) -> Vec<Line<'static>> {
        let kept = self
            .current
            .get(source)
            .and_then(|widths| widths.iter().find(|(kept, _)| *kept == width));
        if let Some((_, lines)) = kept {
            return lines.clone();
        }
        let earlier = self.previous.get_mut(source).and_then(|widths| {
            let index = widths.iter().position(|(kept, _)| *kept == width)?;
            Some(widths.swap_remove(index).1)
        });
        let lines = earlier.unwrap_or_else(|| {
            #[cfg(test)]
            {
                self.renders += 1;
            }
            render(source, width)
        });
        self.current
            .entry(source.to_owned())
            .or_default()
            .push((width, lines.clone()));
        lines
    }

    /// How many texts the cache has rendered, for tests that check it
    /// renders no text twice.
    #[cfg(test)]
    pub fn renders(&self) -> usize {
        self.renders
    }
}

#[cfg(test)]
mod tests {
    use super::super::source_line::columns;
    use ratatui::style::{Color, Modifier, Style};

    use super::*;

    /// The text of each line.
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

    /// Each span of `line` with its style.
    fn spans(line: &Line) -> Vec<(String, Style)> {
        line.spans
            .iter()
            .map(|span| (span.content.to_string(), span.style))
            .collect()
    }

    #[test]
    fn emphasis_and_inline_code_keep_patines_styles() {
        let lines = render("a **bold** *it* `code`", 40);

        assert_eq!(
            spans(&lines[0]),
            [
                ("a ".to_owned(), Style::new()),
                ("bold".to_owned(), Style::new().add_modifier(Modifier::BOLD)),
                (" ".to_owned(), Style::new()),
                ("it".to_owned(), Style::new().add_modifier(Modifier::ITALIC)),
                (" ".to_owned(), Style::new()),
                ("code".to_owned(), Style::new().fg(Color::Indexed(3))),
            ]
        );
    }

    #[test]
    fn lists_paragraphs_and_headings_render() {
        assert_eq!(
            texts(&render("# Plan\n\n- one\n- two\n\n1. first\n2. second", 40)),
            ["Plan", "", "• one", "• two", "", "1. first", "2. second"]
        );
    }

    #[test]
    fn prose_wraps_at_the_width() {
        assert_eq!(
            texts(&render("Some text that wraps around nicely here.", 24)),
            ["Some text that wraps", "around nicely here."]
        );
    }

    #[test]
    fn a_wide_code_line_wraps_behind_the_dim_marker() {
        let lines = render("```\nlet total = first + second + third;\n```\n", 20);

        assert_eq!(
            texts(&lines),
            ["  let total = first ", "  ↪ + second + third", "  ↪ ;"]
        );
        assert_eq!(lines[1].spans[1].style, Style::new().dim());
        assert!(
            lines
                .iter()
                .all(|line| columns(&texts(std::slice::from_ref(line))[0]) <= 20)
        );
    }

    #[test]
    fn a_single_newline_breaks_the_line() {
        assert_eq!(
            texts(&render("first line\nsecond line\n\nnext paragraph", 40)),
            ["first line", "second line", "", "next paragraph"]
        );
        assert_eq!(
            texts(&render("first\r\nsecond", 40)),
            ["first", "second"],
            "a Windows line ending too"
        );
        assert_eq!(
            texts(&render("> quoted\nsecond", 40)),
            ["│ quoted", "│ second"],
            "inside a quote, behind its bar"
        );
    }

    #[test]
    fn a_single_newline_in_a_list_item_goes_on_under_its_text() {
        assert_eq!(
            texts(&render("- item\nmore\n- next", 40)),
            ["• item", "  more", "• next"]
        );
    }

    #[test]
    fn code_keeps_its_newlines_and_angle_brackets() {
        assert_eq!(
            texts(&render("```\nlet v: Vec<u8> = a;\nnext\n```", 40)),
            ["  let v: Vec<u8> = a;", "  next"]
        );
        assert_eq!(
            texts(&render("`Vec<u8>` and `a\nb`", 40)),
            ["Vec<u8> and a b"],
            "inline code shows a newline as a space"
        );
    }

    #[test]
    fn raw_html_shows_as_written() {
        assert_eq!(
            texts(&render("Use a Vec<String> here.", 40)),
            ["Use a Vec<String> here."]
        );
        assert_eq!(
            texts(&render("<div>\nblock\n</div>\n\nafter", 40)),
            ["<div>", "block", "</div>", "", "after"]
        );
        assert_eq!(texts(&render("<!-- note -->", 40)), ["<!-- note -->"]);
    }

    #[test]
    fn an_autolink_stays_a_link() {
        let lines = render("<https://example.com>", 60);

        assert!(!texts(&lines)[0].contains('<'), "{:?}", texts(&lines));
        assert_eq!(lines[0].spans[0].content, "https://example.com");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn text_without_content_has_no_lines() {
        assert!(render("", 40).is_empty());
        assert!(render("   ", 40).is_empty());
    }

    #[test]
    fn a_text_renders_once_per_width_across_frames() {
        let mut cache = MarkdownCache::default();

        let first = cache.lines("**Plan**", 40);
        assert_eq!(cache.lines("**Plan**", 40), first, "the same frame");
        cache.begin_frame();
        assert_eq!(cache.lines("**Plan**", 40), first, "the next frame");
        assert_eq!(cache.renders(), 1);

        cache.lines("**Plan**", 30);
        cache.lines("*Plan*", 40);
        assert_eq!(cache.renders(), 3, "a new width and a new text");
    }

    #[test]
    fn what_a_frame_leaves_out_is_rendered_again_later() {
        let mut cache = MarkdownCache::default();
        cache.lines("**Plan**", 40);
        cache.lines("**Plan**", 30);

        cache.begin_frame();
        cache.lines("**Plan**", 40);
        cache.begin_frame();
        cache.begin_frame();
        cache.lines("**Plan**", 30);
        assert_eq!(cache.renders(), 3, "width 30 was gone after one frame");

        cache.begin_frame();
        cache.lines("**Plan**", 30);
        assert_eq!(cache.renders(), 3, "while width 30 is used, it stays");
    }
}
