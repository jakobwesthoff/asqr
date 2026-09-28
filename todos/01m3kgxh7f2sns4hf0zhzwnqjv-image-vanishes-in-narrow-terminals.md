# Image vanishes in narrow terminals

Status: found in the first real-world test on 2026-09-28; the user
wants it investigated later, not now.

## What the user saw

Test session 2 ("Test 2: images", question "Image B", a 1024×1024 PNG),
in the user's terminal (Ghostty; whether inside tmux is not known). The
user resized the terminal narrower than 100 columns and noted:

> it mostly got smaller then i once saw it shortly at the absolute bottom
> somewhere, but that never happened again and it then was simple not
> visible all in smaller terminals anymore.

Images beside the question (wide terminals), `z` full screen and `o`
worked; the missing-file placeholder showed.

## What is known

- Below 100 columns of content the image should take a strip under the
  question: a third of the height, at most 12 rows (spec section 7.7).
- The snapshot tests with the half-block protocol show the image in that
  strip, so the layout itself works. The difference is likely in the
  inline graphics path the real terminal uses (Kitty protocol through
  `ratatui-image` 11.1.0): after a resize the image is encoded again, and
  the escape sequence that sends it may sit in a cell the TUI does not
  redraw, or the old placement may not be cleared.
- The user suspects tmux may play a part (passthrough, section 7.7).

## To investigate

- Reproduce in Ghostty directly and inside tmux with
  `allow-passthrough on`: wide, then narrow, then wide again, and a
  fresh start in a narrow terminal.
- Check whether a fresh start at narrow width shows the image (then it
  is the resize path) or not (then the strip placement or its size).
- Check how `ratatui-image` handles a changed area for the Kitty
  protocol, and whether forcing a full redraw after a resize fixes it.
- A pseudo-terminal test cannot see Kitty images; this needs a real
  terminal or a terminal emulator library that understands the Kitty
  graphics protocol.
