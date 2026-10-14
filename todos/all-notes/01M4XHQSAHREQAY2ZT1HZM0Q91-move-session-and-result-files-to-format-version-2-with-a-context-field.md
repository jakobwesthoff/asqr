---
title: "Move session and result files to format version 2 with a context field"
kind: feature
component: format
horizon: next
origin: discussion
tags: [milestone/v0-11]
---
# Move session and result files to format version 2 with a context field

Step 1 of the context pane plan (design settled with the user on
2026-10-14). Session and result files move to format version 2.
Questions and options get a `context` field that replaces `image`, and
asqr stops accepting version 1. The other steps build their viewers on
this field.

## Proposal

The plan's Design section, *Format version 2*, has the full design. In
short:

- `context` on questions and options: a list, limited to one entry
  until phase 2 lifts the limit. Entry types `image` (`path`), `text`
  (`path` or `text`, `language`, `render`) and `diff` (`old` and `new`,
  each `{ "path" }` or `{ "text" }`, `name`, `language`), each with an
  optional `title`. The `image` fields go away.
- `asqr ask` makes every path absolute, as for images today.
  `validate` and `ask` warn about an entry above 1 MiB and, as today,
  about images asqr cannot show.
- A file with a lower version is refused with a message that this asqr
  no longer supports format version N and that `asqr skill` describes
  the current format. Version 1 sessions waiting in a queue get that
  error result (status `error`, spec section 3.8), so the waiting agent
  sees exit code 11 and the hint.
- Result files move to version 2 as well; the session note from its own
  todo is the first new field there.
- The TUI shows image entries where images show today. Text and diff
  entries show a placeholder until their viewers land (steps 4 and 5).
- New schemas in `schema/` (`asqr schema` prints them), `asqr new`'s
  skeleton, the example sessions in `examples/sessions/` (tests include
  them), the spec, the skill, and an ADR that supersedes ADR 0007 and
  ADR 0025's format part.
- `schema/session.v1.json` and `schema/result.v1.json` are deleted; git
  history keeps them (user, 2026-10-14).

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
- Relates to: [Let the person add a note to the whole session on the review tab](01M4XGJK7305W8JG58WBP4SC98-let-the-person-add-a-note-to-the-whole-session-on-the-review-tab.md), its `note` is the first new result field
