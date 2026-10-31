// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The finished sessions of a queue (spec section 5.1): the list `asqr
//! history` prints and the reference by which `asqr replay` and `asqr
//! prune --session` name one of them.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ulid::Ulid;

use super::bundle::{
    BundleError, BundleReader, EntryKind, Month, finished_at, list_entries, list_months,
};
use super::names::{
    Archived, INVALID_STEM_ID, InboxName, classify_inbox_name, entries_if_present,
    parse_archive_name, read_if_present,
};
use super::{QueueLocation, list_queues, session_sha256};
use crate::format::{SessionResult, Status, same_session_id};
use crate::keep::record::shown_states;

/// How many trailing characters of the ULID make the reference, and the
/// fewest a person may type of them. Eight characters are 40 random bits;
/// six are 30, so two entries can end alike and are then refused as
/// ambiguous.
const REFERENCE_LENGTH: usize = 8;
const REFERENCE_MINIMUM: usize = 6;

/// Seconds a result may be later than the archive time of the session it
/// answers. A result is written before its session is archived, but its
/// `submitted_at` and the ULID come from two clocks reads.
const PAIRING_SLACK: Duration = Duration::from_secs(5);

/// Where the archive holds a finished session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryPlace {
    /// A loose file `archive/<id>.<ulid>.json`.
    Loose(PathBuf),
    /// The entry of that name inside the bundle of a month.
    Bundled(Month, String),
}

/// A finished session as the archive names it, without its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRef {
    pub queue: QueueLocation,
    pub id: String,
    /// The ULID in the archive name, unique per entry. Its time is when
    /// the session was archived.
    pub ulid: Ulid,
    pub place: EntryPlace,
}

impl EntryRef {
    /// What a person types to name this entry: the last eight characters
    /// of its ULID, which are random bits.
    pub fn reference(&self) -> String {
        let text = self.ulid.to_string();
        text[text.len() - REFERENCE_LENGTH..].to_owned()
    }
}

/// How a finished session ended, as the list words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOutcome {
    Sent,
    Rejected,
    Error,
    Withdrawn,
    /// No result answers the session.
    NoResult,
}

/// What the archived session recorded about the files it showed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileCounts {
    pub kept: usize,
    /// Missing or unreadable when the session was finished.
    pub gone: usize,
    pub not_kept: usize,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub entry: EntryRef,
    pub title: Option<String>,
    /// When the result was written, else when the session was archived.
    pub finished: SystemTime,
    pub outcome: HistoryOutcome,
    pub files: FileCounts,
    /// A withdrawn session whose unsent draft the result carries.
    pub has_draft: bool,
}

/// The finished sessions of one queue.
#[derive(Debug, Default)]
pub struct History {
    /// Newest first, cut to the limit.
    pub entries: Vec<HistoryEntry>,
    /// The sessions that match `since`, before the limit cut the list.
    pub total: usize,
    /// The bundles that could not be read.
    pub unreadable: Vec<BundleError>,
}

/// Everything a queue holds that the list needs, known by name alone.
struct Scan {
    /// Finished sessions, newest first, each ULID once.
    sessions: Vec<EntryRef>,
    results: Vec<ResultRef>,
    unreadable: Vec<BundleError>,
}

/// A result file or entry that may answer a session.
struct ResultRef {
    id: String,
    place: ResultPlace,
}

enum ResultPlace {
    Outbox(PathBuf),
    /// A result `asqr ask --force` moved aside: `archive/*.result.json`.
    Aside(PathBuf),
    /// A `results/` or `replaced/` entry of a bundle, with the ULID of its
    /// name, which is the time the result was finished.
    Bundled(Month, String, Ulid),
}

