// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::*;
use crate::keep::sha256_hex;
use crate::keep::snapshot::{Limits, snapshot};
use crate::keep::test_support::gradient_png;
use crate::queue::lock_drafts;

// =========================================================
// Fixtures
// =========================================================

/// A queue in a temp dir and the plain archived session of a session that
/// showed one file, `name` with `bytes`, as an image.
struct Archived {
    _scratch: tempfile::TempDir,
    location: QueueLocation,
    file: PathBuf,
    session: ArchivedSession,
}

fn archived_showing(name: &str, bytes: &[u8]) -> Archived {
    let scratch = tempfile::tempdir().expect("temp dir");
    let location = QueueLocation::at(scratch.path().join("queue"));
    location.create_layout().expect("layout");
    let shown = scratch.path().join(name);
    fs::write(&shown, bytes).expect("writes");
    let session = serde_json::to_vec_pretty(&json!({
        "asqr": 2, "id": "hero-2",
        "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "image", "path": shown}]}]
    }))
    .expect("serializes");
    let path = location
        .archive()
        .join("hero-2.01J0000000000000000000000A.json");
    fs::write(&path, &session).expect("writes");
    Archived {
        _scratch: scratch,
        location,
        file: shown,
        session: ArchivedSession {
            path,
            bytes: session,
        },
    }
}

impl Archived {
    fn job(&self, mode: KeepFiles) -> KeepJob {
        let snapshot = snapshot(&self.session.bytes, &Limits::VIEWERS);
        let mut job = KeepJob::new(self.location.clone(), self.session.clone(), snapshot, mode);
        job.lock_attempts = 1;
        job
    }

