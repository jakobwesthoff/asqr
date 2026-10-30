---
name: asqr
description: Ask the person you work with many questions at once through asqr, a terminal inbox for questions, and get their answers back as JSON. Only use it when the user explicitly asks you to use asqr for something; never switch to it on your own.
---

Read this file to its last line before you use asqr. Its rules apply
to every session. The last line is `End of the asqr skill.` If you
have not seen that line, because you read only a head or a range, got
a preview with a file path, or got cut-off output, read the file again
in full: from its path, or from the file your tool saved the output of
`asqr skill` to. Read a file under `references/` when a step below
names it. Each of those files ends with its own end line too.

# asqr

asqr (pronounced "asker") lets you hand the person a whole batch of
questions and get the answers back as a file. You write a session file,
drop it into a queue with `asqr ask`, and the person answers it in a
terminal of their own where `asqr` runs. When they submit, asqr writes a
result file that you read back.

Three files hold details you need only for some steps. The step names
its file when you reach it.

- [references/context.md](references/context.md), or
  `asqr skill context`: before you write any `context` entry or
  `caption`.
- [references/withdrawing.md](references/withdrawing.md), or
  `asqr skill withdrawing`: before you run `asqr withdraw`,
  `ask --replaces` or any `--force`, before you ask an id again, when
  `ask` or `withdraw` refuses, and when a result has status
  `withdrawn` or a `draft`.
- [references/patterns.md](references/patterns.md), or
  `asqr skill patterns`: before you build a session to compare visual
  variants, decide before implementing, review findings, pick
  candidates in a pipeline, walk through a debug, triage a backlog,
  restructure a document or collect ratings.

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

Every session you ask comes back as a result, an info-only one too,
since the person can write a note on any item. Start `asqr wait` for
each session you ask, in the background where you can, and read the
whole result, every note included, before you treat the matter as done.
When the person asks in the chat about something they may have answered
in asqr, run `asqr result <id>` or `asqr status --json --since <age>`
before you answer.

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

When you no longer need the answers to a session, such as when the
person answered its questions in the chat, withdraw it with
`asqr withdraw <id>`, so they do not answer it for nothing. Ask a
session you rebuilt under a new id with
`asqr ask <file> --replaces <old-id>`, which withdraws the old one.
Read [references/withdrawing.md](references/withdrawing.md), or
`asqr skill withdrawing`, before you run either or any `--force`,
before you ask an id again, and when `ask` or `withdraw` refuses.

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
  answers, and a `draft` only after a forced withdrawal). A `draft`
  holds no decisions: read
  [references/withdrawing.md](references/withdrawing.md), or
  `asqr skill withdrawing`, before you use one.
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

## The session file

