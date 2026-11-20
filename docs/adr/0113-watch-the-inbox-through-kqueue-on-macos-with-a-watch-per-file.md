# 113. Watch the inbox through kqueue on macOS, with a watch per file

Date: 2026-11-20

## Status

Accepted

Amends [9. Crate choices](0009-crate-choices.md): notify watches through
its kqueue backend on macOS

## Context

- notify watches through FSEvents on macOS by default. FSEvents events
  pass through the system daemon `fseventsd`.
- On a machine under load, with `fseventsd` at 100 % CPU and 3.2 GB of
  memory after 31 days, an FSEvents stream with notify's settings missed
  16 of 20 files written into a directory 200 ms apart, and reported the
  rest 3.6 to 4.8 s late. A kqueue watch on the same kind of directory
  reported every write within about 220 ms. asqr's TUI then showed a
  session that `asqr ask` had dropped late or not at all, and the tests
  that wait for a dropped session failed. After a restart of
  `fseventsd`, the same tests passed.
- A kqueue watch on a directory reports entries that are added, removed
  or renamed, but not a file rewritten in place. Spec section 5.4 says a
  changed inbox file is reloaded.
- notify's kqueue backend watches a directory's files only as they
  arrive, one per report of the directory: not the files there when the
  watch starts, and not the second of two files that arrive together.

## Decision

- `notify-debouncer-full` is built without its default features and
  with `macos_kqueue`, so notify watches through kqueue on macOS. Other
  systems keep their default backend, inotify on Linux.
- The inbox watcher watches each regular file in the inbox as well as
  the inbox itself, the files there at start included. After each burst
  of changes it watches the files that arrived, watches a file again
  that a rename replaced, gives up the watches of files that left, and
  then reports the change.

## Consequences

- Watching the inbox no longer depends on `fseventsd`.
- asqr holds one file descriptor per file in the inbox, besides the one
  for the inbox.
- The same per-file watches run on Linux, where the watch on the
  directory reports rewrites already.
- The macOS build links `kqueue` and `kqueue-sys` (both MIT).
