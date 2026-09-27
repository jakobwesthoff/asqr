// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finishing a session (spec sections 3.7 and 3.8). Submit, cancel and an
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
    Archived, INVALID_STEM_ID, QueueLocation, archive_name, delete_draft, find_session_file,
    scan_inbox,
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
fn stem(session: &Path) -> &str {
    session
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("inbox sessions are UTF-8 <id>.json names")
}

/// Finishes the session in `session` (an inbox file) with `result`.
pub fn finish(
    location: &QueueLocation,
    session: &Path,
    result: &SessionResult,
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
    archive_session(location, session, id)?;
    delete_draft(location, id)?;
    Ok(())
}

/// Moves a session file into the archive; a file that is gone already was
/// archived by an earlier run.
fn archive_session(location: &QueueLocation, session: &Path, id: &str) -> io::Result<()> {
    let target = location
        .archive()
        .join(archive_name(id, Ulid::generate(), Archived::Session));
    match std::fs::rename(session, target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
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
/// starts. Sessions without a result are left alone.
pub fn recover(location: &QueueLocation) -> io::Result<Vec<Recovery>> {
    let mut recovered = Vec::new();
    for waiting in scan_inbox(location)?.sessions {
        let bytes = std::fs::read(&waiting.path)?;
        match outbox_state(location, &waiting.id, &bytes)? {
            OutboxState::NoResult => {}
            OutboxState::ThisSession => {
                archive_session(location, &waiting.path, &waiting.id)?;
                delete_draft(location, &waiting.id)?;
                recovered.push(Recovery::Completed(waiting.id));
            }
            OutboxState::OtherSession => {
                archive_session(location, &waiting.path, &waiting.id)?;
                recovered.push(Recovery::Conflict(waiting.id));
            }
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
}

/// Handles a session file that failed validation: an error result naming
/// `message`, unless an unread result is in the way.
pub fn reject_invalid(
    location: &QueueLocation,
    session: &Path,
    bytes: &[u8],
    message: &str,
    at: &str,
) -> Result<Rejected, FinishError> {
    let id = stem(session);
    if outbox_state(location, id, bytes)? == OutboxState::OtherSession {
        archive_session(location, session, id)?;
        return Ok(Rejected::Conflict);
    }
    let result = SessionResult::error(id, &session_sha256(bytes), at, message);
    finish(location, session, &result)?;
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
        SessionResult::submitted(id, &session_sha256(bytes), AT, vec![Answer::skipped("q")])
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
        let session = dropped(&location, "broken", b"{\"asqr\": 2}");

        let outcome = reject_invalid(
            &location,
            &session,
            b"{\"asqr\": 2}",
            "asqr: unsupported format version 2",
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
            Some("asqr: unsupported format version 2")
        );
        assert_eq!(
            result.session_sha256,
            Some(session_sha256(b"{\"asqr\": 2}"))
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

    #[test]
    fn errors_name_the_conflict() {
        assert_eq!(
            FinishError::Conflict("b".into()).to_string(),
            "the outbox holds an unread result of another session \"b\""
        );
    }
}