    fn archived_json(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.session.path).expect("reads")).expect("JSON")
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.location.archive().join("files"))
            .map(|entries| {
                entries
                    .map(|entry| {
                        entry
                            .expect("entry")
                            .file_name()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn nothing() {}

// =========================================================
// Replacing the plain session with the recording
// =========================================================

#[test]
fn keeping_replaces_the_plain_session_with_the_recording_and_places_the_copy() {
    let archived = archived_showing("hero.png", &gradient_png());

    let kept = keep(&archived.job(KeepFiles::Compact), &nothing).expect("keeps");

    assert_eq!(kept, Kept::Recorded);
    let json = archived.archived_json();
    let path = json["questions"][0]["context"][0]["path"]
        .as_str()
        .expect("a path");
    assert!(
        path.starts_with("files/") && path.ends_with(".webp"),
        "{path}"
    );
    let name = path.trim_start_matches("files/");
    assert_eq!(archived.files(), [name]);
    let copy = fs::read(archived.location.archive().join("files").join(name)).expect("copy");
    assert_eq!(format!("{}.webp", sha256_hex(&copy)), name);
    assert_eq!(
        json["archived"]["session_sha256"],
        sha256_hex(&archived.session.bytes)
    );
}

#[test]
fn a_session_that_shows_no_file_by_path_stays_as_it_is() {
    let archived = archived_showing("x.png", b"x");
    let plain = serde_json::to_vec(&json!({"asqr": 2, "id": "hero-2", "questions": []}))
        .expect("serializes");
    fs::write(&archived.session.path, &plain).expect("writes");
    let job = KeepJob::new(
        archived.location.clone(),
        ArchivedSession {
            path: archived.session.path.clone(),
            bytes: plain.clone(),
        },
        snapshot(&plain, &Limits::VIEWERS),
        KeepFiles::Compact,
    );

    assert_eq!(keep(&job, &nothing).expect("keeps"), Kept::NothingToRecord);
    assert_eq!(fs::read(&archived.session.path).expect("reads"), plain);
}

#[test]
fn a_file_past_the_memory_cap_is_read_by_the_worker() {
    let archived = archived_showing("hero.png", &gradient_png());
    let limits = Limits {
        memory: 0,
        ..Limits::VIEWERS
    };
    let mut job = archived.job(KeepFiles::Compact);
    job.snapshot = snapshot(&archived.session.bytes, &limits);

    assert_eq!(keep(&job, &nothing).expect("keeps"), Kept::Recorded);
    assert_eq!(
        archived.archived_json()["questions"][0]["context"][0]["shown"]["state"],
        "kept"
    );
}

#[test]
fn off_records_fingerprints_and_places_no_copy() {
    let archived = archived_showing("hero.png", &gradient_png());

    assert_eq!(
        keep(&archived.job(KeepFiles::Off), &nothing).expect("keeps"),
        Kept::Recorded
    );

    assert!(archived.files().is_empty());
    assert_eq!(
        archived.archived_json()["questions"][0]["context"][0]["shown"]["state"],
        "not_kept"
    );
}

#[test]
fn a_reask_with_identical_bytes_gets_its_own_entry() {
    let first = archived_showing("hero.png", &gradient_png());
    let second_path = first
        .location
        .archive()
        .join("hero-2.01J0000000000000000000000B.json");
    fs::write(&second_path, &first.session.bytes).expect("writes");
    let second = ArchivedSession {
        path: second_path,
        bytes: first.session.bytes.clone(),
    };

    keep(&first.job(KeepFiles::Compact), &nothing).expect("keeps");
    let mut job = first.job(KeepFiles::Compact);
    job.archived = second.clone();
    keep(&job, &nothing).expect("keeps");

    let recorded = |path: &PathBuf| -> Value {
        serde_json::from_slice(&fs::read(path).expect("reads")).expect("JSON")
    };
    assert!(recorded(&first.session.path).get("archived").is_some());
    assert!(recorded(&second.path).get("archived").is_some());
    assert_eq!(first.files().len(), 1, "one copy serves both entries");
}

// =========================================================
// What leaves the plain session in place
// =========================================================

#[test]
fn a_replacement_is_refused_when_the_archived_file_changed() {
    let archived = archived_showing("hero.png", &gradient_png());
    let path = archived.session.path.clone();

    let kept = keep(&archived.job(KeepFiles::Compact), &|| {
        fs::write(&path, b"{\"asqr\": 2, \"id\": \"other\"}").expect("changes");
    })
    .expect("keeps");

    assert_eq!(
        kept,
        Kept::Skipped("the archived session changed or is gone")
    );
    assert_eq!(
        fs::read(&path).expect("reads"),
        b"{\"asqr\": 2, \"id\": \"other\"}"
    );
    assert!(archived.files().is_empty(), "no copy is placed");
}

#[test]
fn a_replacement_is_refused_when_the_archived_file_went() {
    let archived = archived_showing("hero.png", &gradient_png());
    let path = archived.session.path.clone();

    let kept = keep(&archived.job(KeepFiles::Compact), &|| {
        fs::remove_file(&path).expect("removes");
    })
    .expect("keeps");

    assert_eq!(
        kept,
        Kept::Skipped("the archived session changed or is gone")
    );
    assert!(!path.exists());
    assert!(archived.files().is_empty());
}

#[test]
fn a_failing_copy_leaves_the_plain_session() {
    let archived = archived_showing("hero.png", &gradient_png());
    // A file where the directory of the copies belongs.
    fs::write(archived.location.archive().join("files"), b"in the way").expect("writes");

    let failed = keep(&archived.job(KeepFiles::Compact), &nothing);

    assert!(failed.is_err());
    assert_eq!(
        fs::read(&archived.session.path).expect("reads"),
        archived.session.bytes
    );
}

#[test]
fn an_archived_path_that_cannot_be_read_is_an_error() {
    let archived = archived_showing("hero.png", &gradient_png());
    let mut job = archived.job(KeepFiles::Off);
    job.archived.path = archived.location.archive();

    assert!(keep(&job, &nothing).is_err());
}

#[test]
fn a_failed_recording_after_a_withdrawal_is_only_logged() {
    let archived = archived_showing("hero.png", &gradient_png());
    let unreadable = ArchivedSession {
        path: archived.location.archive(),
        bytes: archived.session.bytes.clone(),
    };

    record_after_withdrawal(&archived.location, Some(unreadable));
    record_after_withdrawal(&archived.location, None);

    assert_eq!(
        fs::read(&archived.session.path).expect("reads"),
        archived.session.bytes
    );
}

#[test]
fn a_busy_lock_skips_the_replacement() {
    let archived = archived_showing("hero.png", &gradient_png());
    let held = lock_drafts(&archived.location, 1).expect("lock");

    let kept = keep(&archived.job(KeepFiles::Compact), &nothing).expect("keeps");
    drop(held);

    assert_eq!(kept, Kept::Skipped("the drafts lock is busy"));
    assert_eq!(
        fs::read(&archived.session.path).expect("reads"),
        archived.session.bytes
    );
    assert!(archived.files().is_empty());
}

// =========================================================
// Without a worker: fingerprints only
// =========================================================

#[test]
fn fingerprints_are_recorded_without_holding_the_drafts_lock_while_files_are_read() {
    let archived = archived_showing("hero.png", &gradient_png());
    let free_while_reading = Cell::new(false);

    let kept = record_fingerprints(&archived.location, archived.session.clone(), &|_| {
        free_while_reading.set(lock_drafts(&archived.location, 1).is_ok());
    })
    .expect("records");

    assert_eq!(kept, Kept::Recorded);
    assert!(free_while_reading.get(), "the lock was free while reading");
    let json = archived.archived_json();
    let entry = &json["questions"][0]["context"][0];
    assert_eq!(entry["shown"]["state"], "not_kept");
    assert_eq!(entry["path"], archived.file.to_string_lossy().as_ref());
    assert_eq!(json["archived"]["keep"], "off");
    assert!(archived.files().is_empty());
}

#[test]
fn a_recording_that_cannot_be_written_takes_the_copies_it_placed_with_it() {
    let archived = archived_showing("hero.png", &gradient_png());
    let job = archived.job(KeepFiles::Exact);
    // A copy of the same name that was there before the job stays.
    let failed = keep_with(&job, &nothing, &|_, _| {
        Err(io::Error::other("the disk is full"))
    });

    assert!(failed.is_err());
    assert!(archived.files().is_empty(), "{:?}", archived.files());
    assert_eq!(
        fs::read(&archived.session.path).expect("reads"),
        archived.session.bytes
    );
}

#[test]
fn a_copy_that_was_there_before_a_failed_job_stays() {
    let archived = archived_showing("hero.png", &gradient_png());
    keep(&archived.job(KeepFiles::Exact), &nothing).expect("kept once");
    let placed = archived.files();
    // The same file shown by a second session whose recording fails.
    let second = archived_showing("hero.png", &gradient_png());
    fs::create_dir_all(second.location.kept_files()).expect("created");
    for name in &placed {
        fs::copy(
            archived.location.kept_files().join(name),
            second.location.kept_files().join(name),
        )
        .expect("copied");
    }

    let failed = keep_with(&second.job(KeepFiles::Exact), &nothing, &|_, _| {
        Err(io::Error::other("the disk is full"))
    });

    assert!(failed.is_err());
    assert_eq!(
        second.files(),
        placed,
        "what another session's recording refers to stays"
    );
}
