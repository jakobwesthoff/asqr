// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Waiting for a result, shared by `asqr ask --wait` and `asqr wait`.
//!
//! The asker polls the outbox. Results are written atomically, so a result
//! that is there is complete, and polling needs no watcher on the asking
//! side, which keeps `ask --wait` usable from any script.

use std::io::Write;
use std::time::{Duration, Instant};

use super::Exit;
use crate::format::{SessionResult, Status, same_session_id};
use crate::queue::{QueueLocation, entries_if_present, find_session_file, parse_archive_name};

/// How often the outbox is checked. Short enough that an answer shows up
/// without a noticeable delay, long enough to cost nothing.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Waits until the result of `id` is in the outbox, prints it to stdout and
/// returns the exit its status leads to. Without `timeout` it waits for
/// good.
pub(super) fn wait_for_result(
    location: &QueueLocation,
    id: &str,
    timeout: Option<Duration>,
) -> Exit {
    let started = Instant::now();
    loop {
        match find_session_file(&location.outbox(), id) {
            Ok(Some(path)) => {
                return print_result(&path).map_or_else(|exit| exit, Exit::for_status);
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("error: cannot read the outbox: {error}");
                return Exit::Failure;
            }
        }
        if timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
            eprintln!("asqr: no result for {id} yet (timeout)");
            return Exit::Timeout;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// `asqr wait <id>`: an id nothing in the queue knows ends at once, since
/// no result will ever come for it.
pub(super) fn run_wait(location: &QueueLocation, id: &str, timeout: Option<Duration>) -> Exit {
    match is_known(location, id) {
        Ok(true) => wait_for_result(location, id, timeout),
        Ok(false) => {
            eprintln!("error: no session {id:?} in this queue");
            Exit::UnknownId
        }
        Err(error) => {
            eprintln!("error: cannot read the queue: {error}");
            Exit::Failure
        }
    }
}

/// `asqr result <id>`: prints the result if it is there. Finding and
/// printing it is the command's job, so any status exits with success.
pub(super) fn run_result(location: &QueueLocation, id: &str) -> Exit {
    match find_session_file(&location.outbox(), id) {
        Ok(Some(path)) => print_result(&path).map_or_else(|exit| exit, |_| Exit::Success),
        Ok(None) => Exit::UnknownId,
        Err(error) => {
            eprintln!("error: cannot read the outbox: {error}");
            Exit::Failure
        }
    }
}

/// Whether anything in the queue has this id: a waiting session, a result
/// or an archive entry.
fn is_known(location: &QueueLocation, id: &str) -> std::io::Result<bool> {
    if find_session_file(&location.inbox(), id)?.is_some()
        || find_session_file(&location.outbox(), id)?.is_some()
    {
        return Ok(true);
    }
    Ok(entries_if_present(&location.archive())?
        .iter()
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(parse_archive_name)
                .is_some_and(|archived| same_session_id(&archived.id, id))
        }))
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
