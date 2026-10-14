// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The session file (spec section 4): what an asker writes into the inbox.
//!
//! The types only describe the shape. Parsing is deliberately lenient:
//! unknown fields are ignored so later format versions can add optional
//! fields, and every rule beyond the shape (ids, option counts, bounds)
//! lives in validation, which can name the offending field.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::{
    ContextEntry, FORMAT_VERSION, FieldPath, MAX_CONTEXT_ENTRIES, Problem, ValidationError,
    version_problem,
};

/// Why a file could not be read as a session.
#[derive(Debug, Error)]
pub enum ParseError {
    /// The file names a format version other than this asqr's, or has
    /// entries whose `type` the parser would only report as unknown. Each
    /// error says what to write instead.
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Refused(Vec<ValidationError>),

    /// The file is no JSON at all.
    #[error("not a session file: {0}")]
    NotASession(serde_json::Error),

    /// The JSON is not shaped like a session.
    #[error("not a session file: {0}")]
    Shape(ShapeError),
}

/// A field whose value has the wrong type, or a required field that is
/// missing, with the path to it. A file with several questions needs the
/// path, as the parser's own message names no question.
#[derive(Debug)]
pub struct ShapeError {
    path: serde_path_to_error::Path,
    error: serde_json::Error,
}

impl ShapeError {
    /// Whether the error is about a `context` field itself rather than one
    /// of its entries. A `null` context parses, so this is a value of
    /// another type, most likely a single entry written without its list.
    fn context_is_no_list(&self) -> bool {
        matches!(
            self.path.iter().next_back(),
            Some(serde_path_to_error::Segment::Map { key }) if key == "context"
        )
    }
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // An error of the whole file, such as a missing `questions`, has an
        // empty path, which the crate writes as `.`.
        if self.path.iter().next().is_some() {
            write!(formatter, "{}: ", self.path)?;
        }
        let hint = if self.context_is_no_list() {
            "; context is a list with one entry: wrap the entry in [ ]"
        } else {
            ""
        };
        write!(formatter, "{}{hint}", self.error)
    }
}

/// Reads a session from `bytes`, returning the raw JSON too, which keeps
/// the unknown fields the typed session drops.
///
/// Some problems are checked on the raw JSON before the shape, because
/// the parser's message would name the wrong cause. A file of another
/// format version has the rules of that version, so its shape errors would
/// only mislead. An entry of `type: "text"`, a natural guess for an agent
/// that wants to show text, would only be an unknown variant, while the
/// error here says which of the two text types to pick. A version that is
/// no unsigned integer, such as `2.0` or `"2"`, would only be a type error
/// of the parser.
pub fn parse_session(bytes: &[u8]) -> Result<(Value, Session), ParseError> {
    let raw: Value = serde_json::from_slice(bytes).map_err(ParseError::NotASession)?;
    // A missing version is left to the parser, which names the missing
    // field.
    let version = raw.get("asqr").and_then(|version| match version.as_u64() {
        Some(number) => version_problem(number),
        None => Some(Problem::UnsupportedVersion(version.to_string())),
    });
    if let Some(problem) = version {
        return Err(ParseError::Refused(vec![ValidationError {
            path: FieldPath::root("asqr"),
            problem,
        }]));
    }
    let text_entries = text_entries(&raw);
    if !text_entries.is_empty() {
        return Err(ParseError::Refused(text_entries));
    }
    let session = serde_path_to_error::deserialize(raw.clone()).map_err(|error| {
        ParseError::Shape(ShapeError {
            path: error.path().clone(),
            error: error.into_inner(),
        })
    })?;
    Ok((raw, session))
}

