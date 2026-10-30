// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr prune` (spec section 5.9).

mod common;

use asqr::format::{SessionResult, now_rfc3339};
use asqr::queue::{QueueLocation, finish, session_sha256};
use common::{Sandbox, stdout};

fn asked_and_answered(sandbox: &Sandbox, id: &str) {
    let file = sandbox.file(
        &format!("{id}.json"),
        &format!(r#"{{"asqr": 2, "id": "{id}", "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#),
    );
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    let path = sandbox.queue_dir().join("inbox").join(format!("{id}.json"));
    let sha = session_sha256(&std::fs::read(&path).expect("waiting"));
    finish(
        &QueueLocation::at(sandbox.queue_dir()),
        &path,
        &SessionResult::submitted(id, &sha, &now_rfc3339(), None, Vec::new()),
    )
    .expect("finished");
}

fn count(sandbox: &Sandbox, dir: &str) -> usize {
    std::fs::read_dir(sandbox.queue_dir().join(dir))
        .expect("readable")
        .count()
}

#[test]
fn removes_archive_entries_and_prints_them() {
    let sandbox = Sandbox::new();
    asked_and_answered(&sandbox, "done");

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "0s"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        stdout(&output).starts_with("removed "),
        "{}",
        stdout(&output)
    );
    assert_eq!(count(&sandbox, "archive"), 0);
    assert_eq!(
        count(&sandbox, "outbox"),
        1,
        "results stay without --results"
    );
}

#[test]
fn removes_results_only_when_asked() {
    let sandbox = Sandbox::new();
    asked_and_answered(&sandbox, "done");

    sandbox
        .asqr_in_queue(&["prune", "--older-than", "0s", "--results"])
        .assert()
        .success();

    assert_eq!(count(&sandbox, "outbox"), 0);
}

#[test]
fn keeps_what_is_younger_than_the_age() {
    let sandbox = Sandbox::new();
    asked_and_answered(&sandbox, "done");

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "30d"])
        .output()
        .expect("runs");

    assert_eq!(stdout(&output), "");
    assert_eq!(count(&sandbox, "archive"), 1);
}

#[test]
fn a_malformed_age_is_a_usage_error() {
    let output = Sandbox::new()
        .asqr_in_queue(&["prune", "--older-than", "soon"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn fails_when_the_archive_cannot_be_read() {
    let sandbox = Sandbox::new();
    asked_and_answered(&sandbox, "done");
    let archive = sandbox.queue_dir().join("archive");
    std::fs::remove_dir_all(&archive).expect("removed");
    std::fs::write(&archive, "not a directory").expect("written");

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "0s"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn removes_a_named_queue_that_holds_nothing_and_prints_it() {
    let sandbox = Sandbox::new();
    // A mistyped queue name made a queue on its first use.
    let output = sandbox
        .asqr()
        .args(["--queue", "asqr-todo", "status"])
        .output()
        .expect("runs");
    assert!(output.status.success());
    let paths = sandbox
        .asqr()
        .args(["--queue", "asqr-todo", "paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&paths)).expect("JSON");
    let stray = paths["queue"].as_str().expect("a queue").to_owned();
    std::fs::create_dir_all(std::path::Path::new(&stray).join("inbox")).expect("made");

    let output = sandbox
        .asqr()
        .args(["prune", "--older-than", "30d"])
        .output()
        .expect("runs");

    assert!(output.status.success());
    assert_eq!(stdout(&output), format!("removed {stray}\n"));
    assert!(!std::path::Path::new(&stray).exists());
}

#[test]
fn queues_that_cannot_be_listed_fail_the_prune() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let paths = sandbox
        .asqr()
        .args(["--queue", "work", "paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&paths)).expect("JSON");
    let queue = std::path::PathBuf::from(paths["queue"].as_str().expect("a queue"));
    std::fs::create_dir_all(&queue).expect("made");
    let queues = queue.parent().expect("the queues directory").to_path_buf();
    std::fs::set_permissions(&queues, std::fs::Permissions::from_mode(0o000))
        .expect("permissions change");

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "30d"])
        .output()
        .expect("runs");
    std::fs::set_permissions(&queues, std::fs::Permissions::from_mode(0o755))
        .expect("permissions back");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with("error: cannot prune the queue: "),
        "{output:?}"
    );
}
