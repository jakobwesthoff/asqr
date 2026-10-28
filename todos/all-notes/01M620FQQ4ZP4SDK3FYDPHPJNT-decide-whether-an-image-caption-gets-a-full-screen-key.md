---
title: "Decide whether an image caption gets a full-screen key"
kind: investigation
component: tui
status: needs-discussion
horizon: someday
origin: idea
tags: [ux]
---
# Decide whether an image caption gets a full-screen key

An idea of the user's from 2026-10-28: a key such as `c` that shows the
caption of an image context entry full screen, as `z` shows the image.
It is not decided whether asqr should have it at all. The user parked it
the same day as a question to discuss, not as work to do.

## Where captions stand

Decided on 2026-10-28 while building the caption (format version 2, an
optional Markdown `caption` on image entries, inline text only, images
only):

- Terminal graphics protocols draw an image only whole, never partly
  scrolled off, so image and caption cannot scroll together as one page.
  Ten layouts were rendered as mockups before the choice.
- Chosen layout "Fit": the image is laid out in the rows the caption
  leaves, so the whole caption shows under it and nothing scrolls. When
  the caption alone is taller than the pane body, the caption scrolls.
  Below a minimum image height the pane shows the caption alone with a
  line saying the image shows with `z`.
- `z` from a shown pane shows the image alone at full size. From the
  one-line hint below the pane threshold, `z` shows image and caption
  laid out by the same rule, so the caption is reachable there too.
- The skill tells agents to keep a caption to one or two lines with only
  the essentials, and to put a full prompt or a list of notes into the
  question's text or another context entry, since a long caption makes
  the image smaller.

## For and against

- For: a long caption on a small terminal scrolls in a few rows, and a
  full-screen view would read more comfortably.
- Against: the caption is always reachable without it, and the skill
  steers agents away from long captions. Every key adds to the key maps
  of pane, question and full-screen view, the key bar, the help and the
  spec. `c` is not checked for conflicts yet.

## Open questions

- Is the key needed at all, given Fit and short captions?
- If so: which key, what it shows (caption alone, or caption with a
  small image), and how it goes back (`c`, `esc`).

## Revisit when

Long captions turn out to be hard to read in practice, for example
agents writing long captions despite the skill, or small terminals
where the caption scrolls in a few rows.
