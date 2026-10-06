# VHS demo GIF for the README

Status: idea, deferred by the user on 2026-09-27 ("create a todos file
about the vhs idea but do not do that now").

## Idea

Show asqr in action at the top of the README with an animated GIF,
recorded with [VHS](https://github.com/charmbracelet/vhs). VHS renders a
terminal session from a `.tape` script, so the GIF is reproducible: the
tape lives in the repo, and a changed UI means re-running the tape, not
re-recording by hand.

## What was considered

Offered in the version 1 planning round next to an asciinema cast and a
static Ghostty screenshot. An asciinema cast cannot show the inline
images; a screenshot shows no interaction.

## Notes for when it is done

- A tape such as `docs/demo/demo.tape` with a session file from
  `examples/`, answering a few questions and submitting.
- VHS runs its own terminal; check whether it renders the Kitty graphics
  protocol for images, otherwise the demo shows the block fallback.
- A CI job could re-render the GIF and fail on differences; not decided.
