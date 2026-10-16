---
title: "Show the title of the session a follow-up continues"
kind: improvement
component: tui
origin: request
tags: [ux, field-test]
---
# Show the title of the session a follow-up continues

Found on 2026-10-16: Jakob answered a follow-up session and could not
tell which session it continued. The header shows
`follows: 01M51VZTB20Y36XSFZ43GMZMZF`, as spec section 3 asks
("The header shows `follows: <id>`"), and an id says nothing to a
person.

## Proposal

asqr keeps an answered session in the queue's `archive/`, title
included (checked 2026-10-16 for the session above). The header shows
`follows "Test 1/7 · Fragearten und Markdown"` when the archive holds
the session, a waiting session's title when it is still in the inbox,
and the id otherwise. Change the spec's sentence with it.
