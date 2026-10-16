---
title: "Show diffs line by line without marked words"
kind: improvement
component: tui
origin: request
tags: [ux, field-test]
---
# Show diffs line by line without marked words

Jakob, 2026-10-16, after testing the diff of `src/tui/render/viewer.rs`
before and after ADR 46 and comparing three marking variants (asqr
session 01M51ZBV7A04CN2RED495S89V6): the marked words inside changed
lines did not show what changed, and none of the variants convinced
him. asqr goes back to a plain line diff.

## Change

- `src/tui/render/diff.rs` stops marking words: removed lines red,
  added lines green, no reversed emphasis inside a line. similar's
  `iter_inline_changes_with_options_deadline` gives way to plain line
  changes.
- Spec (`docs/spec.md:1121-1124`) and ADR 33 (lines
  30-32, 50-51 and 68-74) describe marked words; rewrite them, and drop
  or replace the tests about marked words in `diff.rs` (for example
  `the_changed_words_of_a_changed_line_stand_out`,
  `words_split_at_punctuation_so_code_shows_the_changed_name`,
  `a_line_rewritten_entirely_has_no_emphasis`).

How others mark words is a separate investigation for later.
