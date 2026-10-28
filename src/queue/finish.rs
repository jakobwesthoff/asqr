// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finishing a session (spec sections 5.7 and 5.8). Submit, cancel and an
//! error result take one path: write the result, archive the session file,
//! delete the draft. Each step can run again safely, and the result's
//! `session_sha256` tells an interrupted finish from a new session that
//! arrived under the same id.

use std::fmt::Write as _;
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};
use thiserror::Error;
use ulid::Ulid;

use super::atomic::write_new_atomically;
use super::{
    Archived, Gate, INVALID_STEM_ID, QueueLocation, Step, archive_name, delete_draft,
    find_session_file, lock_drafts, scan_inbox,
};
use crate::format::SessionResult;

/// The SHA-256 of a session file's bytes, as lowercase hex.
pub fn session_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
            hex
        })
}

#[derive(Debug, Error)]
pub enum FinishError {
    #[error("the outbox holds an unread result of another session {0:?}")]
    Conflict(String),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// How the outbox relates to a waiting session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutboxState {
    NoResult,
    /// The result answers exactly these session bytes.
    ThisSession,
    /// The result belongs to an earlier session under the same id, or
    /// cannot be read; either way it must not be touched.
    OtherSession,
}

fn outbox_state(
    location: &QueueLocation,
    id: &str,
    session_bytes: &[u8],
) -> io::Result<OutboxState> {
    let Some(path) = find_session_file(&location.outbox(), id)? else {
        return Ok(OutboxState::NoResult);
    };
    let result: Option<SessionResult> = serde_json::from_slice(&std::fs::read(path)?).ok();
    let hash = session_sha256(session_bytes);
    Ok(match result {
        Some(result) if result.session_sha256.as_deref() == Some(hash.as_str()) => {
            OutboxState::ThisSession
        }
        _ => OutboxState::OtherSession,
    })
}

/// The id a session file in the inbox stands for: its stem.
pub(super) fn stem(session: &Path) -> &str {
    session
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("inbox sessions are UTF-8 <id>.json names")
}

/// Finishes the session in `session` (an inbox file) with `result`. The
/// caller holds the drafts lock, or runs where no other process changes the
/// queue.
pub fn finish(
    location: &QueueLocation,
    session: &Path,
    result: &SessionResult,
) -> Result<(), FinishError> {
    finish_steps(location, session, result, &mut |_| {})
}

/// [`finish`], calling `step` after each of its steps.
pub(super) fn finish_steps(
    location: &QueueLocation,
    session: &Path,
    result: &SessionResult,
    step: &mut dyn FnMut(Step),
) -> Result<(), FinishError> {
    let id = stem(session);
    let target = location.outbox().join(format!("{id}.json"));
    let json = serde_json::to_vec_pretty(result).map_err(io::Error::other)?;

    match find_session_file(&location.outbox(), id)? {
        None => write_new_atomically(&target, &json)?,
        // Written by an earlier, interrupted run of this same finish.
        Some(existing) if std::fs::read(&existing)? == json => {}
        Some(_) => return Err(FinishError::Conflict(id.to_owned())),
    }
    step(Step::ResultWritten);
    archive_session(location, session, id)?;
    step(Step::Archived);
    delete_draft(location, id)?;
    step(Step::DraftDeleted);
    Ok(())
}

