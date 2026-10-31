// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cleaning up on request (spec section 5.9). asqr deletes nothing on its
//! own apart from drafts, so the archive grows until `asqr prune` runs.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::names::entries_if_present;
use super::{
    Archived, BundleReader, DRAFTS_LOCK_ATTEMPTS, EntryKind, EntryName, EntryRef, Month,
    QueueLocation, bundle_path, list_entries, list_queues, lock_drafts, parse_archive_name,
    prepare_rewrite, try_lock_queue,
};
use crate::keep::record::{is_kept_name, kept_file_names};

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
    // The kept files go with the sessions that showed them.
    found.extend(unneeded_kept_files(location, &found, cutoff)?);
    found.sort();
    if remove {
        for path in &found {
            remove_if_present(std::fs::remove_file(path))?;
        }
        remove_kept_dir_if_empty(location)?;
    }
    Ok(found)
}

/// The kept files in `archive/files/` that no loose session that stays
/// shows, where the sessions `going` are the ones that do not stay: those
/// that only they showed, and those whose own modification time lies
/// before `cutoff`, which a session that was removed earlier left behind.
/// Only files named as kept files count, so a temp file of a copy being
/// written and anything else in the folder stay.
///
/// A file that a bundled session shows is no reason to keep the loose
/// copy: a bundle holds its own copy of every file its sessions show.
fn unneeded_kept_files(
    location: &QueueLocation,
    going: &[PathBuf],
    cutoff: SystemTime,
) -> io::Result<Vec<PathBuf>> {
    let mut staying: HashSet<String> = HashSet::new();
    let mut leaving: HashSet<String> = HashSet::new();
    for entry in entries_if_present(&location.archive())? {
        let path = entry.path();
        let is_session = entry
            .file_name()
            .to_str()
            .and_then(parse_archive_name)
            .is_some_and(|parsed| parsed.kind == Archived::Session);
        if !is_session {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let names = kept_file_names(&bytes);
        if going.contains(&path) {
            leaving.extend(names);
        } else {
            staying.extend(names);
        }
    }
    let mut unneeded = Vec::new();
    for entry in entries_if_present(&location.kept_files())? {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_kept_name(&name) || staying.contains(&name) {
            continue;
        }
        let aged = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified < cutoff);
        if leaving.contains(&name) || aged {
            unneeded.push(entry.path());
        }
    }
    Ok(unneeded)
}

/// Removes `archive/files/` when nothing is left in it. A folder left
/// empty would keep an otherwise empty queue from being pruned.
fn remove_kept_dir_if_empty(location: &QueueLocation) -> io::Result<()> {
    match std::fs::remove_dir(location.kept_files()) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// What `prune --session` removes or would remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// A file of the archive.
    File(PathBuf),
    /// An entry of a month bundle.
    BundleEntry { bundle: PathBuf, name: String },
}

