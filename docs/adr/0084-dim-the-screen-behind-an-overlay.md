# 84. Dim the screen behind an overlay

Date: 2026-10-30

## Status

Accepted

Amends [37. Colour the TUI by roles](0037-colour-the-tui-by-roles.md):
the screen behind an overlay is dim and shows no focus fill

Amended by [88. Draw Talqi as his outline behind an overlay](0088-draw-talqi-as-his-outline-behind-an-overlay.md):
the dimming takes the fill off the submit button and a diff's changed
lines as well as the focus fill

## Context

- The help, the intro, the session list and the dialog of an invalid
  session are drawn over the screen. `popup` clears only the frame's
  own cells; the screen behind keeps its colours and its focus fill.
- The session list fills its current entry. With the row under the
  cursor filled behind it, two focus fills show at once, while the
  design guide allows one at a time.

## Decision

- Behind every overlay and behind the dialog of an invalid session,
  every cell above the status line is dim, and a cell with the focus
  fill loses it. Other fills, such as the submit button, stay, dim.
- The status line and the key bar are not dimmed: they show the
  overlay's keys and notices. An overlay that reaches the status line,
  as a long intro does, covers it with its own cells.
- The cells of a Kitty image keep their style, since a placeholder's
  colour names its image.
- Under `NO_COLOR` the screen behind is dim as well.

## Consequences

- One focus fill shows at a time, also while an overlay is open.
