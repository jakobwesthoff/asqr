// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Texts the screen is built from, kept apart from the layout so they can
//! be tested on their own.

use crate::format::{Answer, Kind, Length, Question, is_answered};

/// How a length counter is coloured (spec section 7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Within the target.
    Fine,
    /// Above the target, up to `warn`.
    Warn,
    /// Above `warn`, or at the hard limit.
    Over,
}

/// The counter text for `length` characters against `limits`: the length
/// over the end of the target range (or `warn`, or `max`), `!` above the
/// target and `max` at the hard limit. The text says everything the
/// colour says, so it works without colour.
pub fn counter(length: usize, limits: &Length) -> (String, Level) {
    let length32 = u32::try_from(length).unwrap_or(u32::MAX);
    let target_end = limits.target.map(|(_, end)| end);
    let above_target = target_end.is_some_and(|end| length32 > end);
    let at_max = limits.max.is_some_and(|max| length32 >= max);

    let mut text = match target_end.or(limits.warn).or(limits.max) {
        Some(reference) => format!("{length}/{reference}"),
        None => length.to_string(),
    };
    if above_target {
        text.push_str(" !");
    }
    if at_max {
        text.push_str(" max");
    }

    let level = if at_max || limits.warn.is_some_and(|warn| length32 > warn) {
        Level::Over
    } else if above_target {
        Level::Warn
    } else {
        Level::Fine
    };
    (text, level)
}

/// The short state of a question in the question list: the chosen option
/// ids, `own` or `typed` for typed text, `default`, or `-` while it is not
/// answered; `+note` when it carries a note.
pub fn answer_summary(question: &Question, answer: &Answer) -> String {
    let typed = answer
        .custom
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty());
    let mut summary = if !is_answered(question, answer) {
        "-".to_owned()
    } else if answer.defaulted {
        "default".to_owned()
    } else if question.kind == Kind::Text {
        "typed".to_owned()
    } else {
        let mut parts: Vec<&str> = answer.selected.iter().map(String::as_str).collect();
        if typed {
            parts.push("own");
        }
        parts.join(",")
    };
    if answer.note.is_some() {
        summary.push_str(" +note");
    }
    summary
}

/// One line under the question text on how to answer it.
pub fn kind_hint(question: &Question) -> String {
    let mut hint = match (question.kind, question.min, question.max) {
        (Kind::Single, _, _) => "pick one".to_owned(),
        (Kind::Multi, Some(min), Some(max)) => format!("pick {min} to {max}"),
        (Kind::Multi, Some(min), None) => format!("pick at least {min}"),
        (Kind::Multi, None, Some(max)) => format!("pick up to {max}"),
        (Kind::Multi, None, None) => "pick any".to_owned(),
        (Kind::Text, _, _) => "type your answer with enter or c".to_owned(),
    };
    if question.kind != Kind::Text
        && question
            .custom
            .as_ref()
            .is_some_and(|custom| custom.is_enabled())
    {
        hint.push_str(", or type your own with c");
    }
    if question.required {
        hint.push_str(" · required");
    }
    hint
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Answer, Length, Question};

    fn limits(target: Option<(u32, u32)>, warn: Option<u32>, max: Option<u32>) -> Length {
        Length { target, warn, max }
    }

    #[test]
    fn the_counter_reads_against_the_target_end() {
        let length = limits(Some((80, 125)), Some(145), Some(175));

        assert_eq!(counter(112, &length), ("112/125".to_owned(), Level::Fine));
        assert_eq!(counter(140, &length), ("140/125 !".to_owned(), Level::Warn));
        assert_eq!(counter(150, &length), ("150/125 !".to_owned(), Level::Over));
        assert_eq!(
            counter(175, &length),
            ("175/125 ! max".to_owned(), Level::Over)
        );
    }

    #[test]
    fn the_counter_falls_back_to_warn_and_max() {
        assert_eq!(
            counter(3, &limits(None, Some(10), Some(20))),
            ("3/10".to_owned(), Level::Fine)
        );
        assert_eq!(
            counter(12, &limits(None, Some(10), None)),
            ("12/10".to_owned(), Level::Over)
        );
        assert_eq!(
            counter(5, &limits(None, None, Some(5))),
            ("5/5 max".to_owned(), Level::Over)
        );
        assert_eq!(
            counter(2, &limits(None, None, None)),
            ("2".to_owned(), Level::Fine)
        );
    }

    fn question(json: &str) -> Question {
        serde_json::from_str(json).expect("test question parses")
    }

    #[test]
    fn the_question_list_shows_what_was_answered() {
        let multi = question(
            r#"{"id": "m", "text": "?", "kind": "multi",
                "options": [{"id": "x", "label": "X"}, {"id": "y", "label": "Y"}]}"#,
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

        assert_eq!(answer_summary(&multi, &chosen), "x,y");
        assert_eq!(answer_summary(&multi, &typed), "own +note");
        assert_eq!(answer_summary(&multi, &defaulted), "default");
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

        assert_eq!(answer_summary(&multi, &both), "x,own");
    }

    #[test]
    fn a_text_answer_shows_as_typed() {
        let text = question(r#"{"id": "t", "text": "?", "kind": "text"}"#);
        let typed = Answer {
            custom: Some("done".into()),
            ..Answer::new("t")
        };

        assert_eq!(answer_summary(&text, &typed), "typed");
    }

    #[test]
    fn hints_say_how_to_answer() {
        let hint = |json: &str| kind_hint(&question(json));

        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "single", "options": []}"#),
            "pick one"
        );
        assert_eq!(
            hint(
                r#"{"id": "q", "text": "?", "kind": "single", "custom": true, "required": true, "options": []}"#
            ),
            "pick one, or type your own with c · required"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "options": []}"#),
            "pick any"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "min": 1, "max": 2, "options": []}"#),
            "pick 1 to 2"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "min": 2, "options": []}"#),
            "pick at least 2"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "multi", "max": 3, "options": []}"#),
            "pick up to 3"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "text"}"#),
            "type your answer with enter or c"
        );
    }
}
