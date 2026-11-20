---
title: "Own text typed beside a picked default is dropped without a word"
kind: improvement
component: tui
status: needs-discussion
origin: incident
tags: [ux]
---
# Own text typed beside a picked default is dropped without a word

On a `single` question with a default, a person who types their own
answer in the own-answer row and moves on with `→` or `←` keeps the
default as the answer. The typed text stays in the draft but never
reaches the result, and nothing on screen says so before the submit.
This is the documented design (ADR 8 rejects "typing that selects the
own answer on single-choice questions"; spec section 4.2: "the option
counts while it is chosen"), and it is the one path found that matches
the incident below.

## Incident

On 2026-11-02 Jakob answered the session "Where does the Dachser backup
go in Google Drive?" (`asqr replay 7T0YVJVV`, id
`01M6E5CSZ5Z0J69GHZSM3K8M1E`). He remembers writing that the backup
should go elsewhere than the preselected drive. The first question
("drive", a `single` with the own answer "Another place" and the default
`kunden`) came back as `selected: ["kunden"]` with no `note`, no
`custom` and no `defaulted`; the agent acted on the default.

Checked on 2026-11-20 against the code and the archived result:

- A note on that question would have reached the result in every way of
  moving on: tests in `src/tui/session.rs` ("A note beside an untouched
  default") cover moving on with `l`, going to the review, picking the
  default with `enter` after the note, and a draft round trip.
- The missing `defaulted` says the question was edited. All three
  questions of the session had defaults, and none came back with
  `defaulted`. On "folder" there is no note or text either, so `enter`
  on the default is how Jakob moved through it, and the same may hold
  for "drive".
- Own text in "Another place" with the default left picked, then `→` at
  the end of the text (or `esc` and `l`), gives exactly the stored
  answer: the default, no `custom`, no `defaulted`. The test
  `a_chosen_option_wins_over_typed_text_until_the_own_answer_is_picked`
  pins that behaviour.

Which of the two happened (`enter` on the default with nothing typed,
or own text that was dropped) cannot be told from the result.

## Options

- Leave it as designed and close this todo.
- Let typing into the own answer deselect a default the person has not
  touched, while a pick the person made stays (only an untouched default
  gives way). ADR 8 rejected this for picks in general.
- Keep the design but say it: a warning when the person leaves the
  question with own text that does not count, or a mark on the review
  tab next to the question.
