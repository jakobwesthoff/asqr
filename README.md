# asqr

**Your agent saved up some questions. Answer them all in one go, right in
your terminal.**

> Work in progress. Nothing here works yet; this README describes the
> intent, not the current state.

asqr (pronounced "asker") makes it easy to answer questions that an AI
coding agent has prepared for you. The agent writes its questions into a
file, asqr shows them in a terminal of its own, and you work through them
at your own pace. When you submit, asqr writes your answers into a result
file that the agent reads back. Scripts and builds can ask the same way.

## Planned

- Any number of questions per file.
- Single and multiple choice, with any number of options.
- A custom answer instead of the given options, and an optional note on
  every question.
- Optional images next to a question, shown inline in terminals that
  support it.
- A documented, versioned JSON format with a JSON Schema, so any tool can
  ask.
- A skill that teaches agents how to use asqr, built into the binary.

The full design is in [`docs/spec.md`](docs/spec.md).

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
