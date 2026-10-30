---
name: asqr
description: Ask the person you work with many questions at once through asqr, a terminal inbox for questions, and get their answers back as JSON. Only use it when the user explicitly asks you to use asqr for something; never switch to it on your own.
---

# asqr

asqr (pronounced "asker") lets you hand the person a whole batch of
questions and get the answers back as a file. You write a session file,
drop it into a queue with `asqr ask`, and the person answers it in a
terminal of their own where `asqr` runs. When they submit, asqr writes a
result file that you read back.

## When to use it

Only when the user explicitly asks you to use asqr, for example "ask me
through asqr" or "put the questions into asqr". Without such a request,
ask the way you normally do, even when you have many questions; you may
mention that asqr exists, but do not switch to it on your own.

Once asked, asqr fits best when:

- there are more than a handful of questions, or more options per
  question than a chat question offers
- each question needs context to decide: a longer description per
  option, an image, a file, a diff, a suggested default
- the person should be able to skip, add a note, or type their own
  answer
- the answers feed straight into your next step as data

## The session file

```json
{
  "asqr": 2,
  "title": "Release 0.3.0",
  "intro": "The checks so far, then three decisions before the release. Skip what you **cannot** decide yet.",
  "from": "your-agent, project name",
  "questions": [
    {
      "id": "checks",
      "header": "Checks",
      "text": "All checks passed. Two flaky tests passed on a retry:\n\n- `fetch::retries_on_timeout`\n- `queue::drops_in_order`",
      "kind": "info"
    },
    {
      "id": "go",
      "header": "Go?",
      "text": "Publish 0.3.0 now?",
      "kind": "single",
      "context": [
        { "type": "markdown", "title": "CHANGELOG.md", "content": "## 0.3.0\n\n- Faster startup\n- A `--quiet` flag\n" }
      ],
      "options": [
        { "id": "yes", "label": "Publish now" },
        { "id": "later", "label": "Later", "description": "Keep the tag, publish after the weekend." }
      ]
    },
    {
      "id": "channels",
      "header": "Channels",
      "text": "Where should the release be announced?",
      "kind": "multi",
      "max": 2,
      "options": [
        { "id": "changelog", "label": "Changelog", "default": true },
        { "id": "blog", "label": "Blog post" }
      ],
      "custom": { "label": "Somewhere else" }
    },
    {
      "id": "highlight",
      "header": "Highlight",
      "text": "One sentence for the release notes:",
      "kind": "text",
      "length": { "target": [40, 120], "warn": 160 }
    }
  ]
}
```

- `kind` is `single` (pick one), `multi` (pick several, optional `min`
  and `max`), `text` (typed answer, no options) or `info` (nothing to
  answer).
- An `info` item shows its `text` and `context` like a question and asks
  nothing: it takes no `options`, `custom` or `length`, and the person
  may add a note to it as to any question.
- A session needs no info item at all. Before you add one, check
  whether what it would say fits into the `text` or `context` of the
  question it leads to: there the person reads it where they decide,
  and the question is asked directly. Use an info item only for what
  belongs to no single question, such as results so far or a problem
  you ran into, and never a made-up question with options like "OK"
  instead.
- Keep an info item's text to a few lines: on a small terminal a longer
  text shows a few lines at a time while the person scrolls through it
  with `↑`. Split more material into several short info items, one
  topic each, and place each just before the questions it bears on, so
  the person works through the session piece by piece instead of taking
  in everything at once. A compact list or a small table per item reads
  at a glance; a small table can also go into a `markdown` entry of the
  item's `context`, which scrolls and shows full screen with `z`. Give a
  long document to read in one piece only when the person asks for one.
  A session may hold only info items, as a report the person can answer
  with notes.
- `custom: true`, or `{ "label": ..., "length": ... }`, adds an own
  answer to a `single` or `multi` question.
- `length` gives a target range and a `warn` limit for typed text. It
  guides with a counter; nothing the person types is refused.
- Do not set `note`; it has no effect. The person can add a note to
  every question and info item: a condition on an answer, why they
  skipped, or that a question is wrong. Read every note before you act
  on the answers.
- Every question is optional: the person may skip any of them, one
  with a `default` included, and a skipped question may still carry a
  note explaining why.
- `min` and `max` count the picked options; the own answer of a `multi`
  comes on top of them.
