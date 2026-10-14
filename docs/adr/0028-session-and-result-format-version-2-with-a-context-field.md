# 28. Session and result format version 2 with a context field

Date: 2026-10-14

## Status

Accepted

Supersedes [7. Session and result format version 1](0007-session-and-result-format-version-1.md)

Supersedes the format part of [25. Show an image per option](0025-show-an-image-per-option.md)

Amends [12. Use ULIDs as session ids](0012-use-ulids-as-session-ids.md)

Amended by [29. Let the person add a note to the whole session](0029-let-the-person-add-a-note-to-the-whole-session.md)

Amended by [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [32. Let an image entry opt out of enlarging](0032-let-an-image-entry-opt-out-of-enlarging.md)

Amended by [33. Show diffs as a unified diff computed with similar](0033-show-diffs-as-a-unified-diff-computed-with-similar.md)

Amended by [34. Show code and Markdown entries through patine](0034-show-code-and-markdown-entries-through-patine.md)

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
    optional `name`. A missing `old` is a new file, a missing `new` a
    deleted one.
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
- `o` acts on image entries only. Diff entries show as a unified diff
  (ADR 33), code and Markdown entries through their viewers (ADR 34).

## Consequences

- Every asker has to write version 2. Agents learn it from `asqr skill`,
  which the refusal names.
- Sessions of version 1 still waiting at an upgrade come back as error
  results instead of being shown.
- The format carries text and diff entries before asqr can show them;
  their viewers need no further format change.

## Amendment (2026-10-14, code and Markdown entries)

Decided by the user before format version 2 was released, so the
version stays 2. Two entry types, each with a viewer of its own, replace
the `text` entry and its `render` flag (todo "Show code and Markdown
entries in the context pane"):

- **`code`:** `path` or inline `content`, an optional `language`.
  Shown exactly as written and highlighted, like a fenced code block.
- **`markdown`:** `path` or inline `content`. Rendered.
- **`content`:** the inline key `text` becomes `content` in every entry,
  the sides of a diff included.

The reasons, from the same decision:

- Each viewer does one job. The code viewer reads, highlights once,
  numbers its lines, cuts long lines and scrolls sideways; the Markdown
  viewer renders for the width and wraps. In one viewer these were two
  paths behind a flag, with different caching.
- No flag that only means something for one language. A `.md` file as
  a `code` entry shows its source, which is what reviewing the Markdown
  itself needs.
- `text` was overloaded: the question kind `text`, the question field
  `text` and the inline content key already used the word, so an agent
  could easily put an explanation that belongs in the question into a
  `text` entry. `code` matches the fenced code block agents write all
  the time (verbatim, monospace, a language tag), which is also where
  logs, terminal output and configuration files go. With `content`,
  "text" and "kind" each mean one thing in the format.

Agent's choices within that design:

- An entry of type `text` is refused before the rest of the file is
  parsed, as another format version is, with an error that names `code`
  and `markdown`. The parser would only report an unknown variant.
  Every such entry is listed. `text` is a natural guess for an agent
  that wants to show text, so the hint saves it a round trip.
- The keys `render` and an inline `text` are unknown fields like any
  other: format version 2 was never released with them, so no file
  carries them (user, 2026-10-14).
- Code entries show highlighted and Markdown entries rendered (ADR 34).

## Amendment (2026-10-14, review of the format)

An adversarial review of the unreleased version 2 found places where an
agent could write a file that asqr reads differently from what the agent
meant, or gets a message that does not say what to do. Agreed with the
user, the version stays 2:

- **Diff sides:** a diff with neither `old` nor `new`, or with `path` or
  `content` on the entry itself, is one validation error per entry. It
  says that a diff takes `old` and/or `new`, each `{"path": ...}` or
  `{"content": ...}`, the text before and after the change, and that
  asqr computes the diff itself and does not read a unified diff. An
  agent that has a unified diff at hand puts it there, and the
  unknown-field warning it got before did not say where the texts go.
- **Version:** an `asqr` value that is no unsigned integer, such as
  `2.0`, `"2"`, `-1` or `true`, is refused as "unsupported format
  version" with the value as written, checked before the shape like any
  other version. The parser's type error named neither.
- **Unified diff in a side:** `validate` and `ask` warn when a side,
  inline or in a file of at most 1 MiB, looks like a unified diff (a
  line starting with `@@ -` after one starting with `--- `, `+++ ` or
  `diff --git`). asqr would diff it like any other text and show a diff
  of the diff. Requiring a file header before the hunk header keeps a
  text that only mentions `@@` free of the warning.
- **Not a file:** a path that exists but names no regular file, such as
  a directory, gets the warning "not a file" instead of "file not
  found".
- **Schema rules:** the schemas state what a schema can: `asqr` is the
  constant 2 in the session and the result schema, `context` has
  `maxItems: 1`, and a code or Markdown entry or a diff side takes
  exactly one of `path` and `content` (a `oneOf` of two branches that
  each require one key, so an empty `content` counts as given and
  `context: []` stays valid). The descriptions of `intro`, question
  `text` and option `description` say that they are Markdown, that a
  single newline breaks the line and that raw HTML shows as written. An
  agent that checks its file against the schema learns these rules
  without reading the spec.
- **`asqr new` without an id:** the skeleton leaves out the session
  `id`, as the skill tells agents to, and `asqr ask` assigns one. This
  amends ADR 12, under which `asqr new` printed a fresh ULID.
