---
title: "Get an option into ratatui-image to skip tmux set allow-passthrough on"
kind: chore
component: terminal
status: needs-discussion
origin: review
---
# Get an option into ratatui-image to skip tmux set allow-passthrough on

Whenever `TERM` starts with `tmux` or `TERM_PROGRAM` is `tmux`,
ratatui-image runs `tmux set -p allow-passthrough on` in every `Picker`
constructor, `Picker::halfblocks` included
(`detect_tmux_and_outer_protocol_from_env`, 12.0.0-rc.2 `picker.rs`
l.334-348). Inside tmux that overrides the person's own setting for
asqr's pane on every start. Without `TMUX`, as over ssh from a tmux
pane, the command goes to the default tmux server of the host asqr runs
on, if one is running, and changes an option of a pane that has nothing
to do with asqr. `PanePassthrough` (`src/graphics/detect.rs`) reads
the pane's value before the picker is built and puts it back
afterwards, so the change lasts only while the query runs, and stays
when asqr is killed during it. The tests build no `Picker` in a
terminal that names tmux (ADR 45), so a test run leaves the pane alone.

asqr builds a `Picker` only where it queries the terminal; where it
skips the query it takes half blocks without one, and no `tmux set`
runs. Since the passthrough probe went (2026-10-16), asqr queries, and
so builds a picker, whenever it runs outside a tmux pane, also over ssh
from one.

## History

This todo started as "Leave no graphics query reader blocked on stdin":
ratatui-image 11.1.0 read the query's answer on a thread of its own that
stayed blocked on stdin after a timeout and took the first key or paste.
12.0.0-rc.2 reads with `poll` on the calling thread, and the PTY test
`a_key_typed_after_an_unanswered_query_reaches_asqr` covers it. What is
left is the `tmux set`.

## Suggested fix

Propose upstream an option that skips the `tmux set`, such as a field
of `QueryStdioOptions` and a constructor for half blocks that runs no
command, then drop `PanePassthrough`. Commits for that pull request
carry no AI attribution: the maintainer rejects it.

## Open question

If upstream refuses the option, 12.0.0 stalls past a date the user
sets, or a protocol bug is not fixed upstream, owning the image code is
the documented fallback (the evaluation of 2026-10-15, option (c):
asqr's own query, Kitty, iTerm2 and half-block code, with `icy_sixel`
kept for Sixel). The user decides when to take it.