- `default: true` on an option marks it as your recommendation and
  preselects it. The person sees `◀ default` at the end of the option's
  first line, also after picking another option, so they can tell your
  recommendation from their own pick. The default is the answer when
  they leave the question as it is. They may keep it, pick another
  option, or clear it and skip the question.
- An option's `label` is the short handle the person picks the option
  by, such as `Publish now`. It wraps and is never cut. Keep it to a few
  words, and put longer text into the option's `description`, or into
  its `context` when it is material to read.
- `context` holds material that helps to decide: a list with one entry,
  since asqr shows one per question or option. The person sees it in a
  pane of its own, beside the question on a wide terminal and below it
  on a tall one, under a header line with the entry's `title` (or its
  file name). The pane scrolls on its own, and `z` shows the entry full
  screen; a terminal too small for the pane shows one line naming the
  entry, and `z` opens it. So put long material, such as a file, a log,
  a diff or a screenshot, into `context` and keep the question `text` to
  what the person reads to decide. Give each entry a `title` that says
  what it is. An option can have its own `context`, shown instead while
  the cursor is on that option, so the person can flip through variants
  in one question and pick one. Every entry has a `type` and an
  optional `title`:
  - `{ "type": "image", "path": "..." }`: an image file. asqr shows PNG,
    JPEG, GIF (first frame only), WebP, BMP, TIFF, ICO, TGA, PNM, QOI,
    DDS, OpenEXR, HDR and farbfeld. It cannot show SVG or AVIF, so
    convert those to PNG first. The person only sees an error card with
    the reason for an image asqr cannot show, and `validate` and `ask`
    warn about it. asqr scales an image up to fill the pane, apart from
    the rows a caption takes, which suits most images: the person sees
    them larger when the space is there. Add `"enlarge": false` only when
    the exact pixel size matters, such as pixel art, a screenshot whose
    size is the point, or an icon at its real size; the image then keeps
    its natural size and only shrinks when it does not fit.
    Add `"caption"` with one short line of Markdown, two at most, that
    says only what matters about the image, such as `"caption": "Draft
    3: warmer palette, thicker outline"`. The person sees it under the
    image, which takes the rows the caption leaves, so a longer caption
    makes the image smaller, and one that leaves it too little room
    hides it from the pane until the person presses `z`. Do not put a
    prompt paragraph or a list into a caption: longer material, such as
    the full prompt or a list of notes, goes into the question's `text`
    or another context entry. `z` shows the image alone where the pane
    shows. A caption follows the rules of a Markdown entry (a single
    newline joins the lines). Other entry types ignore a caption, and
    `validate` and `ask` warn about it as an unknown field.
  - `{ "type": "code", "path": "..." }` or `{ "type": "code", "content":
    "..." }`: text shown exactly as written and highlighted, like a
    fenced code block: source code, a log, terminal output, a
    configuration file. `language`, such as `rust`, picks the
    highlighting; without it the extension of `path`, then of `title`,
    decides. A Markdown file as `code` shows its source, which is what
    you want when the Markdown itself is under review.
  - `{ "type": "markdown", "path": "..." }` or `{ "type": "markdown",
    "content": "..." }`: Markdown the person reads rendered, such as
    notes, a plan or a draft section of documentation. A Markdown entry
    follows CommonMark, unlike the question text: a single newline joins
    the lines (a blank line starts a paragraph, a trailing backslash
    breaks the line), and raw HTML is left out, so put `<` into code
    spans, such as `` `Vec<String>` ``.
  - `{ "type": "ansi", "path": "..." }` or `{ "type": "ansi",
    "content": "..." }`: text in the colours its own escape sequences
    set, such as the output of a command run with its colours or a
    mockup of a terminal screen. asqr shows the colours, bold, dim,
    italic, underline, reversed and crossed out that SGR sequences
    (`ESC [ … m`) set, and drops every other escape sequence. Cursor
    movement is dropped too: the output of a full-screen program, which
    places its text by moving the cursor, does not show as on its
    screen; capture a command that prints line by line. Ask your tool
    for its colours, as `git diff --color=always` does. In JSON, write
    the escape character as `\u001b`, as in
    `"\u001b[31mfailed\u001b[0m"`; a file at `path` holds the raw bytes.
    The named colours and the first 16 of the 256 (`30` to `37`, `90` to
    `97`, `38;5;0` to `38;5;15`, and their backgrounds) follow the
    person's terminal theme; colours 16 to 255 and RGB colours show as
    given, so draw a mockup in the named colours. Under `NO_COLOR` the
    person sees the text without any colour. Long lines do not wrap, and
    there are no line numbers.
  - Choose by what the person should see: the text as written is
    `code`, rendered prose is `markdown`, text in the colours you or your
    tool chose is `ansi`. None of them is the place for your explanation
    of the question; that goes into the question's `text`.
  - Inline text goes into `content` in every entry, the sides of a diff
    included. The key `text` belongs to questions only.
  - asqr shows U+10EEEE, the character the Kitty graphics protocol uses
    to place images, as `�` in every text, so it cannot place an image
    on the person's screen.
  - `{ "type": "diff", "old": {...}, "new": {...} }`: a change, which asqr
    diffs itself. `old` is the text before the change and `new` the text
    after it, each `{ "path": "..." }` or `{ "content": "..." }`: two
    versions of the same text, never a unified diff of your own, which
    asqr would show as a changed file of `+` and `-` lines. `validate`
    and `ask` warn that a side "looks like a unified diff" when it holds
    one; give the two versions instead. The two
    versions can be whole files or the same excerpt of both; the line
    numbers then count from the excerpt's first line. Leave out
    `old` for a new file and `new` for a deleted one, never both: a
    diff needs at least one side. `name` is the
    file's name, such as `src/fetch.rs`, shown in the header when there
    is no `title`. Its extension picks the language the unchanged lines
    around the change are highlighted in; the removed and added lines
    show on a red and a green fill. `language`, such as `tsx`, overrides
    the language `name` gives: give it when `name` does not say the
    language you want, such as a `.txt` name whose text is Rust. Without
    `language`, the extension of `name` decides, or the whole name when
    it has none, as `Makefile` and `Dockerfile` do. Show a suggested
    change as a diff, not as a code block of the new version: only the
    diff shows what the change replaces. A new file shows as a diff of
    added lines only, without syntax colours; when the person should
    read new code highlighted, give it as a `code` entry. Give a `title`
    that says what the change does, such as "Retry with backoff".
  - asqr shows a diff unified: only the changed lines with 3 unchanged
    lines around each change, a separator between such hunks, the old
    and the new line number of every line, removed lines in a red fill
    and added lines in a green fill across the pane, and the unchanged
    lines highlighted in the diff's language. The person can switch a
    diff to side by side, the old version on the left and the new one on
    the right with each removed line next to an added one; each side
    keeps its own line numbers, removed and added lines keep their red
    and green fill within their column, and unchanged lines their
    highlighting. A diff whose two versions
    together exceed 1 MiB shows without highlighting. A long line does
    not wrap; the person scrolls sideways. Identical versions show "no
    changes".
  - asqr shows a code entry highlighted, with line numbers in a gutter.
    Long lines do not wrap; the person scrolls sideways. A Markdown entry
    is rendered and wrapped to the pane: headings, emphasis, lists, task
    lists, quotes, tables and highlighted code blocks. A table too wide
    for the pane, or one that would have to break a word to fit, shows as
    records: a block of lines for each row, each column's name before its
    value. A long line of a code block wraps behind a `↪` marker. Name the
    language of a code block's fence so it is highlighted. A file asqr
    cannot read shows as an error card with the reason.
  - Keep a code, Markdown, ANSI or diff entry, and an image's caption,
    under 1 MiB (both sides of a diff together, escape sequences
    included); `validate` and `ask` warn above that. asqr shows at most
    the first 8 MiB of a file and the first 500,000 lines of each text,
    each side of a diff on its own, with a note where it cuts, and
    `validate` and `ask` warn about more lines. Show an excerpt instead
    of a whole large file.
  - Paths are absolute. `asqr ask` turns relative paths into absolute
    ones, resolved against the session file. asqr reads a file when it
    shows it, so leave it in place until the person has answered.
  - Questions and options have no `image` field. A file that still uses
    one gets a warning, and the image does not show.
