# 31. Make the context area a pane with a header, focus and scrolling

Date: 2026-10-14

## Status

Accepted

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

Amends [28. Session and result format version 2 with a context field](0028-session-and-result-format-version-2-with-a-context-field.md)

Amends [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md)

Amended by [32. Let an image entry opt out of enlarging](0032-let-an-image-entry-opt-out-of-enlarging.md)

Amended by [33. Show diffs as a unified diff computed with similar](0033-show-diffs-as-a-unified-diff-computed-with-similar.md)

Amended by [34. Show code and Markdown entries through patine](0034-show-code-and-markdown-entries-through-patine.md)

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
- **Viewers:** each kind of entry is shown by a viewer behind one
  interface, so new kinds plug into the pane without changing it.
  Images are the first viewer; text and diff entries keep their
  placeholder, as a viewer, until their own viewers land.
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

Settled with the user in the review of the viewer interface:

- **Ownership:** a viewer owns its content, which does not change for
  the viewer's life, and everything derived from it. The pane owns
  everything around it: the header, the scroll position and the keys.
  An image viewer owns its decoded image, built from its own copy of
  the terminal's graphics protocol; no shared image cache remains.
- **Lifecycle:** a viewer is built cheaply when its entry is first
  shown, and `load` then prepares everything that does not depend on
  the body's size, once: reading files, decoding images, highlighting,
  computing a diff. `lay_out` arranges the content for a body size and
  returns its size; `draw` draws it at a scroll offset on every frame.
  The interface is used as a trait object, which cannot hold a
  constructor, so without `load` each viewer would decide alone whether
  expensive work happens when it is built or on first use. The pane
  decides when to load, which leaves room to load ahead of time.
- **Failures are content:** `load` returns nothing. A missing file or an
  undecodable image shows in the pane, which could do nothing better
  with it, and the pane needs no error handling per kind of entry.
- **Viewers live as long as their entry** in a waiting session, not one
  frame. Building them on every frame would read, highlight and render
  again each time. A viewer is rebuilt when a replaced session file
  puts another entry in its place, and dropped when its session leaves
  the queue.
- **Layout only on a new size:** as the content is fixed, the layout
  depends on the body's size alone. The keeper of the viewers remembers
  the size each viewer was last laid out for and what it returned, and
  calls `lay_out` again only for another size, so a viewer computes
  without checking for changes itself, and `draw` always follows a
  `lay_out` of its area's size.
- **Sideways scrolling follows from the size:** content that wraps
  reports at most the body's width, so lines wider than the body mean
  cut lines, and the pane offers sideways scrolling only then. A
  separate flag could contradict the layout.
- **Drawing:** `draw` gets an area and a buffer, as ratatui's own
  widgets do. It needs the area for its position, which depends on the
  arrangement and on `z` while the size stays the same, and nothing on
  the frame beyond the buffer. A viewer never learns whether it is
  shown full screen: `z` only gives it a larger area.
- **Images scale to their area,** keeping their proportions, in the pane
  as in the `z` view. A small image fills the pane, so the person sees
  more of it.
- **Focus regained (ADR 24)** drops only the image viewers, so their
  images are sent to the terminal again. Text and diff viewers keep what
  they derived: highlighting a large file again on every change of
  window would stall.

Agent's choices within that design:

- An entry without a title and without a path, such as inline text, is
  named by its kind (`text`, `diff`) in the header.
- When the content is taller than the pane, the header's right end says
  which lines show, such as `4–15 of 120`, as the intro overlay does
  (ADR 30). While the pane does not have the focus and its content is
  taller or wider than the pane, the right end also names `tab`, such
  as `1–10 of 31 · tab`. On 80 columns the key bar of a `multi`
  question drops its `tab context` hint, and `pgup`/`pgdn` never show
  there, so no other place would name a key that reaches the pane. The
  `tab` goes before the name would have to be cut.
- A row without context leaves the pane empty, header included, so the
  layout does not jump.
- The full-screen view is the pane over the whole tab area, header
  included, inside the same one-column margins as the tab.
- The hint that stands in for the pane on a small terminal offers `z`
  for every entry. While a field or a note has the keys, where `z` is
  typed, it reads `esc, z: full screen`, and the cut intro in the header
  reads `… esc, i: more` for the same reason; one function decides both.
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
  takes every key, and only `?`, `z`, `o` and `ctrl-c` keep their
  meaning from the question. `q` goes back like `tab` and `esc`, as it
  closes the intro overlay: a person who moved into the pane from a
  field and goes on typing would otherwise quit asqr with the first `q`
  of a word. The pane's focus is kept apart from a field's, so `tab`,
  `esc` or `q` return to the field or note being typed in with its text
  cursor where it was.
- While the pane has the focus, its header is highlighted the way the
  current tab is, and a field's text cursor hides, so only one place
  looks like it takes the keys.
- A pane that goes away while it has the focus, as after a resize down
  to the hint, gives the focus back.
- The full-screen view reads like a pager too: it takes the focus,
  remembering where it was, and scrolls with the pane's keys, with `o`
  and `?` as in the pane. `z`, `esc`, `tab` and `q` leave it and give
  the focus back to the question, the field being typed in or the
  focused pane. The question is hidden behind the view, so a key that
  picked an option or opened a note there would act where the person
  cannot see it. As no key moves the cursor while it shows, only a
  replaced session file can change the entry under it, and the view
  ends when that happens.
- `o` on a code, Markdown or diff entry says that it opens images only
  and, outside the full-screen view, that `z` shows the entry full
  screen. Where no entry shows, it says there is nothing to open. The
  key never does nothing without a word.
- The key bar names `tab` before `?` wherever a pane shows. The general
  help keeps its 13 lines, which fill the smallest screen: `space` and
  `1`-`9` share a line, as do `n` and `ctrl-j`, to make room for `tab`
  and the page keys. While the pane or the full-screen view has the
  focus, `?` lists the pane's keys instead, and the key bar shows them
  with `? help` and the keys that go back: `tab/esc/q back` in the pane,
  `z/esc/q back` in the full-screen view.

## Consequences

- The text and diff viewers plug into the pane without changing it, and
  so does phase 2's tab strip above the body.
- The pane takes one row for its header, so an image beside or below
  the question gets one row less than before. An image smaller than the
  pane grows to fill it, where it kept its natural size before.
- Every viewer can be tested by drawing into a plain buffer.
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
- ratatui never rewrites the cells under a Sixel or iTerm2 image, which
  `ratatui-image` marks as skipped, so the first frame after the help,
  the intro or the session list closes over an image entry clears the
  terminal and draws the whole screen afresh.
