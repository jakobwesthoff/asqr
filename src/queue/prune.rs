// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cleaning up on request (spec section 5.9). asqr deletes nothing on its
//! own apart from drafts, so the archive grows until `asqr prune` runs.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::names::entries_if_present;
use super::{QueueLocation, list_queues, lock_drafts, parse_archive_name, try_lock_queue};

/// Removes archive entries archived more than `older_than` before `now`,
/// judged by the ULID in their name, and returns what was removed.
///
/// Results in the outbox belong to the asker, so they are only removed with
/// `include_results`, going by their modification time, and then unread
/// ones go as well. Files in the archive that are not archive entries stay.
pub fn prune(
    location: &QueueLocation,
    older_than: Duration,
    include_results: bool,
    now: SystemTime,
) -> io::Result<Vec<PathBuf>> {
    let cutoff = now
        .checked_sub(older_than)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut removed = Vec::new();

    for entry in entries_if_present(&location.archive())? {
        let archived = entry
            .file_name()
            .to_str()
            .and_then(parse_archive_name)
            .map(|parsed| parsed.ulid.datetime());
        if archived.is_some_and(|archived| archived < cutoff) {
            std::fs::remove_file(entry.path())?;
            removed.push(entry.path());
        }
    }

    if include_results {
        for entry in entries_if_present(&location.outbox())? {
            if entry.metadata()?.modified()? < cutoff {
                std::fs::remove_file(entry.path())?;
                removed.push(entry.path());
            }
        }
    }

    removed.sort();
    Ok(removed)
}

/// Removes every named queue under `root` that no asqr watches and that
/// holds no file, and returns their directories. Its lock files, `lock`
/// and `drafts.lock`, hold no session data and do not count. A queue that
/// cannot be locked at once, or holds anything, stays.
///
/// Both locks are held while the queue goes, and the lock files are
/// removed while they are held, so a process that waits on one finds its
/// file gone and takes the lock anew on a fresh queue (ADR 85).
pub fn prune_empty_queues(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for summary in list_queues(root)? {
        let location = summary.location;
        // The default queue is listed before its first use. A watched
        // queue is skipped by its lock below.
        if !location.dir().is_dir() {
            continue;
        }
        let Some(_instance) = try_lock_queue(&location)? else {
            continue;
        };
        let Ok(_drafts) = lock_drafts(&location, 1) else {
            continue;
        };
        if !holds_nothing(&location)? {
            continue;
        }
        for dir in [
            location.inbox(),
            location.outbox(),
            location.drafts(),
            location.archive(),
        ] {
            remove_if_present(std::fs::remove_dir(dir))?;
        }
        std::fs::remove_file(location.drafts_lock_file())?;
        std::fs::remove_file(location.lock_file())?;
        std::fs::remove_dir(location.dir())?;
        removed.push(location.dir().to_path_buf());
    }
    Ok(removed)
}

/// Whether the queue at `location` holds no file but its lock files: its
/// four directories are empty or missing, and nothing else lies beside
/// them.
fn holds_nothing(location: &QueueLocation) -> io::Result<bool> {
    let layout = ["inbox", "outbox", "drafts", "archive"];
    for entry in entries_if_present(location.dir())? {
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        let empty_dir = layout.contains(&name)
            && entry.file_type()?.is_dir()
            && entries_if_present(&entry.path())?.is_empty();
        if !empty_dir && name != "lock" && name != "drafts.lock" {
            return Ok(false);
        }
    }
    Ok(true)
}

