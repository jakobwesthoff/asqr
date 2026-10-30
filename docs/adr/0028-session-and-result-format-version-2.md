# 28. Session and result format version 2

Date: 2026-10-14

## Status

Accepted

Amended by [29. Let the person add a note to the whole session](0029-let-the-person-add-a-note-to-the-whole-session.md)

Amended by [32. Let an image entry opt out of enlarging](0032-let-an-image-entry-opt-out-of-enlarging.md)

Amended by [51. Add context entry types within format version 2](0051-add-context-entry-types-within-format-version-2.md)

Amended by [56. Show text with ANSI colours as a context entry](0056-show-text-with-ansi-colours-as-a-context-entry.md)

Amended by [60. Highlight a diff's unchanged lines and fill its changed ones](0060-highlight-a-diffs-unchanged-lines-and-fill-its-changed-ones.md)

Amended by [61. Add question kinds within format version 2](0061-add-question-kinds-within-format-version-2.md)

Amended by [62. Show text and context without asking through an info item](0062-show-text-and-context-without-asking-through-an-info-item.md)

Amended by [64. Let the asker withdraw a session](0064-let-the-asker-withdraw-a-session.md)

Amended by [68. Every item takes a note](0068-every-item-takes-a-note.md)

Amended by [69. Show a Markdown caption with an image entry](0069-show-a-markdown-caption-with-an-image-entry.md)

Amended by [71. Let backspace clear the pick of a single question](0071-let-backspace-clear-the-pick-of-a-single-question.md)

Amended by [75. Cut a text entry after 500,000 lines](0075-cut-a-text-entry-after-500000-lines.md)

Amended by [77. Count a cleared default as the person having started](0077-count-a-cleared-default-as-the-person-having-started.md)

## Context

The file format is what other tools build on, so it has to be small,
versioned and checkable.

A context pane next to the question shows material the person needs to
answer: images, source code, Markdown and diffs. Choosing between
variants, such as logo variants, needs that material per option, so
that a person can go through the variants in one question and the pick
is its answer.

## Decision

The format is JSON. The full description is in `docs/spec.md` sections
3 and 4, the specification of the current asqr, with the format version
named at its top.

Version:

- Session and result files carry `"asqr": 2`. asqr has no other format
  version.
- A file with a lower version is refused with the message that this asqr
  no longer supports format version N and that `asqr skill` describes
  the current format. A version 1 session that waits in a queue when
  asqr is upgraded gets this as an error result (spec section 5.8), so
  the waiting agent sees exit code 11 and the hint.
- An `asqr` value that is no unsigned integer, such as `2.0`, `"2"`, `-1`
  or `true`, is refused as "unsupported format version" with the value
  as written. A parser type error names neither.
- The version is checked on the raw JSON before the file is parsed into
  a session. A version 1 file whose shape does not parse under version 2
  still gets the refusal with the hint, not a parser error.
- Results of version 1 that an earlier asqr left in an outbox are not
  refused. `wait` and `result` print them as they are, so an agent
  waiting across the upgrade gets its answers.
- Unknown fields are ignored when parsing and reported as warnings.

Questions:

- Question kinds are `single`, `multi` (with optional `min` and `max`)
  and `text`. Other kinds such as `confirm`, `rank` and `number` are not
  part of the format.
- Options can be any number, each with a label and an optional long
  description.
- Optional per question: a custom entry (the own answer), one note, a
  length (`target`, `warn`) that guides with a live counter, and
  context.
- Every question is optional. There is no `required`, and submit is
  never blocked; skipped questions come back as `skipped: true` with
  their notes. A file that sets `required` parses, and `validate` and
  `ask` report the field as unknown. The same holds for `length.max`
  and `custom.multiline`: lengths only guide, and the own answer is a
  typed field like every other (ADR 36).
- `min` and `max` count the picked options only, and the own answer of a
  `multi` comes on top.
- The intro, question texts and option descriptions are Markdown
  (ADR 35).
- `follows` is only shown in the header.
- Ids follow ADR 12.

Context:

- Questions and options have a `context` field, a list limited to one
  entry. The list form leaves room for more entries without a format
  change.
- The option under the cursor shows its context in place of the
  question's; every other row, and an option without context, shows the
  question's.
- Every entry has a `type` and an optional `title`:
  - `image`: `path`, and `enlarge` (ADR 32).
  - `code`: `path` or inline `content`, an optional `language`. Shown
    exactly as written and highlighted, like a fenced code block.
  - `markdown`: `path` or inline `content`. Rendered.
  - `diff`: `old` and `new`, each `{ "path" }` or `{ "content" }`, and an
    optional `name`. A missing `old` is a new file, a missing `new` a
    deleted one.
- Code and Markdown are two types, each with a viewer of its own:
  - Each viewer does one job. The code viewer reads, highlights once,
    numbers its lines, cuts long lines and scrolls sideways; the
    Markdown viewer renders for the width and wraps.
  - No flag that only means something for one language. A `.md` file as
    a `code` entry shows its source, which is what reviewing the
    Markdown itself needs.
  - The question kind `text`, the question field `text` and the inline
    content key would all use the word "text", so an agent could easily
    put an explanation that belongs in the question into a `text` entry.
    `code` matches the fenced code block agents write all the time
    (verbatim, monospace, a language tag), which is also where logs,
    terminal output and configuration files go. With `content`, "text"
    and "kind" each mean one thing in the format.
- An entry of type `text` is refused before the rest of the file is
  parsed, as another format version is, with an error that names `code`
  and `markdown`. The parser would only report an unknown variant, and
  `text` is a natural guess for an agent that wants to show text. Every
  such entry is listed.
- There is no `image` field on questions or options. A file that still
  has one gets a warning saying that an image is a context entry of type
  `image`, since agents working from an older skill write exactly that.

Diffs:

- A diff entry carries two texts, and asqr computes the diff itself. An
  agent can always produce the two versions, while a hand-written
  unified diff goes wrong easily, as every hunk header needs exact line
  counts.
- A diff with neither `old` nor `new`, or with `path` or `content` on
  the entry itself, is one validation error per entry. It says that a
  diff takes `old` and/or `new`, each `{"path": ...}` or
  `{"content": ...}`, the text before and after the change, and that
  asqr computes the diff itself and does not read a unified diff. An
  agent that has a unified diff at hand puts it there, and an
  unknown-field warning does not say where the texts go.
- `validate` and `ask` warn when a side, inline or in a file of at most
  1 MiB, looks like a unified diff: a line that is a full hunk header
  (`@@ -N[,N] +N[,N] @@`), or a line starting with `@@ -` after one
  starting with `--- `, `+++ ` or `diff --git`. asqr would diff it like
  any other text and show a diff of the diff. The full hunk header alone
  catches a hunk copied without its file headers; a text that only
  mentions `@@` inside a line gets no warning.

Paths and sizes:

- `asqr ask` makes every path absolute. Files dropped into the inbox by
  hand must use absolute paths.
- `validate` and `ask` warn about relative paths, missing files, a path
  that names no regular file ("not a file"), and images asqr cannot
  show.
- `validate` and `ask` warn about a code, Markdown or diff entry above
  1 MiB, from a file or inline; asqr still shows it. The two sides of a
  diff count together. Image entries get no size warning, as
  screenshots above 1 MiB are common. Files are read when their entry
  loads, shown or ahead (ADR 46).

Results and drafts:

- There is one result per session, written when the session is
  submitted, rejected or answered with an error, with the status
  `submitted`, `cancelled` or `error`. There are no partial submits.
- Results carry `session_sha256` over the session file's bytes.
- A submitted result carries the session note (ADR 29).
- Answer state (spec section 4.2): a default counts as answered, and the
  answer carries `defaulted: true` until the person edits the question.
  A question's note never answers the question. `min` and `max` only apply once
  something is selected. Custom text that is empty after trimming does
  not answer the question. The answer shape per kind is fixed.
- Drafts use the result format with `status: "draft"` and `current`.

Schema:

- A JSON Schema is derived from the Rust types. `asqr schema` prints it
  and `asqr validate` checks a file.
- The schemas state what a schema can: `asqr` is the constant 2 in the
  session and the result schema, `context` has `maxItems: 1`, and a code
  or Markdown entry or a diff side takes exactly one of `path` and
  `content` (a `oneOf` of two branches that each require one key as a
  string, so an empty `content` counts as given, a `null` counts as
  missing as it does for asqr, and `context: []` stays valid). A diff
  takes `old`, `new` or both (an `anyOf` of two branches that each
  require one side as an object). The descriptions of `intro`, question
  `text` and option `description` say that they are Markdown, that a
  single newline breaks the line and that raw HTML shows as written. An
  agent that checks its file against the schema learns these rules
  without reading the spec.

## Consequences

- Every asker writes version 2. Agents learn it from `asqr skill`, which
  the refusal names.
- Sessions of version 1 still waiting at an upgrade come back as error
  results instead of being shown.
- An asker cannot force an answer or enforce a length; it handles
  skipped questions.
- Adding kinds or fields later needs a new format version or optional
  fields that readers ignore when unknown.
