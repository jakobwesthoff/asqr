# asqr specification (draft)

Status: reviewed with the user and in an adversarial review on
2026-09-27, with the findings F1 to F12 worked in. The decisions behind
it are recorded as ADRs 2 to 22 in `docs/adr/`.

## 1. Purpose

asqr (pronounced "asker") puts questions from another program in front
of a person and hands the answers back. Its main use is AI coding agents
that have prepared questions for the person working with them. The agent
writes the questions down, and the person answers them at their own pace
in a terminal that runs asqr. Scripts and builds can ask the same way.

The asqr file format is the part other tools build on. The TUI is one
way to answer those files, and the CLI and the agent integration are
ways to ask.

Design goals:

- Any number of questions per session, and any number of options per
  question.
- Answering is fast: keyboard only, no dialogs to click through, and the
  person can move freely between questions before submitting.
- The asking side needs nothing but files. Wrappers (CLI, skill, MCP)
  are conveniences on top.
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
- **Draft**: the answers to a session while it is being answered.
- **Queue**: a directory with an inbox, an outbox, drafts and an archive,
  watched by at most one asqr instance. The default queue serves
  everything, and named queues keep projects apart.

## 3. Queues and files

### 3.1 Layout

```
<queue>/
  inbox/<id>.json              sessions waiting, written by askers
  outbox/<id>.json             results, written by asqr, owned by the asker
  drafts/<id>.json             answers in progress
  archive/<id>.<ulid>.json     finished sessions (submitted, cancelled, error)
  lock                         the instance lock (section 3.6)
```

The four directories are created on first use.

### 3.2 Location

The default root is the platform's data directory, resolved with the
`directories` crate: `~/Library/Application Support/asqr/` on macOS,
`$XDG_DATA_HOME/asqr/` (usually `~/.local/share/asqr/`) on Linux. Named
queues live in `<root>/queues/<name>/`, and the default queue is
`default`.

- `--queue <name>` or `ASQR_QUEUE` picks a named queue under the root.
- `--dir <path>` or `ASQR_DIR` is the queue directory itself, the one
  with `inbox/` inside. `--queue` together with `--dir` is a usage error.
- Flags win over environment variables, and environment variables win
  over the default.

`asqr paths [--json]` prints the root (`none` with `--dir`), the queue
directory, its four directories, the lock file and the log file, so
agents and scripts find them without knowing the platform rules.

Queues on network or synced filesystems (NFS, Dropbox, iCloud Drive)
are not supported: the lock is per host (section 3.6).

### 3.3 Ids and file names

- The file stem is the session id: `inbox/batch-01.json` is session
  `batch-01`.
- Id syntax: letters, digits, `-`, `_` and `.`, not starting with `.`, at
  most 200 bytes.
- Ids are compared case-insensitively on every platform: `Batch-01` and
  `batch-01` are the same session for every rule in this section. A file
  keeps the spelling it was given. Lookups therefore search directory
  listings case-insensitively instead of building a path from the id.
- Ids asqr creates are ULIDs: the ids `asqr ask` assigns, the ids `asqr
  new` writes, and the suffix of archive names.

### 3.4 What the watcher considers

asqr only considers files named `inbox/<stem>.json` whose stem is a
valid id. Everything else is logged and ignored: `*.tmp`, dotfiles,
editor swap and backup files, `.DS_Store`.

- Waiting sessions are ordered by file modification time (the drop
  time), with the id as the tie-break.
- On start, the watcher is registered first and the inbox scanned
  second. A file dropped in between then shows up twice, which is
  harmless because the state is keyed by id.
- A changed inbox file is reloaded, and its draft is re-matched by
  question id and option id (section 5.3).

### 3.5 Writing into the inbox

`asqr ask` writes through a temp file in the inbox
(`tempfile::Builder::new().suffix(".tmp")`) and renames it into place.

- No session with that id is waiting: the file is placed with
  `persist_noclobber`, so two parallel `ask` calls with the same id
  cannot both win.
- A session with that id is waiting and its draft has no answer: `ask`
  replaces it (`persist`).
- A session with that id is waiting and its draft has at least one
  answer: `ask` refuses. It exits with 1, writes nothing and writes no
  error result.
- A result with that id is still in the outbox: `ask` refuses, so an
  unread result is never lost. `ask --force` first moves the old result
  into the archive and then drops the new session.

