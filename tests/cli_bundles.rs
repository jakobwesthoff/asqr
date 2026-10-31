// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `result`, `wait`, `status`, `withdraw` and `queues` read the month
//! bundles of the archive (spec sections 5.1, 6.4 and 6.5).

mod common;

use std::time::{Duration, SystemTime};

use asqr::format::{SessionResult, rfc3339};
use asqr::queue::{
    Archived, EntryKind, EntryName, Month, NewEntry, QueueLocation, add_entries, archive_name,
    finish, result_entry_name, session_sha256,
};
use common::{Sandbox, stderr, stdout};
use ulid::Ulid;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn ago(days: u64) -> SystemTime {
    SystemTime::now() - DAY * u32::try_from(days).expect("a few days")
}

fn location(sandbox: &Sandbox) -> QueueLocation {
    let location = QueueLocation::at(sandbox.queue_dir());
    location.create_layout().expect("layout");
    location
}

fn text(time: SystemTime) -> String {
    let zoned = jiff::Timestamp::try_from(time)
        .expect("a timestamp")
        .to_zoned(jiff::tz::TimeZone::system());
    rfc3339(&zoned)
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64
}

/// A submitted result of session `id`, finished at `when`, as the file
/// holds it.
fn result_bytes(id: &str, when: SystemTime) -> Vec<u8> {
    let result =
        SessionResult::submitted(id, "0".repeat(64).as_str(), &text(when), None, Vec::new());
    serde_json::to_vec_pretty(&result).expect("serialises")
}

/// Puts one entry into the bundle of its month, bundled at `bundled`.
fn bundle(location: &QueueLocation, name: EntryName, bytes: Vec<u8>, bundled: SystemTime) {
    let month = Month::of(name.ulid);
    add_entries(location, month, &[NewEntry::named(name, bytes)], bundled).expect("bundled");
}

/// The result of `id`, finished `age_days` ago, bundled `bundled_days`
/// ago; its bytes.
fn bundle_result(location: &QueueLocation, id: &str, age_days: u64, bundled_days: u64) -> Vec<u8> {
    let finished = ago(age_days);
    let bytes = result_bytes(id, finished);
    // `submitted_at` has whole seconds, and the entry name must carry the
    // time the text does.
    let name = result_entry_name(id, &bytes, whole_seconds(finished));
    bundle(location, name, bytes.clone(), ago(bundled_days));
    bytes
}

fn whole_seconds(time: SystemTime) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(millis(time) / 1000)
}

fn bundle_session(location: &QueueLocation, id: &str, age_days: u64, bundled_days: u64) {
    let name = EntryName {
        kind: EntryKind::Session,
        id: id.to_owned(),
        ulid: Ulid::from_parts(millis(ago(age_days)), 1),
    };
    bundle(location, name, b"{}".to_vec(), ago(bundled_days));
}

