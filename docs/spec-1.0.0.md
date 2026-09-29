---
title: asqr specification
version: 1.0.0
format: 1
status: draft
---

# asqr 1.0.0 specification (draft)

This file describes asqr 1.0.0, the first release, with version 1 of
the session and result format. A later version of asqr gets a
specification file of its own.

Status: reviewed with the user and in an adversarial review on
2026-09-27, with the findings F1 to F12 worked in, and revised with the
decisions made while building it. The decisions behind it are recorded
as ADRs 2 to 23 in `docs/adr/`.

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
  optional options (each with an optional image), an optional image, and
  optional custom entry and note.
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

The default root is the platform's data directory:
`~/Library/Application Support/asqr/` on macOS, `$XDG_DATA_HOME/asqr/`
(usually `~/.local/share/asqr/`) on Linux. Named queues live in
`<root>/queues/<name>/`, and the default queue is `default`.

- `--queue <name>` or `ASQR_QUEUE` picks a named queue under the root. A
  queue name follows the id syntax (section 3.3).
- `--dir <path>` or `ASQR_DIR` is the queue directory itself, the one
  with `inbox/` inside.
- `--queue` together with `--dir` is a usage error, and so are
  `ASQR_QUEUE` and `ASQR_DIR` together. Empty variables count as unset.
- Flags win over environment variables as a whole (`--dir` with
  `ASQR_QUEUE` set is fine), and environment variables win over the
  default. A platform without a data directory is a usage error unless
  `--dir` or `ASQR_DIR` names the queue.
- The queue is resolved for every command, so a bad `ASQR_QUEUE` is a
  usage error even for commands that do not touch a queue.

`asqr paths [--json]` prints the root (`none` with `--dir`, `null` in
JSON), the queue directory, its four directories, the lock file and the
log file, so agents and scripts find them without knowing the platform
rules. The JSON keys are `root`, `queue`, `inbox`, `outbox`, `drafts`,
`archive`, `lock` and `log`.

Commands log the sessions they drop, finish, archive or prune into
`asqr.log` in the platform's cache directory (ADR 15). Draft saves are
not logged. A log that cannot be written never stops a command.

Queues on network or synced filesystems (NFS, Dropbox, iCloud Drive)
are not supported: the lock is per host (section 3.6).

### 3.3 Ids and file names

- The file stem is the session id: `inbox/batch-01.json` is session
  `batch-01`.
- Id syntax: letters, digits, `-`, `_` and `.`, not starting with `.`, at
  most 200 bytes.
- Ids are compared case-insensitively on every platform: `Batch-01` and
  `batch-01` are the same session for every rule in this section. A file
  keeps the spelling it was given, and so does its draft. Lookups search
  directory listings case-insensitively instead of building a path from
  the id.
- Ids asqr creates are ULIDs: the ids `asqr ask` assigns, the ids `asqr
  new` writes, and the suffix of archive names.

### 3.4 What the watcher considers

asqr only considers files named `inbox/<stem>.json` whose stem is a
valid id. Everything else is ignored and logged once per run: `*.tmp`,
dotfiles, editor swap and backup files, `.DS_Store`.

- Waiting sessions are ordered by file modification time (the drop
  time), with the id as the tie-break.
- On start, the watcher is registered first and the inbox scanned
  second. A file dropped in between then shows up twice, which is
  harmless because the state is keyed by id.
- Every change in the inbox leads to a scan of the whole inbox, which
  works out what arrived, changed or left.
- A changed inbox file is reloaded, and the answers given so far are
  matched to it by question id and option id (section 5.3).
- A file that leaves the inbox without a result (taken back by hand)
  leaves the TUI. Its draft stays, and applies again if the session is
  dropped once more.

### 3.5 Writing into the inbox

`asqr ask` writes the session into a temp file in the inbox, whose
`.tmp` suffix the watcher ignores, and renames it into place.

What `ask` drops is a copy of the asker's file: pretty-printed, with the
`id` filled in and image paths made absolute. The asker's own file is
never changed. `session_sha256` in the result (section 3.7) is the hash
of that copy.

