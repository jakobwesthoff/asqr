# Images cropped when the cell size differs from the one at startup

Status: seen on 2026-10-01 in daily use; the user asked for this todo
with everything known so far. Not yet reproduced on purpose.

## What the user saw

Images in asqr sessions showed only a cropped cutout of the picture.
Every session before about 22:59 CEST had shown its images in full.
The first one affected was session 01M3WM86TKAGSEC199KRP6A1FZ, dropped
at 20:59:45Z (asqr log). The user had restarted asqr shortly before.
All later sessions were affected too, among them
01M3WMY5EZERQY2G0RF9SPY68V, a size round whose images are 1566×424 px
WebP files of four tiles.

The image files were checked and are whole (dimensions and content
looked at outside asqr). The asqr binary (0.9.1, built 2026-09-29) had not
changed. The session files were built the same way all day.

Things the user tried:

- Restarting the terminal: no change.
- `tmux attach -d`: no change, "feels even more broken". The other
  client was in fact still attached afterwards (see below).
- Zooming in, raising the terminal's font size: the image fit and
  displayed whole again.

## The environment at the time

One tmux server (3.7c, running since 2026-09-28), with
`allow-passthrough all` and `default-terminal tmux-256color`. Two
clients were attached to the same session:

| Client | Terminal | Size | Cell size (px) | Attached since |
|---|---|---|---|---|
| `/dev/ttys000` | Ghostty `1.3.2-mouse-fix-only-+448062571`, local on the Mac | 197×56 | 19×48 | 09:41 |
| `/dev/ttys009` | Ghostty `1.3.1`, over SSH from another machine | 219×50 | 15×39 | 23:18 |

The restarted terminal was the SSH session. Which client was in use
while images still worked is not known; the local one, attached since
09:41, is the likely one. After the local client was
detached (`tmux detach-client -t /dev/ttys000`), the SSH client
reported 205×46 with 16×42 px cells. Whether asqr was restarted after
that detach, and whether the crop persisted then, is not known.

## What the code does

- `src/terminal.rs` builds the `Picker` once at startup with
  `Picker::from_query_stdio()`, provided `may_query_protocol` allows the
  query. Inside tmux it does so when passthrough is on and at least one
  client is attached. The query result, including the cell size in
  pixels, is never refreshed.
- `src/tui/render/images.rs` encodes each image with
  `picker.new_resize_protocol` and caches it by path. `Resize::Fit`
  fits the image to the area in cells, converted to pixels with the
  picker's cell size.

## Hypothesis (unconfirmed)

asqr's cell size in pixels does not match the cell size of the terminal
that displays the image:

- With two clients attached, both terminals may answer the passthrough
  query, and asqr takes whichever answer arrives first. Here that could
  have been the local client's 19×48. tmux then lays the window out for
  the other client's 15×39 or 16×42 cells.
- An image sized for larger cells covers more pixels than the area has
  on the smaller-celled terminal, so the terminal clips it to the cells
  of the placement. That is a cropped cutout.
- Raising the font size makes the real cells big enough, which matches
  the user's observation.

A font size change while asqr runs would produce the same mismatch, as
the picker is never asked again.

## To investigate

- Log the picker's protocol type and font size at startup (the log
  shows neither today).
- Reproduce directly in Ghostty, without tmux: start asqr, then change
  the font size, then show an image.
- Reproduce in tmux with two clients of different cell sizes attached
  to one session, starting asqr while both are attached.
- Decide how asqr should learn a changed cell size. Options:
  - query again on resize or focus events;
  - read the pixel size from `TIOCGWINSZ` (ws_xpixel/ws_ypixel) on
    each resize;
  - at least clear the image cache on resize, as `Images::forget` does
    for focus.
- Related: `01m3kgxh7f2sns4hf0zhzwnqjv-image-vanishes-in-narrow-terminals.md`
  covers images lost after a resize; both may share the "terminal
  state learned once" cause.