- `intro`, question `text`, option `label` and option `description` are
  Markdown and show rendered: headings, emphasis, inline code, lists, task lists
  (`- [ ]`, `- [x]`), quotes, links, tables, rules and code blocks. Name the language of a code block's
  fence so it is highlighted. A line wider than the space, such as a
  long line of code, wraps behind a `↪` marker. A table that does not
  fit the space, or would have to break a word in it, shows as records,
  as in a Markdown entry: a block of lines for each row, each column's
  name before its value. A single newline
  breaks the line, and a blank line starts a new paragraph. `<` shows as
  written, so `Vec<String>` needs no backticks, and HTML tags show as
  their source instead of formatting the text. Keep option descriptions
  short: on a small terminal an option whose label and description take
  more lines than the question's area shows a few of them at a time, and
  the person scrolls through it before reaching the next option. Long
  code belongs in a `code` entry of the `context`, which
  shows it with line numbers, not in a question text.
- `intro` shows under the title in at most two lines of the reading
  width, fewer on a low terminal. Anything longer
  is cut off with `… i: more`, and the person reads the rest only by
  pressing `i`. Keep it to a sentence or two that
  identifies the session, such as which round of which piece of work it
  is. Everything the person needs to answer a question goes into that
  question's `text` or its `context`: the text wraps instead of being
  cut and takes the Markdown above, and a little context there makes
  the decision easier, while longer material goes into `context`. On a
  small terminal a long `text` scrolls, and the person reaches its start
  with `↑` from the first row; longer material still belongs in
  `context`, which has a pane of its own.
