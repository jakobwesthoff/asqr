// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The tab bar (spec section 7.1): one tab per question and the review,
//! scrolled so the current tab shows, over a rule that marks the current
//! tab.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::parts::answer_mark;
use super::source_line::columns;
use super::theme;
use crate::tui::SessionState;

/// One tab of the bar: its mark, styled as it shows, and its text.
pub(super) struct Tab {
    pub(super) mark: Span<'static>,
    pub(super) text: String,
}

/// The tab bar over two rows: the tabs, then a rule under them. It knows
/// nothing of sessions; [`tab_bar`] builds its tabs from one.
pub(super) struct TabBar {
    tabs: Vec<Tab>,
    current: usize,
}

impl TabBar {
    /// The bar of `tabs` with the one at `current` the current tab.
    pub(super) fn new(tabs: Vec<Tab>, current: usize) -> Self {
        TabBar { tabs, current }
    }

    /// The tabs as one line of `width` columns, and the columns the
    /// current tab takes in it. When they do not fit, the tabs around the
    /// current one are shown, with `←` and `→` where more follow.
    fn tab_line(self, width: usize) -> (Line<'static>, (usize, usize)) {
        let widths: Vec<usize> = self
            .tabs
            .iter()
            .map(|tab| columns(&tab.mark.content) + 1 + columns(&tab.text) + 2)
            .collect();
        let current = self.current;
        // Grow the visible window around the current tab while it fits,
        // keeping two columns on each side for the arrows.
        let room = width.saturating_sub(4);
        let (mut first, mut last) = (current, current);
        let mut used = widths[current];
        loop {
            let grow_right = last + 1 < self.tabs.len() && used + widths[last + 1] <= room;
            if grow_right {
                last += 1;
                used += widths[last];
            }
            let grow_left = first > 0 && used + widths[first - 1] <= room;
            if grow_left {
                first -= 1;
                used += widths[first];
            }
            if !grow_left && !grow_right {
                break;
            }
        }

        let more_right = last + 1 < self.tabs.len();
        let mut spans = vec![Span::from(if first > 0 { "← " } else { "  " })];
        let mut start = 0;
        for (index, Tab { mark, text }) in
            self.tabs.into_iter().enumerate().take(last + 1).skip(first)
        {
            let tab = [Span::from(" "), mark, Span::from(format!(" {text} "))];
            if index == current {
                start = spans.iter().map(|span| columns(&span.content)).sum();
                // The current tab is one colour, its mark included: the
                // rule under it already says which tab it is.
                let style = theme::primary().bold().not_dim();
                spans.extend(tab.map(|span| span.patch_style(style)));
            } else {
                spans.extend(tab);
            }
        }
        if more_right {
            spans.push(Span::from(" →"));
        }
        (Line::from(spans), (start, widths[current]))
    }
}

impl Widget for TabBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [tabs, rule] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
        let width = usize::from(tabs.width);
        let (line, (start, current)) = self.tab_line(width);
        line.render(tabs, buf);
        // The current tab may end past the edge of a bar narrower than it.
        let start = start.min(width);
        let current = current.min(width - start);
        Line::from(vec![
            Span::styled("─".repeat(start), theme::secondary()),
            Span::styled("━".repeat(current), theme::primary()),
            Span::styled("─".repeat(width - start - current), theme::secondary()),
        ])
        .render(rule, buf);
    }
}

