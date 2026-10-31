// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reading the files a session shows, within the limits of the viewers,
//! before the result is written: an agent that cleans up after `asqr wait`
//! must not take away what the person saw (ADR 100).

use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use jiff::Timestamp;
use serde_json::Value;

use super::places::places_mut;
use super::{HashingReader, KeepFiles, ShownKind, sha256_hex};
use crate::format::{MAX_BYTES, MAX_IMAGE_BYTES};
use crate::regular_file::{self, OpenError};

/// What a snapshot reads of the files of a session.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The most bytes read of an image file.
    pub image: u64,
    /// The most bytes read of any other file.
    pub text: u64,
}

impl Limits {
    /// The limits the viewers read by.
    pub const VIEWERS: Limits = Limits {
        image: MAX_IMAGE_BYTES,
        text: MAX_BYTES,
    };

    fn for_kind(&self, kind: ShownKind) -> u64 {
        match kind {
            ShownKind::Image => self.image,
            _ => self.text,
        }
    }
}

/// What a snapshot does with the content of a file it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// The bytes are held, so a copy can be made of them later.
    Bytes,
    /// The bytes are hashed as they stream by and not held. This is all
    /// a mode that keeps no copy needs.
    Fingerprint,
}

impl Hold {
    /// What `mode` needs of a file it is shown.
    pub fn for_mode(mode: KeepFiles) -> Hold {
        match mode {
            KeepFiles::Off => Hold::Fingerprint,
            KeepFiles::Compact | KeepFiles::Exact => Hold::Bytes,
        }
    }
}

/// The bytes of file content that snapshots may hold in memory at once. Clones
/// share one count, so every snapshot waiting for the worker draws on the
/// same budget. Claiming never waits: what does not fit is left to be read
/// later by its path.
#[derive(Debug, Clone)]
pub struct MemoryBudget {
    unclaimed: Arc<AtomicU64>,
}

impl MemoryBudget {
    /// What the snapshots of one run hold together: 256 MiB.
    pub const RUN: u64 = 256 * MIB;

    pub fn new(bytes: u64) -> Self {
        MemoryBudget {
            unclaimed: Arc::new(AtomicU64::new(bytes)),
        }
    }

    /// The bytes nobody holds.
    pub fn unclaimed(&self) -> u64 {
        self.unclaimed.load(Ordering::SeqCst)
    }

    /// Takes `bytes` and says whether they were there.
    fn claim(&self, bytes: u64) -> bool {
        self.unclaimed
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(bytes)
            })
            .is_ok()
    }

    fn give_back(&self, bytes: u64) {
        self.unclaimed.fetch_add(bytes, Ordering::SeqCst);
    }
}

/// The part of a [`MemoryBudget`] one snapshot holds. It goes back when the
/// snapshot is dropped, and file by file earlier, as the bytes of a file
/// are processed.
#[derive(Debug)]
pub struct Reservation {
    budget: MemoryBudget,
    bytes: u64,
}

impl Reservation {
    fn none() -> Self {
        Reservation {
            budget: MemoryBudget::new(0),
            bytes: 0,
        }
    }

    /// Returns `bytes` of what is held to the budget.
    pub fn release(&mut self, bytes: u64) {
        let bytes = bytes.min(self.bytes);
        self.bytes -= bytes;
        self.budget.give_back(bytes);
    }

    /// The bytes still held.
    pub fn held(&self) -> u64 {
        self.bytes
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.give_back(self.bytes);
    }
}

const MIB: u64 = 1024 * 1024;

/// A file as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBytes {
    pub bytes: Vec<u8>,
    /// The modification time as RFC 3339 in UTC, when the system tells.
    pub modified: Option<String>,
    pub sha256: String,
}

/// A file as hashed, with its content not held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub size: u64,
    /// The modification time as RFC 3339 in UTC, when the system tells.
    pub modified: Option<String>,
    pub sha256: String,
}

/// What became of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    Read(FileBytes),
    /// Hashed while it streamed by, with no bytes held ([`Hold::Fingerprint`]).
    Hashed(Fingerprint),
    /// Within the limits but past what the memory budget had left.
    Deferred,
    /// Larger than its limit: never read, so not fingerprinted.
    TooLarge {
        size: u64,
        modified: Option<String>,
        reason: String,
    },
    Missing,
    Unreadable(String),
}

/// One place of the session that names a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub kind: ShownKind,
    pub path: String,
    pub seen: Seen,
}

/// The files a session names, in the order the session lists them.
#[derive(Debug)]
pub struct Snapshot {
    pub places: Vec<Place>,
    /// The part of the memory budget the bytes in `places` take.
    pub held: Reservation,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            places: Vec::new(),
            held: Reservation::none(),
        }
    }
}

/// Reads every file the raw `session` names, holding what `hold` says and
/// at most what is left of `budget`.
pub fn snapshot(session: &[u8], limits: &Limits, hold: Hold, budget: &MemoryBudget) -> Snapshot {
    snapshot_with(session, limits, hold, budget, &|_| {})
}