- Leave the session's `id` out: `asqr ask` assigns one and prints it.
  Questions and options always need their own `id`.

`asqr new` prints a skeleton to start from, `asqr validate <file>` checks
a file (warnings go to stderr), and `asqr schema` prints the JSON
Schema. `asqr ask` refuses an invalid file with exit code 11 and lists the
errors on stderr, so nothing broken reaches the person. Files of an
earlier format version, such as `"asqr": 1`, are refused as well: write
them in the format shown here. A context entry of a `type` this asqr
does not know is refused too, with the types it supports, and so is a
question of a `kind` it does not know, with the kinds it supports. asqr
0.13.1 and earlier know no `info` kind: their `asqr ask` refuses it, and
such an asqr still running answers the session with a result of status
`error`, both with
``questions[0].kind: unknown variant `info`, expected one of `single`, `multi`, `text` ``.
When an error names a type or a kind described here as not supported,
or as an unknown variant, the asqr that read the file is older than this
skill: tell the person, who can update or restart it, and ask again once
they have.

## Asking and waiting

The main pattern works under any time limit on your commands:

```sh
id=$(asqr ask questions.json)
asqr wait "$id" --timeout 540
```

- `asqr ask` validates the file, drops a copy into the queue and prints
  the id. It exits 0 when the session was dropped.
- `asqr wait` prints the result JSON on stdout as soon as it is there.
  When the timeout passes first, it exits 12. Then run the same `wait`
  again, as often as needed. People take their time, and they may answer
  hours later.
- Pick a timeout below your command time limit.

If you can run commands in the background, the variant is one command:

```sh
asqr ask questions.json --wait
```

It prints the id on stderr and only the result JSON on stdout, so it can
be piped into `jq`. `--timeout <secs>` works here too.

`asqr result <id>` looks once without waiting, with the same exit codes.

If `ask` warns "no asqr is watching queue ...", nobody sees your
questions yet. Tell the person to start `asqr` in a terminal of its own;
the session waits in the queue until they do.

### Withdrawing a session

When you no longer need the answers to a session you asked, withdraw
it, so the person does not answer it for nothing:

```sh
asqr withdraw "$id"
```

- Withdraw when the questions are settled another way, such as when the
  person answered them in the chat. For a session you rebuilt, use
  `ask --replaces` (below).
- `withdraw` takes the session out of the person's queue and prints a
  result with `"status": "withdrawn"`. Every `asqr wait` for the
  session ends with exit code 14, your own in the background included,
  and `asqr result` returns the same result later. An asqr older than
  this skill does not know the status: its `wait` ends with exit 1 and
  "not a result file" instead.
- When the person has started answering, `withdraw` refuses with exit
  code 1. A default you set that they left as it is does not count as
  starting. Ask them before you take the session away; `--force`
  withdraws it anyway, and asqr tells them that you withdrew it.
- The result of such a withdrawal carries what they had entered in
  `draft`: their `note` and the `answers` they had given so far. These
  are not decisions: they never sent them, and they might have rejected
  the session in the end. Use them only as presets in a replacement
  session (`"default": true` on the options they picked, which asqr
  shows marked `◀ default`; say in the question's text that these are
  their earlier answers, so the marker does not read as your
  recommendation), where they confirm everything again. Typed text
  (`custom`) and notes have no preset in the format: quote them in the
  replacement question's text, such as "Earlier you wrote: ...", so the
  person can paste or retype them. When you build no replacement,
  ignore them.
