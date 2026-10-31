// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Keeping the files of a session that is finished and archived (ADR 100).
//! The archived file has two states, plain and recorded, and goes from one
//! to the other by one atomic rename, so no step can lose the session.

use std::fs;
use std::io;
use std::path::Path;

use super::KeepFiles;
use super::record::record;
use super::snapshot::{Limits, Snapshot, snapshot, snapshot_with};
use super::store::{place, stage};
use crate::queue::{
    ArchivedSession, DRAFTS_LOCK_ATTEMPTS, QueueLocation, lock_drafts, session_sha256,
    write_atomically,
};

/// One finished session whose files are to be kept.
#[derive(Debug)]
pub struct KeepJob {
    /// The queue the session was archived in.
    pub location: QueueLocation,
    pub archived: ArchivedSession,
    /// The files as they were read before the result was written.
    pub snapshot: Snapshot,
    /// The mode in force when the person finished the session.
    pub mode: KeepFiles,
    /// How often the drafts lock is tried before the replacement is
    /// skipped.
    pub lock_attempts: u32,
}

impl KeepJob {
    pub fn new(
        location: QueueLocation,
        archived: ArchivedSession,
        snapshot: Snapshot,
        mode: KeepFiles,
    ) -> Self {
        KeepJob {
            location,
            archived,
            snapshot,
            mode,
            lock_attempts: DRAFTS_LOCK_ATTEMPTS,
        }
    }
}

/// What a job did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// The archived session now records its files.
    Recorded,
    /// The session shows no file by path, so it stays as it is.
    NothingToRecord,
    /// The plain archived session stays, for the reason.
    Skipped(&'static str),
}

/// Keeps the files of `job` and replaces the archived session with the
/// recording. `between` runs after the copies are written and before the
/// drafts lock is taken, so a test can change the queue at that moment.
///
/// The copies are written as temp files first. Under the drafts lock, which
/// the prune and the compaction also take, the archived file must still
/// hold exactly the bytes it was archived with; then the copies take their
/// names and the recording replaces the archived file by one rename. A
/// copy therefore lies under its name only once a recording that refers to
/// it was written, and a failure or a crash before the rename leaves the
/// plain session and, at worst, temp files for the compaction's sweep.
pub fn keep(job: &KeepJob, between: &dyn Fn()) -> io::Result<Kept> {
    let mut snapshot = job.snapshot.clone();
    snapshot.resolve_deferred(&Limits::VIEWERS);
    let recording = record(&job.archived.bytes, &snapshot, job.mode);
    if recording.copies.is_empty() && recording.json == job.archived.bytes {
        return Ok(Kept::NothingToRecord);
    }

    let files_dir = job.location.kept_files();
    let mut staged = Vec::new();
    for copy in &recording.copies {
        staged.push((copy.name.as_str(), stage(&files_dir, &copy.bytes)?));
    }
    between();

    let _lock = match lock_drafts(&job.location, job.lock_attempts) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            return Ok(Kept::Skipped("the drafts lock is busy"));
        }
        Err(error) => return Err(error),
    };
    match fs::read(&job.archived.path) {
        Ok(now) if now == job.archived.bytes => {}
        Ok(_) => return Ok(Kept::Skipped(CHANGED)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Kept::Skipped(CHANGED));
        }
        Err(error) => return Err(error),
    }
    for (name, copy) in staged {
        place(&files_dir, name, copy)?;
    }
    write_atomically(&job.archived.path, &recording.json)?;
    Ok(Kept::Recorded)
}

const CHANGED: &str = "the archived session changed or is gone";

/// The snapshot of the inbox file at `path`, which the person saw with the
/// hash `sha256`. A file that holds other bytes by now, or cannot be read,
/// gives an empty snapshot: the finish will not go through for it either.
pub fn snapshot_seen(path: &Path, sha256: &str, limits: &Limits) -> Snapshot {
    match fs::read(path) {
        Ok(bytes) if session_sha256(&bytes) == sha256 => snapshot(&bytes, limits),
        _ => Snapshot::default(),
    }
}

/// [`record_fingerprints`] for the session a withdrawal archived, if it
/// did. A failure is logged and goes no further: the withdrawal is done and
/// the plain archived session is complete.
pub fn record_after_withdrawal(location: &QueueLocation, archived: Option<ArchivedSession>) {
    let Some(archived) = archived else {
        return;
    };
    if let Err(error) = record_fingerprints(location, archived, &|_| {}) {
        tracing::warn!(%error, "could not record the files a withdrawn session showed");
    }
}

/// Records the fingerprints of the files `archived` showed without keeping
/// copies, for a process that has no worker: the files are read with no
/// lock held, `reading` runs before each, and the replacement takes the
/// drafts lock only for its checks and its rename.
pub fn record_fingerprints(
    location: &QueueLocation,
    archived: ArchivedSession,
    reading: &dyn Fn(&Path),
) -> io::Result<Kept> {
    let snapshot = snapshot_with(&archived.bytes, &Limits::VIEWERS, reading);
    keep(
        &KeepJob::new(location.clone(), archived, snapshot, KeepFiles::Off),
        &|| {},
    )
}

#[cfg(test)]
mod tests;
