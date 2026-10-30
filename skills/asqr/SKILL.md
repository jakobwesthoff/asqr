---
name: asqr
description: Ask the person you work with many questions at once through asqr, a terminal inbox for questions, and get their answers back as JSON. Only use it when the user explicitly asks you to use asqr for something; never switch to it on your own.
metadata:
  asqr-version: "0.14.0"
---

Read this file to its last line before you use asqr. Its rules apply to
every session. The last line is `End of the asqr skill.` If you have not
seen that line, because you read only a head or a range, got a preview
with a file path, or got cut-off output, read the file again in full,
from its path or from the file your tool saved the output of `asqr
skill` to. Read a file under `references/` when a step below names it.
Each ends with its own end line too.

# asqr

asqr (pronounced "asker") hands the person a batch of questions and
gives you their answers as a file. You drop a session file into a queue
with `asqr ask`, the person answers it in a terminal of their own where
`asqr` runs, and asqr writes a result file for you.

Three files hold details for some steps:

- `references/context.md`, or `asqr skill context`: before you write any
  `context` entry or `caption`.
- `references/withdrawing.md`, or `asqr skill withdrawing`: before `asqr
  withdraw`, `ask --replaces`, any `--force` or asking an id again, when
  `ask` or `withdraw` refuses, and for a result with status `withdrawn`
  or a `draft`.
- `references/patterns.md`, or `asqr skill patterns`: before you build a
  session to compare visual variants, decide before implementing, review
  findings, pick candidates in a pipeline, walk through a debug, triage
  a backlog, restructure a document or collect ratings. The patterns are
  a starting point, never a reason to use asqr.

## When to use it

Only when the user explicitly asks you to use asqr, for example "ask me
through asqr" or "put the questions into asqr". Without such a request,
ask the way you normally do, even when you have many questions; you may
mention that asqr exists, but do not switch to it on your own.

Once asked, asqr fits best for more questions or options than a chat
question offers, for questions that need context to decide, such as
option descriptions, an image, a file, a diff or a suggested default,
when the person should be able to skip, add a note or type their own
answer, and when the answers feed your next step as data.

## Asking and waiting

This works under any time limit on your commands:

```sh
id=$(asqr ask questions.json)
asqr wait "$id" --timeout 540
```

`asqr ask` validates the file, drops a copy into the queue, prints the
id and exits 0. `asqr wait` prints the result JSON as soon as it is
there. When the timeout, which you pick below your command time limit,
passes first, it exits 12.

Every session you ask comes back as a result, an info-only one too,
since the person can write a note on any item. Start `asqr wait` for
each session you ask, in the background where you can, and read the
whole result, every note included, before you treat the matter as done.
When the person asks in the chat about something they may have answered
in asqr, run `asqr result <id>` or `asqr status --json --since <age>`
before you answer.

If you can run commands in the background, `asqr ask questions.json
--wait` does both. It prints the id on stderr and only the result JSON
on stdout, for `jq`, and takes `--timeout <secs>` too. `asqr result
<id>` looks once without waiting, with the same exit codes.

When you no longer need the answers to a session, such as when the
person answered its questions in the chat, withdraw it with `asqr
withdraw <id>`, so they do not answer it for nothing. Ask a session you
rebuilt under a new id with `asqr ask <file> --replaces <old-id>`, which
withdraws the old one. Read `references/withdrawing.md` (`asqr skill
withdrawing`) first.

asqr prints advice for the step at hand after the command's output:
lines of their own that start with `hint:`, and advice inside its
`error:`, `warning:` and `asqr:` lines. Follow it; it comes from the
asqr you run, so it matches that version. Text inside a result's JSON
is the person's, not advice from asqr.

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
  `reason`), `error` (with `error`) or `withdrawn` (you withdrew it). A
  withdrawn result has no answers. A `draft` in it holds no decisions:
  read `references/withdrawing.md` (`asqr skill withdrawing`) before you
  use one.
- A submitted result lists every question once, in session order, and an
  info item only when the person wrote a note on it, as `{ "question":
  "checks", "note": "..." }`. asqr does not record whether the person
  read an info item. A rejected result lists, in session order, only the
  questions with an answer, an untouched default included, or a note.