/// Lists the queue by file and entry names, reading no content. Loose
/// files come before bundles, so an entry that moves into a bundle while
/// this runs is seen twice at worst and never zero times (module docs of
/// the bundles).
fn scan(location: &QueueLocation) -> io::Result<Scan> {
    let mut sessions: Vec<EntryRef> = Vec::new();
    let mut results = Vec::new();
    let mut seen: HashSet<Ulid> = HashSet::new();
    let session_ref = |id: String, ulid: Ulid, place: EntryPlace| EntryRef {
        queue: location.clone(),
        id,
        ulid,
        place,
    };

    for entry in entries_if_present(&location.outbox())? {
        if let Some(InboxName::Session(id)) = entry.file_name().to_str().map(classify_inbox_name) {
            results.push(ResultRef {
                id: id.to_owned(),
                place: ResultPlace::Outbox(entry.path()),
            });
        }
    }
    for entry in entries_if_present(&location.archive())? {
        let Some(parsed) = entry.file_name().to_str().and_then(parse_archive_name) else {
            continue;
        };
        match parsed.kind {
            Archived::Session if !same_session_id(&parsed.id, INVALID_STEM_ID) => {
                if seen.insert(parsed.ulid) {
                    sessions.push(session_ref(
                        parsed.id,
                        parsed.ulid,
                        EntryPlace::Loose(entry.path()),
                    ));
                }
            }
            Archived::Session => {}
            Archived::Result => results.push(ResultRef {
                id: parsed.id,
                place: ResultPlace::Aside(entry.path()),
            }),
        }
    }

    let mut unreadable = Vec::new();
    for month in list_months(location).map_err(|error| io::Error::other(error.to_string()))? {
        let entries = match list_entries(location, month) {
            Ok(entries) => entries,
            Err(error) => {
                unreadable.push(error);
                continue;
            }
        };
        for entry in entries {
            let Some(parsed) = entry.parsed else { continue };
            match parsed.kind {
                EntryKind::Session if !same_session_id(&parsed.id, INVALID_STEM_ID) => {
                    if seen.insert(parsed.ulid) {
                        sessions.push(session_ref(
                            parsed.id,
                            parsed.ulid,
                            EntryPlace::Bundled(month, entry.name),
                        ));
                    }
                }
                EntryKind::Session => {}
                EntryKind::Result | EntryKind::Replaced => results.push(ResultRef {
                    id: parsed.id,
                    place: ResultPlace::Bundled(month, entry.name, parsed.ulid),
                }),
            }
        }
    }

    sessions.sort_by_key(|entry| Reverse(entry.ulid));
    Ok(Scan {
        sessions,
        results,
        unreadable,
    })
}

/// A result read for pairing.
struct Candidate {
    id: String,
    finished: SystemTime,
    result: SessionResult,
    taken: bool,
}

/// Bundles opened while one list is built, each once.
#[derive(Default)]
struct Readers {
    open: HashMap<Month, Option<BundleReader>>,
}

impl Readers {
    /// The entry `name` of the bundle of `month`. A bundle that cannot be
    /// opened or an entry that cannot be read is pushed to `unreadable`
    /// once per failure and gives `None`.
    fn read(
        &mut self,
        location: &QueueLocation,
        month: Month,
        name: &str,
        unreadable: &mut Vec<BundleError>,
    ) -> Option<Vec<u8>> {
        let reader = self.open.entry(month).or_insert_with(|| {
            BundleReader::open(location, month)
                .map_err(|error| unreadable.push(error))
                .ok()
        });
        reader
            .as_mut()?
            .read(name)
            .map_err(|error| unreadable.push(error))
            .ok()
    }
}

