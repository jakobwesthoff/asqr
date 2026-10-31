// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! How much of a text asqr shows (spec section 8.3). The viewers cut a
//! text by these limits, and the warnings about a session file count by
//! them, so both sides read a text the same way.

/// How much of a file a viewer reads. The read happens while the person
/// waits for the screen, and a viewer keeps all of it, so a log that grew
/// to gigabytes must not stall asqr or fill its memory.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// How many lines of a text a viewer keeps, inline or read from a file.
/// A viewer keeps every line as a row of its own, and a text of millions
/// of lines looks like an error or misuse rather than something to show.
/// 8 MiB of ordinary code, Markdown or a lock file holds at most about
/// 350,000 lines, so the limit cuts none of them before [`MAX_BYTES`]
/// does (ADR 75).
pub const MAX_LINES: usize = 500_000;

/// The size above which a code, Markdown, ANSI or diff entry gets a
/// warning and a Markdown entry shows as plain text (spec sections 3.4 and
/// 8.7). Images are exempt from the warning: screenshots above it are
/// common, and the image viewer handles them.
pub const LARGE_ENTRY_BYTES: u64 = 1024 * 1024;

/// Which characters end a line of a text, as the viewer of its kind
/// reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineBreaks {
    /// `\n`, `\r\n` and a lone `\r`, as CommonMark counts them: code and
    /// Markdown through patine, an image's caption, and both sides of a
    /// diff.
    Any,
    /// Only `\n`: an ANSI entry keeps a lone `\r` as a character of its
    /// line.
    Newline,
}

/// Where `text` is cut to keep its first `limit` lines: right after the
/// line end of line `limit`, when anything follows it. A text of at most
/// `limit` lines, with or without a line end after the last, is not cut.
/// The offset follows an ASCII line end, so it is a character boundary of
/// a `str`.
pub fn line_cut(text: &[u8], limit: usize, breaks: LineBreaks) -> Option<usize> {
    let mut lines = 0;
    let mut index = 0;
    while index < text.len() {
        let end = match (text[index], breaks) {
            (b'\n', _) => Some(index + 1),
            // A `\r` before a `\n` is half of a Windows line end, which the
            // `\n` ends.
            (b'\r', LineBreaks::Any) if text.get(index + 1) != Some(&b'\n') => Some(index + 1),
            _ => None,
        };
        index += 1;
        if let Some(end) = end {
            lines += 1;
            if lines == limit {
                return (end < text.len()).then_some(end);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_of_the_limit_is_not_cut() {
        assert_eq!(line_cut(b"a\nb\n", 2, LineBreaks::Any), None);
    }

    #[test]
    fn a_last_line_without_a_line_end_is_not_cut() {
        assert_eq!(line_cut(b"a\nb", 2, LineBreaks::Any), None);
    }

    #[test]
    fn a_line_after_the_limit_cuts_the_text() {
        assert_eq!(line_cut(b"a\nb\nc", 2, LineBreaks::Any), Some(4));
    }

    #[test]
    fn an_empty_line_after_the_limit_cuts_the_text() {
        assert_eq!(line_cut(b"a\nb\n\n", 2, LineBreaks::Any), Some(4));
    }

    #[test]
    fn a_crlf_ends_a_line_once() {
        assert_eq!(line_cut(b"a\r\nb", 1, LineBreaks::Any), Some(3));
    }

    #[test]
    fn a_lone_cr_ends_a_line_of_code() {
        assert_eq!(line_cut(b"a\rb", 1, LineBreaks::Any), Some(2));
    }

    #[test]
    fn a_lone_cr_does_not_end_an_ansi_line() {
        assert_eq!(line_cut(b"a\rb\nc", 1, LineBreaks::Newline), Some(4));
    }

    /// The notes of a cut text and the warning about one name the limit
    /// as "500,000".
    #[test]
    fn the_notes_and_the_warning_name_the_limit() {
        assert_eq!(MAX_LINES, 500_000);
    }

    /// The warning and the Markdown viewer name the limit as "1 MiB".
    #[test]
    fn the_warning_and_the_markdown_viewer_name_one_mib() {
        assert_eq!(LARGE_ENTRY_BYTES, 1024 * 1024);
    }
}
