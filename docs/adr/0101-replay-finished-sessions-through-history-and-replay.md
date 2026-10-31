# 101. Replay finished sessions through asqr history and asqr replay

Date: 2026-10-31

## Status

Accepted

Amends [97. Delete only through prune --older-than with long units, after a dry run](0097-delete-only-through-prune-older-than-with-long-units-after-a-dry-run.md):
`asqr prune --session <ref>` deletes one session, after the same dry run

## Context

- Archived sessions record the files they showed and keep copies of them
  (ADR 100), so a finished session can be shown again as the person saw
  it.
- Session ids repeat over time and across queues, and agents often choose
  them. The ULID in the archive name of an entry is unique.
- `asqr status` lists answered sessions by id and status only, and reads
  the results, not the archived sessions.

## Decision

### Two commands

`asqr history` prints the list of finished sessions and `asqr replay <ref>`
opens one entry in the terminal UI. `history` is plain output like
`status` and `queues`, while `replay` takes the terminal over, which a
verb says. A bare `asqr history` is the list.

### The list

- `asqr history` covers the queue every command resolves (`--queue`,
  `--dir`, `ASQR_QUEUE`, `ASQR_DIR`, tmux, default). `--all` lists every
  queue under the data directory, one block per queue, the queue in use
  first.
- `--limit` (default 10) and `--since` apply per queue and read like
  those of `status`.
- Columns: REF, WHEN, STATUS, FILES, TITLE. A withdrawn session whose
  result carries a draft shows `withdrawn*` and a footnote.
- `--json` prints the same list for scripts.
- The exit code is 0, 1 or 2. A result status shows in the row, never as
  10 to 14, which are the statuses of the waiting commands.
- A bundle that cannot be read is a warning on stderr and the other
  sessions are listed, as `status` does.

### Replay

- `asqr replay <ref>` takes the reference of the list, in any case, 6 to 8
  characters, and searches it in the default queue, every named queue
  under the data directory and the queue `--dir` names. A reference too
  short, too long or with a letter no ULID holds is a usage error. No
  match or several matches end with exit code 1, the latter listing the
  candidates. The replay never ends with 10 to 14.
- It takes no lock and writes nothing into the queue, so it runs beside
  the asqr that watches it. It reuses the run loop of the terminal UI with
  an inbox that only counts the sessions waiting in the queue, so no
  session arrives in it and no notification, bell or progress report comes
  from it. The terminal title is `asqr replay <REF>`.
- Loading happens before the terminal is taken, so an error that ends the
  replay prints on the normal screen with exit code 1.

### Pruning one session

- `asqr prune --session <ref>` takes the same reference as `replay`, not a
  session id: ids repeat and one delete must hit exactly one entry. It
  lists what it would remove and removes only with `--yes`, as
  `--older-than` does.
- A session lying both loose and in the bundle of its month is removed
  from both. The bundle is written anew without the session and without
  the kept files only it showed, in one hold of the drafts lock, and
  removed when nothing remains in it. A bundle that cannot be read is an
  error and nothing is removed. The result of the session stays.
- `prune --older-than` removes the loose kept files of the loose sessions
  it removes that no loose session that stays shows, and loose kept files
  no loose session shows whose own modification time is before the cutoff;
  a dry run lists them. A loose copy is needed by loose sessions only,
  since a bundle holds a copy of every file its sessions show.

### The reference

An entry is named by the last 8 characters of its archive ULID, which are
random bits, so two entries finished in the same second still differ.
Eight characters are 40 bits and six are 30, so two entries can end in the
same six characters; the commands that take a reference refuse an
ambiguous one and list the candidates. A reference is searched by the
file and bundle entry names alone, so resolving one reads no content.
The session id is not the address.

### Pairing a session with its result

A result answers a session when its `session_sha256` equals the hash the
archived session records in `archived.session_sha256` or the hash of the
file's own bytes. Candidates come from the outbox, from the results `ask
--force` moved aside and from the bundles. Sessions of one id with
identical bytes are told apart by time: the oldest session takes the
latest matching result finished at or before it was archived, and each
result answers one session.

### Loading an entry

- The session is the archived file, read from its loose place or from its
  month bundle. If the loose file has gone into the bundle meanwhile, it is
  read there. A session that does not parse or validate ends the replay
  with an error that names the file or bundle, and a bundle that cannot be
  read ends it with an error that names the bundle.
- The answers come from the paired result: those of a `submitted` or
  `cancelled` result, and the unsent `draft` of a `withdrawn` one. A
  question the person skipped shows no pick, also where it has a default.
  An `error` result and an entry without a result start empty.
- Each kept file the session shows is read from `archive/files/` or from
  the bundle, checked against the SHA-256 in its name and written once
  into the run directory that `o` uses for the files it hands to the
  opener (a directory per run, 0700, kept for a day after the run ends).
  The session's paths then point at those copies, so the replay and `o` do
  not depend on the archive afterwards. A kept file whose copy is missing,
  damaged or named other than `files/<64 hex>.<extension>` is not
  extracted and the replay records why. Only the files the session shows
  are read, never the whole bundle.
- A file kept under the extension `dat` is written out under the name it
  had, since the viewers pick a language from the name.
- Measured on a bundled session that kept the six largest images of the
  research corpus (31 MB of originals, 3 MB of WebP copies), loading took
  about 4 ms.

### Read-only

A replayed session starts on its first question. `space`, `1` to `9`, `n`, `backspace`, `L`, `w`, `,`, `ctrl-g`,
typing and paste do nothing, no field takes the keys, and no key asks the
app to save a draft. `enter` moves to the next question and, on the last
one, to the last tab, where it does nothing. Moving between questions and
rows, `tab`, `o`, `z`, `s`, `i`, `?`, `q` and `ctrl-c` keep their meaning.

### The look

- The title bar carries a permanent mark `◷ read-only` right after `asqr`,
  in the warning colour without a fill, and the right end names the queue
  and the reference. The title bar draws the mark itself, so no state of
  the screen hides it.
- The status line at rest says what the screen is, when the session ended
  and how, and what the colours mean. A notice replaces it for as long as
  it lasts and the title bar keeps the mark.
- Unpicked options are dim and the pick keeps its mark in the primary
  colour. The cursor still browses every option, since the option under it
  shows its own context. A withdrawn session's draft shows dim, with
  `◀ draft` on the option it picked.
- A file the archive could not read, did not keep or no longer holds a
  good copy of shows a card with the original path: red for a missing,
  unreadable or damaged file, a warning for a file that was not kept, with
  its size, time and fingerprint. The pane's bottom rule says what became
  of the file, and `d` opens a frame with the copy, the original path,
  size, time and SHA-256.
- A diff names two files, so the cards, the bottom rule's words and `d`
  do not apply to it; its kept sides show through the normal viewer.
- The last tab is `Result`. It says how the session ended, lists one line
  per answer and shows the note and, for a rejection, the reason.
- While sessions wait in the queue, the title bar counts them and the
  status line says where to answer. The replay does not notify and has no
  key that hands the session over.
