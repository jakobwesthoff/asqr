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
need a deliberate answer before they take effect: keeping files (the
`keep_files` setting, ADR 100) and the update check (todo
01M5J41K58B5CF23DN61PWM6MC).

## Decisions (Jakob, 2026-10-30, in the chat)

- **A first-start screen.** The first time the TUI starts without a
  settings file, it asks the few settings that matter, writes the file
  and does not ask again. Rejected: start with defaults and only name
  the file.
- **It comes with the first setting that needs a deliberate answer,**
  keeping files (the `keep_files` setting, ADR 100), and the update check
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

## Decision (Jakob, 2026-10-31, in the chat)

- **The screen is built now, with keeping files as its only setting.**
  The update check joins it when that todo is done. Rejected: a one-time
  line in the status line instead of a screen, and parking the screen
  until the update check. The lead had noted that accepting the default
  changes nothing, since `compact` is already the default.
- **The texts are written together with Jakob** before the screen is
  implemented, as decided on 2026-10-30.

## Texts (Jakob, 2026-10-31, in the chat)

The pane's Keep files row is final as built (`src/tui/settings.rs`):
label `Keep files`; values `compact copies` ("Compact copies: images are
kept as WebP copies, about a fifth of their size; every other file is
kept as it is."), `exact copies` ("Exact copies: every file is kept byte
for byte, images included.") and `no copies` ("No copies: no file is
kept. asqr still records each file's size and checksum, so a replay can
tell the file is gone."). The first-start screen uses these value texts
word for word, so the setting reads the same on both screens.

The first-start screen, in the queue switcher's frame (design
"Sibling"):

- Lead: `Before your first session, one setting needs your answer. Press
  , later to change it.`
- Question: `Keep copies of the files a session showed you?`
- Why (dim): `Files a session shows are often temporary. Replay can show
  them later only if a copy was kept.`
- The three values with `compact copies` preselected and marked
  `◀ default`, each with its pane text.
- Button `[ Save and start ]`; footer `writes <path to settings.toml>`.
- `q` and `ctrl-c` write nothing, and the next start asks again, since
  quitting is no answer; the line `q quits without writing; the next
  start asks again.` says so. Rejected: quitting writes the defaults.
- After saving, the pane's own notice: `✔ saved keep files: compact
  copies` (with the value chosen).

## What the settings need

- Keeping files: `compact`, `exact` or `off`, default `compact` (the
  `keep_files` setting, ADR 100). `ASQR_KEEP_FILES` overrides it, and
  the pane shows such a setting locked, as it does `ASQR_LAYOUT`.
- The update check (todo 01M5J41K58B5CF23DN61PWM6MC): asked on first
  start with "no" preselected, and kept as a setting.
- Each new setting is a key of `settings.toml`, a row of the pane with
  its agreed texts and, where an environment variable or a flag can
  override it, a locked row.
