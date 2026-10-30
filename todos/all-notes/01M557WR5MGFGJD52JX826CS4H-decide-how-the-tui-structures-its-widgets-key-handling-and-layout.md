---
title: "Decide how the TUI structures its widgets, key handling and layout feedback"
kind: investigation
component: tui
status: needs-discussion
origin: discussion
---
# Decide how the TUI structures its widgets, key handling and layout feedback

Raised in a discussion with Jakob on 2026-10-17 about the TUI's
architecture. The state and key handling follow one consistent schema,
but no ADR names it, and the drawing side mixes three styles without a
rule for which one to use. Jakob wants to discuss whether to tidy it up:
real widgets in place of the free drawing functions, possibly an action
layer, and possibly removing the layout feedback through `Drawn`.
Nothing is decided yet.

## Context

What the code does today (read on 2026-10-17 at `7fb0c67`):

- **State and keys.** A state machine in three levels, each taking a key
  and returning an effect: `TextField::key` returns `FieldKey`,
  `SessionState::handle` returns `Effect`, `App::handle` returns
  `AppEffect` after `App::apply` translates the session's effect. Each
  level routes the key by focus (`Focus` in `src/tui/session.rs`).
  There is no message type of its own; the raw `KeyEvent` and `Event`
  (`src/tui/run.rs`) play that part.
- **Side effects.** `Inbox::apply` (`src/tui/inbox.rs`) carries out
  `AppEffect` (drafts, finishing a session); the `Host` trait
  (`src/tui/run.rs`) covers the terminal and the OS (alert, image
  viewer, title, Kitty deletes).
- **Drawing.** `draw(frame, &App, &mut View) -> Drawn`
  (`src/tui/render/screen.rs`). `View` keeps state between frames
  (viewer cache, Kitty book, overlay flags).
- **Layout feedback.** `Drawn` carries what only drawing measures back
  into the state before the next key: `intro_page`, `pane_page`,
  `field_shape`, `invalid_dialog`. `run` therefore draws before every
  key, and `continue`s after `App::set_field_shape` when a field's first
  draw moved its cursor.
- **Documentation.** ADR 0008 covers the interaction (layout, keys), not
  the code structure. `src/tui/mod.rs` states the principle in one
  sentence: a state machine driven by key events, kept apart from
  drawing and from the terminal so tests can run every key without one.

The drawing side uses three styles:

1. Free functions taking `(area, buf, …)`, such as `draw_title` and
   `draw_invalid` in `screen.rs`. About 39 functions named `draw…` or
   `render…` in `src/tui/render/` (counted with `rg`, 2026-10-17).
2. Ratatui `Widget` impls, 5 in all. Built from the state for each frame
   and consumed: `TabBar`, `EmptyQueue`, `GutterRow`. Rendered by
   reference: `&Template` (Talqi, parsed once into a `LazyLock` static in
   `mascot/template.rs`) and `&Note` (`const` texts, or rows a viewer
   keeps). `StatefulWidget` appears once, for the session list in
   `overlay.rs`.
3. The `Viewer` trait (`src/tui/render/viewer.rs`) with `lay_out` and
   `draw`, for the context pane's content. Viewers are `Send`, built and
   laid out on worker threads (ADR 0046) and cached by `ViewerKey`. They
   take no keys: scrolling the pane is `SessionState::pane_key`.

## Options

1. **Name the schema and set a rule for the drawing side**, in an ADR:
   a screen region is a `Widget` (title bar, header, tabs, status line,
   key line, dialogs), a helper that draws part of one stays a function,
   and whatever keeps state between frames or loads apart from drawing
   is a `Viewer`. `draw_screen` then reads as a composition of
   `render_widget` calls. The existing regions move over one at a time.
2. **An action layer** between key and behaviour (key to action, then
   action to state change), the message type of the Elm architecture.
   Its payoff is a configurable key map, which ADR 0008 leaves for
   later.
3. **A layout pass of its own** that computes from state and terminal
   size what `Drawn` reports today, so a key no longer waits for a draw.
   It changes the most delicate part of the loop, which works today.
4. **Scroll offsets as `StatefulWidget` state**, a smaller form of option
   3: a key only moves the offset, and drawing clamps it to what the
   current height allows. `intro_page` and `pane_page` then leave
   `Drawn` without a layout pass of its own. Two catches:
   - `field_shape` stays. Whether `↑`/`↓` move within a field or leave
     it depends on where its rows break; that decides what the key
     means, which drawing cannot correct afterwards.
   - Tests that only press keys check scroll limits today by setting the
     page directly (`App::set_pane_page`, `App::set_intro_page`, and
     `SessionState::set_pane_page` in the tests of `src/tui/app.rs` and
     `src/tui/session.rs`). With the clamp in drawing, each limit test
     needs a draw.

## A generic architecture write-up for comparison

On 2026-10-17 Jakob shared a generic proposal for structuring a Ratatui
TUI in layers: terminal and runtime, app with dispatcher, actions,
screens, components, widgets, plus layout, focus, theme, keymap,
background work and tests. Measured against asqr:

- Its "actions plus effects" variant, where `update` returns effects the
  app carries out so `update` stays testable, is what asqr does with
  `Effect`, `AppEffect` and `Inbox::apply`.
- Its widget section matches the per-frame versus by-reference rule
  below, and its "component is logic, widget is drawing" matches
  option 1. Its configurable keymap is option 2's trigger.
- Its theme by roles, its `Loadable<T>` (asqr: `Content<T>` with
  `LoadingNote` and `Placeholder`), its "terminal too small" notice and
  its overlay enum (asqr: `Overlay`, at most one at a time) exist in
  asqr already.
- Its central `Component` trait puts `handle_event` and
  `render(&mut self)` on one object, which joins what asqr keeps apart.
  Its screen stack, action broadcast and tokio runtime have no use in a
  single screen with tabs and an overlay enum, whose background work
  runs on threads with a channel.
- Option 4 comes from its rule that drawing may change display state
  such as a scroll offset.

It is a source of terms for the ADR of option 1, which would place asqr
as "actions plus effects" and say why it has no component trait.

## Assessment so far

The agent's view in the discussion, not decided by Jakob:

- Against a component trait that bundles key handling and rendering:
  it joins state and drawing, which `src/tui/mod.rs` keeps apart on
  purpose. A `Viewer` would not fit as a special component either: it
  takes no keys and must be `Send`, so it is a render cache, not a
  component.
- Widgets built per frame versus kept across frames: one derived from
  the state and cheap to build is built each frame and consumed
  (`impl Widget for T`). One that is expensive or fixed is built once and
  rendered by reference (`impl Widget for &T`), cached under a key that
  says when it goes stale, as `&Template` and the viewers do. A widget
  kept across frames would hold a second copy of the state, and it
  cannot borrow from `App`, which `App::handle` takes as `&mut` between
  frames.
- Option 1 is worth doing; option 2 when the configurable key map
  comes; option 3 only if the feedback through `Drawn` causes bugs;
  option 4 is the cheaper way to shrink `Drawn` if that is wanted.

## Open questions

- Which of the options to take, and in what order.
- Whether the schema and the rule go into one ADR or two.
- Whether option 3 or 4 is worth it without a bug that calls for it,
  and whether option 4's cost to the key-only tests is acceptable.
