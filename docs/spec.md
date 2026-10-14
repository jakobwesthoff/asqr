---
title: asqr specification
format: 2
---

# asqr specification

This file describes asqr as it is now, with version 2 of the session and
result format. The specifications of earlier versions are in the git
history.

Status: reviewed with the user and in an adversarial review on
2026-09-27, with the findings F1 to F12 worked in, in two adversarial
reviews on 2026-10-14, and revised with the decisions made while
building it. The decisions behind it are recorded as ADRs in
`docs/adr/`.

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
  optional options (each with optional context), optional context, and
  optional custom entry and note.
- **Context**: material shown next to a question that helps to answer
  it: an image, a text or a diff.
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
`id` filled in and the paths of context entries made absolute. The asker's own file is
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
file: " followed by the parser's message. A shape error names the path
of its field first, as in `questions[1].context: invalid type: map,
expected a sequence`, and a `context` that is no list adds `context is a
list with one entry: wrap the entry in [ ]`. The TUI shows a notice for
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

A session of an earlier format version is invalid in the same way
(section 4). One that still waits in a queue when asqr is upgraded gets
an error result that names the version and points to `asqr skill`, so
the asker waiting for it sees exit code 11 and what to write instead.

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

## 4. Session file (version 2)

```json
{
  "asqr": 2,
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
      "context": [
        { "type": "image", "path": "/Users/someone/mascots/301-holly-crown-green-robe-feast-ghost.png" }
      ],
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

- `asqr` (required): the format version, `2`. Format version 1, which
  asqr up to 0.10 read, is the validation error "this asqr no longer
  supports format version 1; `asqr skill` describes the current format
  (version 2)". Any other value is the validation error "unsupported
  format version", followed by the value as the file writes it, which
  covers values that are no unsigned integer, such as `2.0`, `"2"`, `-1`
  or `true`. The version is checked before the rest of the file,
  so a file of another version gets only this error, even when its
  shape does not parse under version 2.
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
  and `max`) or `text` (typed answer, no options). Format version 2 has
  no other kinds.
- `options` (required for `single` and `multi`, not allowed for `text`):
  each option has an `id` (unique within the question), a `label`, an
  optional `description` of any length shown in full and wrapped, an
  optional `default: true`, and optional `context`, shown in place of
  the question's context while the cursor is on the option (section
  7.7).
- `custom` (optional, only on `single` and `multi`): allows a typed
  answer, which may span several lines like every typed field (ADR 36).
  `true`, or an object with `label` and `length`.
- `length` (optional, on `custom` and on `text` questions): `target` (a
  range) and `warn` (above it the counter warns). Both are optional, and
  `warn` must not be below the end of `target`. There is no hard limit:
  the counter guides, and input is never refused (user, 2026-09-27).
- `context` (optional): material that helps to answer the question,
  shown next to it (section 7.7). Most questions have none. A list,
  limited to one entry: the format allows more, so showing several
  needs no new format version. An empty list and `null` are the same as
  none, on a question and on an option.
- `note` (optional, default `true`): whether the person may add a note.
  One note per question.
- Every question is optional (ADR 23): there is no way to force an
  answer, and a skipped question comes back as `skipped: true`. Files
  that still set the former `required` field parse as before; the field
  is ignored and reported as unknown.

Context entry fields. Every entry has a `type` and an optional `title`,
a name for the entry such as a file name:

- `image`: `path` (required), an image file. `enlarge` (optional,
  default `true`): `false` keeps the image at its natural size where it
  fits, in the pane and full screen, instead of scaling it up; a larger
  image still shrinks to fit (ADR 32).
- `code`: text shown exactly as written and highlighted, like a fenced
  code block: source code, a log, terminal output, a configuration file
  or the source of a Markdown file. From the file at `path` or inline in
  `content`, exactly one of the two. `language` (optional), such as
  `rust`, names the language; without it the extension of `path`, then
  of `title`, decides, and failing that it is plain text.
- `markdown`: Markdown, rendered, from the file at `path` or inline in
  `content`, exactly one of the two.
- `diff`: a change between two versions of a text, which asqr diffs
  itself. `old` and `new` are each `{ "path": ... }` or
  `{ "content": ... }`, exactly one of the two. A missing `old` is a new
  file, a missing `new` a deleted one. `name` (optional) is the file
  name the diff is about, shown in the header. A diff shows without
  syntax highlighting, so it takes no language. A unified diff written
  by the asker is not understood: asqr diffs it like any other text and
  shows a diff of the diff. Every hunk header of a unified diff needs
  exact line counts, which goes wrong easily, so the asker gives the two
  versions instead. asqr shows the change as a unified diff (section
  7.7).

Paths in context entries, for every type:

- A path is absolute. `asqr ask` resolves relative paths against the
  file it was given and drops absolute ones. `asqr validate` warns about
  relative paths, which a file dropped by hand must not use.
- `asqr validate` and `asqr ask` both warn about files that do not
  exist ("file not found"), about paths that exist but are no regular
  file, such as a directory ("not a file"), and about image files the
  terminal UI cannot decode (section 7.7). That check reads only the
  image file's header. For an unsupported format the warning lists the
  formats that work. Both check a relative path against the absolute
  directory of the session file, so the warning names the file by an
  absolute path even when the session file was given by its bare name.
- `asqr validate` and `asqr ask` warn when a side of a `diff`, inline or
  in a file of at most 1 MiB, looks like a unified diff: a line that is
  a full hunk header, `@@ -N[,N] +N[,N] @@` with optional text after a
  blank, so a hunk copied without its file headers counts, or a line
  starting with `@@ -` after a line starting with `--- `, `+++ ` or
  `diff --git`. A hunk header inside a line, as in prose about diffs,
  does not count. The warning says to give the text before and after the
  change as `old` and `new`, since asqr computes the diff.
- `asqr validate` and `asqr ask` warn about a `code`, `markdown` or
  `diff` entry above 1 MiB, from its file or inline; for a diff, both sides count
  together. asqr still shows it. Image entries get no such warning:
  screenshots above 1 MiB are common.
- asqr reads a file when it first shows it, not when the session
  arrives.

Validation errors, besides missing or mistyped fields:

- a format version other than 2, or an `id` that is no valid id
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
- more than one context entry on a question or an option
- a `code` or `markdown` entry or a side of a `diff` with neither or
  both of `path` and `content`
- a `diff` with neither `old` nor `new`, or with `path` or `content` on
  the entry itself: one error per entry, saying that a diff takes `old`
  and/or `new`, each `{"path": ...}` or `{"content": ...}`, the text
  before and after the change, and that asqr computes the diff itself
  and does not read a unified diff. Such a `path` or `content` gets no
  unknown-field warning on top.
- an entry whose `type` is not `image`, `code`, `markdown` or `diff`
  (the file is not shaped like a session). An entry of type `text` gets
  an error of its own that names `code` and `markdown`. Like the
  version, it is checked before the rest of the file, and every such
  entry is listed.
- in the inbox: an `id` that differs from the file stem

Unknown fields are ignored when parsing, so later format versions can
add optional fields. `asqr validate` and `asqr ask` warn about them,
comparing the keys against the schema, so a typo like `requred` is
reported. Context entries are compared against the fields of their
`type`. The `image` field of format version 1 on a question or an option
gets a warning of its own, saying that an image is a context entry of
type `image`.

`asqr validate <file>` prints `<file>: valid` and exits with 0, or lists
every error and exits with 11. Warnings go to stderr and never fail it.
A file that is not JSON, or not shaped like a session, exits with 11 as
well; a file that cannot be read exits with 1.

Text fields (`intro`, question `text`, option `description`) are
Markdown, rendered in full (section 7.6).

The repo ships the JSON Schema (`schema/session.v2.json`,
`schema/result.v2.json`). `asqr schema` prints the session schema and
`asqr schema --result` the result schema. Besides the shape, the schemas
state the rules a schema can state directly: `asqr` is the constant `2`
in both, a session holds at least one question, `context` holds at most
one entry (an empty list and `null` are valid), and a `code` or
`markdown` entry or a side of a `diff` takes exactly one of `path` and
`content`, written as a `oneOf` of a branch that requires `path` as a
string and one that requires `content` as a string; an empty `content`
counts as given, and a `null` counts as missing, as it does for asqr. A
`diff` takes `old`, `new` or both, written as an `anyOf` of a branch
that requires `old` as an object and one that requires `new` as an
object. The descriptions of `intro`, question `text` and option
`description` say that they are Markdown, that a single newline breaks
the line and that raw HTML shows as written. The other rules are
validation's alone.

## 5. Results and drafts

### 5.1 Result file

```json
{
  "asqr": 2,
  "id": "alt-rework-3-batch-01",
  "status": "submitted",
  "submitted_at": "2026-09-27T17:05:12+02:00",
  "session_sha256": "9f2c...",
  "note": "fine as long as the old line stays available",
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
  - `submitted`: the person submitted, possibly with skipped questions,
    with an optional `note` to the whole session (trimmed; absent when
    empty; ADR 29)
  - `cancelled`: the person rejected the session, with an optional
    `reason` (trimmed; absent when empty) and never a `note`
  - `error`: the file was invalid, and `error` holds the message
- `submitted_at` holds the time of finishing, as RFC 3339 with the
  offset. It is present for every status.
- For `submitted`, every question appears exactly once, in session
  order. For `cancelled` and `error`, `answers` is empty.
- asqr writes results in format version 2: the fields of version 1
  plus `note`. `wait` and `result` print a result of version 1 that an
  earlier asqr left in the outbox as it is, so an asker waiting across
  an upgrade still gets its answers.

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
  whether they are empty trims them. Every typed field takes line
  breaks, so `custom`, `note` and `reason` may contain `\n` (ADR 36).

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
shown), `note`, the session note typed so far, and `reason`, the reject
reason typed so far, each when there is one and as typed.
It is written on every change. On restore, answers are matched by
question id and option id, and anything that no longer fits (a question
or option that is gone) is dropped. A draft that cannot be read is
logged and ignored, and the session starts fresh, with only its defaults
selected.

