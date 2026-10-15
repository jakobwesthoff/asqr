---
title: "Build context viewers loaded instead of in two steps"
kind: refactor
component: tui
status: blocked
origin: review
---
# Build context viewers loaded instead of in two steps

From the architecture review of 2026-10-16 (finding OA9). `Viewer`
splits construction into build and `load`, but the only caller does both
back to back (`viewer.rs` `laid_out`). The split costs a runtime
invariant: each viewer stores `Option<Loaded<T>>` and `loaded()` panics
with "the pane loads a viewer before laying it out"; and `ImageViewer`
keeps an encoder clone for its whole life only to use it in `load`. The
same pattern is in code.rs, diff.rs and markdown_viewer.rs. No
background loading is planned (lead's decision, 2026-10-16).

## Change

Building is loading: `viewer_for(entry, ..)` returns a ready viewer;
each viewer's constructor produces its content directly; `Viewer::load`,
the `Option`, `loaded()` and its panic go; `ImageViewer` holds no
encoder. Do it after the graphics module todo, so the image viewer takes
`Images`.

## Note from the cleanup review after rc.2 (2026-10-16)

After the graphics module, `ImageViewer::new` decodes the file and
`lay_out` encodes for the size (FA8), which is the shape the `Viewer`
trait describes; this todo then only removes the separate `load` step.

## Relations

- Depends on: [Gather image handling into a graphics module around an asqr-owned Graphics value](01M50SJN3A7R62BKXYG89R29PA-gather-image-handling-into-a-graphics-module-around-an-asqr-owned.md)
