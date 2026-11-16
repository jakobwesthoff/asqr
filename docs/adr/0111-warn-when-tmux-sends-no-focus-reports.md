# 111. Warn when tmux sends no focus reports

Date: 2026-11-16

## Status

Accepted

Amends [44. Pass Kitty images through tmux from a hidden pane](0044-pass-kitty-images-through-tmux-from-a-hidden-pane.md):
the question to tmux at start also asks for `focus-events`

Amends [110. Send Kitty images again after a tmux session switch](0110-send-kitty-images-again-after-a-tmux-session-switch.md):
asqr warns at start where that rule cannot work for want of focus
reports

## Context

- Behind tmux, a Kitty image or delete that tmux dropped goes out again
  only when asqr's pane gains the focus (ADRs 24 and 110). tmux sends
  focus reports to its panes only with `focus-events on`. Without them
  such an image stays blank until its session is asked again.
- tmux applies a change of `focus-events` to an attached client only
  after it detaches and attaches again (tmux(1)).
- asqr asks tmux about its pane once at start (ADR 44). tmux 3.8
  expands `#{focus-events}` to `0` or `1`.
- The status line holds one notice, and asqr draws no screen narrower
  than 60 columns.

## Decision

- The question to tmux at start ends with `#{focus-events}`.
- Where images go out under the Kitty protocol behind tmux with `on` or
  `all`, and tmux answers `0`, asqr shows the warning "set tmux's
  focus-events on, or images may stay blank" in the status line at
  start, unless another notice holds the line already, such as a
  problem with the settings file. The log gets the warning with what it
  means and that the client has to detach and attach again after the
  change.
- An answer without the field, or with another value, counts as focus
  reports on.

## Consequences

- With `focus-events off`, every start of asqr inside tmux under the
  Kitty protocol shows the warning until the option is on.
- The warning takes the place of the confirmation of the first start's
  settings, which shows only where no other notice does; a failure to
  save them still shows.