- No session with that id is waiting: the rename only succeeds while no
  file of that name exists, so two parallel `ask` calls with the same id
  cannot both win. The loser exits with 1.
- A session with that id is waiting and its draft has no answer: `ask`
  replaces it.
- A session with that id is waiting and its draft has an answer
  (section 5.3): `ask` refuses.
- A result with that id is still in the outbox: `ask` refuses, so an
  unread result is never lost. `ask --force` moves the old result into
  the archive and then drops the new session.

A refused `ask` exits with 1 and names the reason on stderr. A session
being answered and an unread result are refused before anything moves,
even with `--force`. Only the loser of a race under `--force` has
already archived the old result.

These are rules of `asqr ask`, not of the queue. A file someone drops
into the inbox by hand replaces whatever is there, draft or not. A
replacement is never an error in itself; a replacing file that fails
validation gets its error result like any invalid file (section 3.8),
and the session it replaced leaves the TUI.

When no asqr holds the queue lock, `ask` still drops the file and warns
on stderr: "no asqr is watching queue <name>; start `asqr` in another
terminal". For a queue given by directory, the warning names the
directory.

### 3.6 The instance lock

One asqr instance per queue. On start the TUI opens `<queue>/lock` and
takes an exclusive advisory lock on it (flock on Unix).

- A lock that stays held for about 200 ms means another instance runs. asqr
  prints "another asqr is watching this queue (pid <pid> on <host>)" and
  exits with 1. Any other error is a plain failure.
- The file holds the pid and host name of the holder, only for that
  message. The file stays after exit and its text may be stale. The
  lock itself is released by the kernel when the process ends, so there
  is no stale-lock handling.
- `ask`, `wait`, `result`, `status`, `paths` and `prune` never hold the
  lock. `ask` and `status` only test it for a moment to see whether an
  asqr is watching; the retries above keep such a test from refusing a
  starting TUI.
- The TUI never reads the outbox or the archive after a submit.

### 3.7 Finishing a session

Submit, reject (both on the review tab) and an error result take one
path. Each step can be repeated safely:

1. Write the result into `outbox/<id>.json` (atomically). Every result
   carries `session_sha256`, the SHA-256 of the session file's bytes as
   read from the inbox.
2. Move the session file to `archive/<id>.<ulid>.json`. (A result that
   `ask --force` moves out of the way is archived as
   `<id>.<ulid>.result.json`.)
3. Delete the draft. This is the one thing asqr deletes on its own.

Step 1 never overwrites a result: an existing result with the same bytes
means the step already ran, any other result belongs to another session
and stops the finish with a conflict, which the TUI shows as a notice.

Before step 1 the TUI reads the session file again. When it changed
since it was shown, nothing is written: the new version is loaded, and a
notice says the session changed on disk and was not sent. A result
therefore always answers the bytes the person saw.

Recovery runs whenever asqr reads the inbox, on start and after every
change. For an inbox session whose outbox result has the same
`session_sha256`, the finish was interrupted, so steps 2 and 3 are
completed and the session is not shown again. With a different hash, the
file is a new session colliding with an unread result. It is moved to
the archive and logged, the TUI shows a notice, and no result is
written. A file that cannot be read is left for the scan to report.

### 3.8 Invalid sessions

A file that fails validation gets a result with `"status": "error"`
whose `error` names the field and the problem (several problems joined
with `; `), and is then archived, so it is handled once. A file that is
not JSON, or not shaped like a session, gets the message "not a session
file: " followed by the parser's message. The TUI shows a notice for
an invalid session file, also when no session is waiting. The status
line holds one notice, so when several files fail in one scan, the last
one shows.

An error result never overwrites an existing `outbox/<id>.json`: in that
case the file is archived without a result and the conflict is logged
and shown. A file whose stem is not a valid id gets no result at all,
since no asker can be waiting on that name. It is logged with its
original name and archived as `invalid.<ulid>.json`, without a notice.

A session file that cannot be read (permissions) stays in the inbox and
is reported with a notice.

