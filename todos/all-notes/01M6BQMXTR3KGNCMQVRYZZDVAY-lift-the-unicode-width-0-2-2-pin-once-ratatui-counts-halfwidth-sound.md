---
title: "Lift the unicode-width 0.2.2 pin once ratatui counts halfwidth sound marks once"
kind: chore
component: project
status: blocked
origin: upstream
tags: [ux]
---
# Lift the unicode-width 0.2.2 pin once ratatui counts halfwidth sound marks once

asqr pins `unicode-width = "=0.2.2"` in `Cargo.toml` since 4550bf7
(2026-11-01). It does not use the crate itself; the pin holds the version
in the lockfile, as ratatui draws the halfwidth katakana sound marks U+FF9E
(`ﾞ`) and U+FF9F (`ﾟ`) in two cells each with unicode-width 0.2.3. Once
ratatui counts them once, the pin and its comment go, and `cargo update`
takes unicode-width 0.2.3 or newer.

## What was found (2026-11-01)

- Terminals draw each of the two marks in a cell of its own. unicode-width
  0.2.2 gives them no width, as they extend the grapheme before them;
  unicode-width 0.2.3 gives each of them width 1 (its crate docs: U+FF9E
  and U+FF9F "have width 1").
- ratatui-core's `impl CellWidth for str` (`src/buffer/cell_width.rs`,
  lines 42-43 in ratatui-core 0.1.2, the newest release, and unchanged on
  ratatui `main`) returns `self.width() + count_halfwidth_sound_marks(self)`,
  one extra cell per mark on top of unicode-width. That came in with
  ratatui PR #2499 (merged 2026-04-26), written for unicode-width's zero
  width. ratatui-core accepts `unicode-width >= 0.2.0` since PR #2482, so a
  plain `cargo update` pulls 0.2.3 in.
- Measured with ratatui 0.30.2, ratatui-core 0.1.2 and unicode-width 0.2.3
  (end column of `Buffer::set_stringn` from column 0): `ｶﾞ` 3, `ﾞ` 2,
  `aﾞ` 3, `ﾞﾞ` 4, `👍ﾞ🏻` 6. Terminals draw `ｶﾞ` in 2 and a lone mark in 1.
  With unicode-width 0.2.2, ratatui gives 2, 1, 2 and 2 for the first four.
- asqr's own width code (`columns` in `src/tui/render/source_line.rs`) goes
  through `CellWidth`, so it agrees with what ratatui draws. With 0.2.3 in
  the lockfile four tests fail, among them
  `tui::render::source_line::tests::the_width_is_the_columns_on_screen`
  (left 3, right 2) and
  `tui::render::patine_adapter::tests::widths_agree_with_patine_for_a_halfwidth_sound_mark`.
- patine 2.2.1 counts each mark as one column under unicode-width 0.2.2 and
  0.2.3 alike (patine commit ff89e2a, patine ADR 0019: per mark it adds only
  the column unicode-width leaves out). patine therefore agrees with
  terminals under either version, and with ratatui only under 0.2.2.
  Requiring unicode-width 0.2.3 in patine was considered and dropped on
  2026-11-01, as it would force every program on patine and ratatui into
  ratatui's double count.
- ratatui PR #2790 (open), "fix(core): make Span::width count the cells
  that get rendered", makes `Span::width` count through `CellWidth`. It
  keeps the extra cell, so it does not fix this.
- An issue for ratatui was drafted on 2026-11-01 (asking `CellWidth` to add
  only the cell unicode-width leaves out, as patine 2.2.1 does); whether it
  is posted is up to the owner.

## What lifting the pin takes

A ratatui-core release whose `CellWidth` gives each mark one cell under
unicode-width 0.2.3. Then: remove the pin and its comment from
`Cargo.toml`, `cargo update`, and run `just check` with unicode-width 0.2.3
in the lockfile; the four tests above must pass unchanged, as their
expectations are what terminals draw. The CHANGELOG line of 0.17.0 explains
the pin; the release that lifts it says so.

## Relations

- Blocked by: a ratatui-core release that counts the halfwidth sound marks once under unicode-width 0.2.3
