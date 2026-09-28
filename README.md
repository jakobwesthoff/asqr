# asqr

**Your agent saved up some questions. Answer them all in one go, right in
your terminal.**

asqr (pronounced "asker") makes it easy to answer questions that an AI
coding agent has prepared for you. The agent writes its questions into a
file, asqr shows them in a terminal of its own, and you work through them
at your own pace. When you submit, asqr writes your answers into a result
file that the agent reads back. Scripts and builds can ask the same way.

- Any number of questions per session, and any number of options per
  question.
- Single and multiple choice, typed answers, your own answer instead of
  the given options, and a note on every question.
- Descriptions, defaults and length hints, with a little Markdown.
- Optional images next to a question, shown inline in terminals that
  support it.
- A tab per question and a review tab: move freely, then submit or
  reject the whole session.
- A versioned JSON format with a JSON Schema, so any tool can ask.
- A skill that teaches agents how to use asqr, built into the binary.

## Install

asqr needs Rust 1.97 or newer. From a clone of this repository:

    cargo install --path .

## How it works

Start asqr in a terminal of its own and leave it running:

    asqr

In another terminal, or from an agent, ask:

    asqr new > questions.json       # a skeleton to edit
    id=$(asqr ask questions.json)   # drop it into the queue
    asqr wait "$id"                 # print the result once you submitted

asqr shows the session as soon as it arrives, with a desktop
notification and the terminal bell (`--no-notify`, `--no-bell`). Answer
with the keyboard: `↑`/`↓` and `←`/`→` (or `hjkl`) move, `enter` picks,
typing goes straight into the answer fields, `n` adds a note, and the
last tab reviews everything before you submit or reject. `?` shows every
key. `q` quits; your answers so far are kept as a draft.

`asqr wait` exits 0 for a submitted session, 10 when you rejected it, 11
for an invalid file and 12 when its `--timeout` ran out, so scripts can
branch on it. `asqr --help` lists every command.

Sessions wait in a queue: a directory with an inbox, an outbox for the
results, drafts and an archive. The default queue lives in the
platform's data directory; `--queue <name>` or `ASQR_QUEUE` keeps
projects apart, and `asqr paths` shows where everything is.

## For agents

    asqr skill --install .claude/skills

installs the skill (as `asqr/SKILL.md` under the skills directory you
name), which teaches an agent the format, the waiting pattern and the
exit codes. `asqr skill` prints it.

## Inside tmux

Inline images and desktop notifications only reach the terminal around
tmux when tmux passes escape sequences through:

    set -g allow-passthrough on

Without it, asqr shows images as coarse blocks and rings the bell
instead of sending a notification.

## Known limitations

- One asqr per queue, on one machine: queues on network or synced
  filesystems (NFS, Dropbox, iCloud Drive) are not supported.
- macOS and Linux. Windows may work but is not tested.
- The terminal needs at least 60×15 cells.

## Documentation

The full design is in [`docs/spec-1.0.0.md`](docs/spec-1.0.0.md), the
JSON Schemas are in [`schema/`](schema/), and example sessions are in
[`examples/sessions/`](examples/sessions/).

## Development

Development is test first, and `just check` is the validation pipeline:
license headers, `cargo fmt --check`, clippy with `-D warnings`, tests and
doctests. `just coverage` reports test coverage; CI runs both on Ubuntu
and macOS. After cloning, run `just setup` once: it makes the pre-commit
hook in `.githooks/` run the pipeline before every commit, and names any
tool that is missing (`just`, `cargo-llvm-cov`, `llvm-tools-preview`).

## Decisions

Architecture decisions are recorded in [`docs/adr/`](docs/adr/) and
managed with [adrs](https://github.com/joshrotenberg/adrs).

## License

[Mozilla Public License 2.0](LICENSE). Every source file carries the
license notice in its header.
