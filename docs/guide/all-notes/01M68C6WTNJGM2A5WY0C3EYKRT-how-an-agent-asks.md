---
title: "How an agent asks"
tags: [guide/agents]
site:
  index: true
  label: Agents
  order: 1
---
An agent, or a script, asks you questions by writing a file and waiting
for another one. This page follows that path from the agent's side. You
only answer in asqr.

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

`asqr status --json` lists the waiting sessions and the latest answered
ones, to find a session an agent lost track of.

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
result carries your `reason`. [Set up Claude Code](01M68C6WTY7EK8Q9RB1MCWSFM3-set-up-claude-code.md)
shows a real result.

## The skill

asqr ships a skill that teaches an agent the format, the waiting pattern,
the exit codes and what to do about each of them. It is built into the
binary: `asqr skill` prints the core of the skill that matches the asqr
that runs, and `asqr skill <part>` prints one of its parts, `context`,
`withdrawing` or `patterns`. An agent you tell to use asqr can read the
skill from the binary, so installing it is possible but mostly not needed.

The skill tells an agent to use asqr only when you explicitly ask it to,
for example "ask me through asqr". Without such a request an agent asks
the way it normally does.

`asqr --help` tells agents to read the skill first. Where agents tend to go
wrong, asqr adds a line that starts with `hint:` after its output, and the
skill tells them to follow it.