/// An error for every context entry of `type: "text"`, on questions and
/// options alike.
fn text_entries(raw: &Value) -> Vec<ValidationError> {
    let mut found = Vec::new();
    let mut check = |holder: &Value, path: FieldPath| {
        let entries = holder["context"].as_array().into_iter().flatten();
        for (index, entry) in entries.enumerate() {
            if entry["type"] == "text" {
                found.push(ValidationError {
                    path: path.field("context").index(index).field("type"),
                    problem: Problem::TextEntryType,
                });
            }
        }
    };
    let questions = raw["questions"].as_array().into_iter().flatten();
    for (index, question) in questions.enumerate() {
        let path = FieldPath::root("questions").index(index);
        check(question, path.clone());
        let options = question["options"].as_array().into_iter().flatten();
        for (index, option) in options.enumerate() {
            check(option, path.field("options").index(index));
        }
    }
    found
}

/// One session: a list of questions answered and submitted as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// The format version. Only `2` is supported.
    #[schemars(extend("const" = FORMAT_VERSION))]
    // Any `u32` parses; validation rejects the others, so the error names
    // the field instead of failing in the parser. `parse_session` refuses
    // the values that are no `u32` before the parser sees them.
    pub asqr: u32,

    /// The session id. Optional, because `asqr ask` assigns a ULID when it
    /// is missing and a hand-dropped file takes its file stem as the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Markdown, shown dimmed under the title in at most three lines. A
    /// single newline breaks the line, and raw HTML shows as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro: Option<String>,

    /// Who is asking, shown in the session header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,

    /// The id of an earlier session this one continues (spec section 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<String>,

    #[schemars(length(min = 1))]
    pub questions: Vec<Question>,
}

/// One thing for the person to decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Question {
    pub id: String,

    /// A short label for the question list; the id is shown without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,

    /// The question itself. Markdown: a single newline breaks the line,
    /// and raw HTML shows as written.
    pub text: String,

    pub kind: Kind,

    /// The choices of a `single` or `multi` question.
    // An `Option`, so validation can tell a missing list from an empty one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<Choice>>,

    /// Whether a typed answer is allowed next to the options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<Custom>,

    /// Length limits of the answer of a `text` question. A custom entry
    /// carries its own limits inside `custom`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Length>,

    /// Material shown next to the question that helps to answer it. Most
    /// questions have none. A list, of which this asqr shows one entry.
    #[serde(
        default,
        deserialize_with = "null_as_no_context",
        skip_serializing_if = "Vec::is_empty"
    )]
    #[schemars(with = "Option<Vec<ContextEntry>>", length(max = MAX_CONTEXT_ENTRIES))]
    pub context: Vec<ContextEntry>,

    /// Whether the person may add a note to this question.
    #[serde(default = "note_allowed_by_default")]
    pub note: bool,

    /// Bounds on the number of selected options of a `multi` question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

fn note_allowed_by_default() -> bool {
    true
}

/// Reads a `null` context as an empty list. A `null` counts as a missing
/// field everywhere in the format (spec section 4), and the other optional
/// fields get that from their `Option`.
fn null_as_no_context<'de, D>(deserializer: D) -> Result<Vec<ContextEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<ContextEntry>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Pick exactly one option.
    Single,
    /// Pick several options.
    Multi,
    /// A typed answer, without options.
    Text,
}

/// One option of a `single` or `multi` question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Choice {
    pub id: String,

    pub label: String,

    /// Shown in full and wrapped, however long it is. Markdown: a single
    /// newline breaks the line, and raw HTML shows as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Preselects the option.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,

    /// Shown in place of the question's context while the cursor is on
    /// this option, so the person can compare options before picking.
    #[serde(
        default,
        deserialize_with = "null_as_no_context",
        skip_serializing_if = "Vec::is_empty"
    )]
    #[schemars(with = "Option<Vec<ContextEntry>>", length(max = MAX_CONTEXT_ENTRIES))]
    pub context: Vec<ContextEntry>,
}

/// The `custom` field: either a plain switch or a configured entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Custom {
    Enabled(bool),
    Configured(CustomConfig),
}

