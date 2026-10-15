// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The session header under the title bar (spec section 7.1): the session's
//! title and intro, each cut to the lines the screen gives it.

use ratatui::style::Stylize;
use ratatui::text::{Line, Span};

use super::parts::question_key;
use super::source_line::{columns, fitting};
use super::text::field_lines;
use super::wrap::{indented, wrap};
use crate::tui::SessionState;

/// The session header in `width` columns: the title line, then the intro,
/// both wrapped. The title takes at most two lines and the intro three,
/// one each on short terminals, so the question keeps its room. A cut
/// title ends in `…`; a cut intro ends in `… i: more`, as `i` shows all
/// of it, or in `… esc, i: more` where `i` would be typed or taken by the
/// pane.
pub(super) fn header_lines(state: &SessionState, width: usize, height: u16) -> Vec<Line<'static>> {
    let short = height < 24;
    let session = state.session();
    let mut spans = vec![Span::from(state.name().to_owned()).bold()];
    if let Some(from) = &session.from {
        spans.push(Span::from(format!("  ({from})")));
    }
    if let Some(follows) = &session.follows {
        spans.push(Span::from(format!("  follows: {follows}")).dim());
    }
    // Lines end a column before the edge, as the question's text does.
    let room = width.saturating_sub(1);
    let mut lines = at_most(
        wrap(Line::from(spans), room, 1, 1),
        if short { 1 } else { 2 },
        "…",
        room,
    );

    let Some(intro) = session.intro.as_deref() else {
        return lines;
    };
    // Dim comes on top of patine's styles, which still show underneath.
    let intro: Vec<Line<'static>> = field_lines(intro, room.saturating_sub(1))
        .into_iter()
        .map(|line| indented(line, 1).dim())
        .collect();
    let more = format!("… {}: more", question_key(state, "i"));
    lines.extend(at_most(intro, if short { 1 } else { 3 }, &more, room));
    lines
}

/// The first `count` of `lines`; when that cuts some off, the last one
/// kept ends in `marker`, within `width`.
fn at_most(
    mut lines: Vec<Line<'static>>,
    count: usize,
    marker: &str,
    width: usize,
) -> Vec<Line<'static>> {
    if lines.len() <= count {
        return lines;
    }
    lines.truncate(count);
    let last = lines.pop().expect("at least one line is kept");
    lines.push(ending_in(last, marker, width));
    lines
}

/// `line` shortened so that `marker` fits after it within `width`
/// columns. What stays keeps its styles, and blanks before the marker go.
fn ending_in(line: Line<'static>, marker: &str, width: usize) -> Line<'static> {
    let mut room = width.saturating_sub(columns(marker));
    let mut spans: Vec<Span<'static>> = Vec::new();
    for span in line.spans {
        let kept = fitting(&span.content, room);
        room -= columns(kept);
        if !kept.is_empty() {
            spans.push(Span::styled(kept.to_owned(), span.style));
        }
        // A wide character that did not fit ends the line, even where a
        // narrower one after it would fill the column it leaves.
        if kept.len() < span.content.len() {
            break;
        }
    }
    // The indent span stays, so a line of nothing but blanks keeps its
    // place before the marker.
    while spans.len() > 1 {
        let last = spans.last_mut().expect("more than one span");
        let trimmed = last.content.trim_end().to_owned();
        if trimmed.is_empty() {
            spans.pop();
        } else {
            last.content = trimmed.into();
            break;
        }
    }
    spans.push(Span::from(marker.to_owned()));
    Line::from(spans).style(line.style)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::line_texts;
    use super::*;

    #[test]
    fn the_header_names_the_session_by_its_title_else_by_its_id() {
        let header = |title: &str| {
            let json = format!(
                r#"{{"asqr": 2, {title} "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#
            );
            let state =
                SessionState::new("batch", serde_json::from_str(&json).expect("parses"), None);
            header_lines(&state, 40, 30).remove(0)
        };

        assert_eq!(
            header(r#""title": "Release","#).spans,
            [Span::from(" "), Span::from("Release").bold()]
        );
        assert_eq!(
            header("").spans,
            [Span::from(" "), Span::from("batch").bold()]
        );
    }

    #[test]
    fn a_cut_line_keeps_its_styles_and_drops_the_blanks_before_the_marker() {
        let line = Line::from(vec![
            Span::from(" "),
            Span::from("bold").bold(),
            Span::from(" words   "),
        ]);
        let cut = ending_in(line, "…", 13);
        assert_eq!(
            cut.spans,
            [
                Span::from(" "),
                Span::from("bold").bold(),
                Span::from(" words"),
                Span::from("…"),
            ]
        );

        let blank = ending_in(Line::from(vec![Span::from(" "), Span::from("   ")]), "…", 4);
        assert_eq!(blank.spans, [Span::from(" "), Span::from("…")]);
    }

    #[test]
    fn a_cut_line_measures_wide_characters_in_columns() {
        let line = Line::from(vec![Span::from(" "), Span::from("日本語の説明").dim()]);
        let kept = [
            Span::from(" "),
            Span::from("日本").dim(),
            Span::from("… i: more"),
        ];

        // The marker takes 9 of the 14 columns, which leaves 5: the blank,
        // then 日本, which ends at the edge.
        let cut = ending_in(line.clone(), "… i: more", 14);
        assert_eq!(cut.spans, kept);
        assert_eq!(columns(&line_texts(&[cut])[0]), 14);
        assert_eq!(
            ending_in(line, "… i: more", 15).spans,
            kept,
            "語 would cross the edge"
        );
        assert_eq!(
            line_texts(&[ending_in(Line::from("ab🎉cd"), "…", 4)]),
            ["ab…"]
        );
    }
}
