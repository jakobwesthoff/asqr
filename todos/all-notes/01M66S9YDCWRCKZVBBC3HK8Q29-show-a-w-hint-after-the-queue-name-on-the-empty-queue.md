---
title: "Show a (w) hint after the queue name on the empty queue"
kind: improvement
component: tui
status: needs-discussion
horizon: someday
origin: discussion
tags: [ux]
---
# Show a (w) hint after the queue name on the empty queue

While the queue switcher was designed (2026-10-30), Jakob suggested that
the empty queue's screen show `(w)` right after the queue name, perhaps
in yellow, so a person who waits for sessions sees at once that `w`
switches the queue. In the asqr session on the switcher's colours
(01M669B7HC7HZVRJABE6QK3MFR) he set it to "later, keep open"; the
switcher landed without it.

## Context

- The empty queue's key bar names the key since `1e6c94c`:
  ` w  queues   q  quit`. Whether a second hint next to the name still
  adds something is part of the question.
- Lead view at design time: draw `(w)` in the key style
  (`theme::key`, bold primary) rather than yellow, since yellow means
  focus or warning in asqr's roles (ADR 0037).
- Variants drawn then, kept in the logbook of that run
  (`~/playground/2026_10_30_asqr-todos-worked-off-by-agents/plans/instances/`):
  `switcher-color-9-empty-at-rest-no-hint.ans`, `-hint-title-key.ans`,
  `-hint-title-yellow.ans`, `-hint-sentence-key.ans`,
  `-hint-sentence-yellow.ans`, with the trade-offs in
  `switcher-color-notes.md` (C5): in the title line
  `asqr · queue: asqr-todos (w) · 0 sessions waiting`, or in the
  sentence `Nothing to answer in queue asqr-todos (w).`, where the bold
  cyan name and the bold cyan `(w)` run together. Under `NO_COLOR` the
  hint keeps only its text.

## Open questions

- Hint at all, given the key bar? If so: title line or sentence, key
  style or yellow.

## Revisit when

No trigger known; reconsider at the next sweep.
