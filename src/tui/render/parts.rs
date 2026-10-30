// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Texts the screen is built from, kept apart from the layout so they can
//! be tested on their own.

use patine::QuoteStyle;
use ratatui::text::Span;

use super::text::field_source_lines;
use super::theme;
use super::wrap::{trim_end, trim_start};
use crate::format::{Answer, Kind, Question, has_note, is_answered};
use crate::tui::{AnswerState, Focus, SessionState};

/// What marks the row under the cursor in the question, the review and the
/// session list, the blank after it included.
pub(super) const POINTER: &str = "❯ ";

/// The start of a row: [`POINTER`] when the row is `current`, otherwise
/// blanks as wide, so every row's text starts in the same column.
pub(super) fn pointer(current: bool) -> Span<'static> {
    Span::from(if current { POINTER } else { "  " })
}

/// The mark the tab bar and the review put before a question: a dim `☐`
/// while it is skipped, `☒` once it is answered or defaulted, and a dim `ℹ`
/// on an info item. An answer is no status, so `☒` keeps the text colour.
/// An info item recedes like an open question, as there is nothing to do
/// on it, so answered questions stand out more.
pub(super) fn answer_mark(state: AnswerState) -> Span<'static> {
    match state {
        AnswerState::Skipped => Span::styled("☐", theme::secondary()),
        AnswerState::Info => Span::styled("ℹ", theme::secondary()),
        AnswerState::Answered | AnswerState::Defaulted => Span::from("☒"),
    }
}

/// What the review shows for an answer, dim: the chosen options by label,
/// own text in quotes, a text answer as typed on one line, `-` for a
/// skipped question, and marks for a default and a note. A label shows its
/// Markdown on one line, rendered for the `room` the summary gets, with
/// quotes in `quote`.
pub(super) fn answer_summary(
    question: &Question,
    answer: &Answer,
    room: usize,
    quote: QuoteStyle,
) -> Vec<Span<'static>> {
    let dim = |text: String| Span::styled(text, theme::secondary());
    // A single's own text shows only where it is the answer: a chosen
    // option wins over it, as in the result.
    let own = answer
        .custom
        .as_deref()
        .filter(|_| question.kind != Kind::Single || answer.selected.is_empty())
        .filter(|text| !text.trim().is_empty())
        .map(|text| format!("\"{}\"", text.lines().collect::<Vec<_>>().join(" ⏎ ")));
    // An info item has no answer to show, and a blank note on it is none,
    // as the result leaves the item out then.
    if question.kind == Kind::Info {
        return if has_note(answer) {
            vec![dim("+note".to_owned())]
        } else {
            Vec::new()
        };
    }
    let mut summary = Vec::new();
    if !is_answered(question, answer) {
        summary.push(dim("-".to_owned()));
    } else {
        let options = question.options.as_deref().unwrap_or_default();
        let mut parts: Vec<Vec<Span<'static>>> = answer
            .selected
            .iter()
            .map(|id| {
                options.iter().find(|option| &option.id == id).map_or_else(
                    || vec![dim(id.clone())],
                    |option| label_line(&option.label, room, quote),
                )
            })
            .collect();
        parts.extend(own.map(|own| vec![dim(own)]));
        for (index, part) in parts.into_iter().enumerate() {
            if index > 0 {
                summary.push(dim(", ".to_owned()));
            }
            summary.extend(part);
        }
    }
    if answer.defaulted {
        summary.push(dim(" (default)".to_owned()));
    }
    if answer.note.is_some() {
        summary.push(dim(" +note".to_owned()));
    }
    summary
}

/// An option's `label` on one line of the summary: rendered for `room`
/// columns with quotes in `quote`, in the summary's dim under its Markdown
/// styles. Its lines join with a blank, each without the blanks at its
/// ends, and a line patine leaves wider than `room` stays whole, so the
/// cut of the summary ends it.
fn label_line(label: &str, room: usize, quote: QuoteStyle) -> Vec<Span<'static>> {
    let lines = field_source_lines(label, room, quote)
        .into_iter()
        .map(|line| {
            let spans = line
                .pieces()
                .map(|(text, style)| Span::styled(text.to_owned(), theme::secondary().patch(style)))
                .collect();
            trim_start(trim_end(spans))
        })
        .filter(|spans| !spans.is_empty());

    let mut joined = Vec::new();
    for (index, spans) in lines.enumerate() {
        if index > 0 {
            joined.push(Span::styled(" ", theme::secondary()));
        }
        joined.extend(spans);
    }
    joined
}