/// Moves a session file into the archive; a file that is gone already was
/// archived by an earlier run.
pub(super) fn archive_session(
    location: &QueueLocation,
    session: &Path,
    id: &str,
) -> io::Result<()> {
    let target = location
        .archive()
        .join(archive_name(id, Ulid::generate(), Archived::Session));
    match std::fs::rename(session, target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// How a finish of a session the person saw ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finished {
    /// The result is written, the session archived and its draft deleted.
    Done,
    /// The session file is gone or holds other bytes than the person saw,
    /// so nothing was written.
    Changed,
}

/// Finishes the session the person saw, whose file had the hash `sha256`,
/// with `result` (spec section 5.7), taking the drafts lock as `gate`
/// says. A result has to answer the bytes the person saw, so a file that is
/// gone or changed since gets none. The look at the file and the finish run
/// under the drafts lock, so no command changes the session in between.
pub fn finish_unchanged_with(
    location: &QueueLocation,
    session: &Path,
    sha256: &str,
    result: &SessionResult,
    mut gate: Gate<'_>,
) -> Result<Finished, FinishError> {
    let _lock = lock_drafts(location, gate.attempts)?;
    // A file that cannot be read counts as changed: the next sync reports
    // what is wrong with it.
    let unchanged = std::fs::read(session).is_ok_and(|bytes| session_sha256(&bytes) == sha256);
    (gate.step)(Step::Checked);
    if !unchanged {
        return Ok(Finished::Changed);
    }
    finish_steps(location, session, result, &mut gate.step)?;
    Ok(Finished::Done)
}

/// What recovery did with a waiting session that already had a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// The finish had been interrupted; its last steps were completed.
    Completed(String),
    /// A new session collided with an unread result of an earlier one. It
    /// was archived without a result; the TUI shows a notice.
    Conflict(String),
}

/// Completes interrupted finishes and clears collisions, run when the TUI
/// starts and on every change of the inbox. Sessions without a result are
/// left alone, and so are files that cannot be read: whoever loads the
/// session reports those.
pub fn recover(location: &QueueLocation) -> io::Result<Vec<Recovery>> {
    recover_with(location, Gate::default())
}

/// [`recover`] with the lock taken and observed through `gate`.
///
/// Each session is looked at under the drafts lock of its own, as a command
/// may be finishing it at that moment. A session whose lock stays busy is
/// left for the next run, which the next change of the inbox starts; the
/// commands that hold the lock change the inbox themselves when they
/// finish or replace a session.
pub fn recover_with(location: &QueueLocation, mut gate: Gate<'_>) -> io::Result<Vec<Recovery>> {
    let mut recovered = Vec::new();
    for waiting in scan_inbox(location)?.sessions {
        let _lock = match lock_drafts(location, gate.attempts) {
            Ok(lock) => lock,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                tracing::warn!(id = %waiting.id, "left a session to the next recovery, as the queue is busy");
                continue;
            }
            Err(error) => return Err(error),
        };
        let Ok(bytes) = std::fs::read(&waiting.path) else {
            continue;
        };
        let state = outbox_state(location, &waiting.id, &bytes)?;
        if state == OutboxState::NoResult {
            continue;
        }
        (gate.step)(Step::Checked);
        archive_session(location, &waiting.path, &waiting.id)?;
        (gate.step)(Step::Archived);
        if state == OutboxState::ThisSession {
            delete_draft(location, &waiting.id)?;
            (gate.step)(Step::DraftDeleted);
            recovered.push(Recovery::Completed(waiting.id));
        } else {
            recovered.push(Recovery::Conflict(waiting.id));
        }
    }
    Ok(recovered)
}

/// What happened to an invalid session file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// It got an error result and was archived.
    ErrorResult,
    /// An unread result under its id was in the way, so it was only
    /// archived; the error result would have destroyed that result.
    Conflict,
    /// The file left the inbox or changed since it was validated, so
    /// nothing was written; the next sync sees what is there now.
    Left,
}

/// Handles a session file that failed validation with the content `bytes`:
/// an error result naming `message`, unless an unread result is in the way.
pub fn reject_invalid(
    location: &QueueLocation,
    session: &Path,
    bytes: &[u8],
    message: &str,
    at: &str,
) -> Result<Rejected, FinishError> {
    reject_invalid_with(location, session, bytes, message, at, Gate::default())
}

