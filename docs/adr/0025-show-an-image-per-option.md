# 25. Show an image per option

Date: 2026-09-28

## Status

Accepted

Amends [7. Session and result format version 1](0007-session-and-result-format-version-1.md)

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

## Context

Choosing between logo variants through asqr took one question per image
plus a separate question for the pick, since only a question could carry
an image. The user asked for an image per option: "so that one could
iterate through them in one question and the image is always shown from
the selected one".

## Decision

Taken with the user through the question tool:

- An option may have an `image`, with the same rules as the question's:
  an absolute path, made absolute by `asqr ask`, warned about by
  `validate` and `ask` when relative or missing.
- The option under the cursor shows its image, so the person can browse
  before picking; the same on `single` and `multi` questions. Every other
  row, and an option without an image, shows the question's image, or
  nothing when the question has none.
- The room for the image stays on the whole question once the question
  or any option has an image, so the layout does not jump while moving.
- `o` and `z` act on the image shown. The review shows no images.
- It belongs to version 1.0.0, which is not released yet. The format
  version stays 1: the field is optional, and a version 1 reader that
  does not know it ignores it and reports it as unknown.

## Consequences

- A set of variants fits into one question, and the pick is its answer.
- Every option image is loaded when its option is first under the
  cursor; with the Kitty protocol each one is sent to the terminal then.