- When the person has answered already, `withdraw` refuses with exit
  code 1: read the answers with `asqr result <id>`.
- Withdrawing twice is fine: the second call prints the same result and
  exits 0.
- A rebuilt session under the same id needs no `withdraw`: `ask` it
  again, and it replaces the waiting one unless the person has started
  answering it. Questions they have not touched show the defaults of
  the new file. When it refuses because the person has started
  answering, stderr names the way on. After a withdrawal its result
  stays in the outbox, so asking the same id again needs `--force`.

To replace a session you rebuilt under a new id, ask the new one with
`--replaces`:

```sh
id=$(asqr ask questions.json --replaces "$old_id")
```

It withdraws the old session and drops the new one in one step, and
prints the new id. It refuses where `withdraw` or `ask` would refuse,
with the same exit codes; 13 means the old id is unknown. When it
refuses, nothing changes: the old session stays and the new one is not
dropped. If stderr says the old session was withdrawn already, run the
same command again: it finds the old one withdrawn and drops the new
one.

`--force` with `--replaces` also ends a session the person is in the
middle of: their answers so far go into the old result's `draft`, and
they have to start over on the new one. Use it only as the exception,
after asking them. It also moves an unread result of the new id aside;
reading a result with `asqr result` does not move it, so to get past an
old result of that id without `--force`, leave `id` out of the new file
and let `ask` assign a fresh one.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | the person submitted, the file was dropped (`ask` without `--wait`), or `withdraw` withdrew the session |
| 1 | asqr failed, or `ask` or `withdraw` refused (the reason is on stderr) |
| 2 | wrong arguments |
| 10 | the person rejected the session, with an optional `reason` |
| 11 | the file is invalid: `ask` lists the errors; a result with status `error` holds them in `error` |
| 12 | no result yet: wait again |
| 13 | no result will come: the id is unknown, or the session is gone; for `ask --replaces`, the old id is, and nothing was dropped |
| 14 | you withdrew the session: the result has status `withdrawn` |

`ask` refuses to replace a session the person has started answering, and
refuses while an unread result with the same id is in the outbox.
`--force` moves that result into the archive first. `ask --replaces`
refuses the same way for the old session, and its `--force` takes that
one too.

## Reading the result

```json
{
  "asqr": 2,
  "id": "01K...",
  "status": "submitted",
  "submitted_at": "2026-09-27T17:05:12+02:00",
  "session_sha256": "9f2c...",
  "note": "go ahead, but keep the old changelog entry",
  "answers": [
    { "question": "go", "selected": ["yes"] },
    { "question": "channels", "selected": ["changelog"], "defaulted": true },
    { "question": "highlight", "skipped": true, "note": "ask me after the review" }
  ]
}
```

- `status` is `submitted`, `cancelled` (rejected, with an optional
  `reason`), `error` (with `error`) or `withdrawn` (you withdrew it; no
  answers, and a `draft` only after a forced withdrawal, see above).
- A submitted result lists every question once, in session order,
  except `info` items: an info item is listed only when the person wrote
  a note on it, as `{ "question": "checks", "note": "..." }`. asqr does
  not record whether the person read an info item. A rejected result
  (`cancelled`) lists only the questions with an answer, an untouched
  default included, or a note, in session order.
- `note` at the top is the person's note to the whole session, such as
  a condition on their approval or a remark that spans several
  questions. It is absent when they wrote none. Read it before acting
  on the answers. A rejected result has `reason`, and may also carry
  `note` and answers: what the person had answered or noted before
  rejecting, which they did not submit. The reason may refer to them,
  so read them together.
- `selected` holds option ids, `custom` the typed text, `note` the
  person's note. A skipped question has `skipped: true` and may still
  carry a note: read it, it often says why.
- Every typed text may span several lines: `custom`, both kinds of
  `note` and `reason` can contain `\n`, for example when the person
  pasted a list or wrote the text in their editor.
- `defaulted: true` means the person left your default untouched. A
  question whose default they cleared comes back with `skipped: true`
  in a submitted result, and a rejected result leaves it out unless it
  has a note.

## Finding sessions again

`asqr status --json` lists what is waiting and what is answered:

```json
{
  "queue": "default",
  "watched_by": "pid 4242 on laptop",
  "waiting": [{ "id": "01K...", "draft_has_answers": true }],
  "answered": [{ "id": "01J...", "status": "submitted" }],
  "answered_total": 57
}
```

