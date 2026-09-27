// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Waiting for a result, shared by `asqr ask --wait` and `asqr wait`.
//!
//! The asker polls the outbox. Results are written atomically, so a result
//! that is there is complete, and polling needs no watcher on the asking
//! side, which keeps `ask --wait` usable from any script.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::Exit;
use crate::format::{SessionResult, Status, same_session_id};
use crate::queue::{QueueLocation, entries_if_present, find_session_file, parse_archive_name};

/// How often the outbox is checked. Short enough that an answer shows up
/// without a noticeable delay, long enough to cost nothing.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Where a session stands, as far as an asker can tell.
enum Standing {
    /// Its result is in the outbox.
    Answered(PathBuf),
    /// It waits in the inbox for an answer.
    Waiting,
    /// Only its archive entry is left: its result was removed, so none
    /// will come any more.
    ResultGone,
    /// Nothing in the queue has this id.
    Unknown,
}

fn standing(location: &QueueLocation, id: &str) -> std::io::Result<Standing> {
    // The outbox comes first: a finish writes the result before it
    // archives the session, so a session that just left the inbox is
    // always found here.
    if let Some(result) = find_session_file(&location.outbox(), id)? {
        return Ok(Standing::Answered(result));
    }
    if find_session_file(&location.inbox(), id)?.is_some() {
        return Ok(Standing::Waiting);
    }
    let archived = entries_if_present(&location.archive())?
        .iter()
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(parse_archive_name)
                .is_some_and(|archived| same_session_id(&archived.id, id))
        });
    Ok(if archived {
        Standing::ResultGone
    } else {
        Standing::Unknown
    })
}

/// Waits until the result of `id` is in the outbox, prints it to stdout and
/// returns the exit its status leads to. Without `timeout` it waits for
/// good, unless the session leaves the queue in a way no result follows.
pub(super) fn wait_for_result(
    location: &QueueLocation,
    id: &str,
    timeout: Option<Duration>,
) -> Exit {
    let started = Instant::now();
    loop {
        match look(location, id) {
            Look::Done(exit) => return exit,
            Look::Waiting => {}
        }
        if timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
            eprintln!("asqr: no result for {id} yet (timeout)");
            return Exit::Timeout;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

enum Look {
    Done(Exit),
    Waiting,
}

/// One look at the queue: the result printed, or the exit for a session
/// that cannot get one, or word that it still waits.
fn look(location: &QueueLocation, id: &str) -> Look {
    match standing(location, id) {
        Ok(Standing::Answered(path)) => {
            Look::Done(print_result(&path).map_or_else(|exit| exit, Exit::for_status))
        }
        Ok(Standing::Waiting) => Look::Waiting,
        Ok(Standing::ResultGone) => {
            eprintln!("error: the result of {id} is gone; its session is archived");
            Look::Done(Exit::UnknownId)
        }
        Ok(Standing::Unknown) => {
            eprintln!("error: no session {id:?} in this queue");
            Look::Done(Exit::UnknownId)
        }
        Err(error) => {
            eprintln!("error: cannot read the queue: {error}");
            Look::Done(Exit::Failure)
        }
    }
}

/// `asqr wait <id>`.
pub(super) fn run_wait(location: &QueueLocation, id: &str, timeout: Option<Duration>) -> Exit {
    wait_for_result(location, id, timeout)
}

/// `asqr result <id>`: one look, the way `wait --timeout 0` would take
/// it, so the exit codes mean the same (spec section 8).
pub(super) fn run_result(location: &QueueLocation, id: &str) -> Exit {
    wait_for_result(location, id, Some(Duration::ZERO))
}

/// Prints the result file as it is and returns its status, or the exit
/// for a file that cannot be read or is no result.
fn print_result(path: &std::path::Path) -> Result<Status, Exit> {
    let bytes = std::fs::read(path).map_err(|error| {
        eprintln!("error: cannot read {}: {error}", path.display());
        Exit::Failure
    })?;
    let result: SessionResult = serde_json::from_slice(&bytes).map_err(|error| {
        eprintln!("error: {} is not a result file: {error}", path.display());
        Exit::Failure
    })?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .map_err(|_| Exit::Failure)?;
    Ok(result.status)
}
