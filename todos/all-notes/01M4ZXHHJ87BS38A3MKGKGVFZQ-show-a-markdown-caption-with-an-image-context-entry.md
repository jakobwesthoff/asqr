---
title: "Show a Markdown caption with an image context entry"
kind: feature
component: tui
status: needs-discussion
origin: discussion
tags: [ux]
---
# Show a Markdown caption with an image context entry

Came up on 2026-10-15 while an agent presented mascot drafts through
asqr: each option showed a generated image, and the person needed the
prompt that produced it and the agent's notes on it while looking at the
image. An image entry carries only a title, so the text went into
separate questions and the person never saw a draft next to its prompt.
A Markdown caption on the image entry, shown together with the image in
the context pane, keeps the two together (user, 2026-10-15).

## Goal

An image context entry carries a caption of Markdown, as long as a
prompt of several paragraphs or a list of notes, and the context pane
shows it together with the image. The caption is part of the image entry,
not a second item to switch to.

## Proposal

- Format: an optional `caption` on `{ "type": "image" }` entries,
  rendered like the `content` of a `markdown` entry (CommonMark).
- Pane: image and caption share the pane under one header line.

## Open questions

- Layout: caption below the image, beside it on a wide pane, or the
  image scaled down to leave room; how the height is split, and whether
  the caption scrolls on its own or together with the image.
- Full screen (`z`): image and caption, or the image alone.
- Inline content only, or also a path to a Markdown file, as `markdown`
  entries allow.
- An optional field within format version 2, or a version bump.
- Whether code and diff entries want a caption as well.

## Relations

- Relates to: [Show several context items per question in the context pane](01M4XFJG5ZYVHJ12G5NJA5970W-show-several-context-items-per-question-in-the-context-pane.md), several items shown one at a time, while a caption keeps one image and its text together
