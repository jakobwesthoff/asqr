# Paragraph after a raw block element joins its line

Status: seen in real use on 2026-09-28; the user wants it checked and
fixed later, not now.

## What the user saw

A question `text` from the torchsnap mascot logbook (session "Cosplay:
alt texts of the new keys, reworked"), in the user's terminal:

```
**031 pumpkin-lantern-dark-horse-rider**

Now:

> Snappy waving, jack-o'-lantern under a wing -- the horseman rides through the hollow, looking for a head and a free hotkey.

Draft (111 characters, the thrown-pumpkin scene; drops "the hollow" (title word) and the sibling's "hessian"):

> Snappy waving, head tucked under a wing -- the schoolmaster fled, the pumpkin flew, and the app launched first.
```

rendered as (screenshot from the user, transcribed):

```
031 pumpkin-lantern-dark-horse-rider

Now:

> Snappy waving, jack-o'-lantern under a wing -- the horseman rides through the hollow, looking for a head and a free hotkey.Draft (111 characters, the thrown-pumpkin scene;
drops "the hollow" (title word) and the sibling's "hessian"):

> Snappy waving, head tucked under a wing -- the schoolmaster fled, the pumpkin flew, and the app launched first.
```

The paragraph after the first quote is glued onto the quote's line,
without the blank line and without even a space ("hotkey.Draft"). The
user found the question hard to read and could not tell which line was
the draft.

## Expected

The `> ` staying visible is intended: blockquotes are outside the
Markdown subset, and section 7.6 of the spec says every other element is
shown once as written. The glued paragraph is not: "Draft (...)" is its
own paragraph and should start after a blank line, as paragraphs do
everywhere else.

## Likely cause (from reading the code, not run yet)

`src/tui/render/markdown.rs`, `render()`:

1. `Event::Start(tag)` for an unsupported block element (here
   `Tag::BlockQuote`) calls `begin_block()`, then `renderer.raw(range)`,
   which pushes the source slice into `renderer.current`, then skips the
   element's inner events up to its end. Nothing flushes `current`
   afterwards.
2. The next `Event::Start(Tag::Paragraph)` only calls `begin_block()`
   when `renderer.current.is_empty()`. `current` still holds the raw
   quote, so no flush and no blank line happen, and the paragraph's text
   is appended to the quote's line.

If that is right, every unsupported block element followed by a
paragraph shows it (heading, code block, HTML block, thematic break),
not only quotes. Inside a list the `renderer.current.len() > 1` branch
handles it differently; check that too.

## To check and do

- Add a failing test first, next to
  `shows_everything_outside_the_subset_as_written`:
  `text("> a\n\nb")` should give `["> a", "", "b"]`; likewise
  `text("# a\n\nb")` and a code block followed by a paragraph. The
  existing tests only have the unsupported element last, which is why
  this was not caught.
- Then fix: flush after the raw emission of a non-inline element (or
  make the paragraph start check whether the pending line came from a
  raw block).
- Check the skill (`skills/asqr/SKILL.md`, section "The session file")
  describes the formatting precisely enough for agents. Today it lists
  what is styled ("bold, italic, inline code, lists and line breaks")
  but not what happens to everything else. The agent that wrote the
  question above assumed blockquotes work. The skill should say that
  other Markdown (quotes, headings, links, code blocks) shows as literal
  source, and suggest what to use instead for setting a line apart
  (a bold label on its own line, or a list item).
