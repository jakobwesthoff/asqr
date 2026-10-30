// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr queues` (spec section 6.5): every named queue, what it holds and
//! which asqr watches it, with the tmux session that asqr runs in.

use std::collections::HashMap;
use std::path::Path;

use serde_json::json;

use super::Exit;
use crate::pairing::{self, Tmux};
use crate::queue::{QueueSummary, list_queues};

pub(super) fn run(root: Option<&Path>, tmux: &dyn Tmux, json: bool) -> Exit {
    let Some(root) = root else {
        eprintln!("error: no platform data directory found, so there are no named queues");
        return Exit::Failure;
    };
    let summaries = match list_queues(root) {
        Ok(summaries) => summaries,
        Err(error) => {
            eprintln!("error: cannot read the queues: {error}");
            return Exit::Failure;
        }
    };
    let sessions = watcher_sessions(&summaries, tmux);
    let session = |summary: &QueueSummary| -> Option<String> {
        let place = summary.holder.as_ref()?.tmux.as_ref()?;
        sessions.get(&place.socket)?.get(&place.pane).cloned()
    };

    if json {
        let queues: Vec<_> = summaries
            .iter()
            .map(|summary| {
                json!({
                    "queue": summary.name(),
                    "waiting_total": summary.waiting,
                    "answered_total": summary.answered,
                    "watched_by": summary.holder.as_ref().map(|holder| &holder.text),
                    "tmux_session": session(summary),
                })
            })
            .collect();
        println!("{}", serde_json::Value::from(queues));
        return Exit::Success;
    }

    let width = summaries
        .iter()
        .map(|summary| summary.name().len())
        .chain(["QUEUE".len()])
        .max()
        .unwrap_or_default();
    println!("{:<width$}  WAITING  ANSWERED  WATCHED BY", "QUEUE");
    for summary in &summaries {
        let watched_by = match (&summary.holder, session(summary)) {
            (None, _) => "nobody".to_owned(),
            (Some(holder), None) => holder.text.clone(),
            (Some(holder), Some(session)) => format!("{}, tmux session {session}", holder.text),
        };
        println!(
            "{:<width$}  {:>7}  {:>8}  {watched_by}",
            summary.name(),
            summary.waiting,
            summary.answered
        );
    }
    Exit::Success
}

/// The tmux sessions of the panes the watching asqr instances run in, by
/// socket and pane: one call to each tmux server, however many of its
/// panes watch a queue. A server that does not answer names none.
fn watcher_sessions(
    summaries: &[QueueSummary],
    tmux: &dyn Tmux,
) -> HashMap<String, HashMap<String, String>> {
    let mut sessions = HashMap::new();
    for summary in summaries {
        let Some(place) = summary
            .holder
            .as_ref()
            .and_then(|holder| holder.tmux.as_ref())
        else {
            continue;
        };
        sessions
            .entry(place.socket.clone())
            .or_insert_with(|| pairing::sessions_of_panes(tmux, &place.socket).unwrap_or_default());
    }
    sessions
}
