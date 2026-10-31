// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compaction (spec sections 5.1 and 5.9): sessions and results older
//! than 30 days go from loose files into the month bundles of the
//! archive. Nothing is deleted but the loose file of an entry that is
//! proven to be inside its bundle.
//!
//! The asqr that watches a queue runs it when it starts, and
//! `asqr compact` runs it by hand, so the caller holds the queue lock. The
//! bundle of a month is built without any lock, from files that are
//! written once; the drafts lock covers only the check that nothing
//! changed since, the rename and the removal of the loose files.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::names::{Archived, entries_if_present, parse_archive_name};
use super::{
    Added, DRAFTS_LOCK_ATTEMPTS, EntryKind, EntryName, InboxName, Month, NewEntry, QueueLocation,
    QueueLock, STALE_TEMP_AGE, classify_inbox_name, find_session_file, finished_at, lock_drafts,
    prepare_bundle, read_if_present, result_entry_name,
};
use crate::format::same_session_id;

/// How old a session or result must be before it is bundled.
pub const COMPACT_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// What one month's bundle took in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthReport {
    pub month: Month,
    pub sessions: usize,
    pub results: usize,
    pub replaced: usize,
}

/// A month, or a file, that was left as it was, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub month: Option<Month>,
    pub text: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CompactReport {
    pub bundled: Vec<MonthReport>,
    /// Results that stay in the outbox because a session of their id waits
    /// in the inbox; recovery decides what happens to them (spec section
    /// 5.7).
    pub for_recovery: Vec<String>,
    pub problems: Vec<Problem>,
}

/// One loose file due to be bundled.
struct Candidate {
    path: PathBuf,
    name: EntryName,
    in_outbox: bool,
}

/// Bundles what is due in the queue at `location`, judged at `now`. The
/// lock is the queue lock, which keeps another compaction away.
pub fn compact(location: &QueueLocation, lock: &QueueLock, now: SystemTime) -> CompactReport {
    compact_with(location, lock, now, DRAFTS_LOCK_ATTEMPTS, &mut |_| {})
}

/// [`compact`] that tries the drafts lock `lock_attempts` times, and calls
/// `between` for each month after its bundle is written and before the
/// files are checked again. Tests change files there, as a `prune` or an
/// asker might.
pub fn compact_with(
    location: &QueueLocation,
    _lock: &QueueLock,
    now: SystemTime,
    lock_attempts: u32,
    between: &mut dyn FnMut(Month),
) -> CompactReport {
    let mut report = CompactReport::default();
    remove_stale_temps(location, now);
    let cutoff = now
        .checked_sub(COMPACT_AFTER)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut due = match gather(location, cutoff, &mut report) {
        Ok(due) => due,
        Err(error) => {
            report.problems.push(Problem {
                month: None,
                text: format!("cannot list the queue: {error}"),
            });
            return report;
        }
    };
    for (month, candidates) in std::mem::take(&mut due) {
        match bundle_month(
            location,
            month,
            candidates,
            now,
            lock_attempts,
            between,
            &mut report.problems,
        ) {
            Ok(Some(done)) => report.bundled.push(done),
            Ok(None) => {}
            Err(text) => report.problems.push(Problem {
                month: Some(month),
                text,
            }),
        }
    }
    report
}

/// Compacts as the asqr that starts watching a queue does: what happens
/// is logged, and nothing stops the start.
pub fn compact_quietly(location: &QueueLocation, lock: &QueueLock) {
    let report = compact(location, lock, SystemTime::now());
    for done in &report.bundled {
        tracing::info!(
            month = %done.month,
            sessions = done.sessions,
            results = done.results,
            replaced = done.replaced,
            "bundled old entries"
        );
    }
    for problem in &report.problems {
        tracing::warn!(month = ?problem.month.map(|month| month.to_string()), text = %problem.text, "could not bundle");
    }
}

/// Removes temp files a writer left in `archive/`: those last written
/// more than [`STALE_TEMP_AGE`] ago. A writer in progress has a temp file
/// of seconds, so it is never touched.
fn remove_stale_temps(location: &QueueLocation, now: SystemTime) {
    let Ok(entries) = entries_if_present(&location.archive()) else {
        return;
    };
    let limit = now
        .checked_sub(STALE_TEMP_AGE)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        let stale = name.starts_with(".asqr-")
            && name.ends_with(".tmp")
            && entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified < limit);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The ids of the sessions waiting in the inbox.
fn waiting_ids(location: &QueueLocation) -> io::Result<Vec<String>> {
    Ok(entries_if_present(&location.inbox())?
        .iter()
        .filter_map(
            |entry| match classify_inbox_name(entry.file_name().to_str()?) {
                InboxName::Session(id) => Some(id.to_owned()),
                _ => None,
            },
        )
        .collect())
}

