// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The text a viewer shows, from a file or given inline (spec section
//! 8.3). Every viewer of text reads it the same way: a relative path is
//! never resolved, only a regular file is read and only its first 8 MiB,
//! a binary file is not shown, bytes that are not UTF-8 show as `�`, and
//! a text, inline or from a file, keeps at most its first 500,000 lines.
//! The image viewer opens a path by the same rule and refuses an image
//! file larger than 64 MiB.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use image::ImageError;

use super::problem::Card;
use super::source_line::Note;
use crate::format::line_cut;
pub use crate::format::{LineBreaks, MAX_BYTES, MAX_LINES};
use crate::regular_file::{OpenError, check_type, open as open_checked};

/// The limit that ended the text of a source, which the notes around the
/// text name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cut {
    /// The file is longer than [`MAX_BYTES`], and the text holds its
    /// first `MAX_BYTES` bytes, or one fewer where the limit falls inside
    /// a `\r\n`.
    Bytes,
    /// The text, inline or as read of its file, has more than
    /// [`MAX_LINES`] lines, and the text holds its first `MAX_LINES`.
    Lines,
}

impl Cut {
    /// What a viewer shows above a text this limit cut.
    fn head(self) -> Note {
        match self {
            Cut::Bytes => Note::new("first 8 MiB shown"),
            Cut::Lines => Note::new("first 500,000 lines shown"),
        }
    }

    /// What follows such a text, so its end does not look like the end of
    /// the file.
    fn end(self) -> Note {
        match self {
            Cut::Bytes => Note::new("… the rest is cut after 8 MiB"),
            Cut::Lines => Note::new("… the rest is cut after 500,000 lines"),
        }
    }
}

/// The rows of `body`, the text of a source or what a viewer made of it,
/// between the notes of the limits in `cuts`: the first of each comes
/// before the text, so it shows before the person scrolls to where the
/// text stops, and the last ends it. The two sides of a diff can be cut
/// by different limits, so each limit's notes show once, in the order of
/// [`Cut`]. `note` makes a row of a note, which every viewer draws as a
/// [`Note`], one row cut at the body's edge.
pub fn between_cut_notes<T>(
    cuts: impl IntoIterator<Item = Cut>,
    body: impl IntoIterator<Item = T>,
    note: impl Fn(Note) -> T,
) -> impl Iterator<Item = T> {
    let mut cuts: Vec<Cut> = cuts.into_iter().collect();
    cuts.sort();
    cuts.dedup();
    let heads: Vec<T> = cuts.iter().map(|cut| note(cut.head())).collect();
    let ends: Vec<T> = cuts.iter().map(|cut| note(cut.end())).collect();
    heads.into_iter().chain(body).chain(ends)
}

/// The text of a source, and the limit that cut it, if any.
#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    pub text: String,
    pub cut: Option<Cut>,
}

/// What the image decoder's `error` is, for the log: the kind of error,
/// and for a reader's error its kind too. The decoder reads from memory,
/// as asqr reads the file first, so an `IoError` it reports,
/// `UnexpectedEof` above all, means the image data ends early, not that
/// reading the file failed.
fn image_cause(error: &ImageError) -> String {
    match error {
        ImageError::Decoding(_) => "Decoding".to_owned(),
        ImageError::Encoding(_) => "Encoding".to_owned(),
        ImageError::Parameter(_) => "Parameter".to_owned(),
        ImageError::Limits(_) => "Limits".to_owned(),
        ImageError::Unsupported(_) => "Unsupported".to_owned(),
        ImageError::IoError(error) => format!("IoError({:?})", error.kind()),
    }
}

/// A file that cannot be shown: the reason in the words of the system,
/// the image decoder or asqr's own check, which the person sees as they
/// are, and what failed, which the log names.
#[derive(Debug, PartialEq, Eq)]
pub struct Unreadable {
    pub path: String,
    pub reason: String,
    /// What failed, as one word for the log: the system's error kind, the
    /// image decoder's error (see [`image_cause`]), or the name of asqr's
    /// check that refused the file.
    pub cause: String,
}

