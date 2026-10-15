---
title: "Write a user guide with screenshots, every key and the agent setup"
kind: docs
component: project
status: needs-discussion
origin: request
tags: [ux]
---
# Write a user guide with screenshots, every key and the agent setup

Requested by the user on 2026-10-15. asqr's documentation today is the
README (what asqr is, install, flags), the specification
(`docs/spec.md`, which describes behaviour precisely but reads as a
spec, not a guide) and the skill that `asqr skill` prints for agents.
There is no guide that shows a person how asqr looks and how to work
with it, and no step-by-step description of setting it up with an agent.

## What the guide should cover

- How asqr works, with screenshots of the real screens: the empty queue,
  a session with its tabs, options, own answers and notes, the context
  pane beside and below, the review tab, the help, the session list.
- Precise explanations of the interaction, including the less obvious
  keys. Example the user named: `ctrl-u`/`ctrl-d` (and
  `pgup`/`pgdn`) page the context pane even while it does not have
  the focus (`src/tui/scroll.rs`, the doc comment of `scroll`), while
  the line keys only scroll a focused pane.
- How to bring asqr into agents: the skill (`asqr skill`), where the
  queue lives, how an agent writes a session and reads the result, with
  a worked example for Claude Code and others.

## Open questions

- Form and place: a `docs/guide.md` in the repository, a section of the
  README, or a website.
- How the screenshots are made so they stay current: VHS tapes (see the
  related todo), or captures rendered to SVG/PNG as the TUI review did
  (tmux capture plus an ANSI-to-SVG renderer).
- Language and depth: a single guide, or a quick start plus a reference.
- Where the rendered mascot picture (`assets/talqi-512.webp`) gets a
  place of its own. The user wants it shown somewhere in the
  documentation (2026-10-15). The README's logo is the terminal emblem
  (`assets/emblem/talqi-terminal.svg`), and the README only links the
  picture where it explains Talqi's name.

## Relations

- Relates to: [Add a VHS demo GIF to the README](01M3HNJJXAVRC9TJWRTPPTFNC0-add-a-vhs-demo-gif-to-the-readme.md)
