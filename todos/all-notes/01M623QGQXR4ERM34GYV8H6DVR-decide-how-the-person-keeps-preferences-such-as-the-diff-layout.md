---
title: "Decide how the person keeps preferences such as the diff layout"
kind: investigation
component: project
status: needs-discussion
horizon: someday
origin: discussion
tags: [ux]
---
# Decide how the person keeps preferences such as the diff layout

On 2026-10-28 the user decided that a diff entry shows unified by
default and that a key switches it to side by side. They were not sure
which layout they prefer, and want the person to be able to keep the
one they like as their default later. It is the first preference asqr
would have to remember, so it needs a way to store such settings.

## Open questions

- Where preferences live: a config file (path, format), environment
  variables like `ASQR_LAYOUT`, command-line flags, or a mix; how the
  existing `--layout` and `--no-title` fit in.
- Whether asqr remembers a choice made in the TUI (pressing the key)
  or the person sets it only in a file.
- Which other settings would join: the reading width (todo
  01M4Z8K98ZS0BR0FTJ4TDYSEQ7), whether asqr looks for updates on its own
  (todo 01M5J41K58B5CF23DN61PWM6MC, which the user sees as depending on
  this one, with a question on the first start), and others.
- What a settings interface in the TUI looks like and what it offers,
  for example a fixed set of choices per setting rather than free
  values (user, 2026-10-28).

## Revisit when

A second preference comes up, or the side-by-side diff has been used
long enough to know which default the person wants.