These are rules of `asqr ask`, not of the queue. A file someone drops
into the inbox by hand replaces whatever is there, draft or not. The
watcher never writes an error result for a replacement.

When no asqr holds the queue lock, `ask` still drops the file and warns
on stderr: "no asqr is watching queue <name>; start `asqr` in another
terminal".

### 3.6 The instance lock

One asqr instance per queue. On start the TUI opens `<queue>/lock` and
takes an exclusive advisory lock with `std::fs::File::try_lock` (flock
on Unix).

- `TryLockError::WouldBlock` means another instance runs. asqr prints
  the holder and exits. Any other error is a plain failure.
- The file holds the pid and host name of the holder, only for that
  message. The file stays after exit and its text may be stale. The
  lock itself is released by the kernel when the process ends, so there
  is no stale-lock handling.
- `ask`, `wait`, `result`, `status`, `paths` and `prune` do not take the
  lock. The TUI never reads the outbox or the archive after a submit.

### 3.7 Finishing a session

Submit, cancel (`X`) and an error result take one path. Each step can
be repeated safely:

1. Write the result into `outbox/<id>.json` (atomically). Every result
   carries `session_sha256`, the SHA-256 of the session file's bytes as
   read from the inbox.
2. Move the session file to `archive/<id>.<ulid>.json`. (A result that
   `ask --force` moves out of the way is archived as
   `<id>.<ulid>.result.json`.)
3. Delete the draft. This is the one thing asqr deletes on its own.

Step 1 never overwrites a result: an existing result with the same bytes
means the step already ran, any other result belongs to another session
and stops the finish with a conflict.

Recovery on start: for an inbox session whose outbox result has the same
`session_sha256`, the finish was interrupted, so steps 2 and 3 are
completed and the session is not shown again. With a different hash, the
file is a new session colliding with an unread result. It is moved to
the archive and logged, the TUI shows a notice, and no result is
written.

### 3.8 Invalid sessions

A file that fails validation gets a result with `"status": "error"`
naming the field and the problem, and is then archived, so it is handled
once. An error result never overwrites an existing `outbox/<id>.json`: in
that case the file is archived and the conflict logged. A file whose
stem is not a valid id gets no result at all, since no asker can be
waiting on that name. It is logged with its original name and archived as
`invalid.<ulid>.json`.

### 3.9 Cleaning up

Nothing is deleted automatically, apart from drafts in step 3 of section
3.7.

- `asqr prune --older-than <duration>` removes archive entries older than
  the duration, going by the ULID in their name. A duration is a whole
  number with one unit, `s`, `m`, `h`, `d` or `w`, such as `30d`. Names are parsed from
  the end, since the ULID is a fixed 26 characters and ids may contain
  `.`.
- `--results` also removes outbox results older than the duration. Its
  help text says plainly that unread results are removed too.
- Both print every file they remove.

Why files and not a socket: dropping a file already triggers asqr, files
queue while asqr is not running, they survive crashes, and any language
can write them without a client library.

## 4. Session file (version 1)

```json
{
  "asqr": 1,
  "id": "alt-rework-3-batch-01",
  "title": "Alt rework 3, batch 1",
  "intro": "Pick the best line per mascot. 'More' sends it back with new variations.",
  "from": "claude-code, torchsnap mascots",
  "follows": "alt-rework-3-batch-00",
  "questions": [
    {
      "id": "301-holly-crown-green-robe-feast-ghost",
      "header": "301",
      "text": "holly-crown-green-robe-feast-ghost: Ghost of Christmas Present (A Christmas Carol)",
      "image": "/Users/someone/mascots/301-holly-crown-green-robe-feast-ghost.png",
      "kind": "single",
      "options": [
        { "id": "old",  "label": "Old",   "description": "Snappy ... -- ..." },
        { "id": "new1", "label": "New 1", "description": "Snappy ... -- ..." },
        { "id": "new2", "label": "New 2", "description": "Snappy ... -- ..." },
        { "id": "more", "label": "More",  "description": "None of these; new variations next batch." }
      ],
      "custom": { "label": "Own line", "length": { "target": [80, 125], "warn": 145 } },
      "note": true
    }
  ]
}
```

Session fields:

- `asqr` (required): the format version, `1`. Any other value is the
  validation error "unsupported format version".
