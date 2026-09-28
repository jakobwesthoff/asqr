// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The answer-state rules (spec section 5.2): when a question counts as
//! answered, and the shape its answer takes in a result. The TUI keeps its
//! working answers in the draft shape; these functions turn them into what
//! the asker reads.

use super::{Answer, Kind, Question, Session};

/// Typed text counts only when it is more than whitespace.
fn has_text(answer: &Answer) -> bool {
    answer
        .custom
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty())
}

/// Whether `answer` answers `question`. A note never does.
pub fn is_answered(question: &Question, answer: &Answer) -> bool {
    match question.kind {
        Kind::Single => !answer.selected.is_empty() || has_text(answer),
        Kind::Multi => {
            // Custom text answers the question on its own. `min` and `max`
            // bound the selected options, so they only apply once one is
            // selected, and only decide when there is no text.
            let count = answer.selected.len() as u32;
            has_text(answer)
                || (count > 0
                    && question.min.is_none_or(|min| count >= min)
                    && question.max.is_none_or(|max| count <= max))
        }
        Kind::Text => has_text(answer),
    }
}

/// The answers of a result: every question of `session` exactly once, in
/// session order, shaped per kind. `answers` are the working answers, in any
/// order and possibly incomplete.
pub fn result_answers(session: &Session, answers: &[Answer]) -> Vec<Answer> {
    session
        .questions
        .iter()
        .map(|question| {
            let working = answers.iter().find(|answer| answer.question == question.id);
            match working {
                Some(answer) if is_answered(question, answer) => shaped(question, answer),
                Some(answer) => Answer {
                    note: answer.note.clone(),
                    ..Answer::skipped(&question.id)
                },
                None => Answer::skipped(&question.id),
            }
        })
        .collect()
}

