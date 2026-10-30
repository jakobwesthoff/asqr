// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The answer-state rules (spec section 4.2): when a question counts as
//! answered, and the shape its answer takes in a result. The TUI keeps its
//! working answers in the draft shape; these functions turn them into what
//! the asker reads.

use super::{Answer, Kind, Question, Session};

/// Typed text counts only when it is more than whitespace.
pub fn has_text(answer: &Answer) -> bool {
    answer
        .custom
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty())
}

/// Whether `question` has a field for typed text: the own-answer row of a
/// `single` or `multi` while `custom` is on, or the answer of a `text`
/// question. Text a draft kept for a question without one is no answer.
pub fn takes_own_text(question: &Question) -> bool {
    match question.kind {
        Kind::Text => true,
        Kind::Single | Kind::Multi => question
            .custom
            .as_ref()
            .is_some_and(|custom| custom.is_enabled()),
        Kind::Info => false,
    }
}

/// Whether `answer` answers `question`. A note never does.
pub fn is_answered(question: &Question, answer: &Answer) -> bool {
    let own_text = takes_own_text(question) && has_text(answer);
    match question.kind {
        // Own text answers a single only while it is the pick.
        Kind::Single => !answer.selected.is_empty() || (own_text && !answer.custom_unpicked),
        Kind::Multi => {
            // Custom text answers the question on its own. `min` and `max`
            // bound the selected options, so they only apply once one is
            // selected, and only decide when there is no text.
            let count = answer.selected.len() as u32;
            own_text
                || (count > 0
                    && question.min.is_none_or(|min| count >= min)
                    && question.max.is_none_or(|max| count <= max))
        }
        Kind::Text => has_text(answer),
        // An info item takes no answer, so nothing a draft carries answers
        // it.
        Kind::Info => false,
    }
}

/// Whether `answer` carries a note that is more than whitespace.
pub fn has_note(answer: &Answer) -> bool {
    answer
        .note
        .as_deref()
        .is_some_and(|note| !note.trim().is_empty())
}

/// The answers of a result: every question of `session` exactly once, in
/// session order, shaped per kind, apart from info items, which take no
/// answer and appear only with a note (spec section 4.1). `answers` are the
/// working answers, in any order and possibly incomplete.
pub fn result_answers(session: &Session, answers: &[Answer]) -> Vec<Answer> {
    session
        .questions
        .iter()
        .filter_map(|question| {
            let working = answers.iter().find(|answer| answer.question == question.id);
            if question.kind == Kind::Info {
                return working
                    .filter(|answer| has_note(answer))
                    .map(|answer| shaped(question, answer));
            }
            let answer = match working {
                Some(answer) if is_answered(question, answer) => shaped(question, answer),
                Some(answer) => Answer {
                    note: answer.note.clone(),
                    ..Answer::skipped(&question.id)
                },
                None => Answer::skipped(&question.id),
            };
            Some(answer)
        })
        .collect()
}

/// The answers of a rejected session's result (spec section 4.1): those of
/// [`result_answers`] for the questions with an answer or a note that is
/// not blank, in session order. An untouched default stays, flagged, so
/// the asker can tell it from a default the person removed.
pub fn rejected_answers(session: &Session, answers: &[Answer]) -> Vec<Answer> {
    result_answers(session, answers)
        .into_iter()
        .filter(|answer| has_note(answer) || !answer.skipped)
        .collect()
}

