// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr status` (spec section 8): what waits, what is answered, and who
//! watches the queue. Agents use `--json` to find sessions they lost track
//! of after a tool timeout.
//!
//! The outbox keeps every result until someone prunes it, so the answered
//! list grows with every session ever finished. `status` lists the newest
//! [`DEFAULT_LIMIT`] by default and says how many there are, which keeps
//! the output an agent reads short.

use std::cmp::Reverse;
use std::time::{Duration, SystemTime};

use serde_json::json;

use super::Exit;
use crate::format::SessionResult;
use crate::queue::{
    InboxName, QueueLocation, classify_inbox_name, entries_if_present, has_answered_draft,
    lock_holder, scan_inbox,
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
    /// When the session was finished. A result names that time in
    /// `submitted_at`; for a file without a usable one, its modification
    /// time is the closest guess, as asqr writes a result once and never
    /// touches it again.
    finished: SystemTime,
}

/// Every result in the outbox, the most recently finished first.
fn answered(location: &QueueLocation) -> std::io::Result<Vec<Answered>> {
    let mut answered = Vec::new();
    for entry in entries_if_present(&location.outbox())? {
        let name = entry.file_name();
        let Some(InboxName::Session(id)) = name.to_str().map(classify_inbox_name) else {
            continue;
        };
        let result = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SessionResult>(&bytes).ok());
        let status = result
            .as_ref()
            .and_then(|result| serde_json::to_value(result.status).ok())
            .and_then(|status| status.as_str().map(str::to_owned));
        let submitted_at = result
            .as_ref()
            .and_then(|result| result.submitted_at.as_deref())
            .and_then(|at| at.parse::<jiff::Timestamp>().ok());
        let finished = match submitted_at {
            Some(at) => SystemTime::from(at),
            None => entry.metadata()?.modified()?,
        };
        answered.push(Answered {
            id: id.to_owned(),
            status,
            finished,
        });
    }
    // `submitted_at` has whole seconds, so sessions finished in one second
    // tie. The id orders them, the same way on every call.
    answered.sort_by_cached_key(|entry| (Reverse(entry.finished), entry.id.to_ascii_lowercase()));
    Ok(answered)
}

pub(super) fn run(location: &QueueLocation, options: StatusOptions) -> Exit {
    let gathered = scan_inbox(location).and_then(|scan| {
        let answered = answered(location)?;
        let holder = lock_holder(location)?;
        Ok((scan.sessions, answered, holder))
    });
    let (waiting, mut answered, holder) = match gathered {
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
