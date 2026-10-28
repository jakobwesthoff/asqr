---
title: "The context checks of validate and ask can hang on a file swapped for a FIFO"
kind: bug
component: format
impact: low
origin: review
---
# The context checks of validate and ask can hang on a file swapped for a FIFO

Found in a plan review on 2026-10-28. `asqr validate` and `asqr ask`
check the type of a context file by its path and then open the path
again. A file swapped for a FIFO between the two hangs the command. The
viewers in the TUI already guard against this.

## Problem

`check_file` in `src/format/warnings.rs` accepts a path when
`std::fs::metadata` reports a regular file. Three checks then open the
path again:

- `check_unified_diff` reads the metadata again for the size and then
  calls `std::fs::read`.
- `file_cut_by_lines` reads the metadata for the size and then calls
  `File::open`.
- `check_decodable` opens the image with `image::ImageReader::open`.

Opening a FIFO for reading blocks until a writer comes, so a path
replaced by a FIFO after the type check hangs the command. A FIFO has
size 0, and `file_cut_by_lines` returns before its open for a file of
that size, so there the window is only between its own metadata call
and the open. `check_unified_diff` reads without a limit: a path swapped
for a device such as `/dev/zero` after its metadata call is read until
memory runs out.

The viewers avoid this through `open_regular` in
`src/tui/render/source_file.rs`. It opens the path with `O_NONBLOCK`,
checks the type on the opened handle, and only then clears
`O_NONBLOCK`.

Read in the code on 2026-10-28, not reproduced: the window is a race
between two system calls.

## Impact

The command waits for ever. In `ask` the warnings come before anything
reaches the queue, so the queue is not affected. It takes someone
swapping the file in that short window, which ordinary use does not do.

## Suggested fix

Open each context file once, as `open_regular` does, check the type on
the handle, and hand the opened file to the checks instead of the path.
`open_regular` sits in the TUI, so the format module needs it moved to a
place both can use.
