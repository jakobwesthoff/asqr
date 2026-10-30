---
title: "Keep settings in a file, asked on first start and changed in a settings pane"
kind: feature
component: tui
origin: discussion
tags: [ux]
---
# Keep settings in a file, asked on first start and changed in a settings pane

asqr keeps no preference: settings come from flags and environment
variables only (ADR 15, "There is no config file"; spec, "Not in asqr
yet": "A config file"). Several settings now want a home: whether a
finished session's files are kept (todo 01M67A73S1SRPTR8YRZPPC4FSH),
the starting layout of a diff, the reading width, and where context
goes. Jakob asked on 2026-10-15 whether the reading width could be set
per terminal (size, font or use), and on 2026-10-28 wanted it among
these settings rather than as a flag of its own. Decided on 2026-10-30:
a settings file, a short first-start screen that writes it, and a
settings pane in the TUI.

## Today (main 1dcae03)

- The diff layout starts as `Unified` (`src/tui/session.rs:203-207`);
  `s` switches the entry shown until another entry shows, and asqr does
  not keep it (spec 8.5).
- The question is capped at 100 columns (`READING_WIDTH`,
  `src/tui/render/panes.rs:20`, ADR 30): on a terminal of 230 columns
  question texts ran across all of it, and 100 was the first value tried
  from a planned 100 to 110. `MIN_QUESTION_WIDTH` (60) and
  `MIN_CONTEXT_WIDTH` (40) decide when context fits beside it.
- Context placement: `--layout auto|beside|below` and `ASQR_LAYOUT`
  (`src/cli/mod.rs:87-91`, `:272`), checked only when asqr watches
  (`src/cli/mod.rs:385-391`, spec 6.2).
- Environment variables are read through a lookup closure
  (`environment_from`, `src/cli/mod.rs:259-286`), which tests replace.
- The spec's list of variables (6.2) misses several the code reads:
  the terminal names in `src/terminal_env.rs`, `TMPDIR`, the skill
  directory variables, `VISUAL`/`EDITOR` through the `edit` crate.

## Decisions (Jakob, 2026-10-30, in the chat)

- **A settings file, with environment variables and flags above it.**
  asqr reads a TOML file in the platform config directory
  (`~/Library/Application Support/asqr/settings.toml` on macOS,
  `$XDG_CONFIG_HOME/asqr/settings.toml` on Linux); a variable or a flag
  wins over the file for that run. A new ADR reverses ADR 15's "no
  config file". Rejected: environment variables and flags only, which
  cannot hold a first-start answer.
- **The file follows each platform's convention,** through the
  `directories` crate asqr already uses for its data and cache
  (`ProjectDirs::config_dir`): `~/Library/Application Support/asqr/` on
  macOS, beside the queue data, and `$XDG_CONFIG_HOME/asqr/` on Linux.
  No new crate. Rejected: `~/.config/asqr` on macOS too (as Ghostty and
  git do, for example through the `etcetera` crate's XDG strategy);
  reading `~/.config/asqr` first and the Apple path after.
- **TOML,** so the file takes comments and is easy to edit by hand;
  the `toml` crate (pure Rust). Rejected: JSON.
- **A first-start screen.** The first time the TUI starts without a
  settings file, it asks the few settings that matter, writes the file
  and does not ask again. Rejected: start with defaults and only name
  the file.
- **A settings pane in the TUI.** One key opens it; each setting offers
  a fixed set of values (Jakob, 2026-10-28: fixed choices rather than
  free values). Rejected: editing the file only.
- **Four settings** (the detail round below makes version 1 the three
  asqr has today; keeping files joins with its feature):
  - keep files: `compact`, `exact`, `off` (todo
    01M67A73S1SRPTR8YRZPPC4FSH, default `compact`; `ASQR_KEEP_FILES`
    overrides it);
  - diff layout: `unified` or `side-by-side`, how every diff starts;
  - reading width: fixed steps 80, 100, 120 or the full width, 100 by
    default. Rejected: any number from 60 to the terminal's width;
  - context placement: `auto`, `beside`, `below`, as `--layout` and
    `ASQR_LAYOUT`, which keep overriding it.
- **`s` stays as it is:** it switches the diff shown; the setting only
  decides how every diff starts. A trial switch never becomes the
  setting. Rejected: switching for the rest of the run; saving on `s`.
- **The reading width does not move the beside or below choice,**
  which keeps using the terminal's width and shape (ADR 30); the width
  only caps the question. Forcing a side is the placement setting's
  job. Rejected: letting the width move it.

- **Look: the design "Sibling"** from the design study of 2026-10-30
  (conductor logbook `plans/settings-design/`, `index.md` and
  `png/d1-*.png`): the settings pane and the first-start screen use the
  queue switcher's frame; the first start is the same frame with its
  questions open and a "Save and start" button. How values are shown,
  changed and explained, and the texts, were settled in the detail
  round below, which replaces the study's `enter` and option rows. Rejected: "Dogfood" (the
  settings as an asqr session), "Switchboard" (a full page),
  "Drawer" (a band above the key bar). The study proposes `,` as the key
  and a first start that asks only about keeping files and the update
  check; the detail round settles both.

- **Detail round (Jakob, 2026-10-30, in the chat):**
  - **Version 1 is the file and the settings pane** with the settings
    asqr has today: diff layout, reading width, context placement. The
    first-start screen comes with the first setting that needs a
    deliberate answer, keeping files (todo 01M67A73S1SRPTR8YRZPPC4FSH),
    and the update check joins it later. Rejected: a first start now
    that asks the taste settings, which nobody can judge before a
    session; doing the archive work first.
  - **`,` opens the settings,** free on every screen and the key for
    settings on macOS. Rejected: `S`, next to `s`, which switches a
    diff.
  - **`←`/`→` change the value right on the row** and save it, one key
    per change. Each value needs an explanation on screen, and the row
    has to show that `←`/`→` cycle through its values (the look of
    both is decided below). Rejected: `enter` opening the setting in
    place with its values as option rows (two keys per change).
  - **The explanation sits in the frame's footer,** below the list, as
    the queue switcher's footer explains its row: it always explains
    the row under the cursor (what the setting does, what the chosen
    value means), and the frame does not jump as the cursor moves. The
    row under the cursor shows `‹ value ›` to say that `←`/`→` cycle
    its values. Rejected: two explanation lines under the focused row;
    every value shown on the row.
  - **A setting set outside asqr is locked and explained in the
    footer.** Its row shows the value in effect dimmed, without `‹ ›`,
    and beside it, dimmed, what sets it: "set by ASQR_LAYOUT" or "set by
    --layout", the word to look for in a shell setup or an alias (Jakob
    left the name beside the value to the lead; the lead keeps it).
    `←`/`→` do nothing there and no error notice appears. The footer says in full sentences what holds, why and what
    to do, without developer shorthand, for example: "Your shell sets
    ASQR_LAYOUT=below, so asqr always shows context below the question,
    whatever you choose here. To choose here, remove ASQR_LAYOUT from
    your shell setup (for example ~/.zshrc) and start asqr again." For a
    flag: "You started asqr with --layout below, so this run shows
    context below the question. Without the flag, the next start uses
    your choice here." Rejected: changing the file's value while the
    override holds; the design study's "from ASQR_KEEP_FILES" with a
    notice on `enter`.
  - **All texts of the pane are written for someone who has never read
    the docs.** Agreed with Jakob on two renders (conductor logbook
    `plans/settings-design/v2/png/v2-1-pane.png`, `v2-2-override.png`,
    generator `v2/build_v2.py`), the pane's texts are:
    - rows and values on screen: "Diffs open as": "one column"
      (default), "side by side"; "Max question width": "80 columns",
      "100 columns" (default), "120 columns", "full width"; "Context":
      "beside or below" (default), "always beside", "always below". The
      TOML file may keep short technical values (such as `unified`);
      the pane shows these names.
    - footer, what the setting does: "How a diff shows when you come to
      it. The s key still switches the diff you look at." / "The widest
      a question's text may get; on a narrower terminal it gets
      narrower." / "Where the files, images and diffs that come with a
      question show."
    - footer, what the value means, "(default)" after the default:
      "One column: removed and added lines one below the other." /
      "Side by side: the old version on the left, the new one on the
      right." / "100 columns: lines wrap after at most 100 characters."
      (and so on) / "Full width: lines use the whole width of the
      terminal." / "Beside or below: beside the question when the
      terminal is wide enough, otherwise below it." / "Always beside:
      next to the question whenever it fits, even on a tall terminal."
      / "Always below: under the question whenever it fits, even on a
      wide terminal."
    - the override texts above; for `beside` they say "beside the
      question when it fits".
    Jakob, 2026-10-31: the first wording of the two "Always" texts
    ("even on a narrow terminal") was untrue, since beside needs 101
    columns and below a minimum height whatever the setting; "always"
    only overrules the terminal's shape. Rejected: naming the 101
    columns in the text; changing only the beside text.
    Jakob found "Question width" unclear, since it is a maximum; the
    label is "Max question width". Rejected: "up to 100 columns" as the
    values.
  - **The key bar shows only what works:** on a locked row it drops
    `←/→ change` (lead).
  - **The first-start texts are rewritten with Jakob** when the
    first-start screen is built: the drafts in the design study
    (`d1-1`, `d1-2`) do not say clearly what each choice means.
  - **A saved change shows a green notice in the status line,** such as
    "✔ saved diff layout: side-by-side", like the notices after a queue
    switch; a write that fails shows a red notice and the value holds
    for this run only. Rejected: no notice.

