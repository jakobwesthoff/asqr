---
title: "Make the skill keep an agent from leaving the result of an info-only session unread"
kind: improvement
component: cli
origin: incident
---
# Make the skill keep an agent from leaving the result of an info-only session unread

On 2026-10-29 an agent (Claude Code, Opus) asked a session of only
`info` items, never read its result, and so missed a request Jakob had
written into a note on it. Jakob found out about 45 minutes later, when
the agent did something he had not asked for. He asked for this todo.
**The causes and the proposals below come from the agent that missed
the note.** They are its own account and guesses, not a verified
analysis. Analyse and discuss them before changing anything; the
outcome may also be that the skill is fine as it is.

## What happened

- 08:5x: the agent asked session `01M63AA9Y6CHCWWP4V4WP2Y0SB`, two
  `info` items reporting where Jakob's profile photo came from (an
  image and a Markdown trail as context). It started no `asqr wait` for
  it. Another session asked earlier in the same work
  (`01M639ZB81Q5T67B8WE8STGQ67`, real questions) did get a background
  `asqr wait`, and that result was read and acted on.
- 08:55:41: Jakob submitted the info session with a note on the first
  item: add the original photo and the avatar to the Photos album
  "Profile", or open the photo in Photos so he can do it; the avatar
  should go into his library as well.
- Jakob then asked in the chat whether the agent had seen his question
  about the avatar images. The agent did not look at the result. It
  answered a different question (which image it had uploaded to a
  website). When Jakob said he had meant adding to his photo library,
  the agent guessed at that from the chat alone and imported only the
  avatar, without the album and without the original photo.
- Jakob suspected that asqr drops notes on info items. `asqr result`
  showed the note intact: asqr worked, the agent had never read it.

## Causes, as the agent sees them

1. **It never read the part of the skill that says so.** `asqr skill`
   prints 567 lines. The agent read it in two cuts, `head -300` and
   lines 300 to 420, and stopped at the exit codes. The result format
   (an info item comes back only with a note, around line 442) and
   "Read every note, on info items too." (line 564, the end of
   *Patterns for common tasks*) were never in its context.
2. **It treated an info-only session as a one-way report.** It read
   "A session may hold only info items, as a report the person can
   answer with notes" (line 94) but took "report" as the operative
   word: nothing to decide, so nothing to wait for. Waiting looked like
   something tied to questions.
3. **Nothing at `ask` time reminded it.** `asqr ask` printed only the
   id. The "Asking and waiting" section shows `ask` followed by `wait`
   as the pattern, but does not say that every session, info-only ones
   included, needs its result read.
4. **When asked in the chat, it answered from memory** instead of
   checking `asqr result` or `asqr status --since`, which would have
   shown the submitted session.

## Proposals, from the same agent

What it believes would have made it read and act on the note:

- In "Asking and waiting", right after the `ask`/`wait` block, a
  sentence in the imperative: every session you ask gets a result, an
  info-only one included, since the person can answer any item with a
  note; start `asqr wait` for each session (in the background where
  possible) and read the result before you consider the matter done.
  The agent read this section; a rule placed there would have reached
  it.
- At the `info` kind description (line 94), say the same in one
  sentence: an info-only session still returns a result with the
  person's notes, which you must read.
- Move "Read every note, on info items too." from the end of the
  patterns into the result section, and repeat it where the rule above
  sits; at line 564 it sits behind content agents may skip.
- Tell agents near the top that the skill is long and must be read in
  full, or reorder it so the rules an agent cannot work without (ask,
  wait, read every result and note) come before the reference
  material.
- Possibly, outside the skill: `asqr ask` could print a hint on stderr,
  such as "wait for the result: asqr wait <id>", for every session or
  at least for info-only ones. This is a CLI change and needs its own
  decision.
- When the person asks in the chat about something they may have
  answered through asqr, check `asqr status --since` and the result
  first.

## Jakob's direction

Added by Jakob on 2026-10-29, after reading the agent's account:

- The skill probably has to get shorter and more compact overall, so
  that an agent reads all of it every time.
- An instruction at the very top that the skill must be read in full,
  and that an agent which got only the beginning, or a cut-off output,
  reads it again completely, since every part matters.
- Possibly move parts into sub-skills or reference sections that an
  agent reads only when it needs that information (for example the
  context entry types, withdrawing, finding sessions, the patterns).
  To be thought through.

## Open questions

- Which of these address the real cause, and which only add text? A
  longer skill makes cause 1 worse.
- Is a stderr hint from `asqr ask` welcome, or noise for agents that
  already wait?
- How much can the skill shrink, and what moves into sub-skills or
  references without an agent missing a rule it needs on every session?
- Should the skill say anything about reading itself in full, or is
  that the agent's own responsibility?

## Jakob's direction (2026-10-30, asqr session 01M6628SFCCK00ENY6YV0XQZR1)

- Jakob is fairly sure one cause was that the skill is so long that the
  agent decided not to read all of it.
- Two ways, probably both worth doing: split the skill into several
  parts, with whatever the skill call needs so the agent may read the
  other files; and compress the whole skill without losing content.
- A plan comes first, worked out by an agent in an adversarial review
  style, but with fable, opus, sonnet and haiku discussing solutions
  with each other from the start, to find the best one. The lead watches
  it and puts the result to Jakob in an asqr session.

## Decisions (2026-10-30, asqr session 01M663RDEWA9W6SVK9KWFMC1ZV)

Jakob accepted the plan worked out by fable, opus, sonnet and haiku
(kept in the conductor logbook of 2026-10-30,
`~/playground/2026_10_30_asqr-todos-worked-off-by-agents/plans/skill-structure-plan.md`):

- A core `SKILL.md` of at most 16,000 bytes with every rule no asqr check
  catches, plus three reference parts (`context`, `withdrawing`,
  `patterns`) that an agent reads with `asqr skill <part>`. The core
  opens with "read this in full" and ends with `End of the asqr skill.`
- New rule, landing first and on its own: every session, an info-only
  one too, comes back as a result; wait for it and read all of it, every
  note included.
- Duplicates go; an inventory of every rule, ticked by a reviewer who did
  not edit, proves none is lost.
- The skill carries the asqr version in its header (frontmatter
  `metadata`), checked by a test against the crate version. It travels
  with every copy; each release edits it once.
- Tests check the structure plus about nine key rules by a short phrase
  each, so a later rewrite cannot drop one silently.
- `asqr ask` prints a reminder to wait for the result after every
  session it accepts, not only info-only ones.