### 3.9 Cleaning up

Nothing is deleted automatically, apart from drafts in step 3 of section
3.7. Drafts of sessions that left the inbox stay (section 3.4).

- `asqr prune --older-than <duration>` removes archive entries older than
  the duration, going by the ULID in their name. A duration is a whole
  number with one unit, `s`, `m`, `h`, `d` or `w`, such as `30d`. Names
  are parsed from the end, since the ULID is a fixed 26 characters and
  ids may contain `.`. A malformed duration is a usage error.
- `--results` also removes files in the outbox last modified before the
  duration. Its help text says plainly that unread results are removed
  too.
- Drafts are never pruned.
- Both print every file they remove, as `removed <path>`.

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
- `id` (optional): a valid id (section 3.3). In the inbox it must equal
  the file stem, ignoring case. Without it, `asqr ask` assigns a fresh
  ULID and names the dropped file after it. A file dropped by hand
  without `id` has its stem as the id. The name of the file given to
  `asqr ask` or `asqr validate` does not matter.
- `title`, `intro`, `from` (optional): shown in the session header. The
  header shows the id when there is no `title`.
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
  optional `description` of any length shown in full and wrapped, an
  optional `default: true`, and an optional `image`, shown in place of
  the question's image while the cursor is on the option (section 7.7).
- `custom` (optional, only on `single` and `multi`): allows a typed
  answer, always one line. `true`, or an object with `label` and
  `length`.
- `length` (optional, on `custom` and on `text` questions): `target` (a
  range) and `warn` (above it the counter warns). Both are optional, and
  `warn` must not be below the end of `target`. There is no hard limit:
  the counter guides, and input is never refused (user, 2026-09-27).
- `image` (optional): an absolute path to an image file. Most questions
  have none. `asqr ask` resolves relative paths against the file it was
  given and drops absolute ones. `asqr validate` warns about
  relative paths, which a file dropped by hand must not use. `asqr
  validate` and `asqr ask` both warn about image files that do not
  exist, and about files the terminal UI cannot decode (section 7.7).
  That check reads only the file's header. For an unsupported format the
  warning lists the formats that work. All of this applies to an option's
  `image` too.
- `note` (optional, default `true`): whether the person may add a note.
  One note per question.
- Every question is optional (ADR 23): there is no way to force an
  answer, and a skipped question comes back as `skipped: true`. Files
  that still set the former `required` field parse as before; the field
  is ignored and reported as unknown.

Validation errors, besides missing or mistyped fields:

- an unsupported format version, or an `id` that is no valid id
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
- in the inbox: an `id` that differs from the file stem

Unknown fields are ignored when parsing, so later format versions can
add optional fields. `asqr validate` and `asqr ask` warn about them,
comparing the keys against the schema, so a typo like `requred` is
reported.

`asqr validate <file>` prints `<file>: valid` and exits with 0, or lists
every error and exits with 11. Warnings go to stderr and never fail it.
A file that is not JSON, or not shaped like a session, exits with 11 as
well; a file that cannot be read exits with 1.

Text fields (`intro`, question `text`, option `description`) may use a
small Markdown subset: bold, italic, inline code, lists and line breaks.
Everything else is shown exactly as written in the source (section 7.6).

The repo ships the JSON Schema (`schema/session.v1.json`,
`schema/result.v1.json`). `asqr schema` prints the session schema and
`asqr schema --result` the result schema.

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
    `reason` (trimmed; absent when empty)
  - `error`: the file was invalid, and `error` holds the message
- `submitted_at` holds the time of finishing, as RFC 3339 with the
  offset. It is present for every status.
- For `submitted`, every question appears exactly once, in session
  order. For `cancelled` and `error`, `answers` is empty.

### 5.2 Answer state

A question is **answered** when:

- `single`: an option is selected, or the custom text is not empty after
  trimming
- `multi`: the custom text is not empty after trimming, or the number of
  selected options lies within `min` and `max`. The bounds count options
  only, so custom text answers the question whatever is selected.
