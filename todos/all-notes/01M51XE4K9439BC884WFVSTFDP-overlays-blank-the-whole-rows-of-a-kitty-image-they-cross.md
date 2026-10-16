---
title: "Overlays blank the whole rows of a Kitty image they cross"
kind: bug
component: tui
impact: high
horizon: next
origin: implementation
tags: [ux, field-test]
---
# Overlays blank the whole rows of a Kitty image they cross

Seen by Jakob on 2026-10-16 in Ghostty under tmux, release build of
`9b22439`: while the help (`?`) or the intro (`i`) is open over a
context image, the image is gone across its full width in every row
the overlay covers, not only under the overlay. It comes back when the
overlay closes. The session list (`L`) is an overlay too.

## Problem

Under the Kitty protocol ratatui-image draws an image as cells of the
placeholder `U+10EEEE`. Only the first cell of each image row carries
the row, column and id diacritics; every other cell is a bare
placeholder that inherits its position from the placeholder to its
left (`render` in ratatui-image 12.0.0-rc.2, `src/protocol/kitty.rs`,
the `if x == 0` branch, pinned by its test
`only_the_first_cell_of_a_row_states_a_position`). An overlay writes
its own cells over some of those. A bare placeholder whose left
neighbour is no longer a placeholder has nothing to inherit from, and
the terminal draws nothing there. Both screenshots had the overlay
start left of the image, so it covered every row's first cell; an
overlay starting inside the image blanks everything right of it.

## Impact

Every look at the help, the intro or the session list while an image
shows hides most of the image, and it looks broken.

## Proposal

After the overlays are drawn, give every placeholder cell of an image
whose left neighbour is not a placeholder of the same image its row,
column and id diacritics, computed from the image's area. Only cells
after a break pay the extra bytes. Write the failing test first: an
overlay over the first cell of an image row leaves the cells right of
it without a position.

An option in ratatui-image to write the position on every cell would
also fix it; a pull request there needs Jakob's go.
