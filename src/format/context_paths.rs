// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Relative paths in context entries (spec section 3): `asqr ask` resolves
//! them against the directory of the session file, since a relative path
//! means nothing once the session sits in an inbox. The rule is public, so a
//! program that reads session files where they were written resolves their
//! paths as `ask` does, by one rule.

use std::path::Path;

use serde_json::Value;

/// Rewrites the relative paths of the context entries in the raw session
/// `raw` against the directory of `session_file`: the `path` of an image,
/// code or Markdown entry and the paths of a diff's sides, on questions and
/// on options. The raw JSON is edited, not the typed session, so unknown
/// fields survive. A path that is absolute already stays as it is.
pub fn make_context_paths_absolute(raw: &mut Value, session_file: &Path) {
    let base = session_file.parent().unwrap_or(Path::new(""));
    for question in array_mut(raw, "questions") {
        make_context_absolute(question, base);
        for option in array_mut(question, "options") {
            make_context_absolute(option, base);
        }
    }
}

/// The elements of the array in `field` of `object`, none when it is
/// missing. Indexing a `Value` mutably would insert a `null` for a missing
/// field, which a file written from it would then carry.
pub(crate) fn array_mut<'a>(
    object: &'a mut Value,
    field: &str,
) -> impl Iterator<Item = &'a mut Value> {
    object
        .get_mut(field)
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
}

/// Rewrites every path in the `context` of a question or an option.
fn make_context_absolute(holder: &mut Value, base: &Path) {
    for entry in array_mut(holder, "context") {
        make_path_absolute(entry, base);
        for side in ["old", "new"] {
            if let Some(side) = entry.get_mut(side) {
                make_path_absolute(side, base);
            }
        }
    }
}

/// Rewrites the `path` field of `object`, if it is there and relative.
fn make_path_absolute(object: &mut Value, base: &Path) {
    let Some(path) = object.get("path").and_then(Value::as_str) else {
        return;
    };
    let path = Path::new(path);
    if path.is_relative()
        && let Ok(absolute) = std::path::absolute(base.join(path))
    {
        object["path"] = Value::from(absolute.to_string_lossy().into_owned());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const SESSION_FILE: &str = "/work/session/session.json";

    fn resolved(mut raw: Value) -> Value {
        make_context_paths_absolute(&mut raw, Path::new(SESSION_FILE));
        raw
    }

    #[test]
    fn a_relative_image_path_becomes_absolute_against_the_session_file() {
        let raw = resolved(json!({"questions": [
            {"id": "q", "context": [{"type": "image", "path": "files/q.png"}]}
        ]}));

        assert_eq!(
            raw["questions"][0]["context"][0]["path"],
            "/work/session/files/q.png"
        );
    }

    #[test]
    fn an_option_s_context_path_becomes_absolute() {
        let raw = resolved(json!({"questions": [{"id": "q", "options": [
            {"id": "a", "context": [{"type": "code", "path": "fetch.rs"}]}
        ]}]}));

        assert_eq!(
            raw["questions"][0]["options"][0]["context"][0]["path"],
            "/work/session/fetch.rs"
        );
    }

    #[test]
    fn both_sides_of_a_diff_become_absolute() {
        let raw = resolved(json!({"questions": [{"id": "q", "context": [
            {"type": "diff", "old": {"path": "old.rs"}, "new": {"path": "new.rs"}}
        ]}]}));

        let diff = &raw["questions"][0]["context"][0];
        assert_eq!(diff["old"]["path"], "/work/session/old.rs");
        assert_eq!(diff["new"]["path"], "/work/session/new.rs");
    }

    #[test]
    fn an_absolute_path_stays() {
        let raw = resolved(json!({"questions": [
            {"id": "q", "context": [{"type": "image", "path": "/elsewhere/q.png"}]}
        ]}));

        assert_eq!(
            raw["questions"][0]["context"][0]["path"],
            "/elsewhere/q.png"
        );
    }

    #[test]
    fn a_session_without_questions_is_left_alone() {
        let raw = resolved(json!({"asqr": 2}));

        assert_eq!(raw, json!({"asqr": 2}));
    }
}
