---
title: "Keep Kitty image ids clear of the probe ids 31 to 33"
kind: bug
component: tui
impact: low
origin: review
---
# Keep Kitty image ids clear of the probe ids 31 to 33

asqr gives each Kitty image a random 32-bit id, and nothing stops that
id from being 31, 32 or 33, the ids ratatui-image uses for its
capability probe. Found on 2026-10-31 while preparing the todo on two
alternating Kitty ids (closed that day, fresh ids stay).

## Problem

`Ids::issue` (`src/graphics/kitty.rs`) draws again while the id is 0 or
was handed out before, but accepts 31, 32 and 33. ratatui-image
12.0.0-rc.3 probes with `i=31`, `i=32` and `i=33` (`src/picker/cap_parser.rs`
in the crate, lines 181-208), and asqr's own test answer uses 31
(`src/graphics/detect.rs:590`). Read in the code, not reproduced. The
chance is about 3 in 2^32 per id, and whether a clash does any harm is
not checked: the probe queries with `a=q`, which the Kitty protocol does
not keep as an image.

## Suggested fix

Draw again in `Ids::issue` for 31, 32 and 33 as it does for 0, with a
test that feeds those ids through a fixed `IdSource`
(`src/graphics/testing.rs`, `with_ids`).
