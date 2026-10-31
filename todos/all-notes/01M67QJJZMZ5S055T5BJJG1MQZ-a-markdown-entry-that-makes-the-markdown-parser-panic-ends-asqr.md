---
title: "A Markdown entry that makes the markdown parser panic ends asqr"
kind: bug
component: tui
origin: implementation
---
# A Markdown entry that makes the markdown parser panic ends asqr

Found on 2026-10-31 by the viewer-limits agent while measuring the
Markdown viewer: an 8 MiB file made of concatenated local `.md` files
makes the `markdown` crate 1.0.0 panic at `src/to_mdast.rs:193`
("Cannot push to non-parent"). A Markdown entry above 1 MiB now shows
as plain text, which removes this input from the parser.
Whether some input below 1 MiB makes the parser panic is not known.

## Problem

The Markdown entry is laid out in a loading job
(`src/tui/render/loading.rs`). A job's panic is caught there, but the
drawing thread then panics with the job's message once the result
reaches it (`loader_panicked`, lines 433-444), so a parser panic ends
asqr and the person loses the screen. Read in the code; the panic below
1 MiB is not reproduced.

## Impact

One Markdown entry an agent sends could end the TUI while the person
answers. The draft survives, but the session cannot be shown.

## Suggested fix

1. Find a small input that makes `markdown` 1.0.0 panic (cut the 8 MiB
   sample down, or fuzz `to_mdast` briefly), and report it upstream.
2. In the Markdown job only, catch a parser panic and show the entry as
   plain text with a dim note, as above 1 MiB, instead of handing the
   panic on. Other jobs keep today's handling.
