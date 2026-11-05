---
title: "Show several context items per question in the context pane"
kind: feature
component: tui
status: needs-discussion
origin: discussion
tags: [ux]
---
# Show several context items per question in the context pane

Phase 2 of the context pane, split off on 2026-10-14 while discussing
phase 1. A question or option carries several context items, for
example the diffs of three files, or a diff and a screenshot. The pane
shows one item at a time with a tab strip at its top, and the person
switches between items quickly (user, round 1).

## Proposal

- Format: phase 1 defines `context` in format version 2 as a list
  limited to one entry (user, 2026-10-14). Phase 2 lifts the limit; no
  format change.
- The tab strip names each item by its title, or by its file name when
  it has none, and scrolls like the question tab bar when it does not
  fit.
- Each item keeps its own scroll position while the person switches.

## Open questions

- Keys for switching items, usable on non-US layouts (no `[`/`]`)
  and from the question pane as well as the focused pane. Candidates:
  `tab` cycling through the question and every item; digits while the
  pane has the focus; `ctrl-n`/`ctrl-p`. In a text field
  `ctrl-n`/`ctrl-p` move the cursor by a row (`src/tui/text_field.rs:211-212`,
  spec 7.7), so they work on the question rows and in the pane, not in a
  field. Digits pick options on the question rows (`src/tui/session.rs`,
  `command`), so they are free only while the pane has the focus.
- How an option's items combine with the question's while the cursor is
  on that option: instead of them (as option images do today) or in
  front of them.
- A limit on items per question, if any.

## Where the pane leaves it

The context pane (ADR 31) shows one entry under a header rule with the
entry's name set into it (ADR 54), which is where a tab strip can go;
the scroll position sits in a bottom rule. `SessionState` keeps a scroll position per
`EntryKey` (question and option index); an item index in that key gives
each item its own position. `Viewers` keeps one viewer per shown entry
under the same key plus the session id.

## Questions put to Jakob on 2026-10-30, kept open

Jakob chose "later, keep it open" for all four. Options, the lead's
recommendation first:

- Switch key: `ctrl-n`/`ctrl-p` (free on the question rows and in the
  pane, same place on US, German and French layouts); digits while the
  pane has the focus; `,` and `.`.
- Where the strip sits: in the pane's header rule in place of the
  entry's name, so no row is lost; a row of its own below the header.
- Items per question: up to five, refused above; no limit; warn above
  five.
- An option's items: replace the question's while the cursor is on the
  option, as today; come in front of the question's.

On 2026-10-31 Jakob kept the todo parked again (in the chat). The lead's
check that day: the work touches the same pane code as the caption
full-screen key (todo 01M620FQQ4ZP4SDK3FYDPHPJNT) and images partly
scrolled off (todo 01M621K2HGAK8EDMX4T1RVXPAY), and `,` now opens the
settings pane (ADR 92), so of the switch keys above only `ctrl-n`/`ctrl-p`,
the digits and `.` remain free.

Not yet asked: whether switching stops or wraps at the first and last
item, which item shows when the cursor returns to a row (the last shown
or the first), and how an item without a title is named in the strip.
The limit `MAX_CONTEXT_ENTRIES = 1` sits in `src/format/validate.rs:26`,
and `skills/asqr/SKILL.md` tells agents the same.

## A case from the antiq mascot work, 2026-11-03

An agent wanted to show the layer views of two earlier emblems (Talqi
and Snappy) in one info item as references, to explain how a new
emblem's layers should be built. asqr refused the session with
"questions[2].context: 2 entries, but this asqr shows one per question
or option" (exit code 11), so the agent split them into two info items
with the same purpose. Jakob asked for this case to be added here
(chat, 2026-11-03). In this case the two images were references to
compare against each other, so the agent would have preferred to see
them next to each other rather than one at a time.

Jakob's thought on this case (chat, 2026-11-03): maybe the answer is not
this feature but a passage in the skill that explains how to spread
such material sensibly over several questions. Today the skill says
`context` is "a list with one entry", that an option's own `context`
lets the person flip through variants, and that more content goes into
short info items, one topic each. It does not say what to do with
several images on one topic. A passage could say:

- Images that belong together but are read one by one: one info item
  each, placed one after the other, each `header` naming its image.
- Images the person picks among: one question, one image in each
  option's `context`.
- Images the person compares: one composite image with them side by
  side (for example `magick a.png b.png +append out.png`), as the
  single entry.

Settling this todo includes deciding between that passage and the
feature, or both.

## A case from the antiq page review, 2026-11-05

An agent asked one question, "Does the page fit?", about two images that
belonged to the same result: a screenshot of the new project page and
the page's link preview image (`og.png`). asqr refused the session with
"questions[0].context: 2 entries, but this asqr shows one per question
or option". The agent then hung the screenshot on the option "Fits" and
the preview image on the option "Change". That is misleading: each image
looks as if it belonged to its option, and the person only sees the
second image while the cursor rests on "Change".

Jakob's position (chat, 2026-11-05):

- He does not want several context entries that the person switches
  through in the pane. It is intuitive for the agent, not for the
  person: one more context switch, and the person has to notice that
  there are several entries and flip between them.
- His current tendency: material that seems to belong together is often
  two answers that can be split. Then it is two questions, each with
  "fits" and "does not fit" and one image attached, since one image can
  be right and the other wrong, and each may need its own note.
- If a strict connection exists that the session must show, a different
  solution is needed; he does not know which yet.

To discuss in detail: a solution the person can see at a glance. It may
be stricter instructions in the skill for agents building a session
(split into one question per image unless the images must be judged
together, and then a composite image as the one entry), rather than the
feature above. This case leans towards settling the todo with the skill
passage instead of a switcher.
