Part of the asqr skill. Read SKILL.md first, which holds the rules for every session; `asqr skill` prints it.

# asqr: patterns for common tasks

Once the user has asked you to use asqr, these patterns suggest what a
session can hold for a kind of task. They are a starting point. Adapt or
extend one for the task at hand, combine them, or leave them aside where
something else fits better. They never make a task a reason to use asqr
without that request.

- **Compare visual variants.** One question per thing to decide, such as
  each icon. Each variant is an option with a short label such as
  `Draft 3`, its image as `context` and a one-line `caption` on what
  sets it apart. Ask for a `single` pick, or a `multi` pick of keepers.
- **Decide before implementing.** One question per open point, with
  what you settled that bears on it in its `text`. Settled points that
  bear on several questions or on none, and that the person needs, go
  into short `info` items, one topic each, just before the questions
  they bear on, or first when they bear on none. The recommended option
  comes first and as the `default`, a short trade-off goes in each
  `description`, and the current state or each option's own `diff` in
  `context`.
- **Review findings.** One question per finding. Its `text` gives before
  and after in two sentences and the risk, its `context` the fix as a
  `diff` with a `name`, which highlights the unchanged lines. Options
  such as post as a review comment, fix, or drop.
- **Pick candidates inside a pipeline.** Ask for the candidates of one
  step as a `multi` pick, each option with its own `context` where the
  person needs to see the candidate, and `max` when the next step takes
  at most so many.
- **Debug walk-through.** Say what you found in a few lines, in the
  text of the question on the fix. Each candidate fix is an option of it
  whose `context` is its `diff`, so the log or trace goes into a short
  `info` item just before it, as that item's `context`, with a line on
  what to look for. Then one `multi` pick of the tests you would add.
  What you found that bears on neither question, such as a second bug
  you left alone, goes into a short info item before them.
- **Triage a backlog** of bug reports, todos or updates. One question
  per item. Its `text` says in a sentence or two what is wrong or
  missing today and what would change, for a person who has not read
  the item, then your take and fix idea. Its strongest evidence, such
  as a screenshot or a log, goes into `context`, the options are such as
  pursue, defer or close, and your recommendation is the `default`. A
  title, a size or a priority code alone says nothing to the person.
- **Restructure a document.** One question per section, with the
  section's own text as a `markdown` entry in its `context`, the places
  it could go as options with the reason in each `description`, and an
  own answer for somewhere else.
- **Rating grid.** One `single` question per candidate and aspect, such
  as length with too long, right and too short, tone or humour. Each has
  the candidate as `context` and a `header` such as `B tone`. Ask the
  next round, built from the ratings, with `follows`.

End of references/patterns.md.
