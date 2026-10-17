---
title: "Teach the asqr skill task patterns that show what a session can do"
kind: improvement
component: project
status: needs-discussion
origin: discussion
tags: [ux]
---
# Teach the asqr skill task patterns that show what a session can do

Raised by Jakob on 2026-10-18 while collecting example prompts for an
asqr website. The bundled skill (`skills/asqr/SKILL.md`, served by
`asqr skill`) explains the session format and lists when asqr fits
("When to use it"), but it has no examples of what a session can look
like for a given kind of task. Jakob wants a collection of task
patterns in the skill: for a kind of task, what the agent can put in
front of the person in asqr. An agent that loads the skill then knows
what to build for those tasks, and can derive something good for tasks
the list does not name. How exactly this goes into the skill is open.

The patterns are suggestions, not prescriptions: a starting point the
agent can consider, adapt or extend for the task at hand, and leave
aside where it judges something else fits better (Jakob, 2026-10-18).

## Context

The patterns come from the website discussion of 2026-10-17 and
2026-10-18. That discussion started from a scan of real use (Claude
Code transcripts and about 105 answered sessions in the asqr archive,
2026-10-17). By far the most frequent use was choosing between
generated image variants, followed by "grill me on the open questions
before you implement", then review triage with diffs and asqr as a
choice step inside a pipeline. Jakob liked the form "short task, then
what asqr can show you for it"; the second half describes what asqr is
able to show, not what a given prompt produces word for word.

## A first, generalized list

Each pattern names the task and what the session can hold. To be
discussed and worked out.

1. **Compare visual variants** (mockups, renders, screenshots, icons).
   One question per thing to decide, an image as the context of each
   option so the person flips through them; a single pick, or a multi
   pick of the keepers.
2. **Debug walk-through.** The findings as Markdown context, each
   candidate fix as code on its own option, the tests the agent would
   add as a multi pick, a note for anything else.
3. **Triage a backlog** (bug reports, todos, dependency updates). One
   question per item with its evidence (screenshot, log, code), the
   agent's take and fix idea in the description, options such as
   pursue, defer or close, the recommendation as the default.
4. **Restructure a document.** One question per section with its text
   as Markdown context, where it would go and why as options, the own
   answer for "somewhere else", a note on any section.
5. **Review findings.** One question per finding with the diff, the
   before and after in two sentences and the risk; options such as
   comment, fix or drop; answered whenever the person gets to it.
6. **Rating grid.** Several questions per candidate (too long, right,
   too short; tone; humour; substance), then a follow-up round built
   from the ratings (`follows`).
7. **Pick from a batch inside a pipeline.** The candidates of one step
   as a multi pick; the next step runs on the chosen ones only.
8. **Decide before implementing.** The open questions of a plan in one
   session, the recommended option first and as the default, each
   option's trade-off in its description, the current state as Markdown
   context.

Habits that run through them: one question per item, the recommendation
first, before and after spelled out, context per option where the
person compares, a follow-up round for what came up.

## Open questions

- Where the patterns go: a section of `SKILL.md`, or a reference file
  next to it (the skill is one string today, `include_str!` in
  `src/cli/skill.rs`, so a second file needs a way to ship it).
- How long the section may get without crowding out the format
  reference.
- Whether each pattern carries a short JSON sketch of the session or
  stays prose.
- How the patterns relate to "When to use it", which keeps asqr to
  explicit requests.
- Whether the website and the skill share one list.

## Relations

- Relates to: [Write a user guide with screenshots, every key and the agent setup](01M50JVCC635YS0REQGD31MBBQ-write-a-user-guide-with-screenshots-every-key-and-the-agent-setup.md), the guide could show the same patterns
