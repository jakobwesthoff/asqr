---
title: "Mark an untouched default option so the person sees it"
kind: improvement
component: tui
origin: discussion
tags: [ux]
---
# Mark an untouched default option so the person sees it

An option with `default: true` is preselected and drawn exactly like an
option the person picked, `(•)`. The person cannot tell the agent's
preselection from their own choice. Agents use `default` for their
recommendation, so the person should see it as such. Raised by the user
on 2026-10-28 while deciding how `backspace` clears a pick.

## Decided

- One field, `default`: a default is the agent's recommendation and the
  value used when the person leaves it; no separate `recommended` field
  (user, 2026-10-28).
- The marker sits outside anything Markdown can produce, since option
  labels and descriptions are full Markdown. After three rounds of
  rendered mockups the user chose "M": an accent-coloured `◀ default`
  right-aligned on the first line of the default option, with a gap to
  the label, whose first line wraps earlier to leave room. On the row
  under the cursor the marker takes the focus fill's one colour like the
  rest of the row (user, 2026-10-28).
- The skill tells agents that `default` marks their recommendation.

## Open for the implementation

- Which colour role the marker takes (the design guide has no accent
  role yet).
- When the marker shows: only while the default is untouched, or as
  long as the default option is the one picked.
- What happens on narrow terminals where the gap and the marker would
  squeeze the label.
