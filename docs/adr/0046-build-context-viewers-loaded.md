# 46. Build context viewers loaded

Date: 2026-10-16

## Status

Accepted

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amends [45. Keep image handling in a graphics module around an asqr-owned value](0045-keep-image-handling-in-a-graphics-module-around-an-asqr-owned-value.md)

Amended by [49. Present problems through one component in three forms](0049-present-problems-through-one-component-in-three-forms.md)

## Context

- ADR 31 split a viewer's start into two steps: the viewer was built
  cheaply when its entry was first shown, and `load` then prepared what
  does not depend on the body's size. The pane decided when to load,
  which left room to load ahead of time.
- The architecture review of 2026-10-16 found that the only caller
  loaded each viewer right after building it. Each viewer kept its
  content as an `Option` that was empty between the two steps, and laying
  out or drawing one that was not loaded panicked. The image viewer kept
  a copy of what makes images for its whole life only to decode its file
  in `load`.
- No loading ahead of time is planned (the lead's decision under the
  user's delegation, 2026-10-16).

## Decision

- Building a viewer is loading it: the viewer's constructor reads its
  file, decodes its image, highlights its text or computes its diff, and
  the viewer holds its content from the start. The `load` step goes.
- A viewer is still built when its entry is first shown, and a failure
  is still content: a file that cannot be read shows the placeholder it
  showed before.
- The room for loading ahead of time that ADR 31 kept is given up. A
  load step can be brought back if such loading is ever needed.
- Jakob asked on 2026-10-16 why a decoded image is kept in memory; the
  lead decided under his delegation: an image keeps the bytes of its
  file as read and the format they were decoded in, and holds the
  decoded picture only until its first encoding. Each later encoding,
  for another size, decodes the bytes again.

## Consequences

- No viewer can be laid out or drawn before its content exists, so the
  panic for a viewer that was not loaded is gone.
- The image viewer holds its image or its placeholder and nothing for
  decoding. The image encodes for the body's size when it is laid out,
  as ADR 45 has it.
- A shown image holds its file's bytes and its encoding, not its
  decoded picture, which takes four bytes a pixel: 144 MB for one of
  6000×6000. A new size costs a decoding again. An image file changed
  on disk keeps showing what asqr read first.
- What the person sees does not change: the same content, the same
  placeholders and the same moment of reading, the first time an entry
  shows.
