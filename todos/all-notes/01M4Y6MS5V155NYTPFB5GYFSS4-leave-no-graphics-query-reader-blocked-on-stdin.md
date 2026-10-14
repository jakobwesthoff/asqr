---
title: "Leave no graphics query reader blocked on stdin"
kind: bug
component: terminal
status: needs-discussion
origin: review
---
# Leave no graphics query reader blocked on stdin

asqr detects the terminal's graphics protocol with ratatui-image's
`Picker::from_query_stdio`, which writes the query and reads the answer
on a thread of its own. When no answer comes, the picker gives up after
its timeout, but its thread stays blocked reading stdin and takes the
first input that arrives once asqr's own event thread runs. A first
paste then loses its start marker, and its line breaks arrive as
`enter`, which can pick an option or submit the session. Found in the
round-2 adversarial review of the context pane work.

## Problem

`query_with_timeout` in ratatui-image 11.1.0 (`src/picker.rs`) spawns
the reading thread and returns `NoStdinResponse` when `recv_timeout`
runs out. Nothing ends the thread: it sits in the read until input
comes.

asqr avoids the known cases in `may_query_protocol`
(`src/tui/render/images.rs`): inside tmux it asks tmux for passthrough
and an attached client first. Without `TMUX`, when `TERM` starts with
`tmux` or `screen` or `TERM_PROGRAM` is `tmux`, as after ssh from a tmux
pane, it sends a passthrough-wrapped device status request and waits up
to 300 ms with `poll` for the answer (`passthrough_answers`), and
queries only when it came. Any other terminal that leaves the query
unanswered still hits the bug. The probe loses keys typed during its
wait, and an answer slower than 300 ms gives up inline images that
would have worked.

A second effect of the same detection: whenever `TERM` starts with
`tmux` or `TERM_PROGRAM` is `tmux`, ratatui-image runs
`tmux set -p allow-passthrough on` (`detect_tmux_and_outer_protocol_from_env`,
called by `from_query_stdio` and by `Picker::halfblocks` too). Without
`TMUX`, as over ssh from a tmux pane, that command goes to the default
tmux server of the host asqr runs on, if one is running, and changes an
option of a pane that has nothing to do with asqr.

## Options

- Query on asqr's own reader: write the query and read the answer on
  the thread that later reads key events, before it starts handing out
  events, and build the `Picker` from what came back. An unanswered
  query then times out without leaving a second reader behind, and
  input that arrives during the wait can be handed on instead of lost.
- Get a cancellable reader into ratatui-image upstream, such as a read
  with a poll timeout that ends the thread with the query.

Either way, decide whether asqr keeps the `TERM` check and the
passthrough probe afterwards, and whether to avoid `detect_tmux_and_outer_protocol_from_env` when
`TMUX` is unset, which may need an upstream option as well.
