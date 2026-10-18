// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr ask` (spec sections 5.5 and 6.3).

mod common;

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use asqr::format::{Answer, SessionResult, now_rfc3339};
use asqr::queue::{QueueLocation, finish, lock_queue, save_draft, session_sha256};
use common::{Sandbox, stderr, stdout};

const SESSION: &str =
    r#"{"asqr": 2, "id": "batch-01", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

fn inbox_file(sandbox: &Sandbox, id: &str) -> serde_json::Value {
    let path = sandbox.queue_dir().join("inbox").join(format!("{id}.json"));
    serde_json::from_slice(&std::fs::read(&path).expect("inbox file exists"))
        .expect("inbox file is JSON")
}

fn location(sandbox: &Sandbox) -> QueueLocation {
    QueueLocation::at(sandbox.queue_dir())
}

/// Answers the waiting session `id` the way the TUI will: through the
/// finish path, with a result over the exact inbox bytes.
fn answer(sandbox: &Sandbox, id: &str, result: impl Fn(&str) -> SessionResult) {
    let path = sandbox.queue_dir().join("inbox").join(format!("{id}.json"));
    let bytes = std::fs::read(&path).expect("session is waiting");
    finish(&location(sandbox), &path, &result(&session_sha256(&bytes))).expect("finished");
}

#[test]
fn drops_the_session_and_prints_its_id() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "batch-01\n");
    assert_eq!(inbox_file(&sandbox, "batch-01")["id"], "batch-01");
}

#[test]
fn warns_when_no_asqr_watches_the_queue() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    let expected = format!(
        "warning: no asqr is watching queue {}; start `asqr` in another terminal",
        sandbox.queue_dir().display()
    );
    assert!(stderr(&output).contains(&expected), "{}", stderr(&output));
}

#[test]
fn does_not_warn_while_an_asqr_holds_the_queue() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    let _watching = lock_queue(&location(&sandbox)).expect("this test plays the running asqr");

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert!(
        !stderr(&output).contains("no asqr is watching"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn assigns_a_ulid_when_the_session_has_no_id() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "session.json",
        r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    let id = stdout(&output).trim().to_owned();
    assert!(ulid::Ulid::from_string(&id).is_ok(), "{id}");
    assert_eq!(inbox_file(&sandbox, &id)["id"], id.as_str());
}

#[test]
fn makes_every_context_path_absolute_and_keeps_unknown_fields() {
    let sandbox = Sandbox::new();
    for file in ["q.png", "a.png", "notes.md", "fetch.rs", "old.rs", "new.rs"] {
        sandbox.file(&format!("session/files/{file}"), "content");
    }
    let file = sandbox.file(
        "session/session.json",
        r#"{"asqr": 2, "id": "ctx", "later": 1,
            "questions": [{"id": "q", "text": "?", "kind": "text",
                           "context": [{"type": "image", "path": "files/q.png"}]},
                          {"id": "s", "text": "?", "kind": "single",
                           "context": [{"type": "markdown", "path": "files/notes.md"}],
                           "options": [{"id": "a", "label": "A",
                                        "context": [{"type": "image", "path": "files/a.png"}]},
                                       {"id": "b", "label": "B",
                                        "context": [{"type": "diff", "old": {"path": "files/old.rs"},
                                                     "new": {"path": "files/new.rs"}}]},
                                       {"id": "c", "label": "C",
                                        "context": [{"type": "diff", "new": {"content": "inline"}}]},
                                       {"id": "d", "label": "D",
                                        "context": [{"type": "code", "path": "files/fetch.rs"}]}]}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let dropped = inbox_file(&sandbox, "ctx");
    let options = &dropped["questions"][1]["options"];
    for path in [
        &dropped["questions"][0]["context"][0]["path"],
        &dropped["questions"][1]["context"][0]["path"],
        &options[0]["context"][0]["path"],
        &options[1]["context"][0]["old"]["path"],
        &options[1]["context"][0]["new"]["path"],
        &options[3]["context"][0]["path"],
    ] {
        let path = path.as_str().expect("path");
        assert!(Path::new(path).is_absolute(), "{path}");
        assert!(Path::new(path).is_file(), "{path}");
    }
    // Fields that were not there stay away: no `old` on a new file, no
    // `context` or `options` where the asker wrote none.
    assert_eq!(
        options[2]["context"][0],
        serde_json::json!({"type": "diff", "new": {"content": "inline"}})
    );
    assert!(dropped["questions"][0].get("options").is_none());
    assert!(options[2].get("description").is_none());
    assert_eq!(dropped["later"], 1);
    assert!(
        stderr(&output).contains("warning: later: unknown field"),
        "{}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).contains("relative path"),
        "ask fixes relative paths itself"
    );
}

#[test]
fn warnings_name_a_missing_file_by_its_absolute_path() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "session/session.json",
        r#"{"asqr": 2, "id": "missing", "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "code", "path": "files/fetch.rs"}]}]}"#,
    );
    let directory = file.parent().expect("the session has a directory");

    // The session file given by its bare name, from its own directory,
    // as an agent working there writes it.
    let output = sandbox
        .asqr_in_queue(&["ask", "session.json"])
        .current_dir(directory)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let expected = directory
        .canonicalize()
        .expect("the directory exists")
        .join("files/fetch.rs");
    assert!(
        stderr(&output).contains(&format!(
            "warning: questions[0].context[0].path: file not found: {}\n",
            expected.display()
        )),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_question_without_context_gets_none_in_the_dropped_file() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "session.json",
        r#"{"asqr": 2, "id": "plain", "questions": [{"id": "q", "text": "?", "kind": "single",
            "options": [{"id": "a", "label": "A"}]}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let dropped = inbox_file(&sandbox, "plain");
    assert!(dropped["questions"][0].get("context").is_none());
    assert!(
        dropped["questions"][0]["options"][0]
            .get("context")
            .is_none()
    );
}

#[test]
fn refuses_a_session_of_format_version_1_and_points_to_the_skill() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "old.json",
        r#"{"asqr": 1, "id": "old", "questions": [{"id": "q", "text": "?", "kind": "text",
            "image": "/q.png"}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert_eq!(
        stderr(&output),
        "error: asqr: this asqr no longer supports format version 1; `asqr skill` describes the current format (version 2)\n"
    );
    assert!(!sandbox.queue_dir().join("inbox").join("old.json").exists());
}

