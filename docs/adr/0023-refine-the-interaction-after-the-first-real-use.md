# 23. Refine the interaction after the first real use

Date: 2026-09-28

## Status

Accepted

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

Amends [7. Session and result format version 1](0007-session-and-result-format-version-1.md)

Amends [6. Integrate agents through a bundled skill first](0006-integrate-agents-through-a-bundled-skill-first.md)

## Context

The user answered six test sessions in a real terminal and left their
feedback in the answers and notes. Four points needed a decision, taken
with the question tool; the user's wording is in the logbook.

- In a text field `←`/`→` move the text cursor, so the person could not
  reach other questions without first pressing `esc`, which they did not
  find: "i am always hanging in the test field of this text input".
- A required question they could not answer left only a note, which does
  not satisfy `required`: "maybe everything always should be optional".
- A `multi` with `max: 3` and an own answer took three options plus the
  own text, which surprised them: "it should be documented then".
- On the review tab the cursor started on the first question, although
  submitting is what usually follows.

Separately, the user asked that the agent skill only applies when they
explicitly ask the agent to use asqr.

## Decision

- **Field edges:** `←` with the cursor at the start of a field's text
  goes to the previous question, `→` at the end to the next one. Inside
  the text the arrows keep moving the cursor, and `esc` still leaves the
  field. The help lists it; the key bar inside a field stays as it is,
  since it is full at 60 columns.
- **No required questions:** every question is optional. `required`
  leaves the format; a file that still sets it parses as before, the
  field is ignored and `validate` and `ask` report it as unknown. Submit
  is never blocked, and skipped questions come back as `skipped: true`
  with their notes.
- **Own answer on a multi:** `min` and `max` count the picked options
  only, and the own answer comes on top; the spec and the skill say so.
  The hint first said it too ("pick 2 to 3, and type your own if you
  like"). In the retest the user found that too wordy: "the user will
  find out that they can always add their own answer themselves". The
  hint now reads "pick between 2 and 3 items" and leaves the own answer
  out.
- **Review:** the review tab opens with the cursor on Submit.
- **Skill:** its description and its "When to use it" section say that
  it is used only when the user explicitly asks for asqr; an agent may
  mention asqr but never switches to it on its own.

The format version stays 1: dropping an optional field that readers
ignore when unknown changes no file that was valid before.

## Consequences

- A person can move through every question with the arrows alone, text
  fields included.
- An asker can no longer force an answer; it has to handle skipped
  questions, which it had to anyway for everything not marked required.
- `enter` on the last question followed by `enter` on the review
  submits. That is the fast path the user asked for, and a submit can
  no longer be refused.
- An image vanishing in narrow terminals, also found in the test, is not
  decided here; it is tracked separately for a later investigation.
