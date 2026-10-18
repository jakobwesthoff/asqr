// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The session in the title bar and the header under it (spec section
//! 7.2, ADR 47): the title beside `asqr`, then `from`, `follows` and the
//! intro, which hang under the title. On a low terminal the header gives
//! its rows to the tab's area, the blank line first, then intro lines.

use patine::QuoteStyle;
use ratatui::text::{Line, Span};

use super::panes::READING_WIDTH;
use super::parts::question_key;
use super::source_line::{columns, fitting};
use super::text::field_lines;
use super::theme;
use super::wrap::{cut, indented};
use crate::tui::SessionState;

/// The column the session's title starts in, after ` asqr` and two
/// blanks. The lines under the title start there too.
const INDENT: usize = 7;

/// The most lines the intro takes; `i` shows the rest.
const INTRO_LINES: usize = 2;

/// The blanks that at least stand between the title and the queue.
const GAP: usize = 2;

/// The columns the header's text takes after [`INDENT`] on a terminal of
/// `width` columns. It ends where the question's text ends: a column
/// before the edge, and never past the reading width.
fn text_width(width: usize) -> usize {
    let header = width.min(usize::from(READING_WIDTH) + 2);
    header.saturating_sub(1 + INDENT)
}

/// `text` on one line with single blanks, as ratatui drops a line break
/// without a blank in its place; `None` when nothing but blanks is left.
fn one_line(text: &str) -> Option<String> {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then_some(line)
}

/// The title bar while `state` is shown, `width` columns wide: `asqr`, the
/// session's title and, dim at the right end, the queue and how many
/// sessions wait. The title matters more than the queue: where both do not
/// fit, the queue goes, and only then is the title cut. The count always
/// shows.
pub(super) fn title_bar(
    state: &SessionState,
    queue: &str,
    waiting: usize,
    width: usize,
) -> Line<'static> {
    let text_width = text_width(width);
    let title = one_line(state.name()).unwrap_or_default();
    let count = format!("{waiting} waiting ");
    let queue = format!("{queue} · ");

    let whole_title = columns(&title).min(text_width);
    let queue = if INDENT + whole_title + GAP + columns(&queue) + columns(&count) <= width {
        queue
    } else {
        String::new()
    };
    let right = columns(&queue) + columns(&count);
    let title = cut(
        &title,
        text_width.min(width.saturating_sub(INDENT + GAP + right)),
    );
    let blanks = width.saturating_sub(INDENT + columns(&title) + right);

    Line::from(vec![
        Span::styled(" asqr", theme::app_name()),
        Span::from("  "),
        Span::styled(title, theme::title()),
        Span::from(" ".repeat(blanks)),
        Span::styled(format!("{queue}{count}"), theme::secondary()),
    ])
}

/// The session header on a terminal of `width` columns, in at most
/// `spare` lines where it can: `from` and `follows` in one line, the intro
/// in at most two, then a blank line before the tab bar, all under the
/// title. With fewer lines to spare the blank line goes first, then intro
/// lines down to one; the `from` and `follows` line stays.
///
/// A cut intro ends in `… i: more`, as `i` shows all of it, or in
/// `… esc, i: more` where `i` would be typed or taken by the pane. Its
/// quotes show in `quote`.
pub(super) fn header_lines(
    state: &SessionState,
    width: usize,
    spare: usize,
    quote: QuoteStyle,
) -> Vec<Line<'static>> {
    let text_width = text_width(width);
    let meta = meta_line(state, text_width);
    // The intro keeps patine's styles in the text colour: it is the
    // asker's content, not a hint.
    let intro: Vec<Line<'static>> = state
        .session()
        .intro
        .as_deref()
        .map(|intro| field_lines(intro, text_width, quote))
        .unwrap_or_default()
        .into_iter()
        .map(|line| indented(line, INDENT))
        .collect();

    let meta_rows = usize::from(meta.is_some());
    let mut intro_rows = intro.len().min(INTRO_LINES);
    let blank = meta_rows + intro_rows < spare;
    while meta_rows + intro_rows > spare && intro_rows > 1 {
        intro_rows -= 1;
    }

    let more = format!("… {}: more", question_key(state, "i"));
    let mut lines: Vec<Line<'static>> = meta.into_iter().collect();
    lines.extend(at_most(intro, intro_rows, &more, INDENT + text_width));
    if blank {
        lines.push(Line::default());
    }
    lines
}

