# 109. Draw GitHub alerts as callouts, dim under NO_COLOR

Date: 2026-11-01

## Status

Accepted

Amends [15. Configure through flags and environment and log into the cache directory](0015-configure-through-flags-and-environment-and-log-into-the-cache-directory.md):
the quote bar is no longer the one style chosen before drawing; the bars
of callouts are chosen with it

Amends [34. Show code and Markdown entries through patine](0034-show-code-and-markdown-entries-through-patine.md):
a continuation row repeats the bars of callouts as well as those of
quotes

Amends [35. Render the text fields as full Markdown through patine](0035-render-the-text-fields-as-full-markdown-through-patine.md):
the screen hands every place that renders Markdown one look, which
carries the style of quotes and of callouts

Amends [46. Build and lay out context viewers on worker threads](0046-build-and-lay-out-context-viewers-on-worker-threads.md):
a kept viewer and a queued job are matched by the look, not by the quote
style

Amends [56. Show text with ANSI colours as a context entry](0056-show-text-with-ansi-colours-as-a-context-entry.md):
an `ansi` viewer reads whether colours are on off the look

## Context

- patine 2.2.0 renders a quote whose first line is a marker, such as
  `> [!NOTE]`, as a callout: a heavy bar `┃` on every line, and as the
  first line the symbol of its kind with a bold title, in the kind's
  colour. Before, the marker showed as text in a quote.
- patine's callout style is set apart from its quote style. Under
  `NO_COLOR` the colours come off the finished screen and dim stays
  (spec section 7.3), so a callout drawn in colour would be left with
  its bar in the text colour, as a quote bar would.
- asqr handed one patine `QuoteStyle` to every place that renders
  Markdown, and read from it whether colours are on.
- A line wider than its room wraps behind a dim `↪` after the bars it
  starts with, and a list marker turns into blanks. Only the quote bar
  `│` counted as a bar.

## Decision

- **One look.** `MarkdownLook`, a `Copy` value holding whether colours
  are off, replaces the quote style as what the screen builds once per
  frame and hands to every place that renders Markdown. Its `options()`
  gives patine's `RenderOptions` with the quote style and the callout
  style, each chosen from `NO_COLOR` by a function of its own, so
  neither is derived from the other. The text fields and Markdown
  entries add their own options on top of it. A viewer is kept and a job
  is queued for an entry together with the look, and built again when
  the look changes. An `ansi` viewer reads whether colours are on from
  it.
- **Colours.** Without `NO_COLOR` callouts take patine's colours for
  each kind and its default spacing, which put no blank line between a
  title and the body. They take no role from `theme` (ADR 34). Under
  `NO_COLOR` the callout style is patine's `DimBar`: the bars dim, the
  symbol and title bold, no colour.
- **Wrapping.** `┃` counts as a bar like `│`. A callout's symbol and its
  blank count like a list marker after bars that end in `┃`, so a
  wrapped title's continuation rows line up under the title's text, as
  patine lines its own wrapped rows up. A number counts as an item's
  after bars of either kind.