/// The finished session `entry` names, removed when `remove` is set and
/// only listed otherwise, with the kept files only it used.
///
/// A session can lie in two places, as a loose file and as an entry of the
/// bundle of its month (a compaction that was interrupted leaves both);
/// both go. The kept files go that no other session of the same place
/// shows: loose copies that no other loose session shows, and the `files/`
/// entries of the bundle that no other session of it shows. The result of
/// the session stays, as `prune --older-than` removes results with their
/// month.
///
/// Removing takes the drafts lock once for everything, so no compaction
/// runs between the steps. An unreadable bundle is an error and nothing is
/// removed.
pub fn prune_session(entry: &EntryRef, remove: bool) -> io::Result<Vec<Removal>> {
    let location = &entry.queue;
    if !location.dir().is_dir() {
        return Ok(Vec::new());
    }
    let _lock = if remove {
        Some(lock_drafts(location, DRAFTS_LOCK_ATTEMPTS)?)
    } else {
        None
    };
    let mut removals = Vec::new();

    // The bundled place, built first so that a bundle that cannot be read
    // stops everything before a file is removed.
    let month = Month::of(entry.ulid);
    let bundle = bundle_path(location, month);
    let session_name = EntryName {
        kind: EntryKind::Session,
        id: entry.id.clone(),
        ulid: entry.ulid,
    }
    .zip_name();
    let mut rewrite = None;
    if bundle.is_file() {
        let entries = list_entries(location, month).map_err(io::Error::other)?;
        if entries.iter().any(|held| held.name == session_name) {
            let mut reader = BundleReader::open(location, month).map_err(io::Error::other)?;
            let own = reader.read(&session_name).map_err(io::Error::other)?;
            let mut shown_by_others: HashSet<String> = HashSet::new();
            for other in &entries {
                if other.name != session_name
                    && other
                        .parsed
                        .as_ref()
                        .is_some_and(|parsed| parsed.kind == EntryKind::Session)
                {
                    let bytes = reader.read(&other.name).map_err(io::Error::other)?;
                    shown_by_others.extend(kept_file_names(&bytes));
                }
            }
            let mut drop = vec![session_name.clone()];
            drop.extend(
                kept_file_names(&own)
                    .into_iter()
                    .filter(|name| !shown_by_others.contains(name))
                    .map(|name| format!("files/{name}")),
            );
            // Only the files the bundle holds are removals.
            drop.retain(|name| entries.iter().any(|held| held.name == *name));
            removals.extend(drop.iter().map(|name| Removal::BundleEntry {
                bundle: bundle.clone(),
                name: name.clone(),
            }));
            rewrite = Some(prepare_rewrite(location, month, &drop).map_err(io::Error::other)?);
        }
    }

    // The loose place.
    let loose = entries_if_present(&location.archive())?
        .into_iter()
        .find(|held| {
            held.file_name()
                .to_str()
                .and_then(parse_archive_name)
                .is_some_and(|parsed| parsed.kind == Archived::Session && parsed.ulid == entry.ulid)
        })
        .map(|held| held.path());
    let mut loose_files = Vec::new();
    if let Some(path) = &loose {
        removals.push(Removal::File(path.clone()));
        let own = std::fs::read(path).unwrap_or_default();
        let mut shown_by_others: HashSet<String> = HashSet::new();
        for other in entries_if_present(&location.archive())? {
            let other = other.path();
            let is_session = other
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_archive_name)
                .is_some_and(|parsed| parsed.kind == Archived::Session);
            if is_session
                && other != *path
                && let Ok(bytes) = std::fs::read(&other)
            {
                shown_by_others.extend(kept_file_names(&bytes));
            }
        }
        for name in kept_file_names(&own) {
            let file = location.kept_files().join(&name);
            if !shown_by_others.contains(&name) && file.is_file() {
                removals.push(Removal::File(file.clone()));
                loose_files.push(file);
            }
        }
    }

    if remove {
        if let Some(prepared) = rewrite {
            if !prepared.still_current()? {
                return Err(io::Error::other(format!(
                    "{} changed while it was being rewritten",
                    bundle.display()
                )));
            }
            prepared.commit().map_err(io::Error::other)?;
        }
        if let Some(path) = &loose {
            remove_if_present(std::fs::remove_file(path))?;
        }
        for file in &loose_files {
            remove_if_present(std::fs::remove_file(file))?;
        }
        remove_kept_dir_if_empty(location)?;
    }
    Ok(removals)
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

    // -----------------------------------------------------------------
    // Kept files
    // -----------------------------------------------------------------

    use crate::queue::{EntryPlace, read_entry};

    /// The SHA-256 of the one letter each of these is named for.
    const SHA_A: &str = "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb";
    const SHA_B: &str = "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d";
    const SHA_C: &str = "2e7d2c03a9507ae265ecf5b5356885a53393a2029d241394997265a1a25aefc6";

    /// A session that shows the kept files `names`, one entry each.
    fn showing(names: &[&str]) -> String {
        let context: Vec<serde_json::Value> = names
            .iter()
            .map(|name| serde_json::json!({"type": "code", "path": format!("files/{name}.dat")}))
            .collect();
        serde_json::json!({"asqr": 2, "id": "x", "questions": [
            {"id": "q", "text": "?", "kind": "text", "context": context}]})
        .to_string()
    }

    /// A loose session at `when` that shows `names`; its path and entry.
    fn loose_showing(
        location: &QueueLocation,
        id: &str,
        when: &str,
        names: &[&str],
    ) -> (PathBuf, EntryRef) {
        let ulid = Ulid::from_parts(millis(at(when)), 7);
        let path = location
            .archive()
            .join(archive_name(id, ulid, Archived::Session));
        fs::write(&path, showing(names)).expect("written");
        let entry = EntryRef {
            queue: location.clone(),
            id: id.to_owned(),
            ulid,
            place: EntryPlace::Loose(path.clone()),
        };
        (path, entry)
    }

    /// A kept file under `archive/files/`, with the modification time `when`.
    fn kept(location: &QueueLocation, sha: &str, when: &str) -> PathBuf {
        fs::create_dir_all(location.kept_files()).expect("created");
        let path = location.kept_files().join(format!("{sha}.dat"));
        fs::write(&path, "kept").expect("written");
        File::options()
            .write(true)
            .open(&path)
            .expect("opens")
            .set_modified(at(when))
            .expect("mtime set");
        path
    }

    #[test]
    fn pruning_an_old_loose_session_removes_its_unshared_kept_files() {
        let (_scratch, location) = queue();
        let (old, _) = loose_showing(&location, "old", "2026-08-10T10:00:00Z", &[SHA_A, SHA_B]);
        loose_showing(&location, "new", "2027-01-10T10:00:00Z", &[SHA_B]);
        let unshared = kept(&location, SHA_A, "2027-01-12T00:00:00Z");
        let shared = kept(&location, SHA_B, "2027-01-12T00:00:00Z");

        let removed = prune(&location, 30 * DAY, now(), true).expect("prunes");

        assert!(
            removed.contains(&old) && removed.contains(&unshared),
            "{removed:?}"
        );
        assert!(!removed.contains(&shared));
        assert!(!old.exists() && !unshared.exists());
        assert!(shared.exists(), "a session that stays shows it");
    }

    #[test]
    fn prune_keeps_kept_files_it_did_not_remove_sessions_for() {
        let (_scratch, location) = queue();
        loose_showing(&location, "new", "2027-01-10T10:00:00Z", &[SHA_A]);
        let shown = kept(&location, SHA_A, "2026-08-01T00:00:00Z");
        let fresh_orphan = kept(&location, SHA_C, "2027-01-12T00:00:00Z");

        let removed = prune(&location, 30 * DAY, now(), true).expect("prunes");

        assert!(removed.is_empty(), "{removed:?}");
        assert!(shown.exists() && fresh_orphan.exists());
    }

    #[test]
    fn an_orphan_kept_file_past_the_cutoff_goes() {
        let (_scratch, location) = queue();
        let orphan = kept(&location, SHA_C, "2026-08-01T00:00:00Z");

        let removed = prune(&location, 30 * DAY, now(), true).expect("prunes");

        assert_eq!(removed, std::slice::from_ref(&orphan));
        assert!(!orphan.exists());
    }

    #[test]
    fn files_in_the_kept_folder_that_are_no_kept_names_stay() {
        let (_scratch, location) = queue();
        kept(&location, SHA_A, "2027-01-12T00:00:00Z");
        let temp = location.kept_files().join(".asqr-copy.tmp");
        let stray = location.kept_files().join("notes.txt");
        for path in [&temp, &stray] {
            fs::write(path, "x").expect("written");
            File::options()
                .write(true)
                .open(path)
                .expect("opens")
                .set_modified(at("2026-01-01T00:00:00Z"))
                .expect("mtime set");
        }

        let removed = prune(&location, 30 * DAY, now(), true).expect("prunes");

        assert!(removed.is_empty(), "{removed:?}");
        assert!(temp.exists() && stray.exists());
    }

    #[test]
    fn a_dry_run_lists_the_kept_files_and_removes_nothing() {
        let (_scratch, location) = queue();
        let (old, _) = loose_showing(&location, "old", "2026-08-10T10:00:00Z", &[SHA_A]);
        let file = kept(&location, SHA_A, "2027-01-12T00:00:00Z");

        let listed = prune(&location, 30 * DAY, now(), false).expect("prunes");

        assert_eq!(listed, {
            let mut expected = vec![old.clone(), file.clone()];
            expected.sort();
            expected
        });
        assert!(old.exists() && file.exists());
    }

    #[test]
    fn the_kept_folder_goes_with_its_last_file() {
        let (_scratch, location) = queue();
        loose_showing(&location, "old", "2026-08-10T10:00:00Z", &[SHA_A]);
        kept(&location, SHA_A, "2027-01-12T00:00:00Z");

        prune(&location, 30 * DAY, now(), true).expect("prunes");

        assert!(!location.kept_files().exists());
    }

    // -----------------------------------------------------------------
    // One session
    // -----------------------------------------------------------------

    fn bundled_showing(location: &QueueLocation, id: &str, when: &str, names: &[&str]) -> EntryRef {
        let ulid = Ulid::from_parts(millis(at(when)), 9);
        let name = EntryName {
            kind: EntryKind::Session,
            id: id.to_owned(),
            ulid,
        };
        let zip_name = name.zip_name();
        let mut entries = vec![NewEntry::named(name, showing(names).into_bytes())];
        let scratch = location.dir().join("sources");
        fs::create_dir_all(&scratch).expect("created");
        for sha in names {
            let source = scratch.join(sha);
            // The file is named by the hash of its bytes.
            let letter = match *sha {
                SHA_A => "a",
                SHA_B => "b",
                _ => "c",
            };
            fs::write(&source, letter).expect("written");
            entries.push(NewEntry::kept_file(&format!("{sha}.dat"), source).expect("a kept name"));
        }
        // A file shown twice in one month is one entry; the bundle takes
        // the entries as they come.
        add_entries(location, Month::of(ulid), &entries, now()).expect("bundled");
        EntryRef {
            queue: location.clone(),
            id: id.to_owned(),
            ulid,
            place: EntryPlace::Bundled(Month::of(ulid), zip_name),
        }
    }

    #[test]
    fn prune_session_dry_run_removes_nothing() {
        let (_scratch, location) = queue();
        let (path, entry) = loose_showing(&location, "old", "2026-08-10T10:00:00Z", &[SHA_A]);
        let file = kept(&location, SHA_A, "2027-01-12T00:00:00Z");

        let listed = prune_session(&entry, false).expect("listed");

        assert_eq!(
            listed,
            [Removal::File(path.clone()), Removal::File(file.clone())]
        );
        assert!(path.exists() && file.exists());
    }

    #[test]
    fn prune_session_removes_a_loose_session_and_its_unshared_files() {
        let (_scratch, location) = queue();
        let (path, entry) =
            loose_showing(&location, "old", "2026-08-10T10:00:00Z", &[SHA_A, SHA_B]);
        let (other, _) = loose_showing(&location, "other", "2026-08-11T10:00:00Z", &[SHA_B]);
        let alone = kept(&location, SHA_A, "2027-01-12T00:00:00Z");
        let shared = kept(&location, SHA_B, "2027-01-12T00:00:00Z");

        prune_session(&entry, true).expect("removed");

        assert!(!path.exists() && !alone.exists());
        assert!(
            other.exists() && shared.exists(),
            "what another session shows stays"
        );
    }

    #[test]
    fn prune_session_leaves_the_result_and_the_other_entries_of_the_id() {
        let (_scratch, location) = queue();
        let (_, entry) = loose_showing(&location, "same", "2026-08-10T10:00:00Z", &[]);
        let (second, _) = loose_showing(&location, "same", "2026-08-12T10:00:00Z", &[]);
        let result = location.outbox().join("same.json");
        fs::write(&result, "{}").expect("written");

        prune_session(&entry, true).expect("removed");

        assert!(second.exists(), "ids repeat, and one delete hits one entry");
        assert!(result.exists());
    }

    #[test]
    fn prune_session_rewrites_the_bundle_without_the_session_and_its_unshared_files() {
        let (_scratch, location) = queue();
        let entry = bundled_showing(&location, "old", "2026-10-10T10:00:00Z", &[SHA_A, SHA_B]);
        // A second session of the month shows the file B too.
        let other = bundled_showing(&location, "other", "2026-10-11T10:00:00Z", &[SHA_B]);
        let EntryPlace::Bundled(month, name) = entry.place.clone() else {
            unreachable!("built bundled")
        };

        let listed = prune_session(&entry, false).expect("listed");
        assert!(listed.contains(&Removal::BundleEntry {
            bundle: bundle_path(&location, month),
            name: name.clone()
        }));
        assert!(listed.contains(&Removal::BundleEntry {
            bundle: bundle_path(&location, month),
            name: format!("files/{SHA_A}.dat")
        }));
        assert!(!listed.contains(&Removal::BundleEntry {
            bundle: bundle_path(&location, month),
            name: format!("files/{SHA_B}.dat")
        }));
        prune_session(&entry, true).expect("removed");

        let left: Vec<String> = crate::queue::list_entries(&location, month)
            .expect("listed")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert!(!left.contains(&name) && !left.contains(&format!("files/{SHA_A}.dat")));
        assert!(left.contains(&format!("files/{SHA_B}.dat")), "{left:?}");
        let EntryPlace::Bundled(_, other_name) = other.place else {
            unreachable!("built bundled")
        };
        assert!(read_entry(&location, month, &other_name).is_ok());
    }

    #[test]
    fn prune_session_removes_a_session_that_is_both_loose_and_bundled() {
        let (_scratch, location) = queue();
        let entry = bundled_showing(&location, "old", "2026-10-10T10:00:00Z", &[SHA_A]);
        let loose = location
            .archive()
            .join(archive_name("old", entry.ulid, Archived::Session));
        fs::write(&loose, showing(&[SHA_A])).expect("written");
        let file = kept(&location, SHA_A, "2027-01-12T00:00:00Z");

        prune_session(&entry, true).expect("removed");

        assert!(!loose.exists() && !file.exists());
        assert!(
            !bundle_path(&location, Month::of(entry.ulid)).exists(),
            "its only entries were this session's"
        );
    }

    #[test]
    fn prune_session_in_an_unreadable_bundle_is_an_error_and_removes_nothing() {
        let (_scratch, location) = queue();
        let (path, loose_entry) = loose_showing(&location, "old", "2026-10-10T10:00:00Z", &[]);
        fs::write(
            bundle_path(&location, Month::of(loose_entry.ulid)),
            "no zip",
        )
        .expect("written");
        let entry = EntryRef {
            place: EntryPlace::Bundled(
                Month::of(loose_entry.ulid),
                "sessions/old.x.json".to_owned(),
            ),
            ..loose_entry
        };

        let error = prune_session(&entry, true).expect_err("refused");

        assert!(error.to_string().contains("2026-10.zip"), "{error}");
        assert!(path.exists());
    }

    #[test]
    fn prune_session_in_a_busy_queue_is_an_error() {
        let (_scratch, location) = queue();
        let (path, entry) = loose_showing(&location, "old", "2026-08-10T10:00:00Z", &[]);
        let _held = lock_drafts(&location, 1).expect("locks");

        assert!(prune_session(&entry, true).is_err());
        assert!(path.exists());
    }
}
