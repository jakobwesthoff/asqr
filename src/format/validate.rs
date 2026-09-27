// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Validation of a parsed session (spec section 4). Every rule beyond the
//! JSON shape lives here, and every error names the field it is about, so
//! an error result tells the asker exactly what to fix. All errors are
//! collected, not just the first, so one round trip fixes a file.

use std::collections::HashSet;
use std::fmt;

use thiserror::Error;

use super::{
    Kind, Length, MAX_SESSION_ID_BYTES, Question, Session, is_valid_session_id, same_session_id,
};

/// The only format version this asqr reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// One problem in a session file, with the path of the field it is about,
/// such as `questions[2].options[0].id`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{path}: {problem}")]
pub struct ValidationError {
    pub path: FieldPath,
    pub problem: Problem,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Problem {
    #[error("unsupported format version {0}")]
    UnsupportedVersion(u32),

    #[error(
        "invalid session id {0:?} (letters, digits, '-', '_' and '.', not starting with '.', at most {MAX_SESSION_ID_BYTES} bytes)"
    )]
    InvalidSessionId(String),

    #[error("{id:?} differs from the file name {stem:?}")]
    IdDiffersFromFileName { id: String, stem: String },

    #[error("a session needs at least one question")]
    NoQuestions,

    #[error("duplicate id {0:?}")]
    DuplicateId(String),

    #[error("a {0} question needs at least one option")]
    OptionsRequired(KindName),

    #[error("a text question takes no options")]
    OptionsNotAllowed,

    #[error("a text question is answered by typing; custom is not allowed")]
    CustomNotAllowed,

    #[error("only a multi question takes min and max")]
    BoundsNotAllowed,

    #[error("only a text question takes length; a custom entry sets it inside custom")]
    LengthNotAllowed,

    #[error("{defaults} defaults, but at most {allowed} can be selected")]
    TooManyDefaults { defaults: usize, allowed: usize },

    #[error("min {min} is greater than max {max}")]
    MinAboveMax { min: u32, max: u32 },

    #[error("{bound} is more than the {options} options")]
    BoundAboveOptions { bound: u32, options: usize },

    #[error("the range {start}..{end} is reversed")]
    ReversedTarget { start: u32, end: u32 },

    #[error("{value} is below the end of the target range {target_end}")]
    BelowTarget { value: u32, target_end: u32 },
}

/// A question kind as it is written in the file, for messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindName(pub Kind);

impl fmt::Display for KindName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.0 {
            Kind::Single => "single",
            Kind::Multi => "multi",
            Kind::Text => "text",
        })
    }
}

/// The path of a field in the session file, written the way a person reads
/// it: `questions[1].options[0].id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPath(String);

impl FieldPath {
    pub(crate) fn root(field: &str) -> Self {
        FieldPath(field.to_owned())
    }

    pub(crate) fn field(&self, field: &str) -> Self {
        FieldPath(format!("{}.{field}", self.0))
    }

    pub(crate) fn index(&self, index: usize) -> Self {
        FieldPath(format!("{}[{index}]", self.0))
    }
}

