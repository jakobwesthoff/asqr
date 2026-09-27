// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! File names in a queue (spec sections 3.3, 3.4 and 3.7): which inbox
//! files are sessions, how a session is found whatever the case of its
//! name, and how archive entries are named so they never collide.

use std::io;
use std::path::{Path, PathBuf};

use ulid::Ulid;

use crate::format::{is_valid_session_id, same_session_id};

/// What an inbox file name means to the watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxName<'a> {
    /// `<id>.json` with a valid id: a session.
    Session(&'a str),
    /// A `.json` file whose stem is not a valid id. No asker can be waiting
    /// on that name, so it gets no result and is only archived.
    InvalidStem(&'a str),
    /// Anything else: temp files, dotfiles, editor swap and backup files.
    Ignored,
}

pub fn classify_inbox_name(file_name: &str) -> InboxName<'_> {
    match file_name.strip_suffix(".json") {
        Some(stem) if !file_name.starts_with('.') && !stem.is_empty() => {
            if is_valid_session_id(stem) {
                InboxName::Session(stem)
            } else {
                InboxName::InvalidStem(stem)
            }
        }
        _ => InboxName::Ignored,
    }
}

/// The path of the `<id>.json` file in `dir` whose stem names the same
/// session as `id`, compared ignoring case. The file keeps the spelling it
/// was written with, so the path is found by listing, never built from
/// `id`. A missing directory holds no session.
pub fn find_session_file(dir: &Path, id: &str) -> io::Result<Option<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if let Some(InboxName::Session(stem)) = name.to_str().map(classify_inbox_name)
            && same_session_id(stem, id)
        {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

/// What an archive entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archived {
    /// A finished session file.
    Session,
    /// A result moved out of the outbox by `asqr ask --force`.
    Result,
}

/// A parsed archive file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub id: String,
    /// Unique per entry; its time is when the entry was archived, which is
    /// what `asqr prune` goes by.
    pub ulid: Ulid,
    pub kind: Archived,
}

/// The id a `.json` file with an invalid stem is archived under.
pub const INVALID_STEM_ID: &str = "invalid";

/// `<id>.<ulid>.json` for a session, `<id>.<ulid>.result.json` for a
/// result. The ULID keeps entries of a reused id apart.
pub fn archive_name(id: &str, ulid: Ulid, kind: Archived) -> String {
    match kind {
        Archived::Session => format!("{id}.{ulid}.json"),
        Archived::Result => format!("{id}.{ulid}.result.json"),
    }
}

/// Parses an archive file name from the end: ids may contain `.`, but the
/// ULID has a fixed 26 characters right before the suffix.
pub fn parse_archive_name(file_name: &str) -> Option<ArchiveEntry> {
    let (rest, kind) = match file_name.strip_suffix(".result.json") {
        Some(rest) => (rest, Archived::Result),
        None => (file_name.strip_suffix(".json")?, Archived::Session),
    };
    let (id, ulid) = rest.rsplit_once('.')?;
    let ulid = Ulid::from_string(ulid).ok()?;
    is_valid_session_id(id).then(|| ArchiveEntry {
        id: id.to_owned(),
        ulid,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_json_file_with_a_valid_stem_is_a_session() {
        assert_eq!(
            classify_inbox_name("batch-01.json"),
            InboxName::Session("batch-01")
        );
        assert_eq!(
            classify_inbox_name("with.dots.json"),
            InboxName::Session("with.dots")
        );
    }

    #[test]
    fn a_json_file_with_an_invalid_stem_is_marked_for_the_archive() {
        assert_eq!(
            classify_inbox_name("with space.json"),
            InboxName::InvalidStem("with space")
        );
        assert_eq!(classify_inbox_name(".json"), InboxName::Ignored);
    }

    #[test]
    fn temp_hidden_and_editor_files_are_ignored() {
        for name in [
            ".tmpk3j4h.tmp",
            "batch-01.json.tmp",
            "batch-01.tmp",
            ".hidden.json",
            ".DS_Store",
            "4913",
            "batch-01.json~",
            ".batch-01.json.swp",
            "batch-01.JSON",
        ] {
            assert_eq!(classify_inbox_name(name), InboxName::Ignored, "{name}");
        }
    }

    #[test]
    fn finds_a_session_file_ignoring_case() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("Batch-01.json"), "{}").expect("file is written");
        fs::write(dir.path().join("other.json"), "{}").expect("file is written");

        assert_eq!(
            find_session_file(dir.path(), "batch-01").expect("dir is readable"),
            Some(dir.path().join("Batch-01.json"))
        );
        assert_eq!(
            find_session_file(dir.path(), "missing").expect("dir is readable"),
            None
        );
    }

    #[test]
    fn a_missing_directory_holds_no_session() {
        let dir = tempfile::tempdir().expect("temp dir");

        assert_eq!(
            find_session_file(&dir.path().join("gone"), "x").expect("no error"),
            None
        );
    }

    #[test]
    fn an_unreadable_directory_is_an_error() {
        let file = tempfile::NamedTempFile::new().expect("temp file");

        assert!(find_session_file(file.path(), "x").is_err());
    }

    #[test]
    fn archive_names_carry_a_ulid_and_parse_back_from_the_end() {
        let ulid = ulid::Ulid::from_parts(1_700_000_000_000, 42);

        let session = archive_name("with.dots", ulid, Archived::Session);
        let result = archive_name("with.dots", ulid, Archived::Result);

        assert_eq!(session, format!("with.dots.{ulid}.json"));
        assert_eq!(result, format!("with.dots.{ulid}.result.json"));
        assert_eq!(
            parse_archive_name(&session),
            Some(ArchiveEntry {
                id: "with.dots".into(),
                ulid,
                kind: Archived::Session
            })
        );
        assert_eq!(
            parse_archive_name(&result),
            Some(ArchiveEntry {
                id: "with.dots".into(),
                ulid,
                kind: Archived::Result
            })
        );
    }

    #[test]
    fn names_that_are_not_archive_entries_do_not_parse() {
        for name in [
            "plain.json",
            "id.NOTAULIDNOTAULIDNOTAULID.json",
            ".01HX0000000000000000000000.json",
            "id.01HX0000000000000000000000.txt",
        ] {
            assert_eq!(parse_archive_name(name), None, "{name}");
        }
    }
}
