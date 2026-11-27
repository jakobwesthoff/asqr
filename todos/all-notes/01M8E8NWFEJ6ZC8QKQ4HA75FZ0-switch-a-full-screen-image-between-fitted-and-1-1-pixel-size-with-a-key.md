---
title: "Switch a full-screen image between fitted and 1:1 pixel size with a key"
kind: feature
component: tui
origin: request
tags: [ux]
---
# Switch a full-screen image between fitted and 1:1 pixel size with a key

Requested by Jakob on 2026-11-27 while he reviewed screenshots in an asqr
round: in the full-screen view (`z`), a key that shows the image at 1:1
pixel size, one image pixel per screen pixel, and switches back to the
fitted size. Fitted is what the viewer does today: it scales an image up
to fill the body as far as its proportions allow, or, for an entry with
`enlarge: false`, keeps its natural size where that fits
(`src/tui/render/viewer.rs`, `image_content`). A screenshot scaled to the
pane hides whether a line is one pixel or two, a blurred edge, or text
rendered at the wrong size, which is what a person checks it for.

## Scope

- At least the full-screen view takes the key; the context pane may take
  it too.
- The key switches between fitted and 1:1 for the image shown, and the
  key hints and the help name it. The key itself is still to choose: `1`
  is the usual one in image viewers, if no other binding of the full-screen
  view takes it (`src/tui/session.rs`, `pane_key`, where `z` steps
  through the full-screen views).
- 1:1 needs the terminal's cell size in pixels to know how many cells an
  image takes. An image larger than the view at 1:1 shows its top left
  part, or scrolls, which is what the related todo covers.

## Done when

- In the full-screen view the key shows the image at 1:1 and again at the
  fitted size, in the terminals of the field test.

## Relations

- Relates to: [Draw an image partly scrolled off in a pane](01M621K2HGAK8EDMX4T1RVXPAY-draw-an-image-partly-scrolled-off-in-a-pane.md)
