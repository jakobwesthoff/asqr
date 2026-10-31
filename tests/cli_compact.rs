// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr compact` (spec section 5.9).

mod common;

use std::time::{Duration, SystemTime};

use asqr::queue::{Archived, QueueLocation, archive_name, lock_queue};
use common::{Sandbox, stderr, stdout};
use ulid::Ulid;

/// A session archived loose `days` ago in `location`; its path.
fn loose(location: &QueueLocation, id: &str, days: u64) -> std::path::PathBuf {
    let when = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    let millis = when
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64;
    let path = location.archive().join(archive_name(
        id,
        Ulid::from_parts(millis, 1),
        Archived::Session,
    ));
    std::fs::write(&path, "{}").expect("written");
    path
}

fn named(sandbox: &Sandbox, name: &str) -> QueueLocation {
    let output = sandbox
        .asqr()
        .args(["--queue", name, "paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    let location = QueueLocation::at(paths["queue"].as_str().expect("a queue"));
    location.create_layout().expect("layout");
    location
}

fn directory(sandbox: &Sandbox) -> QueueLocation {
    let location = QueueLocation::at(sandbox.queue_dir());
    location.create_layout().expect("layout");
    location
}

#[test]
fn compact_prints_what_it_bundled() {
    let sandbox = Sandbox::new();
    let location = directory(&sandbox);
    let old = loose(&location, "old", 40);
    loose(&location, "older", 40);
    let young = loose(&location, "young", 2);

    let output = sandbox.asqr_in_queue(&["compact"]).output().expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let printed = stdout(&output);
    assert!(printed.contains(".zip: 2 sessions\n"), "{printed}");
    assert!(!old.exists());
    assert!(young.exists());
}

#[test]
fn compact_with_nothing_due_says_so() {
    let sandbox = Sandbox::new();
    let location = directory(&sandbox);
    loose(&location, "young", 2);

    let output = sandbox.asqr_in_queue(&["compact"]).output().expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "nothing to compact\n");
}

#[test]
fn compact_on_a_watched_queue_skips_it_and_fails() {
    let sandbox = Sandbox::new();
    let location = directory(&sandbox);
    let old = loose(&location, "old", 40);
    let _watching = lock_queue(&location, None).expect("locked");

    let output = sandbox.asqr_in_queue(&["compact"]).output().expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("another asqr watches this queue"),
        "{}",
        stderr(&output)
    );
    assert!(old.exists());
}

#[test]
fn compact_on_a_missing_queue_creates_nothing() {
    let sandbox = Sandbox::new();

    let output = sandbox.asqr_in_queue(&["compact"]).output().expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "nothing to compact\n");
    assert!(!sandbox.queue_dir().exists());
}

