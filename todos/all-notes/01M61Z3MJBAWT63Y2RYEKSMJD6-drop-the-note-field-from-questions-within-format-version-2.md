---
title: "Drop the note field from questions within format version 2"
kind: refactor
component: format
origin: request
---
# Drop the note field from questions within format version 2

Every question and every info item takes a note (decided by the user on
2026-10-28; ADR 68, "Every item takes a note"). Format version 2 keeps
reading `"note": false` only so that released session files stay valid:
the field has no effect, and `validate` and `ask` warn that it is
ignored. The user asked on 2026-10-28 that the field go completely.

## Decisions (Jakob, 2026-10-31, in the chat)

- **No new format version.** The field leaves format version 2: unknown
  fields are ignored and reported in every version (ADR 28, spec 3.4),
  so a file that still sets `note` stays valid. Rejected: a version 3
  that reads version 2 as well; a version 3 that refuses version 2 (it
  would refuse every queued session and every agent with an older skill,
  for a change that needs no version). Nothing else is planned for a
  version 3.
- **A file that still sets `note` gets a pointed warning** and the field
  is ignored, in the style of the `image` warning
  (`src/format/warnings.rs`), for example
  `questions[0].note: ignored; every question takes a note`. Rejected:
  the generic unknown-field warning; refusing the file.
- **The skill drops its line about `note`** (`skills/asqr/SKILL.md`,
  "Do not set `note`; it has no effect"); the guidance that the person
  can add a note to every question stays. Rejected: keeping a line that
  says the field was removed.

## What to do

- Remove `note` from the question and info item shape, the schema in
  `schema/`, the spec's field list and the skill line above.
- Keep the warning, now as the pointed warning for a field asqr no
  longer has; remove parsing that exists only for the old behaviour.
- Keep `ask` removing `"note": false` from the file it drops
  (`src/cli/ask.rs`, `drop_note_false`): an asqr up to 0.13.1 watching
  the queue would still turn notes off.
- A new ADR records the decision; ADR 68 gains its status link.
