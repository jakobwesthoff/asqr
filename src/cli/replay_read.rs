// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr replay <ref> --json` (spec section 6.5): a finished session, its
//! result and the files it showed as data for an agent, where the terminal
//! UI is for the person.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::history::{finished_at, queue_label, status_json};
use crate::keep::places::places_mut;
use crate::queue::{EntryRef, LoadedReplay, ReplayError, ReplayFile, load_replay};

/// Loads `entry` for reading. Nothing is written: the copies of the kept
/// files are checked against their hashes and handed out under the name
/// they have in the archive.
pub(super) fn load_for_reading(entry: &EntryRef) -> Result<LoadedReplay, ReplayError> {
    load_replay(entry, &mut |copy_name, _, _| {
        Ok(PathBuf::from(format!("files/{copy_name}")))
    })
}

/// The object `replay --json` prints.
///
/// The session is the archived one as stored, so fields this asqr does not
/// know survive, but every file's `path` is the one the asker wrote: an
/// agent never sees the archive's names. The result is read through the
/// typed format, which leaves out what asqr does not know.
pub(super) fn replay_json(loaded: &LoadedReplay) -> Value {
    let entry = &loaded.entry;
    json!({
        "ref": entry.entry.reference(),
        "queue": queue_label(&entry.entry.queue),
        "id": entry.entry.id,
        "finished_at": finished_at(entry.finished),
        "status": status_json(entry.outcome),
        "draft": entry.has_draft,
        "session": session_with_original_paths(&loaded.raw),
        "result": loaded.result.as_ref().map(|result| {
            serde_json::to_value(result).expect("a result is plain data")
        }),
        "files": files_json(&loaded.shown),
        "extracted_to": null,
    })
}

/// `raw` with the `path` of every place that records where its file lay
/// set to that original path.
fn session_with_original_paths(raw: &Value) -> Value {
    let mut session = raw.clone();
    for (_, place) in places_mut(&mut session) {
        let original = place["shown"]["original_path"].as_str().map(str::to_owned);
        if let Some(original) = original {
            place["path"] = Value::String(original);
        }
    }
    session
}

/// One entry for each distinct original path and content, sorted by that
/// pair. A file shown at several places is one entry, and the first place
/// that names it decides what the entry says.
fn files_json(shown: &[ReplayFile]) -> Vec<Value> {
    let mut distinct: Vec<&ReplayFile> = shown.iter().collect();
    distinct.sort_by(|a, b| (&a.original_path, &a.sha256).cmp(&(&b.original_path, &b.sha256)));
    distinct.dedup_by(|a, b| (&a.original_path, &a.sha256) == (&b.original_path, &b.sha256));
    distinct.into_iter().map(file_json).collect()
}

fn file_json(file: &ReplayFile) -> Value {
    // The session records why a file is missing or unreadable, and both
    // mean the same to a reader: it was not there to keep.
    let state = match file.state.as_str() {
        "missing" | "unreadable" => "gone",
        other => other,
    };
    json!({
        "original_path": file.original_path,
        "state": state,
        "sha256": file.sha256,
        "size": file.size,
        "modified": file.modified,
        "kept_size": file.kept_size,
        "reason": file.reason,
        "problem": file.damaged,
        "extracted": null,
    })
}

#[cfg(test)]
mod tests;
