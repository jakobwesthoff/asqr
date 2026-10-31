# 103. Let agents read finished sessions with replay --json and --extract

Date: 2026-10-31

## Status

Accepted

Builds on [101. Replay finished sessions through asqr history and asqr replay](0101-replay-finished-sessions-through-history-and-replay.md):
`asqr replay` gains two flags that print or write the session instead of
opening it

## Context

- `asqr history --json` prints per entry the reference, id, finish time,
  status, title, draft flag and the counts of kept, gone and not kept
  files. It prints no content and no place.
- `asqr replay <ref>` opened the terminal UI only, so an agent had no way
  through asqr to a finished session's questions, its result or its files.
- After 30 days a session, its result and its kept files lie in a month
  bundle (ADR 96, ADR 100). Reading the archive directly would tie an
  agent to the bundle format.

## Decision

### Content, not paths

asqr hands out the content, so the format of the archive stays internal.
Printing the loose path, or the bundle and entry name, for an agent to
read itself was rejected.

### Two flags

`asqr replay <ref> [--json] [--extract <dir>]`. Both are optional and
combine. With either, no terminal UI starts, no terminal is needed, and
the run directory for opened files is not created. The reference is
resolved and refused as in ADR 101, with the same messages.

Exit codes are 0, 1 and 2, never 10 to 14. On 1 or 2 stdout is empty and
the reason is on stderr, also with `--json`. `--extract ""` is a usage
error.

Warnings go to stderr as `warning: <error>`: the bundles the reference
search could not read, as in ADR 101, and the bundles the pairing of the
session with its result could not read, so an unreadable result does not
read as "no result". A bundle both name is warned about once.

### `--json`

One object on one line. `ref`, `id`, `finished_at`, `status` and `draft`
are the words and the whole-second time that `history --json` uses. `queue`
is the queue the entry was found in, by its name, or for a queue given with
`--dir` by the absolute path of its directory.

- `session` is the archived session as stored, fields unknown to this
  asqr included, with the `path` of every file it shows set to the path
  the asker wrote, so no archive name appears.
- `result` is the paired result as asqr reads it, which leaves out fields
  asqr does not know and options without a value, or `null` when no result
  answers the session. A withdrawn result carries its `draft`.
- `files` has one object for each distinct pair of original path and
  SHA-256, sorted by that pair, so a file shown at two places is one
  object, and identical bytes at two original paths are two. `state` is
  `kept`, `gone` (recorded missing or unreadable, with the recorded text
  in `reason`), `not_kept` or `not_recorded`. `problem` says why the copy
  of a kept file cannot be handed out, and is `null` otherwise.
  `extracted` is the path of the copy relative to `extracted_to`, only
  with `--extract` and a good copy. When one original and fingerprint is
  shown at two places with two different kept copies, it names the first.
- `extracted_to` is the absolute directory with `--extract`, else `null`.
- No file bytes are printed.

### `--extract <dir>`

```
<dir>/
  session.json           the archived session as stored
  result.json            the paired result as in --json; absent without a result
  files/<sha256>.<ext>   each good kept copy, once
```

Names come from content only. The kept `path`s in `session.json` are
`files/<sha256>.<ext>` and resolve below `<dir>`; the original names are in
`session.json` as `shown.original_path` and in `files` of `--json`.

- The session loads and validates, and every copy is read and checked
  against its hash, before anything is written. A session that does not
  parse or validate leaves an absent `<dir>` absent and an empty one empty.
- `<dir>` must be absent or empty. A hidden file makes it non-empty.
  asqr refuses a symbolic link at `<dir>`, a path that is not a directory,
  and a `<dir>` that lies inside the data directory, the selected queue's
  directory or the directory of the queue the session was found in,
  judged by where the path really lies. There is no `--force`.
- The files go into a staging directory `.<name>.asqr-<ulid>` next to
  `<dir>`, created with mode 0700, with the files at 0600, and the staging
  directory is renamed onto `<dir>` when all is written. A failure removes
  the staging directory and the parents this run created. A race on an
  existing empty `<dir>` between the check and the rename is accepted.
- Without `--json` stdout lists `extracted to <dir>`, `session.json`,
  `result.json` or `no result`, the number of files in `files/`, and one
  line `<original path>: <state or problem>` for each shown file without a
  good copy. Original paths and states pass through the same control
  character replacement as titles in `history`. Such files do not change
  the exit code.
- With `--json`, stdout holds the JSON object only.

### The skill

The core of the skill names `asqr history --json`, `asqr replay <ref>
--json` and `asqr replay <ref> --extract <new dir>` in the bullet that
tells agents to keep a record of the answers they act on.
