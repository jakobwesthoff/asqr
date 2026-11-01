// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Taking a named queue out of the lists for new work (spec section 5.11,
//! ADR 108). A queue is in use while it carries the `active` marker, which
//! every use writes; removing deletes the marker and nothing else, so the
//! finished sessions stay for `asqr history` and `asqr replay`.

use std::io;

use thiserror::Error;

use super::finish::{ArchivedSession, OutboxState, Recovery, outbox_state, recover_one};
use super::finish::{FinishError, archive_session, finish_steps};
use super::{
    DEFAULT_QUEUE, Gate, Holder, LOCK_ATTEMPTS, LockError, QueueLocation, Step, Waiting,
    load_draft, lock_drafts, lock_queue_for, scan_inbox, session_sha256,
};
use crate::format::{
    Answer, SessionResult, now_rfc3339, parse_session, rejected_answers, validate,
};

/// How a removal ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// The queue is out of the lists now. With `--force`, the sessions that
    /// waited in it are in `rejected`.
    Now { rejected: Vec<Rejection> },
    /// The queue was not in use: no marker, and no session waited.
    Already,
}

/// What `--force` did with a session that waited in the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// The id, spelled as the file name spells it.
    pub id: String,
    pub how: Ended,
    /// Where the session was archived, with the bytes it held, so the files
    /// it showed can be recorded once the locks are let go of.
    pub archived: Option<ArchivedSession>,
}

/// How a waiting session ended under `--force`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Rejected, with the removal as its reason.
    Cancelled,
    /// Its result was in the outbox already; recovery's last steps ran.
    Completed,
    /// An unread result of an earlier session under its id was in the
    /// outbox, so it was archived without a result, as recovery does.
    ArchivedUnanswered,
}

#[derive(Debug, Error)]
pub enum RemoveError {
    #[error("the default queue cannot be removed")]
    Default,

    #[error("no such queue")]
    NotThere,

    #[error("the queue is a symbolic link")]
    Link,

    /// Another process holds the queue lock: an asqr watching the queue,
    /// or a command such as `asqr compact`, whose holder text says so.
    #[error("the queue is held ({})", .0.text)]
    Held(Holder),

    #[error("the queue is busy")]
    Busy,

    /// Sessions wait in the queue, and `--force` was not given.
    #[error("{0} sessions wait in the queue")]
    Waiting(usize),

    /// `--force` stopped at session `id`; the sessions in `rejected` ended
    /// before it. Where `result_written`, the rejected result of `id` is in
    /// the outbox already and the next step failed.
    #[error("cannot reject session {id}: {error}")]
    Stopped {
        id: String,
        error: io::Error,
        rejected: Vec<Rejection>,
        result_written: bool,
    },

    /// The marker could not be deleted, so the queue stays in the lists;
    /// with `--force`, the sessions in `rejected` are rejected already.
    #[error("{error}")]
    Unmarked {
        error: io::Error,
        rejected: Vec<Rejection>,
    },

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// What the lock file says while a removal holds the queue lock.
pub const REMOVAL_PURPOSE: &str = "asqr queues remove";

/// Takes the named queue at `location` out of the lists for new work. With
/// `force`, the sessions waiting in it are rejected first; without it, a
/// queue in which a session waits is refused.
pub fn remove_queue(location: &QueueLocation, force: bool) -> Result<Removed, RemoveError> {
    remove_queue_with(
        location,
        force,
        Gate {
            attempts: LOCK_ATTEMPTS,
            ..Gate::default()
        },
    )
}

/// [`remove_queue`] with the drafts lock taken and observed through
/// `gate`. `Step::Checked` comes once after the look at the inbox, and with
/// `force` once more for each session after the look at its result.
pub fn remove_queue_with(
    location: &QueueLocation,
    force: bool,
    gate: Gate<'_>,
) -> Result<Removed, RemoveError> {
    let Some((name, root)) = location.name().zip(location.root()) else {
        // Only named queues are in the lists; a `--dir` queue is the
        // person's own directory.
        return Err(RemoveError::NotThere);
    };
    // The name as given, before anything else: `same_queue` and every open
    // follow a link, and a link's name must never stand for the queue it
    // leads to, the default queue included.
    match std::fs::symlink_metadata(location.dir()) {
        Ok(metadata) if metadata.is_symlink() => return Err(RemoveError::Link),
        Ok(metadata) if metadata.is_dir() => {}
        // The default queue is listed before its first use, when it has no
        // directory yet.
        _ if location.is_default() => return Err(RemoveError::Default),
        Ok(_) => return Err(RemoveError::NotThere),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(RemoveError::NotThere);
        }
        Err(error) => return Err(error.into()),
    }
    if location.is_default() || location.same_queue(&QueueLocation::named(root, DEFAULT_QUEUE)) {
        return Err(RemoveError::Default);
    }

