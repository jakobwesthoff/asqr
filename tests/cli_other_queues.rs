// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A session asked in another named queue than the one a command uses:
//! `wait` and `result` follow it there, `withdraw` and `ask --replaces`
//! name that queue and change nothing (spec sections 5.5 and 5.10).

mod common;

use common::{Sandbox, stderr, stdout};

/// Asks session `id` into the named queue `queue`.
fn ask_into(sandbox: &Sandbox, queue: &str, id: &str) {
    let file = sandbox.file(
        &format!("{queue}-{id}.json"),
        &format!(r#"{{"asqr": 2, "id": "{id}", "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#),
    );
    sandbox
        .asqr()
        .args(["--queue", queue, "ask"])
        .arg(&file)
        .assert()
        .success();
}

/// `asqr --queue <queue> <args>`.
fn in_queue(sandbox: &Sandbox, queue: &str, args: &[&str]) -> std::process::Output {
    sandbox
        .asqr()
        .args(["--queue", queue])
        .args(args)
        .output()
        .expect("runs")
}

#[test]
fn result_finds_a_session_in_another_queue_and_warns_where() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "other", "batch-01");

    let output = in_queue(&sandbox, "mine", &["result", "batch-01"]);

    assert_eq!(output.status.code(), Some(12), "it waits there");
    assert_eq!(
        stderr(&output),
        "warning: session batch-01 is in queue other, not in mine; using queue other\n\
         asqr: no result for batch-01 yet; the person has not answered yet, so wait for it with `asqr --queue other wait batch-01 --timeout 540`, or a lower timeout if your command limit is shorter\n"
    );
}

#[test]
fn wait_finds_a_session_in_another_queue_and_warns_where() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "other", "batch-01");

    let output = in_queue(&sandbox, "mine", &["wait", "batch-01", "--timeout", "0"]);

    assert_eq!(output.status.code(), Some(12));
    assert!(
        stderr(&output).starts_with(
            "warning: session batch-01 is in queue other, not in mine; using queue other\n"
        ),
        "{}",
        stderr(&output)
    );
}

#[test]
fn withdraw_names_the_queue_and_withdraws_nothing() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "other", "batch-01");

    let output = in_queue(&sandbox, "mine", &["withdraw", "batch-01"]);

    assert_eq!(output.status.code(), Some(13));
    assert_eq!(
        stderr(&output),
        "error: session \"batch-01\" is in queue other, not in mine; if it is yours, withdraw it with `asqr --queue other withdraw batch-01`\n"
    );
    let still = in_queue(&sandbox, "other", &["result", "batch-01"]);
    assert_eq!(still.status.code(), Some(12), "it still waits");
}

#[test]
fn ask_replaces_names_the_queue_the_old_session_is_in() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "other", "batch-01");
    let file = sandbox.file(
        "new.json",
        r#"{"asqr": 2, "id": "batch-02", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
    );

    let output = sandbox
        .asqr()
        .args(["--queue", "mine", "ask"])
        .arg(&file)
        .args(["--replaces", "batch-01"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(13));
    assert_eq!(
        stderr(&output),
        "error: session \"batch-01\" is in queue other, not in mine; withdraw it with `asqr --queue other withdraw batch-01`, then ask without --replaces\n"
    );
    let dropped = in_queue(&sandbox, "mine", &["result", "batch-02"]);
    assert_eq!(dropped.status.code(), Some(13), "nothing was dropped");
}

#[test]
fn a_session_in_two_other_queues_is_an_error_naming_both() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "first", "batch-01");
    ask_into(&sandbox, "second", "batch-01");

    let output = in_queue(&sandbox, "mine", &["result", "batch-01"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "error: session \"batch-01\" is in several queues: first, second; run this command again with --queue and one of them\n"
    );
}

#[test]
fn an_id_finished_here_is_not_looked_up_elsewhere() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "mine", "batch-01");
    let withdrawn = in_queue(&sandbox, "mine", &["withdraw", "batch-01"]);
    assert!(withdrawn.status.success());
    ask_into(&sandbox, "other", "batch-01");

    let output = in_queue(&sandbox, "mine", &["result", "batch-01"]);

    assert_eq!(output.status.code(), Some(14), "its own withdrawn result");
    assert!(stdout(&output).contains("\"withdrawn\""));
    assert!(!stderr(&output).contains("other"), "{}", stderr(&output));
}

#[test]
fn wait_withdraw_and_ask_replaces_name_several_queues_too() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "first", "batch-01");
    ask_into(&sandbox, "second", "batch-01");
    let file = sandbox.file(
        "new.json",
        r#"{"asqr": 2, "id": "batch-02", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
    );
    let several = "error: session \"batch-01\" is in several queues: first, second; run this command again with --queue and one of them\n";

    let waited = in_queue(&sandbox, "mine", &["wait", "batch-01", "--timeout", "0"]);
    let withdrawn = in_queue(&sandbox, "mine", &["withdraw", "batch-01"]);
    let replaced = sandbox
        .asqr()
        .args(["--queue", "mine", "ask"])
        .arg(&file)
        .args(["--replaces", "batch-01"])
        .output()
        .expect("runs");

    assert_eq!(
        (waited.status.code(), stderr(&waited)),
        (Some(1), several.to_owned())
    );
    assert_eq!(
        (withdrawn.status.code(), stderr(&withdrawn)),
        (Some(1), several.to_owned())
    );
    assert_eq!(
        (replaced.status.code(), stderr(&replaced)),
        (Some(1), several.to_owned())
    );
}
