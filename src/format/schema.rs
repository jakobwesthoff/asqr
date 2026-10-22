// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The JSON Schema of the format, derived from the same Rust types that
//! read and write the files, so the published schema cannot drift from what
//! asqr accepts. The repo ships it as `schema/session.v2.json` and
//! `schema/result.v2.json`, and `asqr schema` prints it.
//!
//! The schema describes the shape, plus the rules a schema states
//! directly: the format version, at least one question, one context
//! entry, and exactly one of `path` and `content`. The other rules that
//! span fields (ids, option counts, length bounds) are checked by
//! validation and noted in the spec.

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

    use serde_json::json;

    use super::*;
    use crate::format::FORMAT_VERSION;

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
        assert_matches_file(
            &session_schema(),
            &format!("session.v{FORMAT_VERSION}.json"),
        );
    }

    #[test]
    fn the_result_schema_file_matches_the_types() {
        assert_matches_file(&result_schema(), &format!("result.v{FORMAT_VERSION}.json"));
    }

    #[test]
    fn the_session_schema_describes_questions_and_options() {
        let schema = session_schema();

        assert_eq!(schema["title"], "Session");
        assert!(schema["properties"]["questions"].is_object());
        assert!(schema["$defs"]["Question"]["properties"]["options"].is_object());
        assert!(schema["$defs"]["Choice"]["properties"]["default"].is_object());
        assert!(schema["$defs"]["Choice"]["properties"]["context"].is_object());
        assert!(schema["$defs"]["ContextEntry"]["oneOf"].is_array());
    }

    #[test]
    fn both_schemas_pin_the_format_version() {
        for schema in [session_schema(), result_schema()] {
            assert_eq!(
                schema["properties"]["asqr"]["const"], FORMAT_VERSION,
                "{}",
                schema["title"]
            );
        }
    }

    #[test]
    fn a_session_holds_at_least_one_question() {
        // Validation refuses an empty list, so the schema must not accept
        // what `asqr validate` refuses.
        assert_eq!(session_schema()["properties"]["questions"]["minItems"], 1);
    }

    #[test]
    fn context_holds_at_most_one_entry_and_may_be_empty() {
        let schema = session_schema();

        for holder in ["Question", "Choice"] {
            let context = &schema["$defs"][holder]["properties"]["context"];
            assert_eq!(context["maxItems"], 1, "{holder}");
            assert!(context.get("minItems").is_none(), "{holder}");
            // A `null` context is no context, as the parser reads it.
            assert_eq!(context["type"], json!(["array", "null"]), "{holder}");
        }
    }

    /// How many branches of `definition`'s `oneOf` or `anyOf` (`keyword`)
    /// `object` matches, as a schema validator counts them. The branches
    /// only use `required` and the `type` of a property, so those two are
    /// all this checks; the repository has no schema validator to ask.
    fn matching_branches(
        definition: &serde_json::Value,
        keyword: &str,
        object: &serde_json::Value,
    ) -> usize {
        let has_type = |value: &serde_json::Value, wanted: &str| match wanted {
            "string" => value.is_string(),
            "object" => value.is_object(),
            other => panic!("no branch checks for type {other}"),
        };
        definition[keyword]
            .as_array()
            .unwrap_or_else(|| panic!("the definition has {keyword}"))
            .iter()
            .filter(|branch| {
                let required = branch["required"].as_array().into_iter().flatten();
                let typed = branch["properties"].as_object().into_iter().flatten();
                required
                    .map(|key| key.as_str().expect("a key name"))
                    .all(|key| object.get(key).is_some())
                    && typed.into_iter().all(|(key, property)| {
                        let wanted = property["type"].as_str().expect("a property type");
                        object.get(key).is_none_or(|value| has_type(value, wanted))
                    })
            })
            .count()
    }

    /// Checks that the definition `name` of `schema` takes exactly one of
    /// `path` and `content`, as validation does.
    fn assert_one_of_path_and_content(schema: &serde_json::Value, name: &str) {
        let definition = &schema["$defs"][name];
        let matches = |object| matching_branches(definition, "oneOf", &object);
        assert_eq!(matches(json!({"path": "/a.rs"})), 1, "{name}");
        // A key counts by being there, so an empty `content` names an
        // empty text, as validation sees it.
        assert_eq!(matches(json!({"content": ""})), 1, "{name}");
        // `null` is how serde reads a missing value, so validation accepts
        // it next to the other key, and so must the schema.
        assert_eq!(matches(json!({"path": null, "content": "x"})), 1, "{name}");
        assert_eq!(
            matches(json!({"path": "/a.rs", "content": null})),
            1,
            "{name}"
        );
        assert_eq!(matches(json!({})), 0, "{name}");
        assert_eq!(matches(json!({"path": null, "content": null})), 0, "{name}");
        assert_eq!(
            matches(json!({"path": "/a.rs", "content": "x"})),
            2,
            "{name}: both match, so oneOf fails"
        );
    }

    #[test]
    fn code_markdown_and_diff_sides_take_exactly_one_of_path_and_content() {
        let schema = session_schema();

        for name in ["CodeEntry", "MarkdownEntry", "DiffSide"] {
            assert_one_of_path_and_content(&schema, name);
        }
    }

    #[test]
    fn a_diff_has_old_new_or_both() {
        let schema = session_schema();
        let definition = &schema["$defs"]["DiffEntry"];
        let matches = |object| matching_branches(definition, "anyOf", &object);
        let side = json!({"content": "x"});

        assert!(matches(json!({"old": side})) > 0);
        assert!(matches(json!({"new": side})) > 0);
        assert!(matches(json!({"old": side, "new": side})) > 0);
        assert!(matches(json!({"old": null, "new": side})) > 0);
        assert_eq!(matches(json!({"name": "a.rs"})), 0);
        assert_eq!(matches(json!({"old": null, "new": null})), 0);
    }

    #[test]
    fn the_text_fields_say_that_they_are_markdown() {
        let schema = session_schema();

        for description in [
            &schema["properties"]["intro"]["description"],
            &schema["$defs"]["Question"]["properties"]["text"]["description"],
            &schema["$defs"]["Choice"]["properties"]["description"]["description"],
            &schema["$defs"]["Choice"]["properties"]["label"]["description"],
        ] {
            // The doc comment's line breaks carry over into the schema.
            let description = description
                .as_str()
                .expect("the field has a description")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(description.contains("Markdown"), "{description}");
            assert!(description.contains("single newline"), "{description}");
            assert!(description.contains("HTML"), "{description}");
        }
    }

    #[test]
    fn the_label_says_it_is_a_short_handle() {
        let schema = session_schema();
        let description = schema["$defs"]["Choice"]["properties"]["label"]["description"]
            .as_str()
            .expect("the label has a description")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        for wanted in ["short handle", "`description`", "`context`"] {
            assert!(description.contains(wanted), "{wanted}: {description}");
        }
    }
}
