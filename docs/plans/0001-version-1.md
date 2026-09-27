# Plan: asqr version 1

A rough plan for building version 1, from `docs/spec-1.0.0.md` and ADRs 2-21.
It is a working document. It is removed from the repo again as the last
step, once version 1 is done. The open questions Q1-Q13 were settled
with the user on 2026-09-27 (ADRs 12-20). The adversarial review
findings F1-F12 of the same day are worked into the spec and noted here
where they change a step.

## Status

- Done: phases 0 to 6 and 5b (phase 6 ends with `cb2481d`).
- Next: bring every dependency to its newest version (user, after phase
  6), then phase 7, the skill.

## How every step runs

- Test first: write the test, watch it fail (red), write the feature,
  watch it pass (green), then the next test (ADR 11).
- `just check` stays green after every step: license notices, fmt,
  clippy with `-D warnings`, tests and doctests. Coverage runs with
  `just coverage` and in CI (ADR 21), and review checks that every
  written or changed line is covered.
- Terminal setup and teardown, and the terminal query for the image
  protocol, live in the binary (`src/terminal.rs`) and stay as thin as
  possible. The pseudo-terminal tests in `tests/tui_pty.rs` run them.
- Every source file carries the MPL 2.0 notice (ADR 10).
- Crates come in with `cargo add` when the first test needs them
  (ADR 9).
- One commit per finished feature, with its tests. Push at the end of
  each phase (ADR 18).

## Phase 0: groundwork

1. Done: GitHub Actions reviewed and hardened (ADR 19).
2. Done: `rust-version = "1.97"` (ADR 17).
3. The pipeline after review F7 and ADR 21: `just check` without
   coverage, but with `cargo test --doc`; `just coverage` separate;
   `just setup` checks for `just`, `cargo-llvm-cov` and
   `llvm-tools-preview` and names what is missing; the hook comment
   documents that it checks the working tree, not the index; CI runs
   `just check` and `just coverage`.
4. Split the crate into a library and a thin binary (`src/lib.rs` plus
   `src/main.rs`, ADR 16). Then push, and check that the hardened
   workflow runs green.

## Phase 1: the format (spec sections 4 and 5)

1. Session types with serde: `asqr`, optional `id`, `title`, `intro`,
   `from`, `follows`, `questions`; question `id`, `header`, `text`,
   `kind` (`single`, `multi`, `text`), `options` (`id`, `label`,
   `description`, `default`), `custom`, `length`, `note`, `required`,
   `image`. Parsing is lenient about unknown fields. Round-trip tests on
   example files.
