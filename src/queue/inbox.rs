// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reading the inbox (spec section 5.4): which files are sessions and in
//! which order they are answered.

use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

use super::names::entries_if_present;
use super::{InboxName, QueueLocation, classify_inbox_name};

/// A session file waiting in the inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    /// The id, spelled as the file name spells it.
    pub id: String,
    pub path: PathBuf,
    /// The drop time: a rename into the inbox keeps the temp file's time.
    pub modified: SystemTime,
}

/// What the inbox holds: sessions in queue order, `.json` files whose stem
/// is no valid id, which only get archived (spec section 5.8), and
/// everything else, which is left alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxScan {
    pub sessions: Vec<Waiting>,
    pub invalid_stems: Vec<PathBuf>,
    pub ignored: Vec<PathBuf>,
}

/// Lists the inbox. Sessions are ordered by drop time; ids break ties,
/// since custom ids do not sort by time and two askers can drop in the same
/// instant. A missing inbox is empty, and so is a session that leaves it
/// while the inbox is read.
pub fn scan_inbox(location: &QueueLocation) -> io::Result<InboxScan> {
    scan_inbox_with(location, &|entry| entry.metadata()?.modified())
}

/// [`scan_inbox`] with the reading of a file's drop time given, so a test
/// can make a file vanish after the directory was listed.
fn scan_inbox_with(
    location: &QueueLocation,
    drop_time: &dyn Fn(&std::fs::DirEntry) -> io::Result<SystemTime>,
) -> io::Result<InboxScan> {
    let mut scan = InboxScan::default();
    for entry in entries_if_present(&location.inbox())? {
        let name = entry.file_name();
        match name.to_str().map(classify_inbox_name) {
            Some(InboxName::Session(id)) => match drop_time(&entry) {
                Ok(modified) => scan.sessions.push(Waiting {
                    id: id.to_owned(),
                    path: entry.path(),
                    modified,
                }),
                // The asqr that shows the queue archived it between the
                // listing and now, so it waits no more.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            },
            Some(InboxName::InvalidStem(_)) => scan.invalid_stems.push(entry.path()),
            Some(InboxName::Ignored) | None => scan.ignored.push(entry.path()),
        }
    }
    scan.sessions.sort_by(|a, b| {
        a.modified
            .cmp(&b.modified)
            .then_with(|| a.id.to_ascii_lowercase().cmp(&b.id.to_ascii_lowercase()))
    });
    scan.invalid_stems.sort();
    scan.ignored.sort();
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::*;

    fn write_at(dir: &Path, name: &str, seconds: u64) {
        let path = dir.join(name);
        fs::write(&path, "{}").expect("file is written");
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + seconds);
        File::options()
            .write(true)
            .open(&path)
            .expect("file opens")
            .set_modified(time)
            .expect("mtime is set");
    }

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path());
        location.create_layout().expect("layout");
        (scratch, location)
    }

    #[test]
    fn lists_sessions_by_drop_time_with_the_id_breaking_ties() {
        let (_scratch, location) = queue();
        let inbox = location.inbox();
        write_at(&inbox, "late.json", 30);
        write_at(&inbox, "b-early.json", 10);
        write_at(&inbox, "A-early.json", 10);
        write_at(&inbox, "middle.json", 20);

        let scan = scan_inbox(&location).expect("inbox is readable");
        let ids: Vec<_> = scan
            .sessions
            .iter()
            .map(|waiting| waiting.id.as_str())
            .collect();

        assert_eq!(ids, ["A-early", "b-early", "middle", "late"]);
        assert_eq!(scan.sessions[0].path, inbox.join("A-early.json"));
    }

    #[test]
    fn keeps_invalid_stems_apart_and_ignores_everything_else() {
        let (_scratch, location) = queue();
        let inbox = location.inbox();
        write_at(&inbox, "ok.json", 1);
        write_at(&inbox, "bad name.json", 1);
        write_at(&inbox, ".tmpk3j4h.tmp", 1);
        write_at(&inbox, ".DS_Store", 1);
        write_at(&inbox, "4913", 1);

        let scan = scan_inbox(&location).expect("inbox is readable");

        assert_eq!(scan.sessions.len(), 1);
        assert_eq!(scan.invalid_stems, [inbox.join("bad name.json")]);
        assert_eq!(
            scan.ignored,
            [
                inbox.join(".DS_Store"),
                inbox.join(".tmpk3j4h.tmp"),
                inbox.join("4913")
            ]
        );
    }

    #[test]
    fn a_session_archived_after_the_listing_is_not_waiting_and_no_error() {
        let (_scratch, location) = queue();
        let inbox = location.inbox();
        write_at(&inbox, "gone.json", 10);
        write_at(&inbox, "stays.json", 20);
        let archived_meanwhile = |entry: &fs::DirEntry| {
            if entry.file_name() == "gone.json" {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            entry.metadata()?.modified()
        };

        let scan = scan_inbox_with(&location, &archived_meanwhile).expect("no error");

        let ids: Vec<_> = scan
            .sessions
            .iter()
            .map(|waiting| waiting.id.as_str())
            .collect();
        assert_eq!(ids, ["stays"]);
    }

    #[test]
    fn a_session_whose_drop_time_cannot_be_read_for_another_reason_is_an_error() {
        let (_scratch, location) = queue();
        write_at(&location.inbox(), "a.json", 10);
        let denied = |_: &fs::DirEntry| Err(io::Error::from(io::ErrorKind::PermissionDenied));

        assert!(scan_inbox_with(&location, &denied).is_err());
    }

    #[test]
    fn an_unreadable_inbox_is_an_error() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path());
        fs::write(location.inbox(), "not a directory").expect("file is written");

        assert!(scan_inbox(&location).is_err());
    }

    #[test]
    fn a_missing_inbox_is_empty() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("never-used"));

        let scan = scan_inbox(&location).expect("no error");

        assert!(scan.sessions.is_empty() && scan.invalid_stems.is_empty());
    }
}
