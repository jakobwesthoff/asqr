// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The text a viewer shows, from a file or given inline (spec section
//! 7.7). Every viewer of text reads it the same way: a relative path is
//! never resolved, only a regular file is read and only its first 8 MiB,
//! a binary file is not shown, and bytes that are not UTF-8 show as `�`.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use super::viewer::Placeholder;

/// How much of a file a viewer reads. The read happens while the person
/// waits for the screen, and a viewer keeps all of it, so a log that grew
/// to gigabytes must not stall asqr or fill its memory.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// What a viewer shows, as a dim note above the text, when it got only
/// the first [`MAX_BYTES`] of a file.
pub const CUT_NOTE: &str = "first 8 MiB shown";

/// The dim note after the text of such a file, so its end does not look
/// like the end of the file.
pub const END_NOTE: &str = "… the rest is cut after 8 MiB";

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
    fn a_directory_is_not_a_regular_file() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().to_str().expect("a UTF-8 path");

        assert_eq!(
            read_source(Some(path), None),
            Err(Unreadable {
                path: path.to_owned(),
                reason: "not a regular file".into()
            })
        );
    }

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
        // read runs apart and a hang fails the test instead of stalling it.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sender
                .send(read_source(Some(&path), None))
                .expect("the test waits for the result");
        });
        let read = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("reading a FIFO must not block");

        assert_eq!(read.expect_err("a FIFO").reason, "not a regular file");
    }

    #[test]
    fn a_relative_a_missing_and_a_binary_file_say_why_they_do_not_show() {
        assert_eq!(
            read_source(Some("Cargo.toml"), None),
            Err(Unreadable {
                path: "Cargo.toml".into(),
                reason: "relative path".into()
            })
        );

        let missing = read_source(Some("/nowhere/fetch.rs"), None).expect_err("missing");
        assert_eq!(missing.path, "/nowhere/fetch.rs");
        assert!(!missing.reason.is_empty());

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
