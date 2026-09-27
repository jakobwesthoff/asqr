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

/// One session: a list of questions answered and submitted as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// The format version. Only `1` is supported.
    // Any number parses; validation rejects the others, so the error names
    // the field instead of failing in the parser.
    pub asqr: u32,

    /// The session id. Optional, because `asqr ask` assigns a ULID when it
    /// is missing and a hand-dropped file takes its file stem as the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro: Option<String>,

    /// Who is asking, shown in the session header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,

    /// The id of an earlier session this one continues (spec section 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<String>,

    pub questions: Vec<Question>,
}

/// One thing for the person to decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Question {
    pub id: String,

    /// A short label for the question list; the id is shown without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,

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

    /// An image shown next to the question. Most questions have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Whether the person may add a note to this question.
    #[serde(default = "note_allowed_by_default")]
    pub note: bool,

    #[serde(default)]
    pub required: bool,

    /// Bounds on the number of selected options of a `multi` question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

fn note_allowed_by_default() -> bool {
    true
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

    /// Shown in full and wrapped, however long it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Preselects the option.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
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

        assert_eq!(session.asqr, 1);
        assert_eq!(session.id.as_deref(), Some("alt-rework-3-batch-01"));
        assert_eq!(session.follows.as_deref(), Some("alt-rework-3-batch-00"));
        assert_eq!(session.questions.len(), 3);

        let single = &session.questions[0];
        assert_eq!(single.kind, Kind::Single);
        assert_eq!(single.header.as_deref(), Some("301"));
        let options = single.options.as_ref().expect("single has options");
        assert_eq!(options.len(), 4);
        assert_eq!(options[1].id, "new1");
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
            r#"{"asqr": 1, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
        )
        .expect("minimal session parses");

        assert_eq!(session.id, None);
        let question = &session.questions[0];
        // Notes are allowed unless the asker switches them off (spec
        // section 4), and nothing is required unless asked for.
        assert!(question.note);
        assert!(!question.required);
        assert_eq!(question.custom, None);
        assert_eq!(question.image, None);
    }

    #[test]
    fn unknown_fields_are_ignored_when_parsing() {
        // Later format versions may add optional fields, so version 1
        // readers must not reject them (spec section 4); warnings about them
        // come from validation.
        let session: Session = serde_json::from_str(
            r#"{"asqr": 1, "future": 1,
                "questions": [{"id": "q", "text": "?", "kind": "text", "requred": true}]}"#,
        )
        .expect("unknown fields do not fail parsing");

        assert!(!session.questions[0].required);
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
