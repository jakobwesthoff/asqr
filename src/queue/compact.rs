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

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::names::{Archived, entries_if_present, parse_archive_name};
use super::{
    Added, DRAFTS_LOCK_ATTEMPTS, EntryKind, EntryName, InboxName, Month, NewEntry, Probed,
    QueueLocation, QueueLock, STALE_TEMP_AGE, classify_inbox_name, find_session_file, finished_at,
    list_entries, lock_drafts, prepare_bundle, probe, read_entry, read_if_present,
    result_entry_name,
};
use crate::format::same_session_id;
use crate::keep::record::kept_file_names;
use crate::keep::sha256_hex_of_reader;

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

/// Removes temp files a writer left in `archive/` and `archive/files/`:
/// those last written more than [`STALE_TEMP_AGE`] ago. A writer in
/// progress has a temp file of seconds, so it is never touched.
fn remove_stale_temps(location: &QueueLocation, now: SystemTime) {
    let limit = now
        .checked_sub(STALE_TEMP_AGE)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    remove_stale_temps_in(&location.archive(), limit);
    remove_stale_temps_in(&location.kept_files(), limit);
}

fn remove_stale_temps_in(dir: &Path, limit: SystemTime) {
    let Ok(entries) = entries_if_present(dir) else {
        return;
    };
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
    let kept = plan_kept_files(location, month, &loaded, problems)?;
    let loaded: Vec<_> = loaded
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !kept.held_back.contains(index))
        .map(|(_, loaded)| loaded)
        .collect();
    let mut entries: Vec<NewEntry> = loaded
        .iter()
        .map(|(candidate, bytes)| NewEntry::named(candidate.name.clone(), bytes.clone()))
        .collect();
    entries.extend(kept.entries);
    let prepared =
        prepare_bundle(location, month, &entries, now).map_err(|error| error.to_string())?;
    between(month);

    let outcomes = prepared.outcomes().to_vec();
    // The kept files come after the sessions and results. One that the
    // bundle holds with other content since the probe is left for the next
    // run, as nothing is committed yet.
    if outcomes[loaded.len()..].contains(&Added::Conflict) {
        return Err("the bundle changed while it was written; left for the next run".to_owned());
    }
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
    // Taken before the loose sessions go: the kept files they showed are
    // removed only if nothing loose shows them afterwards.
    let shown: BTreeSet<String> = bundled
        .iter()
        .filter(|(candidate, _)| candidate.name.kind == EntryKind::Session)
        .flat_map(|(_, bytes)| kept_file_names(bytes))
        .filter(|name| kept.intact.contains(name))
        .collect();
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
    remove_bundled_kept_files(location, month, &shown, problems);
    Ok(Some(done))
}

// =========================================================================
// Kept files
// =========================================================================

/// What a month's bundle needs to take in for the kept files its sessions
/// show.
#[derive(Default)]
struct KeptFiles {
    /// The files to add to the bundle.
    entries: Vec<NewEntry>,
    /// Names of loose files that are intact and are, or will be, in the
    /// bundle.
    intact: HashSet<String>,
    /// Indices into the candidates of sessions that stay loose because the
    /// bundle holds other content under the name of a file they show.
    held_back: HashSet<usize>,
}

/// The state of a loose kept file.
enum Loose {
    Intact,
    Missing,
    /// Cannot be read, or does not hash to its name.
    Damaged(String),
}

/// Reads the loose kept file `name` through, as the bundle will, to see
/// whether it still hashes to its name.
fn check_loose(path: &Path, name: &str) -> Loose {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Loose::Missing,
        Err(error) => return Loose::Damaged(error.to_string()),
    };
    match sha256_hex_of_reader(file) {
        Ok(hash) if name.starts_with(&hash) => Loose::Intact,
        Ok(_) => Loose::Damaged("it does not hash to its name".to_owned()),
        Err(error) => Loose::Damaged(error.to_string()),
    }
}

