---
title: "Move to ratatui-image 12.0.0-rc.2 and drop the Kitty placement workaround"
kind: chore
component: tui
origin: upstream
---
# Move to ratatui-image 12.0.0-rc.2 and drop the Kitty placement workaround

The user asked for this todo while waiting for a `ratatui-image` release since 2026-10-03. On 2026-10-15 the user decided to move to the release candidate `=12.0.0-rc.2` (2026-10-05), which contains the fix (commit `2d7d7f6` is an ancestor of the `v12.0.0-rc.2` tag), and to keep `ratatui-image` rather than draw images with asqr's own code; the evaluation and its adversarial review are in the logbook `~/playground/2026_10_15_asqr-maskottchen/99-eval-ratatui-image.md` and `99-review-impl-ledger.md` (round 3). Do it after the Kitty-delete item (todo 01M50PJX24T6G6V3DNZKM56K0J) and the emblem item (todo 01M50RJD7AN3H7W76KCDYRQH6Z) have landed.

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

- Pin `ratatui-image = "=12.0.0-rc.2"` with `cargo add` (cargo selects a
  pre-release only when named exactly).
- Remove `size_kitty_placement`, its constants and its call, following
  its doc comment. `a_kitty_image_is_placed_over_exactly_its_placeholder_cells`
  in `src/tui/render/screen.rs` must stay green without it.
- Adapt the Kitty constructor the delete item added: rc.2's
  `StatefulKitty::new(id, is_tmux, compress, smo)` takes a fourth
  argument, from `Capability::KittySharedMemory`. rc.2 may transmit
  through shared memory (`t=s`) locally, so tests that check exact
  transmissions adapt.
- rc.2 returns a half-block `Picker` instead of an error when the
  terminal answers nothing (picker.rs:164-175), so asqr's log line
  "no graphics protocol detected" (`src/terminal.rs`, around l.96) never
  fires. Log the half-block reason from `protocol_type()` and empty
  `capabilities()` instead, and adapt tests that expect the error. Only a
  terminal that answers nothing at all loses the iTerm2 protocol hinted
  by the environment; a missing cell-size answer already meant half
  blocks in 11.1.0.
- Optionally set a shorter query timeout through
  `Picker::from_query_stdio_with_options` (`QueryStdioOptions::timeout_ms`).
- Give context images a fresh Kitty id when their pane size changes:
  ratatui-image re-encodes on resize under the same id (rc.2
  kitty.rs:160-166), which Ghostty blanks during the transfer. Before
  drawing, if `ResizeEncodeRender::needs_resize` says a resize is due,
  build a new protocol with a fresh id through the delete item's
  constructor and queue the old id through its drop path (about 15 lines;
  the decoded source image is kept next to the protocol).
- Scale context images with a real filter: `images.rs` passes
  `Resize::Fit(None)` / `Resize::Scale(None)`, which means Nearest; pass
  e.g. `Some(FilterType::Triangle)` and check the snapshots.
- Read `StatefulProtocol::last_encoding_result` after drawing and log an
  encode error (today a failed Sixel encode leaves the image empty
  without a trace).
- PTY acceptance tests (`tests/tui_pty.rs`, extend `answer_the_query`):
  answering as Kitty with a cell size, the output carries `a=T,U=1` with
  `c=`/`r=`; with no answer, a key typed after the deadline reaches asqr
  (the blocked-reader bug of todo 01M4Y6MS, gone in rc.2).
- Check 01M4Y6MS afterwards: rc.2 reads the answer with `poll` on the
  calling thread; what may remain is the `tmux set -p allow-passthrough
  on` that `PassthroughRestore` undoes.
- The fork `jakobwesthoff/ratatui-image` and its branches
  `kitty-placement-size` and `kitty-placement-size-v11` are no longer
  used and can be deleted (the user's call, outside this repository).

## Later, not part of this todo

- Propose upstream an option to skip `tmux set -p allow-passthrough on`
  (no AI attribution in commits: the maintainer rejects it).
- If upstream refuses that option, 12.0.0 stalls past a date the user
  sets, or a protocol bug is not fixed upstream, owning the image code
  (the evaluation's option (c)) is the documented fallback.