- `text`: the typed text is not empty after trimming

Otherwise it is **skipped**.

- A default counts as answered, and the review tab marks it as a
  default. The answer carries `"defaulted": true` only when the person
  never edited the question. Any edit removes the flag, even one that
  restores the default, and so does writing a note.
- A note never answers a question. A skipped question keeps its note.
- `min` and `max` only apply once something is selected, so a `multi`
  with `min: 2` can be skipped. They count the picked options; the own
  answer of a `multi` comes on top of them (ADR 23).
- Nothing blocks a submit: every question is optional.
- Typed text and notes go into the result as typed; only the check
  whether they are empty trims them.

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
question the cursor is on (the last question while the review tab is
shown), and `reason`, the reject reason typed so far, when there is one.
It is written on every change. On restore, answers are matched by
question id and option id, and anything that no longer fits (a question
or option that is gone) is dropped. A draft that cannot be read is
logged and ignored, and the session starts fresh, with only its defaults
selected.

A draft "has an answer" (section 3.5) when any question has a selection,
typed text that is not blank or a note that is not blank, or when the
draft cannot be read. A selection counts even where it does not answer
the question yet, since it is still work a replacement would throw
away.

## 6. Errors

An invalid session produces an error result (section 3.8) naming the
field and the problem, and asqr also shows it in the TUI. While an asqr
watches the queue, the asker never waits for a result that will not
come: `wait` also ends when the session leaves the inbox without a
result, or when only its archive entry is left (section 8). Without a
watching asqr, `ask` warns (section 3.5), and `--timeout` bounds the
wait.

## 7. The TUI

`asqr` (or `asqr watch`) starts on the queue chosen by section 3.2. It
needs a terminal on stdin and stdout; otherwise it prints "asqr needs a
terminal" and exits with 1. It creates the queue's directories and
takes the queue lock (section 3.6). `q` quits with exit code 0 and gives
the terminal back as it was, also after a crash.

### 7.1 Layout

One column over the full width (user, 2026-09-27):

```
 asqr · queue: default · 2 sessions waiting
 Alt rework 3, batch 1  (claude-code, torchsnap mascots)
 Pick the best line per mascot. More sends it back with new variations.
 ← ☒ 300  ☐ 301  ☐ 302  ☐ 303  ✔ Review
 ──────────────────────────────────────────────────────────
 holly-crown-green-robe-feast-ghost: Ghost of Christmas
 Present (A Christmas Carol)
 pick one, or type your own

 ❯ ( ) 1. Old     Snappy in holly -- the old line.
   ( ) 2. New 1   Snappy in holly -- the first new line.
   ( ) 3. New 2   Snappy in holly -- the second new line.
   ( ) 4. More    None of these; new variations next batch.
   (•) ✎ Own line: Snappy in holly -- my own…

 Notes: —

 [image, if the question or one of its options has one]
 [notices and messages]
 ↑/↓ move  enter pick  ←/→ question  n note  ? help  q quit
```

- The title bar names the queue and counts the waiting sessions. A queue
  name too long for the line is cut from the left with `…`, at a `/`
  where possible, so the count stays visible.
- The session header shows the title (or the id), `from` and `follows`
  in one line, and the intro under it: at most three lines, one on
  terminals below 24 rows, ending in `…` when it is cut.
- The tab bar lists every question by its `header` (or id) with `☒`
  once it is answered and `☐` while it is not, and `✔ Review` last. The
  current tab is highlighted. When the tabs do not fit, the bar scrolls
  around the current one and shows `←`/`→` where more follow.
- Under the question text a hint says how to answer: "pick one", "pick
  between 2 and 3 items", "pick at least 2 items", "pick up to 3 items",
  "pick 2 items", "pick any" or "type your answer". A `single` with
  `custom` adds ", or type your own"; a `multi` does not mention its own
  answer, which people find on their own (ADR 23).
- Options show `( )`/`(•)` in a `single` and `[ ]`/`[x]` in a `multi`.
  Option descriptions start after the widest label and wrap under
  themselves; where that leaves less than 30 columns, they go below the
  label.