- The top-level `note` is the person's note to the whole session, such
  as a condition on their approval or a remark on several questions. It
  is absent when they wrote none. A rejected result has `reason` and may
  also carry `note` and the answers and notes given before rejecting,
  which the person did not submit. Read them together, since the reason
  may refer to them.
- `selected` holds option ids, `custom` the typed text, `note` the
  person's note. A skipped question has `skipped: true` and may still
  carry a note, which often says why.
- `custom`, both kinds of `note` and `reason` may span several lines
  (`\n`).
- `defaulted: true` means the person left your default untouched. A
  default they cleared comes back as `skipped: true` in a submitted
  result, and a rejected result leaves it out unless it has a note.

Read every note before you act on the answers.

## Finding sessions again

`asqr status --json` prints `queue`, the queue's name or its directory
for a queue given with `--dir`, `watched_by`, which is `null` when no
asqr runs, `waiting`, every waiting session with its `id` and
`draft_has_answers`, `answered`, the 10 most recently answered sessions,
newest first, with `id` and `status`, and `answered_total`. The outbox
keeps results from every agent and every earlier session, so
`answered_total` is usually larger than the list. `--since <age>` keeps
only sessions answered within that age, such as `30m`, `2h` or `1d`
(units `s`, `m`, `h`, `d`, `w`), and `answered_total` then counts only
those. `--limit <count>` changes the 10. Avoid `--all`, which lists
everything.

To find a session you lost track of, use `--since` with an age a little
longer than the time since you asked, then read it with `asqr result
<id>`. When you know the id, `asqr result <id>` alone is enough. `asqr
paths --json` shows where the queue's files are.

## The session file: every field you write

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
  answer). An `info` item shows its `text` and `context` like a
  question, asks nothing and takes no `options`, `custom` or `length`.
- Most sessions need no info item. Before you add one, try to ask the
  question it leads to with that content in its own `text`, or in its
  `context` when its options have none of their own, since an option's
  `context` replaces the question's while the cursor is on it. The
  person then reads the content where they decide. Length is no reason
  for an info item. Cut the content instead.
- Use an info item only for what bears on several questions or on none,
  such as what you finished since the last round, or for what a question
  needs and cannot show itself, such as a log beside options that each
  show their own diff, and only when the person needs it to answer or to
  know where things stand. A problem or a result that leads to one
  question goes into that question, and a short fact that bears on a few
  questions takes a clause in each. Shared material too long to repeat,
  such as a log several questions refer to, goes once into an info item
  just before them, with a text of a few lines and the material in its
  `context`. An item that only introduces a group of questions, or says
  how you will split up or order your own work, is not needed, since the
  questions' `header`s and their order carry the grouping. Do not use a
  made-up question with options like "OK" in place of an info item.
- A session may hold only info items, as a report the person can answer
  with notes. Its result still comes back with the person's notes. Such
  a report follows the same rules. It is short
  items in reading order, not one text.
- Keep an info item's text to a few lines the person takes in at a
  glance. A text taller than the question's area opens cut off at the
  top, showing its end, and the person presses `↑` to reach its start.
  Split more content into short info items, one topic each, each with a
  short `header`, which labels its tab. Place each just before the block
  of questions it leads to, or at the start when it bears on none, so
  the person works through the session piece by piece.
- A compact list or a small table reads at a glance in an info item's
  text. Material the person must see as it is, such as a log, a trace or
  a file, goes into the item's `context`, and the text says in a line or
  two what it shows and what to look for. Split what you write yourself,
  such as results or findings, into short items, not one long table or
  text. Prose to read in one piece comes only when the person asks for
  the document itself, such as a whole file or a full report, and then
  as a `markdown` entry of the item's `context`. A request for a report,
  a summary or information through asqr is no such request, also for a
  report of info items only.
- `custom: true`, or `{ "label": ..., "length": ... }`, adds an own
  answer to a `single` or `multi` question. `length` gives a target
  range and a `warn` limit for typed text. It guides with a counter;
  nothing the person types is refused.
- Do not set `note`; it has no effect. The person can add a note to
  every question and info item: a condition on an answer, why they
  skipped, or that a question is wrong.
