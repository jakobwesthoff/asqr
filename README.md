# asqr

> Work in progress. Nothing here works yet; this README describes the
> intent, not the current state.

asqr (pronounced "asker") is a terminal UI for answering questions that
another program puts in front of you. A script, a build or an AI coding
agent writes a question file into an inbox. asqr, running in a terminal of
its own, picks it up and lets you work through the questions. When you
submit, it writes your answers into a result file that the asking program
reads back.

## Planned

- Any number of questions per file.
- Single and multiple choice, with any number of options.
- A custom answer instead of the given options, and an optional note on
  every answer.
- Images next to a question, shown inline in terminals that support it.
- A documented, versioned JSON format for questions and answers, so any
  tool can ask.

## Decisions

Architecture decisions are recorded in [`docs/adr/`](docs/adr/) and
managed with [adrs](https://github.com/joshrotenberg/adrs).
