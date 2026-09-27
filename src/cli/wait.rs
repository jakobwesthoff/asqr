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
use crate::format::SessionResult;
use crate::queue::{QueueLocation, find_session_file};

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
            Ok(Some(path)) => return print_result(&path),
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

/// Prints the result file as it is and maps its status to the exit.
pub(super) fn print_result(path: &std::path::Path) -> Exit {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("error: cannot read {}: {error}", path.display());
            return Exit::Failure;
        }
    };
    let status = match serde_json::from_slice::<SessionResult>(&bytes) {
        Ok(result) => result.status,
        Err(error) => {
            eprintln!("error: {} is not a result file: {error}", path.display());
            return Exit::Failure;
        }
    };
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&bytes).is_err() || stdout.flush().is_err() {
        return Exit::Failure;
    }
    Exit::for_status(status)
}