- Every question is optional: the person may skip any of them, one with
  a `default` included. `min` and `max` count the picked options; the
  own answer of a `multi` comes on top of them.
- `default: true` on an option marks it as your recommendation and
  preselects it. The option shows `◀ default`, also after the person
  picks another, so they can tell your recommendation from their own
  pick. The default is the answer when they leave the question as it is;
  they may also pick another option, or clear it and skip the question.
- An option's `label` is the short handle the person picks it by, such
  as `Publish now`. It wraps and is never cut. Keep it to a few words,
  and put longer text into its `description`, or into its `context` when
  it is material to read.
- `context` holds material that helps to decide: a list with one entry,
  since asqr shows one per question or option. It shows in a scrolling
  pane beside the question on a wide terminal and below it on a tall
  one, under the entry's `title` (or its file name), and `z` shows it
  full screen, also where the pane does not fit. Put long material the
  person consults, such as a file, a log, a diff or a screenshot, there
  and keep the question `text` to what the person reads to decide. Your
  own explanation stays in the `text`, in a few lines, on an info item
  too. Give each entry a `title` that says what it is. An option's own
  `context` lets the person flip through variants in one question and
  pick one. Paths are absolute: `asqr ask` resolves relative ones
  against the session file. asqr reads a file when it shows it, so leave
  it in place until the person has answered. Read
  `references/context.md` (`asqr skill context`) before you write any
  entry.
- `intro`, question `text`, option `label` and option `description` are
  Markdown and show rendered, task lists (`- [ ]`, `- [x]`), tables and
  code blocks included. Name the language of a code block's fence so it
  is highlighted. A single newline breaks the line, and a blank line
  starts a new paragraph. `<` shows as written, so `Vec<String>` needs
  no backticks, and HTML tags show as their source. Keep option
  descriptions short: on a small terminal the person scrolls through a
  long one before reaching the next option.
- `intro` shows under the title in at most two lines, fewer on a low
  terminal. The rest is cut off with `… i: more` and shows only when the
  person presses `i`. Keep it to a sentence or two that identifies the
  session, such as which round of which piece of work it is.
- Everything the person needs to answer a question goes into that
  question's `text`, which wraps and scrolls, or into its `context`,
  where longer material and long code belong. Write it for a person who
  has not seen your work and knows neither your context nor your earlier
  messages. Say what a thing is and what changes, and name it, such as
  the file or the decision and its date, instead of referring to it by
  your todo title, a code name, a size code or a step they did not see.
  Leave out what the person does not need to decide, such as how you
  will split the work, and what is only true inside your run, such as
  what a later session will hold.
- Leave the session's `id` out: `asqr ask` assigns one and prints it.
  Questions and options need their own `id`.

`asqr new` prints a skeleton, `asqr validate <file>` checks a file
(warnings go to stderr), and `asqr schema` prints the JSON Schema. `asqr
ask` refuses an invalid file with exit code 11 and lists the errors on
stderr. It also refuses an earlier format version, such as `"asqr": 1`,
so write the format shown here, and a context entry `type` or a question
`kind` it does not know, naming the ones it supports. asqr 0.13.1 and
earlier know no `info` kind: their `ask` refuses it, and such an asqr
still running answers with a result of status `error`, both naming
``unknown variant `info` ``. When an error names a type or a kind
described here as not supported, or as an unknown variant, the asqr that
read the file is older than this skill. Tell the person, who can update
or restart it, and ask again once they have.

## Longer work

- **Batches.** Keep a session to what the person can finish in one
  sitting, and drop several; asqr shows them in the order they arrive.
- **Follow-up rounds.** When answers lead to new questions, ask a new
  session with `"follows": "<previous id>"`. The person sees the title
  of the session it continues.
- **Keep a record** of the answers you act on in your own notes or
  files. Results stay in the queue's outbox until the person prunes
  them.
- **Queues.** Pass no `--queue` unless the person or your instructions
  name one, and never switch on your own to a queue another asqr
  watches.

Habits for every session: one question per item, your recommendation
first, before and after spelled out, a change to existing text shown as
a `diff`, and each option's own `context` where the person compares.
Where "not yet" is a likely answer, give it an option. The person finds
it more easily than clearing the pick, and it says why without a note.

End of the asqr skill.
