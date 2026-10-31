---
title: "What agents can ask"
tags: [guide/agents]
site:
  order: 3
---
The skill suggests what a session can hold for a kind of task. These
patterns are a starting point: an agent adapts them, combines them or
leaves them aside. They never make a task a reason to use asqr without
your request. This page shows three of them as you would see them.

## Decide before implementing

One question for each open point. What the agent settled that bears on
several questions, and that you need to know, comes first in short info
items. The recommended option stands first and is the default. Each option
has a short trade-off as its description, and the current state or the
option's own diff is the context.

![A decision before implementing: each option shows its own diff](../pictures/pattern-decide.png)

Here the context changes as the cursor moves between the options, so you
see what each choice would do.

## Review findings

One question for each finding. Its text gives the code before and after in
two sentences, and the risk. The context is the fix as a diff with a name,
which highlights the unchanged lines. The options are such as post as a
review comment, fix, or drop.

![Review findings: the fix as a diff and three ways to answer](../pictures/pattern-review.png)

## Triage a backlog

One question for each item of bug reports, todos or updates. Its text says
in a sentence or two what is wrong or missing today and what would change,
for a person who has not read the item, then the agent's take and fix
idea. The strongest evidence, such as a log or a screenshot, goes into the
context. The options are such as pursue, defer or close, and the agent's
recommendation is the default.

![Triage of a backlog item with its evidence beside the question](../pictures/pattern-triage.png)

## More patterns

The skill describes more: comparing visual variants, picking candidates
inside a pipeline, walking through a debug, restructuring a document and a
rating grid. `asqr skill patterns` prints all of them.
