// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Validation of a parsed session (spec section 3.4). Every rule beyond the
//! JSON shape lives here, and every error names the field it is about, so
//! an error result tells the asker exactly what to fix. All errors are
//! collected, not just the first, so one round trip fixes a file.

use std::collections::HashSet;
use std::fmt;

use thiserror::Error;

use super::{
    CONTEXT_ENTRY_TYPES, ContextEntry, DiffSide, Kind, Length, MAX_SESSION_ID_BYTES, Question,
    Session, is_valid_session_id, same_session_id,
};

/// The only format version this asqr reads and writes.
pub const FORMAT_VERSION: u32 = 2;

/// The context entries one question or option may carry. The format
/// allows a list, so showing several entries later needs no new format
/// version (spec section 3.1).
pub const MAX_CONTEXT_ENTRIES: usize = 1;

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
    /// Carries the version as the file writes it, which need not be a
    /// number, such as `"2"` or `2.0`.
    #[error("unsupported format version {0}")]
    UnsupportedVersion(String),

    /// A version that an earlier asqr read. The skill compiled into this
    /// asqr describes the current format, so an agent learns there what to
    /// write instead.
    #[error(
        "this asqr no longer supports format version {0}; `asqr skill` describes the current format (version {FORMAT_VERSION})"
    )]
    DroppedVersion(u32),

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

    #[error("{0} entries, but this asqr shows one per question or option")]
    TooManyContextEntries(usize),

    #[error("needs path or content")]
    SourceMissing,

    #[error("takes path or content, not both")]
    SourceTwice,

    /// A context entry of `type: "text"`, a natural guess for an agent
    /// that wants to show text. The parser's own message would only list
    /// the types; this one says which to pick.
    #[error(
        r#""text" is no entry type; use "code" for text shown as written (source, a log, a Markdown file's source) or "markdown" for Markdown to render, with "path" or "content""#
    )]
    TextEntryType,

    /// A context entry of a type this asqr does not know. New types are
    /// added within format version 2 (ADR 51), so the type may be one a
    /// newer asqr shows.
    #[error(
        "{0:?} is not supported by this asqr (version {version}), which supports {types}; a newer asqr may support it",
        version = env!("CARGO_PKG_VERSION"),
        types = EntryTypeList
    )]
    UnknownEntryType(String),

    /// A diff without sides, or with `path` or `content` on the entry
    /// itself. Both come from an agent that has not learned that asqr
    /// diffs two texts, often one holding a unified diff, so the message
    /// says what a diff takes.
    #[error(
        r#"a diff takes `old` and/or `new`, each {{"path": ...}} or {{"content": ...}}: the text before and after the change; asqr computes the diff itself and does not read a unified diff"#
    )]
    DiffSides,
}

/// The entry types this asqr knows, written as a message lists them:
/// `"image", "code", "markdown" and "diff"`.
struct EntryTypeList;

impl fmt::Display for EntryTypeList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let last = CONTEXT_ENTRY_TYPES.len() - 1;
        for (index, name) in CONTEXT_ENTRY_TYPES.iter().enumerate() {
            let separator = match index {
                0 => "",
                _ if index == last => " and ",
                _ => ", ",
            };
            write!(formatter, "{separator}{name:?}")?;
        }
        Ok(())
    }
}

