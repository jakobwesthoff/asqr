# 64. Let the asker withdraw a session

Date: 2026-10-28

## Status

Accepted

Amends [4. Hand sessions over through watched queue directories](0004-hand-sessions-over-through-watched-queue-directories.md)

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md)

## Context

An asker could not take back a session it had dropped into a queue. An
agent that rebuilt a session in a readable layout and asked it again
could only tell the person to reject the old one, and the person had
answered the old one by then. An agent whose question the person had
answered another way deleted the session's inbox file by hand, and had
to stop its own `asqr wait` separately.

## Decision

- `asqr withdraw <id>` takes a waiting session back, with the usual
  `--queue` and `--dir`. A command of its own says exactly what it does
  and fits `ask`, `wait` and `result`.
- A session whose draft has an answer is refused, as `ask` refuses to
  replace such a session. `--force` withdraws it anyway, and the TUI
  tells the person that the asker withdrew it.
- A withdrawn session gets a normal result with the status `withdrawn`,
  so every `asqr wait` ends cleanly, and `asqr result` returns the same
  later.
- `wait` and `result` exit with 14 for a withdrawn result, so a script
  tells "the asker took it back" from an unknown id (13).
- `withdraw` prints the result it wrote, as `wait` and `result` print a
  result: every command that ends a session gives one output shape.
- A session answered already is refused with exit 1, and the message
  names `asqr result <id>`: a withdraw that did not happen must not
  look like one that did. An id the queue does not know, or whose
  result is gone, exits with 13 as it does for `wait`.
- A withdrawn result keeps `answers` empty. What the person had entered
  and not sent goes into a separate `draft` field: their note to the
  whole session and the questions with an answer or a note, in the
  shape of a rejected result (ADR 53). The asker cannot tell from
  unsent answers whether the person would have submitted or rejected in
  the end, so they must not read as answers; yet the person should not
  have to type them twice. The skill tells the asker to use them only
  as presets of a replacement session, where the person confirms
  everything again, and to ignore them otherwise.
- The status and the `draft` field are added within format version 2,
  as ADR 51 adds context entry types within it. The result schema keeps
  its name.
- A withdrawal takes the finish path of ADR 4: the result, then the
  session into the archive, then the draft is deleted. Its look at the
  files and its writes run under the drafts lock of ADR 63, since the
  TUI and the CLI are separate processes and a check-then-write between
  them races.
- The TUI's notice goes by the result: with a `draft`, it says the
  person's answers went with the withdrawal; without one, while the TUI
  held answers, it says they are dropped; otherwise it only names the
  session.

## Consequences

- An asqr from before this decision refuses a withdrawn result: its
  `wait` and `result` print "not a result file" and exit with 1, its
  `status` lists the status as `null`, and its TUI takes the result for
  one of another session. The skill says so.
- A withdrawn result stays in the outbox like any result, so asking the
  same id again needs `ask --force`.
