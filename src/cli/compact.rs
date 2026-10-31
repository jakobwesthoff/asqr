// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr compact` (spec section 5.9): bundles the sessions and results
//! older than 30 days of a queue by hand, for a queue no TUI opens.

use std::path::Path;
use std::time::SystemTime;

use super::Exit;
use crate::queue::{
    CompactReport, LOCK_ATTEMPTS, QueueLocation, compact, list_queues, try_lock_queue_for,
};

/// How compacting one queue ended.
enum Outcome {
    /// The queue has no directory yet, so it holds nothing.
    Missing,
    /// Another asqr watches the queue and compacts it when it starts.
    Watched,
    Done(CompactReport),
    Failed(std::io::Error),
}

fn compact_queue(location: &QueueLocation) -> Outcome {
    // Taking the lock creates the queue's directory, which a mistyped
    // queue name must not leave behind.
    if !location.dir().is_dir() {
        return Outcome::Missing;
    }
    match try_lock_queue_for(location, LOCK_ATTEMPTS) {
        Ok(Some(lock)) => Outcome::Done(compact(location, &lock, SystemTime::now())),
        Ok(None) => Outcome::Watched,
        Err(error) => Outcome::Failed(error),
    }
}

fn count(number: usize, noun: &str) -> Option<String> {
    match number {
        0 => None,
        1 => Some(format!("1 {noun}")),
        _ => Some(format!("{number} {noun}s")),
    }
}

/// Prints what was done in `report`, each line behind `prefix`, and says
/// whether everything went through.
fn print_report(report: &CompactReport, prefix: &str) -> bool {
    for done in &report.bundled {
        let parts: Vec<String> = [
            count(done.sessions, "session"),
            count(done.results, "result"),
            count(done.replaced, "replaced result"),
        ]
        .into_iter()
        .flatten()
        .collect();
        println!("{prefix}{}.zip: {}", done.month, parts.join(", "));
    }
    for id in &report.for_recovery {
        eprintln!("{prefix}left the result of {id} in the outbox: a session of that id waits");
    }
    for problem in &report.problems {
        match problem.month {
            Some(month) => eprintln!("{prefix}skipped {month}: {}", problem.text),
            None => eprintln!("{prefix}skipped: {}", problem.text),
        }
    }
    report.problems.is_empty()
}

pub(super) fn run(location: &QueueLocation, root: Option<&Path>, all: bool) -> Exit {
    if !all {
        return match compact_queue(location) {
            Outcome::Missing => {
                println!("nothing to compact");
                Exit::Success
            }
            Outcome::Watched => {
                eprintln!(
                    "skipped: another asqr watches this queue and compacts it when it starts"
                );
                Exit::Failure
            }
            Outcome::Done(report) => {
                let complete = print_report(&report, "");
                if report.bundled.is_empty() && complete {
                    println!("nothing to compact");
                }
                if complete {
                    Exit::Success
                } else {
                    Exit::Failure
                }
            }
            Outcome::Failed(error) => {
                eprintln!("error: cannot compact the queue: {error}");
                Exit::Failure
            }
        };
    }

    let Some(root) = root else {
        eprintln!("error: no platform data directory found, so there are no named queues");
        return Exit::Failure;
    };
    let queues = match list_queues(root) {
        Ok(queues) => queues,
        Err(error) => {
            eprintln!("error: cannot read the queues: {error}");
            return Exit::Failure;
        }
    };
    let mut bundled = false;
    let mut complete = true;
    for summary in queues {
        let prefix = format!("{}: ", summary.name());
        match compact_queue(&summary.location) {
            Outcome::Missing => {}
            Outcome::Watched => eprintln!("{prefix}skipped: another asqr watches it"),
            Outcome::Done(report) => {
                bundled |= !report.bundled.is_empty();
                complete &= print_report(&report, &prefix);
            }
            Outcome::Failed(error) => {
                eprintln!("{prefix}error: cannot compact the queue: {error}");
                complete = false;
            }
        }
    }
    if !bundled && complete {
        println!("nothing to compact");
    }
    if complete {
        Exit::Success
    } else {
        Exit::Failure
    }
}
