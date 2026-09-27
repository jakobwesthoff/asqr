// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr ask` (spec sections 3.5 and 8).

mod common;

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use asqr::format::{Answer, SessionResult, now_rfc3339};
use asqr::queue::{QueueLocation, finish, lock_queue, save_draft, session_sha256};
use common::{Sandbox, stderr, stdout};

const SESSION: &str =
    r#"{"asqr": 1, "id": "batch-01", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

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
        r#"{"asqr": 1, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
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
fn makes_image_paths_absolute_and_keeps_unknown_fields() {
    let sandbox = Sandbox::new();
    sandbox.file("session/images/q.png", "png");
    let file = sandbox.file(
        "session/session.json",
        r#"{"asqr": 1, "id": "img", "later": 1,
            "questions": [{"id": "q", "text": "?", "kind": "text", "image": "images/q.png"}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    let dropped = inbox_file(&sandbox, "img");
    let image = dropped["questions"][0]["image"].as_str().expect("image");
    assert!(Path::new(image).is_absolute(), "{image}");
    assert!(Path::new(image).is_file(), "{image}");
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
fn refuses_an_invalid_session() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("bad.json", r#"{"asqr": 2, "id": "bad", "questions": []}"#);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert!(stderr(&output).contains("error: asqr: unsupported format version 2"));
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
        SessionResult::submitted("batch-01", sha, &now_rfc3339(), Vec::new())
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
        SessionResult::submitted("batch-01", sha, &now_rfc3339(), vec![Answer::skipped("q")])
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
