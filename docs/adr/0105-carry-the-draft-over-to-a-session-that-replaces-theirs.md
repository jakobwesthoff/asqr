# 105. Carry the draft over to a session that replaces theirs

Date: 2026-10-31

## Status

Accepted

Amends [64. Let the asker withdraw a session](0064-let-the-asker-withdraw-a-session.md):
the `draft` of a withdrawn result is no longer meant as presets for a
replacement session; `ask --replaces` carries the draft over itself

Builds on [77. Count a cleared default as the person having started](0077-count-a-cleared-default-as-the-person-having-started.md):
a draft marks a question the person cleared, and the carry keeps the
mark

Builds on [76. Mark the default option](0076-mark-the-default-option.md):
a carried `cleared` question shows its default marked and not picked

## Context

- `ask --replaces <old>` withdraws a session the person had started on.
  What they entered went into the `draft` of the withdrawn result, and
  the replacement started empty.
- The format presets only options (`"default": true`). The skill told an
  asker to set `default` on the options the person had picked and to
  quote typed text and notes in the replacement's question text, so the
  person could paste or retype them. Typed text costs the most to redo.
- What the person has entered must not get lost when an agent reworks a
  session, and an agent must not be able to write arbitrary text into
  the person's fields, since that defeats their purpose.
- The file of a replacement does not record that it replaces another
  session, as `--replaces` is a flag of `ask`. `ask` has both sessions
  at hand.

## Decision

- **asqr carries the draft.** On `ask <new> --replaces <old>`, the
  draft of `<old>` becomes the draft of `<new>`, written under the
  drafts lock that `--replaces` holds for the withdrawal and the drop.
  The asker writes no text and sets no flag. Carrying only on a flag
  such as `--keep-draft` was rejected, since an asker can forget it, and
  so were presets the asker writes together with a result flag for
  presets sent unchanged.
- **Matched by id.** The answers are matched by question id and option
  id under the rules of a restore (spec section 4.3). A question kept
  under its id keeps the person's input; a question under a new id
  starts fresh, so an asker gives a question whose meaning changed a new
  id.
- **What does not fit is dropped.** An answer whose picked option or
  own-text field the new file has no place for is dropped, and the
  question takes the defaults of the new file. A restore alone would
  keep such a question touched and `cleared`, which suppresses the new
  default. A `cleared` mark the person made stays. Where nothing fits,
  no draft is written.
- **The person's input.** Carried entries count as entered
  (`person_entered`): a further `--replaces` of the replacement needs
  `--force`, and `ask` of its id is refused, also with `--force`, as for
  any started session. `ask` with the id of a waiting session nobody
  has started needs nothing, as it has no draft to carry.
- **What rides along.** The session note, the reject reason and the
  current question are carried with the answers. A reject reason alone
  is not carried, as it does not count as an answer.
- **Source.** For a session withdrawn now, the draft file, read before
  the withdrawal deletes it. For a session withdrawn before, the `draft`
  of its result, which holds less: no `cleared` mark, no own text that
  was unpicked, no selection below `min` without a note, no own text
  next to a pick.
- **Where it applies.** Only for a new session that does not wait
  already: a running TUI rebuilds a waiting session from its own state,
  not from the draft file. Not where `drafts/<new>.json` holds an
  answer, which stays as it is, unless it names the old session in
  `carried_from`: a rerun after a crash keeps it. A draft that cannot be
  written costs the carry only. A drop that fails after the draft was
  written removes it.
- **Replaced once.** Where a draft that continues the old session
  exists, a later `--replaces` of the same old id carries nothing from
  the old result's `draft`, which may be stale by then.
- **The marker.** A draft that continues another has `carried_from`
  with the `id` and the `title` of the earlier session. Only drafts
  carry it, like `custom_unpicked` and `cleared`, and the result schema
  leaves it out. The TUI keeps it in every draft it saves and shows
  `continues your draft of "<title>"` in the session header, the id in
  place of the title where the earlier session had none (spec section
  7.2).
- **Notice for the old session.** A session the person had open, whose
  draft was carried over, leaves with the information notice `↩ "<old>"
  was replaced by "<new>"; your answers carried over` in place of the
  withdrawal warning (spec section 7.9). It names a session that arrived
  in the same scan of the inbox. It stays the warning where the session
  held changes no saved draft had. A scan that sees the old session gone
  while the drafts lock is held, with no new session arrived, leaves the
  notice for the next scan.
- **Hints.** The hint after `ask --replaces` says that asqr carried the
  draft over, that the `draft` of the old result still holds their answers and notes, and
  what it means for the replacement; where nothing was carried it says
  so. The hint for a withdrawn result with a `draft` tells the asker to
  ignore the `draft` and that `ask --replaces` carries what fits over.
- **The skill.** `withdrawing.md` no longer advises `default` on earlier
  picks or quoting typed text. It says that asqr carries the draft over
  and that the asker keeps the ids of the questions and options whose
  earlier answers should stay.

## Consequences

- asqr 0.15.0 and earlier ignore `carried_from`: a draft written by this
  version restores there as an ordinary draft, without the header note.
- A crash between the write of the draft and the drop of the session
  leaves a draft without a session, as a session taken back by hand
  does (spec section 5.4).
- An asker that withdraws a session by hand and asks the replacement
  without `--replaces` gets no carry-over.