#[test]
fn refuses_an_invalid_session() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("bad.json", r#"{"asqr": 3, "id": "bad", "questions": []}"#);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert!(stderr(&output).contains("error: asqr: unsupported format version 3"));
    assert!(!sandbox.queue_dir().join("inbox").join("bad.json").exists());
}

#[test]
fn refuses_a_file_that_is_not_a_session() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("bad.json", "[]");

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
}

#[test]
fn refuses_a_session_that_is_no_json_object_without_a_panic() {
    // serde reads the session's struct from an array of its fields in
    // their order, so this array passes the typed parse.
    let array = r#"[2, null, null, null, null, null, [{"id": "a", "text": "?", "kind": "text"}]]"#;
    for (name, content) in [("array", array), ("string", r#""session""#)] {
        let sandbox = Sandbox::new();
        let file = sandbox.file("bad.json", content);

        let output = sandbox
            .asqr_in_queue(&["ask"])
            .arg(&file)
            .output()
            .expect("runs");

        assert_eq!(
            output.status.code(),
            Some(11),
            "{name}: {}",
            stderr(&output)
        );
        assert!(
            stderr(&output).starts_with("error: not a session file:"),
            "{name}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn replaces_a_waiting_session_nobody_answered() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    let changed = sandbox.file("changed.json", &SESSION.replace("\"?\"", "\"changed?\""));

    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&changed)
        .assert()
        .success();

    assert_eq!(
        inbox_file(&sandbox, "batch-01")["questions"][0]["text"],
        "changed?"
    );
}

#[test]
fn refuses_while_the_session_is_being_answered() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    let started = Answer {
        custom: Some("half".into()),
        ..Answer::new("q")
    };
    save_draft(
        &location(&sandbox),
        &SessionResult::draft("batch-01", "q", vec![started]),
    )
    .expect("saved");

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("is being answered"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn refuses_an_unread_result_unless_forced() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    answer(&sandbox, "batch-01", |sha| {
        SessionResult::submitted("batch-01", sha, &now_rfc3339(), None, Vec::new())
    });

    let refused = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");
    let forced = sandbox
        .asqr_in_queue(&["ask", "--force"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("unread result"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(forced.status.code(), Some(0));
    assert_eq!(inbox_file(&sandbox, "batch-01")["id"], "batch-01");
}

/// Starts `asqr ask --wait` in the background, answers the session once it
/// arrives, and returns what the waiting asker printed and how it exited.
fn ask_and_answer(result: impl Fn(&str) -> SessionResult) -> (Option<i32>, String, String) {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    let mut command = sandbox.spawnable_asqr();
    command
        .arg("--dir")
        .arg(sandbox.queue_dir())
        .args(["ask", "--wait", "--timeout", "20"])
        .arg(&file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().expect("asqr starts");

    let inbox = sandbox.queue_dir().join("inbox").join("batch-01.json");
    let started = Instant::now();
    while !inbox.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the session never arrived"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    answer(&sandbox, "batch-01", result);

    let output = child.wait_with_output().expect("asqr ends");
    (output.status.code(), stdout(&output), stderr(&output))
}

#[test]
fn waits_for_the_result_and_prints_only_the_result() {
    let (code, out, err) = ask_and_answer(|sha| {
        SessionResult::submitted(
            "batch-01",
            sha,
            &now_rfc3339(),
            None,
            vec![Answer::skipped("q")],
        )
    });

    assert_eq!(code, Some(0));
    let result: SessionResult = serde_json::from_str(&out).expect("stdout is the result JSON");
    assert_eq!(result.answers, [Answer::skipped("q")]);
    assert!(err.contains("batch-01"), "the id goes to stderr: {err}");
}

#[test]
fn a_rejected_session_exits_with_the_cancelled_code() {
    let (code, _, _) =
        ask_and_answer(|sha| SessionResult::cancelled("batch-01", sha, &now_rfc3339(), None));

    assert_eq!(code, Some(10));
}

#[test]
fn stops_waiting_after_the_timeout() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    let started = Instant::now();

    let output = sandbox
        .asqr_in_queue(&["ask", "--wait", "--timeout", "1"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(12));
    assert_eq!(stdout(&output), "");
    assert!(started.elapsed() >= Duration::from_secs(1));
}