impl Unreadable {
    /// A file refused by asqr's check `check`, in its words.
    fn refused(path: &str, check: &str, reason: &str) -> Self {
        Unreadable {
            path: path.to_owned(),
            reason: reason.to_owned(),
            cause: check.to_owned(),
        }
    }

    /// A file the system could not open or read.
    pub fn io(path: &str, error: &io::Error) -> Self {
        Unreadable {
            path: path.to_owned(),
            reason: error.to_string(),
            cause: format!("{:?}", error.kind()),
        }
    }

    /// An image the decoder could not read.
    pub fn image(path: &str, error: &ImageError) -> Self {
        Unreadable {
            path: path.to_owned(),
            reason: error.to_string(),
            cause: image_cause(error),
        }
    }

    /// What a viewer shows in place of its content: a card with the reason
    /// and the file, the home folder `home` in its path shown as `~`.
    pub fn card(&self, home: Option<&Path>) -> Card {
        self.card_of(&self.reason, home)
    }

    /// The card of one side of a diff, `old` or `new`, which names the
    /// side before the reason, as a diff shows the first side that fails.
    pub fn card_on_side(&self, side: &str, home: Option<&Path>) -> Card {
        self.card_of(&format!("{side} side: {}", self.reason), home)
    }

    /// The card of `headline`, the reason as the card puts it. Every card
    /// is built here, once per build of its viewer, so the log has the
    /// message once per load, also for an entry loaded ahead and never
    /// shown.
    fn card_of(&self, headline: &str, home: Option<&Path>) -> Card {
        tracing::warn!(
            path = %self.path,
            cause = %self.cause,
            reason = %self.reason,
            "a context entry cannot be shown"
        );
        Card::error(headline, &self.path, home)
    }
}

/// Whether the file at `path` may be opened at all, which every viewer
/// that reads a file, images included, asks before it opens one.
pub fn regular_file(path: &str) -> Result<(), Unreadable> {
    // A relative path depends on where asqr was started, so it is never
    // resolved (spec section 3.3).
    if Path::new(path).is_relative() {
        return Err(Unreadable::refused(path, "RelativePath", "relative path"));
    }
    // The type is checked before the open as well as on the opened file
    // (see [`open_checked`]), so a path that is plainly no regular file
    // is never opened.
    let metadata = fs::metadata(path).map_err(|error| Unreadable::io(path, &error))?;
    check_type(&metadata).map_err(|error| refused(path, error))
}

/// The file at `path` opened for reading, by the rule every viewer of a
/// file follows: what [`regular_file`] refuses is never opened.
pub fn open_regular(path: &str) -> Result<File, Unreadable> {
    regular_file(path)?;
    open_checked(Path::new(path)).map_err(|error| refused(path, error))
}

/// `error`, in the words a viewer shows for the file at `path`. A path that
/// is a directory or another file that is no regular file reads the same to
/// the person; the log tells them apart.
fn refused(path: &str, error: OpenError) -> Unreadable {
    match error {
        OpenError::NotRegular { directory: true } => {
            Unreadable::refused(path, "Directory", "not a regular file")
        }
        OpenError::NotRegular { directory: false } => {
            Unreadable::refused(path, "NotRegular", "not a regular file")
        }
        OpenError::Io(error) => Unreadable::io(path, &error),
    }
}

/// How large an image file asqr reads. An image keeps its file's bytes
/// while its entry is inside the window (ADR 46), so a file of gigabytes
/// would stay in memory; a larger one is refused before it is read.
pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Why an image file larger than [`MAX_IMAGE_BYTES`] does not show.
const TOO_LARGE: &str = "image file larger than 64 MiB";

/// The bytes of the image file at `path`, opened by the rule every viewer
/// of a file follows, unless it is larger than [`MAX_IMAGE_BYTES`].
pub fn read_image(path: &str) -> Result<Vec<u8>, Unreadable> {
    let file = open_regular(path)?;
    read_within(path, file, MAX_IMAGE_BYTES)
}

