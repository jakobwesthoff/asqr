---
title: "Reconsider two alternating Kitty ids for context images"
kind: improvement
component: tui
status: needs-discussion
horizon: someday
origin: discussion
tags: [ux]
---
# Reconsider two alternating Kitty ids for context images

An idea from the user on 2026-10-15, evaluated in two adversarial reviews
and not taken. It is recorded so it is not lost, but it is **not ready to
implement**: picking it up means discussing and weighing it again against
the design asqr has by then.

## The idea

asqr shows at most one context image per frame (`draw_pane` runs once per
frame; an option's image replaces the question's, ADR 28; the review
shows none). Instead of a fresh random Kitty id per encode plus deleting
every dropped id after the frame (ADR 42, built on 2026-10-15), each asqr
process would own two Kitty ids and send every new transmission,
including a re-send after a focus gain or a resize, to the id that is not
on screen. At most two images would ever live in the terminal, nothing
would blink (Ghostty and kitty replace an id's old data only when its new
transmission begins), and deletes would be needed only when asqr quits.
The pattern comes from Yazi (one id per process) and timg (two
alternating ids for animations).

## Why it was not taken (2026-10-15)

- **Wrong picture under tmux `on`.** With `allow-passthrough on`, tmux
  drops a transmission from a hidden pane. The id not on screen still
  holds an earlier picture, so the placeholders, now pointing at it, show
  that earlier picture: a wrong image instead of an empty one. With fresh
  ids the same drop leaves the area empty until the focus-gain re-send.
  For images under options (ADR 28) a wrong picture is worse than none.
  (Reviews: OR4 and FR4 in the logbook ledger.)
- **Re-sends while browsing.** Today the images of a session stay in the
  terminal while it waits, so going back to an image shows it at once.
  With two ids, browsing three or more images re-encodes and re-sends a
  full image on every move, which is slow over ssh.
- **The bookkeeping moves, it does not vanish.** Kept image viewers would
  need to know whether their id still holds their picture (a slot
  owner/generation check), and a resize needs a caller-side swap to the
  other id (ratatui-image re-encodes under the same id).
- **The built design works everywhere.** ADR 42's fresh ids plus deletes
  is implemented and tested, works with every tmux version, and the
  architecture review of 2026-10-16 proposes to simplify its bookkeeping
  further (one id book, exit deletes by drop order).

## What changed since, and what to weigh when this comes back

- The user decided that asqr sets `allow-passthrough all` for its pane
  under tmux 3.4 or later when the protocol is Kitty. There, hidden-pane
  transmissions are no longer dropped, so the wrong-picture objection
  holds only for tmux before 3.4.
- Reviewer F confirmed that on the terminal side (Ghostty, kitty) the
  scheme replaces without blinking, provided ids are unique per outer
  terminal (random per process, away from ratatui-image's probe ids 31
  and 32, below 2^24) and both ids are deleted on exit.
- Still open: the browse re-sends, the slot check, and whether simpler
  bookkeeping outweighs them. A decision needs a fresh look at the code
  and at which tmux versions still matter.

## Revisit when

tmux before 3.4 no longer needs support, or the id bookkeeping of ADR 42
causes problems in practice. Even then: discuss and weigh first, do not
implement from this note directly.

The reviews are in the logbook
`~/playground/2026_10_15_asqr-maskottchen/99-review-impl-ledger.md`
(rounds 3 and 5) and `99-research-image-libs.md` (Yazi, timg).