/// Decides what happens to the kept files that the sessions of `month`
/// show. A file that is missing or damaged on disk is not bundled, and the
/// sessions that show it are bundled anyway: they are a record that stays
/// true. A file the bundle holds with other content cannot be bundled
/// either, and its sessions stay loose so no bundle holds a session with a
/// file it contradicts.
///
/// Files that sessions already in the bundle show and the bundle lacks are
/// added too: an asqr that knew no kept files bundled those sessions.
fn plan_kept_files(
    location: &QueueLocation,
    month: Month,
    loaded: &[(Candidate, Vec<u8>)],
    problems: &mut Vec<Problem>,
) -> Result<KeptFiles, String> {
    let mut shown_by: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, (candidate, bytes)) in loaded.iter().enumerate() {
        if candidate.name.kind == EntryKind::Session {
            for name in kept_file_names(bytes) {
                shown_by.entry(name).or_default().push(index);
            }
        }
    }
    let strays = bundled_strays(location, month, None);
    let wanted: Vec<String> = shown_by
        .keys()
        .cloned()
        .chain(
            strays
                .iter()
                .filter(|name| !shown_by.contains_key(*name))
                .cloned(),
        )
        .collect();
    if wanted.is_empty() {
        return Ok(KeptFiles::default());
    }
    let probed = probe(location, month, &wanted).map_err(|error| error.to_string())?;

    let mut kept = KeptFiles::default();
    let mut to_add = Vec::new();
    for (name, probed) in wanted.iter().zip(probed) {
        let path = location.kept_files().join(name);
        let problem = |text: String| Problem {
            month: Some(month),
            text,
        };
        match check_loose(&path, name) {
            Loose::Missing => {
                if shown_by.contains_key(name) {
                    problems.push(problem(format!(
                        "{} is missing; the sessions that show it are bundled without it",
                        path.display()
                    )));
                }
            }
            Loose::Damaged(reason) => {
                if shown_by.contains_key(name) {
                    problems.push(problem(format!(
                        "{} stays and is not bundled ({reason}); the sessions that show it are bundled without it",
                        path.display()
                    )));
                }
            }
            Loose::Intact if probed == Probed::Conflicts => {
                if let Some(sessions) = shown_by.get(name) {
                    kept.held_back.extend(sessions);
                    problems.push(problem(format!(
                        "the bundle holds other content under files/{name}; the sessions that show it stay"
                    )));
                }
            }
            Loose::Intact => {
                kept.intact.insert(name.clone());
                if probed == Probed::Absent {
                    to_add.push((name, path));
                }
            }
        }
    }
    // A file only sessions that stay loose show is not worth a bundle
    // rewrite.
    for (name, path) in to_add {
        let wanted_by_a_bundled_session = strays.contains(name)
            || shown_by.get(name).is_some_and(|sessions| {
                sessions.iter().any(|index| !kept.held_back.contains(index))
            });
        if wanted_by_a_bundled_session && let Some(entry) = NewEntry::kept_file(name, path) {
            kept.entries.push(entry);
        }
    }
    Ok(kept)
}

/// The kept files that sessions already in the bundle of `month` show and
/// the bundle lacks. A bundle that cannot be read has none to report; the
/// build of the new bundle reports it. The session `ignoring`, which is
/// about to leave the bundle, shows nothing.
pub(super) fn bundled_strays(
    location: &QueueLocation,
    month: Month,
    ignoring: Option<&str>,
) -> Vec<String> {
    let Ok(entries) = list_entries(location, month) else {
        return Vec::new();
    };
    let present: HashSet<&str> = entries
        .iter()
        .filter_map(|entry| entry.name.strip_prefix("files/"))
        .collect();
    let mut strays = BTreeSet::new();
    for entry in &entries {
        if Some(entry.name.as_str()) != ignoring
            && entry
                .parsed
                .as_ref()
                .is_some_and(|name| name.kind == EntryKind::Session)
            && let Ok(bytes) = read_entry(location, month, &entry.name)
        {
            strays.extend(
                kept_file_names(&bytes)
                    .into_iter()
                    .filter(|name| !present.contains(name.as_str())),
            );
        }
    }
    strays.into_iter().collect()
}

/// Removes the loose kept files `shown` by sessions that are bundled now,
/// except those a session still loose shows. Runs under the drafts lock, so
/// no copy is placed meanwhile. A file that no session of this commit
/// showed is never touched: it is an orphan or belongs to a session an
/// older asqr bundled.
fn remove_bundled_kept_files(
    location: &QueueLocation,
    month: Month,
    shown: &BTreeSet<String>,
    problems: &mut Vec<Problem>,
) {
    if shown.is_empty() {
        return;
    }
    let still_shown = match names_shown_by_loose_sessions(location) {
        Ok(names) => names,
        Err(error) => {
            problems.push(Problem {
                month: Some(month),
                text: format!("loose kept files stay: cannot read the archive: {error}"),
            });
            return;
        }
    };
    for name in shown.difference(&still_shown) {
        let path = location.kept_files().join(name);
        if let Err(error) = std::fs::remove_file(&path) {
            problems.push(Problem {
                month: Some(month),
                text: format!("cannot remove {}: {error}", path.display()),
            });
        }
    }
}

/// The kept files the loose archived sessions show.
fn names_shown_by_loose_sessions(location: &QueueLocation) -> io::Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for entry in entries_if_present(&location.archive())? {
        if entry
            .file_name()
            .to_str()
            .and_then(parse_archive_name)
            .is_some_and(|parsed| parsed.kind == Archived::Session)
            && let Some(bytes) = read_if_present(&entry.path())?
        {
            names.extend(kept_file_names(&bytes));
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests;
