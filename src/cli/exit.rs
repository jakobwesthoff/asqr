// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exit codes (spec section 8), the same for every command. 1 and 2 keep
//! their usual meaning; asqr's own outcomes start at 10, so a script can
//! tell "the person rejected it" from "asqr broke".

use std::process::ExitCode;

use crate::format::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The result is `submitted`, the file was dropped, or the command did
    /// its job.
    Success,
    /// asqr itself failed: I/O, a refused `ask`, a held lock.
    Failure,
    /// A usage error; clap reports its own with the same code.
    Usage,
    /// The result is `cancelled`.
    Cancelled,
    /// The result is `error`, or `ask` got a file that fails validation.
    ErrorResult,
    Timeout,
    /// Nothing in the inbox, outbox or archive has this id.
    UnknownId,
}

impl Exit {
    pub fn code(self) -> u8 {
        match self {
            Exit::Success => 0,
            Exit::Failure => 1,
            Exit::Usage => 2,
            Exit::Cancelled => 10,
            Exit::ErrorResult => 11,
            Exit::Timeout => 12,
            Exit::UnknownId => 13,
        }
    }

    /// How a waiting asker ends for a result with `status`.
    pub fn for_status(status: Status) -> Self {
        match status {
            Status::Submitted => Exit::Success,
            Status::Cancelled => Exit::Cancelled,
            Status::Error => Exit::ErrorResult,
            // A draft never reaches the outbox; finding one there means the
            // queue was written by something else.
            Status::Draft => Exit::Failure,
        }
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        ExitCode::from(exit.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_the_spec_table() {
        let table = [
            (Exit::Success, 0),
            (Exit::Failure, 1),
            (Exit::Usage, 2),
            (Exit::Cancelled, 10),
            (Exit::ErrorResult, 11),
            (Exit::Timeout, 12),
            (Exit::UnknownId, 13),
        ];
        for (exit, code) in table {
            assert_eq!(exit.code(), code, "{exit:?}");
        }
    }

    #[test]
    fn results_map_to_their_exit() {
        assert_eq!(Exit::for_status(Status::Submitted), Exit::Success);
        assert_eq!(Exit::for_status(Status::Cancelled), Exit::Cancelled);
        assert_eq!(Exit::for_status(Status::Error), Exit::ErrorResult);
        assert_eq!(Exit::for_status(Status::Draft), Exit::Failure);
    }

    #[test]
    fn converts_into_a_process_exit_code() {
        assert_eq!(ExitCode::from(Exit::Timeout), ExitCode::from(12));
    }
}