    // Both locks are held from the look at the inbox to the deletion of the
    // marker, so an `ask` (drafts lock) or an asqr that starts on the queue
    // (queue lock) comes either before the look and is seen, or after the
    // deletion and writes the marker again. Taking a lock makes the queue's
    // directory where `prune` removed it in between; it then holds nothing
    // and is answered as removed already.
    let _queue = match lock_queue_for(location, LOCK_ATTEMPTS, REMOVAL_PURPOSE) {
        Ok(lock) => lock,
        Err(LockError::Held(holder)) => return Err(RemoveError::Held(holder)),
        Err(LockError::Io(error)) => return Err(error.into()),
    };
    let Gate { attempts, mut step } = gate;
    let _drafts = match lock_drafts(location, attempts) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Err(RemoveError::Busy),
        Err(error) => return Err(error.into()),
    };
    let waiting = scan_inbox(location)?.sessions;
    step(Step::Checked);
    if !waiting.is_empty() && !force {
        return Err(RemoveError::Waiting(waiting.len()));
    }
    if waiting.is_empty() && !location.active_marker().exists() {
        return Ok(Removed::Already);
    }

    let reason = format!("the person removed queue {name} while this session waited in it");
    let mut rejected = Vec::new();
    for session in &waiting {
        // A failure after the rejected result was written leaves that
        // result in the outbox, which the error says.
        let mut result_written = false;
        let mut watched = |done: Step| {
            result_written |= done == Step::ResultWritten;
            step(done);
        };
        match reject_waiting(location, session, &reason, &mut watched) {
            Ok(Some(rejection)) => rejected.push(rejection),
            // It left the inbox since the look; whoever took it decides.
            Ok(None) => {}
            Err(error) => {
                return Err(RemoveError::Stopped {
                    id: session.id.clone(),
                    error,
                    rejected,
                    result_written,
                });
            }
        }
    }
    // The sessions rejected so far stay rejected either way, so the error
    // carries them for their record and their names.
    match std::fs::remove_file(location.active_marker()) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(RemoveError::Unmarked { error, rejected });
        }
        _ => {}
    }
    tracing::info!(
        queue = name,
        forced = force,
        rejected = rejected.len(),
        "removed the queue from the lists"
    );
    Ok(Removed::Now { rejected })
}

/// Ends the waiting session `waiting` for a removal with `--force`, under
/// the drafts lock the caller holds: as recovery does where the outbox holds
/// a result under its id, otherwise as rejected with `reason`. `None` when
/// the file left the inbox meanwhile.
fn reject_waiting(
    location: &QueueLocation,
    waiting: &Waiting,
    reason: &str,
    step: &mut dyn FnMut(Step),
) -> io::Result<Option<Rejection>> {
    let bytes = match std::fs::read(&waiting.path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let state = outbox_state(location, &waiting.id, &bytes)?;
    step(Step::Checked);
    if state != OutboxState::NoResult {
        let rejection = match recover_one(location, waiting, bytes, state, step)? {
            Recovery::Completed { id, archived } => Rejection {
                id,
                how: Ended::Completed,
                archived,
            },
            Recovery::Conflict(id) => Rejection {
                id,
                how: Ended::ArchivedUnanswered,
                archived: None,
            },
        };
        return Ok(Some(rejection));
    }

    let (note, answers) = draft_parts(location, &waiting.id, &bytes);
    let result = SessionResult::cancelled(
        &waiting.id,
        &session_sha256(&bytes),
        &now_rfc3339(),
        Some(reason.to_owned()),
        note,
        answers,
    );
    match finish_steps(location, &waiting.path, &result, step) {
        Ok(archived) => Ok(Some(Rejection {
            id: waiting.id.clone(),
            how: Ended::Cancelled,
            archived: archived.map(|path| ArchivedSession { path, bytes }),
        })),
        // A result landed after the look, from a writer that takes no lock:
        // it is the result of an earlier session to this removal, as to
        // recovery, and stays with the draft.
        Err(FinishError::Conflict(_)) => {
            archive_session(location, &waiting.path, &waiting.id)?;
            step(Step::Archived);
            Ok(Some(Rejection {
                id: waiting.id.clone(),
                how: Ended::ArchivedUnanswered,
                archived: None,
            }))
        }
        Err(FinishError::Io(error)) => Err(error),
    }
}

/// The note and the answers a rejected result carries for the session file
/// `bytes` of `id`: those of its draft, as a rejection in the TUI carries
/// them, an untouched default included (spec section 4.1, ADR 53). A file
/// that is no valid session, or a session without a draft, gives none: the
/// answers can only be read against the questions, and without a draft the
/// person never saw the session.
fn draft_parts(location: &QueueLocation, id: &str, bytes: &[u8]) -> (Option<String>, Vec<Answer>) {
    let Ok((_, session)) = parse_session(bytes) else {
        return (None, Vec::new());
    };
    if validate(&session, Some(id)).is_err() {
        return (None, Vec::new());
    }
    let Some(draft) = load_draft(location, id).ok().flatten() else {
        return (None, Vec::new());
    };
    let note = draft
        .note
        .as_deref()
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .map(str::to_owned);
    (note, rejected_answers(&session, &draft.answers))
}

#[cfg(test)]
mod tests;
