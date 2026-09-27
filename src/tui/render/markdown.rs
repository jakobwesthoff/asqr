// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Markdown subset of question texts (spec section 7.6): bold, italic,
//! inline code, lists and line breaks are styled. Every other element, and
//! every escape or entity, is shown exactly as it is written in the
//! source: the renderer walks the events with their source offsets and
//! emits the raw slice of whatever it does not style.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Tags that sit inside a line; an unsupported one of these is emitted in
/// place, without starting a new block.
fn is_inline(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Link { .. }
            | Tag::Image { .. }
            | Tag::Superscript
            | Tag::Subscript
    )
}

#[derive(Default)]
struct Renderer<'a> {
    source: &'a str,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    modifiers: Vec<Modifier>,
    /// The next number of each open list; `None` for a bullet list.
    lists: Vec<Option<u64>>,
    /// Where the last event ended in the source. The parser leaves the
    /// backslash of an escape out of every event, so it is found in the gap
    /// before the escaped text.
    last_end: usize,
}

impl Renderer<'_> {
    fn style(&self) -> Style {
        Style::default().add_modifier(
            self.modifiers
                .iter()
                .fold(Modifier::empty(), |all, one| all | *one),
        )
    }

    fn push(&mut self, text: &str, style: Style) {
        if !text.is_empty() {
            self.current.push(Span::styled(text.to_owned(), style));
        }
    }

    fn flush(&mut self) {
        if !self.current.is_empty() {
            self.lines
                .push(Line::from(std::mem::take(&mut self.current)));
        }
    }

    /// Starts a new block: a blank line separates it from what came before,
    /// except inside a list, whose items follow each other directly.
    fn begin_block(&mut self) {
        self.flush();
        if !self.lines.is_empty() && self.lists.is_empty() {
            self.lines.push(Line::default());
        }
    }

    /// Emits the source of `range` as it is written, line by line.
    fn raw(&mut self, range: Range<usize>) {
        let raw = self.source[range].trim_end_matches('\n');
        let mut raw_lines = raw.split('\n');
        if let Some(first) = raw_lines.next() {
            self.push(first, Style::default());
        }
        for line in raw_lines {
            self.flush();
            self.push(line, Style::default());
        }
    }
}

