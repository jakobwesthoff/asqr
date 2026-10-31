// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr history` lists the finished sessions of a queue, loose and
//! bundled, and with `--all` of every queue under the data directory
//! (spec section 6.5).

mod common;

use std::fs;
use std::time::{Duration, SystemTime};

use asqr::format::{SessionResult, rfc3339};
use asqr::queue::{
    Archived, EntryKind, EntryName, Month, NewEntry, QueueLocation, add_entries, archive_name,
    session_sha256,
};
use common::{Sandbox, stderr, stdout};
use ulid::Ulid;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn ago(days: u64) -> SystemTime {
    SystemTime::now() - DAY * u32::try_from(days).expect("a few days")
}

fn ulid_at(time: SystemTime) -> Ulid {
    let millis = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64;
    Ulid::from_parts(millis, 0x00AB_CDEF_0123_4567)
}

fn session(title: &str) -> String {
    format!(r#"{{"asqr": 2, "id": "x", "title": "{title}", "questions": []}}"#)
}

fn text(time: SystemTime) -> String {
    let zoned = jiff::Timestamp::try_from(time)
        .expect("a timestamp")
        .to_zoned(jiff::tz::TimeZone::system());
    rfc3339(&zoned)
}

/// A session archived loose `days` ago and answered by a result in the
/// outbox; its reference.
fn finish_loose(location: &QueueLocation, id: &str, title: &str, days: u64) -> String {
    location.create_layout().expect("layout");
    let body = session(title);
    let ulid = ulid_at(ago(days));
    let name = archive_name(id, ulid, Archived::Session);
    fs::write(location.archive().join(name), &body).expect("written");
    let result = SessionResult::submitted(
        id,
        &session_sha256(body.as_bytes()),
        &text(ago(days)),
        None,
        Vec::new(),
    );
    fs::write(
        location.outbox().join(format!("{id}.json")),
        serde_json::to_vec_pretty(&result).expect("serialises"),
    )
    .expect("written");
    let text = ulid.to_string();
    text[text.len() - 8..].to_owned()
}

/// A session that only a month bundle holds; its reference.
fn finish_bundled(location: &QueueLocation, id: &str, title: &str, days: u64) -> String {
    location.create_layout().expect("layout");
    let ulid = ulid_at(ago(days));
    let name = EntryName {
        kind: EntryKind::Session,
        id: id.to_owned(),
        ulid,
    };
    add_entries(
        location,
        Month::of(ulid),
        &[NewEntry::named(name, session(title).into_bytes())],
        SystemTime::now(),
    )
    .expect("bundled");
    let text = ulid.to_string();
    text[text.len() - 8..].to_owned()
}

#[test]
fn history_lists_loose_and_bundled_sessions_newest_first() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::at(sandbox.queue_dir());
    let loose = finish_loose(&location, "hero-2", "Pick the hero", 1);
    let bundled = finish_bundled(&location, "old-1", "An old question", 60);

    let output = sandbox.asqr_in_queue(&["history"]).output().expect("runs");

    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!("queue: {}", sandbox.queue_dir().display())
    );
    assert_eq!(lines[1], "finished (2):");
    assert!(lines[2].starts_with("REF"), "{out}");
    assert!(
        lines[3].starts_with(&loose)
            && lines[3].contains("sent")
            && lines[3].ends_with("Pick the hero"),
        "{out}"
    );
    assert!(
        lines[4].starts_with(&bundled)
            && lines[4].contains("no result")
            && lines[4].ends_with("An old question"),
        "{out}"
    );
    assert!(
        out.ends_with(&format!(
            "\nOpen one with: asqr --dir {} replay <REF>\n",
            sandbox.queue_dir().display()
        )),
        "{out}"
    );
}

#[test]
fn history_all_covers_the_named_queues_under_the_data_directory_the_queue_in_use_first() {
    let sandbox = Sandbox::new();
    let root = sandbox.data_dir();
    finish_loose(
        &QueueLocation::named(&root, "default"),
        "a-1",
        "In the default",
        1,
    );
    finish_loose(&QueueLocation::named(&root, "work"), "b-1", "In work", 1);

    let output = sandbox
        .asqr()
        .args(["--queue", "work", "history", "--all"])
        .output()
        .expect("runs");

    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    let queues: Vec<&str> = out
        .lines()
        .filter(|line| line.starts_with("queue: "))
        .collect();
    assert_eq!(queues, ["queue: work", "queue: default"]);
    assert!(
        out.contains("In the default") && out.contains("In work"),
        "{out}"
    );
    assert!(out.ends_with("Open one with: asqr replay <REF>\n"), "{out}");
}

#[test]
fn history_limit_and_since_cut_the_list() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::at(sandbox.queue_dir());
    finish_loose(&location, "one", "Newest", 1);
    finish_loose(&location, "two", "Middle", 3);
    finish_loose(&location, "three", "Oldest", 5);

    let limited = sandbox
        .asqr_in_queue(&["history", "--limit", "2"])
        .output()
        .expect("runs");
    let recent = sandbox
        .asqr_in_queue(&["history", "--since", "4d"])
        .output()
        .expect("runs");

    let limited = stdout(&limited);
    assert!(
        limited.contains("finished (2 of 3):") && !limited.contains("Oldest"),
        "{limited}"
    );
    let recent = stdout(&recent);
    assert!(
        recent.contains("finished (2 in 4d):") && !recent.contains("Oldest"),
        "{recent}"
    );
}

#[test]
fn a_limit_of_zero_is_a_usage_error() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr_in_queue(&["history", "--limit", "0"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("the limit must be at least 1"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn history_json_is_one_parseable_object_per_queue() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::at(sandbox.queue_dir());
    let reference = finish_loose(&location, "hero-2", "Pick the hero", 1);

    let single = sandbox
        .asqr_in_queue(&["history", "--json"])
        .output()
        .expect("runs");
    let all = sandbox
        .asqr()
        .args(["history", "--all", "--json"])
        .output()
        .expect("runs");

    let single: serde_json::Value = serde_json::from_str(&stdout(&single)).expect("JSON");
    assert_eq!(single["total"], 1);
    assert_eq!(single["entries"][0]["ref"], reference.as_str());
    assert_eq!(single["entries"][0]["status"], "sent");
    let all: serde_json::Value = serde_json::from_str(&stdout(&all)).expect("JSON");
    assert_eq!(all[0]["queue"], "default");
    assert_eq!(all[0]["entries"], serde_json::json!([]));
}

#[test]
fn an_unreadable_bundle_is_a_warning_and_the_rest_is_listed() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::at(sandbox.queue_dir());
    finish_loose(&location, "hero-2", "Still here", 1);
    fs::write(location.archive().join("2026-01.zip"), "no zip").expect("written");

    let output = sandbox.asqr_in_queue(&["history"]).output().expect("runs");

    assert!(output.status.success());
    assert!(
        stderr(&output).starts_with("warning: cannot read "),
        "{}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("2026-01.zip"));
    assert!(stdout(&output).contains("Still here"));
}
