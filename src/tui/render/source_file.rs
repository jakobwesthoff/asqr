// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The text a viewer shows, from a file or given inline (spec section
//! 7.7). Every viewer of text reads it the same way: a relative path is
//! never resolved, only a regular file is read and only its first 8 MiB,
//! a binary file is not shown, and bytes that are not UTF-8 show as `�`.
//! The image viewer opens a path by the same rule.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use super::source_line::Note;
use super::viewer::Placeholder;

/// How much of a file a viewer reads. The read happens while the person
/// waits for the screen, and a viewer keeps all of it, so a log that grew
/// to gigabytes must not stall asqr or fill its memory.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// What a viewer shows above the text when it got only the first
/// [`MAX_BYTES`] of a file.
const CUT_NOTE: Note = Note::new("first 8 MiB shown");

/// What follows the text of such a file, so its end does not look like
/// the end of the file.
const END_NOTE: Note = Note::new("… the rest is cut after 8 MiB");

/// The rows of `body`, the text of a source or what a viewer made of it,
/// between the notes of a `cut` source: the first comes before the text,
/// so it shows before the person scrolls to where the text stops, and the
/// last ends it. `note` makes a row of a note, which every viewer draws
/// as a [`Note`], one row cut at the body's edge.
pub fn between_cut_notes<T>(
    cut: bool,
    body: impl IntoIterator<Item = T>,
    note: impl Fn(Note) -> T,
) -> impl Iterator<Item = T> {
    let row = |text| cut.then(|| note(text));
    row(CUT_NOTE).into_iter().chain(body).chain(row(END_NOTE))
}

/// The text of a source, and whether it is only the start of its file.
#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    pub text: String,
    /// The file is longer than [`MAX_BYTES`], and `text` holds that many.
    pub cut: bool,
}

/// A file that cannot be shown, and why.
#[derive(Debug, PartialEq, Eq)]
pub struct Unreadable {
    pub path: String,
    pub reason: String,
}

impl Unreadable {
    /// What a viewer shows in place of its content: `heading`, such as
    /// "diff not shown", the path and the reason.
    pub fn placeholder(&self, heading: &'static str) -> Placeholder {
        Placeholder::new(heading, Some(&self.path), self.reason.clone())
    }
}

/// Whether the file at `path` may be opened at all, which every viewer
/// that reads a file, images included, asks before it opens one.
pub fn regular_file(path: &str) -> Result<(), Unreadable> {
    let unreadable = |reason: String| Unreadable {
        path: path.to_owned(),
        reason,
    };
    // A relative path depends on where asqr was started, so it is never
    // resolved (spec section 4).
    if Path::new(path).is_relative() {
        return Err(unreadable("relative path".to_owned()));
    }
    // Opening a FIFO waits for a writer, and a device such as /dev/zero
    // never ends, so only a regular file is opened at all. The type is
    // checked before the open, as the open is what would block; a file
    // swapped for a FIFO in between still could.
    let metadata = fs::metadata(path).map_err(|error| unreadable(error.to_string()))?;
    if !metadata.is_file() {
        return Err(unreadable("not a regular file".to_owned()));
    }
    Ok(())
}

