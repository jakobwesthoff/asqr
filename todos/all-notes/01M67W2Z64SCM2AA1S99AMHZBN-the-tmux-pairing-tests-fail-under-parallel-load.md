---
title: "The tmux pairing tests fail under parallel load"
kind: bug
component: project
origin: implementation
---
# The tmux pairing tests fail under parallel load

During the conductor run of 2026-10-31 the tests in
`tests/cli_pairing.rs` failed three times in `just check` while other
builds and test runs used the machine, and passed when rerun alone. Up to
seven failed at once, for example `the_pin_wins_over_the_session_name`
(`left: "default"`, `right: "asqr-todos"`) and
`the_refusal_names_the_tmux_session_of_the_asqr_watching` ("asqr needs a
terminal"). Two Kitty pty tests in `tests/tui_pty.rs`
(`asqr_log_debug_writes_the_frames_that_send_a_kitty_image`,
`a_kitty_image_is_placed_with_its_size_in_cells`) failed the same way
once.

## Problem

Every pairing test writes its stand-in `tmux` script into the sandbox and
then runs asqr, which executes it (`StandInTmux::new`, the top of
`tests/cli_pairing.rs`). A failing lookup makes asqr fall back to the
default queue without a word, which matches the `"default"` results.

Hypothesis, not verified: the "text file busy" race. While one test
thread holds the script open for writing, another test thread forks a
child, which inherits that descriptor until its own exec; executing the
script meanwhile fails with `ETXTBSY`. Nothing confirms it yet beyond
the symptoms.

## Impact

A red `just check` that passes on a rerun costs every agent a second run
and hides real failures among flakes.

## Suggested fix

1. Confirm: run the pairing tests in a loop under load (for example
   beside `cargo build` of another worktree) and log the exit and stderr
   of the stand-in's exec, or check for `ETXTBSY` in asqr's log.
2. If confirmed, write each script so no other thread can hold it open:
   write it once per test binary before any test forks (a `Once`), or
   write it from a child process (`sh -c 'cat > tmux'`), or retry the
   exec on `ETXTBSY` in the test helper. Check the pty tests' helpers for
   the same pattern.