/// The problem with format version `version`, if it is not the one this
/// asqr reads.
pub(crate) fn version_problem(version: u64) -> Option<Problem> {
    let current = u64::from(FORMAT_VERSION);
    if version == current {
        None
    } else if (1..current).contains(&version) {
        let dropped = u32::try_from(version).expect("below the current version, so it fits");
        Some(Problem::DroppedVersion(dropped))
    } else {
        Some(Problem::UnsupportedVersion(version.to_string()))
    }
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

/// Checks `session` against every rule of spec section 3.4.
///
/// `file_stem` is the name the session has in the inbox. It is `None` for a
/// file that is not in a queue yet, such as the argument of `asqr ask`,
/// whose file name does not matter because `ask` names the inbox file after
/// the session id.
pub fn validate(session: &Session, file_stem: Option<&str>) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut report =
        |path: FieldPath, problem: Problem| errors.push(ValidationError { path, problem });

    if let Some(problem) = version_problem(u64::from(session.asqr)) {
        report(FieldPath::root("asqr"), problem);
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

    validate_context(&question.context, &path.field("context"), report);

    let mut option_ids = HashSet::new();
    for (index, option) in options.iter().enumerate() {
        let option_path = path.field("options").index(index);
        if !option_ids.insert(option.id.as_str()) {
            report(
                option_path.field("id"),
                Problem::DuplicateId(option.id.clone()),
            );
        }
        validate_context(&option.context, &option_path.field("context"), report);
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

fn validate_context(
    context: &[ContextEntry],
    path: &FieldPath,
    report: &mut impl FnMut(FieldPath, Problem),
) {
    if context.len() > MAX_CONTEXT_ENTRIES {
        report(path.clone(), Problem::TooManyContextEntries(context.len()));
    }
    for (index, entry) in context.iter().enumerate() {
        let path = path.index(index);
        match entry {
            ContextEntry::Image(_) => {}
            ContextEntry::Code(code) => {
                validate_source(code.path.is_some(), code.content.is_some(), &path, report);
            }
            ContextEntry::Markdown(markdown) => {
                validate_source(
                    markdown.path.is_some(),
                    markdown.content.is_some(),
                    &path,
                    report,
                );
            }
            ContextEntry::Diff(diff) => {
                // One missing side is a new or a deleted file; with both
                // missing there is nothing to compare.
                if (diff.old.is_none() && diff.new.is_none()) || diff.misplaced_source() {
                    report(path.clone(), Problem::DiffSides);
                }
                let sides = [("old", &diff.old), ("new", &diff.new)];
                for (field, side) in sides {
                    if let Some(DiffSide {
                        path: file,
                        content,
                    }) = side
                    {
                        validate_source(
                            file.is_some(),
                            content.is_some(),
                            &path.field(field),
                            report,
                        );
                    }
                }
            }
        }
    }
}

/// Text comes from a file or is written inline, and exactly one of the two
/// says where.
fn validate_source(
    has_path: bool,
    has_content: bool,
    path: &FieldPath,
    report: &mut impl FnMut(FieldPath, Problem),
) {
    match (has_path, has_content) {
        (false, false) => report(path.clone(), Problem::SourceMissing),
        (true, true) => report(path.clone(), Problem::SourceTwice),
        _ => {}
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
        format!(r#"{{"asqr": 2, "questions": [{question}]}}"#)
    }

    #[test]
    fn the_example_session_is_valid() {
        assert_eq!(errors(EXAMPLE), Vec::<String>::new());
    }

    #[test]
    fn rejects_other_format_versions() {
        assert_eq!(
            errors(r#"{"asqr": 3, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#),
            ["asqr: unsupported format version 3"]
        );
    }

    #[test]
    fn rejects_invalid_session_ids() {
        assert_eq!(
            errors(
                r#"{"asqr": 2, "id": ".hidden", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#
            ),
            [
                "id: invalid session id \".hidden\" (letters, digits, '-', '_' and '.', not starting with '.', at most 200 bytes)"
            ]
        );
    }

    #[test]
    fn the_id_must_match_the_file_stem_ignoring_case() {
        let session = r#"{"asqr": 2, "id": "Batch-01", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

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
            errors(r#"{"asqr": 2, "questions": []}"#),
            ["questions: a session needs at least one question"]
        );
    }

    #[test]
    fn rejects_duplicate_question_ids() {
        assert_eq!(
            errors(
                r#"{"asqr": 2, "questions": [
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
    fn refuses_older_format_versions_with_a_pointer_to_the_skill() {
        assert_eq!(
            errors(r#"{"asqr": 1, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#),
            [
                "asqr: this asqr no longer supports format version 1; `asqr skill` describes the current format (version 2)"
            ]
        );
        // Version 0 never existed, so there is nothing this asqr dropped.
        assert_eq!(
            errors(r#"{"asqr": 0, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#),
            ["asqr: unsupported format version 0"]
        );
    }

    #[test]
    fn every_kind_of_context_entry_is_valid_on_questions_and_options() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "single",
                    "context": [{"type": "code", "path": "/notes.md", "language": "markdown"}],
                    "options": [
                        {"id": "a", "label": "A", "context": [{"type": "image", "title": "A", "path": "/a.png"}]},
                        {"id": "b", "label": "B", "context": [{"type": "diff", "name": "b.rs",
                            "old": {"path": "/old.rs"}, "new": {"content": "fn b() {}"}}]},
                        {"id": "c", "label": "C", "context": [{"type": "diff", "new": {"content": "new file"}}]},
                        {"id": "d", "label": "D", "context": [{"type": "code", "content": "inline"}]},
                        {"id": "e", "label": "E", "context": [{"type": "markdown", "content": "*inline*"}]},
                        {"id": "f", "label": "F", "context": [{"type": "markdown", "path": "/plan.md"}]},
                        {"id": "g", "label": "G", "context": []}]}"#
            )),
            Vec::<String>::new()
        );
    }

    #[test]
    fn this_asqr_shows_one_context_entry_per_question_or_option() {
        let two = r#"[{"type": "image", "path": "/a.png"}, {"type": "image", "path": "/b.png"}]"#;

        assert_eq!(
            errors(&with_question(&format!(
                r#"{{"id": "q", "text": "?", "kind": "single", "context": {two},
                    "options": [{{"id": "a", "label": "A", "context": {two}}}]}}"#
            ))),
            [
                "questions[0].context: 2 entries, but this asqr shows one per question or option",
                "questions[0].options[0].context: 2 entries, but this asqr shows one per question or option",
            ]
        );
    }

    #[test]
    fn code_and_markdown_entries_name_a_file_or_hold_the_content() {
        for kind in ["code", "markdown"] {
            assert_eq!(
                errors(&with_question(&format!(
                    r#"{{"id": "q", "text": "?", "kind": "text", "context": [{{"type": "{kind}", "title": "Log"}}]}}"#
                ))),
                ["questions[0].context[0]: needs path or content"],
                "{kind}"
            );
            assert_eq!(
                errors(&with_question(&format!(
                    r#"{{"id": "q", "text": "?", "kind": "text",
                        "context": [{{"type": "{kind}", "path": "/log.txt", "content": "ok"}}]}}"#
                ))),
                ["questions[0].context[0]: takes path or content, not both"],
                "{kind}"
            );
        }
    }

    const DIFF_SIDES: &str = r#"a diff takes `old` and/or `new`, each {"path": ...} or {"content": ...}: the text before and after the change; asqr computes the diff itself and does not read a unified diff"#;

    #[test]
    fn a_diff_has_at_least_one_side_and_each_side_one_source() {
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "text", "context": [{"type": "diff", "name": "a.rs"}]}"#
            )),
            [format!("questions[0].context[0]: {DIFF_SIDES}")]
        );
        assert_eq!(
            errors(&with_question(
                r#"{"id": "q", "text": "?", "kind": "text",
                    "context": [{"type": "diff", "old": {}, "new": {"path": "/a.rs", "content": "x"}}]}"#
            )),
            [
                "questions[0].context[0].old: needs path or content",
                "questions[0].context[0].new: takes path or content, not both",
            ]
        );
    }

    #[test]
    fn a_path_or_content_on_the_diff_itself_says_where_the_texts_go() {
        // Once per entry, whether or not it also has sides, and whatever
        // the misplaced key holds: an agent with a unified diff at hand
        // writes it there.
        for entry in [
            r#"{"type": "diff", "content": "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n"}"#,
            r#"{"type": "diff", "path": "/change.diff"}"#,
            r#"{"type": "diff", "path": "/change.diff", "content": ""}"#,
            r#"{"type": "diff", "content": "b", "old": {"content": "a"}}"#,
        ] {
            assert_eq!(
                errors(&with_question(&format!(
                    r#"{{"id": "q", "text": "?", "kind": "text", "context": [{entry}]}}"#
                ))),
                [format!("questions[0].context[0]: {DIFF_SIDES}")],
                "{entry}"
            );
        }
    }

    #[test]
    fn reports_every_error_at_once() {
        let errors = errors(r#"{"asqr": 3, "id": "bad id", "questions": []}"#);

        assert_eq!(errors.len(), 3);
    }
}
