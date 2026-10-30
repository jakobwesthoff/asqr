---
title: "Write a user guide with screenshots, every key and the agent setup"
kind: docs
component: project
origin: request
tags: [ux]
---
# Write a user guide with screenshots, every key and the agent setup

Requested by the user on 2026-10-15. asqr's documentation today is the
README (what asqr is, install, flags), the specification
(`docs/spec.md`, which describes behaviour precisely but reads as a
spec, not a guide) and the skill that `asqr skill` prints for agents.
There is no guide that shows a person how asqr looks and how to work
with it, and no step-by-step description of setting it up with an agent.

## What the guide should cover

- How asqr works, with screenshots of the real screens: the empty queue,
  a session with its tabs, options, own answers and notes, the context
  pane beside and below, the review tab, the help, the session list.
- Precise explanations of the interaction, including the less obvious
  keys. Example the user named: `ctrl-u`/`ctrl-d` (and
  `pgup`/`pgdn`) page the context pane even while it does not have
  the focus (`src/tui/scroll.rs`, the doc comment of `scroll`), while
  the line keys only scroll a focused pane.
- How to bring asqr into agents: the skill (`asqr skill`), where the
  queue lives, how an agent writes a session and reads the result, with
  a worked example for Claude Code and others.

## Decisions (Jakob, 2026-10-30, in the chat)

- **An ntropy vault,** so a website can be built from it later as the
  ntropy project does: ntropy keeps its site as the vault
  `docs/website` and exports it with
  `ntropy --vault docs/website -n site -o dist --strict` in its
  `.github/workflows/pages.yml`. Rejected: `docs/guide.md`; a long
  README section; a project-page-starter site.
- **The vault now, the website later.** The notes read on GitHub
  meanwhile; a Pages workflow is its own step once the content stands.
- **Pictures drawn by asqr's own code, with a Rust tool in the
  repository.** A small dev crate, `tools/screens/`, not published and
  run with `just screens`, draws each screen through asqr's public API
  (`asqr::tui::App`, keys through `App::handle`,
  `asqr::tui::render::draw` into a ratatui `TestBackend`) and rasterizes
  the cell buffer straight into PNG in pure Rust (for example
  `ab_glyph` with the `image` crate asqr already uses). Screens are data,
  one small TOML file per picture: the sessions, the keys to press, the
  terminal size, the queue state, the caption. A release refreshes every
  picture with one command. The tool has a README of its own on how to
  use it. Rejected: VHS tapes; screenshots by hand; keeping the
  prototype from the 2026-10-30 conductor run as it is (a Rust program
  that writes SGR text plus Python scripts that parse it again and
  render with macOS fonts, with hard-coded paths and screens; logbook
  `~/playground/2026_10_30_asqr-todos-worked-off-by-agents/plans/instances-impl/helpers/capture/`
  and `render_png.py`, the model for the new tool); hanging the pictures
  off the insta snapshot tests, so that accepting a snapshot never
  changes a picture.
- **Jakob's terminal look in every picture,** from his Ghostty config:
  - colours: Gruvbox Material Hard dark (sainnhe/gruvbox-material, MIT),
    background `#1d2021`, foreground `#d4be98`, palette 0 to 15
    `#1d2021 #ea6962 #a9b665 #d8a657 #7daea3 #d3869b #89b482 #d4be98
    #eddeb5 #ea6962 #a9b665 #d8a657 #7daea3 #d3869b #89b482 #d4be98`;
  - font: Victor Mono, SemiBold for regular text, Bold, SemiBold Italic
    and Bold Italic (Ghostty uses the Nerd Fonts build "VictorMono Nerd
    Font Mono" at 14 pt).
- **Fonts committed with their licences.** The upstream Victor Mono
  (rubjo/victor-mono) is under the SIL Open Font License 1.1 with no
  reserved font name (checked 2026-10-30), so it may be redistributed.
  The Nerd Fonts build is not committed: its metadata says "All rights
  reserved" and it carries icon sets under licences of their own, and
  asqr draws no Nerd Font icon. Victor Mono lacks `✎ ☐ ☒ ✓ ✔ ✗ ⏎ ⎋`,
  which Ghostty takes from macOS fallback fonts; DejaVu Sans Mono
  (Bitstream Vera licence, free) has all of them and is the fallback.
  A test checks that the fonts cover every glyph asqr draws.
- **Upstream Victor Mono with Ghostty's cell proportions** (Jakob,
  2026-10-30). For every glyph asqr draws the Nerd Fonts build and the
  upstream font are the same (1241 of 1267 upstream glyphs identical in
  outline and advance; the rest are combining marks), but the Nerd build
  has taller line metrics (hhea ascent and descent 1200 and -350 against
  1100 and -250, at 1100 units per em), so Ghostty's cells are taller.
  The tool sets cell size and baseline to what Ghostty derives from the
  Nerd build at 14 pt. Rejected: committing the Nerd build (10 MB for
  four styles plus the icon sets' licences); local font files that
  override the committed ones, which would make pictures depend on the
  machine. macOS's own fallback fonts (Apple Symbols, Menlo), which
  Ghostty uses for glyphs Victor Mono lacks, cannot be committed.
- **Later, real images:** as the tool draws the PNG itself, it can
  paste the actual picture into the context pane where asqr places an
  image, instead of half blocks, once asqr's drawing reports where it
  placed one. Not part of the first step.
- **The full tour:** every screen: the empty queue, a session with its
  tabs, options, own answers and notes, context beside and below, full
  screen, diffs unified and side by side, the review tab, the help, the
  session list, the queue switcher, the start picker, the invalid-session
  dialog, and the less obvious keys (the example above). Rejected: a
  quick start with 5 to 8 pictures first.
- **Agent setup as steps plus one worked example for Claude Code:**
  install the skill, start asqr in the agent's tmux session, ask the
  agent to put its questions through asqr, answer, see the result come
  back. The guide points to `asqr skill` for the format. Rejected:
  examples for several agents; copying the format into the guide.
- **Two or three task patterns with a picture each,** from
  `asqr skill patterns`, so the person sees what agents can ask
  (`examples/sessions/alt-rework-batch.json` and `mascot-poses.json`
  can feed them). Rejected: only pointing to the skill.
- **Talqi at the top of the guide's start page** (`assets/talqi-512.webp`);
  the README logo stays. Rejected: also in the README.
- **English only,** like the README, the spec and the skill.

## Technical notes (lead, 2026-10-30)

- The tool comes first, as its own step; the vault and its pages
  follow.
- The prototype's sessions come from `examples/sessions/`; new
  sessions written for the guide go there too, so the README examples
  and the pictures share sources.
- The key table follows spec 7.6; a check that fails when they differ
  is cheap and goes in with the guide.
- `Cargo.toml` excludes `docs/` and `assets/` from the crate, so links
  from the README into the guide are absolute GitHub URLs.
- The vault's layout (one note per screen or per topic) and its
  `ntropy` views are the plan's to propose, following ntropy's own
  `docs/website`.

## Relations

- Relates to: [Add a VHS demo GIF to the README](01M3HNJJXAVRC9TJWRTPPTFNC0-add-a-vhs-demo-gif-to-the-readme.md)
