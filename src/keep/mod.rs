// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Keeping the files a finished session showed (ADR 100). A session names
//! files by path, many of them in temporary folders, so an archived answer
//! means nothing once the file is gone. This module decides what is kept
//! of each file.

pub mod apply;
mod places;
pub mod processor;
pub mod record;
pub mod snapshot;
pub mod store;
pub mod worker;

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use std::fmt::{self, Write as _};
use std::io::{self, Read};
use std::str::FromStr;

use clap::ValueEnum;
use sha2::{Digest, Sha256};

/// The SHA-256 of `bytes` as 64 lowercase hex digits, what `shasum -a 256`
/// prints and what names a kept file.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The SHA-256 of everything `reader` yields, as [`sha256_hex`] prints it,
/// without holding the content in memory.
pub fn sha256_hex_of_reader(reader: impl Read) -> io::Result<String> {
    let mut hashing = HashingReader::new(reader);
    io::copy(&mut hashing, &mut io::sink())?;
    Ok(hashing.finish())
}

/// A reader that hashes what passes through it.
pub struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
}

impl<R: Read> HashingReader<R> {
    pub fn new(inner: R) -> Self {
        HashingReader {
            inner,
            hasher: Sha256::new(),
        }
    }

    /// The SHA-256 of everything read so far, as lowercase hex.
    pub fn finish(self) -> String {
        hex(&self.hasher.finalize())
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

/// `bytes` as lowercase hex digits.
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            write!(text, "{byte:02x}").expect("writing to a String cannot fail");
            text
        })
}

/// What asqr keeps of the files a session showed (spec section 5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum KeepFiles {
    /// Images as WebP copies, every other file as it is.
    #[default]
    Compact,
    /// Every file byte for byte, images included.
    Exact,
    /// No file. The size and checksum of each are still recorded.
    Off,
}

impl fmt::Display for KeepFiles {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.to_possible_value().expect("no variant is skipped");
        out.write_str(value.get_name())
    }
}

impl FromStr for KeepFiles {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        <Self as ValueEnum>::from_str(text, false)
    }
}

/// What a shown file is for, which decides how it is processed. The kinds
/// are those the viewers know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShownKind {
    Image,
    Code,
    Markdown,
    Ansi,
    DiffSide,
}
