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

## Open questions

- Where the check runs: `asqr validate` and `asqr ask` (where an agent
  meets it), the TUI on start, `asqr status`, or a separate
  `asqr skill --check <dir>`.
- How an installed skill is found and identified: a version line in the
  installed file's frontmatter, compared with the binary's version, and
  which skill directories asqr knows to look in (`.claude/skills`, the
  user's home, others).
- Whether `--install` should also offer to update an existing older copy
  in place, and whether asqr should name the command to run.
