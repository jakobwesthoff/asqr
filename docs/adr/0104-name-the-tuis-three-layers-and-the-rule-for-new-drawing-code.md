# 106. Name the TUI's three layers and the rule for new drawing code

Date: 2026-10-31

## Status

Accepted

Builds on [8. TUI interaction model](0008-tui-interaction-model.md):
the keys stay fixed

Builds on [46. Build and lay out context viewers on worker threads](0046-build-and-lay-out-context-viewers-on-worker-threads.md):
a viewer stays `Send`

## Context

- `src/tui/mod.rs` states the principle in one sentence: the interaction
  is a state machine driven by key events, kept apart from drawing and
  from the terminal, so tests can run every key without one.
- No ADR names the layers that follow from it, and the drawing code uses
  four styles with no rule for which one a new region takes.
- Keys go through handlers that each change their own state and return
  a value. None of them draws or touches the terminal.
  `TextField::key` returns `FieldKey`, `SessionState::handle` returns
  `Effect`, routed by `Focus`, and `App::handle` returns `AppEffect`,
  after `App::apply` translated the session's `Effect`. The switcher and
  the settings pane have handlers of the same kind, returning
  `SwitcherKey` and `SettingsKey`. The input is the `Event` of the run
  loop. No action or message type exists.
- `Inbox::apply` carries out an `AppEffect`. The `Host` trait covers the
  terminal and the operating system: alerts, the system opener, the
  terminal title, Kitty deletes, progress, the editor and the queues.
- `draw(frame, &App, &mut View) -> Drawn` draws a frame. `View` keeps
  state from one frame to the next. `Drawn` carries back what only
  drawing measures: `intro_page`, `pane_page`, `field_shape`, `row_cut`,
  `invalid_dialog`, `session_shown` and `image_sent`. The run loop draws
  before every key, hands these to the app, and takes another pass when
  `set_field_shape` reports that a field's first draw moved its cursor.
- The drawing code has four styles:
  1. Five `Widget` impls: `TabBar`, `EmptyQueue` and `GutterRow` are built
     from the state for a draw and consumed. `&Template` is the emblem,
     parsed once into `LazyLock` statics. `&Note` is drawn by reference.
     One `StatefulWidget` use draws the session list, whose list state is
     made fresh on every draw.
  2. Functions that return a ratatui value built from the state for the
     frame, which `render_widget` draws: `title_bar`, `status_line` and
     `key_line` return a `Line`, the header is a `Paragraph` of
     `header_lines`, and `tab_bar` returns a `TabBar`.
  3. Free functions that take `(area, buf, ...)` and write cells:
     `draw_title`, `draw_invalid`, `draw_intro`, `draw_session_list`,
     `draw_help`, `draw_switcher`, `draw_settings`, `draw_details`,
     `draw_dialog`, `draw_tab` and `draw_pane`.
  4. The `Viewer` trait for the content of the context pane, with
     `lay_out` and `draw` and no method for keys. It is `Send`, built and
     laid out on worker threads (ADR 46) and kept by `Viewers` under a
     `ViewerKey` in the tab state of the `View`. The keys that scroll the
     pane are `SessionState::pane_key`.

## Decision

### Three layers

- **State.** A key goes through handlers that return a value. A handler
  does not draw and does not touch the terminal.
- **Effects.** `Inbox::apply` and the `Host` trait carry out what a
  handler asks for.
- **Drawing.** It reads `&App` and writes the frame. What only drawing
  measures goes back through `Drawn`.

### Keys

The keys stay fixed (ADR 8). A new key goes through the existing handlers
and their return values. asqr has no action layer between key and
behaviour, and no component trait that puts key handling and drawing on
one object.

### Layout feedback

The feedback through `Drawn` stays as it is.

### Drawing

- A screen region is a function that returns a value implementing
  `Widget`: a `Line`, a `Paragraph`, or a small widget struct built from
  the state for the frame. `render_widget` draws it.
- A widget struct built from the state is built for each frame and
  consumed (`impl Widget for T`). One whose content is fixed or costly to
  build is built once and drawn by reference (`impl Widget for &T`), and
  cached under a key that says when it goes stale, as `&Template` is.
- A helper takes `(area, buf)` only where a region has to write cells
  directly.
- What keeps state across frames, such as a scroll position or a cache,
  or loads apart from drawing, is a `Viewer`. It is owned by `Viewers`
  and `SessionState`, not by the draw.

A widget is not kept across frames, as it would hold a second copy of the
state and cannot borrow from `App`, which `App::handle` takes as `&mut`
between frames. A component trait is not used, as it would join the state
and the drawing that `src/tui/mod.rs` keeps apart. A `Viewer` is no
component: it takes no keys and is `Send`.

### Where the code mixes styles

The functions of the third style above draw regions with `(area, buf)`.
They stay as they are. This ADR asks for no migration of them.

## Consequences

- This ADR changes no code.
- The code of the third style stays as it is.
