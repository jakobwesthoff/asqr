# An image per option

Status: feature request from the user on 2026-09-28; not designed yet.

## The request

The user's words:

> allow each possible answer to have a different image, so that one could
> iterate through them in one question and the image is always shown from
> the selected one

Where it came from: choosing between logo variants, the session needed
one question per image plus a separate question for the pick. With an
image per option, one question could show all variants: moving the
cursor through the options shows each option's image, and picking one
answers the question.

## Open points to decide

- Format: an `image` field on an option, next to the question's own
  `image`. Which one shows when the cursor is on an option without an
  image: the question's image, or nothing?
- "Selected" most likely means the option under the cursor, not only the
  picked one, so browsing works before deciding; confirm with the user.
- `multi` questions: the option under the cursor as well?
- `z` (full screen) and `o` (open) act on the image currently shown.
- The review tab: show the image of the picked option, or none?
- Format version: an optional new field that older readers ignore as
  unknown may stay in version 1 (the reasoning of ADR 23); needs an ADR,
  the spec (next version file, not `spec-1.0.0.md`), the schema and the
  skill.
