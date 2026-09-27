# asqr specification (draft)

Status: reviewed with the user on 2026-09-27 (all review answers are
worked in). The decisions behind it are recorded as ADRs 2 to 20 in
`docs/adr/`.

## 1. Purpose

asqr (pronounced "asker") puts questions from another program in front
of a person and hands the answers back. Its main use is AI coding agents
that have prepared questions for the person working with them: the agent
writes the questions down, and the person answers them at their own pace
in a terminal that runs asqr. Scripts and builds can ask the same way.

The asqr file format is the part other tools build on. The TUI is one
way to answer those files, and the CLI and agent integration are ways to
ask.

Design goals:

- Any number of questions per session, and any number of options per
  question.
- Answering is fast: keyboard only, no dialogs to click through, and the
  person can move freely between questions before submitting.
- The asking side needs nothing but files. Wrappers (CLI, skill, MCP) are
  conveniences on top.
- Works for anyone: macOS and Linux, any terminal, no account, no
  network.

## 2. Concepts

- **Session**: one question file. It has an id, an optional title and
  intro, and a list of questions. It is answered and submitted as a
  whole.
- **Question**: one thing to decide. It has a kind (section 4), text,
  optional options, an optional image, and optional custom entry and
  note.
- **Result**: the answer file for one session.
- **Queue**: a directory pair (inbox and outbox) that one asqr instance
  watches. The default queue serves everything, and named queues keep
  projects apart.

## 3. Transport

asqr watches the inbox of a queue. The asking side writes a session file
into it and waits for the matching result file in the outbox.

```
<queue>/
  inbox/<session-id>.json      written by the asker
  outbox/<session-id>.json     written by asqr on submit
  drafts/<session-id>.json     answers in progress, kept across restarts
  archive/<session-id>.json    answered sessions, moved out of the inbox
```

- Writers create files atomically: write `<name>.tmp` and rename it.
  asqr ignores `*.tmp`.
- asqr picks up new files while running and shows the existing ones on
  start, oldest first.
- On submit asqr writes the result into the outbox and then moves the
  session file to `archive/`. The result belongs to the asker, who may
  delete it after reading.
- Nothing is deleted automatically. `asqr prune --older-than <duration>`
  removes archived sessions and their results on request (user, round 2).
- All four directories live under one root (user, round 2), so `asqr
  paths` reports a single place.
- A session file with the same id as a session already waiting replaces
  it, as long as nothing has been answered. Otherwise asqr rejects it
  with an error result.
- An id whose earlier session was already answered, with its result
  still in the outbox, is rejected, so an unread result is never lost.
  `asqr ask --force` moves the old result into the archive and accepts
  the new session (user, plan Q5).
- One asqr instance per queue: a lock file in the queue keeps a second
  instance out, and it names the process that holds the lock (user, plan
  Q10).
- An invalid session file gets an error result in the outbox (section
  6), so the asker never waits forever.

The default queue root is the platform's data directory, resolved with
the `directories` crate (`ProjectDirs`): `~/Library/Application
Support/asqr/` on macOS, `$XDG_DATA_HOME/asqr/` (usually
`~/.local/share/asqr/`) on Linux. Queues live in `queues/<name>/` below
it; the default queue is `default`. It can be changed with `--queue
<name>` (a named queue under the root), `--dir <path>` (any directory),
or the environment variables `ASQR_QUEUE` and `ASQR_DIR`.

`asqr paths [--queue <name>] [--json]` prints the root, the inbox,
outbox, drafts and archive directories of a queue and the log file, so an agent or script
finds them without knowing the platform rules.

Files only, no socket (user, Q1): dropping a file already triggers
asqr, it queues while asqr is not running, it survives crashes, and any
language can do it without a client library.

## 4. Session file (version 1)

