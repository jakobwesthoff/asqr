---
title: "Write the stamped SKILL.md last when installing the skill"
kind: bug
component: cli
origin: review
---
# Write the stamped SKILL.md last when installing the skill

Found on 2026-11-17 in an opus review of the same install code in ntropy,
where it was fixed in ntropy `39ca28e`. `asqr skill --install` writes the
core `SKILL.md`, which carries the `asqr-version` stamp, before it
replaces `references/`. An install that fails after the core is written
leaves a copy stamped as current over missing or stale parts, and the
outdated-copy warning then stays silent about it.

## Problem

`src/cli/skill.rs`, `write_skill`: it writes `SKILL.md` first, then
removes `references/` whole and writes each part. A failure in the
removal, in `create_dir` or in any part write (disk full, permissions)
returns an error, but the new stamp is already on disk. Read in the code,
not reproduced (2026-11-17).

## Impact

After a failed install, `asqr skill` and `asqr --version` treat the copy
as current, so nothing tells the user that its parts are missing or from
an older asqr. An agent loading it reads a core that names parts that are
absent or out of date.

## Suggested fix

Write the parts first and the stamped `SKILL.md` last, so the stamp only
changes once every part is in place, as ntropy does since `39ca28e`
(`src/skill/mod.rs` there). The order of the "installed" lines changes
with it; update any test or doc that shows the output.

## Relations

- Relates to: antiq todo 01M7N352SG6G6CS5MVB82ESSQK (Write the stamped SKILL.md last when installing the skill), the same write order
