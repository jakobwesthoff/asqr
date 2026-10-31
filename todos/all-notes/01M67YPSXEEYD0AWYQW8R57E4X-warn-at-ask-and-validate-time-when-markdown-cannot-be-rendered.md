---
title: "Warn at ask and validate time when Markdown cannot be rendered"
kind: improvement
component: format
origin: implementation
---
# Warn at ask and validate time when Markdown cannot be rendered

Proposed on 2026-10-31 while fixing todo 01M67QJJZMZ5S055T5BJJG1MQZ (a
Markdown entry that makes the `markdown` parser panic ends asqr). That
fix makes the TUI show such text plain with the dim note "Markdown
failed: shown as plain text", so asqr no longer ends, but the agent that
sent the text never learns of it.

## Proposal

`asqr ask` and `asqr validate` already warn about a Markdown entry above
1 MiB (spec 3.4). They could also run the Markdown rendering once, with
the panic caught, over every Markdown text of the session (Markdown
entries, inline Markdown, image captions, question texts and option
labels) and warn when it fails, naming where, so the agent can fix its
text before the person sees it plain.

## Open points

- Cost: rendering every Markdown text at `ask` time adds work to a
  command agents call often; measure it on large sessions.
- Whether the warning is worth it once the `markdown` crate fixes the
  panics found on 2026-10-31 (inputs such as `1. ~~~\n*`), which the
  robustness work drafts an upstream report for.