/// One line under the question text on how to answer it.
pub(super) fn kind_hint(question: &Question) -> String {
    let items = |count: u32| format!("{count} item{}", if count == 1 { "" } else { "s" });
    let hint = match (question.kind, question.min, question.max) {
        (Kind::Single, _, _) => "pick one".to_owned(),
        (Kind::Multi, Some(min), Some(max)) if min == max => format!("pick {}", items(min)),
        (Kind::Multi, Some(min), Some(max)) => format!("pick between {min} and {}", items(max)),
        (Kind::Multi, Some(min), None) => format!("pick at least {}", items(min)),
        (Kind::Multi, None, Some(max)) => format!("pick up to {}", items(max)),
        (Kind::Multi, None, None) => "pick any".to_owned(),
        (Kind::Text, _, _) => "type your answer".to_owned(),
        (Kind::Info, _, _) => "nothing to answer".to_owned(),
    };
    // A single takes the own answer instead of an option, which the hint
    // says. A multi takes it on top of its options; people find that row
    // on their own, so its hint leaves it out.
    let own_answer = question
        .custom
        .as_ref()
        .is_some_and(|custom| custom.is_enabled());
    if question.kind == Kind::Single && own_answer {
        format!("{hint}, or type your own")
    } else {
        hint
    }
}

