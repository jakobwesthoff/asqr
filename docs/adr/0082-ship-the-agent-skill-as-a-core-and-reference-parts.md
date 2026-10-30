# 82. Ship the agent skill as a core and reference parts

Date: 2026-10-30

## Status

Accepted

Amends [6. Integrate agents through a bundled skill first](0006-integrate-agents-through-a-bundled-skill-first.md):
the skill is a directory of a core and reference parts, `asqr skill
<part>` prints a part, and an installed skill is the one of the asqr
that installed it

## Context

- The skill is 33,839 bytes and 626 lines. An agent reading it in parts
  can stop before the rules on reading the result and leave a result
  unread, its notes included.
- No rule says that a session of info items only comes back as a result
  to read.
- Claude Code's Bash tool returns output inline "up to roughly 30,000
  characters by default; past that, the path of a file saved to the
  session directory ... plus a preview of up to the first 2,000
  characters" (<https://code.claude.com/docs/en/tools-reference.md>).
  After compacting a conversation, Claude Code re-attaches each skill it
  had loaded, "keeping the first 5,000 tokens of each"
  (<https://code.claude.com/docs/en/skills.md>).

## Decision

- `skills/asqr/SKILL.md`, the core, holds every rule whose violation no
  asqr check catches, and the ask, wait and read procedure, which comes
  before the session format. It states each rule once. A test caps it
  at 17,945 bytes, so a tool that returns up to roughly 30,000
  characters inline shows it whole. The cap is the size of the core with
  every rule stated once; the example session, the exit codes and the
  rules on info items, on writing for a person who has not seen the
  agent's work and on waiting for and reading every result keep it above
  16,000 bytes.
- Three parts under `skills/asqr/references/` hold details that
  `asqr validate` and `asqr ask` already guard, or that apply on one
  branch of the work: `context.md` (the context entry types, how asqr
  shows them, their limits), `withdrawing.md` (`asqr withdraw`,
  `ask --replaces`, `--force`, refusals, a `draft`) and `patterns.md`
  (the task patterns). A test keeps each under 30,000 bytes.
- The core's first paragraph tells the agent to read it to its last
  line and names that line, `End of the asqr skill.`, so the agent can
  check whether it saw all of it. Each part starts by naming the core
  and ends with a line of its own.
- The core names each part by its relative path and by
  `asqr skill <part>`, in a list at the top and at the step that needs
  it. A host that loads the skill directory reads the file; an agent
  that only ran `asqr skill` has the command.
- `asqr skill` prints the core and `asqr skill <part>` one part. No
  option prints them all at once, since that output would again exceed
  30,000 characters. An unknown part is a usage error that names the
  parts and says that `asqr skill` prints the skill of this asqr, since
  the skill that named the part may belong to another version.
- `asqr skill --install <dir>` writes `<dir>/asqr/SKILL.md`, replaces
  `<dir>/asqr/references/` whole and leaves any other file in
  `<dir>/asqr/` alone. asqr owns `references/`.
- Tests pin the structure and ten key rules of the core, each by a
  short phrase, so a later rewrite cannot drop one silently, and
  rewording around a phrase needs no test change.

## Consequences

- An installed skill is the one of the asqr that installed it. After an
  update of asqr it is older until `--install` runs again, which then
  replaces it whole, parts included. `asqr skill` always prints the
  skill of the asqr that runs it.
- A copy of `SKILL.md` alone lacks the parts as files; `asqr skill
  <part>` still prints them.
- What a part holds reaches an agent only when it opens the part.
