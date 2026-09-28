# The first question's image is not shown

Status: reported by the user on 2026-09-28; not investigated yet.

## What the user saw

A session of seven questions, the first six each with an absolute
`image` path (session `01M3KQ69K5WC621SKQNWGSY514`, file in the asqr
logbook at
`~/playground/2026_09_27_asqr_question_tui/logo/choose-session.json`).
The user's words in the reject reason:

> it seems the first question doesnt display its assigned image

The other questions showed their images. The first image is a valid
2048×2048 PNG of 2.1 MB; questions 3 and 5 carry PNGs of the same size
(3.9 and 2.7 MB), so size alone does not explain it. The session arrived
while asqr was already running. Terminal: the user's usual one
(Ghostty); whether inside tmux is not known.

## What is known

- Images load on first display and are cached per path.
- Whether the image stays missing when the user moves to another tab and
  back is not known; ask or reproduce first.

## Hypotheses (unverified)

- The first question is drawn in the same frame the session arrives in,
  and the image's escape sequence is written before, or overwritten by,
  a full redraw that follows the arrival (notice, title bar count).
- Same family as
  `01m3kgxh7f2sns4hf0zhzwnqjv-image-vanishes-in-narrow-terminals.md`:
  the Kitty placement is not redrawn when the cells around it change.

## To investigate

- Reproduce: start asqr, drop a session whose first question has an
  image, and compare with a session dropped before asqr starts.
- Move away from the first tab and back: does the image appear then?
- Check whether forcing a redraw after an arrival fixes it.
