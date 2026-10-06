// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr wait`, `asqr result` and `asqr status` (spec section 8).

mod common;

use std::time::{Duration, Instant};

use asqr::format::{Answer, SessionResult, now_rfc3339, rfc3339};
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

    // The exit says the status, the same way `wait` does.
    assert_eq!(output.status.code(), Some(10));
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
fn result_of_a_session_still_waiting_is_the_timeout_code() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "pending");

    let output = sandbox
        .asqr_in_queue(&["result", "pending"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(12));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("no result for pending yet"),
        "{}",
        stderr(&output)
    );
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
fn an_archived_session_whose_result_is_gone_ends_the_wait() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "gone");
    answer(&sandbox, "gone", |sha| {
        SessionResult::submitted("gone", sha, &now_rfc3339(), Vec::new())
    });
    std::fs::remove_file(sandbox.queue_dir().join("outbox/gone.json")).expect("pruned");

    for command in ["wait", "result"] {
        let output = sandbox
            .asqr_in_queue(&[command, "gone"])
            .timeout(std::time::Duration::from_secs(10))
            .output()
            .expect("runs");

        assert_eq!(output.status.code(), Some(13), "{command}");
        assert!(
            stderr(&output).contains("result of gone is gone"),
            "{command}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn a_session_taken_out_of_the_inbox_ends_the_wait() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "withdrawn");
    let mut waiting = sandbox
        .spawnable_asqr()
        .args(["--dir"])
        .arg(sandbox.queue_dir())
        .args(["wait", "withdrawn"])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("wait starts");

    std::thread::sleep(std::time::Duration::from_millis(300));
    std::fs::remove_file(sandbox.queue_dir().join("inbox/withdrawn.json")).expect("removed");

    let output = wait_with_deadline(&mut waiting);
    assert_eq!(output.status.code(), Some(13));
}

/// Waits for `child`, killing it when it runs over, so a regression fails
/// the test instead of hanging it.
fn wait_with_deadline(child: &mut std::process::Child) -> std::process::Output {
    let started = std::time::Instant::now();
    while child.try_wait().expect("status").is_none() {
        if started.elapsed() > std::time::Duration::from_secs(10) {
            child.kill().expect("killed");
            panic!("the wait never ended");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let mut stderr = String::new();
    std::io::Read::read_to_string(child.stderr.as_mut().expect("piped"), &mut stderr)
        .expect("stderr");
    std::process::Output {
        status: child.wait().expect("status"),
        stdout: Vec::new(),
        stderr: stderr.into_bytes(),
    }
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

/// Writes a result for `id` straight into the outbox, as if the session was
/// finished at `submitted_at`. `None` leaves the field out.
fn result_finished_at(sandbox: &Sandbox, id: &str, submitted_at: Option<&str>) {
    location(sandbox).create_layout().expect("layout");
    let result = SessionResult {
        submitted_at: submitted_at.map(str::to_owned),
        ..SessionResult::submitted(id, "sha", "", Vec::new())
    };
    std::fs::write(
        sandbox
            .queue_dir()
            .join("outbox")
            .join(format!("{id}.json")),
        serde_json::to_vec(&result).expect("serializes"),
    )
    .expect("written");
}

/// Sets the modification time of the outbox file `name`.
fn modified_at(sandbox: &Sandbox, name: &str, time: &str) {
    let time: jiff::Timestamp = time.parse().expect("valid time");
    std::fs::File::options()
        .write(true)
        .open(sandbox.queue_dir().join("outbox").join(name))
        .expect("opens")
        .set_modified(time.into())
        .expect("mtime set");
}

/// The current time minus `minutes`, in the form results use.
fn minutes_ago(minutes: i64) -> String {
    let then = jiff::Zoned::now()
        .checked_sub(jiff::SignedDuration::from_mins(minutes))
        .expect("in range");
    rfc3339(&then)
}

fn status_json(sandbox: &Sandbox, args: &[&str]) -> serde_json::Value {
    let output = sandbox
        .asqr_in_queue(&[&["status", "--json"], args].concat())
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    serde_json::from_str(&stdout(&output)).expect("status JSON")
}

fn status_text(sandbox: &Sandbox, args: &[&str]) -> String {
    stdout(
        &sandbox
            .asqr_in_queue(&[&["status"], args].concat())
            .output()
            .expect("runs"),
    )
}

fn answered_ids(status: &serde_json::Value) -> Vec<String> {
    status["answered"]
        .as_array()
        .expect("answered list")
        .iter()
        .map(|entry| entry["id"].as_str().expect("id").to_owned())
        .collect()
}

/// Twelve results, one per hour. The ids are shuffled against the hours, so
/// neither order of the ids matches the order of finishing.
fn twelve_results(sandbox: &Sandbox) -> Vec<String> {
    let mut newest_first = Vec::new();
    for hour in 0..12 {
        let id = format!("r{:02}", (hour * 5) % 12);
        let at = format!("2026-10-01T{hour:02}:00:00+02:00");
        result_finished_at(sandbox, &id, Some(&at));
        newest_first.insert(0, id);
    }
    newest_first
}

#[test]
fn status_lists_the_ten_newest_results_first() {
    let sandbox = Sandbox::new();
    let newest_first = twelve_results(&sandbox);

    let status = status_json(&sandbox, &[]);
    let text = status_text(&sandbox, &[]);

    assert_eq!(answered_ids(&status), newest_first[..10]);
    assert_eq!(status["answered_total"], 12, "{status}");
    let listed: String = newest_first[..10]
        .iter()
        .map(|id| format!("  {id}  submitted\n"))
        .collect();
    assert!(
        text.ends_with(&format!("answered (10 of 12):\n{listed}")),
        "{text}"
    );
}

#[test]
fn status_limit_and_all_choose_how_many_results_show() {
    let sandbox = Sandbox::new();
    let newest_first = twelve_results(&sandbox);

    let three = status_json(&sandbox, &["--limit", "3"]);
    assert_eq!(answered_ids(&three), newest_first[..3]);
    assert_eq!(three["answered_total"], 12);

    let all = status_json(&sandbox, &["--all"]);
    assert_eq!(answered_ids(&all), newest_first);
    assert!(
        status_text(&sandbox, &["--all"]).contains("answered (12):\n"),
        "an uncut list shows no total"
    );

    let more_than_there_are = status_json(&sandbox, &["--limit", "50"]);
    assert_eq!(answered_ids(&more_than_there_are), newest_first);

    // A limit of 0 leaves only the count.
    let none = status_json(&sandbox, &["--limit", "0"]);
    assert_eq!(answered_ids(&none), Vec::<String>::new());
    assert_eq!(none["answered_total"], 12);
    assert!(
        status_text(&sandbox, &["--limit", "0"]).ends_with("answered (0 of 12):\n"),
        "the text form shows the count too"
    );
}

#[test]
fn status_since_keeps_results_finished_within_the_age() {
    let sandbox = Sandbox::new();
    result_finished_at(&sandbox, "recent", Some(&minutes_ago(30)));
    result_finished_at(&sandbox, "older", Some(&minutes_ago(180)));
    ask(&sandbox, "pending");

    let hour = status_json(&sandbox, &["--since", "1h"]);
    assert_eq!(answered_ids(&hour), ["recent"]);
    assert_eq!(hour["answered_total"], 1, "the total counts what matches");
    assert!(
        status_text(&sandbox, &["--since", "1h"]).ends_with("answered (1):\n  recent  submitted\n")
    );

    let day = status_json(&sandbox, &["--since", "1d", "--limit", "1"]);
    assert_eq!(answered_ids(&day), ["recent"]);
    assert_eq!(day["answered_total"], 2);

    // Waiting sessions are still open, so no age filters them.
    let now = status_json(&sandbox, &["--since", "0s"]);
    assert_eq!(answered_ids(&now), Vec::<String>::new());
    assert_eq!(now["waiting"][0]["id"], "pending", "{now}");
}

#[test]
fn status_orders_by_the_finish_time_and_falls_back_to_the_file_time() {
    let sandbox = Sandbox::new();
    // Instants, not strings, are compared: 10:00 in +02:00 is 08:00 UTC, an
    // hour before 09:00 UTC.
    result_finished_at(&sandbox, "berlin", Some("2026-10-01T10:00:00+02:00"));
    result_finished_at(&sandbox, "utc", Some("2026-10-01T09:00:00+00:00"));
    result_finished_at(&sandbox, "same", Some("2026-10-01T09:00:00Z"));
    // Without a usable finish time, the file's modification time places a
    // result.
    result_finished_at(&sandbox, "undated", None);
    modified_at(&sandbox, "undated.json", "2026-10-01T08:45:00Z");
    result_finished_at(&sandbox, "garbled", Some("yesterday"));
    modified_at(&sandbox, "garbled.json", "2026-10-01T07:00:00Z");
    std::fs::write(sandbox.queue_dir().join("outbox/odd.json"), "[]").expect("written");
    modified_at(&sandbox, "odd.json", "2026-10-01T08:30:00Z");

    let status = status_json(&sandbox, &["--all"]);

    // Equal times fall back to the id, so the order is stable.
    assert_eq!(
        answered_ids(&status),
        ["same", "utc", "undated", "odd", "berlin", "garbled"]
    );
    assert!(status["answered"][3]["status"].is_null(), "{status}");
}

#[test]
fn status_refuses_all_with_a_limit_and_a_malformed_age() {
    let sandbox = Sandbox::new();

    for (args, message) in [
        (
            &["status", "--all", "--limit", "3"][..],
            "cannot be used with",
        ),
        (
            &["status", "--since", "soon"],
            "expected a number and a unit",
        ),
        (&["status", "--limit", "-1"], "unexpected argument '-1'"),
    ] {
        let output = sandbox.asqr_in_queue(args).output().expect("runs");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).contains(message), "{}", stderr(&output));
    }
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
