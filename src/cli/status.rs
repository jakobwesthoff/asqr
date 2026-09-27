// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr status` (spec section 8): what waits, what is answered, and who
//! watches the queue. Agents use `--json` to find sessions they lost track
//! of after a tool timeout.

use serde_json::json;

use super::Exit;
use crate::format::SessionResult;
use crate::queue::{
    InboxName, QueueLocation, classify_inbox_name, entries_if_present, has_answered_draft,
    lock_holder, scan_inbox,
};

struct Answered {
    id: String,
    /// The status as the file writes it; `None` when the file is no
    /// readable result.
    status: Option<String>,
}

fn answered(location: &QueueLocation) -> std::io::Result<Vec<Answered>> {
    let mut answered = Vec::new();
    for entry in entries_if_present(&location.outbox())? {
        let name = entry.file_name();
        let Some(InboxName::Session(id)) = name.to_str().map(classify_inbox_name) else {
            continue;
        };
        let status = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SessionResult>(&bytes).ok())
            .and_then(|result| serde_json::to_value(result.status).ok())
            .and_then(|status| status.as_str().map(str::to_owned));
        answered.push(Answered {
            id: id.to_owned(),
            status,
        });
    }
    answered.sort_by_key(|entry| entry.id.to_ascii_lowercase());
    Ok(answered)
}

pub(super) fn run(location: &QueueLocation, as_json: bool) -> Exit {
    let gathered = scan_inbox(location).and_then(|scan| {
        let answered = answered(location)?;
        let holder = lock_holder(location)?;
        Ok((scan.sessions, answered, holder))
    });
    let (waiting, answered, holder) = match gathered {
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

    if as_json {
        let status = json!({
            "queue": queue,
            "watched_by": holder,
            "waiting": waiting.iter().map(|(id, draft)| json!({"id": id, "draft_has_answers": draft})).collect::<Vec<_>>(),
            "answered": answered.iter().map(|entry| json!({"id": entry.id, "status": entry.status})).collect::<Vec<_>>(),
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
        println!("answered ({}):", answered.len());
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
