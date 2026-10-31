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
/// can take the directory away between two jobs, so the creation is not a
/// one-off at start.
pub fn stage(files_dir: &Path, bytes: &[u8]) -> io::Result<Staged> {
    let mut temp = match temp_file_in(files_dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(files_dir)?;
            temp_file_in(files_dir)?
        }
        made => made?,
    };
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
