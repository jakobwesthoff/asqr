---
title: "The skill and other agents"
tags: [guide/agents]
site:
  order: 2
---
asqr ships a skill that teaches your agent how to ask you through asqr.
Your agent reads it from asqr itself, so in most cases nothing needs
installing. This page says what the skill holds, how to install it as a
file, and where agents other than Claude Code keep skills.

## What the skill is

The skill teaches an agent the session format, the waiting pattern, the
exit codes and what to do about each of them. It is built into the binary:
`asqr skill` prints the core of the skill that matches the asqr that runs,
and `asqr skill <part>` prints one of its parts, `context`, `withdrawing`
or `patterns`. An agent you tell to use asqr can read the skill from the
binary, so installing it is possible but mostly not needed.

The skill tells an agent to use asqr only when you explicitly ask it to,
for example "ask me through asqr". Without such a request an agent asks
the way it normally does.

`asqr --help` tells agents to read the skill first. Where agents tend to go
wrong, asqr adds a line that starts with `hint:` after its output, and the
skill tells them to follow it.

## Installing the skill as a file

To give Claude Code a copy of the skill, run this in your project:

```sh
asqr skill --install .claude/skills
```

It writes `asqr/SKILL.md` and its parts in `asqr/references/` under the
skills directory you name. `.claude/skills` in a project and
`~/.claude/skills` for every project are skills directories of Claude Code.
Running the install again replaces the copy, parts included.

An installed copy stays as it is when asqr is updated. `asqr ask` and
`asqr validate` print a hint when they find a copy older than asqr: it
tells your agent to read the current skill with `asqr skill` and to ask you
whether to update the copy.

## Other agents

asqr looks for installed copies of the skill in the skills directories of
Claude Code, Codex, Gemini CLI, GitHub Copilot and Cursor: `.agents/skills`,
`.claude/skills`, `.gemini/skills`, `.github/skills`, `.cursor/skills` and
`.codex/skills` in the project, searched up to the git root, and
`~/.agents/skills`, `~/.claude/skills`, `~/.gemini/skills`,
`~/.copilot/skills`, `~/.cursor/skills` and `~/.codex/skills` in your home
directory. Where `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `COPILOT_HOME` or
`GEMINI_CLI_HOME` holds an absolute path, asqr searches the skills
directories below it too. `asqr skill --install <directory>` writes the
skill under the directory you name.