/// The bytes of `file`, opened from `path`, unless it holds more than
/// `limit`. The size comes from the opened file, so a file that is too
/// large is never read.
fn read_within(path: &str, file: File, limit: u64) -> Result<Vec<u8>, Unreadable> {
    let size = file
        .metadata()
        .map_err(|error| Unreadable::io(path, &error))?
        .len();
    refuse_size(path, size, limit)?;
    bytes_within(path, file, limit)
}

/// Refuses an image file of `size` bytes past `limit`.
fn refuse_size(path: &str, size: u64, limit: u64) -> Result<(), Unreadable> {
    if size > limit {
        return Err(Unreadable::refused(path, "TooLarge", TOO_LARGE));
    }
    Ok(())
}

/// What `reader` holds, unless it is more than `limit`. A file may grow
/// after its size was taken, so the read stops one byte past the limit,
/// which tells a file that ends there from a longer one.
fn bytes_within(path: &str, reader: impl Read, limit: u64) -> Result<Vec<u8>, Unreadable> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Unreadable::io(path, &error))?;
    refuse_size(path, bytes.len() as u64, limit)?;
    Ok(bytes)
}

/// The text of a source given as `content` or as the file at `path`.
/// Validation lets exactly one of the two through; a source with neither
/// never reaches the pane, and would show as empty. Lines end at `breaks`.
pub fn read_source(
    path: Option<&str>,
    content: Option<&str>,
    breaks: LineBreaks,
) -> Result<Source, Unreadable> {
    if let Some(content) = content {
        return Ok(inline(content, breaks));
    }
    let Some(path) = path else {
        return Ok(inline("", breaks));
    };
    text_within(path, open_regular(path)?, MAX_BYTES, breaks)
}

/// The text of a source given inline, whose lines end at `breaks`. It
/// already sits in memory with the session, so no byte limit applies.
pub fn inline(text: &str, breaks: LineBreaks) -> Source {
    // Only the lines kept are copied, so a cut text holds no memory for
    // the rest.
    match line_cut(text.as_bytes(), MAX_LINES, breaks) {
        Some(end) => Source {
            text: text[..end].to_owned(),
            cut: Some(Cut::Lines),
        },
        None => Source {
            text: text.to_owned(),
            cut: None,
        },
    }
}

