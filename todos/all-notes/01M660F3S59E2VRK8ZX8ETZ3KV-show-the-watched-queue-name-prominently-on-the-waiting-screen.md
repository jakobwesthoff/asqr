---
title: "Show the watched queue name prominently on the waiting screen"
kind: improvement
component: tui
status: needs-discussion
horizon: next
origin: request
tags: [ux]
---
# Show the watched queue name prominently on the waiting screen

Requested by Jakob on 2026-10-30. While no session waits, asqr shows
Talqi's emblem and the sentence "Nothing to answer. New sessions show
up here as soon as they arrive." The queue asqr watches, the name given
with `--queue` (or `ASQR_QUEUE`), appears only in the title bar as
`asqr · queue: default · 0 sessions waiting`. With several asqr
instances on different queues, the person cannot tell at a glance which
one a window watches. The name should show more clearly on the waiting
screen, centred near the emblem.

## Where it is today

- `src/tui/render/empty.rs`: `EmptyQueue` draws the emblem
  (`emblem_screen_areas`, shown when the main area holds 26 rows and 44
  columns) and the centred `SENTENCE`; it knows nothing of the queue.
- The title bar names the queue (snapshot
  `asqr__tui__render__screen__tests__an_empty_queue_says_so.snap`).
- Spec section 7.5 and ADR 43 describe the empty queue.

## Open questions

- Where the name shows: above or below the emblem, inside the sentence,
  as a framed label, in a larger rendering (block letters), or in the
  queue's own colour.
- What shows for the default queue and for a queue given with `--dir`
  (a directory, not a name).
- What shows on a screen too small for the emblem.

Jakob asked for rendered design ideas to discuss before any code
(2026-10-30).

## Decisions (Jakob, 2026-10-30, asqr session 01M6628SFR6RSZNB8DP4YNGBQH)

- None of the first eight rendered variants is chosen. Name in the
  sentence was liked most but makes the sentence too long; a rule with
  the name set into it was a bit too pushy; the name as a headline gave
  too little context. A second round of variants follows.
- The name shows only when a specific queue is chosen; for the default
  queue the screen stays as it is.
- On a screen too short for both, Talqi gives way and the name stays.
- A `--dir` queue shows its path for now. Still open: the path may be
  too long, to be discussed later.
- The terminal tab title names the queue too, as
  `asqr · asqr-todos · 2 waiting`.

## Relations

- Relates to: [Let several asqr instances run side by side and route each agent to the right one](01M63MZBJYJ1ANX77CW5QAR9AG-let-several-asqr-instances-run-side-by-side-and-route-each-agent-to-the.md)
