# 87. Tell the agent when an installed skill is older than asqr

Date: 2026-10-30

## Status

Accepted

Amends [82. Ship the agent skill as a core and reference parts](0082-ship-the-agent-skill-as-a-core-and-reference-parts.md):
`asqr ask` and `asqr validate` compare the `asqr-version` of installed
copies of the skill with their own version and hint when a copy is older

## Context

- `asqr skill --install` copies the skill once. A copy stays as it was
  when asqr is updated (ADR 82).
- The core's frontmatter carries `asqr-version`, equal to the crate
  version (ADR 82). Copies installed by asqr 0.14 or earlier carry none.
- A skill from an older asqr can teach a session format that this asqr
  refuses.
- Agent runners read skills from directories of their own.
  Claude Code loads project skills from `.claude/skills/` in the
  directory it starts in and every parent up to the repository root
  (<https://code.claude.com/docs/en/skills.md>). Codex scans
  `.agents/skills` in every directory from the working directory up to
  the repository root and reads `$HOME/.agents/skills`
  (<https://learn.chatgpt.com/docs/build-skills>). Gemini CLI reads
  `.gemini/skills` and `~/.gemini/skills`, and the alias `.agents/skills`
  in both places (<https://geminicli.com/docs/cli/skills/>). GitHub
  Copilot reads `.github/skills`, `.claude/skills` and `.agents/skills`
  in a project and `~/.copilot/skills` and `~/.agents/skills` for a person
  (<https://docs.github.com/en/copilot/concepts/customizations/agent-skills>).
  Cursor reads `.agents/skills`, `.cursor/skills`, `.claude/skills` and
  `.codex/skills` in a project, and `~/.agents/skills`,
  `~/.cursor/skills`, `~/.claude/skills` and `~/.codex/skills` for a
  person (<https://cursor.com/docs/skills>).

## Decision

- `asqr ask` and `asqr validate` print one hint on stderr when an
  installed copy of the skill is older than the asqr that runs. It is the
  first line they print. There is no separate command for the check.
- A copy is the directory `asqr` with a `SKILL.md` in a skills directory.
  The project directories are `.agents/skills`, `.claude/skills`,
  `.gemini/skills`, `.github/skills`, `.cursor/skills` and
  `.codex/skills`, in the current directory and each directory above it
  up to the git root, and the global ones are `~/.agents/skills`,
  `~/.claude/skills`, `~/.gemini/skills`, `~/.copilot/skills` and
  `~/.cursor/skills`. Each entry is a location that the documentation of
  an agent names. The table does not cover the roughly 70 runners of the
  npm `skills` installer: that would be about 70 directories per level,
  and a table of another project to keep in step.
- The git root is the nearest directory with a `.git`, a directory or,
  in a linked worktree, a file. Without one only the current directory is
  searched. asqr does not start git to find it.
- A copy is older when its `asqr-version` is below this asqr's version
  or missing. A copy of the same or a newer version, an unreadable file
  and a stamp that is no `MAJOR.MINOR.PATCH` version give no hint and no
  message.
- Copies are compared by the real path of their `SKILL.md`: the
  installer links the directories of the agents to one real copy, and
  that copy is named once. One hint names every older copy.
- The hint names the copy and the version it is from, tells the agent to
  read the current skill with `asqr skill` and to ask the person whether
  to update the copy with `asqr skill --install <skills directory>`. A
  copy without a stamp is "from an older asqr".
- The hint repeats on every `ask` and `validate` until the copy is
  updated. There is no way to turn it off.
- The directory table, the search and the text of the hint are in
  `src/cli/skill_age.rs`, which `ask` and `validate` call once each.
- Installing the skill stays possible and in most cases unneeded: an
  agent told to use asqr reads the current skill with `asqr skill`.

## Consequences

- Every `ask` and `validate` looks for up to six skills directories per
  directory from the current one to the git root, and five in the home
  directory. Each is one failed `open` when no copy is there.
- The stamp equals the crate version, so the check compares versions and
  not builds.
- In a linked worktree the walk ends at the worktree root and does not
  look at the main checkout. Claude Code from v2.1.277 on loads the main
  checkout's project skills when the worktree has no `.claude/skills` at
  its root (<https://code.claude.com/docs/en/skills.md>).
