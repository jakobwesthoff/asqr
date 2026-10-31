---
title: "How an agent asks"
tags: [guide/agents]
site:
  order: 2
---
An agent, or a script, asks you questions by writing a file and waiting
for another one. This page follows that path from the agent's side, with
the file an agent writes and the result it reads back. You only answer in
asqr, and a script asks with the same commands.

## The path

1. The agent writes a session file in JSON: a title, an intro, and
   questions of the four kinds you know from the earlier pages.
   `asqr new` prints a skeleton to start from, `asqr validate <file>`
   checks a file and `asqr schema` prints the JSON Schema.
2. `asqr ask <file>` validates the file, drops a copy into the queue and
   prints the session's id. When the file has no `id`, asqr assigns one.
   When no asqr watches the queue, `ask` warns about it.
3. You see the session in asqr, answer it and submit it.
4. `asqr wait <id> --timeout <seconds>` prints the result as soon as it is
   there. `asqr result <id>` looks once and does not wait. With
   `asqr ask <file> --wait`, one command asks and waits.
5. If the agent no longer needs the answers, `asqr withdraw <id>` takes the
   session back, so you do not answer it for nothing.

To rebuild a session under another id, `asqr ask <file> --replaces <old-id>`
withdraws the old session and drops the new one in one step. If you had
started on the old session, asqr carries your draft over into the new one,
matched by question id and option id: a question the asker keeps under its
id keeps what you entered, and one under a new id starts fresh. The asker
writes no text into your fields. The new session counts as started: `ask`
of its id is refused, also with `--force`, and replacing it again takes
`--replaces` with `--force`. The hint `ask --replaces` prints says whether
anything was carried, and the `draft` of the old result still holds your
answers and notes. What you see is described in [Review, submit and come back
later](01M68BT9BMFVCVEAACWR03B827-review-submit-and-come-back-later.md).

`asqr status --json` lists the waiting sessions and the latest answered
ones, to find a session an agent lost track of. Every command with its
flags is on the page [Commands](01M68CASKFMEPMTN3JA6Z2DGCQ-commands.md).

`asqr validate` and `asqr ask` print warnings on stderr and do not fail for
them. They warn about fields they do not know, a relative path in a context
entry (`validate` only, as `ask` resolves it), a file that does not exist or
is no regular file, a file the viewer shows a red card for, an entry above 1
MiB, and a `note` on a question or an info item. Every question takes a
note, so asqr ignores the `note` field of a question, whatever its value,
and `ask` removes it from the copy it puts into the inbox.

An agent reads earlier sessions too. `asqr history --json` lists the
finished sessions, `asqr replay <ref> --json` prints one with its result and
the state of each file it showed, and `asqr replay <ref> --extract <dir>`
writes the session, the result and the kept copies of its files, images
included, into a new or empty directory. None of the three needs a
terminal. The skill names all three, and [Finished sessions: history and
replay](01M68KBBY1Y1YB4YFQ8WC37QP5-finished-sessions-history-and-replay.md)
describes the output.

## An example

You tell your agent: "Use asqr to ask me the open questions about the release."

In the format the skill describes, the agent writes the questions as a
session file and asks. This is a session it could write, as
`questions.json`:

```json
{
  "asqr": 2,
  "title": "Release 0.2.0",
  "intro": "Three decisions before the release. Skip what you cannot decide `yet`.",
  "from": "release script",
  "questions": [
    {
      "id": "go",
      "header": "Go?",
      "text": "All checks passed. Publish **0.2.0** now?",
      "kind": "single",
      "options": [
        { "id": "yes", "label": "Publish now" },
        { "id": "later", "label": "Later", "description": "Keep the tag, publish after the weekend." },
        { "id": "no", "label": "Do not publish", "description": "Delete the tag again." }
      ]
    },
    {
      "id": "channels",
      "header": "Channels",
      "text": "Where should the release be announced?",
      "kind": "multi",
      "max": 3,
      "options": [
        { "id": "changelog", "label": "Changelog", "default": true },
        { "id": "blog", "label": "Blog post" },
        { "id": "social", "label": "Social media" }
      ],
      "custom": { "label": "Somewhere else" }
    },
    {
      "id": "highlights",
      "header": "Highlights",
      "text": "One sentence for the release notes:",
      "kind": "text",
      "length": { "target": [40, 120], "warn": 140 }
    }
  ]
}
```

The agent's own questions will differ. The file has no `id`, as the skill
tells agents to leave it out. Then the agent asks and waits with the
commands the skill prescribes:

```sh
id=$(asqr ask questions.json)
asqr wait "$id" --timeout 540
```

In asqr, this session shows as on the pictures of [Get
started](01M68ASRJ527YVE41S9V50YXQW-get-started.md). You pick "Publish
now", leave the channels as they are, type a sentence and submit on the
review tab.

`asqr wait` prints the result
and exits with 0. This is the result of a real run of this session. The id
and the time differ from run to run:

```json
{
  "asqr": 2,
  "id": "01M68C6004X3C3SK4S5X9RYNBS",
  "status": "submitted",
  "submitted_at": "2026-10-31T08:01:09+01:00",
  "session_sha256": "0b7af853270911da0703acd0ad93371681c078f649738c6a1d852b3590e83813",
  "answers": [
    {
      "question": "go",
      "selected": [
        "yes"
      ]
    },
    {
      "question": "channels",
      "selected": [
        "changelog"
      ],
      "defaulted": true
    },
    {
      "question": "highlights",
      "custom": "Startup is twice as fast."
    }
  ]
}
```

`defaulted: true` says that you left the default of the channels as it
was. The `custom` text is your typed sentence.

## The exit codes

`asqr wait` and `asqr result` exit by the result.

| Code | Meaning |
|---|---|
| 0 | You submitted. |
| 10 | You rejected the session, with an optional reason. |
| 11 | The file is invalid. |
| 12 | No result yet: the agent waits again. |
| 13 | No result will come: the id is unknown, or the session is gone. |
| 14 | The asker withdrew the session. |

`asqr ask` exits with 0 when it dropped the file, with 11 for an invalid
file and with 1 when it refused. Exit code 2 stands for wrong arguments.

## The result

The result is a JSON file in the outbox. Its `status` is `submitted`,
`cancelled` for a rejected session, `error` for an invalid file or
`withdrawn`. A submitted result lists every question once, in the order of
the session. An info item appears only when you wrote a note on it.

- `selected` holds the ids of the options you picked.
- `custom` holds the text you typed.
- `note` holds your note on the question.
- A skipped question has `skipped: true`.
- `defaulted: true` means you left the default as it was.

The note to the whole session is the `note` at the top level, and a rejected
result carries your `reason`. The example above shows a real result.

## Hints and the skill

Where agents tend to go wrong, asqr adds a line that starts with `hint:`
after its output, and the skill tells agents to follow it. [The skill and
other agents](01M68C6WTY7EK8Q9RB1MCWSFM3-the-skill-and-other-agents.md)
says what the skill holds and how to install it.
