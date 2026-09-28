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
  option, an image, a suggested default
- the person should be able to skip, add a note, or type their own
  answer
- the answers feed straight into your next step as data

## The session file

```json
{
  "asqr": 1,
  "title": "Release 0.3.0",
  "intro": "Three decisions before the release. Skip what you **cannot** decide yet.",
  "from": "your-agent, project name",
  "questions": [
    {
      "id": "go",
      "header": "Go?",
      "text": "All checks passed. Publish 0.3.0 now?",
      "kind": "single",
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
  and `max`) or `text` (typed answer, no options).
- `custom: true`, or `{ "label": ..., "length": ... }`, adds a one-line
  own answer to a `single` or `multi` question.
- `length` gives a target range and a `warn` limit for typed text. It
  guides with a counter; nothing the person types is refused.
- `note` (default `true`) lets the person add a note to any question;
  set `"note": false` to turn it off.
- Every question is optional: the person may skip any of them, and a
  skipped question may still carry a note explaining why.
- `min` and `max` count the picked options; the own answer of a `multi`
  comes on top of them.
- `default: true` on an option preselects it.
- `image` takes an absolute path to an image shown next to the question.
  `asqr ask` turns relative paths into absolute ones, resolved against
  the session file.
- `intro`, question `text` and option `description` may use bold,
  italic, inline code, lists and line breaks.
- Leave `id` out: `asqr ask` assigns one and prints it.

`asqr new` prints a skeleton to start from, `asqr validate <file>` checks
a file (warnings go to stderr), and `asqr schema` prints the JSON
Schema. `asqr ask` refuses an invalid file with exit code 11 and lists the
errors on stderr, so nothing broken reaches the person.

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

## Exit codes

| Code | Meaning |
|---|---|
| 0 | the person submitted, or the file was dropped (`ask` without `--wait`) |
| 1 | asqr failed, or `ask` refused the drop (the reason is on stderr) |
| 2 | wrong arguments |
| 10 | the person rejected the session, with an optional `reason` |
| 11 | the file is invalid: `ask` lists the errors; a result with status `error` holds them in `error` |
| 12 | no result yet: wait again |
| 13 | no result will come: the id is unknown, or the session is gone |

`ask` refuses to replace a session the person has started answering, and
refuses while an unread result with the same id is in the outbox.
`--force` moves that result into the archive first.

## Reading the result

```json
{
  "asqr": 1,
  "id": "01K...",
  "status": "submitted",
  "submitted_at": "2026-09-27T17:05:12+02:00",
  "session_sha256": "9f2c...",
  "answers": [
    { "question": "go", "selected": ["yes"] },
    { "question": "channels", "selected": ["changelog"], "defaulted": true },
    { "question": "highlight", "skipped": true, "note": "ask me after the review" }
  ]
}
```

- `status` is `submitted`, `cancelled` (rejected, with an optional
  `reason`) or `error` (with `error`).
- A submitted result lists every question once, in session order.
- `selected` holds option ids, `custom` the typed text, `note` the
  person's note. A skipped question has `skipped: true` and may still
  carry a note: read it, it often says why.
- `defaulted: true` means the person left your default untouched.

## Finding sessions again

`asqr status --json` lists what is waiting and what is answered:

```json
{
  "queue": "default",
  "watched_by": "pid 4242 on laptop",
  "waiting": [{ "id": "01K...", "draft_has_answers": true }],
  "answered": [{ "id": "01J...", "status": "submitted" }]
}
```

`queue` is the queue's name, or its directory for a queue given with
`--dir`. `watched_by` is `null` when no asqr runs. `asqr paths --json` shows where
the queue's files are.

## Longer work

- **Batches.** Keep a session to what the person can finish in one
  sitting, and drop several sessions; asqr shows them in the order they
  arrive.
- **Follow-up rounds.** When answers lead to new questions, ask a new
  session with `"follows": "<previous id>"`. The person sees which
  session it continues.
- **Keep a record.** Save the answers you act on in your own notes or
  files. Results stay in the queue's outbox until the person prunes
  them.
- **Queues.** Everything goes to the default queue unless `--queue
  <name>` or `ASQR_QUEUE` names another one. Use the queue the person's
  `asqr` watches.