## Technical notes (lead, 2026-10-30)

- The `directories` crate, which asqr already uses for the data and
  cache directories, gives the config directory too
  (`ProjectDirs::config_dir`); a helper goes beside `platform_data_dir`
  (`src/queue/location.rs:257-266`). On macOS config and data are the
  same folder, on Linux `$XDG_CONFIG_HOME/asqr` and
  `$XDG_DATA_HOME/asqr`.
- Precedence: flag over environment over file over built-in default.
  Spec 5.2 and 6.2 state it; spec 6.2 also gets the full list of
  variables asqr reads.
- A file that does not parse, or a value asqr does not know, never stops
  a command: asqr warns (in the log, and in the TUI's status line when
  watching) and uses the default for that setting, like `ASQR_LAYOUT`
  today, which only `watch` checks.
- The settings pane writes the file; it keeps the comments and keys it
  does not know where the `toml` crate's editing allows, or the plan
  says what it drops.
- The first-start screen does not run for commands agents call (`ask`,
  `wait`, `result` and so on), only for the TUI.
- The update check (todo 01M5J41K58B5CF23DN61PWM6MC) would join as a
  setting and a first-start question once it is decided.
- Docs: a new ADR amending ADR 15 and ADR 30 (reading width as a
  setting), spec 5.2, 6.2, 7.x (first start, settings pane, help),
  8.5, README.