```json
{
  "asqr": 2,
  "title": "Release 0.3.0",
  "intro": "Three decisions before the release. Skip what you **cannot** decide yet.",
  "from": "your-agent, project name",
  "questions": [
    {
      "id": "done",
      "header": "Done",
      "text": "Since the last round: version bumped to 0.3.0 and the changelog written.",
      "kind": "info"
    },
    {
      "id": "go",
      "header": "Go?",
      "text": "All checks passed. Two flaky tests passed on a retry:\n\n- `fetch::retries_on_timeout`\n- `queue::drops_in_order`\n\nTag `v0.3.0` is created locally. Publish 0.3.0 now?",
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
- Most sessions need no info item. Before you add one, try to ask the
  question it leads to with that content in the question's own `text`,
  or in its `context` when its options have none of their own. An
  option's `context` replaces the question's while the cursor is on
  that option. The person then reads the content where they decide.
  Length is no reason for an info item. Cut the content instead.
- Use an info item only for what bears on several questions or on
  none, such as what you finished since the last round, and for what a
  question needs and cannot show itself, such as a log beside options
  that each show their own diff. Even then, add it only when the person
  needs it to answer or to know where things stand. A problem or a
  result that leads to one question goes into that question. A short
  fact that bears on a few questions takes a clause in each. Shared
  material too long to repeat, such as a log or a table several
  questions refer to, goes once into an info item just before them. Its
  text stays a few lines, and the material itself goes into its
  `context`. An item that only introduces a group of questions, or says how you
  will split up or order your own work, is not needed. The questions'
  `header`s and their order carry the grouping. Do not use a made-up
  question with options like "OK" in place of an info item.
- A session may hold only info items, as a report the person can answer
  with notes. Its result still comes back with the person's notes. Wait
  for it and read it. Such a report follows the same rules. It is short items
  in reading order, not one text.
- Keep an info item's text to a few lines the person takes in at a
  glance. A text taller than the question's area opens cut off at the
  top, showing its end, and the person presses `↑` to reach its start.
  Split more content into several short info items, one topic each.
  Give each a short `header`, which labels its tab. Place each just
  before the block of questions it leads to, or at the start when it
  bears on none, so the person works through the session piece by
  piece instead of taking in everything at once.
- A compact list or a small table reads at a glance in an info item's
  text. Material the person must see as it is, such as a log, a trace
  or a file, goes into the item's `context`, which scrolls and shows
  full screen with `z`. The text then says in a line or two what it
  shows and what to look for. What you write yourself, such as results
  or findings, you split into short items instead of one long table or
  text. Prose to read in one piece comes only when the person asks for
  the document itself, such as a whole file or a full report in one
  piece, and then as a `markdown` entry of the item's `context`. A
  request for a report, a summary or information through asqr is not
  such a request. Split it as above, also in a report of info items
  only.
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
  entry, and `z` opens it. So put long material the person consults,
  such as a file, a log, a diff or a screenshot, into `context` and keep
  the question `text` to what the person reads to decide. Your own
  explanation stays in the `text`, in a few lines, on an info item too.
  Give each entry a `title` that says what it is. An option can have its
  own `context`, shown instead while the cursor is on that option, so
  the person can flip through variants in one question and pick one.
  Before you write any `context` entry or `caption`, read
  [references/context.md](references/context.md), or
  `asqr skill context`: the entry types `image`, `code`, `markdown`,
  `ansi` and `diff`, how asqr shows each, and their size limits.
  - Paths are absolute. `asqr ask` turns relative paths into absolute
    ones, resolved against the session file. asqr reads a file when it
    shows it, so leave it in place until the person has answered.
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
  question's `text` or its `context`. Write it for a person who has not
  seen your work and knows neither your context nor your earlier
  messages. Say what a thing is and what changes, and name it, such as
  the file or the decision and its date, instead of referring to it by
  your todo title, a code name, a size code or a step they did not see.
  Leave out what the person does not need to decide, such as how you
  will split the work, and what is only true inside your run, such as
  what a later session will hold. The text wraps instead of being
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

[references/patterns.md](references/patterns.md), or
`asqr skill patterns`, suggests what a session can hold for eight kinds
of task: comparing visual variants, deciding before implementing,
reviewing findings, picking candidates in a pipeline, walking through a
debug, triaging a backlog, restructuring a document and collecting
ratings. They are a starting point, and they never make a task a reason
to use asqr without the user's request.

Habits that run through them: one question per item, your
recommendation first, before and after spelled out, a change to existing
text shown as a `diff`, and each option's own `context` where the person
compares. What the person needs for a question goes into that
question's `text`, or into its `context` when its options have none of
their own. Only what the question cannot show goes into an info item
before it. Content that bears on several questions or on none goes into
short `info` items only when the person needs it, one topic each,
placed before the questions they lead to or at the start when they lead
to none. Never put it into a question with an option like "OK", and
never into one long text. A `single` with a `default` comes back
answered with the default unless the person picks another option or an
own answer, or clears the pick, which skips the question. Where "not
yet" is a likely answer, an option for it is easier for the person to
find than clearing the pick, and it says why without a note.
Read every note, on info items too. For what came up, ask a follow-up
round with `follows`. When the person answers the questions another way,
such as in the chat, withdraw the session. Ask one you rebuilt under a
new id with `--replaces`, which withdraws the old one.

End of the asqr skill.
