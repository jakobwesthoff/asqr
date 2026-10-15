---
title: "Quit cleanly on SIGTERM, SIGINT and SIGHUP"
kind: bug
component: tui
origin: review
---
# Quit cleanly on SIGTERM, SIGINT and SIGHUP

From the architecture review of 2026-10-16 (finding FA4). A signal such
as SIGTERM (`kill <pid>` from a script or supervisor) ends asqr without
its guard: no Kitty deletes, no title pop, focus reports and bracketed
paste left on, raw mode and the alternate screen left on.
`src/terminal.rs` and ADR 42 document this as given. Raw mode turns ISIG
off, so ctrl-c from the keyboard is a key already; this is about signals
sent from outside.

## Change

A thread waiting for SIGTERM, SIGINT and SIGHUP (rustix with a
self-pipe, or the signal-hook crate) sends a quit event on the existing
event channel; `run` returns `Ok(())` on it, so the normal exit path
runs: deletes, title, modes, restore. SIGHUP means the terminal is gone;
the restore is harmless there. SIGKILL stays uncovered. Test in run.rs
(the event ends the run like `q`) and a PTY test sending SIGTERM that
checks the terminal is restored. ADR 42 and the terminal.rs comment
updated.
