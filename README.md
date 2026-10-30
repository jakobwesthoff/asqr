<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/jakobwesthoff/asqr/main/assets/emblem/talqi-terminal-rim-mint.svg">
    <img src="https://raw.githubusercontent.com/jakobwesthoff/asqr/main/assets/emblem/talqi-terminal.svg" width="256" height="256" alt="asqr's logo: Talqi, a teal chameleon with a lavender crest, peeking over a terminal panel with a prompt">
  </picture>
</p>

# asqr

**Your agent saved up some questions. Answer them all in one go, right in
your terminal.**

asqr (pronounced "asker") makes it easy to answer questions that an AI
coding agent has prepared for you. The agent writes its questions into a
file, asqr shows them in a terminal of its own, and you work through them
at your own pace. When you submit, asqr writes your answers into a result
file that the agent reads back. Scripts and builds can ask the same way.

The chameleon in the logo is [Talqi](assets/talqi-512.webp) (pronounced
like "talkie"), asqr's mascot.

- Any number of questions per session, and any number of options per
  question.
- Single and multiple choice, typed answers, your own answer instead of
  the given options, a note on every question and one on the whole
  session.
- Descriptions, defaults and length hints. The intro, question texts,
  option labels and descriptions are Markdown, rendered with highlighted
  code blocks.
- Context next to a question, or one per option to flip through
  variants: an image, highlighted code, rendered Markdown, text in the
  colours of its escape sequences or a diff that asqr computes from the
  old and the new text. It shows in a pane of its own beside or below
  the question, and you scroll it. Images show inline in terminals that
  support it.
- A tab per question and a review tab: move freely, then submit or
  reject the whole session.
- A versioned JSON format, currently version 2, with a JSON Schema, so
  any tool can ask. `asqr skill` describes it.
- A skill that teaches agents how to use asqr, built into the binary.

## Install

With Homebrew, on macOS and Linux:

    brew install jakobwesthoff/tap/asqr

From crates.io, which needs Rust 1.97 or newer:

    cargo install asqr

or from a clone of this repository:

    cargo install --path .

Every GitHub release also carries prebuilt binaries for macOS (Apple
silicon and Intel) and Linux (x86_64 and aarch64).

## How it works

Start asqr in a terminal of its own and leave it running:

    asqr

In another terminal, or from an agent, ask:

    asqr new > questions.json       # a skeleton to edit
    id=$(asqr ask questions.json)   # drop it into the queue
    asqr wait "$id"                 # print the result once you submitted

asqr shows the session as soon as it arrives, with a desktop
notification and the terminal bell (`--no-notify`, `--no-bell`), and the
terminal title counts the waiting sessions and names any queue but the
default one (`--no-title`). Ghostty,
iTerm2, Windows Terminal and kitty inside tmux also show how much of the
session is answered. Answer
with the keyboard: `↑`/`↓` and `←`/`→` (or `hjkl`) move, `enter` picks,
typing goes straight into the answer fields, `ctrl-g` opens a field in
your editor (`VISUAL` or `EDITOR`), `n` adds a note, and the
last tab reviews everything before you submit or reject. `?` shows every
key. `q` quits; your answers so far are kept as a draft.

Questions keep to a width of 100 columns. Context the asker attached, an
image, highlighted code, rendered Markdown, text in the colours of its
escape sequences or a diff, shows in a pane beside the question on a
wide terminal and below it on a tall one;
`--layout beside` or `--layout below` (or `ASQR_LAYOUT`) fixes where it
goes. `pgup`/`pgdn` scroll the pane by a page less 2 lines from anywhere and
`ctrl-u`/`ctrl-d` by half a page, `tab` moves into it to scroll with
the arrow keys and back out, and `z` shows it full screen.

`asqr wait` exits 0 for a submitted session, 10 when you rejected it, 11
for an invalid file, 12 when its `--timeout` ran out and 14 when the
asker took the session back with `asqr withdraw`, so scripts can branch
on it. `asqr --help` lists every command.

