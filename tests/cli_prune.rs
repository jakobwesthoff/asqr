// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr prune` (spec section 5.9).

mod common;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use asqr::queue::{
    Archived, EntryKind, EntryName, Month, NewEntry, QueueLocation, add_entries, archive_name,
    bundle_path,
};
use common::{Sandbox, stderr, stdout};
use ulid::Ulid;

fn queue(sandbox: &Sandbox) -> QueueLocation {
    let location = QueueLocation::at(sandbox.queue_dir());
    location.create_layout().expect("layout");
    location
}

fn ago(days: u64) -> SystemTime {
    SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60)
}

fn ulid_at(time: SystemTime) -> Ulid {
    let millis = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64;
    Ulid::from_parts(millis, 1)
}

/// A session archived loose `days` ago.
fn loose(location: &QueueLocation, id: &str, days: u64) -> PathBuf {
    let path = location
        .archive()
        .join(archive_name(id, ulid_at(ago(days)), Archived::Session));
    std::fs::write(&path, "{}").expect("written");
    path
}

/// The bundle of the month `days` ago, holding one session; its path.
fn bundle(location: &QueueLocation, days: u64) -> PathBuf {
    let ulid = ulid_at(ago(days));
    let month = Month::of(ulid);
    let name = EntryName {
        kind: EntryKind::Session,
        id: "a".to_owned(),
        ulid,
    };
    add_entries(
        location,
        month,
        &[NewEntry {
            name,
            bytes: b"{}".to_vec(),
        }],
        SystemTime::now(),
    )
    .expect("bundled");
    bundle_path(location, month)
}

fn count(sandbox: &Sandbox, dir: &str) -> usize {
    std::fs::read_dir(sandbox.queue_dir().join(dir))
        .expect("readable")
        .count()
}

#[test]
fn without_yes_prune_lists_what_it_would_remove_and_removes_nothing() {
    let sandbox = Sandbox::new();
    let location = queue(&sandbox);
    let entry = loose(&location, "old", 400);
    let zip = bundle(&location, 800);

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "90d"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let printed = stdout(&output);
    assert!(
        printed.contains(&format!("would remove {}\n", entry.display()))
            && printed.contains(&format!("would remove {}\n", zip.display())),
        "{printed}"
    );
    assert!(
        printed.ends_with("nothing was removed; run again with --yes to remove these\n"),
        "{printed}"
    );
    assert!(entry.exists() && zip.exists());
}

#[test]
fn with_yes_prune_removes_and_prints_what_it_removed() {
    let sandbox = Sandbox::new();
    let location = queue(&sandbox);
    let entry = loose(&location, "old", 400);
    let zip = bundle(&location, 800);
    let young = loose(&location, "young", 2);

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "90d", "--yes"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let printed = stdout(&output);
    assert!(
        printed.contains(&format!("removed {}\n", entry.display()))
            && printed.contains(&format!("removed {}\n", zip.display())),
        "{printed}"
    );
    assert!(!entry.exists() && !zip.exists());
    assert!(young.exists());
}

#[test]
fn a_dry_run_with_nothing_to_remove_says_so() {
    let sandbox = Sandbox::new();
    let location = queue(&sandbox);
    loose(&location, "young", 2);

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "30d"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "nothing to remove\n");
}

#[test]
fn removing_nothing_prints_nothing() {
    let sandbox = Sandbox::new();
    let location = queue(&sandbox);
    loose(&location, "young", 2);

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "30d", "--yes"])
        .output()
        .expect("runs");

    assert_eq!(stdout(&output), "");
    assert_eq!(count(&sandbox, "archive"), 1);
}

#[test]
fn the_outbox_is_never_pruned() {
    let sandbox = Sandbox::new();
    let location = queue(&sandbox);
    std::fs::write(location.outbox().join("old.json"), "{}").expect("written");

    sandbox
        .asqr_in_queue(&["prune", "--older-than", "30d", "--yes"])
        .assert()
        .success();

    assert_eq!(count(&sandbox, "outbox"), 1);
}

#[test]
fn results_is_a_usage_error_naming_the_new_form() {
    let output = Sandbox::new()
        .asqr_in_queue(&["prune", "--older-than", "30d", "--results"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("--results is gone")
            && stderr(&output).contains("asqr prune --older-than"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_units_seconds_minutes_and_hours_are_usage_errors() {
    for age in ["30s", "90m", "12h"] {
        let output = Sandbox::new()
            .asqr_in_queue(&["prune", "--older-than", age])
            .output()
            .expect("runs");

        assert_eq!(output.status.code(), Some(2), "{age}");
        assert!(
            stderr(&output).contains("d, w, mo or y"),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn an_age_below_30_days_is_a_usage_error() {
    let output = Sandbox::new()
        .asqr_in_queue(&["prune", "--older-than", "29d"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("at least 30 days"),
        "{}",
        stderr(&output)
    );
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
fn status_since_still_takes_hours_and_minutes() {
    let sandbox = Sandbox::new();
    queue(&sandbox);

    for age in ["2h", "30m", "45s"] {
        sandbox
            .asqr_in_queue(&["status", "--since", age])
            .assert()
            .success();
    }
}

#[test]
fn fails_when_the_archive_cannot_be_read() {
    let sandbox = Sandbox::new();
    queue(&sandbox);
    let archive = sandbox.queue_dir().join("archive");
    std::fs::remove_dir_all(&archive).expect("removed");
    std::fs::write(&archive, "not a directory").expect("written");

    let output = sandbox
        .asqr_in_queue(&["prune", "--older-than", "30d"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
}

/// A named queue that holds nothing, made by a mistyped name; its path.
fn stray_queue(sandbox: &Sandbox) -> String {
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
    stray
}

#[test]
fn a_dry_run_lists_a_named_queue_that_holds_nothing_and_keeps_it() {
    let sandbox = Sandbox::new();
    let stray = stray_queue(&sandbox);

    let output = sandbox
        .asqr()
        .args(["prune", "--older-than", "30d"])
        .output()
        .expect("runs");

    assert!(output.status.success());
    assert!(
        stdout(&output).starts_with(&format!("would remove {stray}\n")),
        "{}",
        stdout(&output)
    );
    assert!(std::path::Path::new(&stray).exists());
}

#[test]
fn removes_a_named_queue_that_holds_nothing_and_prints_it() {
    let sandbox = Sandbox::new();
    let stray = stray_queue(&sandbox);

    let output = sandbox
        .asqr()
        .args(["prune", "--older-than", "30d", "--yes"])
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