/// An answered question's answer with only the fields its kind allows; an
/// info item's is its note alone.
fn shaped(question: &Question, answer: &Answer) -> Answer {
    let custom = if takes_own_text(question) && has_text(answer) {
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
        Kind::Info => (Vec::new(), None),
    };
    Answer {
        question: question.id.clone(),
        selected,
        custom,
        // Drafts alone mark own text as unpicked and a question as cleared.
        custom_unpicked: false,
        cleared: false,
        note: answer.note.clone(),
        defaulted: answer.defaulted,
        skipped: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(questions: &str) -> Session {
        serde_json::from_str(&format!(r#"{{"asqr": 2, "questions": [{questions}]}}"#))
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

    /// A draft's answer to `s` whose own text `own` the person unpicked,
    /// with option ids `selected`.
    fn unpicked(selected: &[&str]) -> Answer {
        let json = serde_json::json!({
            "question": "s",
            "selected": selected,
            "custom": "own",
            "custom_unpicked": true,
        });
        serde_json::from_value(json).expect("a draft answer parses")
    }

    #[test]
    fn unpicked_own_text_answers_nothing_and_the_result_leaves_it_out() {
        let single = question(SINGLE);

        assert!(!is_answered(&single, &unpicked(&[])));
        assert_eq!(
            result_answers(&session(SINGLE), &[unpicked(&[])]),
            [Answer::skipped("s")]
        );
    }

    #[test]
    fn a_result_never_marks_own_text_as_unpicked() {
        // An option picked after the text was unpicked answers the
        // question; the mark belongs to drafts alone.
        assert_eq!(
            result_answers(&session(SINGLE), &[unpicked(&["a"])]),
            [selected("s", &["a"])]
        );
    }

    // -----------------------------------------------------------------
    // Own text on a question that takes none
    // -----------------------------------------------------------------
    //
    // A draft written while a question still took an own answer can carry
    // its text into a session file that turned `custom` off.

    const SINGLE_WITHOUT_OWN: &str = r#"{"id": "s", "text": "?", "kind": "single", "custom": false,
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}"#;
    const MULTI_WITHOUT_OWN: &str = r#"{"id": "m", "text": "?", "kind": "multi",
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}"#;

    #[test]
    fn own_text_does_not_answer_a_single_without_an_own_answer() {
        assert!(!is_answered(
            &question(SINGLE_WITHOUT_OWN),
            &custom("s", "own")
        ));
    }

    #[test]
    fn own_text_does_not_answer_a_multi_without_an_own_answer() {
        assert!(!is_answered(
            &question(MULTI_WITHOUT_OWN),
            &custom("m", "own")
        ));
    }

    #[test]
    fn the_result_leaves_out_own_text_on_a_single_without_an_own_answer() {
        assert_eq!(
            result_answers(&session(SINGLE_WITHOUT_OWN), &[custom("s", "own")]),
            [Answer::skipped("s")]
        );
    }

    #[test]
    fn the_result_leaves_out_own_text_on_a_multi_without_an_own_answer() {
        let both = Answer {
            custom: Some("own".into()),
            ..selected("m", &["a", "b"])
        };

        assert_eq!(
            result_answers(&session(MULTI_WITHOUT_OWN), &[both]),
            [selected("m", &["a", "b"])]
        );
    }

    #[test]
    fn a_result_never_carries_the_cleared_mark() {
        // The mark belongs to drafts alone, whatever else the answer holds.
        let marked = |answer: Answer| Answer {
            cleared: true,
            ..answer
        };

        assert_eq!(
            result_answers(
                &session(&[SINGLE, TEXT].join(",")),
                &[marked(selected("s", &["a"])), marked(Answer::new("t"))]
            ),
            [selected("s", &["a"]), Answer::skipped("t")]
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

    // -----------------------------------------------------------------
    // Info items
    // -----------------------------------------------------------------

    const INFO: &str = r#"{"id": "i", "text": "All checks passed.", "kind": "info"}"#;

    #[test]
    fn an_info_item_is_never_answered() {
        // Not even by text a draft carried over from a question that had
        // the same id.
        assert!(!is_answered(&question(INFO), &custom("i", "typed")));
        assert!(!is_answered(&question(INFO), &Answer::new("i")));
    }

    #[test]
    fn the_result_leaves_out_an_info_item_without_a_note() {
        let session = session(&[INFO, TEXT].join(","));

        assert_eq!(
            result_answers(&session, &[Answer::new("i"), custom("t", "typed")]),
            [custom("t", "typed")]
        );
    }

    #[test]
    fn the_result_lists_an_info_item_with_a_note_as_the_note_alone() {
        let session = session(INFO);
        let noted_with_text = Answer {
            custom: Some("left over".into()),
            ..noted(Answer::new("i"), " looks good\n")
        };

        assert_eq!(
            result_answers(&session, &[noted_with_text]),
            [Answer {
                note: Some(" looks good\n".into()),
                ..Answer::new("i")
            }],
            "the note as typed, nothing else"
        );
    }

    #[test]
    fn a_blank_note_on_an_info_item_leaves_it_out() {
        let session = session(INFO);

        assert_eq!(
            result_answers(&session, &[noted(Answer::new("i"), " \n ")]),
            []
        );
    }

    #[test]
    fn a_rejected_result_keeps_a_noted_info_item_and_leaves_out_the_others() {
        let session = session(&[INFO, r#"{"id": "j", "text": "More.", "kind": "info"}"#].join(","));
        let answers = [noted(Answer::new("i"), "why"), Answer::new("j")];

        assert_eq!(
            rejected_answers(&session, &answers),
            [noted(Answer::new("i"), "why")]
        );
    }

    // -----------------------------------------------------------------
    // The answers of a rejected session
    // -----------------------------------------------------------------

    /// A single question whose option b is the default.
    const DEFAULTED: &str = r#"{"id": "d", "text": "?", "kind": "single",
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B", "default": true}]}"#;

    fn noted(answer: Answer, note: &str) -> Answer {
        Answer {
            note: Some(note.to_owned()),
            ..answer
        }
    }

    #[test]
    fn a_rejected_result_keeps_an_answered_question_in_the_submitted_shape() {
        let session = session(SINGLE);
        let both = Answer {
            custom: Some("own".into()),
            ..selected("s", &["a"])
        };

        assert_eq!(rejected_answers(&session, &[both]), [selected("s", &["a"])]);
    }

    #[test]
    fn a_rejected_result_keeps_a_skipped_question_with_its_note() {
        let session = session(MULTI);
        let below_min = noted(selected("m", &["a"]), "none fit");

        assert_eq!(
            rejected_answers(&session, &[below_min]),
            [noted(Answer::skipped("m"), "none fit")]
        );
    }

    #[test]
    fn a_rejected_result_leaves_out_a_skipped_question_without_a_note() {
        let session = session(&[SINGLE, MULTI, TEXT].join(","));
        let answers = [
            selected("s", &["a"]),
            Answer::new("m"),
            noted(Answer::new("t"), "  \n "),
        ];

        assert_eq!(
            rejected_answers(&session, &answers),
            [selected("s", &["a"])],
            "a blank note is no note"
        );
    }

    #[test]
    fn a_rejected_result_keeps_an_untouched_default_with_its_flag() {
        let session = session(DEFAULTED);
        let untouched = Answer {
            defaulted: true,
            ..selected("d", &["b"])
        };

        assert_eq!(
            rejected_answers(&session, std::slice::from_ref(&untouched)),
            [untouched]
        );
    }
}