```json
{
  "asqr": 1,
  "id": "alt-rework-3-batch-01",
  "title": "Alt rework 3, batch 1",
  "intro": "Pick the best line per mascot. 'More' sends it back with new variations.",
  "from": "claude-code, torchsnap mascots",
  "questions": [
    {
      "id": "301-holly-crown-green-robe-feast-ghost",
      "header": "301",
      "text": "holly-crown-green-robe-feast-ghost: Ghost of Christmas Present (A Christmas Carol)",
      "image": "../mascots/301-holly-crown-green-robe-feast-ghost.png",
      "kind": "single",
      "options": [
        { "id": "old",  "label": "Old",   "description": "Snappy ... -- ..." },
        { "id": "new1", "label": "New 1", "description": "Snappy ... -- ..." },
        { "id": "new2", "label": "New 2", "description": "Snappy ... -- ..." },
        { "id": "more", "label": "More",  "description": "None of these; new variations next batch." }
      ],
      "custom": { "label": "Own line", "length": { "target": [80, 125], "warn": 145, "max": 175 } },
      "note": true
    }
  ]
}
```

Fields:

- `asqr` (required): format version, `1`.
- `id` (optional): unique per queue; letters, digits, `-`, `_`, `.`.
  It names the result file. Ids are ULIDs by default (user, plan Q5):
  without an `id`, `asqr ask` assigns a fresh ULID and prints it, and
  `asqr new` prints a session skeleton with a fresh ULID. A custom id in
  the allowed syntax is still accepted. Ids asqr creates internally are
  ULIDs too.
- `title`, `intro`, `from`: optional texts shown in the session header.
- `questions` (required, at least one).

Per question:

- `id` (required, unique within the session), `text` (required),
  `header` (optional short label for the question list).
- `kind`: `single` (pick one), `multi` (pick several, with optional
  `min` and `max`), or `text` (free text only, no options). No other
  kinds in version 1 (user, Q5); `confirm`, `rank` and `number` were
  considered.
- `options`: for `single` and `multi`. Each has an `id`, a `label` and
  an optional `description` of any length, shown in full and wrapped.
  An optional `default: true` preselects an option.
- `image` (optional): a path to an image file. Most questions have none.
  Section 7 describes the display. `asqr ask` resolves relative paths
  against the file it was given and writes absolute ones into the inbox
  (user, round 2), since a relative path means nothing once the file sits
  in the inbox. Files dropped into the inbox directly must use absolute
  paths; `asqr validate` warns about relative ones.
- `custom` (optional): allows a typed answer instead of or next to the
  options. `true` or an object with `label`, `multiline` and `length`.
- `length` (optional, on `custom` and on `text` questions): `target`
  (a range shown in green), `warn` (above it the counter turns yellow),
  `max` (hard limit; asqr refuses more input). All are optional.
- `note` (optional, default `true`): whether the person may add a note.
  One note per question, whatever is selected (user, Q4).
- `required` (optional, default `false`): a question that must be
  answered before submit.

Text fields (`intro`, question `text`, option `description`) may use a
small Markdown subset: bold, italic, inline code, lists and line breaks
(user, Q6). Anything else shows as written.

The repo ships a JSON Schema (`schema/session.v1.json` and
`schema/result.v1.json`), `asqr schema` prints it, and `asqr validate
<file>` checks a file.

## 5. Result file

```json
{
  "asqr": 1,
  "id": "alt-rework-3-batch-01",
  "status": "submitted",
  "submitted_at": "2026-09-27T17:05:12+02:00",
  "answers": [
    {
      "question": "301-holly-crown-green-robe-feast-ghost",
      "selected": ["new1"],
      "custom": null,
      "note": "maybe shorter setup"
    },
    {
      "question": "302-...",
      "skipped": true
    }
  ]
}
```

- One result per session, written on submit. There are no partial
  submits (user, Q11); answers in progress stay in `drafts/`.
- `status`: `submitted` (the person submitted, possibly with skipped
  questions), `cancelled` (the person rejected the session; the optional
  `reason` holds why) or `error` (the file was invalid; `error` holds
  the message).