- The own-answer row is the last row of a question with `custom`; its
  mark shows whether the typed text counts. A `text` question consists
  of its answer field only.
- The notes line sits under the options (section 7.3).
- The line above the key bar shows notices (about the queue) and
  messages (about the last key) until the next key.
- The key bar shows the keys that work where the cursor is; they change
  inside a field.
- Everything adapts to the terminal: the question scrolls so the row
  under the cursor stays visible, dialogs never exceed the screen, and
  below 60×15 asqr shows "asqr needs at least 60×15; this terminal is
  W×H." instead of the layout.
- With nothing waiting, the screen says "Nothing to answer. New sessions
  show up here as soon as they arrive." Notices still show.

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

- A selection beyond `max` in a `multi` is refused with the message "at
  most N options".
- `enter` on the last question moves on to the review tab.
- `n` on a question with `note: false` says that the question takes no
  note.
- Inside a field every key that produces text types, so `n`, `q` and
  the others work again after `esc`. `ctrl-c` quits everywhere, except
  over the help, which it closes like any other key.
- `o` opens the image shown (section 7.7) with the system's opener
  (`open` on macOS, `xdg-open` elsewhere); when the opener cannot be
  started, a notice says why. `z` shows the image shown over the
  question's area and toggles back; changing the question ends it.
- Help closes on any key.
- The session list (`L`) lists every waiting session in queue order
  (section 3.4): `↑`/`↓` or `k`/`j` move, `enter` switches to the
  session, `esc` or `L` close the list, and `q` quits asqr.

### 7.3 Live fields and counters

Typed text is entered in place, in the line where it is shown, without
a frame (user, 2026-09-27):

- **The own-answer row** is a field as soon as the cursor lands on it:
  every key that produces text types into it. `←`/`→` move the text
  cursor, `↑`/`↓` leave the row, and `esc` leaves the field while the
  cursor stays. `←` with the text cursor at the start of the text goes
  to the previous question, `→` at its end to the next one (ADR 23). After `esc`, `↑`/`↓` move on as usual; where the move
  cannot leave the row (the single row of a `text` question, `↓` on the
  last row), they focus the field again. `enter` picks the own answer
  and moves on, like an option. It is one line and scrolls sideways when
  the text is longer than the line. On a `multi` question the own answer
  counts once it has text.
- **A `text` question's answer** is a field that is active when the
  question is shown. It grows with its lines; `ctrl-j` adds a line, and
  `enter` moves on. `←`/`→` at the start or end of its text switch
  questions as on the own-answer row.
- **The note** is edited in its line under the options after `n`. It is
  multi-line (`ctrl-j` adds a line); `enter` or `esc` leave it.
- **The reject reason** on the review tab is a one-line field like the
  own answer.
- Every edit is saved to the draft continuously, the reject reason
  included; there is no discard.

Where the question sets `length`, a counter at the end of the focused
field shows the length as text, and colour is only added on top:

- `112/125` within the target
- `140/125 !` above the target, up to `warn`
- `182/125 !!` above `warn`

The length counts characters, a line break as one. The number after the
slash is the end of the target range, or `warn` without a target. Green
within the target, yellow above it, red above `warn`. Input is never
refused.

### 7.4 The review tab

The last tab lists every question with its answer on one line: the
chosen options by label, own text and a text answer in quotes (line
breaks shown as `⏎`), `(default)` after an untouched default, `-`
for a skipped question and `+note` for a note. A line longer than the
screen is cut with `…`. Below the list, the counts of answered, skipped
and defaulted questions, then:

- **Submit**: `enter` submits. The review tab opens with the cursor
  here (ADR 23).
- **Reject**: a live field for the optional reason; `enter` rejects the
  session with it.

`enter` on a question in the list moves to that question.

After a submit or reject the session after it in the queue takes over,
or the one before when it was the last, or the empty screen when none
is left.

### 7.5 Sessions and notifications

- `L` lists the waiting sessions and switches between them (section
  7.2).