/// `from` and `follows`, dim in one line under the title and cut with `…`
/// to `text_width`, or `None` where the session has neither. `follows`
/// shows the title of the session it names in quotes where the inbox found
/// one, else the value as the asker wrote it.
fn meta_line(state: &SessionState, text_width: usize) -> Option<Line<'static>> {
    let session = state.session();
    let from = session.from.as_deref().and_then(one_line);
    let follows = match state.follows_title().and_then(one_line) {
        Some(title) => Some(format!("\"{title}\"")),
        None => session.follows.as_deref().and_then(one_line),
    };
    let parts: Vec<String> = [
        from.map(|from| format!("from {from}")),
        follows.map(|follows| format!("follows {follows}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| {
        let text = cut(&parts.join(" · "), text_width);
        indented(Line::styled(text, theme::secondary()), INDENT)
    })
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
    use ratatui::style::{Style, Stylize};

    use super::super::patine_adapter::quote_style;
    use super::super::test_support::line_texts;
    use super::*;

    /// The state of a session of one `single` question with `fields`
    /// before its questions, such as `"title": "Release",`. No field has the
    /// keys, so `i` needs no `esc` first.
    fn session_with(fields: &str) -> SessionState {
        let json = format!(
            r#"{{"asqr": 2, {fields} "questions": [{{"id": "q", "text": "?", "kind": "single",
                "options": [{{"id": "a", "label": "A"}}]}}]}}"#
        );
        SessionState::new("batch", serde_json::from_str(&json).expect("parses"), None)
    }

    #[test]
    fn the_title_bar_names_the_session_by_its_title_else_by_its_id() {
        let title = |fields: &str| {
            let bar = title_bar(&session_with(fields), "default", 1, 40);
            bar.spans[2].clone()
        };

        assert_eq!(
            title(r#""title": "Release","#),
            Span::from("Release").bold()
        );
        assert_eq!(title(""), Span::from("batch").bold());
    }

    #[test]
    fn a_title_with_line_breaks_shows_on_one_line() {
        let state = session_with(r#""title": "Round\n  two\tof it","#);

        // 7 + 15 + 2 + 20 columns would not fit with the queue, so it goes.
        assert_eq!(
            line_texts(&[title_bar(&state, "default", 1, 40)]),
            [format!(" asqr  Round two of it{}1 waiting ", " ".repeat(8))]
        );
    }

    #[test]
    fn follows_shows_the_title_in_quotes_else_the_value_as_written() {
        let mut state = session_with(r#""from": "agent", "follows": "round-1","#);
        let meta = |state: &SessionState| {
            line_texts(&header_lines(state, 80, 10, quote_style(false)))[0].clone()
        };

        assert_eq!(meta(&state), "       from agent · follows round-1");
        state.set_follows_title(Some("Round one,\n  first try".to_owned()));
        assert_eq!(
            meta(&state),
            r#"       from agent · follows "Round one, first try""#
        );
        // A title of nothing but blanks names nothing.
        state.set_follows_title(Some("  ".to_owned()));
        assert_eq!(meta(&state), "       from agent · follows round-1");
    }

    #[test]
    fn the_intro_wraps_under_the_title_in_its_markdown_styles() {
        // 20 columns leave the intro 12 after the indent and the column
        // before the edge.
        let state = session_with(r#""intro": "Round **two** of the new layout here","#);
        let lines = header_lines(&state, 20, 10, quote_style(false));

        assert_eq!(
            line_texts(&lines),
            ["       Round two of", "       the… i: more", ""]
        );
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|span| span.content == "two" && span.style == Style::new().bold()),
            "{lines:?}"
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
