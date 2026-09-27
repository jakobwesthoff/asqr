// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Warnings about a session file (spec section 4): things that do not make
//! it invalid but are likely mistakes. Parsing ignores unknown fields so
//! later versions can add some; a typo such as `requred` would then be
//! silently lost, so `asqr validate` and `asqr ask` report them.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{FieldPath, Session, session_schema};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub path: FieldPath,
    pub kind: WarningKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    UnknownField,
    RelativeImagePath,
    ImageNotFound(PathBuf),
}

impl fmt::Display for Warning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", self.path)?;
        match &self.kind {
            WarningKind::UnknownField => formatter.write_str("unknown field"),
            WarningKind::RelativeImagePath => formatter.write_str(
                "relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path",
            ),
            WarningKind::ImageNotFound(path) => write!(formatter, "file not found: {}", path.display()),
        }
    }
}

/// The warnings for a session. `raw` is the file as parsed JSON, which
/// still holds the unknown fields the typed `session` dropped. `base_dir` is
/// the directory of the file, against which relative image paths are
/// checked; without it only absolute paths are.
pub fn warnings(raw: &Value, session: &Session, base_dir: Option<&Path>) -> Vec<Warning> {
    let mut found = Vec::new();
    unknown_fields(raw, &mut found);
    images(session, base_dir, &mut found);
    found
}

/// The known field names of each object in the format, read from the
/// derived schema so they cannot fall out of step with the types.
struct KnownFields {
    session: BTreeSet<String>,
    question: BTreeSet<String>,
    choice: BTreeSet<String>,
    custom: BTreeSet<String>,
    length: BTreeSet<String>,
}

impl KnownFields {
    fn from_schema() -> Self {
        let schema = session_schema();
        let names = |object: &Value| -> BTreeSet<String> {
            object["properties"]
                .as_object()
                .map(|properties| properties.keys().cloned().collect())
                .unwrap_or_default()
        };
        let definition = |name: &str| names(&schema["$defs"][name]);
        KnownFields {
            session: names(&schema),
            question: definition("Question"),
            choice: definition("Choice"),
            custom: definition("CustomConfig"),
            length: definition("Length"),
        }
    }
}

fn unknown_fields(raw: &Value, found: &mut Vec<Warning>) {
    let known = KnownFields::from_schema();
    let mut report = |object: &Value, fields: &BTreeSet<String>, path: Option<&FieldPath>| {
        for key in object
            .as_object()
            .into_iter()
            .flat_map(|object| object.keys())
        {
            if !fields.contains(key) {
                let path = match path {
                    Some(parent) => parent.field(key),
                    None => FieldPath::root(key),
                };
                found.push(Warning {
                    path,
                    kind: WarningKind::UnknownField,
                });
            }
        }
    };

    report(raw, &known.session, None);
    let questions = raw["questions"].as_array().into_iter().flatten();
    for (index, question) in questions.enumerate() {
        let path = FieldPath::root("questions").index(index);
        report(question, &known.question, Some(&path));

        let options = question["options"].as_array().into_iter().flatten();
        for (index, option) in options.enumerate() {
            report(
                option,
                &known.choice,
                Some(&path.field("options").index(index)),
            );
        }
        // `custom: true` is a plain switch; only the object form has fields.
        if question["custom"].is_object() {
            let custom = path.field("custom");
            report(&question["custom"], &known.custom, Some(&custom));
            report(
                &question["custom"]["length"],
                &known.length,
                Some(&custom.field("length")),
            );
        }
        report(
            &question["length"],
            &known.length,
            Some(&path.field("length")),
        );
    }
}

