---
title: asqr specification
format: 2
---

# asqr specification

This file describes asqr as it is now, with version 2 of the session and
result format.

The decisions behind this specification are recorded as ADRs in
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
- **Question**: one thing to decide. It has a kind (section 3.2), text,
  optional options (each with optional context), optional context, and
  optional custom entry and note.
- **Context**: material shown next to a question that helps to answer
  it: an image, code, Markdown or a diff (section 3.3).
- **Result**: the answer file for one session.
- **Draft**: the answers to a session while it is being answered.
- **Queue**: a directory with an inbox, an outbox, drafts and an archive,
  watched by at most one asqr instance. The default queue serves
  everything, and named queues keep projects apart.

## 3. Session file (version 2)

### 3.1 Example and session fields

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

- `asqr` (required): the format version, `2`. Format version 1 is a
  validation error of its own (section 3.6). Any other value is the
  validation error "unsupported format version", followed by the value
  as the file writes it, which covers values that are no unsigned
  integer, such as `2.0`, `"2"`, `-1` or `true`. The version is checked
  before the rest of the file, so a file of another version gets only
  this error, even when its shape does not parse under version 2.
- `id` (optional): a valid id (section 5.3). In the inbox it must equal
  the file stem, ignoring case. Without it, `asqr ask` assigns a fresh
  ULID and names the dropped file after it. A file dropped by hand
  without `id` has its stem as the id. The name of the file given to
  `asqr ask` or `asqr validate` does not matter.
- `title` (optional): shown in the title bar, which shows the id when
  there is none. `intro`, `from` (optional): shown in the session header.
- `follows` (optional): the id of an earlier session this one continues.
  asqr itself has no loop: a follow-up round is a new session that names
  the previous one here. The header shows that session's title, or the
  id (section 7.2). asqr does not count or match carried-over questions.
- `questions` (required, at least one).

Text fields (`intro`, question `text`, option `description`) are
Markdown, rendered in full (section 7.10).

### 3.2 Question fields

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
  8.1).
- `custom` (optional, only on `single` and `multi`): allows a typed
  answer, which may span several lines like every typed field (ADR 36).
  `true`, or an object with `label` and `length`.
- `length` (optional, on `custom` and on `text` questions): `target` (a
  range) and `warn` (above it the counter warns). Both are optional, and
  `warn` must not be below the end of `target`. There is no hard limit:
  the counter guides, and input is never refused (section 7.7).
- `context` (optional): material that helps to answer the question,
  shown next to it (section 8). Most questions have none. A list of at
  most one entry (section 3.3).
- `note` (optional, default `true`): whether the person may add a note.
  One note per question.

Every question is optional (section 4.2, ADR 28): there is no way to
force an answer, and a skipped question comes back as `skipped: true`.

### 3.3 Context entries

`context`, on a question or an option, is a list limited to one entry
(ADR 28). An empty list and `null` are the same as none, on a question
and on an option.

Every entry has a `type` and an optional `title`, a name for the entry
such as a file name:

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
  shows a diff of the diff. asqr shows the change as a unified diff
  (section 8.5).

New entry types are added within format version 2, so an asqr can meet
an entry whose type a newer asqr knows (section 3.4).

For "exactly one of `path` and `content`", an empty `content` counts as
given, and a `null` counts as missing.

Paths, for every type:

- A path is absolute. `asqr ask` resolves relative paths against the
  file it was given and drops absolute ones. A file dropped by hand must
  not use relative paths (section 3.4 lists the warning).
- asqr reads a file when it first shows it, not when the session
  arrives.

### 3.4 Validation

A file that fails validation is invalid. Besides missing or mistyped
fields, these are validation errors:

- a format version other than 2 (section 3.6), or an `id` that is no
  valid id
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
- an entry whose `type` is a string other than `image`, `code`,
  `markdown` and `diff`: the error names the type as not supported by
  this asqr and its version, lists the four types, and says that a
  newer asqr may support it. An entry of type `text` gets an error of
  its own that names `code` and `markdown`. Like the version, both are
  checked before the rest of the file, and every such entry is listed.
  A missing `type`, or one that is no string, leaves the file not shaped
  like a session.
- in the inbox: an `id` that differs from the file stem

A file that is not JSON, or not shaped like a session, gets the message
"not a session file: " followed by the parser's message. A shape error
names the path of its field first, as in `questions[1].context: invalid
type: map, expected a sequence`, and a `context` that is no list adds
`context is a list with one entry: wrap the entry in [ ]`. For a field
inside a context entry, the path ends at the entry, as in
`questions[0].context[0]: invalid type: null, expected a string` for a
`path` of `null`. Several problems are joined with `; `.

Unknown fields are ignored when parsing, so later format versions can
add optional fields.

`asqr validate` and `asqr ask` warn, on stderr and without failing:

- about unknown fields, comparing the keys against the schema, so a typo
  like `requred` is reported. Context entries are compared against the
  fields of their `type`.
- about relative paths in context entries (`asqr validate`; `asqr ask`
  resolves them).
- about files that do not exist ("file not found"), about paths that
  exist but are no regular file, such as a directory ("not a file"), and
  about image files the terminal UI cannot decode (section 8.8). That
  check reads only the image file's header. For an unsupported format
  the warning lists the formats that work. Both check a relative path
  against the absolute directory of the session file, so the warning
  names the file by an absolute path even when the session file was
  given by its bare name.
- when a side of a `diff`, inline or in a file of at most 1 MiB, looks
  like a unified diff: a line that is a full hunk header, `@@ -N[,N]
  +N[,N] @@` with optional text after a blank, so a hunk copied without
  its file headers counts, or a line starting with `@@ -` after a line
  starting with `--- `, `+++ ` or `diff --git`. A hunk header inside a
  line, as in prose about diffs, does not count. The warning says to
  give the text before and after the change as `old` and `new`, since
  asqr computes the diff.
- about a `code`, `markdown` or `diff` entry above 1 MiB, from its file
  or inline; for a diff, both sides count together. asqr still shows it.
  Image entries get no such warning.
- about fields of earlier format versions (section 3.6).

`asqr validate <file>` prints `<file>: valid` and exits with 0, or lists
every error and exits with 11. Warnings never fail it. A file that is
not JSON, or not shaped like a session, exits with 11 as well; a file
that cannot be read exits with 1.

What happens to an invalid file in a queue is in section 5.8.

### 3.5 JSON Schema

The repo ships the JSON Schema (`schema/session.v2.json`,
`schema/result.v2.json`). `asqr schema` prints the session schema and
`asqr schema --result` the result schema. Besides the shape, the schemas
state the rules a schema can state directly: `asqr` is the constant `2`
in both, a session holds at least one question, `context` holds at most
one entry (an empty list and `null` are valid), a `code` or `markdown`
entry or a side of a `diff` takes exactly one of `path` and `content`
(an empty `content` counts as given, a `null` as missing), and a `diff`
takes `old`, `new` or both. The descriptions of `intro`, question `text`
and option `description` say that they are Markdown, that a single
newline breaks the line and that raw HTML shows as written. The other
rules are validation's alone.

### 3.6 Earlier format versions

- A session of format version 1 is the validation error "this asqr no
  longer supports format version 1; `asqr skill` describes the current
  format (version 2)". Like any other version, it is checked before the
  rest of the file (section 3.1).
- A version 1 session that still waits in a queue when asqr is upgraded
  gets an error result (section 5.8) that names the version and points
  to `asqr skill`, so the asker waiting for it sees exit code 11 and
  what to write instead.
