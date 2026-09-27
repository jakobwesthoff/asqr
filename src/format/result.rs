// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Result and draft files (spec sections 5.1 and 5.3). A draft is a result
//! in progress: the same shape with the status `draft`, so restoring and
//! finishing work on one type.

use serde::{Deserialize, Serialize};

use super::FORMAT_VERSION;

/// The file asqr writes into the outbox when a session is finished, or into
/// `drafts/` while it is being answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResult {
    pub asqr: u32,

    pub id: String,

    pub status: Status,

    /// When the session was finished, as RFC 3339 with the offset. Absent in
    /// drafts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,

    /// The SHA-256 of the session file's bytes as read from the inbox. It
    /// ties the result to one version of a session that may have been
    /// replaced, and lets recovery tell an interrupted finish from a new
    /// session under the same id (spec section 3.7). Absent in drafts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_sha256: Option<String>,

    /// Why the person rejected the session, if they said so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// What is wrong with an invalid session file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// The question the cursor was on, so a restored draft continues there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,

    #[serde(default)]
    pub answers: Vec<Answer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The person submitted, possibly with skipped questions.
    Submitted,
    /// The person rejected the whole session.
    Cancelled,
    /// The session file was invalid.
    Error,
    /// Answers in progress; never found in the outbox.
    Draft,
}

/// The answer to one question. Which fields appear depends on the question
/// kind and the answer state (spec section 5.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub question: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,

    /// The selection is the question's default and the person never edited
    /// the question.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub defaulted: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
}

impl Answer {
    /// An empty answer to `question`, to fill in with struct update syntax.
    pub fn new(question: impl Into<String>) -> Self {
        Answer {
            question: question.into(),
            ..Answer::default()
        }
    }

    pub fn skipped(question: impl Into<String>) -> Self {
        Answer {
            skipped: true,
            ..Answer::new(question)
        }
    }
}

impl SessionResult {
    fn finished(id: &str, status: Status, session_sha256: &str, submitted_at: &str) -> Self {
        SessionResult {
            asqr: FORMAT_VERSION,
            id: id.to_owned(),
            status,
            submitted_at: Some(submitted_at.to_owned()),
            session_sha256: Some(session_sha256.to_owned()),
            reason: None,
            error: None,
            current: None,
            answers: Vec::new(),
        }
    }

    /// `answers` holds every question of the session once, in session order.
    pub fn submitted(
        id: &str,
        session_sha256: &str,
        submitted_at: &str,
        answers: Vec<Answer>,
    ) -> Self {
        SessionResult {
            answers,
            ..Self::finished(id, Status::Submitted, session_sha256, submitted_at)
        }
    }

    pub fn cancelled(
        id: &str,
        session_sha256: &str,
        submitted_at: &str,
        reason: Option<String>,
    ) -> Self {
        SessionResult {
            reason,
            ..Self::finished(id, Status::Cancelled, session_sha256, submitted_at)
        }
    }

    pub fn error(
        id: &str,
        session_sha256: &str,
        submitted_at: &str,
        message: impl Into<String>,
    ) -> Self {
        SessionResult {
            error: Some(message.into()),
            ..Self::finished(id, Status::Error, session_sha256, submitted_at)
        }
    }

    pub fn draft(id: &str, current: &str, answers: Vec<Answer>) -> Self {
        SessionResult {
            asqr: FORMAT_VERSION,
            id: id.to_owned(),
            status: Status::Draft,
            submitted_at: None,
            session_sha256: None,
            reason: None,
            error: None,
            current: Some(current.to_owned()),
            answers,
        }
    }
}

/// `time` as RFC 3339 with its offset and whole seconds, the form results
/// use: `2026-09-27T17:05:12+02:00`.
pub fn rfc3339(time: &jiff::Zoned) -> String {
    time.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// The current local time in the form [`rfc3339`] writes.
pub fn now_rfc3339() -> String {
    rfc3339(&jiff::Zoned::now())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const AT: &str = "2026-09-27T17:05:12+02:00";

    #[test]
    fn a_submitted_result_writes_only_the_fields_in_use() {
        let result = SessionResult::submitted(
            "batch-01",
            "9f2c",
            AT,
            vec![
                Answer {
                    selected: vec!["new1".into()],
                    note: Some("maybe shorter".into()),
                    ..Answer::new("q1")
                },
                Answer {
                    selected: vec!["old".into()],
                    defaulted: true,
                    ..Answer::new("q2")
                },
                Answer {
                    note: Some("skip, the image is wrong".into()),
                    ..Answer::skipped("q3")
                },
                Answer {
                    custom: Some("Snappy -- own".into()),
                    ..Answer::new("q4")
                },
            ],
        );

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "asqr": 1,
                "id": "batch-01",
                "status": "submitted",
                "submitted_at": AT,
                "session_sha256": "9f2c",
                "answers": [
                    { "question": "q1", "selected": ["new1"], "note": "maybe shorter" },
                    { "question": "q2", "selected": ["old"], "defaulted": true },
                    { "question": "q3", "skipped": true, "note": "skip, the image is wrong" },
                    { "question": "q4", "custom": "Snappy -- own" }
                ]
            })
        );
    }

    #[test]
    fn a_cancelled_result_carries_the_reason_and_no_answers() {
        let result = SessionResult::cancelled("batch-01", "9f2c", AT, Some("out of date".into()));

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "asqr": 1,
                "id": "batch-01",
                "status": "cancelled",
                "submitted_at": AT,
                "session_sha256": "9f2c",
                "reason": "out of date",
                "answers": []
            })
        );
    }

    #[test]
    fn an_error_result_carries_the_message() {
        let result =
            SessionResult::error("batch-01", "9f2c", AT, "asqr: unsupported format version 2");

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "asqr": 1,
                "id": "batch-01",
                "status": "error",
                "submitted_at": AT,
                "session_sha256": "9f2c",
                "error": "asqr: unsupported format version 2",
                "answers": []
            })
        );
    }

    #[test]
    fn a_draft_has_the_cursor_but_no_time_and_no_hash() {
        let draft = SessionResult::draft("batch-01", "q2", vec![Answer::new("q1")]);

        assert_eq!(
            serde_json::to_value(&draft).expect("draft serializes"),
            json!({
                "asqr": 1,
                "id": "batch-01",
                "status": "draft",
                "current": "q2",
                "answers": [{ "question": "q1" }]
            })
        );
    }

    #[test]
    fn round_trips_through_json() {
        let result = SessionResult::submitted(
            "batch-01",
            "9f2c",
            AT,
            vec![Answer {
                selected: vec!["a".into(), "b".into()],
                custom: Some("c".into()),
                ..Answer::new("q")
            }],
        );
        let written = serde_json::to_string(&result).expect("result serializes");

        assert_eq!(
            serde_json::from_str::<SessionResult>(&written).expect("parses"),
            result
        );
    }

    #[test]
    fn formats_times_as_rfc_3339_with_the_offset() {
        let zoned: jiff::Zoned = "2026-09-27T17:05:12+02:00[Europe/Berlin]"
            .parse()
            .expect("test time parses");

        assert_eq!(rfc3339(&zoned), AT);
    }

    #[test]
    fn the_current_time_has_the_rfc_3339_shape() {
        let now = now_rfc3339();

        // "2026-09-27T17:05:12+02:00": 25 characters, with the offset last.
        assert_eq!(now.len(), 25, "{now}");
        assert!(now.parse::<jiff::Timestamp>().is_ok(), "{now}");
    }
}
