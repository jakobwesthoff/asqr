// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr wait`, `asqr result` and `asqr status` (spec section 8).

mod common;

use std::time::{Duration, Instant};

use asqr::format::{Answer, SessionResult, now_rfc3339};
use asqr::queue::{QueueLocation, finish, lock_queue, save_draft, session_sha256};
use common::{Sandbox, stderr, stdout};

fn location(sandbox: &Sandbox) -> QueueLocation {
    QueueLocation::at(sandbox.queue_dir())
}

/// Drops session `id` through `asqr ask`.
fn ask(sandbox: &Sandbox, id: &str) {
    let file = sandbox.file(
        &format!("{id}.json"),
        &format!(r#"{{"asqr": 1, "id": "{id}", "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#),
    );
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
}

fn answer(sandbox: &Sandbox, id: &str, result: impl Fn(&str) -> SessionResult) {
    let path = sandbox.queue_dir().join("inbox").join(format!("{id}.json"));
    let bytes = std::fs::read(&path).expect("session is waiting");
    finish(&location(sandbox), &path, &result(&session_sha256(&bytes))).expect("finished");
}

#[test]
fn result_prints_a_result_that_is_there() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "done");
    answer(&sandbox, "done", |sha| {
        SessionResult::cancelled("done", sha, &now_rfc3339(), None)
    });

    let output = sandbox
        .asqr_in_queue(&["result", "DONE"])
        .output()
        .expect("runs");

    // `result` did its job whatever the status says.
    assert_eq!(output.status.code(), Some(0));
    let result: SessionResult = serde_json::from_str(&stdout(&output)).expect("result JSON");
    assert_eq!(result.id, "done");
}

#[test]
fn result_of_a_missing_result_is_the_unknown_id_code() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr_in_queue(&["result", "nothing"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(13));
    assert_eq!(stdout(&output), "");
}

