// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cleaning up on request (spec section 5.9). asqr deletes nothing on its
//! own apart from drafts, so the archive grows until `asqr prune` runs.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::names::entries_if_present;
use super::{
    DRAFTS_LOCK_ATTEMPTS, Month, QueueLocation, list_queues, lock_drafts, parse_archive_name,
    try_lock_queue,
};

/// The archive entries and month bundles older than `older_than` at `now`,
/// removed when `remove` is set and only listed otherwise, as a sorted
/// list of their paths.
///
/// An entry is old once all of its month is: its month ends before the
/// cutoff. A loose archive entry is judged by the month of the ULID in its
/// name, the same as the bundle it would be in, so what goes does not
/// depend on whether the compaction has run. Results in the outbox are
/// never removed, and neither are files in the archive that are no
/// entries or bundles.
///
/// Removing takes the drafts lock, under which the compaction renames
/// bundles and removes loose files, so a bundle that is removed here does
/// not come back.
pub fn prune(
    location: &QueueLocation,
    older_than: Duration,
    now: SystemTime,
    remove: bool,
) -> io::Result<Vec<PathBuf>> {
    prune_with(location, older_than, now, remove, DRAFTS_LOCK_ATTEMPTS)
}

/// [`prune`] that tries the drafts lock `lock_attempts` times.
pub fn prune_with(
    location: &QueueLocation,
    older_than: Duration,
    now: SystemTime,
    remove: bool,
    lock_attempts: u32,
) -> io::Result<Vec<PathBuf>> {
    // Taking the lock would make a queue that is not there.
    if !location.dir().is_dir() {
        return Ok(Vec::new());
    }
    let cutoff = now
        .checked_sub(older_than)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let old = |month: Month| month.end() <= cutoff;
    let _lock = if remove {
        Some(lock_drafts(location, lock_attempts)?)
    } else {
        None
    };

    let mut found = Vec::new();
    for entry in entries_if_present(&location.archive())? {
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        let month = match parse_archive_name(name) {
            Some(parsed) => Some(Month::of(parsed.ulid)),
            None => name
                .strip_suffix(".zip")
                .and_then(|month| month.parse().ok()),
        };
        if month.is_some_and(old) {
            found.push(entry.path());
        }
    }
    found.sort();
    if remove {
        for path in &found {
            remove_if_present(std::fs::remove_file(path))?;
        }
    }
    Ok(found)
}

/// Removes every named queue under `root` that no asqr watches and that
/// holds no file, or only lists them when `remove` is not set, and
/// returns their directories. Files in `also_gone` count as removed. A
/// queue's lock files, `lock` and `drafts.lock`, hold no session data and
/// do not count. A queue that cannot be locked at once, or holds anything,
/// stays.
///
/// Both locks are held while the queue goes, and the lock files are
/// removed while they are held, so a process that waits on one finds its
/// file gone and takes the lock anew on a fresh queue (ADR 85).
pub fn prune_empty_queues(
    root: &Path,
    remove: bool,
    also_gone: &[PathBuf],
) -> io::Result<Vec<PathBuf>> {
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
        if !holds_nothing(&location, also_gone)? {
            continue;
        }
        if remove {
            for dir in [
                location.kept_files(),
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
        }
        removed.push(location.dir().to_path_buf());
    }
    Ok(removed)
}

