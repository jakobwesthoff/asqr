// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The text a viewer shows, from a file or given inline (spec section
//! 7.7). Every viewer of text reads it the same way: a relative path is
//! never resolved, only a regular file is read and only its first 8 MiB,
//! a binary file is not shown, and bytes that are not UTF-8 show as `�`.
//! The image viewer opens a path by the same rule.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use image::ImageError;

use super::problem::{Card, Problem};
use super::source_line::Note;

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

/// Why a file cannot be shown, sorted so that nothing in asqr has to read
/// the words of a message to tell one failure from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The path is relative, which asqr never resolves.
    RelativePath,
    NotFound,
    PermissionDenied,
    Directory,
    /// Something other than a regular file or a directory, such as a FIFO
    /// or a device.
    NotRegular,
    /// Text with a NUL byte in it.
    Binary,
    /// An image in a format the decoder does not know, such as SVG.
    FormatUnsupported,
    /// An image whose data the decoder cannot read, such as a cut PNG.
    Damaged,
    Other,
}

impl Failure {
    /// The failure an error of the system of `kind` stands for.
    pub fn of_io(kind: io::ErrorKind) -> Failure {
        match kind {
            io::ErrorKind::NotFound => Failure::NotFound,
            io::ErrorKind::PermissionDenied => Failure::PermissionDenied,
            io::ErrorKind::IsADirectory => Failure::Directory,
            _ => Failure::Other,
        }
    }

    /// The failure an error of the image decoder stands for.
    pub fn of_image(error: &ImageError) -> Failure {
        match error {
            // The PNG decoder reports data that stops early as the reader's
            // error, an unexpected end of file, rather than as a decoding
            // error.
            ImageError::IoError(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                Failure::Damaged
            }
            ImageError::IoError(error) => Failure::of_io(error.kind()),
            ImageError::Unsupported(_) => Failure::FormatUnsupported,
            ImageError::Decoding(_) => Failure::Damaged,
            ImageError::Parameter(_) | ImageError::Limits(_) | ImageError::Encoding(_) => {
                Failure::Other
            }
        }
    }
}

/// A file that cannot be shown: what failed, and the reason in the words
/// of the system, the image decoder or asqr's own check, which the person
/// sees as they are.
#[derive(Debug, PartialEq, Eq)]
pub struct Unreadable {
    pub path: String,
    pub failure: Failure,
    pub reason: String,
}

impl Unreadable {
    /// A file refused by one of asqr's own checks, in its words.
    fn refused(path: &str, failure: Failure, reason: &str) -> Self {
        Unreadable {
            path: path.to_owned(),
            failure,
            reason: reason.to_owned(),
        }
    }

    /// A file the system could not open or read.
    pub fn io(path: &str, error: &io::Error) -> Self {
        Unreadable {
            path: path.to_owned(),
            failure: Failure::of_io(error.kind()),
            reason: error.to_string(),
        }
    }

    /// An image the decoder could not read.
    pub fn image(path: &str, error: &ImageError) -> Self {
        Unreadable {
            path: path.to_owned(),
            failure: Failure::of_image(error),
            reason: error.to_string(),
        }
    }

    /// What a viewer shows in place of its content: a card with the reason
    /// and the file, the home folder `home` in its path shown as `~`.
    pub fn card(&self, home: Option<&Path>) -> Card {
        self.card_of(Problem::error(&self.reason), home)
    }

    /// The card of one side of a diff, `old` or `new`, which names the
    /// side.
    pub fn card_on_side(&self, side: &str, home: Option<&Path>) -> Card {
        self.card_of(Problem::error(&self.reason).on_side(side), home)
    }

    /// Every card is built here, once per viewer, so the log has the
    /// message once for each time the person is shown it.
    fn card_of(&self, problem: Problem, home: Option<&Path>) -> Card {
        tracing::warn!(
            path = %self.path,
            failure = ?self.failure,
            reason = %self.reason,
            "a context entry cannot be shown"
        );
        Card::new(problem, &self.path, home)
    }
}

/// Whether the file at `path` may be opened at all, which every viewer
/// that reads a file, images included, asks before it opens one.
pub fn regular_file(path: &str) -> Result<(), Unreadable> {
    // A relative path depends on where asqr was started, so it is never
    // resolved (spec section 4).
    if Path::new(path).is_relative() {
        return Err(Unreadable::refused(
            path,
            Failure::RelativePath,
            "relative path",
        ));
    }
    // Opening a FIFO waits for a writer, and a device such as /dev/zero
    // never ends, so only a regular file is opened at all. The type is
    // checked before the open, as the open is what would block, and
    // again on the opened file (see `open_checked`).
    let metadata = fs::metadata(path).map_err(|error| Unreadable::io(path, &error))?;
    refuse_irregular(path, &metadata)
}

/// Refuses `path` unless `metadata` says it is a regular file.
fn refuse_irregular(path: &str, metadata: &fs::Metadata) -> Result<(), Unreadable> {
    if metadata.is_file() {
        return Ok(());
    }
    let failure = if metadata.is_dir() {
        Failure::Directory
    } else {
        Failure::NotRegular
    };
    Err(Unreadable::refused(path, failure, "not a regular file"))
}

/// The file at `path` opened for reading, by the rule every viewer of a
/// file follows: what [`regular_file`] refuses is never opened.
pub fn open_regular(path: &str) -> Result<File, Unreadable> {
    regular_file(path)?;
    open_checked(path)
}