- The `image` field of format version 1 on a question or an option gets
  a warning of its own, saying that an image is a context entry of type
  `image`.
- A `required` field on a question does not make the file invalid: it
  is ignored and reported as unknown.
- asqr writes results in format version 2 (section 4.1). `wait` and
  `result` print a result of version 1 that an earlier asqr left in the
  outbox as it is, so an asker waiting across an upgrade still gets its
  answers.

## 4. Results and drafts

### 4.1 Result file

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
    `reason` (trimmed; absent when empty) and an optional `note` as for
    `submitted` (ADR 53)
  - `error`: the file was invalid, and `error` holds the message
- `submitted_at` holds the time of finishing, as RFC 3339 with the
  offset. It is present for every status.
- `session_sha256` is the SHA-256 of the session file's bytes as read
  from the inbox (section 5.7).
- For `submitted`, every question appears exactly once, in session
  order. For `cancelled`, `answers` holds only the questions with an
  answer or with a note that is not blank, in session order and in the
  same shape; an untouched default counts as an answer and carries
  `"defaulted": true` (section 4.2). The person did not submit any of
  it. For `error`, `answers` is empty.
- The fields are those of format version 1 plus `note` (section 3.6).

### 4.2 Answer state

A question is **answered** when:

- `single`: an option is selected, or the custom text is not empty after
  trimming
- `multi`: the custom text is not empty after trimming, or the number of
  selected options lies within `min` and `max`
- `text`: the typed text is not empty after trimming

Otherwise it is **skipped**.

- A default counts as answered, and the review tab marks it as a
  default. The answer carries `"defaulted": true` only when the person
  never edited the question. Any edit removes the flag, even one that
  restores the default, and so does writing a note.
- A note never answers a question. A skipped question keeps its note.
- `min` and `max` only apply once something is selected, so a `multi`
  with `min: 2` can be skipped. They count the picked options only; the
  own answer of a `multi` comes on top of them, so custom text answers
  the question whatever is selected (ADR 28).
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

### 4.3 Draft file

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

A draft "has an answer" (section 5.5) when any question has a selection,
typed text that is not blank or a note that is not blank, when the
session note is not blank, or when the draft cannot be read. The reject
reason does not count. A selection counts even where it does not answer
the question yet, since it is still work a replacement would throw
away.

## 5. Queues and files

### 5.1 Layout

```
<queue>/
  inbox/<id>.json                     sessions waiting, written by askers
  outbox/<id>.json                    results, written by asqr, owned by the asker
  drafts/<id>.json                    answers in progress
  archive/<id>.<ulid>.json            finished sessions (submitted, cancelled, error)
  archive/<id>.<ulid>.result.json     results moved aside by `ask --force` (section 5.5)
  archive/invalid.<ulid>.json         files whose stem is no valid id (section 5.8)
  lock                                the instance lock (section 5.6)
```

The four directories are created on first use.

### 5.2 Location

The default root is the platform's data directory:
`~/Library/Application Support/asqr/` on macOS, `$XDG_DATA_HOME/asqr/`
(usually `~/.local/share/asqr/`) on Linux. Named queues live in
`<root>/queues/<name>/`, and the default queue is `default`.

- `--queue <name>` or `ASQR_QUEUE` picks a named queue under the root. A
  queue name follows the id syntax (section 5.3).
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
are not supported: the lock is per host (section 5.6).

### 5.3 Ids and file names

- The file stem is the session id: `inbox/batch-01.json` is session
  `batch-01`.
- Id syntax: letters, digits, `-`, `_` and `.`, not starting with `.`, at
  most 200 bytes.
- Ids are compared case-insensitively on every platform: `Batch-01` and
  `batch-01` are the same session for every rule in section 5. A file
  keeps the spelling it was given, and so does its draft. Lookups search
  directory listings case-insensitively instead of building a path from
  the id.
- Ids asqr creates are ULIDs: the ids `asqr ask` assigns and the suffix
  of archive names.

### 5.4 What the watcher considers

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
  matched to it by question id and option id (section 4.3). What the TUI
  keeps of a replaced session is in section 7.9.
- A file that leaves the inbox without a result (taken back by hand)
  leaves the TUI. Its draft stays, and applies again if the session is
  dropped once more.

### 5.5 Writing into the inbox

`asqr ask` writes the session into a temp file in the inbox, whose
`.tmp` suffix the watcher ignores, and renames it into place.

What `ask` drops is a copy of the asker's file: pretty-printed, with the
`id` filled in and the paths of context entries made absolute. The
asker's own file is never changed. `session_sha256` in the result
(section 5.7) is the hash of that copy.

- No session with that id is waiting: the rename only succeeds while no
  file of that name exists, so two parallel `ask` calls with the same id
  cannot both win. The loser exits with 1.
- A session with that id is waiting and its draft has no answer: `ask`
  replaces it.
- A session with that id is waiting and its draft has an answer
  (section 4.3): `ask` refuses.
- A result with that id is still in the outbox: `ask` refuses, so an
  unread result is never lost. `ask --force` moves the old result into
  the archive (section 5.7) and then drops the new session.

A refused `ask` exits with 1 and names the reason on stderr. A session
being answered and an unread result are refused before anything moves,
even with `--force`. Only the loser of a race under `--force` has
already archived the old result.

These are rules of `asqr ask`, not of the queue. A file someone drops
into the inbox by hand replaces whatever is there, draft or not. A
replacement is never an error in itself; a replacing file that fails
validation gets its error result like any invalid file (section 5.8),
and the session it replaced leaves the TUI.

When no asqr holds the queue lock, `ask` still drops the file and warns
on stderr: "no asqr is watching queue <name>; start `asqr` in another
terminal". For a queue given by directory, the warning names the
directory.

### 5.6 The instance lock

One asqr instance per queue. On start the TUI opens `<queue>/lock` and
takes an exclusive advisory lock on it (flock on Unix).

- A lock that stays held for about 200 ms means another instance runs.
  asqr prints "another asqr is watching this queue (pid <pid> on
  <host>)" and exits with 1. Any other error is a plain failure.
- The file holds the pid and host name of the holder, only for that
  message. The file stays after exit and its text may be stale. The
  lock itself is released by the kernel when the process ends, so there
  is no stale-lock handling.
- Only the TUI holds the lock. `ask` and `status` only test it for a
  moment to see whether an asqr is watching. A starting TUI tries for
  those 200 ms, so such a test never makes it refuse to start.

### 5.7 Finishing a session

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

Apart from this recovery, the TUI does not read the outbox or the
archive back after a submit. It reads the archive only for the title
that `follows` names (section 7.2).

### 5.8 Invalid sessions

A file that fails validation (section 3.4) gets a result with
`"status": "error"` whose `error` names the field and the problem
(several problems joined with `; `), and is then archived, so it is
handled once. The TUI shows the file in a dialog (section 7.9).

An error result never overwrites an existing `outbox/<id>.json`: in that
case the file is archived without a result and the conflict is logged
and shown. A file whose stem is not a valid id gets no result at all,
since no asker can be waiting on that name. It is logged with its
original name and archived as `invalid.<ulid>.json`, without a notice.

A session file that cannot be read (permissions) stays in the inbox and
is reported with a notice.

A session of an earlier format version is invalid in the same way
(section 3.6).

So is a session with a context entry of a type this asqr does not know.
The asqr that shows a queue runs as a long-lived process: when asqr is
updated while it runs, the new `asqr ask` accepts a session with a type
the running asqr does not know, and its error result says that a newer
asqr may support the type.

