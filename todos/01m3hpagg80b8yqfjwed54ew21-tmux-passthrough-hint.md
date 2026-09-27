# Hint in the TUI when tmux blocks passthrough

Status: deferred to after version 1 (adversarial review F9, 2026-09-27).

## Problem

Inside tmux, inline images (Kitty and iTerm2 protocols) and OSC 9/777
desktop notifications only reach the outer terminal with
`set -g allow-passthrough on`. Without it asqr silently falls back to
the block rendering and the bell, and the person does not know why the
images look coarse.

## Idea

When `$TMUX` is set and passthrough is off, show a one-time hint in the
key bar pointing to `allow-passthrough on`.

## Why not in version 1

A wrong hint is worse than none, and version 1 documents the limitation
in the README and the spec (section 7.7) and relies on the block
fallback, which always works.

## Since then

asqr already asks tmux for `#{allow-passthrough} #{session_attached}`
at start (`tmux_state` in `src/terminal.rs`, decided by
`may_query_protocol` in `src/tui/render/images.rs`) to avoid a graphics
query nobody answers. A hint could reuse that answer: passthrough `off`
is known for sure then, while a detached session (`0` clients) is a
different case that needs no hint.
