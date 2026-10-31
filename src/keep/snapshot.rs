// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reading the files a session shows, within the limits of the viewers,
//! before the result is written: an agent that cleans up after `asqr wait`
//! must not take away what the person saw (ADR 100).

use std::io::{self, Read};
use std::path::Path;
use std::time::SystemTime;

use jiff::Timestamp;
use serde_json::Value;

use super::places::places_mut;
use super::{ShownKind, sha256_hex};
use crate::format::{MAX_BYTES, MAX_IMAGE_BYTES};
use crate::regular_file::{self, OpenError};

/// What a snapshot reads of the files of a session.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The most bytes read of an image file.
    pub image: u64,
    /// The most bytes read of any other file.
    pub text: u64,
    /// The most bytes of file content a snapshot holds in memory. A file
    /// that does not fit is left for [`Snapshot::resolve_deferred`].
    pub memory: u64,
}

impl Limits {
    /// The limits the viewers read by, and 256 MiB of memory.
    pub const VIEWERS: Limits = Limits {
        image: MAX_IMAGE_BYTES,
        text: MAX_BYTES,
        memory: 256 * MIB,
    };

    fn for_kind(&self, kind: ShownKind) -> u64 {
        match kind {
            ShownKind::Image => self.image,
            _ => self.text,
        }
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

/// What became of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    Read(FileBytes),
    /// Within the limits but past the memory cap.
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub places: Vec<Place>,
}

/// Reads every file the raw `session` names.
pub fn snapshot(session: &[u8], limits: &Limits) -> Snapshot {
    snapshot_with(session, limits, &|_| {})
}

/// [`snapshot`], calling `between` with a file's path after its size was
/// taken and before its bytes are read, so a test can change the file at
/// that moment.
pub(crate) fn snapshot_with(session: &[u8], limits: &Limits, between: &dyn Fn(&Path)) -> Snapshot {
    let Ok(mut raw) = serde_json::from_slice::<Value>(session) else {
        return Snapshot::default();
    };
    let mut memory = limits.memory;
    let places = places_mut(&mut raw)
        .into_iter()
        .map(|(kind, object)| {
            let path = object["path"]
                .as_str()
                .expect("places hold a string path")
                .to_owned();
            let seen = observe(&path, limits.for_kind(kind), Some(&mut memory), between);
            Place { kind, path, seen }
        })
        .collect();
    Snapshot { places }
}

impl Snapshot {
    /// Reads the files [`snapshot`] left because the memory cap was
    /// reached, with no cap.
    pub fn resolve_deferred(&mut self, limits: &Limits) {
        for place in &mut self.places {
            if place.seen == Seen::Deferred {
                place.seen = observe(&place.path, limits.for_kind(place.kind), None, &|_| {});
            }
        }
    }
}

/// Looks at the file at `path`, and reads it unless it is over `limit`
/// bytes or, with a `memory` budget, over what is left of it.
///
/// The file is opened by the rule every viewer of a file follows
/// ([`regular_file::open`]), so a FIFO or a device never blocks the thread
/// this runs on, and a relative path is refused as the viewers refuse it.
/// The size comes from the opened handle. A file whose length differs from
/// it once read has changed under the read, and its bytes match neither
/// state, so it counts as unreadable.
fn observe(path: &str, limit: u64, memory: Option<&mut u64>, between: &dyn Fn(&Path)) -> Seen {
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
    if let Some(memory) = memory {
        if size > *memory {
            return Seen::Deferred;
        }
        *memory -= size;
    }
    between(path_buf);
    // Reading one byte past the limit lets a file that grew beyond it show
    // as a length that differs from the size.
    let mut bytes = Vec::new();
    if let Err(error) = file.take(limit + 1).read_to_end(&mut bytes) {
        return Seen::Unreadable(error.to_string());
    }
    if bytes.len() as u64 != size {
        return Seen::Unreadable("changed while it was read".to_owned());
    }
    Seen::Read(FileBytes {
        sha256: sha256_hex(&bytes),
        bytes,
        modified,
    })
}

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
