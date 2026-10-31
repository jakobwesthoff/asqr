---
title: "The context pane"
tags: [guide/answer]
site:
  order: 3
---
An asker can attach material to a question: a diff, a file, a log, an
image. asqr shows it in a pane of its own next to the question. This page
says where the pane stands and how you move into it. The next page, 
[Looking at an entry](01M68BT9BDTGM96WSRXRSYFZKY-looking-at-an-entry.md),
covers what the entries look like.

## What the pane shows

A question can carry context, and so can each of its options. The option
under the cursor shows its own context, so you can browse the options
before you pick one. Every other row, and an option without context of
its own, shows the question's context. A question without any context
shows no pane.

The space for the pane stays while the cursor moves, also when the current
row shows nothing, so the layout does not jump.

![A question with its options and a diff beside it](../pictures/session-with-context.png)

The pane's first line is a rule with the entry's name in bold: its title,
or the file name of its path, or its kind when it has neither. The last
line is a rule too. When the entry is taller than the pane it says which
lines show, such as `1–20 of 53`, and it names `tab` when the entry is
taller or wider than the pane.

## Beside or below

asqr puts the pane beside the question on a wide terminal and below it on
a tall one. With the default setting, `auto`, the pane goes beside where
the area is landscape, which is where its width divided by twice its
height is at least 1, and below where it is portrait. A terminal of
120 by 40 or 230 by 60 gets the pane beside, and one of 110 by 70 or
90 by 50 gets it below. The pane fits beside from 103 columns and below
from 15 rows.

![The context pane below the question](../pictures/context-below.png)

You can fix the place. `--layout beside` or `--layout below` on the
command line, or the variable `ASQR_LAYOUT`, or the `Context` setting in
`settings.toml` does it. A flag wins over the variable and the variable
over the file. asqr uses the other place when only that one fits.

When neither place fits, the pane shrinks to one line at the bottom. It
names the entry and says `z: full screen`.

## Moving into the pane

`tab` or `shift-tab` moves the focus into the pane, also from a field. A
focused pane takes every key. The arrow keys, or `j` and `k`, scroll it
by a line, and the pane's name shows the focus fill while its rules turn
yellow. `tab`, `shift-tab`, `esc` or `q` give the focus back to where it
was.

![The context pane with the focus after tab: its rules and name turn yellow](../pictures/pane-focused.png)

The keys that scroll the pane without moving into it are on the page about
the [less obvious keys](01M68BT9BTS0J2VR1Y5N4NMEVC-less-obvious-keys.md).
