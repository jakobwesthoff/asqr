---
title: "Select the own answer when the person types in it on a single question"
kind: improvement
component: tui
origin: incident
tags: [ux]
---
# Select the own answer when the person types in it on a single question

On a `single` question with a default, a person who types their own
answer in the own-answer row and moves on with `→` or `←` keeps the
default as the answer. The typed text stays in the draft but never
reaches the result, and nothing on screen says so before the submit.
Jakob decided on 2026-11-21 that this changes (see Decisions). It is today's documented design (ADR 8 rejects "typing that selects the
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

## Decisions

- 2026-11-21, Jakob (asqr session of the GPUI rewrite run, "asqr default (a)"): typing selects the own answer; another option can still be picked afterwards while the typed text stays.
- 2026-11-23, Jakob (asqr session of the GPUI rewrite run): typing always replaces the pick, also an option the person picked themselves, not only an untouched default.

## Goal

On a `single` question, typing into the own-answer row makes the own answer the chosen one, whether the pick it replaces is an untouched default or an option the person chose, so the typed text reaches the result as `custom` and a default does not silently win. Picking another option afterwards is still possible and leaves the typed text in its field, as `backspace` on a cleared pick does today.

This reverses a design choice: ADR 8 lists "typing that selects the own answer on single-choice questions" among the rejected options, and spec section 4.2 says "the option counts while it is chosen". A new ADR records the decision and amends ADR 8, and the spec text follows.

## Done when

- A test per path: typing over an untouched default, typing over an option the person picked, picking another option after typing (typed text stays), moving on with `→`, `←`, `l` and `enter`, and a draft round trip.
- The test `a_chosen_option_wins_over_typed_text_until_the_own_answer_is_picked` in `src/tui/session.rs` is rewritten to the new behaviour.
- A new ADR amends ADR 8, `docs/spec.md` section 4.2 is updated, and the CHANGELOG has an entry.
