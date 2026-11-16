---
title: "Context images stay blank after switching back to the tmux session asqr runs in"
kind: bug
component: tui
origin: incident
tags: [ux, field-test]
---
# Context images stay blank after switching back to the tmux session asqr runs in

Found by Jakob on 2026-11-16 with asqr 0.17.2 in Ghostty, tmux 3.8. A session with an image in a question's context arrived while Jakob was in another tmux session. When he switched back to the session with asqr's pane, the image pane stayed blank. Switching between the tabs and questions of the session, switching tmux sessions back and forth, and taking the focus away from the terminal and back did not bring it back; only asking the session again showed it. Jakob remembers that a case like this was fixed before.

## Setup at the time

- Terminal: Ghostty, `TERM=tmux-256color` inside tmux.
- tmux 3.8 with `allow-passthrough all` and `focus-events on` (global options).
- Image: a PNG screenshot of 1616 × 1144 px in the question's `context`, with a one-line caption.

## What the code does today

`Graphics::resends_on_focus` in `src/graphics/mod.rs` sends images again on a focus gain only for the Kitty protocol with `Passthrough::WhileVisible`, which is tmux's `allow-passthrough on` (ADR 24, commit `b3448cf`). `allow-passthrough all` maps to `Passthrough::Always` in `src/graphics/detect.rs`, so asqr never sends an image again and relies on the first transfer having arrived.

## Hypothesis, not verified

With `all`, tmux passes an invisible pane's output through, but only to the clients that show its session. While the client showed another session, no client had asqr's session, so the transfer reached no terminal (or reached it while it showed another session, which then dropped or replaced the image). Afterwards asqr only refers to the image id, which the terminal does not know. If this holds, `all` needs the same resend on focus as `on`, and a session switch must count as a focus gain (check whether tmux sends a focus-in event to the pane on `switch-client`, or watch the attached session as `src/terminal_env.rs` already does for the pane's visibility).

## Repro

`examples/sessions/image-after-session-switch.json` (one question with the repository's mascot image):

1. Start asqr in a pane of tmux session A, with `allow-passthrough all`.
2. Switch the client to tmux session B.
3. From any shell, `asqr ask examples/sessions/image-after-session-switch.json`.
4. Switch back to session A. Expected: the image beside the question is drawn. Observed on 2026-11-16: the pane stays blank, also after changing tabs or focus.

Repeat with `allow-passthrough on` to see whether the existing resend covers that case, and once with the session asked while A is shown, as the control.

## Relations

- Relates to: [Check image handling and the emblem in real terminals](01M51VD1CY4XM1PSF63S0JGP45-check-image-handling-and-the-emblem-in-real-terminals.md)
