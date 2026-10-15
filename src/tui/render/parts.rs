// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Texts the screen is built from, kept apart from the layout so they can
//! be tested on their own.

use crate::format::{Answer, Kind, Question, is_answered};
use crate::tui::{Focus, SessionState};

/// What the review shows for an answer: the chosen options by label, own
/// text in quotes, a text answer as typed on one line, `-` for a skipped
/// question, and marks for a default and a note.
pub fn answer_summary(question: &Question, answer: &Answer) -> String {
    let own = answer
        .custom
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .map(|text| format!("\"{}\"", text.lines().collect::<Vec<_>>().join(" ⏎ ")));
    let mut summary = if !is_answered(question, answer) {
        "-".to_owned()
    } else {
        let options = question.options.as_deref().unwrap_or_default();
        let mut parts: Vec<String> = answer
            .selected
            .iter()
            .map(|id| {
                options
                    .iter()
                    .find(|option| &option.id == id)
                    .map_or_else(|| id.clone(), |option| option.label.clone())
            })
            .collect();
        parts.extend(own);
        parts.join(", ")
    };
    if answer.defaulted {
        summary.push_str(" (default)");
    }
    if answer.note.is_some() {
        summary.push_str(" +note");
    }
    summary
}

/// One line under the question text on how to answer it.
pub fn kind_hint(question: &Question) -> String {
    let items = |count: u32| format!("{count} item{}", if count == 1 { "" } else { "s" });
    let hint = match (question.kind, question.min, question.max) {
        (Kind::Single, _, _) => "pick one".to_owned(),
        (Kind::Multi, Some(min), Some(max)) if min == max => format!("pick {}", items(min)),
        (Kind::Multi, Some(min), Some(max)) => format!("pick between {min} and {}", items(max)),
        (Kind::Multi, Some(min), None) => format!("pick at least {}", items(min)),
        (Kind::Multi, None, Some(max)) => format!("pick up to {}", items(max)),
        (Kind::Multi, None, None) => "pick any".to_owned(),
        (Kind::Text, _, _) => "type your answer".to_owned(),
    };
    // A single takes the own answer instead of an option, which the hint
    // says. A multi takes it on top of its options; people find that row
    // on their own, so its hint leaves it out (user, 2026-09-28).
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
pub fn question_key(state: &SessionState, key: &str) -> String {
    if state.focus() != Focus::None || state.pane_focused() {
        format!("esc, {key}")
    } else {
        key.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Answer, Question};

    fn question(json: &str) -> Question {
        serde_json::from_str(json).expect("test question parses")
    }

    #[test]
    fn the_question_list_shows_what_was_answered() {
        let multi = question(
            r#"{"id": "m", "text": "?", "kind": "multi",
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

        assert_eq!(answer_summary(&multi, &chosen), "Extra, Yes");
        assert_eq!(answer_summary(&multi, &typed), "\"own\" +note");
        assert_eq!(answer_summary(&multi, &defaulted), "Extra (default)");
        assert_eq!(answer_summary(&multi, &Answer::new("m")), "-");
    }

    #[test]
    fn a_typed_answer_next_to_options_shows_both() {
        let multi = question(
            r#"{"id": "m", "text": "?", "kind": "multi", "options": [{"id": "x", "label": "X"}]}"#,
        );
        let both = Answer {
            selected: vec!["x".into()],
            custom: Some("more".into()),
            ..Answer::new("m")
        };

        assert_eq!(answer_summary(&multi, &both), "X, \"more\"");
    }

    #[test]
    fn a_text_answer_shows_as_typed() {
        let text = question(r#"{"id": "t", "text": "?", "kind": "text"}"#);
        let typed = Answer {
            custom: Some("done\nand dusted".into()),
            ..Answer::new("t")
        };

        assert_eq!(answer_summary(&text, &typed), "\"done ⏎ and dusted\"");
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
        // leaves it out (user, 2026-09-28).
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
    }

    #[test]
    fn a_question_key_needs_esc_first_wherever_something_else_has_the_keys() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        use crate::tui::{Extent, Page, SessionState};

        let session = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single", "custom": true,
            "context": [{"type": "code", "content": "x"}], "options": [{"id": "a", "label": "A"}]}]}"#;
        let mut state =
            SessionState::new("keys", serde_json::from_str(session).expect("parses"), None);
        state.set_pane_page(Some(Page {
            rows: Extent { shown: 5, total: 5 },
            columns: Extent::default(),
        }));
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
