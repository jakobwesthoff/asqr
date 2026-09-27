// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr` and `asqr watch` as far as they go without a terminal: the
//! checks before the TUI starts.

mod common;

use asqr::queue::{QueueLocation, lock_queue};
use common::{Sandbox, stderr};

#[test]
fn refuses_to_start_without_a_terminal() {
    let sandbox = Sandbox::new();

    for args in [&[][..], &["watch"][..]] {
        let output = sandbox.asqr_in_queue(args).output().expect("runs");

        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&output).contains("asqr needs a terminal"),
            "{}",
            stderr(&output)
        );
    }
    assert!(
        sandbox.queue_dir().join("inbox").is_dir(),
        "the queue was set up"
    );
}

#[test]
fn refuses_a_queue_another_asqr_watches() {
    let sandbox = Sandbox::new();
    let _held = lock_queue(&QueueLocation::at(sandbox.queue_dir())).expect("locked");

    let output = sandbox.asqr_in_queue(&[]).output().expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("another asqr is watching this queue"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn watch_flags_next_to_another_command_are_a_usage_error() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr_in_queue(&["--no-bell", "status"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
}