2. Validation with `thiserror` errors naming the field, following the
   list in spec section 4: format version ("unsupported format
   version"), id syntax (no leading `.`, at most 200 bytes), `id` equal
   to the file stem, duplicate ids, options by kind, `custom` and
   `default` not on `text`, the default counts, `min`/`max`, `length`
   bounds.
3. Warnings: unknown fields (keys compared against the schema), relative
   image paths, missing image files.
4. Result and draft types (spec section 5): the statuses, `reason`,
   `error`, `submitted_at`, `session_sha256`, the draft with `current`,
   and the answer shape per kind.
5. The answer-state rules (spec section 5.2) as pure functions:
   answered or skipped per kind, `defaulted`, the effect of notes,
   `min`/`max`, `required`, whitespace-only custom text. Phase 4 builds
   on them.
6. The JSON Schema derived with `schemars`. A test fails when the
   checked-in files (`schema/session.v1.json`, `schema/result.v1.json`)
   differ from the derived ones.
7. Example sessions in `examples/`, validated in the tests.

## Phase 2: queues (spec section 3)

1. Queue resolution: the platform root through `directories`, `--queue`,
   `--dir` (the queue directory itself), `ASQR_QUEUE`, `ASQR_DIR`; flag
   over environment over default; `--queue` with `--dir` is a usage
   error.
2. The layout, created on first use.
3. Ids and names: the case-insensitive id comparison and lookup, the
   inbox file filter (valid stem, `.json`, everything else ignored),
   archive names `<id>.<ulid>.json` parsed from the end.
4. Listing waiting sessions by mtime, id as the tie-break.
5. Dropping a session (spec section 3.5): temp file with the `.tmp`
   suffix, `persist_noclobber` for a new id, `persist` to replace a
   session whose draft has no answer, refusing when the draft has an
   answer or an unread result exists, `--force` archiving the old
   result. Tests for two parallel drops with the same id.
6. Finishing a session (spec section 3.7): result with
   `session_sha256`, archive, delete the draft, each step idempotent;
   recovery on start by hash, and the conflict case.
7. Invalid files (spec section 3.8): error result, then archived; never
   over an existing result; invalid stems archived without a result.
8. The instance lock: `File::try_lock` on `<queue>/lock`, `WouldBlock`
   versus other errors, the holder text (ADR 14).
9. Prune: archive entries by ULID time, `--results` for the outbox,
   printing every removal.

## Phase 3: the command line (spec section 8, clap)

Each command gets `assert_cmd` tests against a temporary queue, with the
exit codes from spec section 8.

1. The exit code table as one type that every command maps into.
2. `asqr paths [--json]`, including the lock and the log file.
3. `asqr new`.
4. `asqr validate <file>` and `asqr schema`.
5. `asqr ask <file> [--wait] [--timeout] [--force]`: validation, ULID
   assignment, absolute image paths, the drop from phase 2.5, the "no
   asqr is watching" warning, the id on stderr with `--wait`.
6. `asqr wait <id> [--timeout]`, `asqr result <id>`, `asqr status
   [--json]` (waiting with draft yes or no, answered, lock holder).
7. `asqr prune --older-than <duration> [--results]`.
8. `asqr skill [--install <dir>]`, writing `<dir>/asqr/SKILL.md`, with a
   placeholder text until phase 7.
9. Logging with `tracing` into the platform cache directory (ADR 15).

## Phase 4: the TUI state (no terminal yet)

The whole interaction as a pure state machine, tested through key
events.

1. Session state on top of the phase 1 answer rules: current question,
   selection, custom text, notes.
2. Navigation: options (`j`/`k`, arrows), questions (`tab`,
   `shift-tab`, `J`, `K`), direct pick with `1`-`9`.
3. Selection rules: `single` replaces and clears custom text, `multi`
   toggles, defaults preselect, any edit clears `defaulted`.
4. Text fields with `ratatui-textarea`: `c` and `n` open them, every key goes
   to the editor except `esc`, which keeps the text; the counter with
   target, warn and max; input beyond `max` is refused.
5. Submit (`S`) and reject (`X`): the confirmation with the counts, the
   optional reason, `required` blocking submit.
6. Drafts: saved on every change, restored on start by question and
   option ids; replaced session files re-matched (review F1).
7. Several sessions: `L`, the list in queue order, switching.
8. `q` and `ctrl-c` quit and keep the draft.

## Phase 5: rendering

Tests through ratatui's `TestBackend` with `insta` snapshots.

1. The layout: header (with `follows`), the question list with states
   (including "default"), the current question, the key bar.
2. Options with long descriptions, wrapped in full.
3. The Markdown subset through `pulldown-cmark` with
   `into_offset_iter()`: supported elements styled, everything else as
   the raw source slice. Tests for `# x`, `> x`, `\*`, HTML, code blocks,
   and an unsupported element nested in a supported one.
4. Text fields with the text counters (`112/125`, `140/125 !`, `max`),
   colour only in addition.
5. The help screen (`?`), listing `shift-tab` and `K` together.
6. Images: the library takes a `Picker`; tests build one with
   `Picker::from_fontsize` and the halfblock protocol; the terminal query
   lives only in `main.rs`. Adaptive placement, `z`, `o`, and the
   placeholder for relative, missing or unreadable paths. A manual check
   in Ghostty is the one step that cannot be automated.
7. Small terminals: a minimum size and a message below it.
8. The conflict notice (spec section 3.7).

## Phase 5b: the interaction revised with the user (ADR 22)

After the first screens the user revised the interaction (tab bar,
answering in place, review tab; spec sections 7.1 to 7.4).

1. Format: drop `length.max` and `custom.multiline`; validation, schema,
   examples and the counter follow (`!!` above `warn`).
2. The session state reworked test first: rows including the own answer,
   live fields for the own answer, text answers and notes, `enter` picks
   and moves on, `←`/`→`/`h`/`l` between questions, the review tab with
   Submit and Reject.
3. The drawing reworked: tab bar, rows with inline fields, the review
   tab, scrolling that keeps the cursor row visible, dialogs that fit the
   screen; snapshots at several sizes (60×15, 80×24, 120×40, 200×50).

## Phase 6: the watcher and the running app

1. Watching the inbox with `notify` and a debouncer, registered before
   the initial scan; new, replaced and removed sessions reach the state.
2. Hand-dropped invalid files go through phase 2.7.
3. Notifications on a new session: OSC 9 or 777 and the bell,
   `--no-notify` and `--no-bell`.
4. The event loop in `main.rs`: taking the lock, the terminal query,
   terminal setup and teardown, restoring the terminal on panic.
5. README and spec note on tmux (`allow-passthrough on`). A tmux hint in
   the TUI stays a todo.

## Phase 7: the skill (ADR 6, spec section 9)

1. `skills/asqr/SKILL.md` in the Agent Skills format, teaching what spec
   section 9 lists: the bounded-wait loop as the main pattern,
   background `ask --wait` as the variant, the exit codes, the "no asqr
   is watching" warning, `status --json`, batches and `follows`.
2. Compiled into the binary with `include_str!`. A test checks that the
   embedded example validates.

## Phase 8: release

1. README rewritten from "planned" to what works, with the known
   limitations (tmux, network filesystems). A VHS demo GIF is deferred
   (`todos/01m3hnjjxavrc9tjwrtpptfnc0-vhs-demo-gif-for-readme.md`).
2. No publishing: asqr is used locally, installed from the repo. The
   user decides when it goes public and onto crates.io (ADR 20).
3. First real use: the torchsnap alt rework 3, with its findings fed
   back as issues.
4. Last step, once version 1 is complete: remove this plan and
   `docs/plans/` from the repo (user).
