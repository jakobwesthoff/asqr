# Drop the Kitty placement workaround once ratatui-image has the fix

Status: waiting on upstream since 2026-10-03; the user asked for this
todo.

## Why the workaround exists

Images were cropped when the terminal's cell size differed from the one
asqr detected at start: after a font size change, or in tmux with
clients of different cell sizes. `ratatui-image` 11.1.0 creates the
Kitty virtual placement without `c=`/`r=`, so the terminal sizes it from
its own cells instead of the placeholder cells. Confirmed in Ghostty on
2026-10-03, with and without tmux.

The proper fix is upstream PR
<https://github.com/ratatui/ratatui-image/pull/219> (branch
`kitty-placement-size` on `jakobwesthoff/ratatui-image`, based on
`master`, 12.0.0-rc.0 at the time). A `[patch.crates-io]` with a
backport onto v11.1.0 (branch `kitty-placement-size-v11`) fixed local
builds, but `cargo publish` drops patches (checked with
`cargo package`), so a crates.io release would have kept the bug.

asqr therefore carries the workaround `size_kitty_placement` in
`src/tui/render/images.rs`: after the image widget has drawn, it adds
`c=`/`r=` to the placement command in the buffer. It relies on the exact
text `ratatui-image` writes, so `Cargo.toml` pins the crate to
`=11.1.0`. The function's doc comment explains the details.

## What to do

- Check whether PR 219 is merged and in a `ratatui-image` release.
- If so, follow the removal steps in the doc comment of
  `size_kitty_placement`: bump the crate with `cargo add` and drop the
  `=` pin, delete the function, its constants and its call, and run the
  tests. `a_kitty_image_is_placed_over_exactly_its_placeholder_cells`
  in `src/tui/render/screen.rs` must stay green without the workaround.
  A 12.x release may also change the `Picker` API that `src/terminal.rs`
  and `src/tui/render/images.rs` use.
- If the PR stalls or is rejected, the workaround can stay; a bump of
  `ratatui-image` then has to check the command text it edits.
- Delete the `kitty-placement-size-v11` branch from the fork, as nothing
  uses it any more.
