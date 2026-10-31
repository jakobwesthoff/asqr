// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Waiting for a result, shared by `asqr ask --wait` and `asqr wait`.
//!
//! The asker polls the outbox. Results are written atomically, so a result
//! that is there is complete, and polling needs no watcher on the asking
//! side, which keeps `ask --wait` usable from any script.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use super::Exit;
use super::hint::{self, Caller, Invocation};
use crate::format::{SessionResult, Status};
use crate::queue::{
    ArchivedStanding, QueueLocation, archived_standing, find_session_file, list_queues,
    read_if_present,
};

/// How often the outbox is checked. Short enough that an answer shows up
/// without a noticeable delay, long enough to cost nothing.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Where a session stands, as far as an asker can tell.
enum Standing {
    /// Its result: in the outbox, or in the archive once it left the
    /// outbox. The bytes are the file's.
    Answered(Vec<u8>),
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
    if let Some(path) = find_session_file(&location.outbox(), id)?
        && let Some(bytes) = read_if_present(&path)?
    {
        return Ok(Standing::Answered(bytes));
    }
    if find_session_file(&location.inbox(), id)?.is_some() {
        return Ok(Standing::Waiting);
    }
    // A file that left the outbox for a bundle is found there, as the
    // archive is read after the outbox.
    Ok(
        match archived_standing(location, id).map_err(std::io::Error::other)? {
            ArchivedStanding::Result(bytes) => Standing::Answered(bytes),
            ArchivedStanding::Gone => Standing::ResultGone,
            ArchivedStanding::Unknown => Standing::Unknown,
        },
    )
}

/// Where else session `id` is, looked for only when the queue a command
/// runs on has no trace of it: no waiting file, no result and no archive
/// entry. A session finished there is never looked up elsewhere.
pub(super) enum Elsewhere {
    Nowhere,
    One(QueueLocation),
    /// The names of the queues that know it.
    Several(Vec<String>),
}

/// The named queues under `root` other than `location` that know session
/// `id` (T2), where `location` does not. A queue directory is never
/// searched: nothing lists them.
pub(super) fn elsewhere(location: &QueueLocation, root: Option<&Path>, id: &str) -> Elsewhere {
    let unknown_here = matches!(standing(location, id), Ok(Standing::Unknown));
    let Some(root) = root.filter(|_| unknown_here) else {
        return Elsewhere::Nowhere;
    };
    let mut found: Vec<QueueLocation> = list_queues(root)
        .unwrap_or_default()
        .into_iter()
        .map(|summary| summary.location)
        .filter(|other| !other.same_queue(location))
        .filter(|other| matches!(standing(other, id), Ok(standing) if !matches!(standing, Standing::Unknown)))
        .collect();
    match found.len() {
        0 => Elsewhere::Nowhere,
        1 => Elsewhere::One(found.remove(0)),
        _ => Elsewhere::Several(
            found
                .iter()
                .filter_map(|other| other.name().map(str::to_owned))
                .collect(),
        ),
    }
}

/// The queue `wait` and `result` look in: `location`, or the one other
/// named queue that knows session `id`, with the advice that names it.
/// Several such queues end the command.
fn located(
    location: &QueueLocation,
    invocation: &Invocation,
    root: Option<&Path>,
    id: &str,
) -> Result<Option<(QueueLocation, Invocation)>, Exit> {
    match elsewhere(location, root, id) {
        Elsewhere::Nowhere => Ok(None),
        Elsewhere::One(found) => {
            let name = found.name().unwrap_or_default().to_owned();
            hint::found_elsewhere(invocation, id, &name).print();
            let invocation = Invocation::new(&found, true);
            Ok(Some((found, invocation)))
        }
        Elsewhere::Several(queues) => {
            hint::in_several_queues(id, &queues).print();
            Err(Exit::Failure)
        }
    }
}

/// Waits until the result of `id` is in the outbox, prints it to stdout and
/// returns the exit its status leads to. Without `timeout` it waits for
/// good, unless the session leaves the queue in a way no result follows.
/// When the timeout passes first, it returns [`Exit::Timeout`] and leaves
/// the message to its caller, which knows how the agent waits again.
pub(super) fn wait_for_result(
    location: &QueueLocation,
    invocation: &Invocation,
    id: &str,
    timeout: Option<Duration>,
) -> Exit {
    let started = Instant::now();
    loop {
        match look(location, invocation, id) {
            Look::Done(exit) => return exit,
            Look::Waiting => {}
        }
        if timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
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
fn look(location: &QueueLocation, invocation: &Invocation, id: &str) -> Look {
    match standing(location, id) {
        Ok(Standing::Answered(bytes)) => {
            Look::Done(print_result(id, &bytes).map_or_else(|exit| exit, Exit::for_status))
        }
        Ok(Standing::Waiting) => Look::Waiting,
        Ok(Standing::ResultGone) => {
            eprintln!("error: the result of {id} is gone; its session is archived");
            Look::Done(Exit::UnknownId)
        }
        Ok(Standing::Unknown) => {
            hint::unknown_session(invocation, id).print();
            Look::Done(Exit::UnknownId)
        }
        Err(error) => {
            eprintln!("error: cannot read the queue: {error}");
            Look::Done(Exit::Failure)
        }
    }
}

/// `asqr wait <id>`.
pub(super) fn run_wait(
    location: &QueueLocation,
    invocation: &Invocation,
    root: Option<&Path>,
    id: &str,
    timeout: Option<Duration>,
) -> Exit {
    let found = match located(location, invocation, root, id) {
        Ok(found) => found,
        Err(exit) => return exit,
    };
    let (location, invocation) = found
        .as_ref()
        .map_or((location, invocation), |(location, invocation)| {
            (location, invocation)
        });
    let exit = wait_for_result(location, invocation, id, timeout);
    if let (Exit::Timeout, Some(timeout)) = (exit, timeout) {
        hint::no_result_yet(invocation, id, Caller::Wait(timeout.as_secs())).print();
    }
    exit
}

/// `asqr result <id>`: one look, the way `wait --timeout 0` would take
/// it, so the exit codes mean the same (spec section 6.6).
pub(super) fn run_result(
    location: &QueueLocation,
    invocation: &Invocation,
    root: Option<&Path>,
    id: &str,
) -> Exit {
    let found = match located(location, invocation, root, id) {
        Ok(found) => found,
        Err(exit) => return exit,
    };
    let (location, invocation) = found
        .as_ref()
        .map_or((location, invocation), |(location, invocation)| {
            (location, invocation)
        });
    let exit = wait_for_result(location, invocation, id, Some(Duration::ZERO));
    if exit == Exit::Timeout {
        hint::no_result_yet(invocation, id, Caller::Result).print();
    }
    exit
}

/// Prints the result file as it is, then the hints it gets on stderr, and
/// returns its status, or the exit for a file that is no result.
fn print_result(id: &str, bytes: &[u8]) -> Result<Status, Exit> {
    let result: SessionResult = serde_json::from_slice(bytes).map_err(|error| {
        eprintln!("error: the result of {id} is not a result file: {error}");
        Exit::Failure
    })?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(bytes)
        .and_then(|()| stdout.flush())
        .map_err(|_| Exit::Failure)?;
    hint::after_result(bytes, &result);
    Ok(result.status)
}
