// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr withdraw` (spec sections 5.10 and 6.4): the asker takes a session
//! back. It prints the result it wrote, as `wait` and `result` print one,
//! since every command that ends a session gives the same output.

use std::io::Write;
use std::path::Path;

use super::Exit;
use super::hint::{self, Invocation, Refusing};
use super::wait::{Elsewhere, elsewhere};
use crate::format::SessionResult;
use crate::keep::apply::record_after_withdrawal;
use crate::queue::{Gate, QueueLocation, WithdrawError, Withdrawn, withdraw_session_archived};

pub(super) fn run(
    location: &QueueLocation,
    invocation: &Invocation,
    root: Option<&Path>,
    id: &str,
    force: bool,
) -> Exit {
    match withdraw_session_archived(location, id, force, Gate::default()) {
        Ok((withdrawn, archived)) => {
            // The drafts lock is let go of by now: reading the files can be
            // slow, and the TUI saves drafts under that lock.
            record_after_withdrawal(location, archived);
            match withdrawn {
                Withdrawn::Now(result) => print(&result),
                Withdrawn::Already(result) => {
                    eprintln!("asqr: {id} was withdrawn already");
                    print(&result)
                }
            }
        }
        // A withdraw that did not happen must not look like one that did,
        // so every refusal fails; the ones that mean no session will ever
        // answer take the code `wait` gives them.
        // Another named queue may hold the id, but a session there may be
        // another agent's: the refusal names the queue, and only a
        // `withdraw` run on it takes the session back.
        Err(WithdrawError::Unknown(id)) => match elsewhere(location, root, &id) {
            Elsewhere::Nowhere => {
                hint::unknown_session(invocation, &id).print();
                Exit::UnknownId
            }
            Elsewhere::One(found) => {
                let found = found.name().unwrap_or_default().to_owned();
                hint::withdraw_elsewhere(invocation, &id, &found).print();
                Exit::UnknownId
            }
            Elsewhere::Several(queues) => {
                hint::in_several_queues(&id, &queues).print();
                Exit::Failure
            }
        },
        Err(error @ WithdrawError::ResultGone(_)) => {
            eprintln!("error: {error}");
            Exit::UnknownId
        }
        Err(WithdrawError::BeingAnswered(id)) => {
            hint::being_answered(invocation, &id, Refusing::Withdraw).print();
            Exit::Failure
        }
        Err(WithdrawError::Io(error)) => {
            eprintln!("error: cannot withdraw {id}: {error}");
            Exit::Failure
        }
        Err(error) => {
            eprintln!("error: {error}");
            Exit::Failure
        }
    }
}

/// Prints `result` with the bytes the outbox holds for it, so the output
/// matches what `wait` and `result` print for the session, then the hints
/// it gets. A result that did not reach stdout gets none, since they point
/// into it.
fn print(result: &SessionResult) -> Exit {
    let json = serde_json::to_vec_pretty(result).expect("a result always serializes");
    let written = {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&json).and_then(|()| stdout.flush())
    };
    match written {
        Ok(()) => {
            hint::after_result(&json, result);
            Exit::Success
        }
        Err(_) => Exit::Failure,
    }
}