fn ask(sandbox: &Sandbox, id: &str) {
    let file = sandbox.file(
        &format!("{id}.json"),
        &format!(r#"{{"asqr": 2, "id": "{id}", "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#),
    );
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
}

fn answer(sandbox: &Sandbox, id: &str) {
    let path = sandbox.queue_dir().join("inbox").join(format!("{id}.json"));
    let sha = session_sha256(&std::fs::read(&path).expect("session is waiting"));
    let result = SessionResult::submitted(id, &sha, &text(SystemTime::now()), None, Vec::new());
    finish(&QueueLocation::at(sandbox.queue_dir()), &path, &result).expect("finished");
}

fn run(sandbox: &Sandbox, args: &[&str]) -> std::process::Output {
    sandbox.asqr_in_queue(args).output().expect("runs")
}

// -------------------------------------------------------------------------
// result and wait
// -------------------------------------------------------------------------

#[test]
fn result_prints_a_bundled_result_byte_for_byte() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    bundle_session(&location, "done", 40, 10);
    let bytes = bundle_result(&location, "done", 40, 10);

    let output = run(&sandbox, &["result", "DONE"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(output.stdout, bytes);
}

#[test]
fn wait_prints_a_bundled_result() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    bundle_session(&location, "done", 40, 10);
    let bytes = bundle_result(&location, "done", 40, 10);

    let output = run(&sandbox, &["wait", "done", "--timeout", "5"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(output.stdout, bytes);
}

#[test]
fn a_waiting_session_hides_the_bundled_result_of_the_same_id() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    bundle_session(&location, "done", 40, 10);
    bundle_result(&location, "done", 40, 10);
    ask(&sandbox, "done");

    let output = run(&sandbox, &["result", "done"]);

    assert_eq!(
        output.status.code(),
        Some(12),
        "it waits for the new session"
    );
}

#[test]
fn a_result_moved_aside_by_force_answers_nothing() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    let bytes = result_bytes("done", ago(40));
    let name = EntryName {
        kind: EntryKind::Replaced,
        id: "done".to_owned(),
        ulid: Ulid::from_parts(millis(ago(40)), 1),
    };
    bundle(&location, name, bytes, ago(10));

    let output = run(&sandbox, &["result", "done"]);

    assert_eq!(output.status.code(), Some(13));
    assert!(stderr(&output).contains("is gone"), "{}", stderr(&output));
}

#[test]
fn a_bundled_session_without_a_result_is_gone() {
    let sandbox = Sandbox::new();
    bundle_session(&location(&sandbox), "done", 40, 10);

    let output = run(&sandbox, &["result", "done"]);

    assert_eq!(output.status.code(), Some(13));
    assert!(
        stderr(&output).contains("the result of done is gone"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_result_deleted_by_hand_after_a_newer_session_is_gone() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    bundle_session(&location, "done", 40, 10);
    bundle_result(&location, "done", 40, 10);
    ask(&sandbox, "done");
    answer(&sandbox, "done");
    std::fs::remove_file(sandbox.queue_dir().join("outbox/done.json")).expect("deleted");

    let output = run(&sandbox, &["result", "done"]);

    assert_eq!(output.status.code(), Some(13));
    assert!(stderr(&output).contains("is gone"), "{}", stderr(&output));
}

#[test]
fn a_session_archived_unanswered_beside_an_unread_result_still_shows_that_result_after_bundling() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    let bytes = bundle_result(&location, "done", 40, 10);
    // Archived without a result while the earlier result was unread.
    let name = archive_name(
        "done",
        Ulid::from_parts(millis(ago(20)), 1),
        Archived::Session,
    );
    std::fs::write(location.archive().join(name), "{}").expect("written");

    let output = run(&sandbox, &["result", "done"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(output.stdout, bytes);
}

#[test]
fn an_id_found_only_in_a_bundle_of_another_queue_is_used_from_there() {
    let sandbox = Sandbox::new();
    let output = sandbox
        .asqr()
        .args(["--queue", "other", "paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    let other = QueueLocation::at(paths["queue"].as_str().expect("a queue"));
    other.create_layout().expect("layout");
    bundle_session(&other, "done", 40, 10);
    let bytes = bundle_result(&other, "done", 40, 10);

    let output = sandbox
        .asqr()
        .args(["--queue", "mine", "result", "done"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(output.stdout, bytes);
    assert!(
        stderr(&output).contains("using queue other"),
        "{}",
        stderr(&output)
    );
}

// -------------------------------------------------------------------------
// status
// -------------------------------------------------------------------------

fn answered(output: &std::process::Output) -> (Vec<String>, u64) {
    let status: serde_json::Value = serde_json::from_str(&stdout(output)).expect("JSON");
    let ids = status["answered"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry["id"].as_str().expect("an id").to_owned())
        .collect();
    (ids, status["answered_total"].as_u64().expect("a total"))
}

/// An outbox result `fresh`, finished a day ago, and the bundled
/// results `mid` (40 days) and `old` (50 days).
fn mixed(sandbox: &Sandbox) {
    let location = location(sandbox);
    std::fs::write(
        location.outbox().join("fresh.json"),
        result_bytes("fresh", ago(1)),
    )
    .expect("written");
    bundle_result(&location, "mid", 40, 10);
    bundle_result(&location, "old", 50, 10);
}

#[test]
fn status_lists_bundled_results_among_the_newest() {
    let sandbox = Sandbox::new();
    mixed(&sandbox);

    let output = run(&sandbox, &["status", "--json"]);

    assert_eq!(
        answered(&output),
        (
            vec!["fresh".to_owned(), "mid".to_owned(), "old".to_owned()],
            3
        )
    );
}

#[test]
fn status_limit_and_since_cover_bundled_results() {
    let sandbox = Sandbox::new();
    mixed(&sandbox);

    let limited = run(&sandbox, &["status", "--json", "--limit", "2"]);
    let since = run(&sandbox, &["status", "--json", "--since", "45d"]);

    assert_eq!(
        answered(&limited),
        (vec!["fresh".to_owned(), "mid".to_owned()], 3)
    );
    assert_eq!(
        answered(&since),
        (vec!["fresh".to_owned(), "mid".to_owned()], 2)
    );
}

#[test]
fn a_result_in_the_outbox_and_in_a_bundle_is_listed_once() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    let bytes = bundle_result(&location, "twice", 40, 10);
    std::fs::write(location.outbox().join("twice.json"), bytes).expect("written");

    let output = run(&sandbox, &["status", "--json"]);

    assert_eq!(answered(&output), (vec!["twice".to_owned()], 1));
}

#[test]
fn status_reads_only_the_entries_it_lists() {
    let sandbox = Sandbox::new();
    mixed(&sandbox);
    // Rot the body of the oldest entry; the listing of the newest two
    // never reads it.
    let month = Month::of(Ulid::from_parts(millis(ago(50)), 0));
    let path = asqr::queue::bundle_path(&location(&sandbox), month);
    let mut bytes = std::fs::read(&path).expect("bundle");
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(&path).expect("opens")).expect("zip");
    let name = archive
        .file_names()
        .find(|name| {
            name.as_ref()
                .is_ok_and(|name| name.starts_with("results/old."))
        })
        .expect("the entry")
        .expect("a name")
        .into_owned();
    let start = archive
        .by_name(&name)
        .expect("entry")
        .data_start()
        .expect("started") as usize;
    bytes[start + 2] ^= 0xff;
    std::fs::write(&path, bytes).expect("damaged");

    let output = run(&sandbox, &["status", "--json", "--limit", "2"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        answered(&output),
        (vec!["fresh".to_owned(), "mid".to_owned()], 3)
    );
}

#[test]
fn status_warns_about_an_unreadable_bundle_and_lists_the_rest() {
    let sandbox = Sandbox::new();
    mixed(&sandbox);
    std::fs::write(location(&sandbox).archive().join("2001-01.zip"), "garbage").expect("written");

    let output = run(&sandbox, &["status", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(answered(&output).1, 3);
    assert!(
        stderr(&output).contains("warning: cannot read") && stderr(&output).contains("2001-01.zip"),
        "{}",
        stderr(&output)
    );
}

// -------------------------------------------------------------------------
// withdraw
// -------------------------------------------------------------------------

#[test]
fn withdraw_of_a_bundled_answered_id_says_answered() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    bundle_session(&location, "done", 40, 10);
    bundle_result(&location, "done", 40, 10);

    let output = run(&sandbox, &["withdraw", "done"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("is answered already"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn withdraw_of_a_bundled_withdrawn_id_says_withdrawn_already_and_exits_0() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    bundle_session(&location, "done", 40, 10);
    let finished = ago(40);
    let result = SessionResult::withdrawn("done", &"0".repeat(64), &text(finished), None);
    let bytes = serde_json::to_vec_pretty(&result).expect("serialises");
    let name = result_entry_name("done", &bytes, whole_seconds(finished));
    bundle(&location, name, bytes, ago(10));

    let output = run(&sandbox, &["withdraw", "done"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("done was withdrawn already"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn withdraw_after_a_newer_session_without_a_result_says_gone() {
    let sandbox = Sandbox::new();
    let location = location(&sandbox);
    bundle_session(&location, "done", 40, 10);
    bundle_result(&location, "done", 40, 10);
    let name = archive_name(
        "done",
        Ulid::from_parts(millis(ago(1)), 1),
        Archived::Session,
    );
    std::fs::write(location.archive().join(name), "{}").expect("written");

    let output = run(&sandbox, &["withdraw", "done"]);

    assert_eq!(output.status.code(), Some(13));
    assert!(stderr(&output).contains("is gone"), "{}", stderr(&output));
}

// -------------------------------------------------------------------------
// queues
// -------------------------------------------------------------------------

#[test]
fn queues_counts_bundled_results() {
    let sandbox = Sandbox::new();
    let output = sandbox
        .asqr()
        .args(["--queue", "work", "paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    let work = QueueLocation::at(paths["queue"].as_str().expect("a queue"));
    work.create_layout().expect("layout");
    std::fs::write(
        work.outbox().join("fresh.json"),
        result_bytes("fresh", ago(1)),
    )
    .expect("written");
    bundle_result(&work, "mid", 40, 10);
    bundle_result(&work, "old", 50, 10);

    let output = sandbox
        .asqr()
        .args(["queues", "--json"])
        .output()
        .expect("runs");

    let queues: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    let work = queues
        .as_array()
        .expect("a list")
        .iter()
        .find(|queue| queue["queue"] == "work")
        .expect("the work queue");
    assert_eq!(work["answered_total"], 3);
}
