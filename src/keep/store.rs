// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where the kept files of a queue lie (spec section 5.1): loose under
//! `archive/files/`, named by the hash of their bytes. A copy is written
//! as a synced temp file first and placed under its name only while the
//! caller holds the drafts lock, together with the recording that refers
//! to it.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

use crate::queue::temp_file_in;

/// A kept file written next to its place, not yet under its name. A temp
/// file that is dropped without being placed removes itself; after a crash
/// the compaction's sweep of stale temp files removes it.
#[derive(Debug)]
pub struct Staged {
    temp: NamedTempFile,
}

/// Writes `bytes` to a synced temp file in `files_dir`, creating the
/// directory when it is missing. A prune or the removal of an empty queue
/// can take the directory away between two jobs, or between its creation
/// and the temp file, so the creation is not a one-off at start.
pub fn stage(files_dir: &Path, bytes: &[u8]) -> io::Result<Staged> {
    stage_with(files_dir, bytes, &|dir| fs::create_dir_all(dir))
}

/// [`stage`] with the creation of the directory given, so a test can take
/// the directory away right after it is made.
fn stage_with(
    files_dir: &Path,
    bytes: &[u8],
    create: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<Staged> {
    // A prune can take the directory away again between its creation and
    // the temp file, so the creation is tried a second time before the
    // failure is the caller's.
    let mut made = temp_file_in(files_dir);
    for _ in 0..2 {
        if made
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        {
            create(files_dir)?;
            made = temp_file_in(files_dir);
        }
    }
    let mut temp = made?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    Ok(Staged { temp })
}

/// Moves `staged` to `files_dir/name`. When a file of that name holds the
/// same bytes already, the temp file is dropped instead. A file of that
/// name with other bytes is damaged, since the name is the hash of the
/// bytes, and is replaced.
pub fn place(files_dir: &Path, name: &str, staged: Staged) -> io::Result<()> {
    let target = files_dir.join(name);
    let present = match fs::read(&target) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Some(present) = present
        && present == fs::read(staged.temp.path())?
    {
        return Ok(());
    }
    staged
        .temp
        .persist(&target)
        .map(drop)
        .map_err(|error| error.error)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_missing_directory_is_made_and_the_file_staged() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let files = scratch.path().join("files");

        let staged = stage(&files, b"bytes").expect("staged");

        assert_eq!(fs::read(staged.temp.path()).expect("reads"), b"bytes");
    }

    #[test]
    fn a_directory_taken_away_right_after_it_was_made_is_made_again() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let files = scratch.path().join("files");
        let taken_away = Cell::new(false);
        // A prune removing the empty directory between the creation and
        // the temp file.
        let create = |dir: &Path| {
            fs::create_dir_all(dir)?;
            if !taken_away.replace(true) {
                fs::remove_dir(dir)?;
            }
            Ok(())
        };

        let staged = stage_with(&files, b"bytes", &create).expect("staged");

        assert_eq!(fs::read(staged.temp.path()).expect("reads"), b"bytes");
    }

    #[test]
    fn a_directory_that_cannot_be_made_is_an_error() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let blocker = scratch.path().join("files");
        fs::write(&blocker, "a file where the directory belongs").expect("written");

        assert!(stage(&blocker, b"bytes").is_err());
    }
}
