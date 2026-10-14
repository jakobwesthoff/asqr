# 28. Session and result format version 2 with a context field

Date: 2026-10-14

## Status

Accepted

Supersedes [7. Session and result format version 1](0007-session-and-result-format-version-1.md)

Supersedes the format part of [25. Show an image per option](0025-show-an-image-per-option.md)

Amended by [29. Let the person add a note to the whole session](0029-let-the-person-add-a-note-to-the-whole-session.md)

Amended by [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [32. Let an image entry opt out of enlarging](0032-let-an-image-entry-opt-out-of-enlarging.md)

Amended by [33. Show diffs as a unified diff computed with similar](0033-show-diffs-as-a-unified-diff-computed-with-similar.md)

## Context

The user asked on 2026-10-14 for a context pane next to the question:
images as before, plus viewers for longer text (source code, Markdown)
and for diffs. The design was settled with the user in four asqr rounds
that day and is recorded in the plan todo "Split the TUI into a question
pane and a context pane". This ADR covers its first step, the file
format; the pane and its viewers follow in their own steps.

Format version 1 (ADR 7) attached material to a question or an option
only as an `image` path (ADR 25). Text and diffs need fields of their
own, and phase 2 of the plan shows several entries per question.

## Decision

Taken with the user:

- **Version 2:** session and result files move to `"asqr": 2`. asqr
  drops version 1 entirely.
- **Refusal:** a file with a lower version is refused with the message
  that this asqr no longer supports format version N and that `asqr
  skill` describes the current format. A version 1 session that waits
  in a queue when asqr is upgraded gets this as an error result (spec
  section 3.8), so the waiting agent sees exit code 11 and the hint.
- **`context`:** questions and options get a `context` field, a list
  limited to one entry. Phase 2 lifts the limit without a format change.
  The option under the cursor shows its context in place of the
  question's, as option images did (ADR 25).
- **Entries:** every entry has a `type` and an optional `title`.
  - `image`: `path`.
  - `text`: `path` or inline `text`, an optional `language`, and an
    optional `render: false` that shows Markdown as its source.
  - `diff`: `old` and `new`, each `{ "path" }` or `{ "text" }`, and an
    optional `name` and `language`. A missing `old` is a new file, a
    missing `new` a deleted one.
- **Diffs as two texts:** asqr computes the diff itself. An agent can
  always produce the two versions, while a hand-written unified diff
  goes wrong easily, as every hunk header needs exact line counts
  (proposed by the agent, confirmed by the user).
- **No `image` field:** it is folded into `context`. A version 2 file
  that still has one gets a warning saying that an image is a context
  entry of type `image`, since agents working from an older skill write
  exactly that.
- **Paths:** `asqr ask` makes every path absolute, as it did for images.
  `validate` and `ask` warn about relative paths, missing files and
  images asqr cannot show.
- **Size:** `validate` and `ask` warn about a text or diff entry above
  1 MiB, from a file or inline; asqr still shows it. Files are read
  when first shown. Image entries get no size warning, as screenshots
  above 1 MiB are common (user, 2026-10-14); the two sides of a diff
  count together.
- **Results:** the result keeps the fields of version 1 under
  `"asqr": 2`. A session note on the review tab, its own todo in the
  same release, is the first new field.
- **Old results:** results of version 1 that an earlier asqr left in an
  outbox are not refused. `wait` and `result` print them as they are, so
  an agent waiting across the upgrade gets its answers.
- **Schemas:** `schema/session.v1.json` and `schema/result.v1.json` are
  deleted; git history keeps them (user, 2026-10-14).
- **Spec:** `docs/spec-1.0.0.md` becomes `docs/spec.md`, the
  specification of the current asqr, with the format version named at
  its top; earlier versions are in git history (user, 2026-10-14).

Agent's choices within that design:

- The version is checked on the raw JSON before the file is parsed into
  a session. A version 1 file whose shape does not parse under version 2
  still gets the refusal with the hint, not a parser error.
- Until the text viewer lands, text entries show a placeholder that
  names them, and `o` and `z` act on image entries only. Diff entries
  show as a unified diff (ADR 33).

## Consequences

- Every asker has to write version 2. Agents learn it from `asqr skill`,
  which the refusal names.
- Sessions of version 1 still waiting at an upgrade come back as error
  results instead of being shown.
- The format carries text and diff entries before asqr can show them;
  their viewers need no further format change.
