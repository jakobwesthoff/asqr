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

use super::source_line::columns;
use crate::tui::{AnswerState, SessionState};

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
    let mut labels: Vec<String> = state
        .session()
        .questions
        .iter()
        .zip(&states)
        .map(|(question, question_state)| {
            let mark = if question_state.answer == AnswerState::Skipped {
                "☐"
            } else {
                "☒"
            };
            format!(
                "{mark} {}",
                question.header.as_deref().unwrap_or(&question.id)
            )
        })
        .collect();
    labels.push("✔ Review".to_owned());

    let widths: Vec<usize> = labels.iter().map(|label| columns(label) + 2).collect();
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

    let mut spans = vec![Span::from(if first > 0 { "← " } else { "  " })];
    for (index, label) in labels.iter().enumerate().take(last + 1).skip(first) {
        let span = Span::from(format!(" {label} "));
        spans.push(if index == current {
            span.reversed().bold()
        } else {
            span
        });
    }
    if last + 1 < labels.len() {
        spans.push(Span::from(" →"));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::line_texts;
    use super::*;

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