A draft "has an answer" (section 3.5) when any question has a selection,
typed text that is not blank or a note that is not blank, when the
session note is not blank, or when the draft cannot be read. The reject
reason does not count. A selection counts even where it does not answer
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

A stack of rows over the full width (user, 2026-09-27), with the
question kept to a reading width and a context pane beside or below it
when the question has context (user, 2026-10-14; ADRs 30 and 31):

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
   (•) ✎ Own line: Snappy in holly -- my own line, which
                   wraps under its start.

 Notes: —

 [context pane below, or beside the question on a landscape terminal]
 [notices and messages]
 ↑/↓ move  enter pick  ←/→ question  n note  ? help  q quit
```

- The title bar names the queue and counts the waiting sessions. A queue
  name too long for the line is cut from the left with `…`, at a `/`
  where possible, so the count stays visible.
- The session header shows the title (or the id), `from` and `follows`,
  then the intro, both wrapped at the reading width. The title takes at
  most two lines and the intro three, one each on terminals below 24
  rows. A cut title ends in `…`, a cut intro in `… i: more`, and `i`
  shows the whole intro (section 7.2). While a field, a note or the
  context pane has the keys, where `i` would be typed or taken by the
  pane, the intro ends in `… esc, i: more`. The tab bar keeps the full
  width.
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
  inside a field. Hints that do not fit its width are left out whole,
  never cut: first `n`, then `tab`, then `←`/`→`, then the others from
  the right. `?` and `q` stay longest, as the help lists every key
  (ADR 31).
- **Reading width:** the question and the review take at most 100
  columns and start at the left, with or without a context pane. The
  tab's area, W×H, is the area under the tab bar inside a margin of one
  column on each side.
- **Context pane:** a question that has context, or has an option with
  context, shares the tab's area with a context pane (section 7.7):
  - Beside: the question takes half of W after the blank separator
    column, at least 60 and at most 100 columns, and the context pane
    takes the rest. Beside fits from W = 101, a terminal of 103 columns.
  - Below: the question keeps its width and takes the rows the tallest
    question with context in the session needs, at most half of H, and
    the context pane takes the rest over the whole of W. So the pane
    keeps its place while the person moves between questions (user,
    2026-10-14). A question's rows are counted at that width without a
    cursor: every field unfocused with the text typed so far, a row for
    the counter of a field with a length, and no row for the cursor
    after a full line. While a field or a question's note has the keys,
    the current question counts as drawn, its text cursor included, so
    a text that fills its line exactly gives the cursor's blank its own
    row at once. Typing moves the pane only when the tallest question
    gains a row, and it moves back up only once no field or note has the
    keys. A resize, another session or a replaced session
    file count afresh. Below fits when the context pane gets at least 8
    rows with the question at half of H, that is from H = 15.
  - `--layout auto`, the default: when both fit, the context goes
    beside on a landscape area, where W ÷ 2H is at least 1 (cells are
    about twice as tall as wide), and below on a portrait one. After a
    resize, beside stays until W ÷ 2H drops under 0.9 and below stays
    until it reaches 1.1, so a redraw at the same size keeps what is
    shown. When only one fits, that one is used. A 120×40 or 230×60
    terminal gets beside, 110×70 and 90×50 get below.
  - `--layout beside` or `--layout below` (or `ASQR_LAYOUT`, section 8)
    is used when it fits, and the other one when only that fits.
  - When neither fits, the context pane shrinks to one line at the
    bottom of the tab that names the entry shown and adds
    `z: full screen`, or `esc, z: full screen` while a field or a note
    has the keys.
  - A question without context takes the width it can get up to the
    reading width, so its text reflows when moving to a question with
    context beside.
- Everything adapts to the terminal: the question scrolls so the row
  under the cursor stays visible, dialogs never exceed the screen, and
  below 60×15 asqr shows "asqr needs at least 60×15; this terminal is
  W×H." instead of the layout. Back at a larger size, the context
  pane's focus and the full-screen view are as they were.
- When the question or the review does not show all its rows, a dim
  `↑ more` on its first row says that rows are cut off above, and a dim
  `↓ more` on its last row that rows are cut off below. The marker
  takes no row of its own: it goes over the right end of that row where
  it is blank, else over the start where that is blank, else over the
  right end anyway, so the pointer and the option's number stay
  visible. A marker never covers the text cursor: where the end chosen
  holds it, the marker goes over the other end, and where both do, that
  row shows no marker.
- A field or a note taller than the question's area keeps its text
  cursor in view instead of showing its start: the cursor's line stays
  on the area's last row while the field's start lies further up, so
  `↑` scrolls back to the start row by row.
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
| `i` | the whole intro |
| `o`, `z` | open the image shown in the system viewer; show the context shown full screen |
| `tab`, `shift-tab` | move into the context pane; `tab`, `shift-tab`, `esc` or `q` move back |
| `pgup`/`pgdn`, `ctrl-u`/`ctrl-d` | scroll the context pane by a page, also while typing in a field |
| `q`, `ctrl-c` | quit, keeping the draft; in the context pane `q` goes back |
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
  started, a notice says why. On a code, Markdown or diff entry it
  opens nothing and says "o opens images only; z shows this entry full
  screen", in the full-screen view only "o opens images only", and
  where no entry shows it says "nothing to open here".
- `tab` moves the focus into the context pane (section 7.7) wherever one
  shows, also from a field, and does nothing otherwise, such as next to
  the one-line hint. The focused pane takes every key: `↑`/`↓` and
  `k`/`j` scroll by line, `←`/`→` and `h`/`l` by 4 columns sideways
  where the entry's lines are cut at the pane's edge, `pgup`/`pgdn` and
  `ctrl-u`/`ctrl-d` by page, `g`/`G` and `home`/`end` to the ends. `z`,
  `o` and `?` keep their meaning and `ctrl-c` quits. `tab`, `esc` and
  `q` give the focus back to where it was, a field being typed in
  included, so a word typed after `tab` never quits asqr. Every other
  key does nothing. While the pane has the focus, its header is
  highlighted, a field's text cursor hides, and the key bar and the
  help show the pane's keys, with `? help` and `tab/esc/q back`. A pane
  that goes away, as after a resize down to the hint, gives the focus
  back.
- `z` shows the context pane of the entry shown over the question's
  area, header included. It acts on every kind of entry (ADR 31). The
  full-screen view reads like the focused pane: it takes every key, so
  `enter`, the digits, `n` and the others cannot act on the question
  hidden behind it. It scrolls with the pane's keys, `o` and `?` keep
  their meaning, and `z`, `esc`, `tab` and `q` leave it for where the
  focus was before: the question, a field being typed in, or the
  focused pane. The key bar shows the pane's keys with `z/esc/q back`.
  The help is titled "Keys of the full-screen view" and lists the
  pane's scroll keys, `o`, `ctrl-c`, `?` and `z/tab/esc/q  leave the
  full screen`.
- `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` scroll the context pane by a page
  from anywhere, also from inside a field, while a pane shows. These
  are the intro overlay's keys and conventions.
- Help closes on any key.
- `i` shows the session's whole intro in a frame over the question,
  rendered as in the header but not dimmed (section 7.6), with the key
  bar left free. An
  intro longer than the frame scrolls: `↑`/`↓` and `k`/`j` by line,
  `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` by page, `g`/`G` and `home`/`end`
  to the ends. The frame's bottom edge then shows which lines are
  visible, such as `4–15 of 16`. `esc`, `q` and `i` close it and
  `ctrl-c` quits; every other key does nothing, so a key meant to
  scroll never closes it. On a session without an intro, `i` does
  nothing.
- The session list (`L`) lists every waiting session in queue order
  (section 3.4): `↑`/`↓` or `k`/`j` move, `enter` switches to the
  session, `esc` or `L` close the list, and `q` quits asqr.

### 7.3 Live fields and counters

Typed text is entered in place, in the line where it is shown, without
a frame (user, 2026-09-27):

- **The own-answer row** is a field as soon as the cursor lands on it:
  every key that produces text types into it. `←`/`→` and `↑`/`↓` move
  the text cursor (see below), and `esc` leaves the field while the
  cursor stays. `←` with the text cursor at the start of the text goes
  to the previous question, `→` at its end to the next one (ADR 23). After `esc`, `↑`/`↓` move on as usual; where the move
  cannot leave the row (the single row of a `text` question, `↓` on the
  last row), they focus the field again. `enter` picks the own answer
  and moves on, like an option. `ctrl-j` adds a line, as in every
  field (ADR 36). A text longer than the line wraps onto further lines
  under its start, after the row's head, focused or not, and is never
  cut with `…`. Every line after the first starts there too. When the
  head leaves the text less than 20 columns, the further lines start 4
  columns in (ADR 27). On a `multi` question the own answer
  counts once it has text.
- **A `text` question's answer** is a field that is active when the
  question is shown. It grows with its lines; `ctrl-j` adds a line, and
  `enter` moves on. `←`/`→` at the start or end of its text switch
  questions as on the own-answer row.
- **The note** is edited in its line under the options after `n`. It is
  multi-line (`ctrl-j` adds a line); `enter` or `esc` leave it.
- **The session note** and **the reject reason** on the review tab are
  fields like the own answer: `ctrl-j` adds a line, `enter` submits or
  rejects, and they wrap the same way.
- **Moving in a field:** in every field and the note, `↑`/`↓` move the
  text cursor to the row above or below as the field shows it, that is
  the wrapped rows, not the text's lines, at the same display column or
  the end of a shorter row. While `↑` and `↓` follow each other they
  keep the column they started from. A field is left with `↑` only from
  its first row and with `↓` only from its last row, as with `←` and `→`
  at the start and end of its text; the note stays where it is (ADR 36).
  `↓` from the last row of a line whose end shows on a row of its own,
  as after a text that fills its row, goes to that end first.
- Every edit is saved to the draft continuously, the session note and
  the reject reason included; there is no discard.
- Pasted text goes in at the text cursor of the focused field or note
  as a whole. asqr asks the terminal for bracketed paste, so a line
  break in a paste is never an `enter` that picks, submits or rejects.
  It turns bracketed paste and focus reports off again when it ends, a
  crash included. A terminal that refuses either runs asqr without it.
  Every field keeps the pasted line breaks; `\r` and `\r\n` become
  `\n` (ADR 36). A tab becomes blanks up to the next multiple of 4
  columns, counted from the start of its line, and other control
  characters are dropped, as a field shows neither. Text restored from
  a draft is cleaned the same way.
- On a row with a field left with `esc` (the own answer, a `text`
  answer, the session note, the reject reason), a paste gives the field
  the keys again, with the text cursor at the end of its text, and goes
  in there. On any other row it puts nothing in and says "a paste goes
  into a field; press n for a note" on a question that takes notes, and
  "a paste goes into a field" otherwise. In the focused context pane
  the message is "a paste goes into a field; esc leaves the pane", in
  the full-screen view "a paste goes into a field; z/esc leave the full
  screen". Over the help, a paste closes it and does nothing else; over
  the intro and the session list it does nothing.
- `tab` and `shift-tab` never type into a field: they move the focus
  into the context pane where one shows, and do nothing otherwise. Where
  a pane shows, `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` scroll it from
  inside a field; without one, `pgup`/`pgdn` do nothing there, and
  `ctrl-u` and `ctrl-d` undo and delete as listed below (ADR 31).
- **Keys in a field,** besides `enter`, `esc` and `ctrl-j` (asqr keeps
  these keys of its editor, ratatui-textarea):
  - `←`/`→` and `ctrl-b`/`ctrl-f` move by a character, `ctrl-←`/`ctrl-→`
    and `alt-b`/`alt-f` by a word, `home`/`end` and `ctrl-a`/`ctrl-e` to
    the start and end of the line.
  - `↑`/`↓`, with any modifier, and `ctrl-p`/`ctrl-n` move by a row as
    described above, leaving the field from its first or last row.
  - `alt-[`/`alt-]` and `alt-p`/`alt-n` move to the previous and next
    paragraph, `alt-<`/`alt->` and `ctrl-alt-p`/`ctrl-alt-n` to the first
    and last line.
  - `backspace` and `ctrl-h` delete the character before the text
    cursor, `delete` and `ctrl-d` the one under it; `ctrl-w`, `alt-h`
    and `alt-backspace` delete the word before it, `alt-d` and
    `alt-delete` the word after it, and `ctrl-k` the rest of the line.
    `ctrl-y` puts back what the last of these word or line deletes took.
  - `ctrl-u` undoes and `ctrl-r` redoes an edit.
  - There is no selection: `shift` with a move only moves, and `ctrl-x`
    cuts nothing.
  - `ctrl-v` and `alt-v` do nothing, as `pgup`/`pgdn` without a pane.

Where the question sets `length`, a counter on a line of its own after
the focused field's text, at the right edge, shows the length as text,
and colour is only added on top:

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
  here (ADR 23), and the keys stay commands on this row.
- **Note to the asker**: a live field for an optional note to the whole
  session; `enter` submits with it (ADR 29). There is no `n` on the
  review tab.
- **Reject**: a live field for the optional reason; `enter` rejects the
  session with it. A rejected session never carries the note.

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
- A replaced session file keeps the answers that still fit and where
  the person was: the question shown, found again by its id, or the
  review; the row, or the last one when fewer are left, with its field
  taking the keys again; the context pane's focus and the full-screen
  view while the same entry shows; and the scroll position of every
  entry that reads as before. The message "<title> changed on disk and
  was reloaded" says so, with the id for a session without a title. An
  open intro overlay closes when the new file has no intro. A session
  list open while a session leaves keeps its cursor on a waiting
  session, and closes when none is left.
- A session colliding with an unread result (section 3.7), an invalid
  file (section 3.8) and a failure to write a draft or a result trigger
  a notice. A failure never ends asqr; answers stay in the TUI.

### 7.6 Markdown

The intro, question texts and option descriptions render in patine's
style, as a Markdown entry does (section 7.7, ADR 35): headings bold
(level one also italic and underlined, level two underlined), emphasis,
inline code in yellow, lists with bullets, quotes behind a dim bar,
links underlined with their URL dimmed, tables, rules, and code blocks
highlighted when the fence names a language.

- Each text renders at the width it gets: a question text at the
  question's width, which stops at the reading width; the intro at the
  header's width; an option description in the columns from where it
  starts to the end of the line, beside its label or below it (section
  7.1).
- A line patine leaves wider than that, such as a line of a code
  block, a long URL or a table that cannot shrink, wraps as in a
  Markdown entry: each further row repeats the blanks and quote bars
  the line starts with, then a dim `↪`, then the rest. Nothing is cut,
  and nothing scrolls sideways.
- A single newline breaks the line, and a blank line starts a new
  paragraph. Inside a list item the next line starts under the item's
  text, inside a quote behind its bar. Code blocks keep their lines, and
  inline code shows a newline as a blank.
- Raw HTML shows as written, in the style of the text around it, so
  `Vec<String>` stays as it is. An HTML block keeps its lines as they
  are, comments included. An autolink such as `<https://example.com>`
  is a link.
