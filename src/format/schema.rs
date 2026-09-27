// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The JSON Schema of the format, derived from the same Rust types that
//! read and write the files, so the published schema cannot drift from what
//! asqr accepts. The repo ships it as `schema/session.v1.json` and
//! `schema/result.v1.json`, and `asqr schema` prints it.
//!
//! The schema describes the shape. The rules that span fields (ids,
//! option counts, length bounds) are checked by validation and noted in the
//! spec.

use super::{Session, SessionResult};

/// The schema of a session file.
pub fn session_schema() -> serde_json::Value {
    schemars::schema_for!(Session).to_value()
}

/// The schema of a result file, which drafts share.
pub fn result_schema() -> serde_json::Value {
    schemars::schema_for!(SessionResult).to_value()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Compares a derived schema with the checked-in file. With
    /// `ASQR_UPDATE_SCHEMA=1` the file is rewritten instead, which is how a
    /// deliberate format change updates it.
    fn assert_matches_file(schema: &serde_json::Value, file: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schema")
            .join(file);
        let derived = format!(
            "{}\n",
            serde_json::to_string_pretty(schema).expect("schema serializes")
        );

        if std::env::var_os("ASQR_UPDATE_SCHEMA").is_some() {
            std::fs::write(&path, &derived).expect("schema file is writable");
        }
        let checked_in = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            checked_in == derived,
            "{} differs from the Rust types; run `ASQR_UPDATE_SCHEMA=1 cargo test` after a deliberate format change",
            path.display()
        );
    }

    #[test]
    fn the_session_schema_file_matches_the_types() {
        assert_matches_file(&session_schema(), "session.v1.json");
    }

    #[test]
    fn the_result_schema_file_matches_the_types() {
        assert_matches_file(&result_schema(), "result.v1.json");
    }

    #[test]
    fn the_session_schema_describes_questions_and_options() {
        let schema = session_schema();

        assert_eq!(schema["title"], "Session");
        assert!(schema["properties"]["questions"].is_object());
        assert!(schema["$defs"]["Question"]["properties"]["options"].is_object());
        assert!(schema["$defs"]["Choice"]["properties"]["default"].is_object());
    }
}