/// An answered question's answer with only the fields its kind allows.
fn shaped(question: &Question, answer: &Answer) -> Answer {
    let custom = if has_text(answer) {
        answer.custom.clone()
    } else {
        None
    };
    let (selected, custom) = match question.kind {
        // One option or custom text, never both; the TUI keeps them
        // exclusive, and the selection wins should both arrive.
        Kind::Single if !answer.selected.is_empty() => (answer.selected.clone(), None),
        Kind::Single => (Vec::new(), custom),
        Kind::Multi => (answer.selected.clone(), custom),
        Kind::Text => (Vec::new(), custom),
    };
    Answer {
        question: question.id.clone(),
        selected,
        custom,
        note: answer.note.clone(),
        defaulted: answer.defaulted,
        skipped: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(questions: &str) -> Session {
        serde_json::from_str(&format!(r#"{{"asqr": 1, "questions": [{questions}]}}"#))
            .expect("test session parses")
    }

    const SINGLE: &str = r#"{"id": "s", "text": "?", "kind": "single", "custom": true,
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}"#;
    const MULTI: &str = r#"{"id": "m", "text": "?", "kind": "multi", "min": 2, "max": 2, "custom": true,
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}, {"id": "c", "label": "C"}]}"#;
    const TEXT: &str = r#"{"id": "t", "text": "?", "kind": "text"}"#;

    fn question(json: &str) -> Question {
        serde_json::from_str(json).expect("test question parses")
    }

    fn selected(question: &str, ids: &[&str]) -> Answer {
        Answer {
            selected: ids.iter().map(|id| (*id).to_owned()).collect(),
            ..Answer::new(question)
        }
    }

    fn custom(question: &str, text: &str) -> Answer {
        Answer {
            custom: Some(text.to_owned()),
            ..Answer::new(question)
        }
    }

    #[test]
    fn single_is_answered_by_an_option_or_custom_text() {
        let single = question(SINGLE);

        assert!(is_answered(&single, &selected("s", &["a"])));
        assert!(is_answered(&single, &custom("s", "own")));
        assert!(!is_answered(&single, &Answer::new("s")));
    }

    #[test]
    fn custom_text_that_is_empty_after_trimming_answers_nothing() {
        assert!(!is_answered(&question(SINGLE), &custom("s", "  \n ")));
        assert!(!is_answered(&question(TEXT), &custom("t", "\t")));
    }

    #[test]
    fn a_note_alone_answers_nothing() {
        let noted = Answer {
            note: Some("skip, because".into()),
            ..Answer::new("s")
        };

        assert!(!is_answered(&question(SINGLE), &noted));
    }

    #[test]
    fn multi_bounds_apply_once_an_option_is_selected() {
        let multi = question(MULTI);

        assert!(!is_answered(&multi, &selected("m", &["a"])), "below min");
        assert!(is_answered(&multi, &selected("m", &["a", "b"])));
        assert!(
            !is_answered(&multi, &selected("m", &["a", "b", "c"])),
            "above max"
        );
        // The bounds count options; custom text alone answers the question.
        assert!(is_answered(&multi, &custom("m", "own")));
    }

    #[test]
    fn custom_text_answers_a_multi_whatever_is_selected() {
        let multi = question(MULTI);
        let below_min_with_text = Answer {
            custom: Some("own".into()),
            ..selected("m", &["a"])
        };

        assert!(is_answered(&multi, &below_min_with_text));
        assert_eq!(
            result_answers(&session(MULTI), std::slice::from_ref(&below_min_with_text)),
            [below_min_with_text],
            "the result keeps the options next to the text"
        );
    }

    #[test]
    fn text_is_answered_by_typed_text() {
        assert!(is_answered(&question(TEXT), &custom("t", "typed")));
        assert!(!is_answered(&question(TEXT), &Answer::new("t")));
    }

    #[test]
    fn the_result_lists_every_question_in_session_order() {
        let session = session(&[SINGLE, MULTI, TEXT].join(","));
        let answers = [custom("t", "typed"), selected("s", &["a"])];

        let result = result_answers(&session, &answers);

        assert_eq!(
            result,
            [
                selected("s", &["a"]),
                Answer::skipped("m"),
                custom("t", "typed"),
            ]
        );
    }

    #[test]
    fn a_skipped_question_keeps_only_its_note() {
        let session = session(MULTI);
        let below_min = Answer {
            note: Some("none fit".into()),
            ..selected("m", &["a"])
        };

        assert_eq!(
            result_answers(&session, &[below_min]),
            [Answer {
                note: Some("none fit".into()),
                ..Answer::skipped("m")
            }]
        );
    }

    #[test]
    fn the_result_drops_empty_custom_text_and_keeps_the_default_flag() {
        let session = session(SINGLE);
        let answer = Answer {
            custom: Some("   ".into()),
            defaulted: true,
            ..selected("s", &["b"])
        };

        assert_eq!(
            result_answers(&session, &[answer]),
            [Answer {
                defaulted: true,
                ..selected("s", &["b"])
            }]
        );
    }

    #[test]
    fn a_single_answer_is_an_option_or_custom_text_never_both() {
        // The TUI keeps the two exclusive; the result enforces the shape
        // anyway, preferring the selected option.
        let session = session(SINGLE);
        let both = Answer {
            custom: Some("own".into()),
            ..selected("s", &["a"])
        };

        assert_eq!(result_answers(&session, &[both]), [selected("s", &["a"])]);
    }

    #[test]
    fn a_single_answered_by_custom_text_keeps_the_text() {
        let session = session(SINGLE);

        assert_eq!(
            result_answers(&session, &[custom("s", "own")]),
            [custom("s", "own")]
        );
    }

    #[test]
    fn a_text_answer_has_no_selection() {
        let session = session(TEXT);
        let odd = Answer {
            custom: Some("typed".into()),
            ..selected("t", &["a"])
        };

        assert_eq!(result_answers(&session, &[odd]), [custom("t", "typed")]);
    }

    #[test]
    fn a_multi_answer_keeps_options_and_custom_text_together() {
        let session = session(MULTI);
        let both = Answer {
            custom: Some("own".into()),
            ..selected("m", &["a", "c"])
        };

        assert_eq!(
            result_answers(&session, std::slice::from_ref(&both)),
            [both]
        );
    }
}
