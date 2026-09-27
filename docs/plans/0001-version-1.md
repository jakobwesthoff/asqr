# Plan: asqr version 1

A rough plan for building version 1, from `docs/spec.md` and ADRs 2-11.
It is a working document. It is removed from the repo again as the last
step, once version 1 is done. The open questions Q1-Q13 were settled
with the user on 2026-09-27; each answer is noted where it applies and
recorded in ADRs 12-20.

## How every step runs

- Test first: write the test, watch it fail (red), write the feature,
  watch it pass (green), then the next test (ADR 11).
- `just check` stays green after every step: license notices, fmt,
  clippy with `-D warnings`, tests, coverage report.
- Every written or changed line is covered by tests. Terminal setup and
  teardown are the only exception and stay as thin as possible.
- Every source file carries the MPL 2.0 notice (ADR 10).
- Crates come in with `cargo add` when the first test needs them
  (ADR 9).
- One commit per finished feature, with its tests (Q1). Push at the end
  of each phase (Q2). ADR 18.

## Phase 0: groundwork

1. Done: GitHub Actions reviewed and hardened (SHA pins, permissions,
   concurrency, `persist-credentials: false`, Dependabot), ADR 19.
2. Split the crate into a library and a thin binary (`src/lib.rs` plus
   `src/main.rs`). Tests then reach the logic without the terminal, and
   the later MCP server can reuse the library. One crate, not a
   workspace (Q3, ADR 16).
3. Done: `rust-version = "1.97"` (Q4, ADR 17).

## Phase 1: the format (spec sections 4-6, ADR 7)

1. Session types with serde: session (optional `id`), question, the kinds `single`,
   `multi` and `text`, options, `custom`, `length`, `note`, `required`,
   `image`. Round-trip tests on example files.
2. Validation with `thiserror` errors that name the field: version,
   id syntax, unique question and option ids, options present for
   `single`/`multi` and absent for `text`, `min`/`max` consistent,
   `length` ranges consistent, `default` only once for `single`.
3. Warnings (not errors): relative image paths, missing image files.
4. Result types: `submitted`, `cancelled`, `error`, every question once
   in session order, `skipped`.
5. The JSON Schema derived with `schemars`; a test fails when the
   checked-in schema files (`schema/session.v1.json`,
   `schema/result.v1.json`) differ from the derived ones.
6. Example sessions in `examples/`, validated in the tests.

## Phase 2: queues and paths (spec section 3, ADR 4 and 5)

1. Queue root resolution: the platform default through `directories`,
   `--queue`, `--dir`, `ASQR_QUEUE`, `ASQR_DIR`, with a defined
   precedence (flag over environment over default).
2. Queue layout: create `inbox/`, `outbox/`, `drafts/` and `archive/` on
   first use.
3. Atomic writes through `tempfile` in the target directory.
4. Listing waiting sessions oldest first, ignoring `*.tmp`.
5. Replacement rule: a session with an existing id replaces the waiting
   one if nothing is answered yet, otherwise an error result. An id
   whose answered result still sits in the outbox is rejected unless
   `--force`, which archives the old result (Q5, ADR 12).
8. The queue lock: one instance per queue, stale locks taken over (Q10,
   ADR 14).
6. Submit: write the result, then move the session into the archive.
7. Prune: remove archived sessions and their results older than a
   duration.

## Phase 3: the command line (spec section 8, clap)

Each command gets `assert_cmd` tests against a temporary queue.

1. `asqr paths [--json]`, including the log file.
2. `asqr new`: a session skeleton with a fresh ULID (ADR 12).
3. `asqr validate <file>` and `asqr schema`.
4. `asqr ask <file> [--wait] [--timeout] [--force]`: validate, assign a
   ULID when the id is missing, make image paths absolute, drop
   atomically, print the id; with `--wait` block and print the result,
   exit codes 0, 1 and 2.
5. `asqr wait <id>`, `asqr result <id>`, `asqr status`.
6. `asqr prune --older-than <duration>`.
7. `asqr skill [--install <dir>]` (the skill text itself comes in
   phase 7; the command ships with a placeholder test first).
8. Logging with `tracing` into a file in the platform cache directory
   (Q6, ADR 15).

## Phase 4: the TUI state (no terminal yet)

The whole interaction as a pure state machine, tested through key
events.

1. Session state: current question, the selection per question, custom
   text, notes, skipped.
2. Navigation: options (`j`/`k`, arrows), questions (`tab`,
   `shift-tab`, `J`, `K`), direct pick with `1`-`9`.
3. Selection rules: `single` replaces, `multi` toggles within
   `min`/`max`, `default` preselects.
4. Text fields with `tui-textarea`: custom entry (`c`), note (`n`),
   `esc` leaves, the length counter with target, warn and max; input
   beyond `max` is refused.
5. Submit (`S`): a summary of skipped questions first, `required`
   questions block it.
6. Drafts: saved on every change, restored on start.
7. Several sessions: the session list and switching between them.
8. Rejecting a session: `X`, a confirmation, an optional reason, status
   `cancelled` (Q7, ADR 13).

## Phase 5: rendering

Tests through ratatui's `TestBackend` with `insta` snapshots.

1. The layout: header, the question list with states, the current
   question, the key bar.
2. Options with long descriptions, wrapped in full.
3. The Markdown subset: bold, italic, inline code, lists, line breaks,
   parsed with `pulldown-cmark`, rendering only the allowed elements
   (Q8).
4. Text fields with their counters and colours.
5. The help screen (`?`).
6. Images through `ratatui-image`: the adaptive placement (a column when
   wide, below when narrow), `z` full screen, `o` opens the system
   viewer. Snapshot tests use the block fallback, and the protocol
   detection is tested separately. A manual check in Ghostty, as the
   one step that cannot be automated.
7. Small terminals: a minimum size and a message below it.

## Phase 6: the watcher and the running app

1. Watching the inbox with `notify` and a debouncer: new, replaced and
   removed sessions reach the state.
2. Invalid files dropped by hand get an error result.
3. Notifications on a new session: OSC 9 or 777 and the bell, with the
   flags to switch them off. Flags and environment only, no config file
   (Q9, ADR 15).
4. The event loop: terminal setup and teardown, and restoring the
   terminal on panic.
5. Taking the queue lock on start (phase 2, step 8).

## Phase 7: the skill (ADR 6)

1. `skills/asqr/SKILL.md`: when to use asqr, `asqr paths`, the format
   with a short example, `asqr ask --wait` in the background, waiting
   without blocking, batches and follow-up rounds (`follows`), keeping a
   record.
2. Compiled into the binary with `include_str!`. A test checks that the
   embedded example validates.
3. Format: the Agent Skills `SKILL.md` with `name` and `description`
   frontmatter and a plain Markdown body (Q11).

## Phase 8: release

1. README rewritten from "planned" to what works. A VHS demo GIF is
   deferred (Q12, `todos/01m3hnjjxavrc9tjwrtpptfnc0-vhs-demo-gif-for-readme.md`).
2. No publishing: asqr is used locally, installed from the repo. The
   user decides when it goes public and onto crates.io (Q13, ADR 20).
3. First real use: the torchsnap alt rework 3, with its findings fed
   back as issues.
4. Last step, once version 1 is complete: remove this plan and
   `docs/plans/` from the repo (user).
