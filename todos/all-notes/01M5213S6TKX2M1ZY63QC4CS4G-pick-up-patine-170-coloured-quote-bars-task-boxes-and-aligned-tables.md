---
title: "Pick up patine 1.7.0: coloured quote bars, task boxes and aligned tables"
kind: improvement
component: tui
origin: discussion
tags: [ux, field-test]
---
# Pick up patine 1.7.0: coloured quote bars, task boxes and aligned tables

Jakob decided on 2026-10-16 (asqr sessions 01M51YN8AA9K008BB5WSJQ02YH
and 01M51ZGEGBK7BA28PVMZPJ4RRC) how patine renders quotes, task lists
and table alignment, and that asqr picks the release up right after it
comes out, for asqr's next release. patine 1.7.0 is out since
2026-10-16 (tag `v1.7.0`, crates.io). Quotes get upright text and a
coloured bar, task items `☐`/`☑`, and table columns follow the
delimiter row.

## Change

- Raise `patine` in `Cargo.toml` to 1.7.0 (`cargo update -p patine`).
- Pass `QuoteStyle::Bar(AnsiColor::Blue)` through
  `RenderOptions::with_quote_style` at both call sites: the question texts (`FIELD_OPTIONS`
  in `src/tui/render/text.rs`) and the Markdown context entries, which
  call `patine::render_lines` without options today
  (`src/tui/render/markdown_viewer.rs:100`) and move to
  `render_lines_with`.
- Under `NO_COLOR`, pass `QuoteStyle::DimBar` so the bar stays dim as
  today (Jakob's decision): `strip_colours` in
  `src/tui/render/theme.rs` keeps dim and drops colours, so a coloured,
  non-dim bar would show in the full text colour. The NO_COLOR flag
  (`no_color()` in `src/cli/watch.rs`) has to reach both call sites, so
  `FIELD_OPTIONS` becomes a function or a pair of constants.
- `indent()` in `src/tui/render/patine_adapter.rs` (around 114-152)
  learns `☐ ` and `☑ ` as list markers next to `• `, or a long line in a
  task item wraps under its box instead of its text.
- Update spec sections 7.6 and 7.7 and ADR 35 as commit `d868f58` did
  for an earlier pickup.
- Tests that assert a dim, italic quote change (`src/tui/render/screen.rs`
  around 720-722 and the adapter's quote test), as do `docs/spec.md`
  around lines 1050 and 1178. Record the new list markers as an
  amendment to ADR 34 (patine's notes on the pickup, 2026-10-16).
