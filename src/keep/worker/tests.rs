// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use super::*;
use crate::keep::KeepFiles;
use crate::keep::snapshot::Snapshot;
use crate::queue::{ArchivedSession, QueueLocation};

/// A job that names itself by the file name of its archived session. No
/// runner of these tests opens it.
fn job(name: &str) -> KeepJob {
    KeepJob::new(
        QueueLocation::at("/nonexistent/queue"),
        ArchivedSession {
            path: PathBuf::from(name),
            bytes: Vec::new(),
        },
        Snapshot::default(),
        KeepFiles::Compact,
    )
}

/// A runner that records the name of each job it runs.
fn recording(seen: &Arc<Mutex<Vec<String>>>) -> impl Fn(KeepJob) + Send + 'static {
    let seen = Arc::clone(seen);
    move |job| {
        seen.lock()
            .expect("lock")
            .push(job.archived.path.to_string_lossy().into_owned());
    }
}

/// A runner that waits for a message on `release` before it records.
fn gated(
    seen: &Arc<Mutex<Vec<String>>>,
    release: Receiver<()>,
) -> impl Fn(KeepJob) + Send + 'static {
    let record = recording(seen);
    let release = Mutex::new(release);
    move |job| {
        release.lock().expect("lock").recv().expect("released");
        record(job);
    }
}

#[test]
fn jobs_run_one_after_another_in_the_order_they_came() {
    let seen = Arc::default();
    let worker = KeepWorker::new_with(recording(&seen));

    for name in ["one", "two", "three"] {
        worker.submit(job(name));
    }
    worker.idle();

    assert_eq!(*seen.lock().expect("lock"), ["one", "two", "three"]);
}

#[test]
fn a_finish_does_not_wait_for_a_slow_worker() {
    let seen = Arc::default();
    let (release, gate) = mpsc::channel();
    let worker = KeepWorker::new_with(gated(&seen, gate));

    // The runner cannot return before it is released, so `submit` returning
    // at all shows that it does not wait for it.
    worker.submit(job("slow"));
    assert!(seen.lock().expect("lock").is_empty());

    release.send(()).expect("sends");
    worker.idle();
    assert_eq!(*seen.lock().expect("lock"), ["slow"]);
}

#[test]
fn a_panicking_job_does_not_stop_the_worker() {
    let seen = Arc::default();
    let record = recording(&seen);
    let worker = KeepWorker::new_with(move |job| {
        if job.archived.path.ends_with("boom") {
            panic!("a job that fails");
        }
        record(job);
    });

    worker.submit(job("boom"));
    worker.submit(job("after"));
    worker.idle();

    assert_eq!(*seen.lock().expect("lock"), ["after"]);
}

#[test]
fn quitting_waits_for_the_worker_but_not_longer_than_the_limit() {
    let seen = Arc::default();
    let (release, gate) = mpsc::channel();
    let worker = KeepWorker::new_with(gated(&seen, gate));
    worker.submit(job("slow"));

    let waited_out = worker.drain(Duration::ZERO);
    release.send(()).expect("sends");
    let drained = worker.drain(Duration::from_secs(30));

    assert!(!waited_out, "a busy worker is let go of after the limit");
    assert!(drained, "an idle worker is drained");
    assert_eq!(*seen.lock().expect("lock"), ["slow"]);
}

/// A job in the queue at `queue` whose session shows `shown`, archived as
/// `archive/<archived>.json`, which the caller makes or leaves out.
fn job_showing(queue: &std::path::Path, shown: &std::path::Path, archived: &str) -> KeepJob {
    let session = serde_json::to_vec(&serde_json::json!({
        "asqr": 2, "id": archived,
        "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "code", "path": shown}]}]
    }))
    .expect("serializes");
    let seen = crate::keep::snapshot::snapshot(
        &session,
        &crate::keep::snapshot::Limits::VIEWERS,
        crate::keep::snapshot::Hold::Bytes,
        &crate::keep::snapshot::MemoryBudget::new(crate::keep::snapshot::MemoryBudget::RUN),
    );
    KeepJob::new(
        QueueLocation::at(queue),
        ArchivedSession {
            path: queue.join(format!("archive/{archived}.json")),
            bytes: session,
        },
        seen,
        KeepFiles::Compact,
    )
}

/// Makes the job's archived session a file, as the finish does before the
/// job is submitted.
fn archive(job: &KeepJob) {
    std::fs::write(&job.archived.path, &job.archived.bytes).expect("written");
}

/// Whether the archived session at `path` records the files it showed.
fn recorded(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.contains("\"archived\""))
}

fn queue_with_a_shown_file() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let scratch = tempfile::tempdir().expect("temp dir");
    let queue = scratch.path().join("queue");
    QueueLocation::at(&queue).create_layout().expect("layout");
    let shown = scratch.path().join("notes.txt");
    std::fs::write(&shown, "text").expect("written");
    (scratch, queue, shown)
}

#[test]
fn the_real_runner_ends_a_skipped_job_and_takes_the_next() {
    let (scratch, queue, shown) = queue_with_a_shown_file();
    let worker = KeepWorker::new();
    // The archived file of the first job is gone, so its replacement is
    // skipped.
    let skipped = job_showing(&queue, &scratch.path().join("missing.txt"), "gone");
    let next = job_showing(&queue, &shown, "next");
    archive(&next);
    let next_path = next.archived.path.clone();

    worker.submit(skipped);
    worker.submit(next);
    worker.idle();

    assert!(!queue.join("archive/gone.json").exists());
    assert!(
        recorded(&next_path),
        "the second job ran and recorded its file"
    );
}

