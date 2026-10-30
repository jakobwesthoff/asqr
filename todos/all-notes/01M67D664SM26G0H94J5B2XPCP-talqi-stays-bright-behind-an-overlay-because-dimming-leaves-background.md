---
title: "Talqi stays bright behind an overlay because dimming leaves background colours as they are"
kind: bug
component: tui
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

## Decision (Jakob, 2026-10-30, in the chat)

**Behind an overlay Talqi is drawn as his colourless outline, and
`dim_behind` takes the fill off every reversed cell.** The outline is
the art asqr already draws under NO_COLOR (`src/tui/render/empty.rs:40`),
which SGR 2 dims fully on a dark or light theme, at any colour depth
and under NO_COLOR. Diff fills and the submit button then show as dim
coloured text. Rejected, compared as mockups (conductor logbook,
`plans/dimming-investigation.md` and `plans/dimming-*.png`):

- a dimmed greyscale of every cell's brightness: asqr does not know the
  RGB value of named ANSI colours and the terminal's default colour, so
  text turns near black on a dark theme and fills become dark blocks on
  a light one;
- greyscale only for RGB colours (Talqi): a dark block on a light theme,
  black or white on 16 colours, since asqr detects no colour depth;
- blending RGB backgrounds toward dark: fixes Talqi only.

## Technical notes (lead, 2026-10-30)

- `dim_behind` does not find Talqi in the buffer. `draw_screen`
  (`src/tui/render/screen.rs:391-401`) knows whether an overlay is open
  (`app.overlay()`) before it renders `EmptyQueue`, so it asks for the
  outline there, as it does for NO_COLOR (`no_color: view.no_color`);
  the field becomes one that means "draw the outline". `dim_behind`
  stays a rule over cells: it already takes the focus fill off, and
  takes REVERSED off every cell the same way.
- The invalid-session dialog hides Talqi already
  (`talqi: app.invalid_shown().is_none()`).
- ADR 84 gains the rule (a new ADR amending it); the overlay snapshots
  and `src/tui/render/dim.rs` tests change.