impl fmt::Display for FieldPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Checks `session` against every rule of spec section 4.
///
/// `file_stem` is the name the session has in the inbox. It is `None` for a
/// file that is not in a queue yet, such as the argument of `asqr ask`,
/// whose file name does not matter because `ask` names the inbox file after
/// the session id.
pub fn validate(session: &Session, file_stem: Option<&str>) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut report =
        |path: FieldPath, problem: Problem| errors.push(ValidationError { path, problem });

    if session.asqr != FORMAT_VERSION {
        report(
            FieldPath::root("asqr"),
            Problem::UnsupportedVersion(session.asqr),
        );
    }

    if let Some(id) = &session.id {
        if !is_valid_session_id(id) {
            report(FieldPath::root("id"), Problem::InvalidSessionId(id.clone()));
        } else if let Some(stem) = file_stem
            && !same_session_id(id, stem)
        {
            report(
                FieldPath::root("id"),
                Problem::IdDiffersFromFileName {
                    id: id.clone(),
                    stem: stem.to_owned(),
                },
            );
        }
    }

    if session.questions.is_empty() {
        report(FieldPath::root("questions"), Problem::NoQuestions);
    }

    let mut question_ids = HashSet::new();
    for (index, question) in session.questions.iter().enumerate() {
        let path = FieldPath::root("questions").index(index);
        if !question_ids.insert(question.id.as_str()) {
            report(path.field("id"), Problem::DuplicateId(question.id.clone()));
        }
        validate_question(question, &path, &mut report);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_question(
    question: &Question,
    path: &FieldPath,
    report: &mut impl FnMut(FieldPath, Problem),
) {
    let options = question.options.as_deref().unwrap_or_default();

    // What the kind allows. A text question is answered by typing, so it
    // has neither options nor a separate custom entry; min and max only make
    // sense where several options can be picked.
    match question.kind {
        Kind::Single | Kind::Multi if options.is_empty() => {
            report(
                path.field("options"),
                Problem::OptionsRequired(KindName(question.kind)),
            );
        }
        Kind::Text if question.options.is_some() => {
            report(path.field("options"), Problem::OptionsNotAllowed);
        }
        _ => {}
    }
    if question.kind == Kind::Text
        && question
            .custom
            .as_ref()
            .is_some_and(|custom| custom.is_enabled())
    {
        report(path.field("custom"), Problem::CustomNotAllowed);
    }
    if question.kind != Kind::Multi {
        if question.min.is_some() {
            report(path.field("min"), Problem::BoundsNotAllowed);
        }
        if question.max.is_some() {
            report(path.field("max"), Problem::BoundsNotAllowed);
        }
    }
    if question.kind != Kind::Text && question.length.is_some() {
        report(path.field("length"), Problem::LengthNotAllowed);
    }

    let mut option_ids = HashSet::new();
    for (index, option) in options.iter().enumerate() {
        if !option_ids.insert(option.id.as_str()) {
            report(
                path.field("options").index(index).field("id"),
                Problem::DuplicateId(option.id.clone()),
            );
        }
    }

    if question.kind == Kind::Multi {
        validate_bounds(question, options.len(), path, report);
    }

    // Defaults preselect options, so there can be no more of them than the
    // question lets the person select. Defaults on a text question are
    // already reported as options that are not allowed.
    let defaults = options.iter().filter(|option| option.default).count();
    let allowed = match question.kind {
        Kind::Single => Some(1),
        Kind::Multi => question.max.map(|max| max as usize),
        Kind::Text => None,
    };
    if let Some(allowed) = allowed
        && defaults > allowed
    {
        report(
            path.field("options"),
            Problem::TooManyDefaults { defaults, allowed },
        );
    }

    if let Some(length) = &question.length {
        validate_length(length, &path.field("length"), report);
    }
    if let Some(length) = question.custom.as_ref().and_then(|custom| custom.length()) {
        validate_length(length, &path.field("custom").field("length"), report);
    }
}

fn validate_bounds(
    question: &Question,
    option_count: usize,
    path: &FieldPath,
    report: &mut impl FnMut(FieldPath, Problem),
) {
    if let (Some(min), Some(max)) = (question.min, question.max)
        && min > max
    {
        report(path.field("min"), Problem::MinAboveMax { min, max });
        return;
    }
    for (field, bound) in [("min", question.min), ("max", question.max)] {
        if let Some(bound) = bound
            && bound as usize > option_count
        {
            report(
                path.field(field),
                Problem::BoundAboveOptions {
                    bound,
                    options: option_count,
                },
            );
        }
    }
}

/// The warning threshold must not lie inside the target range. Each check
/// only applies to the parts that are present.
fn validate_length(length: &Length, path: &FieldPath, report: &mut impl FnMut(FieldPath, Problem)) {
    let target_end = match length.target {
        Some((start, end)) if start > end => {
            report(path.field("target"), Problem::ReversedTarget { start, end });
            None
        }
        Some((_, end)) => Some(end),
        None => None,
    };

    if let (Some(warn), Some(target_end)) = (length.warn, target_end)
        && warn < target_end
    {
        report(
            path.field("warn"),
            Problem::BelowTarget {
                value: warn,
                target_end,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../examples/sessions/alt-rework-batch.json");

    fn parse(json: &str) -> Session {
        serde_json::from_str(json).expect("test session parses")
    }

    /// Validates and returns the errors as `path: problem` lines, which is
    /// what an error result shows the asker.
    fn errors(json: &str) -> Vec<String> {
        errors_for_stem(json, None)
    }

    fn errors_for_stem(json: &str, stem: Option<&str>) -> Vec<String> {
        match validate(&parse(json), stem) {
            Ok(()) => Vec::new(),
            Err(errors) => errors.iter().map(ToString::to_string).collect(),
        }
    }

    /// A one-question session around `question`, so each test shows only
    /// the part it is about.
    fn with_question(question: &str) -> String {
        format!(r#"{{"asqr": 1, "questions": [{question}]}}"#)
    }

    #[test]
    fn the_example_session_is_valid() {
        assert_eq!(errors(EXAMPLE), Vec::<String>::new());
    }

    #[test]
    fn rejects_other_format_versions() {
        assert_eq!(
            errors(r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#),
            ["asqr: unsupported format version 2"]
        );
    }

    #[test]
    fn rejects_invalid_session_ids() {
        assert_eq!(
            errors(
                r#"{"asqr": 1, "id": ".hidden", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#
            ),
            [
                "id: invalid session id \".hidden\" (letters, digits, '-', '_' and '.', not starting with '.', at most 200 bytes)"
            ]
        );
    }

    #[test]
    fn the_id_must_match_the_file_stem_ignoring_case() {
        let session = r#"{"asqr": 1, "id": "Batch-01", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

        assert_eq!(
            errors_for_stem(session, Some("batch-01")),
            Vec::<String>::new()
        );
        assert_eq!(
            errors_for_stem(session, Some("batch-02")),
            ["id: \"Batch-01\" differs from the file name \"batch-02\""]
        );
    }

    #[test]
    fn needs_at_least_one_question() {
        assert_eq!(
            errors(r#"{"asqr": 1, "questions": []}"#),
            ["questions: a session needs at least one question"]
        );
    }

    #[test]
    fn rejects_duplicate_question_ids() {
        assert_eq!(
            errors(
                r#"{"asqr": 1, "questions": [
                    {"id": "q", "text": "?", "kind": "text"},
                    {"id": "q", "text": "?", "kind": "text"}]}"#
            ),
            ["questions[1].id: duplicate id \"q\""]
        );
    }

    #[test]
    fn single_and_multi_need_options() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "single"}"#
            )),
            ["questions[0].options: a single question needs at least one option"]
        );
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "multi", "options": []}"#
            )),
            ["questions[0].options: a multi question needs at least one option"]
        );
    }

    #[test]
    fn text_questions_take_no_options_and_no_custom_entry() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "text", "custom": true,
                    "options": [{"id": "a", "label": "A", "default": true}]}"#
            )),
            [
                "questions[0].options: a text question takes no options",
                "questions[0].custom: a text question is answered by typing; custom is not allowed",
            ]
        );
    }

    #[test]
    fn a_switched_off_custom_entry_is_fine_on_text_questions() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "text", "custom": false}"#
            )),
            Vec::<String>::new()
        );
    }

    #[test]
    fn rejects_duplicate_option_ids() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "single",
                    "options": [{"id": "a", "label": "A"}, {"id": "a", "label": "B"}]}"#
            )),
            ["questions[0].options[1].id: duplicate id \"a\""]
        );
    }

    #[test]
    fn a_single_question_has_at_most_one_default() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "single", "options": [
                    {"id": "a", "label": "A", "default": true},
                    {"id": "b", "label": "B", "default": true}]}"#
            )),
            ["questions[0].options: 2 defaults, but at most 1 can be selected"]
        );
    }

    #[test]
    fn a_multi_question_has_no_more_defaults_than_max() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "multi", "max": 1, "options": [
                    {"id": "a", "label": "A", "default": true},
                    {"id": "b", "label": "B", "default": true}]}"#
            )),
            ["questions[0].options: 2 defaults, but at most 1 can be selected"]
        );
    }

    #[test]
    fn min_and_max_must_fit_the_options() {
        let options = r#"[{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]"#;

        assert_eq!(
            errors(&with_question(&format!(
                r#"{{"id": "q", "text": "?", "kind": "multi", "min": 2, "max": 1, "options": {options}}}"#
            ))),
            ["questions[0].min: min 2 is greater than max 1"]
        );
        assert_eq!(
            errors(&with_question(&format!(
                r#"{{"id": "q", "text": "?", "kind": "multi", "min": 3, "max": 3, "options": {options}}}"#
            ))),
            [
                "questions[0].min: 3 is more than the 2 options",
                "questions[0].max: 3 is more than the 2 options",
            ]
        );
    }

    #[test]
    fn min_and_max_belong_to_multi_questions_only() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "single", "min": 1,
                    "options": [{"id": "a", "label": "A"}]}"#
            )),
            ["questions[0].min: only a multi question takes min and max"]
        );
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "text", "max": 1}"#
            )),
            ["questions[0].max: only a multi question takes min and max"]
        );
    }

    #[test]
    fn length_belongs_to_text_questions_and_custom_entries() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "single", "length": {"max": 5},
                    "options": [{"id": "a", "label": "A"}]}"#
            )),
            [
                "questions[0].length: only a text question takes length; a custom entry sets it inside custom"
            ]
        );
    }

    #[test]
    fn length_bounds_must_be_consistent() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "text", "length": {"target": [9, 8]}}"#
            )),
            ["questions[0].length.target: the range 9..8 is reversed"]
        );
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "text", "length": {"target": [1, 50], "warn": 40}}"#
            )),
            ["questions[0].length.warn: 40 is below the end of the target range 50"]
        );
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "text", "length": {"target": [1, 50], "warn": 50}}"#
            )),
            Vec::<String>::new(),
            "warn may equal the end of the target"
        );
    }

    #[test]
    fn checks_the_length_of_a_custom_entry_too() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "single", "options": [{"id": "a", "label": "A"}],
                    "custom": {"length": {"target": [9, 8]}}}"#
            )),
            ["questions[0].custom.length.target: the range 9..8 is reversed"]
        );
    }

    #[test]
    fn names_kinds_the_way_the_file_writes_them() {
        assert_eq!(KindName(Kind::Single).to_string(), "single");
        assert_eq!(KindName(Kind::Multi).to_string(), "multi");
        assert_eq!(KindName(Kind::Text).to_string(), "text");
    }

    #[test]
    fn reports_every_error_at_once() {
        let errors = errors(r#"{"asqr": 3, "id": "bad id", "questions": []}"#);

        assert_eq!(errors.len(), 3);
    }
}