- `id` (optional): must equal the file stem when present (section 3.3).
  Without it, `asqr ask` assigns a fresh ULID, writes it into the file
  and names the file after it. A file dropped by hand without `id` has
  its stem as the id.
- `title`, `intro`, `from` (optional): shown in the session header.
- `follows` (optional): the id of an earlier session this one continues.
  The header shows `follows: <id>`.
- `questions` (required, at least one).

Question fields:

- `id` (required, unique within the session), `text` (required),
  `header` (optional, a short label for the question's tab).
- `kind`: `single` (pick one), `multi` (pick several, with optional `min`
  and `max`) or `text` (typed answer, no options). Version 1 has no other
  kinds.
- `options` (required for `single` and `multi`, not allowed for `text`):
  each option has an `id` (unique within the question), a `label`, an
  optional `description` of any length shown in full and wrapped, and an
  optional `default: true`.
- `custom` (optional, only on `single` and `multi`): allows a typed
  answer, always one line. `true`, or an object with `label` and
  `length`.
- `length` (optional, on `custom` and on `text` questions): `target` (a
  range) and `warn` (above it the counter warns). Both are optional, and
  `warn` must not be below the end of `target`. There is no hard limit:
  the counter guides, and input is never refused (user, 2026-09-27).
- `image` (optional): an absolute path to an image file. Most questions
  have none. `asqr ask` resolves relative paths against the file it was
  given and writes absolute ones. `asqr validate` warns about relative
  paths in files meant for dropping by hand.
- `note` (optional, default `true`): whether the person may add a note.
  One note per question.
- `required` (optional, default `false`): the question must be answered
  before submit (section 5.2).

Validation errors, besides missing or mistyped fields:

- no questions
- a duplicate question id or option id
- `options` on `text`, or missing or empty on `single` or `multi`
- `custom` or `default` on `text` (`custom: false` is allowed, since it
  switches the entry off)
- `min` or `max` on anything but `multi`
- a question-level `length` on anything but `text` (a custom entry sets
  its length inside `custom`)
- more than one `default` in a `single`, or more defaults than `max` in a
  `multi`
- `min` greater than `max`, or either outside `0..=options`
- a reversed `target` range, or `warn` below the end of `target`
- an `id` that differs from the file stem

Unknown fields are ignored when parsing, so later format versions can
add optional fields. `asqr validate` and `asqr ask` warn about them,
comparing the keys against the schema, so a typo like `requred` is
reported.

Text fields (`intro`, question `text`, option `description`) may use a
small Markdown subset: bold, italic, inline code, lists and line breaks.
Everything else is shown exactly as written in the source (section 7.6).

The repo ships the JSON Schema (`schema/session.v1.json`,
`schema/result.v1.json`). `asqr schema` prints it and `asqr validate
<file>` checks a file.

## 5. Results and drafts

### 5.1 Result file

```json
{
  "asqr": 1,
  "id": "alt-rework-3-batch-01",
  "status": "submitted",
  "submitted_at": "2026-09-27T17:05:12+02:00",
  "session_sha256": "9f2c...",
  "answers": [
    { "question": "301-...", "selected": ["new1"], "note": "maybe shorter setup" },
    { "question": "302-...", "selected": ["old"], "defaulted": true },
    { "question": "303-...", "skipped": true, "note": "skip, the image is wrong" },
    { "question": "304-...", "custom": "Snappy ... -- ..." }
  ]
}
```

- One result per session, written when the session is finished. There
  are no partial submits.
- `status`:
  - `submitted`: the person submitted, possibly with skipped questions
  - `cancelled`: the person rejected the session, with an optional
    `reason`
  - `error`: the file was invalid, and `error` holds the message
- `submitted_at` holds the time of finishing, as RFC 3339 with the
  offset. It is present for every status.
- For `submitted`, every question appears exactly once, in session
  order. For `cancelled` and `error`, `answers` is empty.

### 5.2 Answer state

A question is **answered** when:

- `single`: an option is selected, or the custom text is not empty after
  trimming
- `multi`: custom text that is not empty, or selected options whose
  number lies within `min` and `max` (the bounds count options only, so
  custom text alone answers the question)
- `text`: the typed text is not empty after trimming

Otherwise it is **skipped**.

- A default counts as answered, and the review tab shows it as
  "default". The answer carries `"defaulted": true` only when the person
  never edited the question. Any edit removes the flag, even one that
  restores the default.
- A note never answers a question and never satisfies `required`. A
  skipped question keeps its note.
- `min` and `max` only apply once something is selected, so a `multi`
  with `min: 2` can be skipped. Only `required` forces an answer, and
  `required` with `min` means at least `min`.
- `required` questions block submit until they are answered.

Answer shape per kind:

- `single`: `selected` with exactly one option id, or `custom`, never
  both. While answering, a chosen option and typed own-answer text can
  both exist; the option counts while it is chosen, and the typed text
  counts once no option is chosen (`enter` on the own-answer row
  deselects the options). The result keeps only what counts.
- `multi`: `selected` with the chosen option ids, and `custom` as one
  more entry next to them when present.
- `text`: `custom` holds the answer, and there is no `selected`.
- Every kind may carry `note`. A skipped question has `skipped: true` and
  may carry `note`.

### 5.3 Draft file

A draft has the result format with `"status": "draft"`, no
`submitted_at` and no `session_sha256`, plus `current`, the id of the
question the cursor is on. It is written on every change. On restore,
answers are matched by question id and option id, and anything that no
longer fits (a question or option that is gone) is dropped. A draft
"has an answer" (section 3.5) when at least one question is answered or
carries a note.

## 6. Errors

An invalid session produces an error result (section 3.8) naming the
field and the problem, and asqr also shows it in the TUI. While an asqr
watches the queue, the asker never waits for a result that will not
come. Without one, `ask` warns (section 3.5), and `--timeout` bounds the
wait.

## 7. The TUI

`asqr` (or `asqr watch`) starts on the queue chosen by section 3.2. It
takes the queue lock (section 3.6).

### 7.1 Layout

One column over the full width (user, 2026-09-27):

```
 asqr · queue: default · 2 sessions waiting
 Alt rework 3, batch 1  (claude-code, torchsnap mascots)
 ← … ☒ 300  ☐ 301  ☐ 302  ☐ 303 … ✔ Review →
 ──────────────────────────────────────────────────────────
 holly-crown-green-robe-feast-ghost: Ghost of Christmas
 Present (A Christmas Carol)
 pick one, or type your own

 ❯ 1. Old     Snappy in holly -- the old line.
   2. New 1   Snappy in holly -- the first new line.
   3. New 2   Snappy in holly -- the second new line.
   4. More    None of these; new variations next batch.
   5. ✎ Own line: Snappy in holly -- my own…   112/125

 Notes: —

 [image, if the question has one]
 ──────────────────────────────────────────────────────────
 ↑/↓ move  enter pick  ←/→ question  n note  ? help  q quit
```

- The tab bar lists every question by its `header` (or id) with `☒`
  once it is answered and `☐` while it is not, and `✔ Review` last. The
  current tab is highlighted. When the tabs do not fit, the bar scrolls
  around the current one and shows `←`/`→` where more follow.
- Option descriptions start after the label and wrap under it.
- The own-answer row is the last row of a question with `custom`; a
  `text` question consists of its answer field only.
- The notes line sits under the options (section 7.3).
- Everything adapts to the terminal: the question scrolls so the row
  under the cursor stays visible, dialogs never exceed the screen, and
  below a minimum size asqr shows a message instead of the layout.

### 7.2 Keys

On an option row (vim style plus arrows):

| Key | Action |
|---|---|
| `↑`/`↓`, `k`/`j` | move between the rows |
| `←`/`→`, `h`/`l` | previous and next question; the review tab is the last one |
| `enter` | `single`: pick the option and move on to the next question. `multi`: move on |
| `space` | `multi`: toggle the option |
| `1`-`9` | `single`: pick that option and move on. `multi`: toggle it |
| `n` | edit the note in its line |
| `L` | the list of waiting sessions |
| `o`, `z` | open the image in the system viewer; show it full screen |
| `q`, `ctrl-c` | quit, keeping the draft |
| `?` | help |

- A selection beyond `max` in a `multi` is refused with a message.
- `enter` on the last question moves on to the review tab.

### 7.3 Live fields and counters

Typed text is entered in place, in the line where it is shown, without
a frame (user, 2026-09-27):

- **The own-answer row** is a field as soon as the cursor lands on it:
  every key that produces text types into it. `←`/`→` move the text
  cursor, `↑`/`↓` leave the row, and `esc` leaves the field while the
  cursor stays. `enter` picks the own answer and moves on, like an
  option. It is one line and scrolls sideways when the text is longer
  than the line. On a `multi` question the own answer counts once it has
  text.
- **A `text` question's answer** is a field that is active when the
  question is shown. It grows with its lines; `ctrl-j` adds a line, and
  `enter` moves on.
- **The note** is edited in its line under the options after `n`. It is
  multi-line (`ctrl-j` adds a line); `enter` or `esc` leave it.
- Every edit is saved to the draft continuously; there is no discard.

Where the question sets `length`, a counter at the end of the field
shows the length as text, and colour is only added on top:

- `112/125` within the target
- `140/125 !` above the target, up to `warn`
- `182/125 !!` above `warn`

The number after the slash is the end of the target range, or `warn`
without a target. Green within the target, yellow above it, red above
`warn`. Input is never refused.

### 7.4 The review tab

The last tab lists every question with its answer, `default` for an
untouched default, `-` for a skipped question and `+note` for a note.
Unanswered `required` questions are marked. Below the list:

- **Submit**: `enter` submits, unless required questions are
  unanswered; then asqr names them and moves to the first one.
- **Reject**: a live field for the optional reason; `enter` rejects the
  session with it.

`enter` on a question in the list moves to that question.

### 7.5 Sessions and notifications

- When more than one session waits, `L` lists them in queue order
  (section 3.4) and switches between them.
- A new session triggers a desktop notification (OSC 9 or 777, which
  Ghostty, iTerm2 and kitty show) and a terminal bell. `--no-notify` and
  `--no-bell` switch them off.
- A session colliding with an unread result (section 3.7) triggers a
  notice.

### 7.6 Markdown

`pulldown-cmark` parses the text, and the renderer walks the events with
their source offsets (`into_offset_iter()`). Elements in the subset are
styled. For every other element the raw source slice is emitted once,
also when it is nested inside a supported element (a heading marker in a
list item).

### 7.7 Images

Images appear only when a question has one.

- `ratatui-image` shows them inline through the Kitty graphics protocol
  (Ghostty, kitty, WezTerm), the iTerm2 protocol or Sixel. Terminals
  without any of these get a coarse block rendering.
- The protocol is detected only in the binary (`main.rs`), by querying
  the terminal. The library takes the detected `Picker` as an argument,
  so tests never talk to a terminal.
- Placement adapts: a column to the right when the terminal is wide
  enough, below the options otherwise.
- A relative path, a missing file or an unreadable file shows a
  placeholder with the path. A path is never resolved against the
  working directory, and a broken image never becomes an error result.

Known limitation: inside tmux, inline images and OSC notifications need
`set -g allow-passthrough on`. Without it, asqr falls back to the block
rendering and the bell.

## 8. The command line

| Command | What it does |
|---|---|
| `asqr` / `asqr watch` | the TUI (section 7) |
| `asqr new` | prints a session skeleton with a fresh ULID as its id |
| `asqr ask <file> [--wait] [--timeout <secs>] [--force]` | validates, assigns a ULID when the id is missing, makes image paths absolute, drops the file (section 3.5), prints the id |
| `asqr wait <id> [--timeout <secs>]` | waits for the result and prints it; blocks without `--timeout` |
| `asqr result <id>` | prints the result if it is there |
| `asqr status [--json]` | waiting sessions (with or without a draft answer), answered sessions (results in the outbox), and the lock holder |
| `asqr paths [--json]` | section 3.2 |
| `asqr validate <file>`, `asqr schema` | section 4 |
| `asqr prune --older-than <duration> [--results]` | section 3.9 |
| `asqr skill [--install <dir>]` | prints the skill, or writes it to `<dir>/asqr/SKILL.md` (section 9) |

Every command takes `--queue` or `--dir` (section 3.2).

With `--wait`, the id goes to stderr and stdout carries only the result
JSON, so the output can be piped into `jq`.

Exit codes, the same for every command:

| Code | Meaning |
|---|---|
| 0 | success: the result is `submitted`, or the file was dropped (`ask` without `--wait`), or the command did its job |
| 1 | failure of asqr itself (I/O, a refused `ask`, a held lock) |
| 2 | usage error (clap) |
| 10 | the result is `cancelled` |
| 11 | the result is `error`, or `ask` got a file that fails validation |
| 12 | timeout |
| 13 | unknown id: nothing in the inbox, outbox or archive (`wait`, `result`) |

## 9. Agents

Agents are the main askers. Version 1 ships a skill, and an MCP server
comes later as a thin layer on the same library.

The skill lives in the repo at `skills/asqr/SKILL.md` in the Agent Skills
format (`name` and `description` frontmatter, a plain Markdown body). It
is compiled into the binary, so `asqr skill` prints the version that
matches the installed asqr. `asqr skill --install <dir>` takes a skills
root and writes `<dir>/asqr/SKILL.md`, so `asqr skill --install
.claude/skills` works.

The skill teaches:

- when asking through asqr beats asking in the chat
- `asqr paths`, `asqr new` and the format, with a short example
- the main waiting pattern, which works under any command time limit:
  `asqr ask`, then a loop of `asqr wait <id> --timeout <secs>` until the
  exit code is not 12
- as a variant for agents that run commands in the background: `asqr ask
  --wait` in the background
- the exit codes, the "no asqr is watching" warning, which the agent
  should pass on to the person, and `asqr status --json` to find sessions
  it lost track of
- longer sessions: batches, follow-up rounds with `follows`, and keeping
  a record of the answers

The planned MCP server (`asqr mcp`, stdio) offers `ask`, `wait_result`
(waits up to N seconds, then returns the result or "pending") and
`status`. MCP calls have time limits, so long answering times are
bridged with repeated `wait_result` calls.

## 10. Follow-up rounds

asqr itself has no loop. A follow-up is a new session, and its optional
`follows` field names the previous one. The header shows it. asqr does
not count or match carried-over questions.

## 11. Not in version 1

- Networking, several people on one queue, sockets, queues on network or
  synced filesystems.
- The MCP server (section 9).
- `asqr answer <file>`, answering one file in the current terminal
  without a queue.
- Partial submits.
- Question kinds beyond `single`, `multi` and `text`.
- A tmux passthrough hint in the TUI (todo).
- Windows support (it may work, but it is not tested).
- Formats other than JSON.
- A config file: flags and environment variables only.
- Publishing: asqr is used locally first and goes to crates.io later.

## 12. Technology

Rust (edition 2024), minimum version 1.97. One crate with a library
(`src/lib.rs`) and a thin binary (`src/main.rs`), so tests and the later
MCP server reach the logic without the terminal. Licence: MPL-2.0.

| Need | Crate |
|---|---|
| Command line | `clap` with the derive feature |
| Errors | `thiserror` for the typed errors of the format and queue code (validation errors name the field), `anyhow` with `.context()` at the binary's edges |
| TUI | `ratatui` with `crossterm`, `ratatui-textarea` (the maintained continuation of `tui-textarea`, ADR 9), `ratatui-image` |
| Watching the inbox | `notify` with `notify-debouncer-full` |
| Platform paths | `directories` |
| Format | `serde`, `serde_json`, `schemars` (the JSON Schema is derived from the same types) |
| Atomic writes | `tempfile` (`.tmp` suffix, `persist_noclobber` and `persist` in the target directory) |
| Hashes | `sha2` for `session_sha256` |
| Locking | `std::fs::File::try_lock` (no crate) |
| Timestamps | `jiff` (RFC 3339 with the offset) |
| Logging | `tracing` and `tracing-subscriber`, into a file in the platform cache directory |
| Markdown subset | `pulldown-cmark` |
| Session ids | `ulid` |
| Tests | `insta` for snapshots of rendered screens and results, `assert_cmd` for the CLI |

Crates come in with `cargo add` when the first test needs them.

## 13. Development rules

- Test first: write the test, watch it fail (red), write the feature,
  watch it pass (green), then the next one.
- `just check` is the pipeline the pre-commit hook runs: license
  notices, `cargo fmt --check`, `cargo clippy --all-targets
  --all-features -- -D warnings`, `cargo test --all-targets` and `cargo
  test --doc`. Zero warnings, from the start.
- Coverage runs in CI and on demand with `just coverage` (`cargo
  llvm-cov`), not in the hook (ADR 21). It is reported without a
  threshold, and review checks that every written or changed line is
  covered.
- GitHub Actions runs `just check` and `just coverage` on Ubuntu and
  macOS.
- One commit per finished feature, with its tests. Push at the end of
  each plan phase.
- The TUI is tested through its state and ratatui's `TestBackend` with
  `insta` snapshots, not by hand.
