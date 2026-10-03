# Drop the patched ratatui-image once upstream releases the fix

Status: waiting on upstream since 2026-10-03; the user asked for this
todo.

## Why the patch exists

Images were cropped when the terminal's cell size differed from the one
asqr detected at start: after a font size change, or in tmux with
clients of different cell sizes. `ratatui-image` created the Kitty
virtual placement without `c=`/`r=`, so the terminal sized it from its
own cells instead of the placeholder cells. Confirmed in Ghostty on
2026-10-03, with and without tmux, and fixed in commit `f8799a5`.

The fix is upstream PR
<https://github.com/ratatui/ratatui-image/pull/219> (branch
`kitty-placement-size` on `jakobwesthoff/ratatui-image`, based on
`master`, which is 12.0.0-rc.0 at the time). asqr uses a backport onto
v11.1.0, branch `kitty-placement-size-v11`, rev
`69e02fc2e8e68d9e82ee950624b41de70bc89a68`, through
`[patch.crates-io]` in `Cargo.toml`.

## The catch until then

`cargo publish` drops `[patch]`: the packaged crate resolves
`ratatui-image` 11.1.0 from crates.io (checked with `cargo package`).
An asqr release on crates.io ships without the fix, while git and
local builds have it.

## What to do

- Check whether PR 219 is merged and in a `ratatui-image` release.
- If so: bump `ratatui-image` to that release with `cargo add`, remove
  the `[patch.crates-io]` section and its comment from `Cargo.toml`, and
  run the tests. `a_kitty_image_is_placed_over_exactly_its_placeholder_cells`
  in `src/tui/render/screen.rs` must stay green without the patch. A
  12.x release may change the `Picker` API that `src/terminal.rs` and
  `src/tui/render/images.rs` use.
- If the PR stalls or is rejected, decide whether to publish asqr with
  the known crop bug, or carry the fix another way.
- Then delete the `kitty-placement-size-v11` branch from the fork.
