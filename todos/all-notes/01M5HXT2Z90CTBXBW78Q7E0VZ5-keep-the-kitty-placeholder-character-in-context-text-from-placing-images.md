---
title: "Keep the Kitty placeholder character in context text from placing images"
kind: bug
component: tui
impact: low
origin: implementation
tags: [ux, field-test]
---
# Keep the Kitty placeholder character in context text from placing images

Found on 2026-10-22 while building the `ansi` context entry. Kitty places
images through Unicode placeholder cells: the character U+10EEEE drawn
with a foreground colour that names the image. A context entry whose text
holds U+10EEEE and draws it with an RGB or indexed foreground, through a
code entry's highlighting or an `ansi` entry's colours, may lead Kitty to
draw image cells where the entry has text.

## Problem

Reasoned from how Kitty's placeholders work, not reproduced
(2026-10-22). It needs crafted content: the character is a private-use
code point that does not occur in ordinary text.

## Suggested fix

First reproduce it in Kitty with a code entry and an `ansi` entry that
hold U+10EEEE in colour. If Kitty draws image cells, replace the
character before drawing text, for example with U+FFFD, in every viewer
that draws an entry's text.

## Done when

- The case is reproduced in Kitty, or shown not to happen, and the
  outcome is written here or in the fix's commit.
- If it happens, a test pins that an entry's text never puts U+10EEEE
  into the buffer.
