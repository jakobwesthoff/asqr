# Design guide

The colours, their roles and the rules for combining them, for everything
asqr shows outside the terminal: the website, documentation, release
graphics and illustrations. The terminal interface has its own section at
the end.

## Palette

The palette has two cool colour families, a tinted dark, and earth tones
that only illustrations use.

### Teal, the primary family

Teal is one ladder of brightness, from the lightest highlight to the
darkest shade.

| Name | Value | Use |
|---|---|---|
| Mint | `#98F3DD` | highlights, spots, light tints |
| Light teal | `#5EE5CD` | the main teal on dark backgrounds; large lit areas |
| Teal | `#28C9B8` | the main teal in illustrations and on large surfaces |
| Deep teal | `#0DA6A5` | shading; teal elements on dark backgrounds |
| Dark teal | `#0A7C7A` | teal text, links and buttons on light backgrounds |

### Lavender, the accent family

| Name | Value | Use |
|---|---|---|
| Pale lavender | `#C5B2F6` | soft highlights, light accent tints |
| Lavender | `#D1A9FC` | the accent on dark backgrounds |
| Violet | `#9573ED` | shaded lavender; accent borders and focus rings on light backgrounds |
| Dark violet | `#7A55D9` | accent text on light backgrounds |

### Neutrals

| Name | Value | Use |
|---|---|---|
| Ink | `#09171A` | text on light backgrounds; the dark background |
| Dark surface | `#12282C` | panels and cards on the dark background |
| Contour | `#36463F` | outlines in illustrations |
| Paper | `#F7FEFC` | the light background |
| Mint paper | `#EAF8F4` | panels and cards on the light background |
| Light text | `#E6F2EF` | text on dark backgrounds |

### Earth tones

| Name | Value | Use |
|---|---|---|
| Bark | `#A45E31` | illustrations only |
| Leaf | `#BCC24C` | illustrations only |
| Leaf shade | `#4F8F6F` | illustrations only |

### Status

| Name | Light background | Dark background |
|---|---|---|
| Success | `#3E7556` | `#BCC24C` |
| Error | `#B5523F` | `#E8846F` |

## Composition

- **Teal carries, lavender points.** Teal covers the large areas and
  everything the reader acts on. Lavender appears in small amounts, only
  where the eye should go: focus, selection, a highlighted detail. A page
  shows far more teal than lavender.
- **One teal at a time per surface.** Lighter and darker teals are light
  and shade of the same colour, not separate colours side by side.
- **Darks are tinted.** Shadows are deep teal or violet, the darkest
  colour is ink with its teal tint. Pure black and neutral grey do not
  appear.
- **Mint is light, not a colour of its own.** Mint marks highlights,
  spots and tints. It never carries text.
- **Highlights are glossy and bright.** Light falls as mint and near-white
  highlights on teal, and as pale lavender on lavender.
- **Warmth only grounds.** Bark and leaf appear in illustrations, below
  or behind the figure, never on buttons, links, backgrounds or text.
- **Teal is not a status.** Success and error have their own colours, so
  teal never reads as "done" or "good".

## Light and dark

| Element | Light | Dark |
|---|---|---|
| Background | Paper | Ink |
| Panel | Mint paper | Dark surface |
| Body text | Ink | Light text |
| Links, primary buttons | Dark teal | Light teal |
| Text on a primary button | Paper | Ink |
| Focus ring, selection | Violet | Lavender |
| Accent text | Dark violet | Lavender |

## Contrast

Text needs at least 4.5:1 against its background, large text and
interface elements such as focus rings at least 3:1 (WCAG 2 AA). The
pairs above reach:

| Foreground | Background | Ratio |
|---|---|---|
| Ink | Paper | 17.9:1 |
| Ink | Mint paper | 16.7:1 |
| Dark teal | Paper | 4.9:1 |
| Dark teal | Mint paper | 4.6:1 |
| Paper | Dark teal | 4.9:1 |
| Dark violet | Paper | 5.0:1 |
| Violet | Paper | 3.5:1 |
| Success | Paper | 5.3:1 |
| Error | Paper | 4.8:1 |
| Light text | Ink | 15.9:1 |
| Light text | Dark surface | 13.4:1 |
| Light teal | Ink | 11.8:1 |
| Ink | Light teal | 11.8:1 |
| Lavender | Ink | 9.4:1 |
| Lavender | Dark surface | 7.9:1 |
| Success | Ink | 9.5:1 |
| Error | Ink | 6.9:1 |

Deep teal on Paper reaches only 2.9:1 and Violet on Paper 3.5:1; neither
is used for text on light backgrounds.

## Illustrations

- The figure keeps its teal body and its lavender accents. A colour
  change is a joke for one picture, such as camouflage, and the figure
  always returns to teal.
- The eyes are round, glossy cartoon eyes with lavender eyelids, and both
  look at the viewer.
- The figure sits on its branch.
- Every object in a picture is connected to the figure or the branch.
  Nothing floats freely.
- No motion lines or reaction strokes.
- No text inside an illustration.
- Backgrounds are transparent, with hard edges around the figure.

## Terminal interface

The terminal interface uses the terminal's own colours, named and indexed
ANSI colours, so the person's terminal theme decides how they look. It
does not set fixed RGB values, and it leaves the background and body text
at the terminal's defaults. The contrast table above applies to the
palette's own values, not to a terminal theme.

| Role | Terminal colour | Where |
|---|---|---|
| Primary | cyan | the name `asqr` in the title line, the current tab and the line under it, the keys in the key bar, chosen marks, overlay borders, the submit button |
| Focus | yellow, as a fill | the row under the cursor, the focused field's head, the focused context pane's header |
| Warning | yellow | a length counter near or past its limit, a key that was refused |
| Success | green | a length counter in range, a submitted session |
| Error | red | a failure, such as an inbox that cannot be read |
| Secondary | dim | hints, unanswered tabs, the descriptions in the key bar |

- Only the focus is a filled area. The title line and the key bar have no
  fills.
- Code and Markdown keep the colours their highlighting gives them,
  including yellow inline code.
- An answered question is not a status: its tab mark keeps the text
  colour, and an unanswered one is dim.
- Meaning never depends on colour alone. Marks, pointers, text and the fill
  carry it, so the interface reads the same without colours.
