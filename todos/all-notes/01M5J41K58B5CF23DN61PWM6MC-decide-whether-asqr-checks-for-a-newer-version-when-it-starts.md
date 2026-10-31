---
title: "Decide whether asqr checks for a newer version when it starts"
kind: investigation
component: tui
status: needs-discussion
horizon: someday
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

## Discussed on 2026-10-28

The user's direction, not final:

- The automatic check belongs together with user preferences (todo
  01M623QGQXR4ERM34GYV8H6DVR, Ask the settings that need a deliberate
  answer on first start), which it needs: on the first start asqr
  would ask whether it may look for updates automatically, at start and
  at intervals while it runs for a long time, as torchsnap does.
- A possible first step before the preferences exist: the check itself,
  run on request through a command or a flag. Which command or flag is
  open; `asqr version --check` was only a first proposal.
- Then the preferences, then the automatic check, which reuses the
  routines of the first step.
- Open as well: how asqr presents a newer version, how it offers the
  update, and what it suggests.

## Decided so far (Jakob, 2026-10-30, in the chat)

- **asqr asks on first start whether it may look for newer versions,
  with "no" preselected,** and keeps the answer as a setting (todo
  01M623QGQXR4ERM34GYV8H6DVR: the first-start screen; the settings
  file and the settings pane exist). Each check sends only `asqr/<version>` as its
  `User-Agent`, never queue content or paths. The spec's "no network"
  (`docs/spec.md:34-35`, and "Networking" under "Not in asqr yet")
  changes with an ADR to "no network unless the person allows it". No
  check runs before the person has answered that screen, so commands
  agents call never reach the network first. Jakob chose "yes"
  preselected at first and changed it to "no" the same day (no update
  check is switched on without the person choosing it). Rejected: "yes"
  preselected; a check only on request; on by default without asking.
- **The notice links to the GitHub release page.** asqr cannot tell how
  it was installed (no code detects it), and a wrong update command costs
  more than a link. Rejected: guessing the install method from the
  binary's path; printing all three commands.
- **GitHub releases answer "what is the newest version".** The release
  archives come from there and the notice links to the same page; the
  Homebrew tap may lag a release by its bump workflow. Rejected:
  crates.io; a version file of the project's own.
- **Where the notice shows needs a design round with rendered ideas:**
  somewhere the person sees easily and cannot miss.
- **Parked for now (Jakob):** only the decisions above are recorded;
  the work waits for the settings and the design round.

## Open questions

- Where and how the notice shows (the design round above).
- How often to check, and whether to keep the last answer to avoid a
  request on every start.
- The HTTP client: asqr has none (`Cargo.toml`, `Cargo.lock`). TLS in
  Rust usually brings C or assembly (`ring`, `aws-lc-rs`), which the
  rule of no dependency that is not Rust where a Rust one serves
  touches; spawning `curl` needs no dependency. Startup never waits for
  the network: a background thread with short timeouts.

## Re-checked (Jakob, 2026-10-31, in the chat)

Kept parked until the first-start screen (todo
01M623QGQXR4ERM34GYV8H6DVR) is built. Where the notice shows, what it
says and how often asqr checks are discussed when this todo is taken
up, not before. The options put to Jakob that day, for then: the notice
in the empty queue's key bar beside the version, a line of its own above
the key bar, or only in the settings pane; the text with the release
page, with the host only, or with both versions and no link; a check at
start and then at most once every 24 hours, at start only, or at start
with the time of the last check kept in a file.

## Revisit when

The first-start screen exists (todo 01M623QGQXR4ERM34GYV8H6DVR), and the
notice's design round has run.
