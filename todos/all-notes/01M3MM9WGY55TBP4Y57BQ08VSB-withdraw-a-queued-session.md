---
title: "Withdraw a queued session"
kind: feature
component: queue
status: needs-discussion
origin: request
---
# Withdraw a queued session

The user requested this on 2026-09-28 ("create a todos file
in asqr that we want a feature to withdraw a submitted session again").
It is not designed yet.

## The case that prompted it

An agent (the torchsnap mascot logbook) had asked a session whose texts
rendered badly (a paragraph after a raw block joined its line, fixed in
`5473101`). It rebuilt the same questions in a readable layout and asked them again
as a second session. The first one could not be taken back: asqr has no
command for it (`asqr --help` lists watch, paths, new, validate, ask,
wait, result, status, prune, skill, schema). The agent could only tell
the user to reject the old session. Meanwhile the user had already
answered the old one and then had to cancel the new one by hand, with
the reason "the answers for those questions i gave before already".

## What is wanted

A way for the asker to take back a session it dropped into the queue,
so the person no longer sees it.

## Questions to settle

- Command shape: for example `asqr withdraw <id>` (or `asqr ask
  --withdraw <id>`), with the usual `--queue`/`--dir`.
- A session the person has not opened: remove it from the inbox. What
  does a running `asqr` watcher do when a waiting session disappears
  (spec: inbox handling), and does it need to refresh its list?
- A session the person has started answering (a draft exists): refuse,
  as `ask` refuses to replace such a session, or allow with `--force`
  and tell the person in the TUI that the asker withdrew it?
- A session already answered: nothing to withdraw; exit code and
  message.
- What `asqr wait <id>` returns for a withdrawn session: probably a
  result with a new status (for example `withdrawn`) or exit code 13
  ("no result will come"), so a waiting agent stops cleanly.
- Whether a replacement should be one step: `asqr ask --replaces <id>`
  withdrawing the old session and dropping the new one, which is the
  case above.
- Update the skill (`skills/asqr/SKILL.md`, "Asking and waiting" and the
  exit code table) and the spec.