- A new session triggers a desktop notification (OSC 9 or 777, which
  Ghostty, iTerm2 and kitty show) and a terminal bell. `--no-notify` and
  `--no-bell` switch them off. The notification is OSC 777 when `TERM`
  starts with `rxvt` or `foot`, and OSC 9 otherwise. It names the session
  ("new session <id>"), or counts them ("N new sessions") when several
  arrive at once; OSC 9 puts "asqr: " in front, OSC 777 uses "asqr" as
  the title.
  Sessions waiting when asqr starts, and replaced files, trigger nothing.
- A session colliding with an unread result (section 3.7), an invalid
  file (section 3.8) and a failure to write a draft or a result trigger
  a notice. A failure never ends asqr; answers stay in the TUI.

### 7.6 Markdown

Elements in the subset (section 4) are styled. Every other element is
shown once as written in the source, also when it is nested inside a
supported element (a heading marker in a list item).

### 7.7 Images

Images appear only when a question or one of its options has one.

- The option under the cursor shows its own image, so the person can
  browse the options before picking; this holds for `single` and
  `multi`. Every other row, and an option without an image, shows the
  question's image, or nothing when the question has none.
- The room for the image stays while the cursor moves, also when the
  current row shows none, so the layout does not jump.
- The review shows no images.

- Terminals that support the Kitty graphics protocol (Ghostty, kitty,
  WezTerm), the iTerm2 protocol or Sixel show images inline. Terminals
  without any of these get a coarse block rendering.
- asqr decodes PNG, JPEG, GIF (first frame only), WebP, BMP, TIFF, ICO,
  TGA, PNM, QOI, DDS, OpenEXR, HDR and farbfeld. It detects the format
  from the file's content, and from the extension when the content does
  not match a known signature. SVG and AVIF are not decoded. The set follows
  the features of the `image` crate, and a test fails when it changes.
- The protocol is detected once at start, by querying the terminal.
- Placement adapts: beside the question, taking two fifths of the
  width, from 100 columns of content on; below the options otherwise,
  in a strip of a third of the height, at most 12 rows.
- An image is read once and kept until asqr's terminal gains the focus
  again. asqr asks the terminal for focus reports; on regaining the
  focus it reads and sends every image again as it is next shown.
- A relative path, a missing file or a file that cannot be read or
  decoded shows a placeholder saying "image not shown" with the path and
  the reason. A path is never resolved against the working directory,
  and a broken image never becomes an error result.

Known limitation: inside tmux, inline images and OSC notifications need
`set -g allow-passthrough all`. Without passthrough, asqr falls back to
the block rendering and the bell. With `on`, tmux passes sequences only
while asqr's pane is visible: an image first shown while the pane is
hidden stays empty until the pane gains the focus again, which asqr
only learns when tmux forwards focus reports (`set -g focus-events on`).
Inside
tmux, asqr asks tmux whether passthrough
is on for its pane and whether a client is attached, and queries the
terminal only when both hold: otherwise the query gets no answer, and
its reader would go on taking the keys typed afterwards. Notifications
inside tmux go out in tmux's passthrough wrapper.

## 8. The command line

| Command | What it does |
|---|---|
| `asqr` / `asqr watch [--no-notify] [--no-bell]` | the TUI (section 7) |
| `asqr new` | prints a session skeleton with a fresh ULID as its id |
| `asqr ask <file> [--wait] [--timeout <secs>] [--force]` | validates, assigns a ULID when the id is missing, makes image paths absolute, drops the file (section 3.5), prints the id |
| `asqr wait <id> [--timeout <secs>]` | waits for the result and prints it; blocks without `--timeout` |
| `asqr result <id>` | prints the result if it is there, like `wait --timeout 0` |
| `asqr status [--json]` | waiting sessions (with or without a draft answer), answered sessions (results in the outbox), and the lock holder |
| `asqr paths [--json]` | section 3.2 |
| `asqr validate <file>`, `asqr schema [--result]` | section 4 |
| `asqr prune --older-than <duration> [--results]` | section 3.9 |
| `asqr skill [--install <dir>]` | prints the skill, or writes it to `<dir>/asqr/SKILL.md` (section 9) |

