# 67. Show the Kitty placeholder character in text as a replacement character

Date: 2026-10-28

## Status

Accepted

## Context

Under the Kitty graphics protocol asqr places an image through cells of
the character U+10EEEE, whose foreground colour names the image: the low
three bytes of its id as an RGB colour, the high byte as a mark after
the character (ADR 48).

Text from a session reaches the screen through many paths: the title,
the intro, question texts, labels and descriptions, the entries of the
context pane, the session list, notices that name a session, the dialog
of an invalid session, and what the person types. None of them kept
U+10EEEE out. A code entry's highlighting or an `ansi` entry's colours
can draw the character in a colour that names an image, which may lead
Kitty to draw image cells where the text is. This is reasoned from the
protocol and was not reproduced in Kitty.

## Decision

Once everything of a frame is drawn, one pass over it replaces U+10EEEE
with U+FFFD in every cell that is not a cell of the Kitty image the
frame drew. A cell of that image lies inside its area, in the colour
that names its id, and holds the character, which is how the frame
recognises the image's cells when it mends them (ADR 48). The pass runs
under every protocol and changes only what is drawn: answers, notes and
the result keep the character as written.

The replacement is made without a reproduction in Kitty: it is harmless
and cheap, and a reproduction needs a real Kitty.

## Consequences

- Every frame looks at each of its cells once more.
- A text cell inside the image's area, in exactly the colour that names
  the image and holding U+10EEEE, counts as a cell of the image and keeps
  the character.
- Both characters take one column, so a replaced cell keeps the cells
  after it where they were.
