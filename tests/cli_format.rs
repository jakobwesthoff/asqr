// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr new`, `asqr validate` and `asqr schema` (spec sections 4 and 8).

mod common;

use asqr::format::{FORMAT_VERSION, Session, validate};
use common::{Sandbox, stderr, stdout};

const EXAMPLE: &str = include_str!("../examples/sessions/release-checklist.json");

#[test]
fn new_prints_a_valid_skeleton_with_a_fresh_ulid() {
    let sandbox = Sandbox::new();

    let first = sandbox.asqr().arg("new").output().expect("runs");
    let second = sandbox.asqr().arg("new").output().expect("runs");

    assert_eq!(first.status.code(), Some(0));
    let session: Session = serde_json::from_str(&stdout(&first)).expect("the skeleton parses");
    let id = session.id.clone().expect("the skeleton has an id");
    assert!(ulid::Ulid::from_string(&id).is_ok(), "{id} is a ULID");
    assert_eq!(validate(&session, None), Ok(()));
    assert_eq!(session.asqr, FORMAT_VERSION);
    assert!(
        session.questions[0].context[0].title().is_some(),
        "the skeleton shows a context entry"
    );

    let other: Session = serde_json::from_str(&stdout(&second)).expect("parses");
    assert_ne!(other.id, session.id, "every skeleton gets its own id");
}

#[test]
fn validate_accepts_a_valid_file() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", EXAMPLE);

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), format!("{}: valid\n", file.display()));
    assert_eq!(stderr(&output), "");
}

#[test]
fn validate_lists_every_error_and_exits_with_the_error_code() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "bad.json",
        r#"{"asqr": 2, "id": ".x", "questions": [{"id": "q", "text": "?", "kind": "text", "max": 1}]}"#,
    );

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    let errors = stderr(&output);
    assert!(errors.contains("error: id: invalid session id"), "{errors}");
    assert!(
        errors.contains("error: questions[0].max: only a multi question takes min and max\n"),
        "{errors}"
    );
}

#[test]
fn validate_names_only_the_version_of_a_file_in_another_format_version() {
    let sandbox = Sandbox::new();
    // Under the rules of version 2 this file has more problems, but they
    // are not the asker's to fix: the format version is.
    let file = sandbox.file(
        "old.json",
        r#"{"asqr": 1, "id": ".x", "questions": [{"id": "q", "text": "?", "kind": "text", "image": "/q.png"}]}"#,
    );

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert_eq!(
        stderr(&output),
        "error: asqr: this asqr no longer supports format version 1; `asqr skill` describes the current format (version 2)\n"
    );
}

#[test]
fn validate_reports_a_file_that_is_not_a_session() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("broken.json", r#"{"asqr": 2, "questions": [{"id": "q"}]}"#);

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert!(
        stderr(&output).starts_with("error: not a session file:"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn validate_refuses_an_enlarge_that_is_no_boolean() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "pixels.json",
        r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "image", "path": "/sprites/hero.png", "enlarge": "no"}]}]}"#,
    );

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    let errors = stderr(&output);
    assert!(errors.starts_with("error: not a session file:"), "{errors}");
    assert!(errors.contains("expected a boolean"), "{errors}");
}

#[test]
fn validate_warns_without_failing() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "typo.json",
        r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text", "requred": true,
            "image": "/q.png", "context": [{"type": "image", "path": "missing.png"}]}]}"#,
    );

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    let warnings = stderr(&output);
    assert!(
        warnings.contains("warning: questions[0].requred: unknown field\n"),
        "{warnings}"
    );
    assert!(
        warnings.contains("warning: questions[0].image: ignored; since format version 2 an image is a context entry"),
        "{warnings}"
    );
    assert!(
        warnings.contains("warning: questions[0].context[0].path: relative path"),
        "{warnings}"
    );
    assert!(
        warnings.contains("warning: questions[0].context[0].path: file not found"),
        "{warnings}"
    );
}

#[test]
fn validate_fails_on_a_missing_file() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(sandbox.home().join("nope.json"))
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with("error: "),
        "{}",
        stderr(&output)
    );
}

#[test]
fn schema_prints_the_shipped_schema_files() {
    let sandbox = Sandbox::new();

    let session = sandbox.asqr().arg("schema").output().expect("runs");
    let result = sandbox
        .asqr()
        .args(["schema", "--result"])
        .output()
        .expect("runs");

    assert_eq!(stdout(&session), include_str!("../schema/session.v2.json"));
    assert_eq!(stdout(&result), include_str!("../schema/result.v2.json"));
}
