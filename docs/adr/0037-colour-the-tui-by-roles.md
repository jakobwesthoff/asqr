# 37. Colour the TUI by roles

Date: 2026-10-15

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md)

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [38. Give notices kinds and confirm a finished session](0038-give-notices-kinds-and-confirm-a-finished-session.md)

Amended by [47. Put the session's title in the title bar](0047-put-the-sessions-title-in-the-title-bar.md)

## Context

The TUI marked nearly everything the same way, with reversed video: the
title line, the current tab, the row under the cursor, the focused context
pane's header, the submit button and every key in the key bar. Up to nine
equally loud blocks stood on one screen. Colour appeared only for states:
the length counter, diff lines, notices and highlighted code.

The section "Terminal interface" of the design guide gives every role a
terminal colour. The user compared six prototyped colour variants in four
terminal themes on 2026-10-15 and chose "Teal + Amber focus": cyan as the
primary colour, a yellow focus fill.

## Decision

Decided with the user on 2026-10-15.

The roles and their terminal colours, all named ANSI colours, so the
terminal's theme decides how they look:

| Role | Colour | Where |
|---|---|---|
| Primary | cyan | `asqr` in the title line, the current tab and the rule under it, chosen marks of options, the keys in the key bar and the help, overlay borders, the submit button |
| Focus | yellow, as a fill | the row under the cursor, a field's head under the cursor, the current entry of the session list, the focused context pane's header |
| Warning | yellow | a notice, a length counter above its target |
| Success | green | a length counter within its target, an added diff line |
| Error | red | a length counter above `warn`, a removed diff line |
| Secondary | dim | hints, the queue in the title line, unanswered `☐` marks, option numbers, the descriptions in the key bar, the words of the review's counts, overlay footers |

- **The focus fill** is yellow text with reversed video, not a yellow
  background. Where colours are off it stays a plain reversed fill; a
  background fill would vanish there, and with the themes' own black on
  yellow it reads no better (2.4:1 in Catppuccin Latte).
- **One colour under the fill:** a filled row takes the fill's colour in
  every part, and dim is lifted, so a chosen mark or a dim number under
  the cursor shows yellow on the fill like the rest of its row.
- **One focus fill at a time:** while the context pane has the focus, its
  header has the fill, and the question's current row keeps its pointer
  and bold text but loses its fill. ADR 31 had the header highlighted the
  way the current tab was; the current tab is now primary text, and the
  pane's header takes the focus fill.
- **The submit button** is the one fill that is not the focus: always
  `[ Submit ]` in a bold cyan fill, brackets included, so it stays apart
  from the focus fill where colours are off. When the cursor is on it,
  the pointer before it is bold yellow.
- **Title line and key bar have no fills.** The title shows `asqr` bold
  in the primary colour, the queue dim and the count in the text colour.
  Each key in the key bar is bold in the primary colour, its description
  dim.
- **The tab bar** draws the current tab bold in the primary colour, its
  mark included, and under the bar a rule that is heavy (`━`) and primary
  under the current tab and thin (`─`) and dim elsewhere. An answered
  `☒` keeps the text colour, as an answer is no status; an unanswered `☐`
  is dim, in the tab bar and in the review's list.
- **Overlays** (the help, the whole intro, the session list) share one
  frame: rounded corners in the primary colour, a bold title, a dim
  footer and a blank column inside each side. The title and the footer
  stand on the border and take its colour. The help's keys are bold in
  the primary colour. The rows of the session list move one column in,
  and its current entry takes the focus fill.
- **Bold stays on coloured text.**
- **Bold belongs to the roles.** Drawing code takes bold from a role as
  it takes a colour. Bold in the primary colour are the app's name
  (`asqr`), the keys, the current tab and the chosen marks; bold in the
  text colour are titles (the session's name in the header, the
  context entry's name in the pane header, an overlay's title) and
  strong text (an option's label, the head of a question's note, the
  numbers of the review's counts); bold in the focus colour is the
  pointer before the submit button. Each of these is a role of its own.
- Code and Markdown keep the colours their highlighting gives them,
  inline code included, which stays yellow next to the yellow fill.

## Consequences

- The reversed yellow fill has low contrast on light themes: 2.3:1 in
  Catppuccin Latte and 3.0:1 in Solarized Light. Cyan text on a light
  background reaches 3.3:1 in Catppuccin Latte and 2.9:1 in Solarized
  Light.
- The terminal gets cyan as indexed colour 6 and yellow as 3. A terminal
  that draws bold colours as their bright variants shows bold cyan and
  the bold text under the fill in colours 14 and 11, which Solarized
  makes greys (`#93a1a1`, `#657b83`).
- Focus and warning share yellow. Whether that reads clearly where a
  refused key's notice shows next to a focused row was left open.
- The own answer's mark stays in the text colour when it is chosen: it is
  part of the field's head, which has one style. The todo
  `01M50JSEM9KMM2QYWBQNZ61DX2-colour-the-chosen-mark-of-the-own-answer-like-the-option-marks.md`
  holds what colouring it would take.
- The rule under the tab bar and the rounded overlay corners change the
  text of the screen, not only its colours.

## Amendment (2026-10-16, ADR 47)

With a session shown, the title line holds the session's title, bold in
the text colour, after `asqr`, and the queue and the count stand
secondary at its right end. The user chose the dim count on 2026-10-16,
over the text colour the bullet "Title line and key bar have no fills"
gives it; on the empty queue the count keeps the text colour. The
`from` and `follows` line under the title is secondary. The intro keeps
the text colour.

## Amendment (2026-10-16, the count on the empty queue)

Jakob decided on 2026-10-16 that the count is dim on the empty queue
too, so the queue and the count are secondary in every title bar. This
replaces "the count in the text colour" in the bullet "Title line and
key bar have no fills" and "on the empty queue the count keeps the text
colour" in the amendment above.
