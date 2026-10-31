// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr status` (spec section 6.5): what waits, what is answered, and who
//! watches the queue. Agents use `--json` to find sessions they lost track
//! of after a tool timeout.
//!
//! The outbox and the month bundles keep every result until someone prunes
//! them, so the answered list grows with every session ever finished. `status` lists the newest
//! [`DEFAULT_LIMIT`] by default and says how many there are, which keeps
//! the output an agent reads short.

use std::cmp::Reverse;
use std::collections::HashSet;
use std::time::{Duration, SystemTime};

use serde_json::json;

use super::Exit;
use crate::format::SessionResult;
use crate::queue::{
    BundleError, BundledResult, InboxName, QueueLocation, bundled_results, classify_inbox_name,
    entries_if_present, finished_at, has_answered_draft, lock_holder, read_entry,
    result_entry_name, scan_inbox,
};

/// How many answered sessions `status` lists without `--limit` or `--all`.
pub(super) const DEFAULT_LIMIT: usize = 10;

pub(super) struct StatusOptions {
    pub json: bool,
    /// The most answered sessions to list; `None` lists all of them.
    pub limit: Option<usize>,
    /// List only sessions answered within this age. Waiting sessions are
    /// all listed, since each of them is still open.
    pub since: Option<Duration>,
}

struct Answered {
    id: String,
    /// The status as the file writes it; `None` when the file is no
    /// readable result.
    status: Option<String>,
    /// When the session was finished: `submitted_at`, else the result
    /// file's modification time (see [`finished_at`]).
    finished: SystemTime,
    /// Where the result lies, for the status of a bundled one, which is
    /// only read once the entry is on the list.
    bundled: Option<BundledResult>,
}

/// The status a result file writes, if it is one.
fn status_of(bytes: &[u8]) -> Option<String> {
    let result = serde_json::from_slice::<SessionResult>(bytes).ok()?;
    serde_json::to_value(result.status)
        .ok()?
        .as_str()
        .map(str::to_owned)
}

/// Every result in the outbox and in the month bundles, the most recently
/// finished first, and the bundles that could not be read. A bundled
/// result is known by its entry name alone, whose time is when it was
/// finished; its content is read only for the entries that are listed.
fn answered(location: &QueueLocation) -> std::io::Result<(Vec<Answered>, Vec<BundleError>)> {
    let mut answered = Vec::new();
    // A result the compaction has bundled and not yet removed from the
    // outbox is in both places; it counts once, as an outbox result.
    let mut also_bundled = HashSet::new();
    for entry in entries_if_present(&location.outbox())? {
        let name = entry.file_name();
        let Some(InboxName::Session(id)) = name.to_str().map(classify_inbox_name) else {
            continue;
        };
        let bytes = std::fs::read(entry.path()).ok();
        let finished = finished_at(
            bytes.as_deref().unwrap_or_default(),
            entry.metadata()?.modified()?,
        );
        if let Some(bytes) = &bytes {
            also_bundled.insert(result_entry_name(id, bytes, finished).zip_name());
        }
        answered.push(Answered {
            id: id.to_owned(),
            status: bytes.as_deref().and_then(status_of),
            finished,
            bundled: None,
        });
    }
    let (bundled, unreadable) = bundled_results(location)?;
    answered.extend(
        bundled
            .into_iter()
            .filter(|result| !also_bundled.contains(&result.zip_name))
            .map(|result| Answered {
                id: result.name.id.clone(),
                status: None,
                finished: result.name.ulid.datetime(),
                bundled: Some(result),
            }),
    );
    // `submitted_at` has whole seconds, so sessions finished in one second
    // tie. The id orders them, the same way on every call.
    answered.sort_by_cached_key(|entry| (Reverse(entry.finished), entry.id.to_ascii_lowercase()));
    Ok((answered, unreadable))
}

pub(super) fn run(location: &QueueLocation, options: StatusOptions) -> Exit {
    let gathered = scan_inbox(location).and_then(|scan| {
        let (answered, unreadable) = answered(location)?;
        let holder = lock_holder(location)?.map(|holder| holder.text);
        Ok((scan.sessions, answered, unreadable, holder))
    });
    let (waiting, mut answered, unreadable, holder) = match gathered {
        Ok(gathered) => gathered,
        Err(error) => {
            eprintln!("error: cannot read the queue: {error}");
            return Exit::Failure;
        }
    };
    let queue = location
        .name()
        .map_or_else(|| location.dir().display().to_string(), str::to_owned);
    let waiting: Vec<_> = waiting
        .into_iter()
        .map(|session| {
            let draft = has_answered_draft(location, &session.id);
            (session.id, draft)
        })
        .collect();

    // The total counts what matches `--since` before the limit cuts the
    // list, so a total above the list's length tells the reader that older
    // matches exist.
    if let Some(since) = options.since {
        let cutoff = SystemTime::now()
            .checked_sub(since)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        answered.retain(|entry| entry.finished >= cutoff);
    }
    let answered_total = answered.len();
    if let Some(limit) = options.limit {
        answered.truncate(limit);
    }
    for entry in &mut answered {
        if let Some(bundled) = &entry.bundled {
            entry.status = read_entry(location, bundled.month, &bundled.zip_name)
                .ok()
                .and_then(|bytes| status_of(&bytes));
        }
    }
    for error in &unreadable {
        eprintln!("warning: {error}");
    }

    if options.json {
        let status = json!({
            "queue": queue,
            "watched_by": holder,
            "waiting": waiting.iter().map(|(id, draft)| json!({"id": id, "draft_has_answers": draft})).collect::<Vec<_>>(),
            "answered": answered.iter().map(|entry| json!({"id": entry.id, "status": entry.status})).collect::<Vec<_>>(),
            "answered_total": answered_total,
        });
        println!("{status}");
    } else {
        println!("queue: {queue}");
        println!("watched by: {}", holder.as_deref().unwrap_or("nobody"));
        println!("waiting ({}):", waiting.len());
        for (id, draft) in &waiting {
            if *draft {
                println!("  {id}  (draft with answers)");
            } else {
                println!("  {id}");
            }
        }
        if answered.len() < answered_total {
            println!("answered ({} of {answered_total}):", answered.len());
        } else {
            println!("answered ({answered_total}):");
        }
        for entry in &answered {
            println!(
                "  {}  {}",
                entry.id,
                entry.status.as_deref().unwrap_or("unreadable")
            );
        }
    }
    Exit::Success
}