/// Opens `path` and refuses what turns out to be no regular file. The
/// check before the open sees the path; a file swapped for a FIFO after
/// it is caught here, on the file actually opened.
///
/// On Unix the open does not wait: opening a FIFO for reading blocks
/// until a writer comes, which would hold the thread that loads the entry
/// for ever. Once the file is known to be regular, its reads block as
/// usual again.
#[cfg(unix)]
fn open_checked(path: &str) -> Result<File, Unreadable> {
    use rustix::fs::{Mode, OFlags, fcntl_getfl, fcntl_setfl, open};

    let io = |error: rustix::io::Errno| Unreadable::io(path, &error.into());
    let file = File::from(
        open(
            path,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io)?,
    );
    let metadata = file
        .metadata()
        .map_err(|error| Unreadable::io(path, &error))?;
    refuse_irregular(path, &metadata)?;
    fcntl_setfl(&file, fcntl_getfl(&file).map_err(io)? - OFlags::NONBLOCK).map_err(io)?;
    Ok(file)
}

/// Opens `path` and refuses what turns out to be no regular file.
#[cfg(not(unix))]
fn open_checked(path: &str) -> Result<File, Unreadable> {
    let file = File::open(path).map_err(|error| Unreadable::io(path, &error))?;
    let metadata = file
        .metadata()
        .map_err(|error| Unreadable::io(path, &error))?;
    refuse_irregular(path, &metadata)?;
    Ok(file)
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
    let file = open_regular(path)?;
    // One byte past the limit tells a file that ends there from a longer
    // one.
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Unreadable::io(path, &error))?;
    let limit = usize::try_from(MAX_BYTES).expect("8 MiB fit into memory");
    let cut = bytes.len() > limit;
    bytes.truncate(limit);
    // A NUL byte marks a binary file, as git decides; its lines would be
    // noise, however they were decoded.
    if bytes.contains(&0) {
        return Err(Unreadable::refused(path, Failure::Binary, "binary file"));
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
                failure: Failure::RelativePath,
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
                failure: Failure::NotFound,
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
                failure: Failure::Directory,
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

        let refused = checked.expect_err("a FIFO");
        assert_eq!(refused.failure, Failure::NotRegular);
        assert_eq!(refused.reason, "not a regular file");
    }

    /// A path that passed the check before the open and was then swapped
    /// for a FIFO: the open itself must not wait for a writer.
    #[cfg(unix)]
    #[test]
    fn opening_a_path_swapped_for_a_fifo_refuses_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let fifo = directory.path().join("pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        let path = fifo.to_str().expect("a UTF-8 path").to_owned();

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(open_checked(&path).map(|_| ()));
        });
        let opened = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("opening a FIFO must not block");

        let refused = opened.expect_err("a FIFO");
        assert_eq!(refused.failure, Failure::NotRegular);
        assert_eq!(refused.reason, "not a regular file");
    }

    /// Reads from the opened file block as usual: a file left in
    /// non-blocking mode could fail a read with `WouldBlock`.
    #[cfg(unix)]
    #[test]
    fn an_opened_file_reads_without_o_nonblock() {
        let (_directory, path) = file(b"one\n");

        let opened = open_regular(&path).expect("a regular file");

        let flags = rustix::fs::fcntl_getfl(&opened).expect("flags are readable");
        assert!(!flags.contains(rustix::fs::OFlags::NONBLOCK), "{flags:?}");
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

        assert_eq!(unreadable.failure, Failure::PermissionDenied);
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

        let binary = read_source(Some(&path), None).expect_err("binary");

        assert_eq!(binary.failure, Failure::Binary);
        assert_eq!(binary.reason, "binary file");
    }

    #[test]
    fn a_source_with_neither_path_nor_content_is_empty() {
        assert_eq!(read_source(None, None), whole(""));
    }

    // -----------------------------------------------------------------
    // The kind of a failure
    // -----------------------------------------------------------------

    #[test]
    fn io_errors_map_to_their_kind() {
        use std::io::ErrorKind;

        let cases = [
            (ErrorKind::NotFound, Failure::NotFound),
            (ErrorKind::PermissionDenied, Failure::PermissionDenied),
            (ErrorKind::IsADirectory, Failure::Directory),
            (ErrorKind::Interrupted, Failure::Other),
        ];
        for (kind, failure) in cases {
            assert_eq!(Failure::of_io(kind), failure, "{kind:?}");
        }
    }

    #[test]
    fn image_errors_map_to_their_kind() {
        use image::ImageError;
        use image::error::{
            DecodingError, ImageFormatHint, LimitError, LimitErrorKind, UnsupportedError,
            UnsupportedErrorKind,
        };
        use std::io::{Error, ErrorKind};

        let cases = [
            (
                ImageError::Unsupported(UnsupportedError::from_format_and_kind(
                    ImageFormatHint::Unknown,
                    UnsupportedErrorKind::Format(ImageFormatHint::Unknown),
                )),
                Failure::FormatUnsupported,
            ),
            (
                ImageError::Decoding(DecodingError::new(ImageFormatHint::Unknown, "bad chunk")),
                Failure::Damaged,
            ),
            (
                ImageError::IoError(Error::from(ErrorKind::UnexpectedEof)),
                Failure::Damaged,
            ),
            (
                ImageError::IoError(Error::from(ErrorKind::NotFound)),
                Failure::NotFound,
            ),
            (
                ImageError::Limits(LimitError::from_kind(LimitErrorKind::InsufficientMemory)),
                Failure::Other,
            ),
        ];
        for (error, failure) in cases {
            assert_eq!(Failure::of_image(&error), failure, "{error:?}");
        }
    }
}