/// The tab bar of `state`: every question by its header, or its id, with
/// its mark, then the review.
pub(super) fn tab_bar(state: &SessionState) -> TabBar {
    let mut tabs: Vec<Tab> = state
        .session()
        .questions
        .iter()
        .zip(state.question_states())
        .map(|(question, question_state)| Tab {
            mark: answer_mark(question_state.answer),
            text: question
                .header
                .as_deref()
                .unwrap_or(&question.id)
                .to_owned(),
        })
        .collect();
    tabs.push(Tab {
        mark: Span::from("✔"),
        text: "Review".to_owned(),
    });
    TabBar::new(tabs, state.tab())
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{find, row, rows};
    use super::*;

    /// `bar` drawn into a buffer of two rows and `width` columns.
    fn drawn(bar: TabBar, width: u16) -> Buffer {
        let area = Rect::new(0, 0, width, 2);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
        buf
    }

    /// Tabs with a plain `☐` and the given texts.
    fn plain(texts: &[&str]) -> Vec<Tab> {
        texts
            .iter()
            .map(|text| Tab {
                mark: Span::from("☐"),
                text: (*text).to_owned(),
            })
            .collect()
    }

    #[test]
    fn the_current_tab_is_bold_primary_and_an_unanswered_mark_elsewhere_is_dim() {
        let json = r#"{"asqr": 2, "questions": [
            {"id": "skipped", "text": "?", "kind": "text"},
            {"id": "defaulted", "text": "?", "kind": "single",
             "options": [{"id": "a", "label": "A", "default": true}]},
            {"id": "later", "text": "?", "kind": "text"}]}"#;
        let state = SessionState::new("tabs", serde_json::from_str(json).expect("parses"), None);
        let buf = drawn(tab_bar(&state), 50);

        assert_eq!(row(&buf, 0), "   ☐ skipped  ☒ defaulted  ☐ later  ✔ Review");
        let (start, _) = find(&buf, " ☐ skipped ");
        for x in start..start + 11 {
            let cell = &buf[(x, 0)];
            assert_eq!(
                (cell.fg, cell.modifier),
                (Color::Cyan, Modifier::BOLD),
                "column {x}, its mark included"
            );
        }
        let plain = (Color::Reset, Modifier::empty());
        let cell = &buf[(start + 11, 0)];
        assert_eq!((cell.fg, cell.modifier), plain, "after the current tab");
        let cell = &buf[find(&buf, "☒")];
        assert_eq!((cell.fg, cell.modifier), plain, "answered");
        let later = find(&buf, "☐ later");
        assert_eq!(buf[later].modifier, Modifier::DIM, "unanswered");
        assert_eq!(
            buf[(later.0 + 2, 0)].modifier,
            Modifier::empty(),
            "its text"
        );
    }

    #[test]
    fn the_rule_is_heavy_and_primary_under_the_current_tab_and_thin_and_dim_elsewhere() {
        let buf = drawn(TabBar::new(plain(&["one", "two", "three"]), 1), 30);

        assert_eq!(
            rows(&buf),
            [
                "   ☐ one  ☐ two  ☐ three      ",
                "─────────━━━━━━━──────────────"
            ]
        );
        for x in 0..30 {
            let cell = &buf[(x, 1)];
            let expected = if (9..16).contains(&x) {
                (Color::Cyan, Modifier::empty())
            } else {
                (Color::Reset, Modifier::DIM)
            };
            assert_eq!((cell.fg, cell.modifier), expected, "column {x}");
        }
    }

    #[test]
    fn the_bar_scrolls_to_the_current_tab_and_its_rule_follows() {
        let texts = ["first", "second", "third", "fourth"];
        let buf = drawn(TabBar::new(plain(&texts), 3), 24);
        assert_eq!(
            rows(&buf),
            ["←  ☐ third  ☐ fourth    ", "───────────━━━━━━━━━━───"]
        );

        let buf = drawn(TabBar::new(plain(&texts), 0), 24);
        assert_eq!(
            rows(&buf),
            ["   ☐ first  ☐ second  → ", "──━━━━━━━━━─────────────"]
        );
    }

    #[test]
    fn a_tab_wider_than_the_bar_has_its_rule_up_to_the_edge() {
        let buf = drawn(TabBar::new(plain(&["a header longer than the bar"]), 0), 12);

        assert_eq!(
            rows(&buf),
            ["   ☐ a heade", &format!("──{}", "━".repeat(10))]
        );
    }

    #[test]
    fn the_tab_bar_measures_wide_headers_in_columns() {
        let headers = ["質問一", "質問二", "質問三", "質問四", "質問五"];

        // Each tab takes 10 columns, so four fill the 40 between the
        // arrows exactly.
        let buf = drawn(TabBar::new(plain(&headers), 0), 44);

        assert_eq!(row(&buf, 0), "   ☐ 質問一  ☐ 質問二  ☐ 質問三  ☐ 質問四  →");
        assert_eq!(
            row(&buf, 1),
            format!("──{}{}", "━".repeat(10), "─".repeat(32))
        );
    }
}
