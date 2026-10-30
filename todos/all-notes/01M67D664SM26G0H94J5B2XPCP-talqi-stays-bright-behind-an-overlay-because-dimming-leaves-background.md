---
title: "Talqi stays bright behind an overlay because dimming leaves background colours as they are"
kind: bug
component: tui
status: needs-discussion
origin: review
tags: [ux]
---
# Talqi stays bright behind an overlay because dimming leaves background colours as they are

When an overlay opens (help, session list, intro, queue switcher), asqr
dims the screen behind it (ADR 84). The mascot Talqi on the empty queue
is drawn mostly with background colours, which the dimming leaves as
they are, so he stays nearly bright behind every overlay on a terminal
tall enough to show him. Found on 2026-10-30 in the settings design
study (conductor logbook, `plans/settings-design/png/d1-6-wide.png`,
rendered from mockup text, not from asqr); not yet checked in a real
terminal.

## Problem

- `dim_behind` (`src/tui/render/dim.rs:18-34`) inserts
  `Modifier::DIM` (SGR 2) into every cell behind the overlay, takes the
  focus fill off, and leaves the cells of a Kitty image as they are.
  Terminals apply SGR 2 to the foreground; the background colour of a
  cell is drawn as it is.
- Talqi's emblem is ANSI art with RGB foreground and background colours
  (`src/tui/render/mascot/template.rs:160-180`, the `talqi-colour-*.ansi`
  files), shown when the main area has room for the emblem's 22 rows
  plus the sentence (`src/tui/render/empty.rs:102`). Its half and
  quadrant blocks take much of their colour from the background.
- The same holds for every other background fill behind an overlay, such
  as a diff's changed lines and the submit button, which `dim.rs`
  leaves to SGR 2.

## Idea to examine (Jakob, 2026-10-30)

Replace the colours of the dimmed cells by a dimmed greyscale of their
brightness, foreground and background alike, instead of relying on SGR
2. Open: how a named ANSI colour or the terminal's default colour
(whose RGB value asqr does not know) gets a brightness, what terminals
without 24-bit colour get, and whether Kitty image placeholders keep
their style as now.
