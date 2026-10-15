# 40. Set the terminal title

Date: 2026-10-15

## Status

Accepted

## Context

asqr runs in a terminal of its own, often in a tab the person does not
look at. Until now its state showed only inside the window, plus a
desktop notification and the bell when a session arrives (spec section
7.5). The user chose on 2026-10-15 to show the number of waiting
sessions in the terminal title, as `asqr · {n} waiting`, with a
`--no-title` flag that switches it off like `--no-notify` and
`--no-bell`. Reporting progress through OSC 9;4 stays open until it is
known which terminals support it.

A program that sets the title has to decide three things: which OSC
sets it, what happens to the title when the program ends, and how it
goes through tmux. How terminals answer these was established on
2026-10-15 by reading the source of each terminal at the commit named
in the table below, and, for Terminal.app, from Apple's help text and
an analysis of the installed binary. No terminal was checked by hand.

What each terminal does with the sequence asqr writes (see Decision):
the push, OSC 0 titles, an empty OSC 0, the pop.

| Terminal | OSC 0 | Empty OSC 0 | Push and pop (`CSI 22;0 t`, `CSI 23;0 t`) | End state after asqr |
|---|---|---|---|---|
| Ghostty | sets the title, as OSC 2 does; a title of 256 bytes or more is dropped | the pwd reported through OSC 7, or blank | parsed, then ignored | pwd or blank, until the shell integration sets the title at the next prompt (its `title` feature is on by default) |
| kitty | sets the title | the basename of the shell's `argv[0]`, such as `zsh` | kept on a stack 10 deep | the title from before asqr |
| iTerm2 | sets the window and the icon title; the tab shows the icon title | both empty, so the profile's automatic title shows | kept on a stack, for the window and the icon title; gated by the profile setting that lets applications change the title, on by default | the window and the icon title from before asqr |
| VS Code | sets the title, which the tab shows only when `terminal.integrated.tabs.title` contains `${sequence}`; the default is `${process}` | the process name | ignored: xterm.js keeps a stack only when the embedder turns it on, and VS Code does not | the process name |
| Terminal.app 2.15 | sets the window title and clears a tab title set with OSC 1 | the profile's window title, which the title builder hides (the analysis covered an empty OSC 2) | ignored; Terminal.app keeps the window and the tab title per screen and restores the main screen's title on leaving the alternate screen | the title from before asqr |
| tmux | sets the pane title (`allow-set-title`, on by default) | an empty pane title | kept per pane on a stack 10 deep; a push or pop without the second parameter is ignored | the pane title from before asqr; the outer terminal's title is not touched |
| WezTerm | sets the title and clears an icon title set with OSC 1 | an empty title | parsed, then ignored | an empty title |
| Alacritty | sets the title, as OSC 2 does | an empty title | kept on a stack 4096 deep | the title from before asqr, or `Alacritty` when none was set |

Sources, per row, at the commit read:

- Ghostty, ghostty-org/ghostty `22fa134a07f5`: `src/terminal/osc.zig`
  1126-1128 (OSC 0 and 2 share one parser), `src/termio/stream_handler.zig`
  996-1001 (length), 1012-1027 (empty title) and 387-393 (push and pop
  under "Unimplemented"), `src/config/Config.zig` 8899-8906 (`title`
  on by default).
- kitty, kovidgoyal/kitty `535c7502e838` (0.49.2): `kitty/vt-parser.c`
  554-566 (OSC 0), `kitty/window.py` 237-245 and 806 (empty title and
  `default_title`), 808 and 1982-1991 (stack), `kitty/screen.c`
  3261-3268.
