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
- **Pictures drawn by asqr's own code.** The capture program from the
  2026-10-30 conductor run (logbook
  `~/playground/2026_10_30_asqr-todos-worked-off-by-agents/plans/instances-impl/helpers/capture/`)
  draws screens through asqr's public API (`asqr::tui::render::draw`
  into a ratatui `TestBackend`) and writes SGR text, and
  `plans/instances-impl/render_png.py` there renders that into PNGs on
  a fixed cell grid. It moves into the repository with a `just` recipe,
  so a release refreshes every picture. Images in the context pane do
  not show this way (no Kitty protocol); those screens get a stand-in or
  a manual capture. Rejected: VHS tapes; screenshots by hand.
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

- The capture program's sessions come from `examples/sessions/`; new
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