/// Whether the queue at `location` holds no file but its lock files: its
/// four directories are empty or missing, and nothing else lies beside
/// them. The files in `also_gone` count as removed: a dry run asks as if
/// the files it lists were gone already.
fn holds_nothing(location: &QueueLocation, also_gone: &[PathBuf]) -> io::Result<bool> {
    let layout = ["inbox", "outbox", "drafts", "archive"];
    for entry in entries_if_present(location.dir())? {
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        let empty_dir = layout.contains(&name)
            && entry.file_type()?.is_dir()
            && entries_if_present(&entry.path())?.iter().try_fold(
                true,
                |all, inside| -> io::Result<bool> {
                    Ok(all
                        && (also_gone.contains(&inside.path())
                            || is_empty_kept_dir(location, inside)?))
                },
            )?;
        if !empty_dir && name != "lock" && name != "drafts.lock" {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `entry` is the kept-files directory of the queue and empty. An
/// empty one is a trace of the last kept file's removal, not data.
fn is_empty_kept_dir(location: &QueueLocation, entry: &std::fs::DirEntry) -> io::Result<bool> {
    Ok(entry.path() == location.kept_files()
        && entry.file_type()?.is_dir()
        && entries_if_present(&entry.path())?.is_empty())
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
    use crate::queue::{
        Archived, EntryKind, EntryName, Month, NewEntry, add_entries, archive_name, bundle_path,
        lock_drafts,
    };

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

    fn archive_file(location: &QueueLocation, name: &str) -> PathBuf {
        let path = location.archive().join(name);
        fs::write(&path, "{}").expect("written");
        path
    }

    /// The bundle of `month` holding one session.
    fn bundle_of(location: &QueueLocation, month: Month) {
        let name = EntryName {
            kind: EntryKind::Session,
            id: "a".to_owned(),
            ulid: Ulid::from_parts(millis(month.start()), 1),
        };
        add_entries(
            location,
            month,
            &[NewEntry::named(name, b"{}".to_vec())],
            now(),
        )
        .expect("bundled");
    }

    fn millis(time: SystemTime) -> u64 {
        time.duration_since(SystemTime::UNIX_EPOCH)
            .expect("after the epoch")
            .as_millis() as u64
    }

    fn at(text: &str) -> SystemTime {
        SystemTime::from(text.parse::<jiff::Timestamp>().expect("a timestamp"))
    }

    fn loose_at(location: &QueueLocation, id: &str, when: &str) -> PathBuf {
        let name = archive_name(id, Ulid::from_parts(millis(at(when)), 7), Archived::Session);
        let path = location.archive().join(name);
        fs::write(&path, "{}").expect("written");
        path
    }

    #[test]
    fn removes_a_bundle_once_all_of_its_month_is_older_than_the_age() {
        let (_scratch, location) = queue();
        let september = Month::new(2026, 9).expect("a month");
        let october = Month::new(2026, 10).expect("a month");
        bundle_of(&location, september);
        bundle_of(&location, october);
        // September ends at 2026-10-01T00:00:00Z; the age puts the cutoff
        // exactly there.
        let now = at("2026-10-31T00:00:00Z");

        let removed = prune(&location, 30 * DAY, now, true).expect("prunes");

        assert_eq!(removed, [bundle_path(&location, september)]);
        assert!(bundle_path(&location, october).is_file());
    }

    #[test]
    fn keeps_a_bundle_whose_month_ends_one_second_after_the_cutoff() {
        let (_scratch, location) = queue();
        let september = Month::new(2026, 9).expect("a month");
        bundle_of(&location, september);

        let removed = prune(&location, 30 * DAY, at("2026-10-30T23:59:59Z"), true).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
        assert!(bundle_path(&location, september).is_file());
    }

    #[test]
    fn treats_a_loose_entry_like_its_months_bundle() {
        let (_scratch, location) = queue();
        let early = loose_at(&location, "early", "2026-10-02T10:00:00Z");
        let late = loose_at(&location, "late", "2026-09-10T10:00:00Z");
        fs::write(location.archive().join("not-an-entry.txt"), "").expect("written");
        // Cutoff 2026-10-16: September is entirely older, October is not.
        let now = at("2026-11-15T00:00:00Z");

        let removed = prune(&location, 30 * DAY, now, true).expect("prunes");

        assert_eq!(removed, std::slice::from_ref(&late));
        assert!(early.exists());
        assert!(!late.exists());
        assert!(location.archive().join("not-an-entry.txt").exists());
    }

    #[test]
    fn a_dry_run_lists_what_it_would_remove_and_removes_nothing() {
        let (_scratch, location) = queue();
        let loose = loose_at(&location, "old", "2026-08-10T10:00:00Z");
        bundle_of(&location, Month::new(2026, 7).expect("a month"));

        let listed = prune(&location, 30 * DAY, now(), false).expect("prunes");

        assert_eq!(listed.len(), 2, "{listed:?}");
        assert!(loose.exists());
        assert!(bundle_path(&location, Month::new(2026, 7).expect("a month")).exists());
    }

    #[test]
    fn a_bundle_named_after_the_year_9999_stays_and_is_no_panic() {
        let (_scratch, location) = queue();
        let odd = location.archive().join("9999-12.zip");
        fs::write(&odd, "").expect("written");

        let removed = prune(&location, 30 * DAY, now(), true).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
        assert!(odd.exists());
    }

    #[test]
    fn leaves_the_outbox_alone() {
        let (_scratch, location) = queue();
        let unread = result_aged(&location, "unread", 400 * DAY);

        prune(&location, 30 * DAY, now(), true).expect("prunes");

        assert_eq!(names(&location.outbox()), [unread]);
    }

    #[test]
    fn a_queue_that_was_never_used_has_nothing_to_prune() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("unused"));

        assert_eq!(
            prune(&location, 30 * DAY, now(), true).expect("prunes"),
            Vec::<std::path::PathBuf>::new()
        );
    }

    #[test]
    fn a_busy_drafts_lock_stops_a_prune_that_removes() {
        let (_scratch, location) = queue();
        loose_at(&location, "old", "2026-08-10T10:00:00Z");
        let _held = lock_drafts(&location, 1).expect("locked");

        let failed = prune_with(&location, 30 * DAY, now(), true, 1);

        assert_eq!(failed.expect_err("busy").kind(), io::ErrorKind::WouldBlock);
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

        let removed = prune_empty_queues(scratch.path(), true, &[]).expect("prunes");

        assert_eq!(removed, [stray.dir().to_path_buf()]);
        assert!(!stray.dir().exists());
    }

    #[test]
    fn a_dry_run_lists_a_queue_that_holds_nothing_and_keeps_it() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let stray = used(scratch.path(), "asqr-todo");

        let listed = prune_empty_queues(scratch.path(), false, &[]).expect("lists");

        assert_eq!(listed, [stray.dir().to_path_buf()]);
        assert!(stray.inbox().is_dir());
    }

    #[test]
    fn a_dry_run_lists_a_queue_the_listed_paths_would_leave_empty() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let work = used(scratch.path(), "work");
        let old = archive_file(&work, "old");
        let other = used(scratch.path(), "other");
        let kept = archive_file(&other, "kept");

        let listed =
            prune_empty_queues(scratch.path(), false, std::slice::from_ref(&old)).expect("lists");

        assert_eq!(listed, [work.dir().to_path_buf()]);
        assert!(old.exists() && kept.exists(), "a dry run removes nothing");
    }

    #[test]
    fn a_queue_whose_only_trace_is_an_empty_files_dir_is_pruned_as_empty() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let stray = used(scratch.path(), "asqr-todo");
        fs::create_dir(stray.kept_files()).expect("files directory");

        let removed = prune_empty_queues(scratch.path(), true, &[]).expect("prunes");

        assert_eq!(removed, [stray.dir().to_path_buf()]);
        assert!(!stray.dir().exists());
    }

    #[test]
    fn a_files_dir_with_a_file_keeps_the_queue() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let work = used(scratch.path(), "work");
        fs::create_dir(work.kept_files()).expect("files directory");
        fs::write(work.kept_files().join("left.webp"), "").expect("written");

        let removed = prune_empty_queues(scratch.path(), true, &[]).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
    }

    #[test]
    fn keeps_a_queue_with_any_file() {
        let scratch = tempfile::tempdir().expect("temp dir");
        for place in ["inbox", "outbox", "drafts", "archive", "."] {
            let location = used(scratch.path(), "work");
            let file = location.dir().join(place).join("left.json");
            fs::write(&file, "{}").expect("written");

            let removed = prune_empty_queues(scratch.path(), true, &[]).expect("prunes");

            assert_eq!(removed, Vec::<PathBuf>::new(), "a file in {place}");
            fs::remove_file(&file).expect("removed again");
        }
    }

    #[test]
    fn keeps_a_watched_queue() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let watched = used(scratch.path(), "work");
        let _held = crate::queue::lock_queue(&watched, None).expect("locks");

        let removed = prune_empty_queues(scratch.path(), true, &[]).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
        assert!(watched.inbox().is_dir());
    }

    #[test]
    fn keeps_a_queue_whose_drafts_lock_is_held() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let busy = used(scratch.path(), "work");
        let _held = lock_drafts(&busy, 1).expect("locks");

        let removed = prune_empty_queues(scratch.path(), true, &[]).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
        assert!(busy.inbox().is_dir());
    }

    #[test]
    fn the_default_queue_before_its_first_use_is_no_removed_queue() {
        let scratch = tempfile::tempdir().expect("temp dir");

        let removed = prune_empty_queues(scratch.path(), true, &[]).expect("prunes");

        assert_eq!(removed, Vec::<PathBuf>::new());
        assert!(!scratch.path().join("queues").exists(), "nothing made");
    }

    #[test]
    fn a_drop_after_the_removal_makes_the_queue_again() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = used(scratch.path(), "work");
        prune_empty_queues(scratch.path(), true, &[]).expect("prunes");

        crate::queue::drop_session(&location, "batch", b"{}", false).expect("dropped");

        assert!(location.inbox().join("batch.json").is_file());
    }
}
