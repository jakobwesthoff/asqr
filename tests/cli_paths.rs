// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The command line as a whole, and `asqr paths` (spec sections 3.2, 8).

mod common;

use common::{Sandbox, stderr, stdout};

#[test]
fn help_succeeds() {
    Sandbox::new().asqr().arg("--help").assert().success();
}

#[test]
fn an_unknown_flag_is_a_usage_error() {
    Sandbox::new().asqr().arg("--no-such-flag").assert().code(2);
}

#[test]
fn queue_and_dir_together_are_a_usage_error() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr()
        .args(["--queue", "q", "--dir", "/tmp/d", "paths"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("cannot be used together"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_environment_cannot_name_both_either() {
    let output = Sandbox::new()
        .asqr()
        .env("ASQR_QUEUE", "q")
        .env("ASQR_DIR", "/tmp/d")
        .arg("paths")
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn paths_of_a_directory_queue_have_no_root() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();

    let output = sandbox.asqr_in_queue(&["paths"]).output().expect("runs");

    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("root: none\n"), "{text}");
    assert!(
        text.contains(&format!("queue: {}\n", dir.display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("inbox: {}\n", dir.join("inbox").display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("outbox: {}\n", dir.join("outbox").display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("drafts: {}\n", dir.join("drafts").display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("archive: {}\n", dir.join("archive").display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("lock: {}\n", dir.join("lock").display())),
        "{text}"
    );
    assert!(text.contains("log: "), "{text}");
}

#[test]
fn paths_as_json_name_the_default_queue_under_the_sandbox_home() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr()
        .args(["paths", "--json"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("paths are JSON");
    let root = paths["root"].as_str().expect("a named queue has a root");
    assert!(
        root.starts_with(sandbox.home().to_str().expect("UTF-8 home")),
        "{root}"
    );
    assert!(
        paths["queue"]
            .as_str()
            .expect("queue")
            .ends_with("queues/default")
    );
    for key in ["inbox", "outbox", "drafts", "archive", "lock", "log"] {
        assert!(paths[key].is_string(), "{key} missing: {paths}");
    }
    assert!(
        paths["log"]
            .as_str()
            .expect("log")
            .starts_with(sandbox.home().to_str().expect("UTF-8")),
        "the log lives in the sandbox cache: {paths}"
    );
}

#[test]
fn the_environment_picks_a_named_queue() {
    let output = Sandbox::new()
        .asqr()
        .env("ASQR_QUEUE", "mascots")
        .args(["paths", "--json"])
        .output()
        .expect("runs");

    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    assert!(
        paths["queue"]
            .as_str()
            .expect("queue")
            .ends_with("queues/mascots"),
        "{paths}"
    );
}

#[test]
fn an_invalid_queue_name_is_a_usage_error() {
    let output = Sandbox::new()
        .asqr()
        .args(["--queue", "../up", "paths"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("invalid queue name"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_directory_queue_has_a_null_root_in_json() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr_in_queue(&["paths", "--json"])
        .output()
        .expect("runs");

    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    assert!(paths["root"].is_null(), "{paths}");
}