/// The files due, by the month of their entry.
fn gather(
    location: &QueueLocation,
    cutoff: SystemTime,
    report: &mut CompactReport,
) -> io::Result<BTreeMap<Month, Vec<Candidate>>> {
    let mut due: BTreeMap<Month, Vec<Candidate>> = BTreeMap::new();
    for entry in entries_if_present(&location.archive())? {
        let Some(parsed) = entry.file_name().to_str().and_then(parse_archive_name) else {
            continue;
        };
        if parsed.ulid.datetime() >= cutoff {
            continue;
        }
        let name = EntryName {
            kind: match parsed.kind {
                Archived::Session => EntryKind::Session,
                Archived::Result => EntryKind::Replaced,
            },
            id: parsed.id,
            ulid: parsed.ulid,
        };
        due.entry(Month::of(name.ulid))
            .or_default()
            .push(Candidate {
                path: entry.path(),
                name,
                in_outbox: false,
            });
    }
    for entry in entries_if_present(&location.outbox())? {
        let file = entry.file_name();
        let Some(InboxName::Session(id)) = file.to_str().map(classify_inbox_name) else {
            continue;
        };
        // A file that has gone since the listing is skipped: asqr or an
        // asker moved it, and the next run looks again.
        let modified =
            match std::fs::metadata(entry.path()).and_then(|metadata| metadata.modified()) {
                Ok(modified) => modified,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
        let Some(bytes) = read_if_present(&entry.path())? else {
            continue;
        };
        let finished = finished_at(&bytes, modified);
        if finished >= cutoff {
            continue;
        }
        if find_session_file(&location.inbox(), id)?.is_some() {
            report.for_recovery.push(id.to_owned());
            continue;
        }
        let name = result_entry_name(id, &bytes, finished);
        due.entry(Month::of(name.ulid))
            .or_default()
            .push(Candidate {
                path: entry.path(),
                name,
                in_outbox: true,
            });
    }
    Ok(due)
}

/// Bundles the candidates of one month. `Ok(None)` when nothing was
/// bundled, `Err` with the reason when the month was left for the next
/// run. A file whose name the bundle holds with other content stays where
/// it is and is named in `problems`; the others go on.
fn bundle_month(
    location: &QueueLocation,
    month: Month,
    candidates: Vec<Candidate>,
    now: SystemTime,
    lock_attempts: u32,
    between: &mut dyn FnMut(Month),
    problems: &mut Vec<Problem>,
) -> Result<Option<MonthReport>, String> {
    // A file that has gone since the listing was bundled or deleted by
    // someone else; the next run looks again.
    let mut loaded = Vec::new();
    for candidate in candidates {
        match read_if_present(&candidate.path) {
            Ok(Some(bytes)) => loaded.push((candidate, bytes)),
            Ok(None) => {}
            Err(error) => {
                return Err(format!("cannot read {}: {error}", candidate.path.display()));
            }
        }
    }
    if loaded.is_empty() {
        return Ok(None);
    }
    let entries: Vec<NewEntry> = loaded
        .iter()
        .map(|(candidate, bytes)| NewEntry {
            name: candidate.name.clone(),
            bytes: bytes.clone(),
        })
        .collect();
    let prepared =
        prepare_bundle(location, month, &entries, now).map_err(|error| error.to_string())?;
    between(month);

    let outcomes = prepared.outcomes().to_vec();
    let mut bundled = Vec::new();
    for (loaded, outcome) in loaded.iter().zip(&outcomes) {
        if *outcome == Added::Conflict {
            problems.push(Problem {
                month: Some(month),
                text: format!(
                    "{} stays: the bundle holds other content under its name",
                    loaded.0.path.display()
                ),
            });
        } else {
            bundled.push(loaded);
        }
    }

    if bundled.is_empty() {
        return Ok(None);
    }

    // From here on the drafts lock is held: `prune` deletes bundles and
    // loose entries under it, and askers move outbox results under it.
    let _lock = lock_drafts(location, lock_attempts)
        .map_err(|error| format!("the queue is busy: {error}"))?;
    // The inbox is read once for all candidates, not once for each.
    let waiting = waiting_ids(location).map_err(|error| error.to_string())?;
    let unchanged = prepared
        .still_current()
        .map_err(|error| error.to_string())?
        && bundled
            .iter()
            .try_fold(true, |all, (candidate, bytes)| {
                Ok::<_, io::Error>(
                    all && read_if_present(&candidate.path)?.as_deref() == Some(bytes.as_slice())
                        && !(candidate.in_outbox
                            && waiting
                                .iter()
                                .any(|id| same_session_id(id, &candidate.name.id))),
                )
            })
            .map_err(|error| error.to_string())?;
    if !unchanged {
        return Err(
            "a file changed while the bundle was written; left for the next run".to_owned(),
        );
    }
    prepared.commit().map_err(|error| error.to_string())?;
    let mut done = MonthReport {
        month,
        sessions: 0,
        results: 0,
        replaced: 0,
    };
    for (candidate, _) in bundled {
        match std::fs::remove_file(&candidate.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot remove {}: {error}",
                    candidate.path.display()
                ));
            }
        }
        match candidate.name.kind {
            EntryKind::Session => done.sessions += 1,
            EntryKind::Result => done.results += 1,
            EntryKind::Replaced => done.replaced += 1,
        }
    }
    Ok(Some(done))
}

#[cfg(test)]
mod tests;
