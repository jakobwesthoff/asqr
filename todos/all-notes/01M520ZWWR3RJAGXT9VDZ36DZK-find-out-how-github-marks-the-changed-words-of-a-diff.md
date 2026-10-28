---
title: "Find out how GitHub marks the changed words of a diff"
kind: investigation
component: tui
horizon: someday
origin: discussion
tags: [ux, field-test]
---
# Find out how GitHub marks the changed words of a diff

Jakob, 2026-10-16: marking changed words inside a diff did not work in
asqr, neither as built (similar's inline changes over a whole block)
nor in the three variants prepared for him (words across a block with
line breaks and comment leaders as blank, the same without marks on
wholly new lines, and line pairs at 40 % similarity as delta does). He
decided that asqr shows diffs line by line without marked words, and
that it is to be found out how GitHub and others make the marking work.

## What to find out

How GitHub, GitLab, delta and difftastic pair lines and decide which
words to mark, on the cases that failed in asqr: a reflowed comment
block, a reworded Markdown paragraph, a moved line next to changed
ones, re-indented code. Renders of asqr's variants on those cases and
the prototype are in
`~/playground/2026_10_15_asqr-maskottchen/vorbereitung/diff/`.

## Findings, 2026-10-28

Researched from the source of GitLab (gitlab-foss `3605a2aee`), delta
(`3c2269c`) and difftastic (`0b92338`); nothing was run, so the results
per case are read from the code, and delta's distances come from a
small port of its rule.

- **GitHub:** closed source; no documentation or reliable write-up of
  its rule was found. git's `contrib/diff-highlight` (README) pairs
  lines by position only in hunks with as many removed as added lines
  and marks what lies between the common prefix and suffix; whether
  GitHub does the same is a guess.
- **GitLab:** `Gitlab::Diff::PairSelector` pairs a run of `-` lines
  with an immediately following run of as many `+` lines, by position,
  with no similarity check; unequal runs get no marks. `InlineDiff` runs
  a character diff (`diff_match_patch` with `diff_cleanupSemantic`) per
  pair. On the cases: the reflowed comment and the Markdown paragraph
  pair by luck of equal counts and mark the moved words; the moved
  Cargo line and the re-indent (2 removed, 3 added) get no marks.
- **delta:** `infer_edits` (`src/edits.rs`) pairs greedily, in order:
  each removed line takes the first not yet used added line within
  `max-line-distance` (default 0.6, about similarity 0.4); pairs never
  cross, and lines in between stay unpaired. Distance is the changed
  width over changed plus twice the unchanged width, whitespace not
  counted. Words are `\w+` plus every other character as a token,
  aligned by Needleman-Wunsch (`src/align.rs`); unchanged whitespace
  between two changed runs joins the mark, so marks are not spotty.
  Unpaired lines get no marks. On the cases: the moved `ratatui` line
  stays unpaired and `serde`/`serde_json` pair with their new versions,
  marking only the changed numbers; the re-indented lines pair at
  distance 0 with only the indent changed; the reflowed comment and the
  Markdown paragraph still mark words that moved across a line break.
- **difftastic:** for languages with a parser it diffs syntax trees,
  without line pairing; comments and strings in the same place are
  word-diffed with an LCS, marking words only when enough of them are
  common, else the whole comment. Moves are not detected; reflowed doc
  comments are a known hard case in its manual. Markdown has no parser
  and falls back to a block word diff like the one that failed in asqr.

What the evidence supports for asqr: delta's pairing (greedy, in order,
a similarity threshold) gets the pairs of all four cases right, and
`similar` already gives word diffs and a `ratio()` close to delta's
distance. No tool avoids marking words that moved across a line break
in reflowed prose or comments; only treating comment leaders as blank
(asqr's variant A) addresses part of it. Tokens like delta's (`\w+`
plus single punctuation) would mark `228` instead of a whole version
string. How a mark stays visible inside asqr's red or green filled rows
(bold, underline, reversed) has no evidence from these tools, which use
a brighter shade of the same hue.

## Revisit when

No trigger known; reconsider at the next sweep.