fn remove_if_present(removed: io::Result<()>) -> io::Result<()> {
    match removed {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use ulid::Ulid;

    use super::*;
    use crate::queue::{Archived, archive_name, lock_drafts};

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        location.create_layout().expect("layout");
        (scratch, location)
    }

    fn archive_entry(location: &QueueLocation, id: &str, age: Duration, kind: Archived) -> String {
        let time = now() - age;
        let millis = time
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("after epoch")
            .as_millis() as u64;
        let name = archive_name(id, Ulid::from_parts(millis, 7), kind);
        fs::write(location.archive().join(&name), "{}").expect("written");
        name
    }

    fn result_aged(location: &QueueLocation, id: &str, age: Duration) -> String {
        let name = format!("{id}.json");
        let path = location.outbox().join(&name);
        fs::write(&path, "{}").expect("written");
        File::options()
            .write(true)
            .open(&path)
            .expect("opens")
            .set_modified(now() - age)
            .expect("mtime set");
        name
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

    #[test]
    fn removes_archive_entries_older_than_the_duration() {
        let (_scratch, location) = queue();
        let old_session = archive_entry(&location, "old", 40 * DAY, Archived::Session);
        let old_result = archive_entry(&location, "old", 35 * DAY, Archived::Result);
        let recent = archive_entry(&location, "recent", 2 * DAY, Archived::Session);
        fs::write(location.archive().join("not-an-entry.txt"), "").expect("written");

        let removed = prune(&location, 30 * DAY, false, now()).expect("prunes");

        assert_eq!(
            removed,
            [
                location.archive().join(&old_session),
                location.archive().join(&old_result)
            ]
        );
        assert_eq!(
            names(&location.archive()),
            ["not-an-entry.txt".to_owned(), recent]
        );
    }

    #[test]
    fn leaves_the_outbox_alone_unless_asked() {
        let (_scratch, location) = queue();
        let unread = result_aged(&location, "unread", 40 * DAY);

        prune(&location, 30 * DAY, false, now()).expect("prunes");

        assert_eq!(names(&location.outbox()), [unread]);
    }

    #[test]
    fn removes_old_results_when_asked() {
        let (_scratch, location) = queue();
        let old = result_aged(&location, "old", 40 * DAY);
        let recent = result_aged(&location, "recent", DAY);

        let removed = prune(&location, 30 * DAY, true, now()).expect("prunes");

        assert_eq!(removed, [location.outbox().join(old)]);
        assert_eq!(names(&location.outbox()), [recent]);
    }

    #[test]
    fn a_queue_that_was_never_used_has_nothing_to_prune() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("unused"));

        assert_eq!(
            prune(&location, DAY, true, now()).expect("prunes"),
            Vec::<std::path::PathBuf>::new()
        );
    }

    // -----------------------------------------------------------------
    // Queues that hold nothing
    // -----------------------------------------------------------------

    /// The named queue `name` under `root`, used once: its directories and
    /// both lock files exist.
    fn used(root: &Path, name: &str) -> QueueLocation {
        let location = QueueLocation::named(root, name);
        location.create_layout().expect("layout");
        drop(crate::queue::lock_queue(&location, None).expect("locks"));
        drop(lock_drafts(&location, 1).expect("locks"));
        location
    }

    #[test]
    fn removes_a_queue_that_holds_nothing() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let stray = used(scratch.path(), "asqr-todo");

        let removed = prune_empty_queues(scratch.path()).expect("prunes");

        assert_eq!(removed, [stray.dir().to_path_buf()]);
        assert!(!stray.dir().exists());
    }

    #[test]
    fn keeps_a_queue_with_any_file() {
        let scratch = tempfile::tempdir().expect("temp dir");
        for place in ["inbox", "outbox", "drafts", "archive", "."] {
            let location = used(scratch.path(), "work");
            let file = location.dir().join(place).join("left.json");
            fs::write(&file, "{}").expect("written");

            let removed = prune_empty_queues(scratch.path()).expect("prunes");

            assert_eq!(removed, Vec::<PathBuf>::new(), "a file in {place}");
            fs::remove_file(&file).expect("removed again");
        }
    }

    #[test]
    fn keeps_a_watched_queue() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let watched = used(scratch.path(), "work");
        let _held = crate::queue::lock_queue(&watched, None).expect("locks");

        let removed = prune_empty_queues(scratch.path()).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
        assert!(watched.inbox().is_dir());
    }

    #[test]
    fn keeps_a_queue_whose_drafts_lock_is_held() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let busy = used(scratch.path(), "work");
        let _held = lock_drafts(&busy, 1).expect("locks");

        let removed = prune_empty_queues(scratch.path()).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
        assert!(busy.inbox().is_dir());
    }

    #[test]
    fn the_default_queue_before_its_first_use_is_no_removed_queue() {
        let scratch = tempfile::tempdir().expect("temp dir");

        let removed = prune_empty_queues(scratch.path()).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
        assert!(!scratch.path().join("queues").exists(), "nothing made");
    }

    #[test]
    fn a_drop_after_the_removal_makes_the_queue_again() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = used(scratch.path(), "work");
        prune_empty_queues(scratch.path()).expect("prunes");

        crate::queue::drop_session(&location, "batch", b"{}", false).expect("dropped");

        assert!(location.inbox().join("batch.json").is_file());
    }
}
