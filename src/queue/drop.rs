// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Dropping a session into the inbox, the way `asqr ask` does it (spec
//! section 3.5). These are rules of `ask`, not of the queue: a rename
//! replaces an inbox file unconditionally, so only the writer can decide
//! whether a replacement is allowed. A file dropped by hand replaces
//! whatever is there.

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use ulid::Ulid;

use super::atomic::{write_atomically, write_new_atomically};
use super::{Archived, QueueLocation, archive_name, find_session_file, has_answered_draft};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    /// No session with this id was waiting.
    New(PathBuf),
    /// A waiting session nobody had answered yet was replaced.
    Replaced(PathBuf),
}

#[derive(Debug, Error)]
pub enum DropError {
    #[error("session {0:?} is being answered; wait for its result or use another id")]
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
pub fn drop_session(
    location: &QueueLocation,
    id: &str,
    bytes: &[u8],
    force: bool,
) -> Result<Dropped, DropError> {
    location.create_layout()?;

    if let Some(result) = find_session_file(&location.outbox(), id)? {
        if !force {
            return Err(DropError::UnreadResult(id.to_owned()));
        }
        archive_result(location, &result)?;
    }

    match find_session_file(&location.inbox(), id)? {
        Some(waiting) => {
            if has_answered_draft(location, id) {
                return Err(DropError::BeingAnswered(id.to_owned()));
            }
            write_atomically(&waiting, bytes)?;
            Ok(Dropped::Replaced(waiting))
        }
        None => {
            let target = location.inbox().join(format!("{id}.json"));
            place_new(&target, id, bytes)?;
            Ok(Dropped::New(target))
        }
    }
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

    #[test]
    fn errors_name_what_to_do() {
        assert_eq!(
            DropError::BeingAnswered("b".into()).to_string(),
            "session \"b\" is being answered; wait for its result or use another id"
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
