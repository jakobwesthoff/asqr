// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The text a viewer shows, from a file or given inline (spec section
//! 7.7). Every viewer of text reads it the same way: a relative path is
//! never resolved, a binary file is not shown, and bytes that are not
//! UTF-8 show as `�`.

use std::fs;
use std::path::Path;

use super::viewer::Placeholder;

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
/// never reaches the pane, and would show as empty.
pub fn read_source(path: Option<&str>, content: Option<&str>) -> Result<String, Unreadable> {
    if let Some(content) = content {
        return Ok(content.to_owned());
    }
    let Some(path) = path else {
        return Ok(String::new());
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
    let bytes = fs::read(path).map_err(|error| unreadable(error.to_string()))?;
    // A NUL byte marks a binary file, as git decides; its lines would be
    // noise, however they were decoded.
    if bytes.contains(&0) {
        return Err(unreadable("binary file".to_owned()));
    }
    // A stray byte from another encoding shows as `�` and leaves the rest
    // of the file readable.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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

    #[test]
    fn inline_content_is_taken_as_it_is() {
        assert_eq!(
            read_source(None, Some("fn main() {}\n")),
            Ok("fn main() {}\n".to_owned())
        );
    }

    #[test]
    fn a_file_is_read_whole() {
        let (_directory, path) = file(b"one\ntwo\n");

        assert_eq!(read_source(Some(&path), None), Ok("one\ntwo\n".to_owned()));
    }

    #[test]
    fn bytes_that_are_not_utf8_show_as_the_replacement_character() {
        let (_directory, path) = file(b"gr\xfc\xdf\n");

        assert_eq!(
            read_source(Some(&path), None),
            Ok("gr\u{fffd}\u{fffd}\n".to_owned())
        );
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
        assert_eq!(read_source(None, None), Ok(String::new()));
    }
}