### 5.9 Cleaning up

Nothing is deleted automatically, apart from drafts in step 3 of section
5.7. Drafts of sessions that left the inbox stay (section 5.4).

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

## 6. The command line

### 6.1 Commands

| Command | What it does |
|---|---|
| `asqr` / `asqr watch [--no-notify] [--no-bell] [--no-title] [--layout auto\|beside\|below]` | the TUI (section 7) |
| `asqr new` | prints a session skeleton without an id, which `asqr ask` assigns |
| `asqr ask <file> [--wait] [--timeout <secs>] [--force]` | validates, assigns a ULID when the id is missing, makes the paths of context entries absolute, drops the file (section 5.5), prints the id |
| `asqr wait <id> [--timeout <secs>]` | waits for the result and prints it; blocks without `--timeout` |
| `asqr result <id>` | prints the result if it is there, like `wait --timeout 0` |
| `asqr status [--json] [--limit <count> \| --all] [--since <age>]` | waiting sessions (with or without a draft answer), answered sessions (results in the outbox), and the lock holder |
| `asqr paths [--json]` | section 5.2 |
| `asqr validate <file>`, `asqr schema [--result]` | sections 3.4 and 3.5 |
| `asqr prune --older-than <duration> [--results]` | section 5.9 |
| `asqr skill [--install <dir>]` | prints the skill, or writes it to `<dir>/asqr/SKILL.md` (section 9) |

### 6.2 Flags and environment

Every command takes `--queue` or `--dir` (section 5.2). `--no-notify`,
`--no-bell`, `--no-title` and `--layout` may also stand before `watch` or
alone (`asqr --no-bell`); next to any other command they are a usage
error. `--no-notify` and `--no-bell` switch off the desktop notification
and the bell, and `--no-title` leaves the terminal title alone (section
7.9).

`--layout` places the context pane (section 7.4). Without it,
`ASQR_LAYOUT` sets the layout, and without either it is `auto`. A
`--layout` after `watch` wins over one before it. An empty
`ASQR_LAYOUT` counts as unset. Any other value than `auto`, `beside` or
`below` is a usage error when asqr watches the queue; the other
commands ignore the variable.

