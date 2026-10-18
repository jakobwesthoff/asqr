# 32. Let an image entry opt out of enlarging

Date: 2026-10-14

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md)

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

## Context

Every image scales to the area it gets, keeping its proportions, in the
context pane as in the `z` view (ADR 31). A small image fills the pane,
so the person sees more of it. For some images the pixel size is the
point: pixel art, a screenshot whose size is what the question is about,
an icon at its real size. Scaled up, they do not show what the asker
wants the person to judge.

## Decision

- An image entry takes an optional `enlarge`, default `true`. With
  `false`, the image keeps its natural size where it fits and only
  shrinks when it does not, in the pane and in the `z` view alike. `o`
  still opens it in the system viewer.
- The field is part of format version 2. A value that is not a boolean
  makes the file invalid, as for any other field of the wrong type.
- The skill describes the field and tells agents to use it sparingly,
  only where the exact pixel size matters, since for most images the
  person is better served by seeing them larger.
- The image viewer takes the flag from its entry when it is built, so
  the viewer interface of ADR 31 stays as it is.
- The field is written back only when it is `false`, so `asqr ask` adds
  nothing to an image entry that leaves it out. The schema still names
  `true` as the default.

## Consequences

- An asker can show an image at its real size; the pane then leaves
  room around a small image.
- Natural size means the image's pixels at the cell size asqr learns
  from the terminal at start, so it depends on the terminal's font, as
  any image on a terminal does.
