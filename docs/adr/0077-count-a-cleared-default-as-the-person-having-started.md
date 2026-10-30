# 77. Count a cleared default as the person having started

Date: 2026-10-30

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
a draft marks a question the person changed and left empty

Amends [71. Let backspace clear the pick of a single question](0071-let-backspace-clear-the-pick-of-a-single-question.md):
a cleared default counts as an answer in a draft, in place of the last
Consequences bullet of ADR 71

## Context

- A default cleared with `backspace` on a `single` (ADR 71), or toggled
  off on a `multi`, left an answer with no selection and no `defaulted`
  in the draft, the same as a question the person never touched. `asqr
  withdraw` and `asqr ask --replaces` without `--force` took such a
  session, and the person's choice went with it.
- A selection counts as an answer in a draft even where it does not
  answer the question yet, as it is work a replacement would throw away
  (spec section 4.3).
- A draft restored on start restored both kinds of empty answer as
  touched, so a default the file gained since stayed unpicked. A reload
  of a running session tells them apart and picks it.

## Decision

- A cleared default counts as the person having started. `asqr
  withdraw` and `asqr ask --replaces` refuse such a session without
  `--force`; with `--force` the session goes, and the TUI tells the
  person that the asker withdrew it. `asqr ask` of the same id refuses
  it, also with `--force` (spec section 5.5).
- A draft marks a question the person changed and left without a
  selection, typed text that is not blank or a note that is not blank
  with `"cleared": true`. Only drafts carry it, and the result schema
  leaves it out.
- Every question marked so counts as started, info items included: a
  cleared default, a pick cleared on a question without a default, own
  text or a note typed and erased again, and blank text. Each is work
  the person did on the question, and a replacement would throw it away.
  The mark cannot tell a cleared default from a question with nothing
  left to lose, as a draft does not know the session's defaults, and
  refusing is the safe side: the asker can still use `--force`, and the
  person is then told.
- `enter` on the own-answer row of a `single` without a pick and
  without unpicked text changes nothing, so it leaves the question
  untouched.
- A restored draft takes a question as untouched when its answer is an
  untouched default or holds nothing: no selection, no text, no note and
  no `cleared`. It then shows the defaults the file sets now, as a
  reload does, so a default the file gained since is picked.
- A pick or own text the file removed from a `single` or `multi` keeps
  the question touched and marked: the person moved off any default
  there, and the default coming back with `"defaulted": true` would tell
  the asker they left it. A `text` question or an info item has no
  options and so no default that could come back; there only its text
  or note keeps it touched.

## Consequences

- The `draft` of a withdrawn result carries no question marked
  `cleared`, as it is skipped without a note. A forced withdrawal of a
  session whose draft holds only such questions has no `draft`, and the
  TUI says the person's answers are dropped.
- asqr 0.14.0 and earlier ignore `cleared`: they neither count it nor
  read it on restore.
- A draft written by asqr 0.14.0 or earlier holds a cleared default as
  an answer with nothing in it, which restores as untouched, with the
  default selected again.