- Markdown entries (section 7.7) are documents and keep CommonMark's
  rules: a single newline joins the lines, and raw HTML is left out.
- The intro under the title is dim on top of these styles. The `i`
  frame shows it without the dim.

### 7.7 Context

Context appears only when a question or one of its options has some.

- The option under the cursor shows its own context, so the person can
  browse the options before picking; this holds for `single` and
  `multi`. Every other row, and an option without context, shows the
  question's context, or nothing when the question has none.
- The room for the context stays while the cursor moves, also when the
  current row shows none, so the layout does not jump.
- The context pane goes beside or below the question, or shrinks to a
  one-line hint, by the terminal's size and `--layout` (section 7.1).
- The review shows no context.
- The pane's first line is a header that names the entry: its `title`,
  or the file name of its `path` (a diff's `name`), or its kind, `code`,
  `markdown` or `diff`, when it has neither. When the entry is taller than the
  pane, the header's right end says which lines show, such as
  `4–15 of 120`. When the entry is taller or wider than the pane and the
  pane does not have the focus, the right end also names `tab`, as in
  `1–10 of 31 · tab`, or `tab` alone for an entry that is only wider:
  on a narrow terminal the key bar leaves out its `tab` hint first.
  Where the name would have to be cut for it, the `tab` goes first. A
  row without context leaves the pane empty, header included (ADR 31).
- Below the header, a viewer shows the entry. An image is scaled to
  fill the pane as far as its proportions allow, a small one included,
  and so is the `z` view. An image entry with `enlarge: false` keeps its
  natural size in both, and only shrinks when it does not fit.
- The pane scrolls with its own keys (section 7.2). Every entry keeps
  its scroll position while its session waits: moving to an option with
  another entry shows that entry where the person left it, or at its top
  the first time, and coming back restores the first one's. Rows that
  show the question's entry share its position. A replaced session file
  keeps the positions of the entries it leaves as they were and starts
  the others at their top (section 7.5).

Diffs (ADR 33):

- asqr computes the diff from the old and the new text, line by line,
  and shows it unified: hunks with 3 unchanged lines of context around
  each change, and a dim line of `┈` between two hunks. A dim gutter
  holds the old and the new line number, followed by the sign: `-` for
  a removed line, `+` for an added one, a blank for an unchanged one.
- Removed lines are red and added lines green, in the terminal's
  palette, so its theme decides the shades. Inside a changed line the
  changed words show reversed on top of the colour. Words end at blanks
  and punctuation, so `get(url)` changed to `get(uri)` marks only `url`
  and `uri`. A line that changed too much to pair with its old version
  shows without marked words.
- A diff without `old` starts with a dim "new file", one without `new`
  with "deleted file"; when that file is empty, "empty file" follows.
  Two sides with the same text show "no changes".
- A last line without a newline gets a dim "no newline at end of file"
  below it, but only when the other side ends with one: the removed and
  the added line would look the same otherwise.
- Lines compare without their line ends: `\r\n` and a lone `\r` count
  as `\n`. When the two sides use different line ends, a dim "line
  endings differ" says so once, above the hunks or below "no changes". A
  side of a single line without a line end differs from neither.
- Lines do not wrap. A line wider than the pane ends in a dim `›`, and
  the pane scrolls sideways (section 7.2) while the gutter and the sign
  stay in place. Scrolled sideways, a line whose hidden start holds
  more than blanks begins with a dim `‹`. Each marker takes a column of
  the line, and a wide character that would cross into it shows as
  blanks.
- Tabs expand to the next stop of 4 columns, counted from the start of
  the line. Any other control character shows as its picture from
  Unicode's Control Pictures block, such as `␛` for escape; the C1
  controls, which have none, show as `�`. This holds for every viewer
  that shows lines of source.
- Diffs have no syntax highlighting, and the pane shows the hunks only,
  never the whole file.
- Both sides are read when the entry is first shown. A relative path, a
  missing or unreadable file, a path that is not a regular file, such as
  a directory or a FIFO, or a binary file (one with a NUL byte) shows a
  placeholder saying "diff not shown" with the path and the reason, for
  the first side that fails. Bytes that are not UTF-8 show as `�`.
- Of a file longer than 8 MiB, only the first 8 MiB are read. A dim
  "first 8 MiB shown" heads the content and a dim "… the rest is cut
  after 8 MiB" ends it, so the end of what shows does not pass for the
  end of the file. In a diff, the rest of the other side then shows as
  removed or added.
- Computing a diff may take up to a second. Past that, asqr shows a
  diff that is still correct but may mark more lines as changed than
  needed.

Code and Markdown (ADR 34):

- A code entry shows its text exactly as written, highlighted in its
  language (section 4) with the 8 colours of the terminal's palette, so
  its theme decides the shades. Without a language, or with one asqr
  does not know, the text is plain. A file name without an extension,
  such as `Makefile`, counts as a whole, and a `.md` file shows its
  source.
- A dim gutter holds each line's number. Lines do not wrap: a line wider
  than the pane ends in a dim `›`, and the pane scrolls sideways while
  the gutter stays in place. The `‹` of a scrolled line, tabs and
  control characters show as in a diff.
- A Markdown entry shows rendered in patine's style, wrapped at the
  pane's width, without a gutter and without sideways scrolling:
  headings bold (level one also italic and underlined, level two
  underlined), emphasis, inline code in yellow, lists with bullets,
  quotes behind a dim bar, tables, and code blocks highlighted when the
  fence names a language.
- A line patine leaves wider than the pane, such as a line of a code
  block, a long URL or a table that cannot shrink, wraps: each further
  row repeats the blanks and quote bars the line starts with, then
  blanks in place of a list item's `•` or number, then a dim `↪`, then
  the rest. A number counts as an item's only at the start of the line,
  after quote bars, so a line of code such as `1. x` in a code block
  stays code. A wide character that would cross the end of a row
  starts the next one. This holds wherever asqr renders Markdown with
  patine. Tabs in code blocks expand to stops of 4 columns.
- Both are read when the entry is first shown. A relative path, a
  missing or unreadable file, a path that is not a regular file or a
  binary file shows a placeholder saying "code not shown" or "markdown
  not shown" with the path and the reason. Bytes that are not UTF-8 show
  as `�`, and an entry without any text shows a dim "empty". A file
  longer than 8 MiB shows its first 8 MiB between a dim "first 8 MiB
  shown" and a dim "… the rest is cut after 8 MiB", as in a diff.

Images:

- Terminals that support the Kitty graphics protocol (Ghostty, kitty,
  WezTerm), the iTerm2 protocol or Sixel show images inline. Terminals
  without any of these get a coarse block rendering.
- asqr decodes PNG, JPEG, GIF (first frame only), WebP, BMP, TIFF, ICO,
  TGA, PNM, QOI, DDS, OpenEXR, HDR and farbfeld. It detects the format
  from the file's content, and from the extension when the content does
  not match a known signature. SVG and AVIF are not decoded. The set follows
  the features of the `image` crate, and a test fails when it changes.
- The protocol is detected once at start, by querying the terminal.
- An image entry is read when it is first shown and kept while its
  session waits, until asqr's terminal gains the focus again. asqr asks
  the terminal for focus reports; on regaining the focus it reads and
  sends every image again as it is next shown. Code, Markdown and diff
  entries keep what asqr derived from them (ADR 31).
- A relative path, a missing file, a path that is not a regular file or
  a file that cannot be read or decoded shows a placeholder saying
  "image not shown" with the path and the reason. A path is never resolved against the working directory,
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
its reader would go on taking the keys typed afterwards. Without
`TMUX`, when `TERM` starts with `tmux` or `screen` or `TERM_PROGRAM` is
`tmux`, as in GNU screen or after ssh from a tmux pane, asqr cannot ask
that tmux. It sends a device status request through tmux's passthrough
instead and waits up to 300 ms for the answer: with one, it queries the
terminal; without one, images show as blocks. Keys typed during that
wait are lost, and an answer slower than 300 ms, as over a slow ssh
link, also means blocks. Outside Unix asqr does not wait and shows
blocks there. Notifications
inside tmux go out in tmux's passthrough wrapper.

## 8. The command line

| Command | What it does |
|---|---|
| `asqr` / `asqr watch [--no-notify] [--no-bell] [--layout auto\|beside\|below]` | the TUI (section 7) |
| `asqr new` | prints a session skeleton without an id, which `asqr ask` assigns |
| `asqr ask <file> [--wait] [--timeout <secs>] [--force]` | validates, assigns a ULID when the id is missing, makes the paths of context entries absolute, drops the file (section 3.5), prints the id |
| `asqr wait <id> [--timeout <secs>]` | waits for the result and prints it; blocks without `--timeout` |
| `asqr result <id>` | prints the result if it is there, like `wait --timeout 0` |
| `asqr status [--json] [--limit <count> \| --all] [--since <age>]` | waiting sessions (with or without a draft answer), answered sessions (results in the outbox), and the lock holder |
| `asqr paths [--json]` | section 3.2 |
| `asqr validate <file>`, `asqr schema [--result]` | section 4 |
| `asqr prune --older-than <duration> [--results]` | section 3.9 |
| `asqr skill [--install <dir>]` | prints the skill, or writes it to `<dir>/asqr/SKILL.md` (section 9) |

Every command takes `--queue` or `--dir` (section 3.2). `--no-notify`,
`--no-bell` and `--layout` may also stand before `watch` or alone (`asqr
--no-bell`); next to any other command they are a usage error.

`--layout` places the context pane (section 7.1). Without it,
`ASQR_LAYOUT` sets the layout, and without either it is `auto`. A
`--layout` after `watch` wins over one before it. An empty
`ASQR_LAYOUT` counts as unset. Any other value than `auto`, `beside` or
`below` is a usage error when asqr watches the queue; the other
commands ignore the variable.

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
holder, or `null`), `waiting` (a list of `{id, draft_has_answers}`),
`answered` (a list of `{id, status}`, with `status` `null` for a result
that cannot be read) and `answered_total`. The text form shows the same,
with "watched by: nobody" when no asqr watches.