Sessions wait in a queue: a directory with an inbox, an outbox for the
results, drafts and an archive. The default queue lives in the
platform's data directory; `--queue <name>` or `ASQR_QUEUE` keeps
projects apart, and `asqr paths` shows where everything is.
Among them is asqr's log file, and `ASQR_LOG=debug` adds debug lines to
it.

Inside tmux, asqr and the agents in the same tmux session use one queue
without any setup: the one asqr pinned in the session, at first the one
named after the session, so several tmux sessions keep their questions
apart. `asqr --queue <name>` in a session pins another queue there.
`asqr queues` lists every queue with its waiting and answered sessions
and the asqr watching it, with that asqr's tmux session. In asqr, `w`
switches to another queue without a restart, and the status line counts
the sessions waiting in queues no asqr watches. Outside tmux, `asqr`
with several queues and none chosen asks which one to watch first.

## For agents

asqr comes with a skill that teaches an agent the format, the waiting
pattern and the exit codes. An agent you tell to use asqr can read it
from the installed binary with `asqr skill`, and that copy always
matches the binary, so in most cases nothing needs installing. The skill
has a core and three parts, which `asqr skill <part>` prints and the
core names where they are needed. `asqr --help` tells agents to read
the skill first, and where agents tend to go wrong, asqr adds a hint on
stderr.

    asqr skill --install .claude/skills

installs the skill as `asqr/SKILL.md`, with its parts in
`asqr/references/`, under the skills directory you name. An installed
copy stays as it is when asqr is updated; running the install again
replaces it, parts included. `asqr ask` and `asqr validate` print a hint
when they find an installed copy that is older than asqr, in the skills
directories of Claude Code, Codex, Gemini CLI, GitHub Copilot and Cursor,
in the project up to the git root and in your home directory, and in
the directories that `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `COPILOT_HOME`
or `GEMINI_CLI_HOME` name when they are set. The hint
tells the agent to read the current skill with `asqr skill` and to ask
you whether to update the copy.

## Inside tmux

Inline images and desktop notifications only reach the terminal around
tmux when tmux passes escape sequences through:

    set -g allow-passthrough all

Without it, asqr shows images as coarse blocks and rings the bell
instead of sending a notification. With `on` instead of `all`, tmux
drops what asqr sends while its window is in the background, and an
image that arrives then stays empty until asqr gets the focus again,
which it only learns from tmux with `set -g focus-events on`. In kitty
and Ghostty, with tmux 3.4 or later, asqr sets `all` for its own pane
while it runs and puts the pane's setting back when it quits.

## Known limitations

- One asqr per queue, on one machine: queues on network or synced
  filesystems (NFS, Dropbox, iCloud Drive) are not supported.
- macOS and Linux. Windows may work but is not tested.
- The terminal needs at least 60×15 cells.

## Documentation

The full design is in [`docs/spec.md`](docs/spec.md), the
JSON Schemas are in [`schema/`](schema/), and example sessions are in
[`examples/sessions/`](examples/sessions/).

## Development

Development is test first, and `just check` is the validation pipeline:
license headers, `cargo fmt --check`, clippy and rustdoc with
`-D warnings`, tests and doctests. `just coverage` reports test
coverage; CI runs both on Ubuntu and macOS. After cloning, run
`just setup` once: it makes the pre-commit
hook in `.githooks/` run the pipeline before every commit, and names any
tool that is missing (`just`, `cargo-llvm-cov`, `llvm-tools-preview`).

The TUI is tested through its state with `insta` snapshots and end to
end in a pseudo-terminal, not by hand. [`docs/spec.md`](docs/spec.md)
describes behaviour: it names no source files, functions or types, and
the implementation follows the spec, not the other way round.

## Decisions

Architecture decisions are recorded in [`docs/adr/`](docs/adr/) and
managed with [adrs](https://github.com/joshrotenberg/adrs).

## License

[Mozilla Public License 2.0](LICENSE). Every source file carries the
license notice in its header.
