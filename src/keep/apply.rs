// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Keeping the files of a session that is finished and archived (ADR 100).
//! The archived file has two states, plain and recorded, and goes from one
//! to the other by one atomic rename, so no step can lose the session.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::KeepFiles;
use super::record::record;
use super::snapshot::{Hold, Limits, MemoryBudget, Snapshot, snapshot, snapshot_with};
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
/// names and the recording replaces the archived file by one rename.
///
/// The session itself is never at risk: until the rename it stays plain.
/// A failing step removes the copies this job placed. A process that exits
/// between placing the copies and the rename cannot, and leaves them under
/// their names without a recording that refers to them. Those orphans stay
/// until `prune` ages them out. The placing comes first so that a recording
/// never refers to a copy that is not there yet.
pub fn keep(job: KeepJob, between: &dyn Fn()) -> io::Result<Kept> {
    keep_with(job, between, &write_atomically)
}

/// [`keep`] with the step that writes the recording over the archived
/// session given, so a test can make it fail after the copies are placed.
fn keep_with(
    job: KeepJob,
    between: &dyn Fn(),
    write_recording: &dyn Fn(&Path, &[u8]) -> io::Result<()>,
) -> io::Result<Kept> {
    let KeepJob {
        location,
        archived,
        snapshot,
        mode,
        lock_attempts,
    } = job;
    let files_dir = location.kept_files();
    let mut staged = Vec::new();
    let recording = record(
        &archived.bytes,
        snapshot,
        mode,
        &Limits::VIEWERS,
        &mut |copy| {
            staged.push((copy.name.clone(), stage(&files_dir, &copy.bytes)?));
            Ok(())
        },
    )?;
    if recording.copies == 0 && recording.json == archived.bytes {
        return Ok(Kept::NothingToRecord);
    }
    between();

    let _lock = match lock_drafts(&location, lock_attempts) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            return Ok(Kept::Skipped("the drafts lock is busy"));
        }
        Err(error) => return Err(error),
    };
    match fs::read(&archived.path) {
        Ok(now) if now == archived.bytes => {}
        Ok(_) => return Ok(Kept::Skipped(CHANGED)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Kept::Skipped(CHANGED));
        }
        Err(error) => return Err(error),
    }
    // The copies this job puts under a name that was free are removed again
    // if a later step fails, still under the lock: a loose kept file that no
    // recording refers to would be an orphan until a prune ages it out. A
    // name that was taken before is another session's and stays.
    let mut created: Vec<PathBuf> = Vec::new();
    let placed = (|| {
        for (name, copy) in staged {
            let target = files_dir.join(&name);
            let free = !target.exists();
            place(&files_dir, &name, copy)?;
            if free {
                created.push(target);
            }
        }
        write_recording(&archived.path, &recording.json)
    })();
    if let Err(error) = placed {
        for target in &created {
            if let Err(removal) = fs::remove_file(target) {
                tracing::warn!(%removal, path = %target.display(), "could not remove a copy of a failed job");
            }
        }
        return Err(error);
    }
    Ok(Kept::Recorded)
}

const CHANGED: &str = "the archived session changed or is gone";

/// The snapshot of the inbox file at `path`, which the person saw with the
/// hash `sha256`. A file that holds other bytes by now, or cannot be read,
/// gives an empty snapshot: the finish will not go through for it either.
pub fn snapshot_seen(
    path: &Path,
    sha256: &str,
    limits: &Limits,
    hold: Hold,
    budget: &MemoryBudget,
) -> Snapshot {
    match fs::read(path) {
        Ok(bytes) if session_sha256(&bytes) == sha256 => snapshot(&bytes, limits, hold, budget),
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
    let snapshot = snapshot_with(
        &archived.bytes,
        &Limits::VIEWERS,
        Hold::Fingerprint,
        &MemoryBudget::new(0),
        reading,
    );
    keep(
        KeepJob::new(location.clone(), archived, snapshot, KeepFiles::Off),
        &|| {},
    )
}

#[cfg(test)]
mod tests;