/// The text in `reader` up to `limit` bytes and [`MAX_LINES`] lines ended
/// at `breaks`: all of it, or the start of a longer text, marked with the
/// limit that cut it. `path` names the file a failure is about.
fn text_within(
    path: &str,
    reader: impl Read,
    limit: u64,
    breaks: LineBreaks,
) -> Result<Source, Unreadable> {
    // One byte past the limit tells a file that ends there from a longer
    // one.
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Unreadable::io(path, &error))?;
    let limit = usize::try_from(limit).expect("a limit read into memory fits a usize");
    let cut = bytes.len() > limit;
    // The limit can fall between the `\r` and the `\n` of a Windows line
    // end. Kept alone, the `\r` would count as a classic Mac OS line end,
    // and a diff's note would name a `CR` end the file does not use, so the
    // text stops before it. The byte read past the limit tells such a `\r`
    // from a lone one.
    let end = if cut && bytes.ends_with(b"\r\n") {
        limit - 1
    } else {
        limit
    };
    bytes.truncate(end);
    // A NUL byte marks a binary file, as git decides; its lines would be
    // noise, however they were decoded.
    if bytes.contains(&0) {
        return Err(Unreadable::refused(path, "Binary", "binary file"));
    }
    // What is left of the bytes can still hold more lines than a viewer
    // keeps. The line limit then ends the text, so its notes name lines.
    // The bytes are cut before they are decoded, so the text holds only
    // the lines kept.
    let cut = match line_cut(&bytes, MAX_LINES, breaks) {
        Some(end) => {
            bytes.truncate(end);
            Some(Cut::Lines)
        }
        None => cut.then_some(Cut::Bytes),
    };
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
            cut: None,
        })
    }

    #[test]
    fn inline_content_is_taken_as_it_is() {
        assert_eq!(
            read_source(None, Some("fn main() {}\n"), LineBreaks::Any),
            whole("fn main() {}\n")
        );
    }

    #[test]
    fn a_file_is_read_whole() {
        let (_directory, path) = file(b"one\ntwo\n");

        assert_eq!(
            read_source(Some(&path), None, LineBreaks::Any),
            whole("one\ntwo\n")
        );
    }

    #[test]
    fn bytes_that_are_not_utf8_show_as_the_replacement_character() {
        let (_directory, path) = file(b"gr\xfc\xdf\n");

        assert_eq!(
            read_source(Some(&path), None, LineBreaks::Any),
            whole("gr\u{fffd}\u{fffd}\n")
        );
    }

    #[test]
    fn a_file_above_the_limit_is_read_up_to_it() {
        let limit = usize::try_from(MAX_BYTES).expect("8 MiB fit");
        let (_directory, path) = file(&b"x".repeat(limit + 3));

        let source = read_source(Some(&path), None, LineBreaks::Any).expect("readable");

        assert_eq!(source.cut, Some(Cut::Bytes));
        assert_eq!(source.text.len(), limit);
    }

    #[test]
    fn a_file_exactly_at_the_limit_is_whole() {
        let limit = usize::try_from(MAX_BYTES).expect("8 MiB fit");
        let (_directory, path) = file(&b"x".repeat(limit));

        let source = read_source(Some(&path), None, LineBreaks::Any).expect("readable");

        assert_eq!(source.cut, None);
        assert_eq!(source.text.len(), limit);
    }

    /// The limit can fall between the `\r` and the `\n` of a line end;
    /// the text then stops before the `\r`, which alone would be a line end
    /// of another kind. The byte past the limit decides, so a lone `\r`
    /// stays.
    #[test]
    fn a_cut_inside_a_crlf_leaves_its_cr_out() {
        for (bytes, limit, text, cut) in [
            (&b"ab\r\ncd"[..], 3, "ab", Some(Cut::Bytes)),
            (b"ab\rcd", 3, "ab\r", Some(Cut::Bytes)),
            (b"a\r\nb", 3, "a\r\n", Some(Cut::Bytes)),
            (b"ab\r\n", 4, "ab\r\n", None),
            // The blank line whose end the limit cuts is left out too.
            (b"a\r\n\r\nb", 4, "a\r\n", Some(Cut::Bytes)),
        ] {
            assert_eq!(
                text_within("/a.txt", bytes, limit, LineBreaks::Any),
                Ok(Source {
                    text: text.to_owned(),
                    cut
                }),
                "{bytes:?} at {limit}"
            );
        }
    }

    #[test]
    fn a_read_that_fails_says_why() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("the disk went away"))
            }
        }

        let unreadable =
            text_within("/a.txt", Failing, 3, LineBreaks::Any).expect_err("the read fails");

        assert_eq!(unreadable.cause, "Other");
        assert_eq!(unreadable.reason, "the disk went away");
    }

    #[test]
    fn the_notes_of_a_cut_source_come_before_and_after_its_text() {
        let rows = |cuts: &[Cut]| {
            between_cut_notes(
                cuts.iter().copied(),
                ["one", "two"].map(String::from),
                |note| note.text().to_owned(),
            )
            .collect::<Vec<_>>()
        };

        assert_eq!(
            rows(&[Cut::Bytes]),
            [
                "first 8 MiB shown",
                "one",
                "two",
                "… the rest is cut after 8 MiB"
            ]
        );
        assert_eq!(rows(&[]), ["one", "two"], "a whole source has none");
    }

    // -----------------------------------------------------------------
    // The line limit
    // -----------------------------------------------------------------

    /// `count` lines of `x`, each ended by `\n`.
    fn lines_of_x(count: usize) -> String {
        "x\n".repeat(count)
    }

    #[test]
    fn inline_content_above_the_line_limit_keeps_its_first_lines() {
        let text = lines_of_x(MAX_LINES + 1);

        let source = read_source(None, Some(&text), LineBreaks::Any).expect("inline");

        assert_eq!(source.cut, Some(Cut::Lines));
        assert_eq!(source.text, lines_of_x(MAX_LINES));
    }

    #[test]
    fn a_file_above_the_line_limit_keeps_its_first_lines() {
        let (_directory, path) = file(lines_of_x(MAX_LINES + 1).as_bytes());

        let source = read_source(Some(&path), None, LineBreaks::Any).expect("readable");

        assert_eq!(source.cut, Some(Cut::Lines));
        assert_eq!(source.text, lines_of_x(MAX_LINES));
    }

    #[test]
    fn inline_content_at_the_line_limit_is_whole() {
        let text = lines_of_x(MAX_LINES);

        let source = read_source(None, Some(&text), LineBreaks::Any).expect("inline");

        assert_eq!(source.cut, None);
        assert_eq!(source.text, text);
    }

    /// 8 MiB of line ends and one more: the byte limit cuts the file, and
    /// what is left still holds more lines than the line limit, which then
    /// ends the text.
    #[test]
    fn a_file_above_both_limits_names_the_line_limit() {
        let limit = usize::try_from(MAX_BYTES).expect("8 MiB fit");
        let (_directory, path) = file(&b"\n".repeat(limit + 1));

        let source = read_source(Some(&path), None, LineBreaks::Any).expect("readable");

        assert_eq!(source.cut, Some(Cut::Lines));
        assert_eq!(source.text.len(), MAX_LINES);
    }

    #[test]
    fn an_ansi_text_keeps_lone_crs_in_its_lines() {
        let text = "x\r".repeat(MAX_LINES + 1);

        let source = read_source(None, Some(&text), LineBreaks::Newline).expect("inline");

        assert_eq!(source.cut, None);
    }

    #[test]
    fn the_notes_of_a_text_cut_at_the_line_limit_name_lines() {
        let rows: Vec<String> = between_cut_notes([Cut::Lines], ["one".to_owned()], |note| {
            note.text().to_owned()
        })
        .collect();

        assert_eq!(
            rows,
            [
                "first 500,000 lines shown",
                "one",
                "… the rest is cut after 500,000 lines"
            ]
        );
    }

    /// The two sides of a diff can be cut by different limits; each
    /// limit's notes show once, those of 8 MiB first.
    #[test]
    fn a_text_cut_by_both_limits_shows_both_notes() {
        let rows: Vec<String> = between_cut_notes(
            [Cut::Lines, Cut::Bytes, Cut::Lines],
            ["one".to_owned()],
            |note| note.text().to_owned(),
        )
        .collect();

        assert_eq!(
            rows,
            [
                "first 8 MiB shown",
                "first 500,000 lines shown",
                "one",
                "… the rest is cut after 8 MiB",
                "… the rest is cut after 500,000 lines"
            ]
        );
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
                cause: "RelativePath".to_owned(),
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
                cause: "NotFound".to_owned(),
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
                cause: "Directory".to_owned(),
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
        assert_eq!(refused.cause, "NotRegular");
        assert_eq!(refused.reason, "not a regular file");
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
                read_source(Some(&path), None, LineBreaks::Any).map(|_| ()),
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

        let unreadable = read_source(Some(&path), None, LineBreaks::Any).expect_err("not readable");

        assert_eq!(unreadable.cause, "PermissionDenied");
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

        let binary = read_source(Some(&path), None, LineBreaks::Any).expect_err("binary");

        assert_eq!(binary.cause, "Binary");
        assert_eq!(binary.reason, "binary file");
    }

    #[test]
    fn a_source_with_neither_path_nor_content_is_empty() {
        assert_eq!(read_source(None, None, LineBreaks::Any), whole(""));
    }

    // -----------------------------------------------------------------
    // Image files
    // -----------------------------------------------------------------

    #[test]
    fn an_image_file_is_refused_past_the_limit_by_its_size() {
        assert_eq!(refuse_size("/a.png", 10, 10), Ok(()), "at the limit");

        let refused = refuse_size("/a.png", 11, 10).expect_err("past the limit");

        assert_eq!(refused.cause, "TooLarge");
        assert_eq!(refused.reason, "image file larger than 64 MiB");
    }

    /// A file can grow between the look at its size and the read; the
    /// read stops one byte past the limit and refuses it then.
    #[test]
    fn an_image_file_that_grew_past_its_size_is_refused() {
        assert_eq!(bytes_within("/a.png", &[7_u8; 10][..], 10), Ok(vec![7; 10]));

        let refused = bytes_within("/a.png", &[7_u8; 11][..], 10).expect_err("grew");

        assert_eq!(refused.cause, "TooLarge");
    }

    #[test]
    fn an_image_file_at_the_limit_is_read_whole() {
        let (_directory, path) = file(&[1; 10]);
        let opened = open_regular(&path).expect("opens");
        assert_eq!(read_within(&path, opened, 10), Ok(vec![1; 10]));

        let (_directory, longer) = file(&[1; 11]);
        let opened = open_regular(&longer).expect("opens");
        let refused = read_within(&longer, opened, 10).expect_err("past the limit");
        assert_eq!(refused.cause, "TooLarge");
    }

    /// A sparse file takes no room on disk, so the test can afford one
    /// past the real limit. The bound is a guard: a read of the whole
    /// file would take longer on a slow disk.
    #[test]
    fn an_image_file_larger_than_64_mib_is_refused() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("huge.png");
        File::create(&path)
            .and_then(|huge| huge.set_len(MAX_IMAGE_BYTES + 1))
            .expect("sparse file is creatable");
        let path = path.to_str().expect("a UTF-8 path");

        let start = std::time::Instant::now();
        // A failure names the length read, not 64 MiB of bytes.
        let refused = read_image(path)
            .map(|bytes| bytes.len())
            .expect_err("too large");

        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(refused.cause, "TooLarge");
        assert_eq!(refused.reason, "image file larger than 64 MiB");
    }

    #[cfg(unix)]
    #[test]
    fn an_image_file_that_may_not_be_read_says_why() {
        use std::os::unix::fs::PermissionsExt;

        let (_directory, path) = file(b"secret");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))
            .expect("permissions are settable");

        let unreadable = read_image(&path).expect_err("not readable");

        assert_eq!(unreadable.cause, "PermissionDenied");
    }

    // -----------------------------------------------------------------
    // The kind of a failure
    // -----------------------------------------------------------------

    #[test]
    fn the_log_names_a_system_error_by_its_kind() {
        use std::io::{Error, ErrorKind};

        for (kind, cause) in [
            (ErrorKind::NotFound, "NotFound"),
            (ErrorKind::PermissionDenied, "PermissionDenied"),
            (ErrorKind::IsADirectory, "IsADirectory"),
            (ErrorKind::Interrupted, "Interrupted"),
        ] {
            assert_eq!(Unreadable::io("/a", &Error::from(kind)).cause, cause);
        }
    }

    #[test]
    fn the_log_names_an_image_error_by_its_kind() {
        use image::ImageError;
        use image::error::{
            DecodingError, EncodingError, ImageFormatHint, LimitError, LimitErrorKind,
            ParameterError, ParameterErrorKind, UnsupportedError, UnsupportedErrorKind,
        };
        use std::io::{Error, ErrorKind};

        let cases = [
            (
                ImageError::Unsupported(UnsupportedError::from_format_and_kind(
                    ImageFormatHint::Unknown,
                    UnsupportedErrorKind::Format(ImageFormatHint::Unknown),
                )),
                "Unsupported",
            ),
            (
                ImageError::Decoding(DecodingError::new(ImageFormatHint::Unknown, "bad chunk")),
                "Decoding",
            ),
            (
                ImageError::Encoding(EncodingError::new(ImageFormatHint::Unknown, "no room")),
                "Encoding",
            ),
            (
                ImageError::Parameter(ParameterError::from_kind(
                    ParameterErrorKind::DimensionMismatch,
                )),
                "Parameter",
            ),
            (
                ImageError::Limits(LimitError::from_kind(LimitErrorKind::InsufficientMemory)),
                "Limits",
            ),
            (
                ImageError::IoError(Error::from(ErrorKind::UnexpectedEof)),
                "IoError(UnexpectedEof)",
            ),
        ];
        for (error, cause) in cases {
            assert_eq!(
                Unreadable::image("/a.png", &error).cause,
                cause,
                "{error:?}"
            );
        }
    }
}