#[test]
fn the_real_runner_ends_a_failed_job_and_takes_the_next() {
    let (_scratch, queue, shown) = queue_with_a_shown_file();
    let worker = KeepWorker::new();
    // The archived path of the first job is a directory, which cannot be
    // read as a file.
    let failing = job_showing(&queue, &shown, "failing");
    std::fs::create_dir(&failing.archived.path).expect("created");
    let next = job_showing(&queue, &shown, "next");
    archive(&next);
    let next_path = next.archived.path.clone();

    worker.submit(failing);
    worker.submit(next);
    worker.idle();

    assert!(
        recorded(&next_path),
        "the second job ran and recorded its file"
    );
}

#[test]
fn a_default_worker_is_a_working_worker() {
    let worker = KeepWorker::default();

    worker.idle();

    assert!(worker.drain(Duration::from_secs(30)));
}

#[test]
fn clones_share_one_worker() {
    let seen = Arc::default();
    let worker = KeepWorker::new_with(recording(&seen));
    let clone = worker.clone();

    worker.submit(job("one"));
    clone.submit(job("two"));
    clone.idle();

    assert_eq!(*seen.lock().expect("lock"), ["one", "two"]);
}

#[test]
fn pending_counts_the_jobs_handed_over_that_have_not_finished() {
    let seen = Arc::default();
    let (release, gate) = mpsc::channel();
    let worker = KeepWorker::new_with(gated(&seen, gate));
    assert_eq!(worker.pending(), 0);

    worker.submit(job("one"));
    worker.submit(job("two"));
    assert_eq!(worker.pending(), 2);
    release.send(()).expect("sends");
    release.send(()).expect("sends");
    worker.idle();

    assert_eq!(worker.pending(), 0);
    assert_eq!(worker.clone().pending(), 0, "clones count the same jobs");
}

#[test]
fn a_drain_that_is_told_to_stop_ends_while_a_job_runs() {
    let seen = Arc::default();
    let (release, gate) = mpsc::channel();
    let worker = KeepWorker::new_with(gated(&seen, gate));
    worker.submit(job("slow"));

    let started = std::time::Instant::now();
    let drained = worker.drain_unless(Duration::from_secs(30), || true);
    let waited = started.elapsed();
    release.send(()).expect("sends");
    worker.idle();

    assert!(!drained, "stopped before the job ended");
    assert!(
        waited < Duration::from_secs(5),
        "it did not wait for the limit: {waited:?}"
    );
    assert_eq!(
        *seen.lock().expect("lock"),
        ["slow"],
        "the job itself goes on"
    );
}

#[test]
fn a_drain_that_is_not_stopped_waits_for_the_job() {
    let seen = Arc::default();
    let (release, gate) = mpsc::channel();
    let worker = KeepWorker::new_with(gated(&seen, gate));
    worker.submit(job("slow"));
    let asked = std::cell::Cell::new(0);

    let drained = worker.drain_unless(Duration::from_secs(30), || {
        asked.set(asked.get() + 1);
        // The job is released once the drain has asked a few times, so it
        // asks while the job is still running.
        if asked.get() == 3 {
            release.send(()).expect("sends");
        }
        false
    });

    assert!(drained);
    assert!(asked.get() >= 3, "{}", asked.get());
}

#[test]
fn the_quit_notice_names_how_many_sessions_are_still_being_kept() {
    assert_eq!(quit_notice(0), None);
    assert_eq!(
        quit_notice(1).as_deref(),
        Some("keeping the files of 1 finished session, ctrl-c lets go of it...")
    );
    assert_eq!(
        quit_notice(3).as_deref(),
        Some("keeping the files of 3 finished sessions, ctrl-c lets go of them...")
    );
}

#[test]
fn the_gate_is_read_in_a_debug_build_from_a_variable_that_is_not_empty() {
    let value = std::ffi::OsString::from("/tmp/open-sesame");

    assert_eq!(
        gate_from(Some(&value), true),
        Some(PathBuf::from("/tmp/open-sesame"))
    );
    assert_eq!(
        gate_from(Some(&value), false),
        None,
        "a release build has no gate"
    );
    assert_eq!(gate_from(Some(std::ffi::OsStr::new("")), true), None);
    assert_eq!(gate_from(None, true), None);
}

#[test]
fn a_held_job_goes_on_once_the_file_exists_and_not_before() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let gate = scratch.path().join("open");
    let waiting = gate.clone();
    let (polled, looked) = mpsc::channel();
    let (go_on, may_look_again) = mpsc::channel::<()>();
    let waiter = std::thread::spawn(move || {
        wait_for_gate_with(&waiting, Duration::from_secs(30), &|| {
            polled.send(()).expect("sends");
            may_look_again.recv().expect("told to look again");
        })
    });

    // The waiter looked, found no file and has not come back: a gate that
    // opens at once never gets here.
    looked
        .recv_timeout(Duration::from_secs(10))
        .expect("the waiter looked and found the gate shut");
    assert!(!waiter.is_finished(), "the waiter went on without the file");
    std::fs::write(&gate, "").expect("written");
    go_on.send(()).expect("sends");

    assert!(waiter.join().expect("joined"), "the file ended the wait");
}

#[test]
fn a_held_job_gives_up_after_its_patience() {
    let scratch = tempfile::tempdir().expect("temp dir");

    assert!(!wait_for_gate(
        &scratch.path().join("never"),
        Duration::from_millis(100)
    ));
}
