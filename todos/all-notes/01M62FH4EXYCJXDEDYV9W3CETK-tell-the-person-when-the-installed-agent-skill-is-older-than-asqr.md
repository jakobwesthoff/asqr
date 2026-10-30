---
title: "Tell the person when the installed agent skill is older than asqr"
kind: feature
component: cli
status: needs-discussion
origin: review
---
# Tell the person when the installed agent skill is older than asqr

`asqr skill --install <dir>` copies the skill out of the binary once.
Nothing updates it with asqr or checks that it still matches, so an
agent keeps following an old skill after an upgrade and cannot use what
the new asqr offers, or writes sessions the new asqr refuses.

## Problem

The installed `asqr/SKILL.md` carries no version, and asqr never looks
at it. A skill installed at 0.9 teaches format version 1, which asqr
0.11 and later refuse (`src/format/validate.rs`: "this asqr no longer
supports format version 1; `asqr skill` describes the current format").
A skill from 0.11 to 0.13 knows nothing about info items, captions,
`asqr withdraw` or the task patterns of 0.14. Found in the review of the
LinkedIn post for 0.14.0 on 2026-10-29, which now tells readers to rerun
the skill command after an update; the user asked for this todo the
same day. Read in the code, not reproduced.

## Impact

People who upgrade with brew or cargo keep an agent that does not know
the new features, or that writes sessions asqr refuses, until they
remember to reinstall the skill by hand.

## Settled by the skill split (2026-10-30, asqr session 01M663RDEWA9W6SVK9KWFMC1ZV)

Landed with ADR 82:

- The installed skill is the directory `asqr/` with `SKILL.md` and
  `references/` (`context.md`, `withdrawing.md`, `patterns.md`); a check
  treats the directory as the unit.
- `--install` writes `SKILL.md`, replaces `references/` whole, so parts
  an older asqr wrote disappear, and leaves other files in `asqr/` alone.
  Reinstalling is a complete update in place.
- `asqr skill <part>` with an unknown part exits 2, names the known parts
  and says that `asqr skill` prints the skill of this asqr: a first,
  passive signal of a mismatch.
- `SKILL.md`'s frontmatter carries `metadata:` with
  `asqr-version: "<version>"`, equal to the crate version; a test in
  `tests/cli_skill.rs` fails until a release sets it (Jakob chose this
  over a marker file written on install, since only `--install` would
  write a marker and hand copies would have none).


## Decided (2026-10-30, asqr session 01M66KP4BA5TMHBDRYRH7VMYKR)

- The check runs in `asqr ask` and `asqr validate`, where an agent
  reads it, as a `hint:` line on stderr. No separate `--check` command.
  Jakob's note: the hint also tells the agent how to get the current
  skill, by running the command (`asqr skill`).
- A copy without `asqr-version` (installed by 0.14 or earlier) counts
  as older; the hint then names no version.
- Directories: project directories from the current directory up to
  the git root, plus the home directory. Jakob's note: asqr is not only
  for Claude Code. The list of directories lives in a module of its own
  together with the check functions, so it can be extended.
- Several old copies go into one hint (lead).

Research on where agent runners look for skills (2026-10-30, kept in
the run's logbook as `plans/skill-age/skill-dirs-research.md`):
vercel-labs/skills 1.7.4 (commit 31b7451, `src/agents.ts`) knows about
70 runners. Most read the shared `.agents/skills` (Codex, Gemini CLI,
Copilot, Cursor, Cline, Amp, OpenCode, Zed and others). By default the
installer writes one real copy to `.agents/skills/<name>` and symlinks
each other agent's directory (such as `.claude/skills/<name>`) to it, so
copies are compared by their real path. The installer anchors project
paths at the current directory; Claude Code and Codex walk parents up to
the repository root. Documented per agent: Claude Code `.claude/skills`,
`~/.claude/skills`; Codex `.agents/skills`, `~/.agents/skills`; Gemini
CLI `.gemini/skills` and `.agents/skills`, also in home; Copilot
`.github/skills`, `.claude/skills`, `.agents/skills`,
`~/.copilot/skills`, `~/.agents/skills`; Cursor `.agents`, `.cursor`,
`.claude` and `.codex` skill directories, also in home. The doc pages
were read through a summarising fetch; re-read them before pinning
paths in code.

Still open: which set of directories (asked in asqr session
01M66M0CNZPVC10PC088D65K1K: the shared one plus those the big agents
document, all of the installer's runners, or only `.agents/skills` and
`.claude/skills`), and the hint's exact text.

Jakob's note (2026-10-30, asqr session 01M664CT1H5WE3J1MT97JNYGDT): the
docs should say that installing the skill is possible but in most cases
not needed, as an agent told to use asqr can read the current skill from
the installed binary (`asqr skill`). A skill read that way never ages, so
this check matters only for people who install a copy. The README, spec
section 9 and `asqr skill --help` say so since the skill split.