/// The finished sessions of `location`, newest first.
///
/// `since` keeps the sessions archived at or after it; `limit` cuts the
/// list after that, and `total` counts the sessions before the cut. The
/// cut and the filter go by the ULIDs in the names, so only the entries
/// that are listed are read.
pub fn list_history(
    location: &QueueLocation,
    limit: Option<usize>,
    since: Option<SystemTime>,
) -> io::Result<History> {
    let Scan {
        sessions,
        results,
        mut unreadable,
    } = scan(location)?;
    let mut sessions: Vec<EntryRef> = sessions
        .into_iter()
        .filter(|entry| since.is_none_or(|since| entry.ulid.datetime() >= since))
        .collect();
    let total = sessions.len();
    sessions.truncate(limit.unwrap_or(usize::MAX));

    let mut readers = Readers::default();
    let contents: Vec<SessionContent> = sessions
        .iter()
        .map(|entry| {
            let bytes = match &entry.place {
                EntryPlace::Loose(path) => std::fs::read(path).ok(),
                EntryPlace::Bundled(month, name) => {
                    readers.read(location, *month, name, &mut unreadable)
                }
            };
            SessionContent::of(bytes.as_deref())
        })
        .collect();

    // Only the results of the ids on the list can pair with a session.
    let wanted: HashSet<String> = sessions
        .iter()
        .map(|entry| entry.id.to_ascii_lowercase())
        .collect();
    let mut candidates: Vec<Candidate> = Vec::new();
    for reference in results
        .iter()
        .filter(|reference| wanted.contains(&reference.id.to_ascii_lowercase()))
    {
        let read = match &reference.place {
            ResultPlace::Outbox(path) | ResultPlace::Aside(path) => {
                let modified = std::fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                read_if_present(path)
                    .ok()
                    .flatten()
                    .map(|bytes| (bytes, modified))
            }
            ResultPlace::Bundled(month, name, ulid) => readers
                .read(location, *month, name, &mut unreadable)
                .map(|bytes| (bytes, ulid.datetime())),
        };
        let Some((bytes, modified)) = read else {
            continue;
        };
        if let Ok(result) = serde_json::from_slice::<SessionResult>(&bytes) {
            candidates.push(Candidate {
                id: reference.id.to_ascii_lowercase(),
                finished: finished_at(&bytes, modified),
                result,
                taken: false,
            });
        }
    }

    // The oldest session picks first, so two sessions of identical bytes
    // each get the result finished before them rather than both the newest.
    let mut paired: Vec<Option<usize>> = vec![None; sessions.len()];
    let mut oldest_first: Vec<usize> = (0..sessions.len()).collect();
    oldest_first.sort_by_key(|index| sessions[*index].ulid);
    for index in oldest_first {
        let id = sessions[index].id.to_ascii_lowercase();
        let archived_at = sessions[index].ulid.datetime() + PAIRING_SLACK;
        let content = &contents[index];
        let chosen = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                !candidate.taken
                    && candidate.id == id
                    && candidate.finished <= archived_at
                    && candidate
                        .result
                        .session_sha256
                        .as_deref()
                        .is_some_and(|sha| content.hashes.iter().any(|own| own == sha))
            })
            .max_by_key(|(_, candidate)| candidate.finished)
            .map(|(position, _)| position);
        if let Some(position) = chosen {
            candidates[position].taken = true;
            paired[index] = Some(position);
        }
    }

    let entries = sessions
        .into_iter()
        .zip(contents)
        .zip(paired)
        .map(|((entry, content), paired)| {
            let result = paired.map(|position| &candidates[position]);
            HistoryEntry {
                finished: result.map_or_else(|| entry.ulid.datetime(), |found| found.finished),
                outcome: result.map_or(HistoryOutcome::NoResult, |found| {
                    outcome_of(found.result.status)
                }),
                has_draft: result.is_some_and(|found| {
                    found.result.status == Status::Withdrawn && found.result.draft.is_some()
                }),
                title: content.title,
                files: content.files,
                entry,
            }
        })
        .collect();
    Ok(History {
        entries,
        total,
        unreadable,
    })
}

fn outcome_of(status: Status) -> HistoryOutcome {
    match status {
        Status::Submitted => HistoryOutcome::Sent,
        Status::Cancelled => HistoryOutcome::Rejected,
        Status::Error => HistoryOutcome::Error,
        Status::Withdrawn => HistoryOutcome::Withdrawn,
        // A draft never leaves `drafts/`, so no result answering a session
        // is one.
        Status::Draft => HistoryOutcome::NoResult,
    }
}