/// How the header and the context hint name a key of the question, such
/// as `i` or `z`. While a field, a note or the context pane has the keys,
/// the key would be typed or taken by the pane, so `esc` comes first.
pub(super) fn question_key(state: &SessionState, key: &str) -> String {
    if state.focus() != Focus::None || state.pane_focused() {
        format!("esc, {key}")
    } else {
        key.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier, Style};

    use super::super::patine_adapter::quote_style;
    use super::*;
    use crate::format::{Answer, Question};

    fn question(json: &str) -> Question {
        serde_json::from_str(json).expect("test question parses")
    }

    /// The text of the review's summary of `answer`.
    fn summary_text(question: &Question, answer: &Answer) -> String {
        answer_summary(question, answer, 100, quote_style(false))
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// The summary of a `single` question whose option `a`, labelled
    /// `label`, is chosen, rendered for `room` columns.
    fn summary_of_label(label: &str, room: usize) -> Vec<Span<'static>> {
        let single = question(
            &serde_json::json!({"id": "s", "text": "?", "kind": "single",
                "options": [{"id": "a", "label": label}]})
            .to_string(),
        );
        let chosen = Answer {
            selected: vec!["a".into()],
            ..Answer::new("s")
        };
        answer_summary(&single, &chosen, room, quote_style(false))
    }

    /// The text of `spans`.
    fn text_of(spans: &[Span]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    /// The style of the span holding `text` in `spans`.
    fn style_of(spans: &[Span], text: &str) -> Style {
        spans
            .iter()
            .find(|span| span.content.contains(text))
            .unwrap_or_else(|| panic!("{text:?} is not in {spans:?}"))
            .style
    }

    #[test]
    fn a_label_in_the_summary_renders_its_inline_markdown() {
        let spans = summary_of_label("Keep `retry` **now**", 100);

        assert_eq!(text_of(&spans), "Keep retry now");
        assert_eq!(
            style_of(&spans, "retry"),
            Style::new().dim().fg(Color::Indexed(3)),
            "code in yellow, dim like the summary"
        );
        assert_eq!(
            style_of(&spans, "now"),
            Style::new().add_modifier(Modifier::DIM | Modifier::BOLD)
        );
    }

    #[test]
    fn a_chosen_id_without_its_option_shows_as_written() {
        let single = question(
            r#"{"id": "s", "text": "?", "kind": "single", "options": [{"id": "a", "label": "A"}]}"#,
        );
        let gone = Answer {
            selected: vec!["`gone`".into()],
            ..Answer::new("s")
        };

        assert_eq!(summary_text(&single, &gone), "`gone`");
    }

    #[test]
    fn a_label_of_several_lines_shows_on_one_line_in_the_summary() {
        assert_eq!(
            text_of(&summary_of_label("First\nsecond", 100)),
            "First second"
        );
    }

    #[test]
    fn a_label_wider_than_the_summary_shows_no_continuation_marker() {
        let label = "```\nlet total = first + second + third;\n```";

        let text = text_of(&summary_of_label(label, 20));
        assert!(!text.contains('↪'), "{text:?}");
        assert!(!text.contains('`'), "{text:?}");
    }

    #[test]
    fn only_the_current_row_shows_the_pointer_and_every_row_starts_in_the_same_column() {
        use super::super::source_line::columns;

        assert_eq!(pointer(true), Span::from("❯ "));
        assert_eq!(pointer(false), Span::from("  "));
        assert_eq!(columns(&pointer(false).content), columns(POINTER));
    }

    #[test]
    fn a_question_is_marked_answered_unless_it_is_skipped_and_then_dim() {
        use ratatui::style::Style;

        assert_eq!(
            answer_mark(AnswerState::Skipped),
            Span::styled("☐", Style::new().dim())
        );
        assert_eq!(answer_mark(AnswerState::Answered), Span::from("☒"));
        assert_eq!(answer_mark(AnswerState::Defaulted), Span::from("☒"));
    }

    #[test]
    fn an_info_item_is_marked_as_one_and_dim() {
        use ratatui::style::Style;

        // There is nothing to do on it, so it recedes like a question
        // still open, and answered questions stand out more.
        assert_eq!(
            answer_mark(AnswerState::Info),
            Span::styled("ℹ", Style::new().dim())
        );
    }

    #[test]
    fn the_review_shows_only_the_note_of_an_info_item() {
        let info = question(r#"{"id": "i", "text": "Done.", "kind": "info"}"#);
        let noted = |note: &str| Answer {
            note: Some(note.to_owned()),
            ..Answer::new("i")
        };

        assert_eq!(summary_text(&info, &Answer::new("i")), "");
        assert_eq!(summary_text(&info, &noted("why")), "+note");
        assert_eq!(
            summary_text(&info, &noted(" \n")),
            "",
            "a blank note is none"
        );
    }

    #[test]
    fn the_question_list_shows_what_was_answered() {
        let multi = question(
            r#"{"id": "m", "text": "?", "kind": "multi", "custom": true,
                "options": [{"id": "x", "label": "Extra"}, {"id": "y", "label": "Yes"}]}"#,
        );
        let chosen = Answer {
            selected: vec!["x".into(), "y".into()],
            ..Answer::new("m")
        };
        let typed = Answer {
            custom: Some("own".into()),
            note: Some("why".into()),
            ..Answer::new("m")
        };
        let defaulted = Answer {
            selected: vec!["x".into()],
            defaulted: true,
            ..Answer::new("m")
        };

        assert_eq!(summary_text(&multi, &chosen), "Extra, Yes");
        assert_eq!(summary_text(&multi, &typed), "\"own\" +note");
        assert_eq!(summary_text(&multi, &defaulted), "Extra (default)");
        assert_eq!(summary_text(&multi, &Answer::new("m")), "-");
    }

    #[test]
    fn a_typed_answer_next_to_options_shows_both() {
        let multi = question(
            r#"{"id": "m", "text": "?", "kind": "multi", "custom": true, "options": [{"id": "x", "label": "X"}]}"#,
        );
        let both = Answer {
            selected: vec!["x".into()],
            custom: Some("more".into()),
            ..Answer::new("m")
        };

        assert_eq!(summary_text(&multi, &both), "X, \"more\"");
    }

    #[test]
    fn a_single_shows_its_own_text_only_where_it_counts() {
        // Text that is not the pick is not the answer, so the list shows
        // what a submit would send.
        let single = question(
            r#"{"id": "s", "text": "?", "kind": "single", "custom": true,
                "options": [{"id": "a", "label": "A"}]}"#,
        );
        let typed = |selected: &[&str], custom_unpicked: bool| Answer {
            selected: selected.iter().map(|id| (*id).to_owned()).collect(),
            custom: Some("own".into()),
            custom_unpicked,
            ..Answer::new("s")
        };

        assert_eq!(summary_text(&single, &typed(&["a"], false)), "A");
        assert_eq!(summary_text(&single, &typed(&[], false)), "\"own\"");
        assert_eq!(summary_text(&single, &typed(&[], true)), "-");
    }

    #[test]
    fn a_text_answer_shows_as_typed() {
        let text = question(r#"{"id": "t", "text": "?", "kind": "text"}"#);
        let typed = Answer {
            custom: Some("done\nand dusted".into()),
            ..Answer::new("t")
        };

        assert_eq!(summary_text(&text, &typed), "\"done ⏎ and dusted\"");
    }

    #[test]
    fn hints_say_how_to_answer() {
        let hint = |json: &str| kind_hint(&question(json));

        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "single", "options": []}"#),
            "pick one"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "single", "custom": true, "options": []}"#),
            "pick one, or type your own"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "options": []}"#),
            "pick any"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "min": 1, "max": 2, "options": []}"#),
            "pick between 1 and 2 items"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "min": 2, "max": 2, "options": []}"#),
            "pick 2 items"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "min": 2, "options": []}"#),
            "pick at least 2 items"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "max": 1, "options": []}"#),
            "pick up to 1 item"
        );
        // People find the own-answer row on their own, so a multi's hint
        // leaves it out.
        assert_eq!(
            hint(
                r#"{"id": "q", "text": "?", "kind": "multi", "min": 2, "max": 3, "custom": true, "options": []}"#
            ),
            "pick between 2 and 3 items"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "text"}"#),
            "type your answer"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "info"}"#),
            "nothing to answer"
        );
    }

    #[test]
    fn a_question_key_needs_esc_first_wherever_something_else_has_the_keys() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        use crate::tui::{Extent, Page, SessionState};

        let session = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single", "custom": true,
            "context": [{"type": "code", "content": "x"}], "options": [{"id": "a", "label": "A"}]}]}"#;
        let mut state =
            SessionState::new("keys", serde_json::from_str(session).expect("parses"), None);
        state.set_pane_page(Some(crate::tui::PanePage::shown(Page {
            rows: Extent { shown: 5, total: 5 },
            columns: Extent::default(),
        })));
        assert_eq!(question_key(&state, "i"), "i");
        let mut press = |code: KeyCode| {
            state.handle(KeyEvent::new(code, KeyModifiers::NONE));
            question_key(&state, "i")
        };

        assert_eq!(press(KeyCode::Char('n')), "esc, i", "in a note");
        assert_eq!(press(KeyCode::Esc), "i");
        assert_eq!(press(KeyCode::Tab), "esc, i", "in the focused pane");
        assert_eq!(press(KeyCode::Esc), "i");
        assert_eq!(press(KeyCode::Down), "esc, i", "in the own answer");
    }
}