/// The text of a source given as `content` or as the file at `path`.
/// Validation lets exactly one of the two through; a source with neither
/// never reaches the pane, and would show as empty. Inline content is
/// taken whole, as it already sits in memory with the session.
pub fn read_source(path: Option<&str>, content: Option<&str>) -> Result<Source, Unreadable> {
    let whole = |text: String| Source { text, cut: false };
    if let Some(content) = content {
        return Ok(whole(content.to_owned()));
    }
    let Some(path) = path else {
        return Ok(whole(String::new()));
    };
    let unreadable = |reason: String| Unreadable {
        path: path.to_owned(),
        reason,
    };
    regular_file(path)?;
    // One byte past the limit tells a file that ends there from a longer
    // one.
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| unreadable(error.to_string()))?;
    let limit = usize::try_from(MAX_BYTES).expect("8 MiB fit into memory");
    let cut = bytes.len() > limit;
    bytes.truncate(limit);
    // A NUL byte marks a binary file, as git decides; its lines would be
    // noise, however they were decoded.
    if bytes.contains(&0) {
        return Err(unreadable("binary file".to_owned()));
    }
    // A stray byte from another encoding shows as `�` and leaves the rest
    // of the file readable. So does a character the limit cut in two.
    Ok(Source {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        cut,
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_support;
    use super::*;

    /// A file holding `bytes` in a fresh directory, kept alive with it.
    fn file(bytes: &[u8]) -> (tempfile::TempDir, String) {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("source.txt");
        fs::write(&path, bytes).expect("file is writable");
        let path = path.to_str().expect("a UTF-8 path").to_owned();
        (directory, path)
    }

    fn whole(text: &str) -> Result<Source, Unreadable> {
        Ok(Source {
            text: text.to_owned(),
            cut: false,
        })
    }

    #[test]
    fn inline_content_is_taken_as_it_is() {
        assert_eq!(
            read_source(None, Some("fn main() {}\n")),
            whole("fn main() {}\n")
        );
    }

    #[test]
    fn a_file_is_read_whole() {
        let (_directory, path) = file(b"one\ntwo\n");

        assert_eq!(read_source(Some(&path), None), whole("one\ntwo\n"));
    }

    #[test]
    fn bytes_that_are_not_utf8_show_as_the_replacement_character() {
        let (_directory, path) = file(b"gr\xfc\xdf\n");

        assert_eq!(
            read_source(Some(&path), None),
            whole("gr\u{fffd}\u{fffd}\n")
        );
    }

    #[test]
    fn a_file_above_the_limit_is_read_up_to_it() {
        let limit = usize::try_from(MAX_BYTES).expect("8 MiB fit");
        let (_directory, path) = file(&b"x".repeat(limit + 3));

        let source = read_source(Some(&path), None).expect("readable");

        assert!(source.cut);
        assert_eq!(source.text.len(), limit);
    }

    #[test]
    fn a_file_exactly_at_the_limit_is_whole() {
        let limit = usize::try_from(MAX_BYTES).expect("8 MiB fit");
        let (_directory, path) = file(&b"x".repeat(limit));

        let source = read_source(Some(&path), None).expect("readable");

        assert!(!source.cut);
        assert_eq!(source.text.len(), limit);
    }

    #[test]
    fn the_notes_of_a_cut_source_come_before_and_after_its_text() {
        let rows = |cut| {
            between_cut_notes(cut, ["one", "two"].map(String::from), |note| {
                note.text().to_owned()
            })
            .collect::<Vec<_>>()
        };

        assert_eq!(
            rows(true),
            [
                "first 8 MiB shown",
                "one",
                "two",
                "… the rest is cut after 8 MiB"
            ]
        );
        assert_eq!(rows(false), ["one", "two"], "a whole source has none");
    }

    // -----------------------------------------------------------------
    // Which paths are opened
    // -----------------------------------------------------------------

    #[test]
    fn a_relative_path_is_never_resolved() {
        assert_eq!(
            regular_file("Cargo.toml"),
            Err(Unreadable {
                path: "Cargo.toml".into(),
                reason: "relative path".into()
            })
        );
    }

    #[test]
    fn a_missing_file_says_why() {
        let reason = fs::metadata("/nowhere/fetch.rs")
            .expect_err("the file is missing")
            .to_string();

        assert_eq!(
            regular_file("/nowhere/fetch.rs"),
            Err(Unreadable {
                path: "/nowhere/fetch.rs".into(),
                reason
            })
        );
    }

    #[test]
    fn a_directory_is_not_a_regular_file() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().to_str().expect("a UTF-8 path");

        assert_eq!(
            regular_file(path),
            Err(Unreadable {
                path: path.to_owned(),
                reason: "not a regular file".into()
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let fifo = directory.path().join("pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        let path = fifo.to_str().expect("a UTF-8 path").to_owned();

        // Opening a FIFO for reading blocks until a writer comes, so the
        // check runs apart and a hang fails the test instead of stalling it.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sender
                .send(regular_file(&path))
                .expect("the test waits for the result");
        });
        let checked = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("checking a FIFO must not block");

        assert_eq!(checked.expect_err("a FIFO").reason, "not a regular file");
    }

    #[test]
    fn a_regular_file_may_be_opened() {
        let (_directory, path) = file(b"one\n");

        assert_eq!(regular_file(&path), Ok(()));
    }

    #[test]
    fn a_source_refuses_what_the_path_rule_refuses() {
        let (_directory, paths) = test_support::refused_paths();

        for path in paths {
            assert_eq!(
                read_source(Some(&path), None).map(|_| ()),
                regular_file(&path),
                "{path}"
            );
        }
    }

    // -----------------------------------------------------------------
    // Files that are opened but not shown
    // -----------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn a_regular_file_that_may_not_be_read_says_why() {
        use std::os::unix::fs::PermissionsExt;

        let (_directory, path) = file(b"secret\n");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))
            .expect("permissions are settable");

        let unreadable = read_source(Some(&path), None).expect_err("not readable");

        assert!(
            unreadable
                .reason
                .to_lowercase()
                .contains("permission denied"),
            "{}",
            unreadable.reason
        );
    }

    #[test]
    fn a_binary_file_says_why_it_does_not_show() {
        let (_directory, path) = file(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");

        assert_eq!(
            read_source(Some(&path), None).expect_err("binary").reason,
            "binary file"
        );
    }

    #[test]
    fn a_source_with_neither_path_nor_content_is_empty() {
        assert_eq!(read_source(None, None), whole(""));
    }
}