Results stay in the outbox until they are pruned, so `status` lists
only part of them:

- `answered` is ordered by the time each session was finished, the
  newest first. That time is `submitted_at`; a result without a readable
  `submitted_at` goes by its file's modification time. Results finished
  at the same time are ordered by id.
- `--since <age>` keeps the results finished within that age. The age
  has the form of `prune --older-than` (section 3.9).
- `--limit <count>` lists at most that many, 10 when it is not given.
  `--all` lists every result and cannot be combined with `--limit`.
- `answered_total` counts the results `--since` keeps, before the limit
  cuts the list. The text form writes "answered (10 of 57):" when the
  list is cut and "answered (3):" when it is not.
- `waiting` always lists every waiting session.

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

Agents are the main askers. asqr ships a skill, and an MCP server
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

## 11. Not in asqr yet

- Networking, several people on one queue, sockets, queues on network or
  synced filesystems.
- The MCP server (section 9).
- `asqr answer <file>`, answering one file in the current terminal
  without a queue.
- Partial submits.
- Question kinds beyond `single`, `multi` and `text`.
- A tmux passthrough hint in the TUI (todo).
- Diffs side by side in a wide pane (todo).
- Editing a field in `$EDITOR` with `ctrl-g` (todo).
- Windows support (it may work, but it is not tested).
- Formats other than JSON.
- A config file: flags and environment variables only.

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
| Markdown and highlighting | `patine` without default features, for the text fields and for code and Markdown entries (ADR 34, ADR 35) |
| Diffs | `similar` with its `inline` and `unicode` features |
| Columns of text on screen | `unicode-segmentation` for graphemes, each as wide as ratatui's cell width |
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
- A pushed tag `vX.Y.Z` or `vX.Y.Z-rcN` creates the GitHub release and
  attaches binaries for macOS and Linux (ADR 26).
- One commit per finished feature, with its tests.
- The TUI is tested through its state and a test backend with `insta`
  snapshots, and end to end in a pseudo-terminal, not by hand.
- The spec describes behaviour. It names no source files, functions or
  types; the implementation follows the spec, not the other way round.
