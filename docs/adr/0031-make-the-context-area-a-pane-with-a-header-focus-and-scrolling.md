# 31. Make the context area a pane with a header, focus and scrolling

Date: 2026-10-14

## Status

Accepted

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

Amends [28. Session and result format version 2 with a context field](0028-session-and-result-format-version-2-with-a-context-field.md)

Amends [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md)

## Context

ADR 30 placed the context area beside or below the question. It drew an
image fit into the area and a placeholder for text and diff entries, and
`z` and `o` acted on images only (ADR 28). The plan todo "Split the TUI
into a question pane and a context pane" makes the area a pane of its
own in its third step: a header, viewers for each kind of entry, and
scrolling with its own keys. The text and diff viewers follow in the
plan's next two steps, and phase 2 shows several entries per question
with a tab strip.

## Decision

Taken with the user (plan, *Keys*):

- **Header:** the pane starts with a line that names the entry by its
  `title`, or by its file name.
- **Viewers:** each kind of entry is shown by a viewer, which lays its
  content out for the pane's width and height, says whether its lines
  scroll sideways, and draws itself at a scroll offset. Images are the
  first viewer; text and diff entries keep their placeholder, as a
  viewer, until their own viewers land.
- **`z`:** shows any entry full screen, not only an image. `o` still
  opens images only. Both work from the question and from the focused
  pane.
- **Page keys:** `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` scroll the pane
  from anywhere, also while the person types in a field. `ctrl-u` and
  `ctrl-d` serve laptops without page keys and are the same on every
  keyboard layout.
- **Focus:** `tab` gives the pane the focus, `tab` or `esc` give it
  back. `tab` had no meaning since ADR 22 removed it. The focused pane
  scrolls with `↑`/`↓` and `j`/`k` by line, `←`/`→` and `h`/`l`
  sideways, `g`/`G` and `home`/`end` to the ends.
- **Key bar and help** show the pane's keys while it has the focus.
- **No keys that need AltGr or Option** on non-US layouts, such as `[`
  and `]`.

Settled in the review of this step:

- **`tab` never types:** it focuses the pane wherever one shows, from a
  field too, and does nothing otherwise. On a text question, whose
  field has the focus from the start, the pane would otherwise need
  `esc` first.
- **`ctrl-u` and `ctrl-d`** belong to the pane only while one shows.
  Without a pane, a field's editor keeps its own meaning for them
  (`ctrl-u` undoes, `ctrl-d` deletes the character under the cursor).

Agent's choices within that design:

- An entry without a title and without a path, such as inline text, is
  named by its kind (`text`, `diff`) in the header.
- When the content is taller than the pane, the header's right end says
  which lines show, such as `4–15 of 120`, as the intro overlay does
  (ADR 30).
- A row without context leaves the pane empty, header included, so the
  layout does not jump.
- The full-screen view is the pane over the whole tab area, header
  included, inside the same one-column margins as the tab.
- The hint that stands in for the pane on a small terminal offers `z`
  for every entry.
- Viewers live as long as their entry is shown in a waiting session,
  not one draw (settled in the review of this step). The text
  viewer reads a file, highlights it and renders Markdown; building it
  on every frame would redo that work each time. A viewer is rebuilt
  when a replaced session file puts another entry in its place, and
  dropped when its session leaves the queue.
- **Scroll position per entry:** every entry keeps its own position
  while its session waits. Moving to another entry shows it where the
  person left it, or at its top the first time; coming back restores
  the first one's. Offsets of different entries have nothing to do with
  each other, as their content and length differ, so carrying one over
  would land in the middle of unrelated text. Rows that show the
  question's entry share its position. A replaced session file starts
  every entry at its top.
- The scroll keys and their conventions are the intro overlay's (ADR
  30), and both share one function: a line key moves one line, a page
  is the pane's height, and an offset a larger terminal left past the
  end is brought back before a key applies. `←`/`→` move 4 columns.
- The focused pane reads like a pager, as the intro overlay does: it
  takes every key, and only `q`, `?`, `z`, `o` and `ctrl-c` keep their
  meaning from the question. The pane's focus is kept apart from a
  field's, so `tab` or `esc` return to the field or note being typed in
  with its text cursor where it was.
- While the pane has the focus, its header is highlighted the way the
  current tab is, and a field's text cursor hides, so only one place
  looks like it takes the keys.
- A pane that goes away while it has the focus, as after a resize down
  to the hint, gives the focus back.
- The full-screen view keeps the focus as it was, and the scroll keys
  work in it as in the pane.
- The key bar names `tab` before `?` wherever a pane shows. The general
  help keeps its 13 lines, which fill the smallest screen: `space` and
  `1`-`9` share a line, as do `n` and `ctrl-j`, to make room for `tab`
  and the page keys. While the pane has the focus, `?` lists the pane's
  keys instead.

## Consequences

- The text and diff viewers plug into the pane without changing it, and
  so does phase 2's tab strip above the body.
- The pane takes one row for its header, so an image beside or below
  the question gets one row less than before.
- `z` on a text or diff entry shows its placeholder full screen until
  the viewers land.
- A field can no longer take a typed tab character.
- While a pane shows, `ctrl-u` no longer undoes and `ctrl-d` no longer
  deletes in a field.
- `tab context` made the key bar too long for 80 columns, where it cut
  `q quit` in the middle. The key bar now leaves out whole hints that do
  not fit, by a rank: `n` first, then `tab`, then `←`/`→`, then the
  others from the right, and `?` and `q` last, since the help lists
  every key and quitting is what a lost person looks for. On 80 columns
  the question's bar drops `n note`; on 60 columns it also drops
  `tab context`, where before it showed a cut `tab  co` and no
  `? help`.
