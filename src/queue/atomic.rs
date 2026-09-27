// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Atomic writes (spec section 3.5). Every file asqr writes into a queue
//! is written to a temp file in the target directory and renamed into
//! place, so a reader never sees half a file. The temp file carries the
//! `.tmp` suffix the inbox watcher ignores.

use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

pub(crate) fn temp_file_in(dir: &Path) -> io::Result<NamedTempFile> {
    tempfile::Builder::new()
        .prefix(".asqr-")
        .suffix(".tmp")
        .tempfile_in(dir)
}

fn filled_temp_file(target: &Path, bytes: &[u8]) -> io::Result<NamedTempFile> {
    let dir = target
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no directory"))?;
    let mut temp = temp_file_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    Ok(temp)
}

/// Writes `bytes` to `target`, replacing what is there.
pub(crate) fn write_atomically(target: &Path, bytes: &[u8]) -> io::Result<()> {
    filled_temp_file(target, bytes)?
        .persist(target)
        .map(drop)
        .map_err(|error| error.error)
}

/// Writes `bytes` to `target` only if nothing is there yet; otherwise fails
/// with [`io::ErrorKind::AlreadyExists`] and leaves the existing file alone.
/// Of two writers racing for the same name, exactly one wins.
pub(crate) fn write_new_atomically(target: &Path, bytes: &[u8]) -> io::Result<()> {
    filled_temp_file(target, bytes)?
        .persist_noclobber(target)
        .map(drop)
        .map_err(|error| error.error)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn replaces_the_target_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("result.json");
        fs::write(&target, "old").expect("file is written");

        write_atomically(&target, b"new").expect("write succeeds");

        assert_eq!(fs::read_to_string(&target).expect("readable"), "new");
        assert_eq!(fs::read_dir(dir.path()).expect("readable").count(), 1);
    }

    #[test]
    fn a_new_file_is_only_placed_when_nothing_is_there() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("session.json");

        write_new_atomically(&target, b"first").expect("first write succeeds");
        let second = write_new_atomically(&target, b"second");

        assert_eq!(
            second.map_err(|error| error.kind()),
            Err(std::io::ErrorKind::AlreadyExists)
        );
        assert_eq!(fs::read_to_string(&target).expect("readable"), "first");
        assert_eq!(
            fs::read_dir(dir.path()).expect("readable").count(),
            1,
            "no temp file left"
        );
    }

    #[test]
    fn temp_files_carry_the_tmp_suffix_the_watcher_ignores() {
        let dir = tempfile::tempdir().expect("temp dir");

        let temp = temp_file_in(dir.path()).expect("temp file");
        let name = temp
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("utf-8 name");

        assert!(name.ends_with(".tmp"), "{name}");
    }

    #[test]
    fn a_target_without_a_directory_is_an_error() {
        assert!(write_atomically(std::path::Path::new("/"), b"x").is_err());
    }
}
