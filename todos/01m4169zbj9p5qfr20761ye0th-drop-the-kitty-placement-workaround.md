# Drop the Kitty placement workaround once ratatui-image has the fix

Status: waiting for a `ratatui-image` release since 2026-10-03; the user
asked for this todo.

## Why the workaround exists

Images were cropped when the terminal's cell size differed from the one
asqr detected at start: after a font size change, or in tmux with
clients of different cell sizes. `ratatui-image` 11.1.0 creates the
Kitty virtual placement without `c=`/`r=`, so the terminal sizes it from
its own cells instead of the placeholder cells. Confirmed in Ghostty on
2026-10-03, with and without tmux.

Upstream fixed it in PR
<https://github.com/ratatui/ratatui-image/pull/215> (commit `2d7d7f6`,
merged 2026-10-03 15:41 UTC). No release contained it on that day; the
newest was 12.0.0-rc.0, which predates the merge. Our own PR #219 with
the same fix was closed as a duplicate, as #215 had been open since
2026-10-01 and we had not searched for it.

A `[patch.crates-io]` pointing at a fork would fix local builds only, as
`cargo publish` drops patches (checked with `cargo package`). So asqr
0.9.2 carries the workaround `size_kitty_placement` in
`src/tui/render/images.rs`: after the image widget has drawn, it adds
`c=`/`r=` to the placement command in the buffer. It relies on the exact
text `ratatui-image` writes, so `Cargo.toml` pins the crate to
`=11.1.0`. The function's doc comment explains the details.

## What to do

- Check whether a `ratatui-image` release contains commit `2d7d7f6`
  (likely 12.0.0 or a later 12.x).
- If so, follow the removal steps in the doc comment of
  `size_kitty_placement`: bump the crate with `cargo add` and drop the
  `=` pin, delete the function, its constants and its call, and run the
  tests. `a_kitty_image_is_placed_over_exactly_its_placeholder_cells`
  in `src/tui/render/screen.rs` must stay green without the workaround.
  A 12.x release may also change the `Picker` API that `src/terminal.rs`
  and `src/tui/render/images.rs` use.
- Until then, a bump of `ratatui-image` has to check the command text
  the workaround edits.
- The fork `jakobwesthoff/ratatui-image` and its branches
  `kitty-placement-size` and `kitty-placement-size-v11` are no longer
  used and can be deleted.
