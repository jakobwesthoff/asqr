// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What the screen knows about a replayed session besides the session
//! itself (ADR 101): its reference, how it ended and what became of the
//! files it showed.

use std::collections::HashMap;
use std::time::SystemTime;

use jiff::tz::TimeZone;

use crate::queue::{HistoryOutcome, ReplayFile};

/// A finished session shown again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayInfo {
    /// What a person types to name the entry (the last 8 characters of its
    /// archive ULID).
    pub reference: String,
    pub outcome: HistoryOutcome,
    /// When it was finished, as the status line writes it.
    pub finished: String,
    /// Each shown file, by the path the session names it at.
    pub files: HashMap<String, ReplayFile>,
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