- Every question of the session appears exactly once, in session order.
- `selected` lists option ids; `custom` holds the typed text; `note` is
  the note; `skipped: true` marks an unanswered question.

## 6. Errors

An invalid session produces a result with `"status": "error"` and a
message naming the field and the problem. asqr also shows it in the TUI.
The asker never waits for a file that will not come.

## 7. The TUI

`asqr` (or `asqr watch`) starts on the default queue.

Layout:

```
+ asqr ── queue: default ────────── 2 sessions waiting ────────────+
| Alt rework 3, batch 1   (claude-code, torchsnap mascots)  3/16    |
+─────────────────────+────────────────────────────────────────────+
| > 301  new1         | 301-holly-crown-green-robe-feast-ghost:     |
|   302  -            | Ghost of Christmas Present (A Christmas ... |
|   303  more +note   |                                             |
|   304  own line     |  ( ) Old    Snappy ... -- ...          112  |
|   ...               |  (x) New 1  Snappy ... -- ...          118  |
|                     |  ( ) New 2  Snappy ... -- ...          131  |
|                     |  ( ) More   None of these ...               |
|                     |                                             |
|                     |  [image, if the question has one]           |
+─────────────────────+────────────────────────────────────────────+
| j/k move  space select  c own  n note  tab next  S submit  ? help |
+──────────────────────────────────────────────────────────────────+
```

- Left: the questions with their state. Right: the current question.
- Keys, vim style plus arrows (user, Q7): `j`/`k` or arrows move within
  options, `tab`/`shift-tab` or `J`/`K` move between questions, `space`
  or `enter` selects, digits `1`-`9` pick an option directly, `c` opens
  the custom entry, `n` the note, `esc` leaves a text field, `S` submits
  (with a summary of skipped questions first), `X` rejects the whole
  session after a confirmation, with an optional reason (user, plan Q7),
  `q` quits and keeps the draft, `?` shows help.
- Text fields are multi-line editors, with the length counter where the
  question sets `length`.
- The session list: when more than one session is waiting, a key opens
  the list to switch. Answers are saved as drafts continuously, so
  nothing is lost on quit or crash.
- New session arriving: a desktop notification (OSC 9 or 777, which
  Ghostty, iTerm2 and kitty support) and a terminal bell (user, Q8). Both
  can be switched off.
- Images: only when a question has one. They show inline where the
  terminal supports it (Kitty graphics protocol in Ghostty, kitty and
  WezTerm, the iTerm2 protocol, Sixel), through `ratatui-image`, which
  detects the protocol. Terminals without any fall back to a coarse block
  rendering. `o` opens the image in the system viewer and `z` shows it
  full screen. Placement adapts (user, Q9): a column to the right when
  the terminal is wide enough, below the options otherwise.

## 8. Asking from the command line

- `asqr new`: prints a session skeleton with a fresh ULID as its id.
- `asqr ask <file> [--queue <name>] [--wait] [--timeout <secs>]
  [--force]`:
  validates the file, drops it atomically into the inbox and prints the
  session id. With `--wait` it blocks until the result exists, prints it
  to stdout, and exits 0 for `submitted`, 1 for `cancelled` or `error`,
  and 2 on timeout.
- `asqr wait <id>`: waits for an existing session.
- `asqr result <id>`: prints a result if it is there.
- `asqr status`: lists waiting and answered sessions.
- `asqr paths`: section 3.
- `asqr validate <file>`, `asqr schema`: section 4.
- `asqr prune --older-than <duration>`: section 3.
- `asqr skill [--install <dir>]`: section 9.

## 9. Agents

Agents are the main askers. Version 1 ships the skill, and the MCP server
comes later as a thin layer on the same queue code (user, Q3):

- **A skill in the repo** (`skills/asqr/SKILL.md`): it explains the file
  format, `asqr ask --wait` in the background, how to wait without
  blocking the agent, and patterns for longer sessions (batches, follow-up
  rounds, keeping a record). It is cheap, works with any agent that can
  write files and run commands, and does not depend on a protocol.
