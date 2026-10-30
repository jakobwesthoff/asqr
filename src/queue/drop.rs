// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Dropping a session into the inbox, the way `asqr ask` does it (spec
//! section 5.5). These are rules of `ask`, not of the queue: a rename
//! replaces an inbox file unconditionally, so only the writer can decide
//! whether a replacement is allowed. A file dropped by hand replaces
//! whatever is there.

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use ulid::Ulid;

use super::atomic::{write_atomically, write_new_atomically};
use super::{
    Archived, Gate, QueueLocation, Step, archive_name, find_session_file, has_answered_draft,
    lock_drafts,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    /// No session with this id was waiting.
    New(PathBuf),
    /// A waiting session nobody had answered yet was replaced.
    Replaced(PathBuf),
}

#[derive(Debug, Error)]
pub enum DropError {
    /// Another id would leave this session in the person's queue, and
    /// `--force` does not get past this refusal (spec section 5.5), so the
    /// message names the two ways that do.
    #[error(
        "session {0:?} is being answered; wait for its result, or ask the person and withdraw it with `asqr withdraw --force {0}`"
    )]
    BeingAnswered(String),

    #[error(
        "an unread result for {0:?} is in the outbox; read it (`asqr result {0}`) or drop with --force"
    )]
    UnreadResult(String),

    #[error("another session {0:?} was dropped at the same moment")]
    Race(String),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Drops `bytes` as session `id` into the inbox of `location`.
///
/// - An unread result under the same id refuses the drop, so it is never
///   lost; `force` moves it into the archive first.
/// - A waiting session under the same id is replaced only while its draft
///   holds no answer. The replacement keeps the waiting file's name.
/// - Otherwise the file is placed without overwriting anything, so of two
///   askers racing for a new id exactly one wins.
///
/// The checks and the writes run under the drafts lock, so a draft the TUI
/// saves in between cannot slip past the check.
pub fn drop_session(
    location: &QueueLocation,
    id: &str,
    bytes: &[u8],
    force: bool,
) -> Result<Dropped, DropError> {
    drop_session_with(location, id, bytes, force, Gate::default())
}

/// [`drop_session`] with the lock taken and observed through `gate`.
pub fn drop_session_with(
    location: &QueueLocation,
    id: &str,
    bytes: &[u8],
    force: bool,
    mut gate: Gate<'_>,
) -> Result<Dropped, DropError> {
    // The directories are made under the drafts lock, which `asqr prune`
    // holds while it removes an empty queue: a drop never writes into a
    // queue removed after it made the directories.
    let _lock = lock_drafts(location, gate.attempts)?;
    location.create_layout()?;
    let checked = check_drop(location, id, force)?;
    (gate.step)(Step::Checked);
    write_drop(location, id, bytes, checked, &mut gate.step)
}

/// What a drop found that its writes act on.
pub(super) struct CheckedDrop {
    /// The waiting file of the same session, which the drop replaces.
    waiting: Option<PathBuf>,
    /// An unread result under the id, which the drop moves aside.
    result: Option<PathBuf>,
}

/// The refusals of a drop of session `id`, every one of them before the
/// first change, so a refused drop leaves the queue as it was. The caller
/// holds the drafts lock.
pub(super) fn check_drop(
    location: &QueueLocation,
    id: &str,
    force: bool,
) -> Result<CheckedDrop, DropError> {
    let waiting = find_session_file(&location.inbox(), id)?;
    if waiting.is_some() && has_answered_draft(location, id) {
        return Err(DropError::BeingAnswered(id.to_owned()));
    }
    let result = find_session_file(&location.outbox(), id)?;
    if result.is_some() && !force {
        return Err(DropError::UnreadResult(id.to_owned()));
    }
    Ok(CheckedDrop { waiting, result })
}

/// The writes of a drop that [`check_drop`] let through, calling `step`
/// after each. The caller still holds the drafts lock.
pub(super) fn write_drop(
    location: &QueueLocation,
    id: &str,
    bytes: &[u8],
    checked: CheckedDrop,
    step: &mut dyn FnMut(Step),
) -> Result<Dropped, DropError> {
    if let Some(result) = checked.result {
        archive_result(location, &result)?;
        step(Step::Archived);
    }
    let dropped = match checked.waiting {
        Some(waiting) => {
            write_atomically(&waiting, bytes)?;
            Dropped::Replaced(waiting)
        }
        None => {
            let target = location.inbox().join(format!("{id}.json"));
            place_new(&target, id, bytes)?;
            Dropped::New(target)
        }
    };
    step(Step::SessionWritten);
    Ok(dropped)
}

