// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What the screen knows about a replayed session besides the session
//! itself (ADR 101): its reference, how it ended and what became of the
//! files it showed.

use std::collections::HashMap;
use std::time::SystemTime;

use jiff::tz::TimeZone;

use crate::format::ContextEntry;
use crate::queue::{HistoryOutcome, ReplayFile};

/// A finished session shown again.
#[derive(Debug, Clone)]
pub struct ReplayInfo {
    /// What a person types to name the entry (the last 8 characters of its
    /// archive ULID).
    pub reference: String,
    pub outcome: HistoryOutcome,
    /// When it was finished, as the status line writes it.
    pub finished: String,
    /// Each shown file, by the path the session names it at.
    pub files: HashMap<String, ReplayFile>,
    /// The month bundle the session was read from, by its file name;
    /// `None` for a loose session.
    pub bundle: Option<String>,
    /// The zone the times of the files show in.
    pub zone: TimeZone,
}

impl ReplayInfo {
    /// What the archive recorded about the file the session names at
    /// `path`.
    pub fn file_at(&self, path: &str) -> Option<&ReplayFile> {
        self.files.get(path)
    }
}

/// The path of the file a context entry shows, where it shows one. A diff
/// shows two and names none.
pub fn entry_path(entry: &ContextEntry) -> Option<&str> {
    match entry {
        ContextEntry::Image(image) => Some(&image.path),
        ContextEntry::Code(code) => code.path.as_deref(),
        ContextEntry::Markdown(markdown) => markdown.path.as_deref(),
        ContextEntry::Ansi(ansi) => ansi.path.as_deref(),
        ContextEntry::Diff(_) => None,
    }
}

/// `size` in bytes as a short text with decimal units: `312 B`, `1.4 KB`,
/// `213 KB`, `70 MB`.
pub fn human_size(size: u64) -> String {
    if size < 1_000 {
        return format!("{size} B");
    }
    let mut value = size as f64;
    let mut unit = "B";
    for next in ["KB", "MB", "GB"] {
        if value < 1_000.0 {
            break;
        }
        value /= 1_000.0;
        unit = next;
    }
    if value < 10.0 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

/// A SHA-256 as its first 16 and last 8 digits around `…`.
pub fn short_sha(sha: &str) -> String {
    if sha.len() <= 24 {
        return sha.to_owned();
    }
    format!("{}…{}", &sha[..16], &sha[sha.len() - 8..])
}

/// `modified`, an RFC 3339 time, as the replay writes times in `zone`;
/// the text itself where it is no time.
pub fn modified_text(modified: &str, zone: &TimeZone) -> String {
    modified.parse::<jiff::Timestamp>().map_or_else(
        |_| modified.to_owned(),
        |time| replay_time_text(SystemTime::from(time), zone),
    )
}

/// `time` as the replay's status line writes it, in `zone`: `Thu 29 Oct
/// 16:41`.
pub fn replay_time_text(time: SystemTime, zone: &TimeZone) -> String {
    jiff::Timestamp::try_from(time)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(zone.clone())
        .strftime("%a %d %b %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_decimal_units_and_one_decimal_below_ten() {
        assert_eq!(human_size(312), "312 B");
        assert_eq!(human_size(1_420), "1.4 KB");
        assert_eq!(human_size(213_456), "213 KB");
        assert_eq!(human_size(70_000_000), "70 MB");
    }

    #[test]
    fn a_fingerprint_is_shortened_to_its_ends_and_a_short_one_is_kept() {
        let sha = "9f2c61d0b7e3a4c8d1e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7e2b41a6";
        assert_eq!(short_sha(sha), "9f2c61d0b7e3a4c8…7e2b41a6");
        assert_eq!(short_sha("abc123"), "abc123");
    }

    #[test]
    fn a_modification_time_shows_in_the_zone_and_a_text_that_is_no_time_as_it_is() {
        assert_eq!(
            modified_text("2026-10-29T16:40:11Z", &TimeZone::UTC),
            "Thu 29 Oct 16:40"
        );
        assert_eq!(modified_text("yesterday", &TimeZone::UTC), "yesterday");
    }
}
