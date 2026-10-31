---
title: "Tell agents in the skill that Markdown the parser fails on shows as plain text"
kind: docs
component: project
origin: review
---
# Tell agents in the skill that Markdown the parser fails on shows as plain text

Since ADR 98 and ADR 99, asqr shows a Markdown entry or caption that
makes the `markdown` parser panic as plain text below the dim note
"Markdown failed: shown as plain text", and question texts, option
labels and descriptions the parser panics on as written. The skill says
nothing about it (checked 2026-10-31: `skills/asqr/references/context.md`
names the 1 MiB plain-text rule and the 50,000-line highlight limit, not
this one), so an agent does not learn that its text can arrive plain.

## What to do

- Add a sentence to `skills/asqr/references/context.md`, beside the
  1 MiB rule: what the person then sees, and that a short, plain Markdown
  structure avoids it. State only what the code does (ADR 98, ADR 99,
  spec 7.10 and 8.7); no promise about which inputs fail.
- Check whether the core needs anything (ADR 82: the core holds the
  rules no check catches; this is a detail of one entry type, so a
  reference part is the expected place).
- Keep the reference part under its 30,000-byte cap; the skill tests
  pin structure and key phrases.

## Relations

- Relates to: [Warn at ask and validate time when Markdown cannot be rendered](01M67YPSXEEYD0AWYQW8R57E4X-warn-at-ask-and-validate-time-when-markdown-cannot-be-rendered.md)