/// Renders `source` into lines.
pub fn render(source: &str) -> Vec<Line<'static>> {
    let mut renderer = Renderer {
        source,
        ..Renderer::default()
    };
    let mut events = Parser::new(source).into_offset_iter();

    while let Some((event, range)) = events.next() {
        let gap_start = std::mem::replace(
            &mut renderer.last_end,
            if matches!(event, Event::Start(_)) {
                range.start
            } else {
                range.end
            },
        );
        match event {
            Event::Start(Tag::Paragraph) => {
                if renderer.current.is_empty() {
                    renderer.begin_block();
                }
            }
            Event::Start(Tag::Emphasis) => renderer.modifiers.push(Modifier::ITALIC),
            Event::Start(Tag::Strong) => renderer.modifiers.push(Modifier::BOLD),
            Event::Start(Tag::List(start)) => {
                if renderer.lists.is_empty() {
                    renderer.begin_block();
                }
                renderer.lists.push(start);
            }
            Event::Start(Tag::Item) => {
                renderer.flush();
                let marker = match renderer.lists.last_mut() {
                    Some(Some(number)) => {
                        *number += 1;
                        format!("{}. ", *number - 1)
                    }
                    _ => "• ".to_owned(),
                };
                renderer.push(&marker, Style::default());
            }
            Event::Start(tag) => {
                // Outside the subset: the whole element as written, once,
                // and none of its inner events.
                if !is_inline(&tag) {
                    if renderer.lists.is_empty() {
                        renderer.begin_block();
                    } else if renderer.current.len() > 1 {
                        renderer.flush();
                    }
                }
                renderer.raw(range);
                let mut depth = 1;
                for (inner, _) in events.by_ref() {
                    match inner {
                        Event::Start(_) => depth += 1,
                        Event::End(_) => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                }
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong) => {
                renderer.modifiers.pop();
            }
            Event::End(TagEnd::List(_)) => {
                renderer.lists.pop();
            }
            // Paragraphs and items; unsupported elements never get here, as
            // they are skipped together with their end.
            Event::End(_) => renderer.flush(),
            Event::Text(_) => {
                // The source slice, not the parsed text, keeps escapes and
                // entities as they were written.
                let style = renderer.style();
                let gap = source.get(gap_start..range.start).unwrap_or_default();
                let start = if !gap.is_empty() && gap.chars().all(|c| c == '\\') {
                    gap_start
                } else {
                    range.start
                };
                renderer.push(&source[start..range.end], style);
            }
            Event::Code(code) => {
                let style = renderer.style().add_modifier(Modifier::REVERSED);
                renderer.push(&code, style);
            }
            Event::SoftBreak | Event::HardBreak => renderer.flush(),
            _ => renderer.raw(range),
        }
    }
    renderer.flush();
    renderer.lines
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::*;

    /// The lines as plain text.
    fn text(markdown: &str) -> Vec<String> {
        render(markdown)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    /// Each span of the first line with its modifiers.
    fn spans(markdown: &str) -> Vec<(String, Modifier)> {
        render(markdown)[0]
            .spans
            .iter()
            .map(|span| (span.content.to_string(), span.style.add_modifier))
            .collect()
    }

    #[test]
    fn plain_text_stays_as_it_is() {
        assert_eq!(text("Which option?"), ["Which option?"]);
    }

    #[test]
    fn styles_bold_italic_and_inline_code() {
        assert_eq!(
            spans("a **bold** *it* `code`"),
            [
                ("a ".to_owned(), Modifier::empty()),
                ("bold".to_owned(), Modifier::BOLD),
                (" ".to_owned(), Modifier::empty()),
                ("it".to_owned(), Modifier::ITALIC),
                (" ".to_owned(), Modifier::empty()),
                ("code".to_owned(), Modifier::REVERSED),
            ]
        );
    }

    #[test]
    fn nested_styles_add_up() {
        assert_eq!(
            spans("***both***"),
            [("both".to_owned(), Modifier::BOLD | Modifier::ITALIC)]
        );
    }

    #[test]
    fn keeps_line_breaks_and_separates_paragraphs() {
        assert_eq!(
            text("first line\nsecond line\n\nnext paragraph"),
            ["first line", "second line", "", "next paragraph"]
        );
    }

    #[test]
    fn renders_lists_with_markers() {
        assert_eq!(
            text("- one\n- two\n\n1. first\n2. second"),
            ["• one", "• two", "", "1. first", "2. second"]
        );
    }

    #[test]
    fn shows_everything_outside_the_subset_as_written() {
        assert_eq!(text("# x"), ["# x"]);
        assert_eq!(text("> x"), ["> x"]);
        assert_eq!(
            text("```\nfn main() {}\n```"),
            ["```", "fn main() {}", "```"]
        );
        assert_eq!(text("<b>html</b>"), ["<b>html</b>"]);
        assert_eq!(
            text("[link](https://example.com)"),
            ["[link](https://example.com)"]
        );
    }

    #[test]
    fn keeps_escapes_and_entities_as_written() {
        assert_eq!(text(r"a \* b &amp; c"), [r"a \* b &amp; c"]);
    }

    #[test]
    fn an_unsupported_element_inside_a_list_appears_once() {
        assert_eq!(text("- # heading\n- plain"), ["• # heading", "• plain"]);
    }

    #[test]
    fn a_block_after_text_in_a_list_item_starts_a_new_line() {
        assert_eq!(text("- item\n  > quote"), ["• item", "> quote"]);
    }

    #[test]
    fn an_empty_text_has_no_lines() {
        assert!(render("").is_empty());
    }
}