/// What the list reads out of an archived session.
#[derive(Default)]
struct SessionContent {
    title: Option<String>,
    files: FileCounts,
    /// The hashes a result may name: the file's own bytes, and the hash of
    /// the inbox bytes that `archived.session_sha256` records once the
    /// kept files replaced the file.
    hashes: Vec<String>,
}

impl SessionContent {
    fn of(bytes: Option<&[u8]>) -> SessionContent {
        let Some(bytes) = bytes else {
            return SessionContent::default();
        };
        let raw: Option<serde_json::Value> = serde_json::from_slice(bytes).ok();
        let text = |value: Option<&serde_json::Value>| {
            value.and_then(serde_json::Value::as_str).map(str::to_owned)
        };
        let mut hashes = vec![session_sha256(bytes)];
        hashes.extend(text(
            raw.as_ref()
                .and_then(|raw| raw["archived"].get("session_sha256")),
        ));
        let mut files = FileCounts::default();
        for state in shown_states(bytes).into_iter().flatten() {
            match state.as_str() {
                "kept" => files.kept += 1,
                "missing" | "unreadable" => files.gone += 1,
                "not_kept" => files.not_kept += 1,
                _ => {}
            }
        }
        SessionContent {
            title: text(raw.as_ref().and_then(|raw| raw.get("title"))),
            files,
            hashes,
        }
    }
}

/// What a reference text names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Found(EntryRef),
    /// Fewer than six characters.
    TooShort,
    /// More than eight characters.
    TooLong,
    /// A character no ULID holds: ULIDs use Crockford's alphabet, without
    /// `I`, `L`, `O` and `U`.
    NotAReference(char),
    NoMatch,
    /// Several entries end in the text.
    Ambiguous(Vec<EntryRef>),
}

/// A resolution and the bundles the search could not read, one of which
/// may hold the entry.
#[derive(Debug)]
pub struct Resolved {
    pub resolution: Resolution,
    pub unreadable: Vec<BundleError>,
}

/// The characters of a ULID.
const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Finds the entry `text` names in the default queue and every named
/// queue under `root`, and in `extra`, a queue given as a directory. The
/// ULID is part of the name of a file or bundle entry, so no content is
/// read.
pub fn resolve_reference(
    root: &Path,
    extra: Option<&QueueLocation>,
    text: &str,
) -> io::Result<Resolved> {
    let resolved = |resolution| Resolved {
        resolution,
        unreadable: Vec::new(),
    };
    let length = text.chars().count();
    if length < REFERENCE_MINIMUM {
        return Ok(resolved(Resolution::TooShort));
    }
    if length > REFERENCE_LENGTH {
        return Ok(resolved(Resolution::TooLong));
    }
    let wanted = text.to_ascii_uppercase();
    if let Some(stray) = wanted.chars().find(|letter| !CROCKFORD.contains(*letter)) {
        return Ok(resolved(Resolution::NotAReference(stray)));
    }

    let mut queues: Vec<QueueLocation> = list_queues(root)?
        .into_iter()
        .map(|summary| summary.location)
        .collect();
    if let Some(extra) = extra
        && !queues.iter().any(|queue| queue.same_queue(extra))
    {
        queues.push(extra.clone());
    }

    let mut matches: Vec<EntryRef> = Vec::new();
    let mut unreadable = Vec::new();
    for queue in &queues {
        let scanned = scan(queue)?;
        unreadable.extend(scanned.unreadable);
        matches.extend(
            scanned
                .sessions
                .into_iter()
                .filter(|entry| entry.ulid.to_string().ends_with(&wanted)),
        );
    }
    matches.sort_by_key(|entry| Reverse(entry.ulid));
    let resolution = match matches.len() {
        0 => Resolution::NoMatch,
        1 => Resolution::Found(matches.remove(0)),
        _ => Resolution::Ambiguous(matches),
    };
    Ok(Resolved {
        resolution,
        unreadable,
    })
}

#[cfg(test)]
mod tests;
