---
title: "Decide whether asqr checks for a newer version when it starts"
kind: investigation
component: tui
status: needs-discussion
origin: request
tags: [ux]
---
# Decide whether asqr checks for a newer version when it starts

Raised on 2026-10-22 after releasing asqr 0.13.0 and 0.13.1: the asqr
that shows a queue runs as a long-lived process, and nothing tells the
person that a newer asqr exists. Since 0.13.1 the empty queue's key bar
and the help show the running version (ADR 59), but not whether it is
the newest. To be found out whether asqr can sensibly check for a newer
version when it starts, and show the result to the person.

## Context

- asqr is installed through crates.io (`cargo install asqr`), GitHub
  release archives, and the Homebrew tap `jakobwesthoff/tap`.
- A sibling case is already handled: an error result that names a
  context type as unknown tells the agent that a newer asqr may support
  it (ADR 51).

## Options

To be worked out; none is decided.

- Where to ask: the GitHub releases API, the crates.io API, or a file
  the project publishes itself.
- When: at start only, or also now and then while asqr runs; in the
  background, so the start never waits on the network.
- How to show it: a notice in the status line, or beside the version in
  the key bar.
- What to say about updating, since the command depends on how asqr was
  installed (`brew upgrade`, `cargo install`, a downloaded archive).

## Open questions

- Whether asqr should reach the network at all without being asked, and
  how to turn the check off (a flag, an environment variable, a setting).
- Privacy: what a request reveals and to whom.
- How often to check, and whether to remember the last result to avoid
  asking on every start.
