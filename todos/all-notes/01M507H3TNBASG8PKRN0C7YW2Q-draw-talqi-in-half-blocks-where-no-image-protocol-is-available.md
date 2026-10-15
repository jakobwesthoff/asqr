---
title: "Draw Talqi in half blocks where no image protocol is available"
kind: feature
component: tui
origin: discussion
tags: [ux]
---
# Draw Talqi in half blocks where no image protocol is available

The empty queue shows Talqi, asqr's mascot, above its sentence in
terminals with an image protocol (ADR 39, spec section 7.1). In a
terminal without one, which includes tmux without passthrough, the
sentence stands alone: `Mascot::font` in `src/tui/render/mascot.rs`
returns `None` for the half-block picker, with a TODO naming this todo.
The user chose half blocks in the picture's own colours as the fallback
there.

## Decisions

- 2026-10-15, user: the picture is drawn as a real image through the
  terminal's image protocol, the way context images are. Half blocks in
  the picture's own colours are only the fallback when no image can be
  shown.
- 2026-10-15, user: the half-block fallback is computed at run time
  from the two WebP pictures built into asqr; nothing else is embedded.
- 2026-10-15, user: the half blocks keep their RGB colours under
  `NO_COLOR`, as the images do (ADR 15 amendment). The design guide's
  "Terminal interface" section names that exception for illustrations
  when it lands.

## Not ratatui-image's half blocks

ratatui-image 11.1.0 converts a picture to half blocks with `to_rgb8()`
(`protocol/halfblocks/primitive.rs` l.29), which drops the alpha
channel: the transparent background would show as coloured blocks.

## Suggested approach

From the implementation plan's step 7, agreed in its review:

- `Mascot::font` returns a cell of 1×2 pixels for the half-block
  picker, since a half block is one pixel wide and two tall. The
  existing `mascot_rect` then gives at most 44×22 cells, a picture of
  44×44 pixels, and the existing alpha-aware `scaled` scales it.
- `Mascot::draw` picks the path by `picker.protocol_type()`.
- A `HalfBlocks<'a>(&'a RgbaImage)` widget: a pixel counts as opaque at
  alpha 128 or more. Per cell, `▀` with the upper pixel as foreground
  and the lower as background when both are opaque, `▀` or `▄` with a
  foreground only when one is, and nothing when neither is. Colours as
  `Color::Rgb`.
- Tests: transparent pixels leave cells untouched; the `▀` and `▄`
  cases; opaque pixels give `Rgb` colours; a square source in a 44×22
  box gives 44×22 cells; `NO_COLOR` keeps the cells. The screen tests
  that assert an empty queue of 80×20 without any colour
  (`no_color_takes_the_colour_of_a_notice_over_the_empty_queue`) or
  without `Rgb` (`only_images_draw_in_rgb`, 120×30) draw with the
  half-block picker and will then see Talqi.
- Docs: the design guide names the exception; ADR 39 gets the fallback,
  including that terminals without 24-bit colour approximate its
  colours or show them wrong, as for context images; spec section 7.1
  drops "in terminals without an image protocol the sentence stands
  alone".
