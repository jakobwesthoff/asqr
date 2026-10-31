// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reading the archive (spec sections 5.1 and 7.2): the title of the
//! last finished session of an id, which the header shows for a
//! follow-up's `follows`. The session may be a loose file or an entry of
//! a month bundle.

use std::io;

use super::names::INVALID_STEM_ID;
use super::{QueueLocation, newest_session};
use crate::format::same_session_id;

/// The `title` of the session of `id` that was archived last, compared
/// ignoring case, as the file holds it.
///
/// Only the newest entry counts: an older round of a reused id is not the
/// session a follow-up continues. Results and the files archived because
/// their name was no id are never read. The file is read as plain JSON
/// rather than as a session, so an entry of an earlier format version or
/// an invalid session still gives its title.
///
/// An entry that cannot be read, is no JSON or has no `title` string
/// gives `None`; only a failing listing of the archive is an error.
pub fn archived_title(location: &QueueLocation, id: &str) -> io::Result<Option<String>> {
    if same_session_id(id, INVALID_STEM_ID) {
        return Ok(None);
    }
    let Some((_, bytes)) = newest_session(location, id)? else {
        return Ok(None);
    };
    let title = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|session| session.get("title")?.as_str().map(str::to_owned));
    Ok(title)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ulid::Ulid;

    use super::*;
    use crate::queue::{Archived, archive_name};

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        location.create_layout().expect("layout");
        (scratch, location)
    }

    /// Archives `text` as the session `id`, archived at `millis`.
    fn archive(location: &QueueLocation, id: &str, millis: u64, text: &str) -> std::path::PathBuf {
        let name = archive_name(id, Ulid::from_parts(millis, 0), Archived::Session);
        let path = location.archive().join(name);
        fs::write(&path, text).expect("written");
        path
    }

    fn titled(title: &str) -> String {
        format!(r#"{{"asqr": 2, "title": "{title}", "questions": []}}"#)
    }

    #[test]
    fn the_newest_archived_session_of_an_id_gives_its_title() {
        let (_scratch, location) = queue();
        // Spelled so that their names sort in another order than their
        // ULIDs: `R` sorts before `r`.
        archive(&location, "ROUND-1", 3_000, &titled("Third try"));
        archive(&location, "round-1", 2_000, &titled("Second try"));
        archive(&location, "Round-1", 1_000, &titled("First try"));
        archive(&location, "round-2", 4_000, &titled("Another session"));

        assert_eq!(
            archived_title(&location, "round-1").expect("readable"),
            Some("Third try".to_owned())
        );
    }

    #[test]
    fn a_bundled_followed_session_gives_its_title() {
        let (_scratch, location) = queue();
        let at = |millis| Ulid::from_parts(millis, 0);
        let bundle = |ulid, title: &str| {
            let name = crate::queue::EntryName {
                kind: crate::queue::EntryKind::Session,
                id: "round-1".to_owned(),
                ulid,
            };
            crate::queue::add_entries(
                &location,
                crate::queue::Month::of(ulid),
                &[crate::queue::NewEntry {
                    name,
                    bytes: titled(title).into_bytes(),
                }],
                std::time::SystemTime::now(),
            )
            .expect("bundled");
        };
        bundle(at(1_000), "In a bundle");
        assert_eq!(
            archived_title(&location, "ROUND-1").expect("readable"),
            Some("In a bundle".to_owned())
        );

        // A loose entry newer than the bundled one wins, and an older one
        // does not.
        archive(&location, "round-1", 2_000, &titled("Loose and newer"));
        assert_eq!(
            archived_title(&location, "round-1").expect("readable"),
            Some("Loose and newer".to_owned())
        );
        bundle(at(3_000), "Bundled and newest");
        assert_eq!(
            archived_title(&location, "round-1").expect("readable"),
            Some("Bundled and newest".to_owned())
        );
    }

    #[test]
    fn results_and_invalid_entries_are_not_sessions_to_follow() {
        let (_scratch, location) = queue();
        archive(&location, "round-1", 1_000, &titled("The session"));
        let result = archive_name("round-1", Ulid::from_parts(2_000, 0), Archived::Result);
        fs::write(location.archive().join(result), titled("A result")).expect("written");
        archive(&location, INVALID_STEM_ID, 3_000, &titled("Not an id"));

        assert_eq!(
            archived_title(&location, "round-1").expect("readable"),
            Some("The session".to_owned())
        );
        assert_eq!(
            archived_title(&location, "Invalid").expect("readable"),
            None
        );
    }

    #[test]
    fn the_newest_entry_without_a_title_gives_none() {
        for newest in [
            r#"{"asqr": 2, "questions": []}"#,
            r#"{"asqr": 2, "title": 7, "questions": []}"#,
            "not json",
        ] {
            let (_scratch, location) = queue();
            archive(&location, "round-1", 1_000, &titled("An older round"));
            archive(&location, "round-1", 2_000, newest);

            assert_eq!(
                archived_title(&location, "round-1").expect("readable"),
                None,
                "{newest}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_newest_entry_gives_none() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, location) = queue();
        archive(&location, "round-1", 1_000, &titled("An older round"));
        let locked = archive(&location, "round-1", 2_000, &titled("Locked"));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("chmod");

        assert_eq!(
            archived_title(&location, "round-1").expect("listing works"),
            None
        );
    }

    #[test]
    fn a_missing_archive_gives_none() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));

        assert_eq!(
            archived_title(&location, "round-1").expect("no error"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_archive_gives_an_error() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, location) = queue();
        archive(&location, "round-1", 1_000, &titled("Hidden"));
        fs::set_permissions(location.archive(), fs::Permissions::from_mode(0o000)).expect("chmod");

        let listed = archived_title(&location, "round-1");

        fs::set_permissions(location.archive(), fs::Permissions::from_mode(0o755))
            .expect("chmod back, so the temp dir can go");
        assert!(listed.is_err(), "{listed:?}");
    }
}