`queue` is the queue's name, or its directory for a queue given with
`--dir`. `watched_by` is `null` when no asqr runs. `waiting` lists every
waiting session.

`answered` lists the 10 most recently answered sessions, newest first.
The outbox keeps results from every agent and every earlier session, so
`answered_total` is usually larger than the list:

- `--since <age>` keeps only sessions answered within that age, such as
  `30m`, `2h` or `1d` (units `s`, `m`, `h`, `d`, `w`). `answered_total`
  then counts only those.
- `--limit <count>` changes the 10. `--all` lists everything; avoid it,
  the list can be long.

To find a session you lost track of, use `--since` with an age a little
longer than the time since you asked, then read it with
`asqr result <id>`. When you know the id, `asqr result <id>` alone is
enough. `asqr paths --json` shows where the queue's files are.

## Longer work

- **Batches.** Keep a session to what the person can finish in one
  sitting, and drop several sessions; asqr shows them in the order they
  arrive.
- **Follow-up rounds.** When answers lead to new questions, ask a new
  session with `"follows": "<previous id>"`. The person sees the title
  of the session it continues.
- **Keep a record.** Save the answers you act on in your own notes or
  files. Results stay in the queue's outbox until the person prunes
  them.
- **Queues.** Everything goes to the default queue unless `--queue
  <name>` or `ASQR_QUEUE` names another one. Use the queue the person's
  `asqr` watches.

## Patterns for common tasks

Once the user has asked you to use asqr, these patterns suggest what a
session can hold for a kind of task. They are a starting point. Adapt or
extend one for the task at hand, combine them, or leave them aside where
something else fits better. They never make a task a reason to use asqr
without that request.

- **Compare visual variants.** One question per thing to decide, such as
  each icon. Each variant is an option with a short label such as
  `Draft 3`, its image as `context` and a one-line `caption` on what
  sets it apart. Ask for a `single` pick, or a `multi` pick of keepers.
- **Decide before implementing.** One question per open point, with
  what you settled that bears on it in its `text`. An `info` item comes
  first only for settled points that bear on no single question. The
  recommended option comes first and as the `default`, a short trade-off
  goes in each `description`, and the current state or each option's
  own `diff` in `context`.
- **Review findings.** One question per finding. Its `text` gives before
  and after in two sentences and the risk, its `context` the fix as a
  `diff` with a `name`, which highlights the unchanged lines. Options
  such as post as a review comment, fix, or drop.
- **Pick candidates inside a pipeline.** Ask for the candidates of one
  step as a `multi` pick, each option with its own `context` where the
  person needs to see the candidate, and `max` when the next step takes
  at most so many.
- **Debug walk-through.** Say what you found in a few lines: in the
  text of the question on the fix, or in an `info` item before it when
  it takes more, with the log or trace as `context`. Ask one question
  with each candidate fix as an option whose `context` is its `diff`,
  and one `multi` pick of the tests you would add.
- **Triage a backlog** of bug reports, todos or updates. One question
  per item, with its strongest evidence as `context`, such as a
  screenshot or a log, your take and fix idea in the `text`, options
  such as pursue, defer or close, and your recommendation as `default`.
- **Restructure a document.** One question per section, with the
  section's own text as a `markdown` entry in its `context`, the places
  it could go as options with the reason in each `description`, and an
  own answer for somewhere else.
- **Rating grid.** One `single` question per candidate and aspect, such
  as length with too long, right and too short, tone or humour. Each has
  the candidate as `context` and a `header` such as `B tone`. Ask the
  next round, built from the ratings, with `follows`.

Habits that run through them: one question per item, your
recommendation first, before and after spelled out, a change to existing
text shown as a `diff`, and each option's own `context` where the person
compares. What the person needs for a question goes into that
question's `text` or `context`, not into an info item before it. Only
what belongs to no single question, such as results so far, goes into
short `info` items, one topic each and placed before the questions they
lead to, never into a question with an option like "OK" or one long
text. A `single` with a `default` comes back answered with the default
unless the person picks another option or an own answer, or
clears the pick, which skips the question. Where "not yet" is a likely
answer, an option for it is easier for the person to find than clearing
the pick, and it says why without a note.
Read every note, on info items too. For what came up, ask a follow-up
round with `follows`. When the person answers the questions another way,
such as in the chat, withdraw the session. Ask one you rebuilt under a
new id with `--replaces`, which withdraws the old one.