- iTerm2, gnachman/iTerm2 `069b6e0dd8f3`:
  `sources/VT100/VT100Terminal.m` 2721-2732 (OSC 0) and 2982-3012
  (stack), `sources/Language/BuiltInFunctions/iTermSessionTitleBuiltInFunction.m`
  123-130 (an empty title trimmed to none),
  `sources/Settings/iTermProfilePreferences.m` 1206 (the setting's
  default). The tab: iTerm2's Python API documentation, "By default, a
  tab shows the icon title of the current pane"
  (https://iterm2.com/python-api/examples/sync_title.html).
- VS Code, xtermjs/xterm.js `991857a1b1ca` (6.0.0):
  `src/common/InputHandler.ts` 286 (OSC 0), 77-78 and 2937-2984 (stack
  behind `windowOptions`), `src/common/services/OptionsService.ts` 53
  (`windowOptions` empty by default); microsoft/vscode `25f764abcdbd`:
  `src/vs/workbench/contrib/terminal/browser/xterm/xtermTerminal.ts`
  285-289 (only the size queries on),
  `src/vs/workbench/contrib/terminal/common/terminalConfiguration.ts`
  361-365 (`${process}`),
  `src/vs/workbench/contrib/terminal/browser/terminalInstance.ts` 2866
  (empty title falls back to the process name).
- Terminal.app 2.15 (CFBundleVersion 488): Apple's help text in
  `TTAppPreferences.loctable` (OSC 1 sets the tab title, OSC 2 the
  window title), and a static analysis of the installed binary:
  `performOperatingSystemCommand` (OSC 0, the empty title), the
  window-operations jump table, which covers 1 to 18 and lets 22 and 23
  fall through, and `activateAlternateScreen:clearing:`, which saves the
  titles on entering the alternate screen and restores them on leaving
  it.
- tmux, tmux/tmux `cc6d0589cbed`: `input.c` 2725-2734 (OSC 0 and 2),
  2189-2213 and 1147-1152 (push and pop; a missing second parameter
  returns), `options-table.c` 1302-1307 (`allow-set-title` on) and
  995-1000 (`set-titles` off), `screen.c` 237-248 (empty title),
  264-310 (stack), `tty.c` 759-768 and `server-client.c` 2473-2517 (the
  outer title is written only with `set-titles` on).
- WezTerm, wezterm/wezterm `e2a98e1b8af1`:
  `term/src/terminalstate/performer.rs` 757-764 (OSC 0 clears the icon
  title), `term/src/terminalstate/mod.rs` 2212-2217 (push and pop
  ignored), `wezterm-gui/src/tabbar.rs` 221-225 (the tab shows the
  pane's title).
- Alacritty, alacritty/alacritty `29dc55375da8` with vte 0.15.0:
  `vte/src/ansi.rs` 1350-1360 (OSC 0 and 2) and 1737-1744 (push and
  pop, second parameter ignored), `alacritty_terminal/src/term/mod.rs`
  2221-2232 (empty title), 2235-2256 (stack) and 42 (depth),
  `alacritty/src/event.rs` 1873-1878 (no title resets to `Alacritty`).

## Decision

### The title and when it is written

The title is `asqr · {n} waiting`, where `n` counts the waiting
sessions. asqr compares the title with the one it last wrote and writes it
only when the text changed, so a redraw writes nothing.

### OSC 0

asqr writes the title with crossterm's `SetTitle`, which writes OSC 0,
`ESC ] 0 ; <title> BEL` (crossterm 0.29.0, `src/terminal.rs` lines
386-391). OSC 0 sets the window title and the icon title; OSC 2 sets
only the window title. The user decided on OSC 0 on 2026-10-15, after
the research, because iTerm2 shows an OSC 2 title only in the window
bar: its tab shows the icon title of the current pane (iTerm2's Python
API documentation, "By default, a tab shows the icon title of the
current pane"). OSC 0 puts the count on iTerm2's tab. The cost: in
WezTerm, OSC 0 clears a tab title the person set with OSC 1.

### Push, titles, empty title, pop

1. Right before the first title asqr pushes the terminal's title with
   `CSI 22;0 t`. The `0` pushes the window and the icon title.
2. asqr writes its titles.
3. On quitting, only when the push was written: an empty title
   (`ESC ] 0 ; BEL`), then the pop, `CSI 23;0 t`.

All of it happens on the alternate screen: the first title comes after
asqr entered it, and the empty title and the pop come before it leaves
it.

The empty title comes before the pop. A terminal with a title stack
replaces the empty title with the pushed one; a terminal without a
stack ignores the pop and keeps the empty title, which Ghostty and VS
Code replace with a title of their own. Written the other way round, the empty
title would replace the title a terminal with a stack had just taken
back. Without the empty title, WezTerm, and VS Code with `${sequence}`
in its tab title, would keep `asqr · N waiting` after asqr ended.

On a panic, ratatui's panic hook leaves the alternate screen before the
unwinding drops the guard that writes the empty title and the pop
(ratatui 0.30.2, `src/init.rs` lines 566-572). There they land on the
main screen.

### tmux

Inside tmux the push, the titles and the pop go out plain, never in the
passthrough wrapper the notification needs. tmux takes OSC 0 and OSC 2
as the title of the pane (`allow-set-title`, on by default) and keeps a
title stack per pane. It passes the pane title on to the outer terminal
only with `set-titles` on, which is off by default. Both defaults were
checked on a tmux 3.7c server started without a configuration. tmux
ignores a push or a pop without its second parameter, so asqr writes
`22;0` and `23;0`, never a bare `CSI 22 t`.

### `--no-title`

`--no-title` switches the title off. asqr then writes none of these
sequences: no title, no push, no empty title, no pop.

## Consequences

kitty, iTerm2, tmux, Alacritty and Terminal.app end with the title they
showed before asqr. Accepted gaps, where they do not:

- Ghostty has no title stack and shows the pwd, or nothing, until the
  shell's next prompt sets the title.
- VS Code shows the process name on the tab by default, so asqr's title
  does not show there at all. With `${sequence}` in the tab title it
  shows, and after asqr the tab falls back to the process name.
- WezTerm has no title stack and ends with an empty title. OSC 0 also
  clears a tab title the person set with OSC 1, and nothing brings that
  back.
- The Terminal.app row rests on its help text and a static analysis of
  version 2.15's binary, not on its source.

Under `--no-title` asqr leaves the terminal's title alone.