- **An MCP server** (`asqr mcp`, stdio): tools such as `ask` (submit a
  session, returns the id), `wait_result` (wait up to N seconds, then
  return the result or "pending") and `status`. It is typed and easy to
  discover, but it needs configuring per agent. MCP calls have time
  limits, so a person answering for 20 minutes has to be bridged with
  repeated `wait_result` calls.

The skill documents `asqr paths` as the way to find the queue. It lives
in the repo at `skills/asqr/SKILL.md`, is compiled into the binary, and
`asqr skill` prints it or `asqr skill --install <dir>` writes it, so the
installed skill always matches the installed asqr (user, round 2).

## 10. Follow-up rounds

asqr itself has no loop. A follow-up is a new session. An optional
`follows` field names the previous session id, so the TUI can show "3
questions carried over from batch 1".

## 11. Not in version 1

- Networking, several people on one queue, sockets.
- The MCP server (later, section 9).
- `asqr answer <file>`, answering one file in the current terminal
  without a queue (later, user, Q12).
- Partial submits (user, Q11).
- Question kinds beyond `single`, `multi` and `text` (user, Q5).
- Editing a session while it is being answered, beyond replacing it
  before any answer (section 3).
- Windows support (it may work, but it is not tested).
- Formats other than JSON.
- A config file: version 1 is configured through flags and environment
  variables only (user, plan Q9).
- Publishing: asqr is used locally first and published on crates.io
  later (user, plan Q13).

## 12. Technology

Rust (edition 2024), minimum version 1.97 (`rust-version`, user, plan
Q4). One crate with a library (`src/lib.rs`) and a thin binary
(`src/main.rs`), so tests and the later MCP server reach the logic
without the terminal (user, plan Q3). Licence: MPL-2.0 (user, Q10).

Crates (user: use clap, anyhow and/or thiserror, and other best-practice
crates as needed):

| Need | Crate |
|---|---|
| Command line | `clap` with the derive feature |
| Errors | `thiserror` for the typed errors of the format and queue code (validation errors must name the field for the error result), `anyhow` with `.context()` at the binary's edges |
| TUI | `ratatui` with `crossterm`, `tui-textarea` for text fields, `ratatui-image` for images |
| Watching the inbox | `notify` (through `notify-debouncer-full`, so a write followed by a rename counts once) |
| Platform paths | `directories` |
| Format | `serde`, `serde_json`, `schemars` to derive the JSON Schema from the same types |
| Atomic writes | `tempfile` (`NamedTempFile::persist` in the target directory) |
| Timestamps | `jiff` (RFC 3339 with offset in results) |
| Logging | `tracing` and `tracing-subscriber`, writing to a log file in the platform cache directory, as stdout belongs to the TUI (user, plan Q6) |
| Markdown subset | `pulldown-cmark`, rendering only the allowed elements (user, plan Q8) |
| Session ids | `ulid` |
| Tests | `insta` for snapshot tests of rendered screens and results, `assert_cmd` for the CLI |

The exact versions come in with `cargo add` when the code needs them.

## 13. Development rules (user)

- Test driven from the first line: write the test, watch it fail (red),
  write the feature, watch it pass (green), then the next one.
- Zero clippy warnings from the start. `just check` runs `cargo fmt
  --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test` and `cargo llvm-cov`. A versioned git pre-commit hook runs
  it, and GitHub Actions runs it on Ubuntu and macOS.
- One commit per finished feature, with its tests; every commit passes
  the hook. Push at the end of each plan phase (user, plan Q1 and Q2).
- Every piece of code that is written or changed gets excellent test
  coverage. `cargo llvm-cov` reports it without a threshold (user);
  review checks that changes are covered.
- The TUI is tested through its state and a rendered test buffer
  (ratatui's `TestBackend` with `insta` snapshots), not by hand.
