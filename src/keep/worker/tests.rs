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

/// A job in the queue at `queue` whose session shows `shown`, archived at
/// `archive/gone.json`, which is not there.
fn job_showing(queue: &std::path::Path, shown: &std::path::Path) -> KeepJob {
    let session = serde_json::to_vec(&serde_json::json!({
        "asqr": 2, "id": "gone",
        "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "code", "path": shown}]}]
    }))
    .expect("serializes");
    let seen = crate::keep::snapshot::snapshot(&session, &crate::keep::snapshot::Limits::VIEWERS);
    KeepJob::new(
        QueueLocation::at(queue),
        ArchivedSession {
            path: queue.join("archive/gone.json"),
            bytes: session,
        },
        seen,
        KeepFiles::Compact,
    )
}

#[test]
fn the_real_runner_ends_a_job_that_was_skipped_or_failed_and_takes_the_next() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let queue = scratch.path().join("queue");
    QueueLocation::at(&queue).create_layout().expect("layout");
    let shown = scratch.path().join("notes.txt");
    std::fs::write(&shown, "text").expect("written");
    let worker = KeepWorker::new();

    // The archived file is gone, so the replacement is skipped. A file
    // where the directory of the copies belongs makes the next job fail.
    worker.submit(job_showing(&queue, &scratch.path().join("missing.txt")));
    std::fs::write(queue.join("archive/files"), "in the way").expect("written");
    worker.submit(job_showing(&queue, &shown));
    worker.idle();

    assert!(!queue.join("archive/gone.json").exists());
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
