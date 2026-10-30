# 37. Colour the TUI by roles

Date: 2026-10-16

## Status

Accepted

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md):
the focused pane's header takes the focus fill

Amended by [38. Give notices kinds and confirm a finished session](0038-give-notices-kinds-and-confirm-a-finished-session.md)

Amended by [47. Put the session's title in the title bar](0047-put-the-sessions-title-in-the-title-bar.md)

Amended by [54. Set the context pane off from the question with a ledge](0054-set-the-context-pane-off-from-the-question-with-a-ledge.md):
the focused pane fills its name and turns its lines yellow

Amended by [60. Highlight a diff's unchanged lines and fill its changed ones](0060-highlight-a-diffs-unchanged-lines-and-fill-its-changed-ones.md):
changed diff lines are filled

Amended by [76. Mark the default option](0076-mark-the-default-option.md):
the marker of a default option is bold in the primary colour

Amended by [81. Name the chosen queue](0081-name-the-chosen-queue.md):
the name of a chosen queue on the empty queue is bold in the primary
colour

## Context

A TUI that marks nearly everything the same way, with reversed video,
puts up to nine equally loud blocks on one screen: the title line, the
current tab, the row under the cursor, the focused context pane's
header, the submit button and every key in the key bar.

The section "Terminal interface" of the design guide gives every role a
terminal colour. Cyan is the primary colour, and the focus is a yellow
fill.

## Decision

The roles and their terminal colours, all named ANSI colours, so the
terminal's theme decides how they look:

| Role | Colour | Where |
|---|---|---|
| Primary | cyan | `asqr` in the title line, the current tab and the rule under it, chosen marks of options and of the own answer, the keys in the key bar and the help, overlay borders, the submit button |
| Focus | yellow, as a fill | the row under the cursor, a field's head under the cursor while the field does not have the keys, the current entry of the session list, the focused context pane's header |
| Warning | yellow | a refused key or a session set aside (ADR 38), a length counter above its target |
| Success | green | a length counter within its target, an added diff line, a sent session (ADR 38) |
| Error | red | a length counter above `warn`, a removed diff line, a failure (ADR 38) |
| Secondary | dim | hints, the queue and the count in the title line, unanswered `☐` marks, option numbers, the descriptions in the key bar, the words of the review's counts, overlay footers |

- **The focus fill** is yellow text with reversed video, not a yellow
  background. Where colours are off it stays a plain reversed fill; a
  background fill would vanish there, and with the themes' own black on
  yellow it reads no better (2.4:1 in Catppuccin Latte).
- **One colour under the fill:** a filled row takes the fill's colour in
  every part, and dim is lifted, so a chosen mark or a dim number under
  the cursor shows yellow on the fill like the rest of its row.
- **One focus fill at a time:** while the context pane has the focus, its
  header has the fill, and the question's current row keeps its pointer
  and bold text but loses its fill.
- **A field's head** carries no focus fill while the field has the keys;
  the text cursor shows where typing goes. A field's head under the
  cursor is filled only while the field does not have the keys.
- **The own answer's chosen mark** is bold in the primary colour when it
  counts, like an option's, off the fill and while the field has the
  keys. Under the focus fill the mark takes the fill's colour like the
  rest of its row and keeps its bold, as an option's chosen mark does. A
  field's head is a list of spans, so the mark takes a style of its own.
- **The submit button** is the one fill that is not the focus: always
  `[ Submit ]` in a bold cyan fill, brackets included, so it stays apart
  from the focus fill where colours are off. When the cursor is on it,
  the pointer before it is bold yellow.
- **Title line and key bar have no fills.** The title line shows `asqr`
  bold in the primary colour, and beside a session the session's title
  bold in the text colour (ADR 47). The queue and the count are
  secondary in every title bar. Each key in the key bar is bold in the
  primary colour, its description dim.
- **The tab bar** draws the current tab bold in the primary colour, its
  mark included, and under the bar a rule that is heavy (`━`) and primary
  under the current tab and thin (`─`) and dim elsewhere. An answered
  `☒` keeps the text colour, as an answer is no status; an unanswered `☐`
  is dim, in the tab bar and in the review's list.
- **The header under the title:** the `from` and `follows` line is
  secondary, and the intro keeps the text colour (ADR 47).
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
  text colour are titles (the session's title, the context entry's name
  in the pane header, an overlay's title) and strong text (an option's
  label, the head of a question's note, the numbers of the review's
  counts); bold in the focus colour is the pointer before the submit
  button. Each of these is a role of its own.
- Code and Markdown keep the colours their highlighting gives them,
  inline code included, which stays yellow next to the yellow fill.

## Consequences

- The reversed yellow fill, the theme's background colour on yellow,
  has low contrast on light themes: 2.3:1 in
  Catppuccin Latte and 3.0:1 in Solarized Light. Cyan text on a light
  background reaches 3.3:1 in Catppuccin Latte and 2.9:1 in Solarized
  Light.
- The terminal gets cyan as indexed colour 6 and yellow as 3. A terminal
  that draws bold colours as their bright variants shows bold cyan and
  the bold text under the fill in colours 14 and 11, which Solarized
  makes greys (`#93a1a1`, `#657b83`).
- Focus and warning share yellow. Whether that reads clearly where a
  refused key's notice shows next to a focused row is open.
- The rule under the tab bar and the rounded overlay corners change the
  text of the screen, not only its colours.