fn place_new(target: &Path, id: &str, bytes: &[u8]) -> Result<(), DropError> {
    write_new_atomically(target, bytes).map_err(|error| match error.kind() {
        io::ErrorKind::AlreadyExists => DropError::Race(id.to_owned()),
        _ => DropError::Io(error),
    })
}

/// Moves a result out of the outbox into the archive, under the spelling
/// of its file name.
fn archive_result(location: &QueueLocation, result: &Path) -> io::Result<()> {
    let stem = result
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("find_session_file only returns UTF-8 <id>.json names");
    let target = location
        .archive()
        .join(archive_name(stem, Ulid::generate(), Archived::Result));
    std::fs::rename(result, target)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::format::{Answer, SessionResult};
    use crate::queue::{Archived, parse_archive_name, save_draft};

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        (scratch, location)
    }

    fn files(dir: &std::path::Path) -> Vec<String> {
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

    #[test]
    fn drops_a_new_session_into_the_inbox() {
        let (_scratch, location) = queue();

        let dropped = drop_session(&location, "batch-01", b"one", false).expect("dropped");

        assert_eq!(
            dropped,
            Dropped::New(location.inbox().join("batch-01.json"))
        );
        assert_eq!(
            fs::read(location.inbox().join("batch-01.json")).expect("readable"),
            b"one"
        );
        assert_eq!(
            files(&location.inbox()),
            ["batch-01.json"],
            "no temp file left"
        );
    }

    #[test]
    fn replaces_a_waiting_session_nobody_has_answered_keeping_its_name() {
        let (_scratch, location) = queue();
        drop_session(&location, "Batch-01", b"one", false).expect("dropped");
        save_draft(
            &location,
            &SessionResult::draft("Batch-01", "q", vec![Answer::new("q")]),
        )
        .expect("saved");

        let dropped = drop_session(&location, "batch-01", b"two", false).expect("replaced");

        assert_eq!(
            dropped,
            Dropped::Replaced(location.inbox().join("Batch-01.json"))
        );
        assert_eq!(files(&location.inbox()), ["Batch-01.json"]);
        assert_eq!(
            fs::read(location.inbox().join("Batch-01.json")).expect("readable"),
            b"two"
        );
    }

    #[test]
    fn a_forced_drop_refused_over_an_answered_draft_leaves_the_result() {
        let (_scratch, location) = queue();
        drop_session(&location, "batch-01", b"one", false).expect("dropped");
        let started = Answer {
            selected: vec!["a".into()],
            ..Answer::new("q")
        };
        save_draft(
            &location,
            &SessionResult::draft("batch-01", "q", vec![started]),
        )
        .expect("saved");
        fs::write(location.outbox().join("batch-01.json"), b"unread").expect("written");

        let refused = drop_session(&location, "batch-01", b"two", true);

        assert!(matches!(refused, Err(DropError::BeingAnswered(_))));
        assert_eq!(files(&location.outbox()), ["batch-01.json"]);
        assert_eq!(files(&location.archive()), Vec::<String>::new());
    }

    #[test]
    fn a_waiting_session_whose_draft_holds_a_cleared_default_is_not_replaced() {
        let (_scratch, location) = queue();
        drop_session(&location, "batch-01", b"one", false).expect("dropped");
        let cleared = Answer {
            cleared: true,
            ..Answer::new("q")
        };
        save_draft(
            &location,
            &SessionResult::draft("batch-01", "q", vec![cleared]),
        )
        .expect("saved");

        let refused = drop_session(&location, "batch-01", b"two", false);

        assert!(matches!(refused, Err(DropError::BeingAnswered(_))));
        assert_eq!(
            fs::read(location.inbox().join("batch-01.json")).expect("readable"),
            b"one"
        );
    }

    #[test]
    fn refuses_to_replace_a_session_that_is_being_answered() {
        let (_scratch, location) = queue();
        drop_session(&location, "batch-01", b"one", false).expect("dropped");
        let started = Answer {
            selected: vec!["a".into()],
            ..Answer::new("q")
        };
        save_draft(
            &location,
            &SessionResult::draft("batch-01", "q", vec![started]),
        )
        .expect("saved");

        let refused = drop_session(&location, "batch-01", b"two", false);

        assert!(matches!(refused, Err(DropError::BeingAnswered(id)) if id == "batch-01"));
        assert_eq!(
            fs::read(location.inbox().join("batch-01.json")).expect("readable"),
            b"one"
        );
        assert_eq!(
            files(&location.outbox()),
            Vec::<String>::new(),
            "no error result"
        );
    }

    #[test]
    fn refuses_while_an_unread_result_waits_in_the_outbox() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");
        fs::write(location.outbox().join("batch-01.json"), "result").expect("written");

        let refused = drop_session(&location, "BATCH-01", b"new", false);

        assert!(matches!(refused, Err(DropError::UnreadResult(id)) if id == "BATCH-01"));
        assert_eq!(files(&location.inbox()), Vec::<String>::new());
    }

    #[test]
    fn force_archives_the_unread_result_first() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");
        fs::write(location.outbox().join("batch-01.json"), "result").expect("written");

        drop_session(&location, "batch-01", b"new", true).expect("dropped with force");

        assert_eq!(files(&location.outbox()), Vec::<String>::new());
        let archived = files(&location.archive());
        assert_eq!(archived.len(), 1);
        let entry = parse_archive_name(&archived[0]).expect("archive name parses");
        assert_eq!(
            (entry.id.as_str(), entry.kind),
            ("batch-01", Archived::Result)
        );
        assert_eq!(
            fs::read(location.inbox().join("batch-01.json")).expect("readable"),
            b"new"
        );
    }

    #[test]
    fn of_two_drops_racing_for_a_new_id_only_one_wins() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");
        let target = location.inbox().join("batch-01.json");
        // The other asker placed its file between our check and our rename.
        place_new(&target, "batch-01", b"first").expect("first wins");

        let lost = place_new(&target, "batch-01", b"second");

        assert!(matches!(lost, Err(DropError::Race(id)) if id == "batch-01"));
        assert_eq!(fs::read(&target).expect("readable"), b"first");
    }

    #[test]
    fn other_failures_while_placing_stay_io_errors() {
        let (_scratch, location) = queue();
        let target = location.inbox().join("never-created").join("x.json");

        assert!(matches!(
            place_new(&target, "x", b"x"),
            Err(DropError::Io(_))
        ));
    }

    // ---------------------------------------------------------------------
    // Under the drafts lock
    // ---------------------------------------------------------------------

    #[test]
    fn a_replacement_holds_the_drafts_lock_at_every_step() {
        let (_scratch, location) = queue();
        drop_session(&location, "batch-01", b"one", false).expect("dropped");
        fs::write(location.outbox().join("batch-01.json"), b"unread").expect("written");
        let seen = std::cell::RefCell::new(Vec::new());

        drop_session_with(
            &location,
            "batch-01",
            b"two",
            true,
            crate::queue::recording_gate(&location, &seen),
        )
        .expect("replaced");

        assert_eq!(
            seen.into_inner(),
            [
                (Step::Checked, true),
                (Step::Archived, true),
                (Step::SessionWritten, true)
            ]
        );
    }

    #[test]
    fn a_new_drop_holds_the_drafts_lock_at_every_step() {
        let (_scratch, location) = queue();
        let seen = std::cell::RefCell::new(Vec::new());

        drop_session_with(
            &location,
            "batch-01",
            b"one",
            false,
            crate::queue::recording_gate(&location, &seen),
        )
        .expect("dropped");

        assert_eq!(
            seen.into_inner(),
            [(Step::Checked, true), (Step::SessionWritten, true)]
        );
    }

    #[test]
    fn a_drop_while_the_drafts_lock_is_busy_is_refused() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");
        let _held = lock_drafts(&location, 1).expect("locks");
        let gate = Gate {
            attempts: 1,
            step: Box::new(|step| panic!("no step while the lock is busy: {step:?}")),
        };

        let busy = drop_session_with(&location, "batch-01", b"one", false, gate);

        assert!(
            matches!(&busy, Err(DropError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock),
            "{busy:?}"
        );
        assert_eq!(files(&location.inbox()), Vec::<String>::new());
    }

    #[test]
    fn errors_name_what_to_do() {
        assert_eq!(
            DropError::BeingAnswered("b".into()).to_string(),
            "session \"b\" is being answered; wait for its result, or ask the person and withdraw it with `asqr withdraw --force b`"
        );
        assert_eq!(
            DropError::UnreadResult("b".into()).to_string(),
            "an unread result for \"b\" is in the outbox; read it (`asqr result b`) or drop with --force"
        );
        assert_eq!(
            DropError::Race("b".into()).to_string(),
            "another session \"b\" was dropped at the same moment"
        );
    }
}
