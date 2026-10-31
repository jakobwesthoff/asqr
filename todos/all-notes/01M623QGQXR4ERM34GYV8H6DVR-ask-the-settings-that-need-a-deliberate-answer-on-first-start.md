---
title: "Ask the settings that need a deliberate answer on first start"
kind: feature
component: tui
origin: discussion
tags: [ux]
---
# Ask the settings that need a deliberate answer on first start

asqr keeps its settings in `settings.toml` and changes them in a settings
pane on `,` (ADR 92, spec 5.2 and 7.6). The three settings it has today,
how a diff opens, the widest a question gets and where context goes,
have defaults a person can judge only after a session, so asqr does not
ask for them. What remains is a first-start screen for the settings that
need a deliberate answer before they take effect: keeping files (todo
01M67A73S1SRPTR8YRZPPC4FSH) and the update check (todo
01M5J41K58B5CF23DN61PWM6MC).

## Decisions (Jakob, 2026-10-30, in the chat)

- **A first-start screen.** The first time the TUI starts without a
  settings file, it asks the few settings that matter, writes the file
  and does not ask again. Rejected: start with defaults and only name
  the file.
- **It comes with the first setting that needs a deliberate answer,**
  keeping files (todo 01M67A73S1SRPTR8YRZPPC4FSH), and the update check
  joins it later. Rejected: a first start that asks the taste settings,
  which nobody can judge before a session.
- **Look: the design "Sibling"** from the design study of 2026-10-30
  (conductor logbook `plans/settings-design/`, `index.md` and
  `png/d1-*.png`): the first-start screen uses the queue switcher's
  frame, the same frame the settings pane has, with its questions open
  and a "Save and start" button. Rejected: "Dogfood" (the settings as an
  asqr session), "Switchboard" (a full page), "Drawer" (a band above the
  key bar). The study proposes a first start that asks only about keeping
  files and the update check.
- **The first-start texts are rewritten with Jakob** when the screen is
  built: the drafts in the study (`d1-1`, `d1-2`) do not say clearly what
  each choice means. The pane's texts are written for someone who has
  never read the docs, and the first-start texts follow that.
- **The screen does not run for commands agents call** (`ask`, `wait`,
  `result` and so on), only for the TUI.

## What the settings need

- Keeping files: `compact`, `exact` or `off`, default `compact`
  (todo 01M67A73S1SRPTR8YRZPPC4FSH). `ASQR_KEEP_FILES` overrides it, and
  the pane shows such a setting locked, as it does `ASQR_LAYOUT`.
- The update check (todo 01M5J41K58B5CF23DN61PWM6MC): asked on first
  start with "no" preselected, and kept as a setting.
- Each new setting is a key of `settings.toml`, a row of the pane with
  its agreed texts and, where an environment variable or a flag can
  override it, a locked row.
