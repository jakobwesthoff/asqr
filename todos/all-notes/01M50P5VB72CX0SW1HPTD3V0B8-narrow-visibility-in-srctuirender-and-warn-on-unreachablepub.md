---
title: "Narrow visibility in src/tui/render and warn on unreachable_pub"
kind: refactor
component: tui
origin: review
---
# Narrow visibility in src/tui/render and warn on unreachable_pub

Found in the adversarial review of the redesign on 2026-10-15 (finding
S5). The modules of `src/tui/render/` are private (`src/tui/render/mod.rs`),
yet several declare their items plain `pub`: `parts.rs`, `panes.rs`,
`text.rs`, `viewer.rs`, `source_line.rs`. Newer modules (`overlay.rs`,
`tabs.rs`, `empty.rs`, `mascot.rs`) use `pub(super)`. A plain `pub` in a
private module is unreachable from outside the crate, and rustc's
`dead_code` lint does not report such an item when it falls out of use.

## Problem

`cargo clippy --lib -- -W unreachable_pub` lists about 55 items in
`src/tui/render/` (reviewer's count, 2026-10-15). The plain-`pub`
modules predate the redesign; they were added before commit `ff1751e`.

## Suggested fix

Give every item in `src/tui/render/` the narrowest visibility its uses
need (`pub(super)` or `pub(crate)`), keeping `pub` only for what
`src/tui/render/mod.rs` re-exports (`View`, `draw`, `Drawn`,
`PaneLayout`, `MIN_WIDTH`, `MIN_HEIGHT`, `TerminalNames`,
`may_query_protocol`, `passthrough_answers`), and enable
`unreachable_pub` as a warning for
the module so new items follow.
