---
title: "Commands"
tags: [guide/reference]
site:
  order: 2
---
Every asqr command. `asqr --help` lists them too, and `asqr <command>
--help` describes the flags of one.

## Commands

| Command | What it does |
|---|---|
| `asqr`, `asqr watch` | Starts the TUI, in which you answer sessions. Takes `--no-notify`, `--no-bell`, `--no-title` and `--layout auto\|beside\|below`. |
| `asqr new` | Prints a session skeleton without an id, to start a session file from. |
| `asqr validate <file>` | Checks a session file and reports its errors and warnings. |
| `asqr schema [--result]` | Prints the JSON Schema of session files, or with `--result` of result files. |
| `asqr ask <file> [--wait] [--timeout <secs>] [--force] [--replaces <id>]` | Validates a session file, assigns a ULID when the id is missing, makes the paths of context entries absolute, drops the file into the queue and prints the id. `--wait` waits for the result and prints it instead of the id. `--timeout` bounds that wait and needs `--wait`. `--force` moves an unread result of the same id into the archive first. `--replaces` withdraws a waiting session and drops the new one in its place. |
| `asqr wait <id> [--timeout <secs>]` | Waits for the result and prints it. Without `--timeout` it blocks. |
| `asqr result <id>` | Prints the result if it is there, like `wait --timeout 0`. |
| `asqr withdraw <id> [--force]` | Takes a waiting session back and prints its result, with the status `withdrawn`. `--force` takes one that you started answering. |
| `asqr status [--json] [--limit <count>\|--all] [--since <age>]` | Lists the waiting sessions, with or without a draft answer, the answered sessions and the asqr that watches the queue. `--since` keeps the sessions answered within an age such as `2h`, `--limit` lists at most that many, 10 by default. |
| `asqr queues [--json]` | Lists every named queue with its waiting sessions, its results and the asqr that watches it. |
| `asqr paths [--json]` | Prints where the queue and its files are. |
| `asqr compact [--all]` | Bundles finished sessions and results older than 30 days into month bundles. `--all` does it for every named queue. |
| `asqr prune --older-than <age> [--yes]` | Lists what it would delete: archive entries and bundles of months older than the age. With `--yes` it deletes them. |
| `asqr skill [<part> \| --install <dir>]` | Prints the skill of this asqr, or one of its parts, or writes it to `<dir>/asqr/`. |

`asqr prune` takes ages in days (`d`), weeks (`w`), months of 30 days
(`mo`) and years of 365 days (`y`), at least 30 days. It deletes nothing
without `--yes`. It is the only way asqr deletes sessions and results.

## Flags every command takes

`--queue <name>` works on the named queue under the platform's data
directory, and `--dir <path>` on the queue in a directory. The two do not
combine. The [page on queues](01M68C1XV58STWHBH179QYXPZR-queues.md) says
how asqr chooses the queue without them.

`--no-notify`, `--no-bell`, `--no-title` and `--layout` stand before
`watch` or alone. Next to any other command they are a usage error.

## Exit codes

The same for every command:

| Code | Meaning |
|---|---|
| 0 | Success: the result is `submitted`, the file was dropped, `withdraw` withdrew the session, or the command did its job. |
| 1 | asqr itself failed: I/O, a refused `ask` or `withdraw`, a held lock, no terminal. |
| 2 | Usage error. |
| 10 | The result is `cancelled`: you rejected the session. |
| 11 | The result is `error`, or a file fails validation. |
| 12 | Timeout: no result yet. |
| 13 | No result will come: nothing in the queue has the id, or the session left without one. |
| 14 | The result is `withdrawn`: the asker took the session back. |
