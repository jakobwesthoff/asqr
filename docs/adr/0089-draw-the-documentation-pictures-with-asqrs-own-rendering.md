# 89. Draw the documentation pictures with asqr's own rendering

Date: 2026-10-30

## Status

Accepted

Amends [16. Keep one crate with a library and a thin binary](0016-keep-one-crate-with-a-library-and-a-thin-binary.md):
the repository is a Cargo workspace rooted at asqr's crate, with the
development tool `tools/screens` as a member that is not published; asqr
itself stays one crate

## Context

- A user guide shows asqr's real screens, and its pictures have to follow
  every release.
- The TUI draws into any ratatui backend through the library's public
  API: `App`, keys through `App::handle`, and
  `asqr::tui::render::draw`.
- The insta snapshot tests record screens as text for the tests.

## Decision

- A development crate, `tools/screens`, draws each picture through
  asqr's public API into a ratatui `TestBackend` and rasterizes the cells
  straight into PNG in Rust, with fonts committed together with their
  licences. `just screens` runs it. One toolchain draws the pictures, and
  a release refreshes every picture with one command.
- Each picture is a TOML scene file: the sessions, the keys to press, the
  terminal size, the queue state and the caption. Adding a picture means
  writing a file, not editing Rust.
- The pictures are not drawn from the snapshot tests: accepting a
  snapshot never changes a picture.
- The crate is a member of a workspace rooted at asqr's `Cargo.toml`, so
  `just check` builds, lints and tests it; it is not published, and
  `tools/` is excluded from asqr's crate.

Rejected: VHS tapes; screenshots by hand; a program that writes the
screens as text with escape sequences for scripts to parse and render
with the system's fonts; pictures drawn from the insta snapshot tests.

## Consequences

- Every step of `just check` and `just coverage` names the workspace, as
  a plain `cargo` command builds asqr alone.
- The file list and the lock file of the published crate do not change.
