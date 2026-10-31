---
title: "Keep the files a finished session showed, and replay finished sessions"
kind: feature
component: queue
origin: request
tags: [ux]
---
# Keep the files a finished session showed, and replay finished sessions

A session shows files by path: images, Markdown, code, ANSI and diff
sides. The archive kept the session file but not those files, and many
of them lived in temporary folders, so an archived answer such as "B" or
"the second mockup" meant nothing once the picture was gone. Requested by
Jakob on 2026-10-30 ("with AI sessions, more knowledge and history is
most of the times better than less"): copy the files into the archive
when a session finishes, point the archived session at the copies, and
give the archive a read-only viewer so it can be looked at.

## What is done and what remains

Keeping the files is done on the branch that carries this todo: the
processors and modes, the archived session that records each shown file,
the worker that keeps files after a finish, the month bundles that take
the kept files, and the `keep_files` setting with `ASQR_KEEP_FILES`.
Spec sections 5.1, 5.2, 5.7 and 5.9 and ADR 100 describe it.

What remains is the viewer: `asqr history`, `asqr replay <ref>`, the
read-only state of the TUI behind it, and `asqr prune --session <ref>`,
which also removes the loose kept files of the sessions that
`prune --older-than` deletes. This part must land before the next
release: keeping files without a viewer was rejected, and a build with
the first part alone would copy files nobody can look at.

## Decisions for what remains (Jakob, 2026-10-30, in the chat)

- **The replay is reached through the CLI first** (Jakob, 2026-10-30):
  how a replayed session looks and how one gets to it are two
  questions. In the first version there is no entry point in the TUI: a
  command lists what the archives hold, and a command replays one entry,
  opening the read-only view. The list covers the current queue,
  resolved as every command resolves it, and `--all` covers every queue
  under the data directory (with `--since` and `--limit` as `status`
  has them). Each row shows a short reference unique to the entry,
  taken from the ULID in its archive name, since session ids repeat
  across queues and over time; the replay command finds that reference
  in every queue under the data directory without a queue named (a
  `--dir` queue outside it is named). Rejected for now: a list or key in
  the TUI; a list over every queue by default; replay by session id
  within the current queue.
- **Commands and reference** (Jakob, 2026-10-30, after the design study
  in the conductor logbook, `plans/replay-design/`): `asqr history`
  prints the list, `asqr replay <ref>` opens one entry and takes the
  terminal over, which the verb says. The list's columns are REF, WHEN,
  STATUS, FILES and TITLE. The reference is the last 8 characters of the
  entry's archive ULID, its random part, so entries finished in the same
  second still differ; `replay` accepts any end of it of 6 or more
  characters in any case, and refuses one that is too short, matches
  nothing or ends two entries, listing the candidates. Rejected:
  `archive` and `show` (reads like `asqr compact` and the bundles);
  `history list` and `history show`; the start of the ULID as git does
  (the timestamp, shared by every entry of a day); accepting a unique
  session id as well.
- **The look is direction D of the study** (Jakob, 2026-10-30): the
  session screen with the answers filled in and nothing writable. The
  options the person did not pick are dim, the picked one keeps `(•)`.
  The status line carries `◷ replay · sent <time> · read-only` for the
  whole replay; a notice replaces it for a moment. The title bar names
  the queue and the reference. The cursor still moves over every option
  to show its context. The replay opens on the first question, and the
  last tab is `Result` (how it ended, every answer on a line). A new key
  `d` opens the details of a kept file (copy, original path, size, time,
  SHA-256). A missing file shows asqr's red card with its original path,
  a file not kept a card with size, time and fingerprint, and a withdrawn
  session its draft dimmed and marked `◀ draft`. Rejected: a strip row
  under the intro (A), alone or with the dimming; a bar down the left
  edge (B); opening on the `Result` tab (C).
- **A read-only mark that always shows** (Jakob, 2026-10-30): `◷
  read-only` in the warning colour, without a fill, right after `asqr`
  in the title bar. The title bar shows in every state of a session,
  the full-screen view and overlays included, and its left end is never
  cut, so a notice cannot hide the mark the way it hides the status
  line. The clock says "from the past" rather than "forbidden".
  Rejected: a filled badge there; the mark at the right end before the
  count; a badge in place of `asqr`; `⊘`; the word alone.
- **Arrivals during a replay** (lead, 2026-10-30): `asqr replay` holds
  no queue lock and writes nothing; it shows a count of waiting sessions
  in the title bar and says in the status line where to answer. It
  sends no notification and has no key to hand over to the live TUI in
  the first version.
- **Details left to the lead** (2026-10-30): `--limit` counts per queue
  under `--all`; an entry whose month bundle cannot be read ends the
  replay with an error naming the bundle and a non-zero exit. `--json`
  on `history` is in if cheap; search and the in-TUI history wait.
- **`asqr prune --session <ref>` removes one finished session on
  purpose** (Jakob, 2026-10-30; lead, 2026-10-31): a dry run unless
  `--yes` like every delete, it rewrites its month's bundle without the
  session and drops kept files no other session shows. It takes the REF
  that `history` prints and `replay` takes, never a session id, because
  ids repeat and a delete must hit exactly one entry. Rejected: a
  separate todo; whole months only.

## Technical notes (lead, 2026-10-30)

- The archived session points at each copy: its `path` names the copy
  inside the archive, and the original path stays beside it in
  `shown.original_path`, so the replay shows the copy and the person sees
  where it came from. Spec 5.1 has the format.
- The replay reads the month bundles directly, through the module
  `queue::bundle` (`list_entries`, `read_entry`, `newest_session`), which
  `status`, `result` and `withdraw` use too. A bundle is rewritten as
  `prepare_bundle`, `still_current` under the drafts lock and `commit`
  do (module docs of `src/queue/bundle.rs`); `prune --session` rewrites
  a bundle the same way.
- A result lies in the month it was finished in and its session in the
  month it was archived, which can be a later one, so the replay pairs a
  session with its result across bundles by `session_sha256` (the
  archived session's `archived.session_sha256`, or the hash of its own
  bytes), not by assuming one bundle holds both.
- A kept file is verified against its name (the SHA-256 of the kept
  bytes) before it is shown; a copy that does not match shows a red card.
