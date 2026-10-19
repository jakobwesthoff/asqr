---
title: "Scroll half a page with ctrl-u and ctrl-d as Vim does"
kind: improvement
component: tui
origin: request
tags: [ux, field-test]
---
# Scroll half a page with ctrl-u and ctrl-d as Vim does

Raised by the user on 2026-10-19 while using the context pane.
`ctrl-u` and `ctrl-d` scroll the context pane, the full-screen view and
the intro overlay by a whole page, the same distance as `pgup` and
`pgdn`. Vim and `less` scroll half a page with these keys and keep the
whole page for `ctrl-b`/`ctrl-f` and the page keys. asqr borrows the
keys from Vim, so a person who knows them expects half a page, loses
the place on every press and has to scroll back.

## Goal

`ctrl-u` and `ctrl-d` scroll by half the view's height, rounded down
and at least one line. `pgup` and `pgdn` keep scrolling by a whole
page. Everything else about scrolling stays as it is: `ctrl-u` and
`ctrl-d` still scroll the pane from anywhere while one shows, and a
field still keeps them for undo and delete when no pane shows.

## What Vim and less do

Checked on 2026-10-19:

- Neovim's `:help CTRL-D` says the number of lines "comes from the
  'scroll' option (default: half a screen)", and `:help 'scroll'` says
  it "will be set to half the number of lines in the window when the
  window size changes". `<PageDown>` and `CTRL-F` scroll a whole page.
- Measured in `nvim --clean --headless` with `laststatus=0`: a window
  of 2 rows has `&scroll` 1, 3 rows 1, 10 rows 5, 11 rows 5, 12 rows 6.
  So Vim rounds down, and two `ctrl-d` in an 11-row window over 100
  lines put line 11 at the top, 10 lines down.
- `man less`: `d` or `^D` scroll "default one half of the screen size",
  `SPACE`, `f` and `^F` "default one window".
- Vim takes a count before `ctrl-d` as the new `scroll` value. asqr has
  no counts, and this todo does not add them.

## Where it is today

All three views go through one function, `scroll` in
`src/tui/scroll.rs`, so the change lands in one place:

```rust
KeyCode::Char('u') if control => (top.saturating_sub(page.rows.shown), left),
KeyCode::Char('d') if control => (top + page.rows.shown, left),
KeyCode::PageUp => (top.saturating_sub(page.rows.shown), left),
KeyCode::PageDown => (top + page.rows.shown, left),
```

Its callers are the intro overlay, `App::handle` in `src/tui/app.rs`
(around line 309), and the pane and the full-screen view,
`SessionState::scroll_pane` in `src/tui/session.rs` (around line 837).
Neither caller needs to change.

## Proposal

1. In `src/tui/scroll.rs`, give `Extent` a `half` method (or a local
   `half_page`) that returns `(self.shown / 2).max(1)`, and use it for
   the two `ctrl` arms. The `.max(1)` keeps a one-row view moving; the
   final `page.clamp` already brings an empty view back to 0. Rewrite
   the doc comment on `scroll`, which says `ctrl-u`/`ctrl-d` "scroll by
   a page", and keep the reason they scroll in any case (they do not
   type text).
2. Tests first, in the bug-fix order: change the expectations so they
   fail before the code changes.
   - `src/tui/scroll.rs`: `a_focused_view_scrolls_by_line_page_and_to_its_ends`
     (`ctrl('d')` from 25 still ends at 30 through the clamp, so move
     its start to make the half step visible, such as from 5 to 10;
     `ctrl('u')` from 25 goes to 20) and
     `an_unfocused_view_scrolls_only_by_page` (`ctrl('d')` from 0 to 5,
     `ctrl('u')` from 10 to 5). Add a case for an odd height (11 rows
     shown: 5 lines) and for one row shown (1 line).
   - `src/tui/app.rs`: `the_intro_scrolls_by_line_and_page_within_its_ends`
     scrolls a 10-of-40 intro; its `ctrl('d')` and `ctrl('u')` steps
     become 5 lines.
   - `src/tui/session.rs`: `the_focused_pane_scrolls_by_line_page_and_to_its_ends`
     and `page_keys_scroll_the_pane_from_the_question_and_from_a_field`
     use `ctrl('d')` and `ctrl('u')` with a page of 10; their expected
     tops change by 5 instead of 10.
3. Help texts in `src/tui/render/keys.rs`. `PANE_HELP` and
   `FULL_SCREEN_HELP` both have the line
   `("pgup/pgdn", "scroll by page, also ctrl-u/ctrl-d")`. Split it into
   `("pgup/pgdn", "scroll by page")` and
   `("ctrl-u/ctrl-d", "scroll by half a page")`. The pane help has 9
   lines and the full-screen help 8, so one more line still fits the 13
   lines the smallest screen gives the help. The general `HELP` is held
   at exactly 13 lines and says
   `("pgup/pgdn", "scroll the context pane, also ctrl-u/ctrl-d")`,
   which names no distance and can stay. Update the snapshots that show
   these lines (`pane_help`, `full_screen_help`, and any other that
   `cargo insta` flags; `the_help_fits_the_smallest_screen` shows the general line and stays) and review them by eye.
4. Docs, all of which say "by a page" for both pairs today:
   - `docs/spec.md`, section 7.6: the key table row
     `pgup`/`pgdn`, `ctrl-u`/`ctrl-d` (around line 953), the focused
     pane's keys (around line 980), the page keys from anywhere (around
     line 990) and the intro overlay (around line 1012).
   - ADR 31, the *Page keys* decision and the line "a page is the
     pane's height" in the shared scroll conventions. Since `6c08877`
     each ADR holds the one current decision, so edit the decision in
     place instead of adding an amendment. Give the reason: the keys
     come from Vim and `less`, where they move half a page.
   - ADR 30, the intro overlay's keys (around line 52).
   - `README.md` around line 74, which names `ctrl-u`/`ctrl-d` as the
     alternative to `pgup`/`pgdn`.
   - `CHANGELOG.md`: an `Unreleased` section with a *Changed* entry.
     Today the newest section is `[0.12.1] - 2026-10-16`.

## Open questions

- `pgdn` jumps exactly one view height, so the last line shown becomes
  invisible and the reader has no line to hold on to. Vim's `ctrl-f`
  keeps two lines of overlap. That is a separate change; decide during
  this work whether to file it as its own todo.
- `ctrl-f` and `ctrl-b` for a whole page would complete the Vim set,
  but in a field they move the text cursor (spec section 7.7). Leave
  them out unless the user asks.

## Done when

- `ctrl-u` and `ctrl-d` move the pane, the full-screen view and the
  intro by half the view's height, rounded down and at least 1 line,
  and `pgup`/`pgdn` still move by a whole page.
- The tests above cover an even height, an odd height and a one-row
  view, and `just check` passes.
- The help, the spec, ADR 30, ADR 31, the README and the changelog say
  half a page for `ctrl-u`/`ctrl-d`.