`NO_COLOR` (<https://no-color.org>) set to any value but the empty one
makes the TUI draw without colours (section 7.3), a value that is not
UTF-8 included. An empty `NO_COLOR` counts as unset.

The TUI shows the home folder, `HOME`, as `~` at the start of the paths
it shows of context entries it cannot show (section 8.3), a value that
is not UTF-8 included. An empty `HOME`, or `/`, counts as unset, and
paths then show whole.

Other variables asqr reads: `ASQR_QUEUE` and `ASQR_DIR` (section 5.2),
`TERM` (sections 7.9 and 8.9), `TERM_PROGRAM` and `TMUX` (section 8.9).

### 6.3 `ask`

- `--timeout` needs `--wait`.
- With `--wait`, the id goes to stderr and stdout carries only the
  result JSON, so the output can be piped into `jq`.
- Without a watching asqr, `ask` warns (section 5.5), and `--timeout`
  bounds the wait.

### 6.4 `wait` and `result`

`wait` and `result` print the result file as it is, and exit by its
status. `wait` checks the queue every 200 ms. When the timeout passes
first, it prints "asqr: no result for <id> yet (timeout)" on stderr and
exits with 12; `result` does the same at once while the session waits.
Both end with 13 when nothing in the queue has the id ("no session
"<id>" in this queue"), which includes a session taken out of the inbox
without a result, or when only its archive entry is left, since its
result was removed and none will come ("the result of <id> is gone").
So while an asqr watches the queue, the asker never waits for a result
that will not come.

### 6.5 `status`

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
  has the form of `prune --older-than` (section 5.9).
- `--limit <count>` lists at most that many, 10 when it is not given.
  `--all` lists every result and cannot be combined with `--limit`.
- `answered_total` counts the results `--since` keeps, before the limit
  cuts the list. The text form writes "answered (10 of 57):" when the
  list is cut and "answered (3):" when it is not.
- `waiting` always lists every waiting session.

### 6.6 Exit codes

The same for every command:

| Code | Meaning |
|---|---|
| 0 | success: the result is `submitted`, or the file was dropped (`ask` without `--wait`), or the command did its job |
| 1 | failure of asqr itself (I/O, a refused `ask`, a held lock, no terminal) |
| 2 | usage error |
| 10 | the result is `cancelled` |
| 11 | the result is `error`, or a file fails validation |
| 12 | timeout: no result yet |
| 13 | no result will come: nothing in the queue has the id, or the session left without one |

## 7. The TUI

### 7.1 Starting and quitting

`asqr` (or `asqr watch`) starts on the queue chosen by section 5.2. It
needs a terminal on stdin and stdout; otherwise it prints "asqr needs a
terminal" and exits with 1. It creates the queue's directories and
takes the queue lock (section 5.6). `q` quits with exit code 0 and gives
the terminal back as it was, also after a panic. On Unix, SIGTERM,
SIGINT and SIGHUP quit as `q` does. SIGKILL ends asqr at once and leaves
the terminal as asqr had set it up.

asqr asks the terminal for bracketed paste (section 7.7) and for focus
reports (section 8.8). It turns both off again when it quits or panics,
not when SIGKILL ends it. A terminal that refuses either runs asqr
without it. On quitting it also resets the terminal title (section
7.9), deletes the Kitty images it sent (section 8.8) and restores the
pane's passthrough setting inside tmux (section 8.9).

### 7.2 The screen

A stack of rows over the full width, with the question kept to a
reading width and a context pane beside or below it when the question
has context (section 7.4, ADRs 30 and 31):

```
 asqr  Alt rework 3, batch 1                          default · 2 waiting
       from claude-code, torchsnap mascots · follows alt-rework-3-batch-00
       Pick the best line per mascot. More sends it back with new variations.

 ← ☒ 300  ☐ 301  ☐ 302  ☐ 303  ✔ Review
 ─━━━━━━━──────────────────────────────────────────────────
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

- The title bar has no fill: `asqr` is bold in the primary colour. With
  nothing waiting it reads `asqr · queue: <queue> · N sessions waiting`,
  the queue and the count dim, as beside a session. A queue name too long
  for the line is cut from the left with `…`, at a `/` where possible,
  so the count stays visible.
- With a session shown, the session's title (or its id) follows `asqr`
  after two blanks, bold, in one line that ends where the reading width
  ends. The queue and `N waiting` stand dim at the right end. Where the
  whole title and the queue do not both fit, the queue goes, then the
  title is cut and ends in `…`. The count always shows.
- Under the title, from its first column: `from` and `follows` in one
  dim line, `from <from> · follows <follows>`, cut with `…`; then the
  intro in the text colour, wrapped at the reading width, at most two
  lines; then a blank line before the tab bar. A cut intro ends in
  `… i: more`, and `i` shows the whole intro (section 7.6). While a
  field, a note or the context pane has the keys, where `i` would be
  typed or taken by the pane, it ends in `… esc, i: more`. The tab bar
  keeps the full width.
- `follows` shows the title of the session it names, in quotes, when
  asqr finds that session with a title: a waiting session, else the
  newest finished session of that id in the archive
  (`archive/<id>.<ulid>.json`, not a result and not an `invalid.*`
  entry), the id compared ignoring case. A session that names its own id
  finds only the archive. A waiting session without a title, and
  anything else, shows the value as written. asqr looks the title up
  when it loads or reloads the session.
- On a low terminal the header gives way while the tab's area would
  have fewer than 15 rows, the rows a context pane below the question
  needs: first the blank line, then intro lines down to one. The `from`
  and `follows` line stays.
- The tab bar lists every question by its `header` (or id) with `☒`
  once it is answered and a dim `☐` while it is not, and `✔ Review`
  last. The current tab is bold in the primary colour, and the line under
  the bar is heavy (`━`) and in the primary colour under it, thin and dim
  elsewhere. When the tabs do not fit, the bar scrolls around the current
  one and shows `←`/`→` where more follow.
- Under the question text a hint says how to answer: "pick one", "pick
  between 2 and 3 items", "pick at least 2 items", "pick up to 3 items",
  "pick 2 items", "pick any" or "type your answer". A `single` with
  `custom` adds ", or type your own"; a `multi` does not mention its own
  answer (ADR 8).
- Options show `( )`/`(•)` in a `single` and `[ ]`/`[x]` in a `multi`,
  a chosen mark bold in the primary colour and the option's number dim.
  Option descriptions start after the widest label and wrap under
  themselves; where that leaves less than 30 columns, they go below the
  label.
- The own-answer row is the last row of a question with `custom`; its
  mark shows whether the typed text counts, and when it counts it is
  bold in the primary colour like a chosen option's mark (ADR 37). A
  `text` question consists of its answer field only.
- The notes line sits under the options (section 7.7).
- The line above the key bar, the status line, shows notices (about the
  queue) and messages (about the last key) until the next key or paste,
  a notice over a message. Each has a kind that gives it its colour
  (section 7.9).
- The key bar shows the keys that work where the cursor is; they change
  inside a field. Each key is bold in the primary colour and its
  description dim, without a fill. Hints that do not fit its width are
  left out whole, never cut: first `n`, then `tab`, then `←`/`→`, then
  the others from the right. `?` and `q` stay longest, as the help lists
  every key (ADR 31).

### 7.3 Colours

- The colours (ADR 37) are the terminal's named colours, so its theme
  decides how they look: cyan as the primary colour, yellow for the
  focus, green and red for the length counter and diff lines, yellow,
  red and green for notices and messages by their kind, dim for
  secondary text.
- The row under the cursor, a field's head under it while the field
  does not have the keys, the current entry of the session list and the
  focused context pane's name show the focus fill: yellow reversed, in
  one colour over the whole row, or over the name and its blanks in the
  pane's header rule. One focus fill shows at a time. The focused
  context pane's rules and edge turn yellow (section 8.1). The submit
  button is the only other fill (section 7.8).
- The help, the whole intro and the session list show in a frame with
  rounded corners in the primary colour, a bold title, a dim footer and
  a blank column inside each side; the help's keys are bold in the
  primary colour.
- Code and Markdown keep their highlighting colours. Marks, pointers,
  text and the fills carry every meaning, so nothing depends on colour
  alone.
- Under `NO_COLOR` (section 6.2) the TUI draws without colours. Text,
  marks, pointers, bold, dim and reversed stay, so the screen says the
  same as with colours. The bar of a Markdown quote is dim instead of
  blue. Images keep their colours. Talqi on the empty queue is the
  outline of asqr's emblem in the same glyphs, without any colour; cells
  between its lines keep the background (section 7.5).

### 7.4 Size and layout

- **Reading width:** the question and the review take at most 100
  columns and start at the left, with or without a context pane. The
  tab's area, W×H, is the area under the tab bar inside a margin of one
  column on each side.
- **Context pane:** a question that has context, or has an option with
  context, shares the tab's area with a context pane (section 8):
  - Beside: the question takes half of W after the blank separator
    column, at least 60 and at most 100 columns, and the context pane
    takes the rest. Its body is two columns narrower than the pane, 38
    at the narrowest (section 8.1). Beside fits from W = 101, a terminal
    of 103 columns.
  - Below: the question keeps its width and takes the rows the tallest
    question with context in the session needs, at most half of H, and
    the context pane takes the rest over the whole of W. So the pane
    keeps its place while the person moves between questions. A
    question's rows are counted at that width without a
    cursor: every field unfocused with the text typed so far, the own
    answer's label highlighted as under the cursor, a row for the
    counter of a field with a length, and no row for the cursor after a
    full line. The blank after a highlighted label shows, so a label
    ending at the edge moves the answer's text one column right, which
    can take a row more. Counted highlighted, the question keeps its
    rows as the cursor moves onto the own answer and off it. While a
    field or a question's note has the keys, the current question
    counts as drawn, its text cursor included, so
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
  - `--layout beside` or `--layout below` (or `ASQR_LAYOUT`, section
    6.2) is used when it fits, and the other one when only that fits.
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
  `↑` scrolls back to the start row by row. On the text's last line, the
  counter of a field with a length takes the area's last row and the
  cursor's line the row above it; where the area has no room for both,
  the cursor's line shows.

### 7.5 The empty queue

- With nothing waiting, the screen says "Nothing to answer. New sessions
  show up here as soon as they arrive." Notices still show. While the
  dialog of an invalid session shows (section 7.9), the sentence stands
  alone. Otherwise, above the sentence, with a blank row between them,
  Talqi, asqr's mascot, shows as asqr's terminal emblem drawn in text
  cells (ADR 43): a fixed block of 44 columns by 22 rows, each cell a
  glyph with its own 24-bit foreground and background. Cells outside
  the figure keep the terminal's background. Talqi is centred across,
  and Talqi and sentence together are centred down the screen. Talqi is
  text built into asqr and uses no image protocol.
- Talqi shows on terminals of at least 32 rows and 44 columns, with
  colours and without (section 7.3). On smaller ones, 80×24 included,
  the sentence stands alone.
- Terminals asqr recognises as drawing sextant characters themselves
  (kitty, Ghostty, WezTerm, iTerm2 from 3.6.7, foot, terminals built on
  VTE, Konsole, Alacritty, Rio and Contour, from the versions ADR 43
  names) get Talqi in sextants, every other terminal in quadrant
  blocks. Inside tmux, the terminal of the client tmux draws for
  decides; inside GNU screen or Zellij, Talqi is drawn in quadrant
  blocks. There is no setting for it.

### 7.6 Keys

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
| `pgup`/`pgdn` | scroll the context pane by a page, also while typing in a field |
| `ctrl-u`/`ctrl-d` | scroll the context pane by half a page, also while typing in a field |
| `q`, `ctrl-c` | quit, keeping the draft; in the context pane `q` goes back |
| `?` | help |

- A selection beyond `max` in a `multi` is refused with the message "at
  most N options".
- `enter` on the last question moves on to the review tab.
- `n` on a question with `note: false` says that the question takes no
  note.
- Inside a field every key that produces text types, so `n`, `q` and
  the others work again after `esc` (section 7.7). `ctrl-c` quits
  everywhere, except over the help and over the dialog of an invalid
  session, which it closes like any other key.
- `o` opens the image shown (section 8.8) with the system's opener
  (`open` on macOS, `xdg-open` elsewhere); when the opener cannot be
  started, a notice says why. On a code, Markdown or diff entry it
  opens nothing and says "o opens images only; z shows this entry full
  screen", in the full-screen view only "o opens images only", and
  where no entry shows it says "nothing to open here".

The context pane:

- `tab` and `shift-tab` move the focus into the context pane (section
  8.1) wherever one shows, also from a field, and do nothing otherwise,
  such as next to the one-line hint. They never type into a field.
- The focused pane takes every key: `↑`/`↓` and `k`/`j` scroll by line,
  `←`/`→` and `h`/`l` by 4 columns sideways where the entry's lines are
  cut at the pane's edge, `pgup`/`pgdn` by a page, `ctrl-u`/`ctrl-d` by
  half a page, `g`/`G` and `home`/`end` to the ends. `z`, `o` and `?`
  keep their meaning and `ctrl-c` quits. `tab`, `shift-tab`, `esc` and `q` give the
  focus back to where it was, a field being typed in included, so a word
  typed after `tab` never quits asqr. Every other key does nothing.
- While the pane has the focus, its name shows the focus fill and its
  rules turn yellow (section 8.1), the question's current row keeps its
  pointer and bold text but loses its fill, a field's text cursor hides,
  and the key bar and the help show the pane's keys, with `? help` and
  `tab/esc/q back`. A pane that goes away, as after a resize down to the
  hint, gives the focus back.
- `pgup`/`pgdn` scroll the context pane by a page and `ctrl-u`/`ctrl-d`
  by half a page from anywhere, also from inside a field, while a pane
  shows. A page is the height the pane, the full-screen view or the
  intro shows, and half a page is half of it, rounded down, and at least
  one line (ADR 52). Without a pane, `pgup`/`pgdn` do nothing inside a
  field, and `ctrl-u` and `ctrl-d` undo and delete there (section 7.7,
  ADR 31).
- `z` shows the context pane of the entry shown over the question's
  area, its ledge included. It acts on every kind of entry (ADR 31). The
  full-screen view reads like the focused pane: it takes every key, so
  `enter`, the digits, `n` and the others cannot act on the question
  hidden behind it. It scrolls with the pane's keys, `o` and `?` keep
  their meaning, and `z`, `esc`, `tab` and `q` leave it for where the
  focus was before: the question, a field being typed in, or the
  focused pane. The key bar shows the pane's keys with `z/esc/q back`.
  The help is titled "Keys of the full-screen view" and lists the
  pane's scroll keys, `o`, `ctrl-c`, `?` and `z/tab/esc/q  leave the
  full screen`.

Overlays and dialogs:

- Help closes on any key.
- `i` shows the session's whole intro in a frame over the question,
  rendered as in the header (section 7.10), with the key bar left free.
  An intro longer than the frame scrolls: `↑`/`↓` and `k`/`j` by line,
  `pgup`/`pgdn` by a page, `ctrl-u`/`ctrl-d` by half a page, `g`/`G`
  and `home`/`end` to the ends. The frame's bottom edge then shows which
  lines are visible, such as `4–15 of 16`. `esc`, `q` and `i` close it and
  `ctrl-c` quits; every other key does nothing, so a key meant to
  scroll never closes it. On a session without an intro, `i` does
  nothing.
- The session list (`L`) lists every waiting session in queue order
  (section 5.4) and switches between them: `↑`/`↓` or `k`/`j` move,
  `enter` switches to the session, `esc` or `L` close the list, and `q`
  quits asqr.
- A key pressed while the dialog of an invalid session shows (section
  7.9) closes it and does nothing else. While it shows, the key bar says
  `any key  close`.

### 7.7 Fields and counters

Typed text is entered in place, in the line where it is shown, without
a frame. Every field and the note take several lines: `ctrl-j` adds a
line (ADR 36).

- **The own-answer row** is a field as soon as the cursor lands on it:
  every key that produces text types into it. `←`/`→` and `↑`/`↓` move
  the text cursor (see below), and `esc` leaves the field while the
  cursor stays. `←` with the text cursor at the start of the text goes
  to the previous question, `→` at its end to the next one (ADR 36).
  After `esc`, `↑`/`↓` move on as usual; where the move cannot leave the
  row (the single row of a `text` question, `↓` on the last row), they
  focus the field again. `enter` picks the own answer and moves on, like
  an option. A text longer than the line wraps onto further lines under
  its start, after the row's head, focused or not, and is never cut with
  `…`. Every line after the first starts there too. When the head leaves
  the text less than 20 columns, the further lines start 4 columns in
  (ADR 36). On a `multi` question the own answer counts once it has
  text.
- **A `text` question's answer** is a field that is active when the
  question is shown. It grows with its lines, and `enter` moves on.
  `←`/`→` at the start or end of its text switch questions as on the
  own-answer row.
- **The note** is edited in its line under the options after `n`;
  `enter` or `esc` leave it.
- **The session note** and **the reject reason** on the review tab are
  fields like the own answer: `enter` submits or rejects, and they wrap
  the same way.
- **Moving in a field:** in every field and the note, `↑`/`↓` move the
  text cursor to the row above or below as the field shows it, that is
  the wrapped rows, not the text's lines, at the same display column or
  the end of a shorter row. While `↑` and `↓` follow each other they
  keep the column they started from. A field is left with `↑` only from
  its first row and with `↓` only from its last row, as with `←` and `→`
  at the start and end of its text; the note stays where it is (ADR 36).
  `↓` from the last row of a line whose end shows on a row of its own,
  as after a text that fills its row, goes to that end first.
- **Entering a field:** the cursor that moves down onto a field's row
  puts the text cursor at the end of the field's first row, as drawn;
  everywhere else, as moving up onto it, switching questions or after
  `esc`, the text cursor goes to the end of the text, which is the end
  of its last row. In a field of one row the two are the same.
- Every edit is saved to the draft continuously, the session note and
  the reject reason included; there is no discard.
- Pasted text goes in at the text cursor of the focused field or note
  as a whole. asqr asks the terminal for bracketed paste (section 7.1),
  so a line break in a paste is never an `enter` that picks, submits or
  rejects. Every field keeps the pasted line breaks; `\r` and `\r\n`
  become `\n` (ADR 36). A tab becomes blanks up to the next multiple of
  4 columns, counted from the start of its line, and other control
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
  screen". Over the help and over the dialog of an invalid session, a
  paste closes it and does nothing else; over the intro and the session
  list it does nothing.
- **Keys in a field,** besides `enter`, `esc`, `ctrl-j` and the
  `tab`, `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` rules of section 7.6:
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

### 7.8 The review tab

The last tab lists every question with its answer on one line: the
chosen options by label, own text and a text answer in quotes (line
breaks shown as `⏎`), `(default)` after an untouched default, `-`
for a skipped question and `+note` for a note. A line longer than the
screen is cut with `…`. Below the list, the counts of answered, skipped
and defaulted questions, the numbers bold and the words dim, then:

- **Submit**: `[ Submit ]`, always filled in the primary colour and
  bold, brackets included (ADR 37). With the cursor on it, the pointer
  before it is bold yellow. `enter` submits. The review tab opens with
  the cursor here (ADR 8), and the keys stay commands on this row.
- **Note to the asker**: a live field for an optional note to the whole
  session; `enter` submits with it (ADR 29). There is no `n` on the
  review tab.
- **Reject**: a live field for the optional reason; `enter` rejects the
  session with it. The result keeps the session note and the questions
  with an answer or a note (section 4.1).

`enter` on a question in the list moves to that question. The review
shows no context.

After a submit or reject the session after it in the queue takes over,
or the one before when it was the last, or the empty screen when none
is left. A notice confirms what was written, with the session's title,
or its id for a session without a title:

    ✔ sent "Release 0.2.0" · 1 answered, 2 skipped
    ✗ rejected "Release 0.2.0"

The counts are those the review showed, a default counted as answered.
A name too long for the status line is cut to the columns the rest of
the line leaves and ends in `…`, so the counts and the closing quote
show whole. The line for a sent session is a success, the one for a
rejected session information (section 7.9). Like every notice, it stays
until the next key or paste, also over the next session and over the
empty screen. A submit or reject that does not finish (section 5.7)
keeps the session and shows its own notice instead.

### 7.9 Sessions, notices and notifications

- A new session triggers a desktop notification (OSC 9 or 777, which
  terminals such as Ghostty, iTerm2 and kitty show) and a terminal bell.
  `--no-notify` and `--no-bell` switch them off. The notification is
  OSC 777 when `TERM` starts with `rxvt` or `foot`, and OSC 9 otherwise.
  It names the session ("new session <id>"), or counts them ("N new
  sessions") when several arrive at once; OSC 9 puts "asqr: " in front,
  OSC 777 uses "asqr" as the title. Inside tmux it goes out in tmux's
  passthrough wrapper (section 8.9). Sessions waiting when asqr starts,
  and replaced files, trigger nothing.
- The terminal title counts the waiting sessions, "asqr · N waiting",
  and changes with the count (OSC 0, the window and the icon title).
  Before the first title asqr pushes the terminal's title (`CSI 22;0 t`).
  On quitting, also after a panic, it writes an empty title and then
  pops the pushed one (`CSI 23;0 t`), both before it leaves the
  alternate screen: a terminal with a title stack shows its title from
  before asqr again, one without keeps the empty title, which some show
  as a title of their own (ADR 40). Inside tmux these go to tmux
  plainly; tmux shows the title as the pane's. `--no-title` switches the
  title off, and asqr then writes none of these sequences.
- A replaced session file keeps the answers that still fit (section
  5.4) and where the person was: the question shown, found again by its
  id, or the review; the row with its focus, so a field left with `esc`
  stays without the keys, a field typed in keeps its text cursor, and an
  open note stays open while the question still takes notes (otherwise
  the row takes the keys as after the note); the context pane's focus
  and the full-screen view while the same entry shows; and the scroll
  position of every entry that reads as before (section 8.1). A row
  that is gone, such as an own answer turned off, gives way to the row
  in its place, or the last one when fewer are left, which is entered as
  if the cursor moved there. The message "<title> changed on disk and
  was reloaded" says so, with the id for a session without a title. An
  open intro overlay closes when the new file has no intro. A session
  list open while a session leaves keeps its cursor on a waiting
  session, and closes when none is left.
- A session colliding with an unread result (section 5.7) and a failure
  to write a draft or a result trigger a notice. A failure never ends
  asqr; answers stay in the TUI.
- An invalid session file answered with an error (section 5.8) shows in
  a dialog framed in the warning colour, also when no session is
  waiting (ADR 49): its title names the session, and each error of the
  file stands on a line of its own behind a `•`, wrapped under its
  start. Errors that do not fit the screen are left out whole, and a
  last line counts them. The dialog leaves the status line and the key
  bar free; the status line says "<id> is invalid and was answered with
  an error" while the dialog shows. When several files fail in one
  scan, their dialogs show one after another, each closed by a key
  (section 7.6). A key pressed while the dialog waits but no frame
  showed it, as under the message of a terminal too small for asqr,
  acts on the session as it would without the dialog, and the dialog
  shows on the next frame with room for it.
- Every notice and message has a kind, which gives it its colour in the
  status line (ADR 38):
  - error, red, after a `✗`: an operation failed and nothing was done,
    such as an inbox or a session file that cannot be read, a draft or a
    result that cannot be written, or an image that cannot be opened;
  - warning, yellow, after a `!`: asqr refused an action or set
    something aside on purpose, such as a refused key or paste, a
    session archived next to an unread result, an invalid session
    answered with an error, or a submit not sent because the file
    changed on disk;
  - information, the text colour: plain news, such as a session file
    reloaded after it changed, or a session rejected;
  - success, green: a session was sent (section 7.8).

### 7.10 Markdown

The intro, question texts and option descriptions (section 3.1) and
Markdown entries (section 8.7) render the same way (ADRs 34 and 35):
headings bold
(level one also italic and underlined, level two underlined), emphasis,
inline code in yellow, lists with bullets, task lists with `☐` for an
open task and `☑` for a done one, which is dim apart from the items
nested under it, quotes in the style of the text around them behind a
blue bar, links underlined with their URL dimmed, tables whose cells
keep the styles of running text and whose columns follow the alignment
of the delimiter row, a column without one with its header centred and
its data left-aligned, rules, and code blocks highlighted when the
fence names a language.

- Each text renders at the width it gets: a question text at the
  question's width, which stops at the reading width; the intro at the
  header's width; an option description in the columns from where it
  starts to the end of the line, beside its label or below it (section
  7.2); a Markdown entry at the pane's width.
- A table fits the width it gets: a word wider than its column, such as
  a long URL, breaks between characters and continues on the next line
  of its cell in its own style, so a link's URL stays dim, and words
  that fit their column stay whole. A table whose columns do not fit
  even at one character each wraps as below.
- A line wider than the width the text gets, such as a line of a code
  block, a long URL in running text, a table whose columns do not fit
  even at one character each, or any line with a halfwidth sound mark
  such as `ﾞ`, which counts as no column, wraps: each further row
  repeats the blanks and quote bars the line starts with, then blanks in
  place of a list item's `•`, `☐` or `☑`, or its number with the box
  after it, then a dim `↪`, then the rest. Where that leaves no room for
  a wide character after the `↪`, the further rows start with the `↪`
  alone, and where not even that fits, the line breaks without one. A
  number counts as an item's
  only at the start of the line, after quote bars, so a line of code
  such as `1. x` in a code block stays code. A wide character that would
  cross the end of a row starts the next one. Nothing is cut, and
  nothing scrolls sideways. Tabs in code blocks expand to stops of 4
  columns.
The text fields and Markdown entries differ in two rules:

- In the text fields, a single newline breaks the line, and a blank line
  starts a new paragraph. Inside a list item the next line starts under
  the item's text, inside a quote behind its bar. Code blocks keep their
  lines, and inline code shows a newline as a blank. Raw HTML shows as written, in the style of the
  text around it, so `Vec<String>` stays as it is. An HTML block keeps
  its lines as they are, comments included. An autolink such as
  `<https://example.com>` is a link.
- Markdown entries are documents and keep CommonMark's rules: a single
  newline joins the lines, and raw HTML is left out.

The intro under the title shows these styles in the text colour, as
the `i` frame does.

## 8. Context

### 8.1 The context pane

Context appears only when a question or one of its options has some
(section 3.3).

- The option under the cursor shows its own context, so the person can
  browse the options before picking; this holds for `single` and
  `multi`. Every other row, and an option without context, shows the
  question's context, or nothing when the question has none.
- The room for the context stays while the cursor moves, also when the
  current row shows none, so the layout does not jump.
- The context pane goes beside or below the question, or shrinks to a
  one-line hint, by the terminal's size and `--layout` (section 7.4).
  Its keys are in section 7.6.
- The pane stands on a ledge (ADR 54). Its first line is a dim rule
  with the entry's name set into it in bold: its `title`, or the file
  name of its `path` (a diff's `name`), or its kind, `code`, `markdown`
  or `diff`, when it has neither. Beside the question the rule reads
  `╭─ name ─────` and turns down into a dim `│` along the pane's left
  edge, and the content starts two columns right of it. Below the
  question it reads `── name ─────`, without an edge, after a blank row.
  Either way the rule runs on to the pane's right edge, and a name
  longer than it leaves room for is cut with `…`.
- The pane's last line is a dim bottom rule: `╰──── 1–31 of 58 · tab ─`
  beside the question, `──── 1–31 of 58 · tab ──` below it. When the
  entry is taller than the pane, it says which lines show, such as
  `4–15 of 120`. When the entry is taller or wider than the pane and the
  pane does not have the focus, it also names `tab`, as in
  `1–10 of 31 · tab`, or `tab` alone for an entry that is only wider:
  on a narrow terminal the key bar leaves out its `tab` hint early
  (section 7.2). Where the rule has no room for it, the `tab` goes
  first. When the entry fits, the rule carries no text and runs solid to
  the pane's right edge. A row without context leaves the pane empty,
  its ledge included (ADR 31).
- While the pane has the focus, its name shows the focus fill, its
  blanks included, and its rules and edge turn yellow; the position
  stays dim.
- The full-screen view (`z`) shows the pane with the ledge it has beside
  the question: `╭`, left edge and `╰`.
- Below the header, a viewer shows the entry (sections 8.4 to 8.8).
- Every entry keeps its scroll position while its session waits: moving
  to an option with another entry shows that entry where the person
  left it, or at its top the first time, and coming back restores the
  first one's. Rows that show the question's entry share its position.
  A replaced session file keeps the positions of the entries it leaves
  as they were and starts the others at their top (section 7.9).

### 8.2 Loading

- asqr reads, highlights, renders, decodes and scales an entry apart
  from drawing the screen, so keys keep working while it loads (ADR
  46). The pane waits up to 100 ms for the entry it shows; one that
  takes longer shows
  a dim `loading…` in the middle of the pane, below the header that
  names it, until it is ready. A new size of the pane, also the `z` view,
  lays the entry out again the same way. While a key, a paste, a resize
  or a quit waits to be handled, the pane does not wait and shows
  `loading…` at once, so holding a key moves over slow entries without
  stopping on each. While the pane shows `loading…`, its scroll keys
  move nothing, and the entry keeps where the person left it.
- asqr keeps what it loaded for three questions: the previous, the
  current and the next one, with the question's entry and every
  option's. While a question shows with its pane, asqr loads the
  entries of that window ahead: the one shown first, then those of the
  question's other rows, starting below the cursor, then those of the
  next question and of the one before. Everything else goes, also an
  entry that was shown, and coming back to it loads it again. Nothing
  loads ahead for the `z` view, the one-line hint or a question without
  context. An entry loaded ahead before the pane changed its size may
  show `loading…` when it is shown.
- A load that is no longer shown runs to its end, and what it loaded is
  kept while it is inside the window. Coming back to an entry whose load
  still runs waits for that load instead of starting another.
- An entry's files are read when the entry is loaded; both sides of a
  diff are.

### 8.3 Entries asqr cannot show

An entry asqr cannot show draws a card in place of its content (ADR
49): a frame with rounded corners in red, inside it `✗` and the reason
in bold red, a blank row, the file name, and the folder on the rows
after it, dim. The reason is worded as the system or the image decoder
gives it, or as asqr's own checks say it: "relative path", "not a
regular file", "binary file", "image file larger than 64 MiB". The
folder shows the home folder as `~` (section 6.2) and breaks after a
`/`, a part longer than a row where the row ends; the path is never
shortened. The card is as wide as its text, at most 60 columns, and
centred in the pane; a card taller than the pane starts at its top and
scrolls. The pane's header shows a red `✗` before the entry's name; the
one-line hint shows no mark.

Which files get a card:

- Every kind: a relative path (a path is never resolved against the
  working directory, section 3.3), a missing or unreadable file, and a
  path that is not a regular file, such as a directory or a FIFO. A file
  that turns into a FIFO between asqr's check of its path and its
  opening shows the card of a path that is not a regular file.
- Code, Markdown and diffs: a binary file (one with a NUL byte). For a
  diff, the card shows for the first side that fails, its reason after
  `old side: ` or `new side: `.
- Images: a file that cannot be decoded, and an image file larger than
  64 MiB, which is not read; a file that grows past that while asqr
  reads it is refused the same way. A broken image never becomes an
  error result.

For code, Markdown and diffs:

- Bytes that are not UTF-8 show as `�`. A code or Markdown entry
  without any text shows a dim "empty".
- Of a file longer than 8 MiB, only the first 8 MiB are read. A dim
  "first 8 MiB shown" heads the content and a dim "… the rest is cut
  after 8 MiB" ends it, so the end of what shows does not pass for the
  end of the file. In a diff, the rest of the other side then shows as
  removed or added. When the 8 MiB limit falls between the `\r` and the
  `\n` of a line end, the read stops before the `\r`, so the note on
  line endings names no `CR` end the file does not use.

### 8.4 Lines of source

Code entries and diffs show lines of source:

- Lines do not wrap. A line wider than the pane ends in a dim `›`, and
  the pane scrolls sideways (section 7.6) while the gutter (and in a
  diff the sign) stays in place. Scrolled sideways, a line whose hidden
  start holds more than blanks begins with a dim `‹`. Each marker takes
  a column of the line, and a wide character that would cross into it
  shows as blanks.
- Tabs expand to the next stop of 4 columns, counted from the start of
  the line. Any other control character shows as its picture from
  Unicode's Control Pictures block, such as `␛` for escape; the C1
  controls, which have none, show as `�`.

### 8.5 Diffs

- asqr computes the diff from the old and the new text, line by line
  (ADR 33), and shows it unified: hunks with 3 unchanged lines of context around
  each change, and a dim line of `┈` between two hunks. A dim gutter
  holds the old and the new line number, followed by the sign: `-` for
  a removed line, `+` for an added one, a blank for an unchanged one.
- Removed lines are red and added lines green, in the terminal's
  palette, so its theme decides the shades. A changed line shows as its
  old version removed and its new version added, each whole in its
  colour, with nothing marked inside the line.
- A diff without `old` starts with a dim "new file", one without `new`
  with "deleted file"; when that file is empty, "empty file" follows.
  Two sides with the same text show "no changes".
- A last line without a newline gets a dim "no newline at end of file"
  below it, but only when the other side ends with one: the removed and
  the added line would look the same otherwise.
- Lines compare without their line ends: `\r\n` and a lone `\r` count
  as `\n`. When the two sides use different line ends, a dim note says
  so once and names the ends of both sides, as `line endings differ:
  old CRLF, new LF`, above the hunks or below "no changes". `CRLF` is
  `\r\n`, `LF` is `\n` and `CR` a lone `\r`. A side with ends of several
  kinds is `mixed` with each kind, as `old mixed CRLF and LF`. Like
  every note, it is cut at the pane's edge. A side of a single line
  without a line end differs from neither.
- Diffs have no syntax highlighting, and the pane shows the hunks only,
  never the whole file.
- Computing a diff may take up to a second. Past that, asqr shows a
  diff that is still correct but may mark more lines as changed than
  needed.

### 8.6 Code

- A code entry (ADR 34) shows its text exactly as written, highlighted
  in its language (section 3.3) with the 8 colours of the terminal's palette,
  so its theme decides the shades. Without a language, or with one asqr
  does not know, the text is plain. A file name without an extension,
  such as `Makefile`, counts as a whole, and a `.md` file shows its
  source. A code entry longer than 1 MiB shows plain, without
  highlighting.
- A dim gutter holds each line's number.

### 8.7 Markdown entries

A Markdown entry (ADR 34) shows rendered as section 7.10 describes,
wrapped at the pane's width, without a gutter and without sideways
scrolling.

### 8.8 Images

- Terminals that support the Kitty graphics protocol (Ghostty, kitty),
  the iTerm2 protocol (iTerm2, WezTerm) or Sixel show images inline.
  Terminals without any of these get a coarse block rendering.
- asqr decodes PNG, JPEG, GIF (first frame only), WebP, BMP, TIFF, ICO,
  TGA, PNM, QOI, DDS, OpenEXR, HDR and farbfeld. It detects the format
  from the file's content, and from the extension when the content does
  not match a known signature. SVG and AVIF are not decoded.
- The protocol is detected once at start, by querying the terminal. A
  terminal that answers nothing within 5 seconds gets the block
  rendering, also where its environment names a terminal with the
  iTerm2 protocol.
- When the query got no answer within its 5 seconds, or inside tmux
  with more than one client attached, whose terminals each answer,
  asqr discards what arrives on its input before it reads keys, until
  150 ms pass without a byte and for at most 1 second. Keys typed in
  that time are lost. An answer that begins later reaches asqr as typed
  keys, where its digits can pick options.
- An image is scaled to fill the pane as far as its proportions allow,
  a small one included, and so is the `z` view. An image entry with
  `enlarge: false` (section 3.3) keeps its natural size in both, and
  only shrinks when it does not fit.
- An image is scaled to its cells with a filter that mixes the pixels
  each one covers. An image the protocol fails to encode stays empty,
  and the log says why, once.
- An image entry is read when it is loaded and kept while it is inside
  the window (section 8.2): asqr keeps the file as it read it, not its
  decoded pixels, and decodes it again only when it is to show at
  another size in cells; a pane that changes size but leaves the
  image's cells as they were decodes nothing. Code, Markdown and diff
  entries keep what asqr derived from them (ADR 31).
- asqr asks the terminal for focus reports (section 7.1). Under the
  Kitty protocol behind a tmux pane with `allow-passthrough on` (section
  8.9), asqr loads every image a frame showed again behind `loading…`
  and sends it as it is next shown once its terminal regains the focus;
  an image loaded ahead and not shown yet has sent nothing. Elsewhere
  the terminal keeps what it got, and regaining the focus changes
  nothing.
- Closing the help, the intro or the session list over an image entry
  draws the whole screen afresh under Sixel and iTerm2, which leave the
  cells under the image to the terminal. Half blocks and the Kitty
  protocol fill those cells with every frame and need no full redraw.
  Under the Kitty protocol, an overlay hides only the cells of the
  image it covers, also for an image first sent while the overlay is
  open (ADR 48).
- Under the Kitty protocol, asqr asks the terminal to delete an image
  of an entry once the screen no longer needs it, after the next frame:
  when the image was sent again on regaining the focus or under a new id
  at another size, when its entry was replaced, or when its session
  left. Where regaining the focus sends the images again, it also writes
  the deletes since the last time again (section 8.9). On quitting, also
  after a panic, asqr deletes every image it sent and has not deleted
  yet, before it leaves the alternate screen (ADR 42).

### 8.9 tmux

- Inside tmux, inline images and OSC notifications need
  `set -g allow-passthrough all`. Without passthrough, asqr falls back
  to the block rendering and the bell. Notifications inside tmux go out
  in tmux's passthrough wrapper.
- With `on`, tmux passes sequences only while asqr's pane is visible: a
  Kitty image first shown while the pane is hidden stays empty until the
  pane gains the focus again, which asqr only learns when tmux forwards
  focus reports (`set -g focus-events on`). tmux with `on` also drops
  Kitty deletes while asqr's pane is hidden, so asqr writes them again
  on regaining the focus (section 8.8).
- Under the Kitty protocol inside tmux 3.4 or later, asqr sets
  `allow-passthrough all` for its own pane while it runs, so its images
  and their deletes pass from a hidden pane too (ADR 44). Sixel and
  iTerm2 images keep the pane's setting: from a hidden pane they would
  paint over the window that shows.
- Inside tmux, asqr asks tmux whether passthrough is on for its pane,
  whether a client is attached and whether a client shows the pane, and
  queries the terminal only when all three hold: otherwise the query
  gets no answer and holds up the start for its whole wait. A pane no
  client shows at start gets its images as blocks until asqr restarts.
  With more than one client attached, asqr discards late answers to the
  query (section 8.8).
- Without `TMUX`, when `TERM` starts with `tmux` or `TERM_PROGRAM` is
  `tmux`, as after ssh from a tmux pane, asqr cannot ask that tmux and
  queries the terminal through tmux's passthrough. Such a tmux counts as
  having passthrough `on` (section 8.8). Where that tmux passes nothing
  on, as from a pane with `on` that is hidden at start, the start waits
  the query's 5 seconds and images show as blocks.
- Whenever `TERM` starts with `tmux` or `TERM_PROGRAM` is `tmux`, the
  pane's `allow-passthrough` is set to `on` while asqr detects the
  protocol. asqr reads the pane's own value first (`tmux show-options
  -p`) and puts it back once detection is done, or unsets it again when
  the pane had none of its own. Where asqr sets `all` for Kitty images,
  it puts the value back when it quits instead, after it deleted its
  images. Without a tmux server, or with a tmux before 3.3 that has no
  such option, there is nothing to put back, and asqr sets no `all`.
- Known limitation: the pane's passthrough is on while the query runs,
  or `all` while asqr runs, and asqr killed in that window leaves it so.
- Talqi's glyphs inside tmux follow the client's terminal (section 7.5),
  and the terminal title goes to tmux plainly (section 7.9).

## 9. Agents

Agents are the main askers. asqr ships a skill for them (ADR 6).

The skill lives in the repo at `skills/asqr/SKILL.md` in the Agent Skills
format (`name` and `description` frontmatter, a plain Markdown body). It
is compiled into the binary, so `asqr skill` prints the version that
matches the installed asqr. `asqr skill --install <dir>` takes a skills
root and writes `<dir>/asqr/SKILL.md`, so `asqr skill --install
.claude/skills` works.

The skill teaches:

- that it is used only when the user explicitly asks the agent to use
  asqr; the skill's description says so, and an agent never switches to
  asqr on its own (ADR 6)
- once asked, when asqr fits better than a question in the chat
- `asqr paths`, `asqr new` and the format, with a short example
- the main waiting pattern, which works under any command time limit:
  `asqr ask`, then a loop of `asqr wait <id> --timeout <secs>` until the
  exit code is not 12
- as a variant for agents that run commands in the background: `asqr ask
  --wait` in the background
- the exit codes, the "no asqr is watching" warning and an error naming
  a context type the skill describes as not supported, both of which the
  agent should pass on to the person, and `asqr status --json` to find
  sessions it lost track of
- longer sessions: batches, follow-up rounds with `follows`, and keeping
  a record of the answers

## 10. Not in asqr yet

- Networking, several people on one queue, sockets, queues on network or
  synced filesystems.
- An MCP server (`asqr mcp`, stdio), a thin layer on the same library,
  offering `ask`, `wait_result` (waits up to N seconds, then returns the
  result or "pending") and `status`. MCP calls have time limits, so long
  answering times would be bridged with repeated `wait_result` calls.
- `asqr answer <file>`, answering one file in the current terminal
  without a queue.
- Partial submits.
- Question kinds beyond `single`, `multi` and `text`.
- A tmux passthrough hint in the TUI.
- Diffs side by side in a wide pane.
- Editing a field in `$EDITOR` with `ctrl-g`.
- Windows support (it may work, but it is not tested).
- Formats other than JSON.
- A config file: flags and environment variables only.
