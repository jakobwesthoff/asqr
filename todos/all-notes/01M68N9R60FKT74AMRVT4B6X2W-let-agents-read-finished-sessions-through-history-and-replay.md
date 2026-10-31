---
title: "Let agents read finished sessions through history and replay"
kind: feature
component: cli
origin: discussion
---
# Let agents read finished sessions through history and replay

Today an agent cannot get at a finished session's content through asqr.
`asqr history --json` prints per entry `ref`, `id`, `finished_at`,
`status`, `title`, `draft` and the counts of kept, gone and not kept
files, but no content and no place; `asqr replay <ref>` only opens the
TUI. After 30 days a session, its result and its kept files lie in a
month zip (ADR 96, ADR 100, ADR 101), so reading the archive directly
would tie an agent to the bundle format. The core of the skill already
tells agents to keep a record of the answers they act on and that
`asqr result` finds a result after it left the outbox; finding earlier
sessions and their files is the next step.

## Decisions (Jakob, 2026-10-31, in the chat)

The owner asked for this after the lead's proposal; the shape below is
that proposal.

- **Content, not paths.** asqr hands out the content, so the archive
  format stays internal. Rejected: printing the loose path or the
  bundle and entry name for an agent to read itself.
- **`asqr replay <ref> --json`** prints the archived session and its
  result as one JSON object, with each shown file's original path, its
  state (kept, gone, not kept, not recorded) and its fingerprint.
- **`asqr replay <ref> --extract <dir>`** writes the session, the result
  and the kept copies as plain files into `<dir>`, so an agent reads
  them with its own tools, images included.
- **The skill names both** in one line next to "Keep a record of the
  answers", together with `asqr history --json`, so an agent finds
  earlier answers and files.

## Open points for the plan

- The JSON shape (field names, how a missing result or a withdrawn
  draft shows), and whether `--json` and `--extract` combine.
- What `--extract` does with an existing `<dir>` and with a file name
  that two kept copies would share.
- Exit codes and messages, in line with `replay`'s refusals (too short,
  unmatched, ambiguous, unreadable bundle).
- Whether the skill line goes into the core (a rule no check catches)
  or into a reference part (ADR 82; the core is near its 17,980-byte
  cap).