#[test]
fn compact_all_covers_every_named_queue_and_skips_watched_ones() {
    let sandbox = Sandbox::new();
    let work = named(&sandbox, "work");
    let busy = named(&sandbox, "busy");
    let in_work = loose(&work, "old", 40);
    let in_busy = loose(&busy, "old", 40);
    let _watching = lock_queue(&busy, None).expect("locked");

    let output = sandbox
        .asqr()
        .args(["compact", "--all"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("work: "), "{}", stdout(&output));
    assert!(
        stderr(&output).contains("busy: skipped: another asqr watches it"),
        "{}",
        stderr(&output)
    );
    assert!(!in_work.exists());
    assert!(in_busy.exists());
}

#[test]
fn compact_all_does_not_create_the_unused_default_queue() {
    let sandbox = Sandbox::new();
    let work = named(&sandbox, "work");
    let default = sandbox
        .asqr()
        .args(["--queue", "default", "paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&default)).expect("JSON");
    let default = std::path::PathBuf::from(paths["queue"].as_str().expect("a queue"));
    loose(&work, "old", 40);

    let output = sandbox
        .asqr()
        .args(["compact", "--all"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(!default.exists(), "{}", default.display());
}

/// The month bundle name of an entry loose for `days` days.
fn month_of(days: u64) -> String {
    let when = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    let millis = when
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64;
    asqr::queue::Month::of(Ulid::from_parts(millis, 1)).to_string()
}

#[test]
fn compact_names_a_result_that_waits_for_recovery() {
    let sandbox = Sandbox::new();
    let location = directory(&sandbox);
    let result = location.outbox().join("done.json");
    std::fs::write(&result, "{}").expect("written");
    std::fs::File::options()
        .write(true)
        .open(&result)
        .expect("opens")
        .set_modified(SystemTime::now() - Duration::from_secs(40 * 24 * 60 * 60))
        .expect("mtime set");
    std::fs::write(location.inbox().join("done.json"), "{}").expect("written");

    let output = sandbox.asqr_in_queue(&["compact"]).output().expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        stderr(&output).contains("left the result of done in the outbox"),
        "{}",
        stderr(&output)
    );
    assert!(result.exists());
}

#[test]
fn compact_skips_a_month_whose_bundle_cannot_be_read_and_fails() {
    let sandbox = Sandbox::new();
    let location = directory(&sandbox);
    let old = loose(&location, "old", 40);
    let month = month_of(40);
    std::fs::write(location.archive().join(format!("{month}.zip")), "garbage").expect("written");

    let output = sandbox.asqr_in_queue(&["compact"]).output().expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains(&format!("skipped {month}:")),
        "{}",
        stderr(&output)
    );
    assert!(old.exists());
}

#[cfg(unix)]
#[test]
fn compact_fails_when_the_archive_cannot_be_listed() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let location = directory(&sandbox);
    std::fs::set_permissions(location.archive(), std::fs::Permissions::from_mode(0o000))
        .expect("permissions change");

    let output = sandbox.asqr_in_queue(&["compact"]).output().expect("runs");

    std::fs::set_permissions(location.archive(), std::fs::Permissions::from_mode(0o755))
        .expect("permissions back");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("skipped: cannot list the queue"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn compact_fails_when_the_queue_cannot_be_locked() {
    let sandbox = Sandbox::new();
    let location = directory(&sandbox);
    // A directory where the lock file belongs.
    std::fs::create_dir(location.lock_file()).expect("created");

    let output = sandbox.asqr_in_queue(&["compact"]).output().expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("error: cannot compact the queue"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn compact_all_with_nothing_due_says_so() {
    let sandbox = Sandbox::new();
    let work = named(&sandbox, "work");
    loose(&work, "young", 2);

    let output = sandbox
        .asqr()
        .args(["compact", "--all"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "nothing to compact\n");
}

#[test]
fn compact_all_fails_when_a_queue_has_a_bundle_it_cannot_read() {
    let sandbox = Sandbox::new();
    let work = named(&sandbox, "work");
    loose(&work, "old", 40);
    let month = month_of(40);
    std::fs::write(work.archive().join(format!("{month}.zip")), "garbage").expect("written");

    let output = sandbox
        .asqr()
        .args(["compact", "--all"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains(&format!("work: skipped {month}:")),
        "{}",
        stderr(&output)
    );
}

#[test]
fn compact_all_fails_when_a_queue_cannot_be_locked() {
    let sandbox = Sandbox::new();
    let work = named(&sandbox, "work");
    std::fs::create_dir(work.lock_file()).expect("created");

    let output = sandbox
        .asqr()
        .args(["compact", "--all"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("work: error: cannot compact the queue"),
        "{}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[test]
fn compact_all_fails_when_the_queues_cannot_be_listed() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::new();
    let work = named(&sandbox, "work");
    let queues = work
        .dir()
        .parent()
        .expect("the queues directory")
        .to_owned();
    std::fs::set_permissions(&queues, std::fs::Permissions::from_mode(0o000))
        .expect("permissions change");

    let output = sandbox
        .asqr()
        .args(["compact", "--all"])
        .output()
        .expect("runs");

    std::fs::set_permissions(&queues, std::fs::Permissions::from_mode(0o755))
        .expect("permissions back");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("error: cannot read the queues"),
        "{}",
        stderr(&output)
    );
}
