---
title: "Lay out the question and context areas with Layout constraints"
kind: refactor
component: tui
impact: low
origin: review
---
# Lay out the question and context areas with Layout constraints

Found in a refactor review of the TUI's rendering on 2026-10-15.
`panes::areas` (`src/tui/render/panes.rs` l.111-170 at `a3768f6`) builds
the question and context rectangles by hand for each arrangement, with
`x`, `y`, `width` and `height` sums and `..area` updates. ratatui's
`Layout` does this from constraints, as the rest of the screen does.

## Proposal

- Beside: `Layout::horizontal([Length(question_width), Length(SEPARATOR),
  Fill(1)])`, the question width computed as today.
- Below and Hint: `Layout::vertical([Length(question_height), Fill(1)])`,
  the question then capped at the reading width as today.

## Done when

- The tests in `panes.rs` pass unchanged.