fn images(session: &Session, base_dir: Option<&Path>, found: &mut Vec<Warning>) {
    for (index, question) in session.questions.iter().enumerate() {
        let Some(image) = &question.image else {
            continue;
        };
        let path = FieldPath::root("questions").index(index).field("image");
        let image = Path::new(image);

        let resolved = if image.is_absolute() {
            Some(image.to_path_buf())
        } else {
            found.push(Warning {
                path: path.clone(),
                kind: WarningKind::RelativeImagePath,
            });
            base_dir.map(|base| base.join(image))
        };
        if let Some(resolved) = resolved
            && !resolved.is_file()
        {
            found.push(Warning {
                path,
                kind: WarningKind::ImageNotFound(resolved),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const EXAMPLE: &str = include_str!("../../examples/sessions/alt-rework-batch.json");

    fn warn(json: &str, base_dir: Option<&Path>) -> Vec<String> {
        let raw: serde_json::Value = serde_json::from_str(json).expect("test json parses");
        let session: Session = serde_json::from_value(raw.clone()).expect("test session parses");
        warnings(&raw, &session, base_dir)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn repo() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn the_example_only_warns_about_its_placeholder_image() {
        assert_eq!(
            warn(EXAMPLE, None),
            [
                "questions[0].image: file not found: /tmp/asqr-example/301-holly-crown-green-robe-feast-ghost.png"
            ]
        );
    }

    #[test]
    fn reports_unknown_fields_at_every_level() {
        let json = r#"{"asqr": 1, "future": 1, "questions": [
            {"id": "s", "text": "?", "kind": "single", "requred": true,
             "options": [{"id": "a", "label": "A", "colour": "red"}],
             "custom": {"label": "Own", "x": 1, "length": {"warn": 9, "z": 2}}},
            {"id": "t", "text": "?", "kind": "text", "length": {"warn": 9, "y": 3}},
            {"id": "m", "text": "?", "kind": "multi", "custom": true,
             "options": [{"id": "a", "label": "A"}]}]}"#;

        assert_eq!(
            warn(json, None),
            [
                "future: unknown field",
                "questions[0].requred: unknown field",
                "questions[0].options[0].colour: unknown field",
                "questions[0].custom.x: unknown field",
                "questions[0].custom.length.z: unknown field",
                "questions[1].length.y: unknown field",
            ]
        );
    }

    #[test]
    fn the_dropped_max_and_multiline_are_unknown_fields() {
        let json = r#"{"asqr": 1, "questions": [
            {"id": "t", "text": "?", "kind": "text", "length": {"max": 9}},
            {"id": "s", "text": "?", "kind": "single", "options": [{"id": "a", "label": "A"}],
             "custom": {"multiline": true}}]}"#;

        assert_eq!(
            warn(json, None),
            [
                "questions[0].length.max: unknown field",
                "questions[1].custom.multiline: unknown field",
            ]
        );
    }

    #[test]
    fn warns_about_relative_image_paths() {
        let json = r#"{"asqr": 1, "questions": [
            {"id": "q", "text": "?", "kind": "text", "image": "Cargo.toml"}]}"#;

        // Relative to the file's directory the image exists, but a relative
        // path means nothing once the file sits in an inbox.
        assert_eq!(
            warn(json, Some(repo())),
            [
                "questions[0].image: relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path"
            ]
        );
    }

    #[test]
    fn checks_that_relative_images_exist_where_the_file_is() {
        let json = r#"{"asqr": 1, "questions": [
            {"id": "q", "text": "?", "kind": "text", "image": "missing.png"}]}"#;

        assert_eq!(
            warn(json, Some(repo())),
            [
                "questions[0].image: relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path",
                &format!(
                    "questions[0].image: file not found: {}",
                    repo().join("missing.png").display()
                ),
            ]
        );
    }

    #[test]
    fn every_example_session_is_valid_and_uses_only_known_fields() {
        let examples = repo().join("examples/sessions");
        let mut checked = 0;
        for entry in std::fs::read_dir(&examples).expect("examples directory exists") {
            let path = entry.expect("directory entry is readable").path();
            let json = std::fs::read_to_string(&path).expect("example is readable");
            let raw: serde_json::Value = serde_json::from_str(&json).expect("example is JSON");
            let session: Session = serde_json::from_value(raw.clone()).expect("example parses");

            assert_eq!(
                super::super::validate(&session, None),
                Ok(()),
                "{}",
                path.display()
            );
            let unknown: Vec<_> = warnings(&raw, &session, None)
                .into_iter()
                .filter(|warning| warning.kind == WarningKind::UnknownField)
                .collect();
            assert!(unknown.is_empty(), "{}: {unknown:?}", path.display());
            checked += 1;
        }
        assert!(checked >= 3, "only {checked} examples found");
    }

    #[test]
    fn an_existing_absolute_image_is_fine() {
        let image = repo().join("Cargo.toml");
        let json = format!(
            r#"{{"asqr": 1, "questions": [{{"id": "q", "text": "?", "kind": "text", "image": {image:?}}}]}}"#,
            image = image.display().to_string()
        );

        assert_eq!(warn(&json, None), Vec::<String>::new());
    }
}