Every command takes `--queue` or `--dir` (section 3.2). `--no-notify`
and `--no-bell` may also stand before `watch` or alone (`asqr
--no-bell`); next to any other command they are a usage error.

`ask`:

- `--timeout` needs `--wait`.
- With `--wait`, the id goes to stderr and stdout carries only the
  result JSON, so the output can be piped into `jq`.

`wait` and `result` print the result file as it is, and exit by its
status. `wait` checks the queue every 200 ms. When the timeout passes
first, it prints "asqr: no result for <id> yet (timeout)" on stderr and
exits with 12; `result` does the same at once while the session waits.
Both end with 13 when nothing in the queue has the id ("no session
"<id>" in this queue"), which includes a session taken out of the inbox
without a result, or when only its archive entry is left, since its
result was removed and none will come ("the result of <id> is gone").

`status --json` prints an object with `queue`, `watched_by` (the lock
holder, or `null`), `waiting` (a list of `{id, draft_has_answers}`) and
`answered` (a list of `{id, status}`, with `status` `null` for a result
that cannot be read). The text form shows the same, with "watched by:
nobody" when no asqr watches.

Exit codes, the same for every command:

| Code | Meaning |
|---|---|
| 0 | success: the result is `submitted`, or the file was dropped (`ask` without `--wait`), or the command did its job |
| 1 | failure of asqr itself (I/O, a refused `ask`, a held lock, no terminal) |
| 2 | usage error |
| 10 | the result is `cancelled` |
| 11 | the result is `error`, or a file fails validation |
| 12 | timeout: no result yet |
| 13 | no result will come: nothing in the queue has the id, or the session left without one |

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

- that it is used only when the user explicitly asks the agent to use
  asqr; the skill's description says so, and an agent never switches to
  asqr on its own (ADR 23)
- once asked, when asqr fits better than a question in the chat
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
- Editing a field in `$EDITOR` with `ctrl-g` (todo).
- Windows support (it may work, but it is not tested).
- Formats other than JSON.
- A config file: flags and environment variables only.
- Publishing: asqr is used locally first and goes to crates.io later.

## 12. Technology

Rust (edition 2024), minimum version 1.97. One crate with a library
and a thin binary, so tests and the later MCP server reach the logic
without the terminal; only the binary talks to the terminal. Licence:
MPL-2.0.

| Need | Crate |
|---|---|
| Command line | `clap` with the derive feature |
| Errors | `thiserror` for the typed errors of the format and queue code (validation errors name the field), `anyhow` with context where the running TUI meets the terminal |
| TUI | `ratatui` with `crossterm`, `ratatui-textarea` (the maintained continuation of `tui-textarea`, ADR 9) |
| Images | `ratatui-image`, `image` for decoding |
| Watching the inbox | `notify-debouncer-full` (with `notify`) |
| Platform paths | `directories` |
| Lock holder text | `gethostname` |
| Format | `serde`, `serde_json`, `schemars` (the JSON Schema is derived from the same types) |
| Atomic writes | `tempfile` (temp files with the `.tmp` suffix in the target directory, renamed into place) |
| Hashes | `sha2` for `session_sha256` |
| Locking | the standard library's file locks (no crate) |
| Timestamps | `jiff` (RFC 3339 with the offset) |
| Logging | `tracing` and `tracing-subscriber`, into a file in the platform cache directory |
| Markdown subset | `pulldown-cmark` |
| Session ids | `ulid` |
| Tests | `insta` for snapshots of rendered screens, `assert_cmd` for the CLI, `portable-pty` and `vt100` for the TUI in a pseudo-terminal |

Crates come in with `cargo add` when the first test needs them, at their
newest version.

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
- The TUI is tested through its state and a test backend with `insta`
  snapshots, and end to end in a pseudo-terminal, not by hand.
- The spec describes behaviour. It names no source files, functions or
  types; the implementation follows the spec, not the other way round.
