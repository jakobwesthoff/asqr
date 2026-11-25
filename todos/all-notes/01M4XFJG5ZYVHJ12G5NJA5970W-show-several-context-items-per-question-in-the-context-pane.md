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
phase 1. A question carries several context items that only add
context to the question as a whole, such as versions of the same thing:
a light and a dark variant, the old and the new state. The pane shows
one item at a time and the person switches between them. Jakob decided
on 2026-11-25 to build it, with two conditions. The person has to see at
once what is there and get around it without the question becoming
harder to use. The skill has to keep agents from putting material into
it that belongs to one option or needs a question of its own. The
design of the pane is still to be worked out with Jakob, so the todo
stays open for discussion.

## Decisions

- 2026-11-25, user: build the feature together with a skill passage
  that limits its use. This settles the choice between "the feature or
  a skill passage" that the cases below left open. In Jakob's words
  (chat): "wir müssen da bei der implementierung wirklich hirnschmalz
  investieren. wir brauchen sinnvolle übersichtlichkeit und sofortigen
  überblick für den nutzer, wir brauchen sinnvolle navigation zwischen
  den optionen ohne es für den nutzer unübersichtlicher oder
  komplizierter zu machen."
- 2026-11-25, user: several items on one question are only for
  material that gives the question more context, such as different
  versions (light/dark, old/new). Material that belongs to one specific
  option goes into that option's `context`.
- 2026-11-25, user: write the skill passage with great care, and
  tighten it when agents misuse the feature during their work.

## What the design has to meet

- The person sees at a glance that a question has several items, how
  many, and which one is showing.
- Switching between items is quick, and the question is not harder to
  read or to answer than one with a single item.
- A question with one item looks and works as it does today.

## Skill passage

`skills/asqr/SKILL.md` tells agents today that `context` is "a list
with one entry, since asqr shows one per question or option", and that
an option's own `context` lets the person flip through variants and
pick one. A first draft by the lead (2026-11-25), not decided, for
where material goes:

- Versions of the same thing that the person looks at together for one
  answer (light and dark, old and new, before and after): several items
  on the question, each `title` naming its version.
- Material that belongs to one option: that option's `context`.
- Material the person judges on its own, which may be right while the
  rest is wrong or need its own note: a question of its own (Jakob's
  position of 2026-11-05, below).
- A change to a text: one `diff` entry, not two items for the old and
  the new text.

A way to see misuse, proposed by the lead and not decided: after the
release, read finished sessions that use several items with
`asqr history` and `asqr replay` (ADR 101, ADR 103), check each use
against the passage, and tighten the passage where agents strayed.

## Proposal

- Format: phase 1 defines `context` in format version 2 as a list
  limited to one entry (user, 2026-10-14). Phase 2 lifts the limit; no
  format change. The limit is `MAX_CONTEXT_ENTRIES` in
  `src/format/validate.rs`, which the schema derives `maxItems: 1` from
  (`src/format/session.rs`); spec section on validation errors ("more
  than one context entry on a question or an option") and ADR 28 state
  it too, so lifting it takes a new ADR that amends ADR 28.
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
- Whether options keep a limit of one item each, now that several items
  are meant for the question as a whole.

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

## A case from the Quillmoor mascot work, 2026-11-03

An agent wanted to show the layer views of two earlier emblems (Brindle
and Fennick) in one info item as references, to explain how a new
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

Jakob decided on 2026-11-25 to build both the feature and a skill
passage (see Decisions).

## A case from the Quillmoor page review, 2026-11-05

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

On 2026-11-25 Jakob decided for the feature after all, limited to
versions that give the question more context (see Decisions). His
concern from this case, that the person must notice there are several
items, is now a requirement of the design, and the split into one
question per image still holds for material judged on its own.

## Cases from a security release round at work, 2026-11-23 and 2026-11-24

The agent that ran the sessions of that round named three places where
it would have used several items. Jakob asked for them to be listed
here (chat, 2026-11-25); whether each one is a valid use under the
2026-11-25 rule is still to be decided.

- Closing four tickets (2026-11-24). One question asked whether to
  close four tickets with the closing comments the agent had drafted,
  and gave the four comments as four Markdown entries. asqr 0.19.0
  refused the session with "questions[1].context: 4 entries, but this
  asqr shows one per question or option" (exit code 11). The agent
  merged them into one Markdown file with a heading per ticket, which
  Jakob read and answered.
- A delivered change across several files (2026-11-24). An info item
  reported five commits to a backend image: a new start script, the
  Dockerfile line that runs it, an nginx and a php-fpm config change, a
  container stop test and two CI jobs. The agent attached only the new script as a `code`
  entry, as one entry per item allowed, and described the rest in the
  text. It would have attached the Dockerfile, config and CI changes as
  `diff` items next to the script. asqr did not refuse anything here;
  the agent never tried.
- Renders of a release against their references (2026-11-23). One info
  item per changed test case showed one comparison sheet, a composite
  image built with ImageMagick: the old render, the new render, an
  image of the differing pixels and up to three enlarged windows on the
  densest changes. With several items the agent would have attached
  the old and the new render on their own as well, so the person could
  see each at the pane's full size next to the sheet. asqr did not
  refuse anything here either.

## Cases from the Lanternfish UI rewrite, 2026-11-25

The lead of the rewrite's refactor round named three places in one
morning session (asqr 01M88ZFENNXC4FTJ1MHG5MH9XY) where it would have
used several items. Jakob asked for them to be added here (chat,
2026-11-25); whether each one is a valid use under the 2026-11-25 rule
is still to be decided.

- Before and after of a screen change. The questions "Welcome look" and
  "Update window" each asked whether to keep a change of a window's
  appearance. The lead put the before picture into the question's
  `context` and the after picture into the `context` of the option
  "Keep", and for "Welcome look" a second after picture (another page)
  into the option "Revert". So the after picture showed only while the
  cursor rested on "Keep", and the revert option carried a picture that
  did not belong to reverting, the misuse of the Quillmoor page review
  case above. Several items on the question (before, after, and the other
  pages, light and dark) would have shown them as versions of one
  change. asqr did not refuse anything; the lead never tried.
- Old and new app icons. The question "App icons" asked whether to keep
  sharper, recoloured app icons. The lead had two comparison images
  (the icons of two apps, each old left and new right, built beforehand
  by an agent) and hung the second on the option "Keep". Several items
  would have shown both icons as two versions of the same change.
- Ten changed reference pictures behind one decision. The welcome
  change touched ten reference pictures (five pages, light and dark).
  The session showed two of them; the rest stayed in a file the person
  would have had to open. With several items the question could have
  carried the pairs that matter, named by page and theme.