/// [`snapshot`], calling `between` with a file's path after its size was
/// taken and before its bytes are read, so a test can change the file at
/// that moment.
pub(crate) fn snapshot_with(
    session: &[u8],
    limits: &Limits,
    hold: Hold,
    budget: &MemoryBudget,
    between: &dyn Fn(&Path),
) -> Snapshot {
    let Ok(mut raw) = serde_json::from_slice::<Value>(session) else {
        return Snapshot::default();
    };
    let mut held = Reservation {
        budget: budget.clone(),
        bytes: 0,
    };
    let places = places_mut(&mut raw)
        .into_iter()
        .map(|(kind, object)| {
            let path = object["path"]
                .as_str()
                .expect("places hold a string path")
                .to_owned();
            let seen = observe(&path, limits.for_kind(kind), hold, &mut held, between);
            Place { kind, path, seen }
        })
        .collect();
    Snapshot { places, held }
}

impl Place {
    /// Reads the file of a place [`snapshot`] left because the memory budget
    /// was used up, with no budget. Any other place is returned as it is.
    pub fn resolved(mut self, limits: &Limits, hold: Hold) -> Place {
        if self.seen == Seen::Deferred {
            let mut unbudgeted = Reservation {
                budget: MemoryBudget::new(u64::MAX),
                bytes: 0,
            };
            self.seen = observe(
                &self.path,
                limits.for_kind(self.kind),
                hold,
                &mut unbudgeted,
                &|_| {},
            );
        }
        self
    }
}

/// Looks at the file at `path`, and reads it unless it is over `limit`
/// bytes or what `held` can still claim from its budget.
///
/// The file is opened by the rule every viewer of a file follows
/// ([`regular_file::open`]), so a FIFO or a device never blocks the thread
/// this runs on, and a relative path is refused as the viewers refuse it.
/// The size comes from the opened handle. A file whose length differs from
/// it once read has changed under the read, and its bytes match neither
/// state, so it counts as unreadable.
fn observe(
    path: &str,
    limit: u64,
    hold: Hold,
    held: &mut Reservation,
    between: &dyn Fn(&Path),
) -> Seen {
    let path_buf = Path::new(path);
    if path_buf.is_relative() {
        return Seen::Unreadable("relative path".to_owned());
    }
    let file = match regular_file::open(path_buf) {
        Ok(file) => file,
        Err(OpenError::NotRegular { .. }) => {
            return Seen::Unreadable("not a regular file".to_owned());
        }
        Err(OpenError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return Seen::Missing;
        }
        Err(OpenError::Io(error)) => return Seen::Unreadable(error.to_string()),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => return Seen::Unreadable(error.to_string()),
    };
    let size = metadata.len();
    let modified = metadata.modified().ok().and_then(utc_seconds);
    if size > limit {
        return Seen::TooLarge {
            size,
            modified,
            reason: format!("larger than {}", described(limit)),
        };
    }
    match hold {
        Hold::Fingerprint => {
            between(path_buf);
            // Reading one byte past the limit lets a file that grew beyond
            // it show as a length that differs from the size.
            let mut hashing = HashingReader::new(file.take(limit + 1));
            let streamed = match io::copy(&mut hashing, &mut io::sink()) {
                Ok(streamed) => streamed,
                Err(error) => return Seen::Unreadable(error.to_string()),
            };
            if streamed != size {
                return Seen::Unreadable(CHANGED_WHILE_READ.to_owned());
            }
            Seen::Hashed(Fingerprint {
                size,
                modified,
                sha256: hashing.finish(),
            })
        }
        Hold::Bytes => {
            if !held.budget.claim(size) {
                return Seen::Deferred;
            }
            held.bytes += size;
            between(path_buf);
            let mut bytes = Vec::new();
            // Reading one byte past the limit lets a file that grew beyond
            // it show as a length that differs from the size.
            let read = file.take(limit + 1).read_to_end(&mut bytes);
            let failure = match read {
                Err(error) => Some(error.to_string()),
                Ok(_) if bytes.len() as u64 != size => Some(CHANGED_WHILE_READ.to_owned()),
                Ok(_) => None,
            };
            if let Some(reason) = failure {
                held.release(size);
                return Seen::Unreadable(reason);
            }
            Seen::Read(FileBytes {
                sha256: sha256_hex(&bytes),
                bytes,
                modified,
            })
        }
    }
}

const CHANGED_WHILE_READ: &str = "changed while it was read";

/// `limit` in the words of the viewers' refusals: whole mebibytes as
/// such, anything else in bytes.
fn described(limit: u64) -> String {
    if limit != 0 && limit.is_multiple_of(MIB) {
        format!("{} MiB", limit / MIB)
    } else {
        format!("{limit} bytes")
    }
}

/// `time` as RFC 3339 in UTC, to the second.
fn utc_seconds(time: SystemTime) -> Option<String> {
    let timestamp = Timestamp::try_from(time).ok()?;
    Some(
        Timestamp::from_second(timestamp.as_second())
            .ok()?
            .to_string(),
    )
}