impl Custom {
    /// Whether the question offers a typed answer at all; `false` in the
    /// file switches it off like an absent field.
    pub fn is_enabled(&self) -> bool {
        match self {
            Custom::Enabled(enabled) => *enabled,
            Custom::Configured(_) => true,
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Custom::Enabled(_) => None,
            Custom::Configured(config) => config.label.as_deref(),
        }
    }

    pub fn length(&self) -> Option<&Length> {
        match self {
            Custom::Enabled(_) => None,
            Custom::Configured(config) => config.length.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CustomConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Length>,
}

/// How long typed text should be (spec section 7.3). Both parts are
/// optional; validation checks that they are consistent. There is no hard
/// limit: the counter guides, and input is never refused (ADR 22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Length {
    /// The range the text should land in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<(u32, u32)>,

    /// Above this the counter warns more strongly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../examples/sessions/alt-rework-batch.json");

    #[test]
    fn parses_the_example_session() {
        let session: Session = serde_json::from_str(EXAMPLE).expect("example parses");

        assert_eq!(session.asqr, 2);
        assert_eq!(session.id.as_deref(), Some("alt-rework-3-batch-01"));
        assert_eq!(session.follows.as_deref(), Some("alt-rework-3-batch-00"));
        assert_eq!(session.questions.len(), 3);

        let single = &session.questions[0];
        assert_eq!(single.kind, Kind::Single);
        assert_eq!(single.header.as_deref(), Some("301"));
        let options = single.options.as_ref().expect("single has options");
        assert_eq!(options.len(), 4);
        assert_eq!(options[1].id, "new1");
        assert_eq!(
            single.context[0].image_path(),
            Some("/tmp/asqr-example/301-holly-crown-green-robe-feast-ghost.png")
        );
        let custom = single.custom.as_ref().expect("single allows custom");
        assert_eq!(custom.label(), Some("Own line"));
        let length = custom.length().expect("custom has a length");
        assert_eq!(length.target, Some((80, 125)));
        assert_eq!(length.warn, Some(145));

        let multi = &session.questions[1];
        assert_eq!(multi.kind, Kind::Multi);
        assert_eq!((multi.min, multi.max), (Some(1), Some(2)));
        assert!(multi.options.as_ref().expect("multi has options")[0].default);
        assert_eq!(multi.custom, Some(Custom::Enabled(true)));

        let text = &session.questions[2];
        assert_eq!(text.kind, Kind::Text);
        assert!(text.options.is_none());
        assert_eq!(text.length.as_ref().and_then(|l| l.warn), Some(500));
        assert!(!text.note);
    }

    #[test]
    fn optional_fields_take_their_defaults() {
        let session: Session = serde_json::from_str(
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
        )
        .expect("minimal session parses");

        assert_eq!(session.id, None);
        let question = &session.questions[0];
        // Notes are allowed unless the asker switches them off (spec
        // section 4).
        assert!(question.note);
        assert_eq!(question.custom, None);
        assert!(question.context.is_empty());
    }

    #[test]
    fn an_option_may_carry_context_and_writes_it_back() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "options": [{"id": "a", "label": "A", "context": [{"type": "image", "path": "/pictures/a.png"}]},
                        {"id": "b", "label": "B"}]}]}"#;

        let session: Session = serde_json::from_str(json).expect("parses");

        let options = session.questions[0].options.as_ref().expect("options");
        assert_eq!(options[0].context[0].image_path(), Some("/pictures/a.png"));
        assert!(options[1].context.is_empty());
        let written = serde_json::to_value(&session).expect("serializes");
        assert_eq!(
            written["questions"][0]["options"][0]["context"],
            serde_json::json!([{"type": "image", "path": "/pictures/a.png"}])
        );
        assert!(
            written["questions"][0]["options"][1]
                .get("context")
                .is_none()
        );
    }

    #[test]
    fn unknown_fields_are_ignored_when_parsing() {
        // Later format versions may add optional fields, so version 2
        // readers must not reject them (spec section 4); warnings about them
        // come from validation.
        let session: Session = serde_json::from_str(
            r#"{"asqr": 2, "future": 1,
                "questions": [{"id": "q", "text": "?", "kind": "text", "requred": true}]}"#,
        )
        .expect("unknown fields do not fail parsing");

        assert_eq!(session.questions[0].id, "q");
    }

    #[test]
    fn parsing_keeps_the_raw_json_next_to_the_session() {
        let (raw, session) = parse_session(
            br#"{"asqr": 2, "later": 1, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
        )
        .expect("parses");

        assert_eq!(raw["later"], 1);
        assert_eq!(session.questions[0].id, "q");
    }

    #[test]
    fn another_format_version_is_reported_before_the_shape() {
        // A version 1 file whose shape no longer parses still learns that
        // its version is the problem.
        let error = parse_session(br#"{"asqr": 1, "questions": [{"id": "q", "image": 3}]}"#)
            .expect_err("version 1 is refused");

        assert_eq!(
            error.to_string(),
            "asqr: this asqr no longer supports format version 1; `asqr skill` describes the current format (version 2)"
        );
        assert_eq!(
            parse_session(br#"{"asqr": 3}"#)
                .expect_err("version 3 is refused")
                .to_string(),
            "asqr: unsupported format version 3"
        );
    }

    #[test]
    fn a_version_that_is_no_unsigned_integer_is_refused_as_written() {
        // The parser would only say that it expected a `u32`, which names
        // neither the field's meaning nor the version asqr reads.
        for (written, shown) in [
            ("2.0", "2.0"),
            (r#""2""#, r#""2""#),
            ("-1", "-1"),
            ("true", "true"),
            ("null", "null"),
            // Beyond `u64` the JSON parser keeps the number as a float.
            ("18446744073709551616", "1.8446744073709552e+19"),
        ] {
            let file = format!(r#"{{"asqr": {written}, "questions": [{{"id": "q"}}]}}"#);

            assert_eq!(
                parse_session(file.as_bytes())
                    .expect_err("the version is refused")
                    .to_string(),
                format!("asqr: unsupported format version {shown}"),
                "{written}"
            );
        }
    }

    #[test]
    fn a_version_too_large_for_the_format_is_named_in_full() {
        assert_eq!(
            parse_session(br#"{"asqr": 4294967296}"#)
                .expect_err("the version is refused")
                .to_string(),
            "asqr: unsupported format version 4294967296"
        );
    }

    #[test]
    fn a_text_entry_is_refused_with_the_types_that_replace_it() {
        let error = parse_session(
            br#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
                "context": [{"type": "text", "text": "notes"}],
                "options": [{"id": "a", "label": "A", "context": [{"type": "code", "content": "x"}]},
                            {"id": "b", "label": "B", "context": [{"type": "text", "path": "/b.md"}]}]}]}"#,
        )
        .expect_err("text entries are refused");

        let replaced = r#""text" is no entry type; use "code" for text shown as written (source, a log, a Markdown file's source) or "markdown" for Markdown to render, with "path" or "content""#;
        assert_eq!(
            error.to_string(),
            format!(
                "questions[0].context[0].type: {replaced}; questions[0].options[1].context[0].type: {replaced}"
            )
        );
    }

    #[test]
    fn a_file_that_is_no_session_says_so() {
        for bytes in [
            &b"[1, 2"[..],
            br#"{"asqr": 2, "questions": [{"id": "q"}]}"#,
            b"{}",
        ] {
            let error = parse_session(bytes).expect_err("not a session");

            assert!(
                error.to_string().starts_with("not a session file: "),
                "{error}"
            );
        }
    }

    fn shape_error(file: &str) -> String {
        parse_session(file.as_bytes())
            .expect_err("the shape is refused")
            .to_string()
    }

    #[test]
    fn a_shape_error_names_the_field_it_is_about() {
        // Without the path an agent learns that some list is a map, but
        // not which of its questions or fields holds it.
        assert_eq!(
            shape_error(
                r#"{"asqr": 2, "questions": [{"id": "a", "text": "?", "kind": "text"},
                    {"id": "b", "text": "?", "kind": "multi", "min": "one"}]}"#
            ),
            r#"not a session file: questions[1].min: invalid type: string "one", expected u32"#
        );
        assert_eq!(
            shape_error(
                r#"{"asqr": 2, "questions": [{"id": "a", "text": "?", "kind": "text"},
                    {"id": "b", "text": "?"}]}"#
            ),
            "not a session file: questions[1]: missing field `kind`"
        );
        assert_eq!(
            shape_error(
                r#"{"asqr": 2, "questions": [{"id": "a", "text": "?", "kind": "text",
                    "context": [{"type": "image", "path": null}]}]}"#
            ),
            "not a session file: questions[0].context[0]: invalid type: null, expected a string"
        );
    }

    #[test]
    fn a_shape_error_of_the_whole_file_names_no_path() {
        assert_eq!(
            shape_error(r#"{"asqr": 2}"#),
            "not a session file: missing field `questions`"
        );
    }

    #[test]
    fn a_context_that_is_no_list_says_how_to_make_it_one() {
        // A single entry written without its list is the likely slip, as a
        // question holds one entry.
        let hint = "context is a list with one entry: wrap the entry in [ ]";
        assert_eq!(
            shape_error(
                r#"{"asqr": 2, "questions": [{"id": "a", "text": "?", "kind": "text"},
                    {"id": "b", "text": "?", "kind": "text",
                     "context": {"type": "code", "content": "x"}}]}"#
            ),
            format!(
                "not a session file: questions[1].context: invalid type: map, expected a sequence; {hint}"
            )
        );
        assert_eq!(
            shape_error(
                r#"{"asqr": 2, "questions": [{"id": "a", "text": "?", "kind": "single",
                    "options": [{"id": "x", "label": "X", "context": "notes.md"}]}]}"#
            ),
            format!(
                r#"not a session file: questions[0].options[0].context: invalid type: string "notes.md", expected a sequence; {hint}"#
            )
        );
    }

    #[test]
    fn a_null_context_is_no_context() {
        // Spec section 4: a `null` counts as a missing field, as it does
        // for every other optional field.
        let (_, session) = parse_session(
            br#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
                "context": null,
                "options": [{"id": "a", "label": "A", "context": null}]}]}"#,
        )
        .expect("a null context parses");

        let question = &session.questions[0];
        assert!(question.context.is_empty());
        assert!(
            question.options.as_ref().expect("options")[0]
                .context
                .is_empty()
        );
    }

    #[test]
    fn custom_switch_and_configuration_answer_the_same_questions() {
        let switched_on: Custom = serde_json::from_str("true").expect("bool parses");
        let switched_off: Custom = serde_json::from_str("false").expect("bool parses");
        let configured: Custom = serde_json::from_str(r#"{"label": "Own", "length": {"warn": 9}}"#)
            .expect("object parses");

        assert!(switched_on.is_enabled());
        assert!(!switched_off.is_enabled());
        assert!(configured.is_enabled());

        assert_eq!(switched_on.label(), None);
        assert_eq!(switched_on.length(), None);

        assert_eq!(configured.label(), Some("Own"));
        assert_eq!(configured.length().and_then(|l| l.warn), Some(9));
    }

    #[test]
    fn round_trips_through_json() {
        let session: Session = serde_json::from_str(EXAMPLE).expect("example parses");
        let written = serde_json::to_string(&session).expect("session serializes");
        let reread: Session = serde_json::from_str(&written).expect("written session parses");

        assert_eq!(reread, session);
    }
}
