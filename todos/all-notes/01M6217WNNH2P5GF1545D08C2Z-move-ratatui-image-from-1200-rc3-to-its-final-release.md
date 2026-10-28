---
title: "Move ratatui-image from 12.0.0-rc.3 to its final release"
kind: chore
component: tui
status: blocked
origin: request
---
# Move ratatui-image from 12.0.0-rc.3 to its final release

asqr pins the release candidate `ratatui-image = "=12.0.0-rc.3"`
(`Cargo.toml`), since cargo picks a pre-release only when it is named
exactly. The user asked on 2026-10-28 to track the move to a final
release and to drop asqr's workarounds that the new version makes
unnecessary.

## State on 2026-10-28

Checked through the crates.io API and the GitHub repository
`ratatui/ratatui-image`: 12.0.0-rc.3 (2026-10-18) is the newest version,
no final 12.0.0 exists, master's HEAD is the rc.3 release commit, and the
changelog's Unreleased section is empty. Open upstream issues that may
matter: #229 "Keep Kitty images visible during replacement uploads"
(2026-10-21) and #207 "Kitty image protocol doesn't work in Zellij";
neither was read in detail.

## asqr's workarounds for ratatui-image

None can go with rc.3, since its source is what asqr's comments cite.
Check each against the final release:

- Kitty row diacritics only on a row's first cell and the transmission
  on the first cell: the mend in `src/tui/render/screen.rs` and
  `src/graphics/placeholders.rs` (ADR 48). rc.3's `src/protocol/kitty.rs`
  `render()` still does both.
- Every `Picker` constructor sets tmux `allow-passthrough on`:
  `PanePassthrough` in `src/graphics/detect.rs` (ADRs 44 and 45).
- Late query answers reaching the key reader, and the wait rounded to
  whole milliseconds: `discard_late_answers` and `QUERY_WAIT_ROUNDING`
  in `src/graphics/detect.rs`.
- No Kitty deletes, ids handed out but never freed, no access to the
  picture: `src/graphics/kitty.rs` and `src/graphics/image.rs` (ADR 42).
- Sixel and iTerm2 cells marked skipped: `View::must_clear` in
  `src/tui/render/screen.rs` (ADRs 31 and 45); less certain whether
  rc.3's `CellDiffOption::ForcedWidth(1)` is the cause.

## Found on the way

rc.3 already draws an image partly: `Image::allow_clipping(true)` (since
11.0.0, Kitty and halfblocks) and the public `ratatui_image::sliced`
module (`SlicedProtocol`, `SlicedImage::new` with a `SignedPosition`
whose `y` may be negative), which draws an image as if it started
lines above the area. Sixel is cut in 6-pixel bands. Whether this works
with asqr's own Kitty ids and the mend was being checked on 2026-10-28
for the image caption.

## Done when

asqr depends on a final ratatui-image release, `just check` passes, the
real-terminal checks of todo 01M51VD1CY4XM1PSF63S0JGP45 (Check image
handling and the emblem in real terminals) pass with it, and each
workaround above is removed or has a comment saying why it stays.

## Relations

- Blocked by: a final ratatui-image 12.0.0 release
