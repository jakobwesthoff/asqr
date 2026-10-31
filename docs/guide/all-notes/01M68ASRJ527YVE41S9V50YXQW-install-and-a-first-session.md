---
title: "Install and a first session"
tags: [guide/start]
site:
  order: 2
---
This page installs asqr and walks through one session from the first
command to the result.

## Install

With Homebrew, on macOS and Linux:

```sh
brew install jakobwesthoff/tap/asqr
```

From crates.io, which needs Rust 1.97 or newer:

```sh
cargo install asqr
```

Every GitHub release also carries prebuilt binaries for macOS (Apple
silicon and Intel) and Linux (x86_64 and aarch64). Windows may work but
is not tested. The terminal needs at least 60 columns by 15 rows.

## Start asqr

Open a terminal of its own and start asqr:

```sh
asqr
```

The first time asqr starts and has no settings file, it asks one question
before anything else: whether it keeps copies of the files a session
showed. Pick a value with `↑`/`↓` and `enter`, then press `enter` on
`[ Save and start ]`. The
[settings page](01M68C1XVEMX1D38GK6GY0V1NZ-settings.md)
describes the screen.

![The first-start screen, which asks whether asqr keeps the files a session showed](../pictures/first-start.png)

Leave asqr running. With nothing waiting it says "Nothing to answer. New
sessions show up here as soon as they arrive."

![The empty queue with Talqi, and sessions waiting in another queue](../pictures/empty-queue.png)

## Ask a first session

In another terminal, ask the session that `asqr new` prints. It has
three questions: a single choice, a multiple choice and a typed answer.

```sh
asqr new > questions.json
id=$(asqr ask questions.json)
```

`asqr ask` checks the file, drops it into the queue and prints the
session's id. Both commands have to use the same queue. Outside tmux
they both use the default queue. Inside tmux, run them in a pane of the
same tmux session as asqr.

asqr shows the session as soon as it arrives, with a desktop
notification and a terminal bell.

![The first question of a session: title bar, header, tabs, options and key bar](../pictures/first-question.png)

The picture shows another session, a release checklist from the
`examples/sessions` folder of the asqr repository. Most pictures in this
guide show it.

## Answer it

These keys are enough for the first session:

- `↑`/`↓` (or `k`/`j`) move between the options.
- `enter` picks the option and moves on to the next question. In a
  multiple choice, `space` toggles an option and `enter` moves on.
- `←`/`→` (or `h`/`l`) go to the previous and the next question. The
  last tab is the review.
- On the own-answer row and in a text question, what you type goes
  straight into the answer.
- `?` shows every key.
- On the review tab, `enter` on `[ Submit ]` sends your answers. `q`
  quits asqr and keeps your answers so far as a draft.

![The review tab of an answered session: every answer on a line, the counts and the submit button](../pictures/review.png)

The pages after this one explain every screen and every key, starting
with [the screen](01M68BT9AJHW7WX304EQKGBGF6-the-screen.md).

## Read the result

Back in the other terminal:

```sh
asqr wait "$id"
```

`asqr wait` blocks until you have submitted, then prints the result as
JSON. Its exit code is 0 for a submitted session and 10 for one you
rejected. `asqr result "$id"` looks once and does not wait.