/// [`reject_invalid`] with the lock taken and observed through `gate`. The
/// error result answers `bytes`, so it is written only while the file still
/// holds them.
pub fn reject_invalid_with(
    location: &QueueLocation,
    session: &Path,
    bytes: &[u8],
    message: &str,
    at: &str,
    mut gate: Gate<'_>,
) -> Result<Rejected, FinishError> {
    let id = stem(session);
    let _lock = lock_drafts(location, gate.attempts)?;
    match std::fs::read(session) {
        Ok(current) if current == bytes => {}
        Ok(_) => return Ok(Rejected::Left),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Rejected::Left),
        Err(error) => return Err(error.into()),
    }
    let state = outbox_state(location, id, bytes)?;
    (gate.step)(Step::Checked);
    if state == OutboxState::OtherSession {
        archive_session(location, session, id)?;
        (gate.step)(Step::Archived);
        return Ok(Rejected::Conflict);
    }
    let result = SessionResult::error(id, &session_sha256(bytes), at, message);
    finish_steps(location, session, &result, &mut gate.step)?;
    Ok(Rejected::ErrorResult)
}

/// Archives a `.json` file whose stem is not a valid id. No asker can wait
/// on such a name, so it gets no result.
pub fn archive_invalid_stem(location: &QueueLocation, path: &Path) -> io::Result<()> {
    archive_session(location, path, INVALID_STEM_ID)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::format::{Answer, SessionResult};
    use crate::queue::{
        Archived, INVALID_STEM_ID, drop_session, load_draft, parse_archive_name, save_draft,
    };

    const AT: &str = "2026-09-27T17:05:12+02:00";

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        location.create_layout().expect("layout");
        (scratch, location)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .expect("readable")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    fn dropped(location: &QueueLocation, id: &str, bytes: &[u8]) -> PathBuf {
        drop_session(location, id, bytes, false).expect("dropped");
        location.inbox().join(format!("{id}.json"))
    }

    fn submitted(id: &str, bytes: &[u8]) -> SessionResult {
        SessionResult::submitted(
            id,
            &session_sha256(bytes),
            AT,
            None,
            vec![Answer::skipped("q")],
        )
    }

    #[test]
    fn hashes_the_bytes_as_lowercase_hex() {
        assert_eq!(
            session_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn finishing_writes_the_result_archives_the_session_and_deletes_the_draft() {
        let (_scratch, location) = queue();
        let session = dropped(&location, "batch-01", b"session");
        save_draft(
            &location,
            &SessionResult::draft("batch-01", "q", Vec::new()),
        )
        .expect("saved");

        finish(&location, &session, &submitted("batch-01", b"session")).expect("finished");

        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        assert_eq!(names(&location.outbox()), ["batch-01.json"]);
        let archived = names(&location.archive());
        let entry = parse_archive_name(&archived[0]).expect("archive name parses");
        assert_eq!(
            (entry.id.as_str(), entry.kind),
            ("batch-01", Archived::Session)
        );
        assert_eq!(
            fs::read(location.archive().join(&archived[0])).expect("readable"),
            b"session"
        );
        assert_eq!(load_draft(&location, "batch-01").expect("readable"), None);
    }

    #[test]
    fn finishing_twice_is_harmless() {
        let (_scratch, location) = queue();
        let session = dropped(&location, "batch-01", b"session");
        let result = submitted("batch-01", b"session");

        finish(&location, &session, &result).expect("finished");
        finish(&location, &session, &result).expect("finishing again is fine");

        assert_eq!(names(&location.archive()).len(), 1);
    }

    #[test]
    fn finishing_never_overwrites_the_result_of_another_session() {
        let (_scratch, location) = queue();
        let session = dropped(&location, "batch-01", b"new session");
        fs::write(
            location.outbox().join("batch-01.json"),
            serde_json::to_vec(&submitted("batch-01", b"old")).expect("json"),
        )
        .expect("written");

        let refused = finish(&location, &session, &submitted("batch-01", b"new session"));

        assert!(matches!(refused, Err(FinishError::Conflict(id)) if id == "batch-01"));
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
    }

    #[test]
    fn recovery_completes_an_interrupted_finish() {
        let (_scratch, location) = queue();
        dropped(&location, "batch-01", b"session");
        save_draft(
            &location,
            &SessionResult::draft("batch-01", "q", Vec::new()),
        )
        .expect("saved");
        // The crash came after the result was written.
        fs::write(
            location.outbox().join("batch-01.json"),
            serde_json::to_vec(&submitted("batch-01", b"session")).expect("json"),
        )
        .expect("written");

        let recovered = recover(&location).expect("recovery runs");

        assert_eq!(recovered, [Recovery::Completed("batch-01".into())]);
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        assert_eq!(load_draft(&location, "batch-01").expect("readable"), None);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_leaves_an_unreadable_session_alone() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, location) = queue();
        let locked = dropped(&location, "locked", b"session");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("chmod");
        dropped(&location, "batch-01", b"session");
        fs::write(
            location.outbox().join("batch-01.json"),
            serde_json::to_vec(&submitted("batch-01", b"session")).expect("json"),
        )
        .expect("written");

        let recovered = recover(&location).expect("recovery runs");

        assert_eq!(recovered, [Recovery::Completed("batch-01".into())]);
        assert_eq!(names(&location.inbox()), ["locked.json"]);
    }

    #[test]
    fn recovery_archives_a_new_session_colliding_with_an_unread_result() {
        let (_scratch, location) = queue();
        fs::write(
            location.outbox().join("batch-01.json"),
            serde_json::to_vec(&submitted("batch-01", b"old")).expect("json"),
        )
        .expect("written");
        // Dropped by hand, so no ask refused it.
        fs::write(location.inbox().join("batch-01.json"), b"new session").expect("written");

        let recovered = recover(&location).expect("recovery runs");

        assert_eq!(recovered, [Recovery::Conflict("batch-01".into())]);
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        let unread =
            fs::read(location.outbox().join("batch-01.json")).expect("result is untouched");
        assert_eq!(
            serde_json::from_slice::<SessionResult>(&unread).expect("parses"),
            submitted("batch-01", b"old")
        );
    }

    #[test]
    fn an_unreadable_result_counts_as_another_sessions_result() {
        let (_scratch, location) = queue();
        fs::write(location.outbox().join("batch-01.json"), b"{").expect("written");
        fs::write(location.inbox().join("batch-01.json"), b"session").expect("written");

        assert_eq!(
            recover(&location).expect("runs"),
            [Recovery::Conflict("batch-01".into())]
        );
        assert_eq!(
            fs::read(location.outbox().join("batch-01.json")).expect("kept"),
            b"{"
        );
    }

    #[test]
    fn sessions_without_a_result_need_no_recovery() {
        let (_scratch, location) = queue();
        dropped(&location, "waiting", b"session");

        assert_eq!(recover(&location).expect("runs"), []);
        assert_eq!(names(&location.inbox()), ["waiting.json"]);
    }

    #[test]
    fn an_invalid_session_gets_an_error_result_and_is_archived() {
        let (_scratch, location) = queue();
        let session = dropped(&location, "broken", b"{\"asqr\": 3}");

        let outcome = reject_invalid(
            &location,
            &session,
            b"{\"asqr\": 3}",
            "asqr: unsupported format version 3",
            AT,
        )
        .expect("rejected");

        assert_eq!(outcome, Rejected::ErrorResult);
        let result: SessionResult = serde_json::from_slice(
            &fs::read(location.outbox().join("broken.json")).expect("readable"),
        )
        .expect("parses");
        assert_eq!(
            result.error.as_deref(),
            Some("asqr: unsupported format version 3")
        );
        assert_eq!(
            result.session_sha256,
            Some(session_sha256(b"{\"asqr\": 3}"))
        );
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        assert_eq!(names(&location.archive()).len(), 1);
    }

    #[test]
    fn an_error_result_never_replaces_an_unread_result() {
        let (_scratch, location) = queue();
        fs::write(location.outbox().join("batch-01.json"), b"unread").expect("written");
        fs::write(location.inbox().join("batch-01.json"), b"broken").expect("written");

        let outcome = reject_invalid(
            &location,
            &location.inbox().join("batch-01.json"),
            b"broken",
            "bad",
            AT,
        )
        .expect("rejected");

        assert_eq!(outcome, Rejected::Conflict);
        assert_eq!(
            fs::read(location.outbox().join("batch-01.json")).expect("kept"),
            b"unread"
        );
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
    }

    #[test]
    fn a_file_with_an_invalid_stem_is_archived_without_a_result() {
        let (_scratch, location) = queue();
        let path = location.inbox().join("bad name.json");
        fs::write(&path, b"{}").expect("written");

        archive_invalid_stem(&location, &path).expect("archived");

        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
        let archived = names(&location.archive());
        assert_eq!(
            parse_archive_name(&archived[0]).expect("parses").id,
            INVALID_STEM_ID
        );
    }

    // ---------------------------------------------------------------------
    // Under the drafts lock
    // ---------------------------------------------------------------------

    use std::cell::RefCell;

    use crate::queue::recording_gate;

    /// A gate that tries the lock once and fails the test when called: for
    /// operations that must give up before their first step.
    fn busy_gate<'a>() -> Gate<'a> {
        Gate {
            attempts: 1,
            step: Box::new(|step| panic!("no step while the lock is busy: {step:?}")),
        }
    }

    #[test]
    fn a_finish_holds_the_drafts_lock_at_every_step() {
        let (_scratch, location) = queue();
        let session = dropped(&location, "batch-01", b"session");
        save_draft(
            &location,
            &SessionResult::draft("batch-01", "q", Vec::new()),
        )
        .expect("saved");
        let seen = RefCell::new(Vec::new());

        let finished = finish_unchanged_with(
            &location,
            &session,
            &session_sha256(b"session"),
            &submitted("batch-01", b"session"),
            recording_gate(&location, &seen),
        )
        .expect("finished");

        assert_eq!(finished, Finished::Done);
        assert_eq!(
            seen.into_inner(),
            [
                (Step::Checked, true),
                (Step::ResultWritten, true),
                (Step::Archived, true),
                (Step::DraftDeleted, true)
            ]
        );
    }

    #[test]
    fn a_finish_of_a_gone_or_changed_file_writes_nothing() {
        let (_scratch, location) = queue();
        let changed = dropped(&location, "changed", b"new bytes");
        let gone = location.inbox().join("gone.json");

        for session in [changed, gone] {
            let finished = finish_unchanged_with(
                &location,
                &session,
                &session_sha256(b"the bytes the person saw"),
                &submitted("x", b"the bytes the person saw"),
                Gate::default(),
            )
            .expect("no error");

            assert_eq!(finished, Finished::Changed, "{}", session.display());
        }
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
        assert_eq!(names(&location.inbox()), ["changed.json"]);
    }

    #[test]
    fn a_finish_while_the_drafts_lock_is_busy_writes_nothing() {
        let (_scratch, location) = queue();
        let session = dropped(&location, "batch-01", b"session");
        let _held = lock_drafts(&location, 1).expect("locks");

        let busy = finish_unchanged_with(
            &location,
            &session,
            &session_sha256(b"session"),
            &submitted("batch-01", b"session"),
            busy_gate(),
        );

        assert!(
            matches!(&busy, Err(FinishError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock),
            "{busy:?}"
        );
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
    }

    /// A session whose finish was interrupted after its result was written.
    fn interrupted(location: &QueueLocation, id: &str) {
        dropped(location, id, b"session");
        save_draft(location, &SessionResult::draft(id, "q", Vec::new())).expect("saved");
        fs::write(
            location.outbox().join(format!("{id}.json")),
            serde_json::to_vec(&submitted(id, b"session")).expect("json"),
        )
        .expect("written");
    }

    #[test]
    fn recovery_holds_the_drafts_lock_at_every_step() {
        let (_scratch, location) = queue();
        interrupted(&location, "batch-01");
        let seen = RefCell::new(Vec::new());

        let recovered = recover_with(&location, recording_gate(&location, &seen)).expect("runs");

        assert_eq!(recovered, [Recovery::Completed("batch-01".into())]);
        assert_eq!(
            seen.into_inner(),
            [
                (Step::Checked, true),
                (Step::Archived, true),
                (Step::DraftDeleted, true)
            ]
        );
    }

    #[test]
    fn recovery_skips_a_session_while_the_drafts_lock_is_busy() {
        let (_scratch, location) = queue();
        interrupted(&location, "batch-01");
        let _held = lock_drafts(&location, 1).expect("locks");

        let recovered = recover_with(&location, busy_gate()).expect("no error");

        assert_eq!(recovered, []);
        assert_eq!(
            names(&location.inbox()),
            ["batch-01.json"],
            "left for later"
        );
    }

    #[test]
    fn recovery_fails_when_the_drafts_lock_cannot_be_taken() {
        let (_scratch, location) = queue();
        interrupted(&location, "batch-01");
        fs::remove_file(location.drafts_lock_file()).expect("left by the drop");
        fs::create_dir(location.drafts_lock_file()).expect("a directory where the lock goes");

        assert!(recover_with(&location, Gate::default()).is_err());
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
    }

    #[test]
    fn a_rejection_holds_the_drafts_lock_at_every_step() {
        let (_scratch, location) = queue();
        let session = dropped(&location, "broken", b"{");
        let seen = RefCell::new(Vec::new());

        let outcome = reject_invalid_with(
            &location,
            &session,
            b"{",
            "bad",
            AT,
            recording_gate(&location, &seen),
        )
        .expect("rejected");

        assert_eq!(outcome, Rejected::ErrorResult);
        assert_eq!(
            seen.into_inner(),
            [
                (Step::Checked, true),
                (Step::ResultWritten, true),
                (Step::Archived, true),
                (Step::DraftDeleted, true)
            ]
        );
    }

    #[test]
    fn a_rejection_next_to_an_unread_result_holds_the_drafts_lock() {
        let (_scratch, location) = queue();
        fs::write(location.outbox().join("broken.json"), b"unread").expect("written");
        // Dropped by hand, so no ask refused it.
        let session = location.inbox().join("broken.json");
        fs::write(&session, b"{").expect("written");
        let seen = RefCell::new(Vec::new());

        let outcome = reject_invalid_with(
            &location,
            &session,
            b"{",
            "bad",
            AT,
            recording_gate(&location, &seen),
        )
        .expect("rejected");

        assert_eq!(outcome, Rejected::Conflict);
        assert_eq!(
            seen.into_inner(),
            [(Step::Checked, true), (Step::Archived, true)]
        );
    }

    #[test]
    fn a_rejection_of_a_file_that_left_writes_nothing() {
        let (_scratch, location) = queue();
        let replaced = dropped(&location, "replaced", b"{\"asqr\": 2}");
        let gone = location.inbox().join("gone.json");

        for session in [replaced, gone] {
            let outcome =
                reject_invalid_with(&location, &session, b"{", "bad", AT, Gate::default())
                    .expect("no error");

            assert_eq!(outcome, Rejected::Left, "{}", session.display());
        }
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
        assert_eq!(names(&location.inbox()), ["replaced.json"]);
    }

    #[test]
    fn a_rejection_of_a_file_that_cannot_be_read_again_is_an_error() {
        let (_scratch, location) = queue();
        // A directory under the session's name: reading it fails with an
        // error other than a missing file.
        let session = location.inbox().join("broken.json");
        fs::create_dir(&session).expect("created");

        let failed = reject_invalid_with(&location, &session, b"{", "bad", AT, Gate::default());

        assert!(matches!(failed, Err(FinishError::Io(_))), "{failed:?}");
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
    }

    #[test]
    fn a_rejection_while_the_drafts_lock_is_busy_writes_nothing() {
        let (_scratch, location) = queue();
        let session = dropped(&location, "broken", b"{");
        let _held = lock_drafts(&location, 1).expect("locks");

        let busy = reject_invalid_with(&location, &session, b"{", "bad", AT, busy_gate());

        assert!(
            matches!(&busy, Err(FinishError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock),
            "{busy:?}"
        );
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
    }

    #[test]
    fn errors_name_the_conflict() {
        assert_eq!(
            FinishError::Conflict("b".into()).to_string(),
            "the outbox holds an unread result of another session \"b\""
        );
    }
}
