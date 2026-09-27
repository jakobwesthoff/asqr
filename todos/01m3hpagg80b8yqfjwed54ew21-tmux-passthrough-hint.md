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

Knowing whether passthrough is off means running `tmux show -gv
allow-passthrough` as a subprocess and parsing its output (per session
and global settings, older tmux versions without the option). A wrong
hint is worse than none. Version 1 documents the limitation in the
README and the spec (section 7.6) and relies on the block fallback,
which always works.
