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
    /// Above `warn`.
    Over,
}

/// The counter text for `length` characters against `limits`: the length
/// over the end of the target range (or over `warn` without a target), `!`
/// above the target and `!!` above `warn`. The text says everything the
/// colour says, so it works without colour.
pub fn counter(length: usize, limits: &Length) -> (String, Level) {
    let length32 = u32::try_from(length).unwrap_or(u32::MAX);
    let target_end = limits.target.map(|(_, end)| end);
    let above_warn = limits.warn.is_some_and(|warn| length32 > warn);
    let above_target = target_end.is_some_and(|end| length32 > end);

    let mut text = match target_end.or(limits.warn) {
        Some(reference) => format!("{length}/{reference}"),
        None => length.to_string(),
    };
    let level = if above_warn {
        text.push_str(" !!");
        Level::Over
    } else if above_target {
        text.push_str(" !");
        Level::Warn
    } else {
        Level::Fine
    };
    (text, level)
}

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

/// `text` in at most `room` characters, ending in `…` when it had to be
/// cut.
pub fn cut(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_owned();
    }
    if room == 0 {
        return String::new();
    }
    let kept: String = text.chars().take(room - 1).collect();
    format!("{kept}…")
}

/// One line under the question text on how to answer it.
pub fn kind_hint(question: &Question) -> String {
    let mut hint = match (question.kind, question.min, question.max) {
        (Kind::Single, _, _) => "pick one".to_owned(),
        (Kind::Multi, Some(min), Some(max)) => format!("pick {min} to {max}"),
        (Kind::Multi, Some(min), None) => format!("pick at least {min}"),
        (Kind::Multi, None, Some(max)) => format!("pick up to {max}"),
        (Kind::Multi, None, None) => "pick any".to_owned(),
        (Kind::Text, _, _) => "type your answer".to_owned(),
    };
    if question.kind != Kind::Text
        && question
            .custom
            .as_ref()
            .is_some_and(|custom| custom.is_enabled())
    {
        // A single takes the own answer instead of an option; a multi takes
        // it on top of the options, outside `min` and `max`.
        hint.push_str(if question.kind == Kind::Single {
            ", or type your own"
        } else {
            ", and type your own if you like"
        });
    }
    hint
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Answer, Length, Question};

    fn limits(target: Option<(u32, u32)>, warn: Option<u32>) -> Length {
        Length { target, warn }
    }

    #[test]
    fn the_counter_reads_against_the_target_end() {
        let length = limits(Some((80, 125)), Some(145));

        assert_eq!(counter(112, &length), ("112/125".to_owned(), Level::Fine));
        assert_eq!(counter(125, &length), ("125/125".to_owned(), Level::Fine));
        assert_eq!(counter(140, &length), ("140/125 !".to_owned(), Level::Warn));
        assert_eq!(counter(145, &length), ("145/125 !".to_owned(), Level::Warn));
        assert_eq!(
            counter(182, &length),
            ("182/125 !!".to_owned(), Level::Over)
        );
    }

    #[test]
    fn the_counter_falls_back_to_warn() {
        assert_eq!(
            counter(3, &limits(None, Some(10))),
            ("3/10".to_owned(), Level::Fine)
        );
        assert_eq!(
            counter(12, &limits(None, Some(10))),
            ("12/10 !!".to_owned(), Level::Over)
        );
        assert_eq!(
            counter(200, &limits(Some((1, 5)), None)),
            ("200/5 !".to_owned(), Level::Warn)
        );
        assert_eq!(
            counter(2, &limits(None, None)),
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
    fn cutting_keeps_text_within_its_room() {
        assert_eq!(cut("Changelog, Blog post", 30), "Changelog, Blog post");
        assert_eq!(cut("Changelog, Blog post", 12), "Changelog, …");
        assert_eq!(cut("Changelog", 1), "…");
        assert_eq!(cut("Changelog", 0), "");
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
        // The own answer of a multi comes on top of the picked options.
        assert_eq!(
            hint(
                r#"{"id": "q", "text": "?", "kind": "multi", "min": 2, "max": 3, "custom": true, "options": []}"#
            ),
            "pick 2 to 3, and type your own if you like"
        );
        assert_eq!(
            hint(r#"{"id": "q", "text": "?", "kind": "text"}"#),
            "type your answer"
        );
    }
}