#[test]
fn result_fails_on_a_file_that_is_not_a_result() {
    let sandbox = Sandbox::new();
    location(&sandbox).create_layout().expect("layout");
    std::fs::write(sandbox.queue_dir().join("outbox/odd.json"), "[]").expect("written");

    let output = sandbox
        .asqr_in_queue(&["result", "odd"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("is not a result file"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn wait_returns_at_once_for_an_unknown_id() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr_in_queue(&["wait", "nobody-asked"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(13));
}

#[test]
fn wait_prints_a_result_that_is_already_there_with_its_exit_code() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "broken");
    answer(&sandbox, "broken", |sha| {
        SessionResult::error("broken", sha, &now_rfc3339(), "bad")
    });

    let output = sandbox
        .asqr_in_queue(&["wait", "broken"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert!(
        stdout(&output).contains("\"status\": \"error\""),
        "{}",
        stdout(&output)
    );
}

#[test]
fn wait_times_out_on_a_waiting_session() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "pending");
    let started = Instant::now();

    let output = sandbox
        .asqr_in_queue(&["wait", "pending", "--timeout", "1"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(12));
    assert!(started.elapsed() >= Duration::from_secs(1));
}

#[test]
fn an_archived_session_is_known_even_without_a_result() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "gone");
    answer(&sandbox, "gone", |sha| {
        SessionResult::submitted("gone", sha, &now_rfc3339(), Vec::new())
    });
    std::fs::remove_file(sandbox.queue_dir().join("outbox/gone.json")).expect("the asker read it");

    let output = sandbox
        .asqr_in_queue(&["wait", "gone", "--timeout", "0"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(12));
}

#[test]
fn wait_fails_when_the_outbox_cannot_be_read() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "stuck");
    let outbox = sandbox.queue_dir().join("outbox");
    std::fs::remove_dir(&outbox).expect("empty outbox is removed");
    std::fs::write(&outbox, "not a directory").expect("written");

    let output = sandbox
        .asqr_in_queue(&["wait", "stuck", "--timeout", "1"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn status_lists_waiting_and_answered_sessions() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "fresh");
    ask(&sandbox, "started");
    ask(&sandbox, "done");
    let typed = Answer {
        custom: Some("half".into()),
        ..Answer::new("q")
    };
    save_draft(
        &location(&sandbox),
        &SessionResult::draft("started", "q", vec![typed]),
    )
    .expect("saved");
    answer(&sandbox, "done", |sha| {
        SessionResult::submitted("done", sha, &now_rfc3339(), Vec::new())
    });

    let text = sandbox.asqr_in_queue(&["status"]).output().expect("runs");
    let json = sandbox
        .asqr_in_queue(&["status", "--json"])
        .output()
        .expect("runs");

    assert_eq!(text.status.code(), Some(0));
    let text = stdout(&text);
    assert!(text.contains("watched by: nobody\n"), "{text}");
    assert!(text.contains("waiting (2):\n"), "{text}");
    assert!(text.contains("  started  (draft with answers)\n"), "{text}");
    assert!(text.contains("  fresh\n"), "{text}");
    assert!(
        text.contains("answered (1):\n  done  submitted\n"),
        "{text}"
    );

    let status: serde_json::Value = serde_json::from_str(&stdout(&json)).expect("status JSON");
    assert!(status["watched_by"].is_null(), "{status}");
    let waiting = status["waiting"].as_array().expect("waiting list");
    assert_eq!(waiting.len(), 2);
    assert!(
        waiting
            .iter()
            .any(|entry| entry["id"] == "started" && entry["draft_has_answers"] == true),
        "{status}"
    );
    assert_eq!(status["answered"][0]["id"], "done");
    assert_eq!(status["answered"][0]["status"], "submitted");
}

#[test]
fn status_names_the_watching_instance_and_unreadable_results() {
    let sandbox = Sandbox::new();
    let _watching = lock_queue(&location(&sandbox)).expect("this test plays the running asqr");
    location(&sandbox).create_layout().expect("layout");
    std::fs::write(sandbox.queue_dir().join("outbox/odd.json"), "[]").expect("written");

    let text = stdout(&sandbox.asqr_in_queue(&["status"]).output().expect("runs"));
    let json = stdout(
        &sandbox
            .asqr_in_queue(&["status", "--json"])
            .output()
            .expect("runs"),
    );

    assert!(
        text.contains(&format!("watched by: pid {}", std::process::id())),
        "{text}"
    );
    assert!(text.contains("waiting (0):\n"), "{text}");
    assert!(text.contains("  odd  unreadable\n"), "{text}");
    let status: serde_json::Value = serde_json::from_str(&json).expect("JSON");
    assert!(
        status["watched_by"]
            .as_str()
            .expect("holder")
            .starts_with("pid ")
    );
    assert!(status["answered"][0]["status"].is_null(), "{status}");
}

#[test]
fn status_shows_every_result_status_and_skips_other_files() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "rejected");
    ask(&sandbox, "invalid");
    answer(&sandbox, "rejected", |sha| {
        SessionResult::cancelled("rejected", sha, &now_rfc3339(), None)
    });
    answer(&sandbox, "invalid", |sha| {
        SessionResult::error("invalid", sha, &now_rfc3339(), "bad")
    });
    std::fs::write(sandbox.queue_dir().join("outbox/.DS_Store"), "").expect("written");

    let text = stdout(&sandbox.asqr_in_queue(&["status"]).output().expect("runs"));

    assert!(
        text.contains("answered (2):\n  invalid  error\n  rejected  cancelled\n"),
        "{text}"
    );
}

/// Replaces a queue directory with a plain file, so reading it fails.
fn break_dir(sandbox: &Sandbox, dir: &str) {
    let path = sandbox.queue_dir().join(dir);
    std::fs::remove_dir_all(&path).expect("directory is removed");
    std::fs::write(&path, "not a directory").expect("written");
}

#[test]
fn status_fails_when_the_queue_cannot_be_read() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "any");
    break_dir(&sandbox, "inbox");

    assert_eq!(
        sandbox
            .asqr_in_queue(&["status"])
            .output()
            .expect("runs")
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn wait_fails_when_the_inbox_cannot_be_read() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "any");
    break_dir(&sandbox, "inbox");

    assert_eq!(
        sandbox
            .asqr_in_queue(&["wait", "any"])
            .output()
            .expect("runs")
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn result_fails_when_the_outbox_cannot_be_read() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "any");
    break_dir(&sandbox, "outbox");

    assert_eq!(
        sandbox
            .asqr_in_queue(&["result", "any"])
            .output()
            .expect("runs")
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn result_fails_on_a_result_it_may_not_read() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    ask(&sandbox, "locked");
    answer(&sandbox, "locked", |sha| {
        SessionResult::submitted("locked", sha, &now_rfc3339(), Vec::new())
    });
    let path = sandbox.queue_dir().join("outbox/locked.json");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
        .expect("permissions change");

    let output = sandbox
        .asqr_in_queue(&["result", "locked"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("cannot read"),
        "{}",
        stderr(&output)
    );
}
