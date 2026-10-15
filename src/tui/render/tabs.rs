// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The tab bar (spec section 7.1): one tab per question and the review,
//! scrolled so the current tab shows.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};

use super::parts::answer_mark;
use super::source_line::columns;
use super::theme;
use crate::tui::SessionState;

pub(super) fn draw_tabs(area: Rect, buf: &mut Buffer, state: &SessionState) {
    let block = Block::new().borders(Borders::BOTTOM);
    let inner = block.inner(area);
    block.render(area, buf);
    tab_line(state, inner.width as usize).render(inner, buf);
}

/// The tabs as one line of `width` columns: every question with its mark,
/// then the review. When they do not fit, the tabs around the current one
/// are shown, with `←` and `→` where more follow.
fn tab_line(state: &SessionState, width: usize) -> Line<'static> {
    let states = state.question_states();
    // Each label is its mark, then its text.
    let mut labels: Vec<(Span<'static>, String)> = state
        .session()
        .questions
        .iter()
        .zip(&states)
        .map(|(question, question_state)| {
            let header = question.header.as_deref().unwrap_or(&question.id);
            (answer_mark(question_state.answer), header.to_owned())
        })
        .collect();
    labels.push((Span::from("✔"), "Review".to_owned()));

    let widths: Vec<usize> = labels
        .iter()
        .map(|(mark, text)| columns(&mark.content) + 1 + columns(text) + 2)
        .collect();
    let current = state.tab();
    // Grow the visible window around the current tab while it fits,
    // keeping two columns on each side for the arrows.
    let room = width.saturating_sub(4);
    let (mut first, mut last) = (current, current);
    let mut used = widths[current];
    loop {
        let grow_right = last + 1 < labels.len() && used + widths[last + 1] <= room;
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

    let more_right = last + 1 < labels.len();
    let mut spans = vec![Span::from(if first > 0 { "← " } else { "  " })];
    for (index, (mark, text)) in labels.into_iter().enumerate().take(last + 1).skip(first) {
        let tab = [Span::from(" "), mark, Span::from(format!(" {text} "))];
        if index == current {
            spans.extend(tab.map(|span| span.patch_style(theme::focus_fill()).bold()));
        } else {
            spans.extend(tab);
        }
    }
    if more_right {
        spans.push(Span::from(" →"));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::super::test_support::{find, line_texts, row};
    use super::*;

    #[test]
    fn the_current_tab_is_reversed_and_bold_and_each_mark_says_whether_it_is_answered() {
        let json = r#"{"asqr": 2, "questions": [
            {"id": "skipped", "text": "?", "kind": "text"},
            {"id": "defaulted", "text": "?", "kind": "single",
             "options": [{"id": "a", "label": "A", "default": true}]}]}"#;
        let state = SessionState::new("tabs", serde_json::from_str(json).expect("parses"), None);
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        tab_line(&state, 40).render(area, &mut buf);

        assert_eq!(row(&buf, 0), "   ☐ skipped  ☒ defaulted  ✔ Review");
        let (start, _) = find(&buf, " ☐ skipped ");
        let current = Modifier::REVERSED | Modifier::BOLD;
        for x in start..start + 11 {
            assert_eq!(buf[(x, 0)].modifier, current, "column {x}");
        }
        assert_eq!(buf[(start + 11, 0)].modifier, Modifier::empty());
        assert_eq!(buf[find(&buf, "☒")].modifier, Modifier::empty());
    }

    #[test]
    fn the_tab_bar_measures_wide_headers_in_columns() {
        let questions: Vec<String> = ["質問一", "質問二", "質問三", "質問四", "質問五"]
            .iter()
            .enumerate()
            .map(|(index, header)| {
                format!(
                    r#"{{"id": "q{index}", "header": "{header}", "text": "?", "kind": "text"}}"#
                )
            })
            .collect();
        let json = format!(r#"{{"asqr": 2, "questions": [{}]}}"#, questions.join(","));
        let state = SessionState::new("wide", serde_json::from_str(&json).expect("parses"), None);

        // Each tab takes 10 columns, so four fill the 40 between the
        // arrows exactly.
        let line = tab_line(&state, 44);

        assert_eq!(
            line_texts(&[line])[0],
            "   ☐ 質問一  ☐ 質問二  ☐ 質問三  ☐ 質問四  →"
        );
    }
}
