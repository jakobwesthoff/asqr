// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cleaning up on request (spec section 3.9). asqr deletes nothing on its
//! own apart from drafts, so the archive grows until `asqr prune` runs.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::names::entries_if_present;
use super::{QueueLocation, parse_archive_name};

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

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use ulid::Ulid;

    use super::*;
    use crate::queue::{Archived, archive_name};

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
}
