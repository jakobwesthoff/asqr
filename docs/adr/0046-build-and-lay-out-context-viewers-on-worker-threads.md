# 46. Build and lay out context viewers on worker threads

Date: 2026-10-16

## Status

Accepted

Amends [24. Resend images when asqr regains the focus](0024-resend-images-when-asqr-regains-the-focus.md)

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amends [45. Keep image handling in a graphics module around an asqr-owned value](0045-keep-image-handling-in-a-graphics-module-around-an-asqr-owned-value.md)

Amended by [49. Present problems through one component in three forms](0049-present-problems-through-one-component-in-three-forms.md)

## Context

- ADR 31 split a viewer's start into building it when its entry was
  first shown and a `load` step. The architecture review of 2026-10-16
  found that the only caller loaded each viewer right after building it,
  and that laying out or drawing a viewer not loaded panicked. The first
  version of this ADR made building the viewer its loading and dropped
  `load`.
- All of it ran on the thread that draws the screen: reading, patine's
  highlighting, the diff and decoding an image while the viewer was
  built; rendering Markdown and scaling and encoding an image while it
  was laid out, again at every new size of the pane.
- Measured with release builds on 2026-10-16: building the viewer of the
  first 8 MiB of a log took 31.7 s, 23.0 s of it highlighting with the
  log grammar, at about 0.19 ms a line. In a terminal, the key that moved
  to that entry's question was followed by the next frame 23.4 s later.
  An image took 80 to 890 ms from reading to encoding, Markdown of 1.6 MB
  553 ms for one width. No key worked meanwhile, and nothing on the
  screen said why.
- An image kept its decoded picture for the next encoding: four bytes a
  pixel, 144 MB for one of 6000×6000.

## Decision

Jakob decided on 2026-10-16:

- A worker builds the viewer and lays it out, in one form for every
  kind of entry. A new size of the pane lays the viewer out again on a
  worker.
- While an entry loads, the pane shows a dim `loading…`, centred in the
  pane. asqr waits up to 100 ms for the result before it shows it.
- A load whose entry is no longer shown runs to its end, and what it
  loaded is kept while it is inside the window.
- asqr keeps viewers only inside a window of three questions: the
  previous, the current and the next one, each with all of its entries,
  the question's context and every option's. Loading ahead fills that
  window: the shown entry first, then the other options of the current
  question, then the next question, then the previous one. Everything
  outside the window is dropped, also entries that were shown; coming
  back loads them again behind the placeholder.
- A code entry longer than 1 MiB stays plain. Below that, an entry shows
  once it is highlighted, with no plain first pass. Markdown keeps its
  highlighted code blocks at any size.

The lead decided under Jakob's delegation:

- The entry shown gets a thread of its own at once. Loading ahead runs
  on one background thread in the window's order; the entry shown takes
  its job out of that queue when it has not started.
- Results come back on a channel the viewers own; a payload-free event
  wakes the run loop, which draws again.
- The entry shown counts as inside the window when its result arrives,
  as the pane asks for the entry of a new question before the window of
  that question is sent.
- Files are opened without waiting on a FIFO.
- An image keeps the bytes of its file as read and the format they were
  decoded in, and holds the decoded picture only until its first
  encoding. Each later encoding, for another size of the image in
  cells, decodes the bytes again on the worker. The image keeps its
  width and height in pixels from the first decoding, which give its
  cells for a new pane size, so a pane that changes size but leaves the
  image's cells as they were decodes nothing.
- An image file larger than 64 MiB is not read, and its card says
  "image file larger than 64 MiB". The size comes from the opened file,
  and the read stops one byte past the limit, so a file that grows in
  between is refused too. The review of 2026-10-16 found that asqr read
  an image file whole, whatever its size, and kept it for the window.

## Consequences

- Viewers and what they hold are `Send`. One job builds and lays out a
  viewer, and a worker runs it; drawing stays on the drawing thread.
- Every job has a ticket, and only the result of the latest job for an
  entry is taken: a session that left and came back under its id, an
  entry replaced or an image forgotten on regaining the focus leaves
  the result of an older job unused. A job keeps its thread to the end,
  as patine cannot be stopped. It stays noted when its entry leaves the
  window, and a frame that wants the same entry with the same quote
  style again takes it over instead of starting another, so coming back
  to an entry never starts a second job for it. Its result is dropped
  if it arrives while its entry is outside the window and is not the
  entry shown.
- A panic on a worker ends asqr as a panic while drawing does: the panic
  hook passes over the worker, the drawing thread panics with the
  worker's message on the frame the load woke, and the hook gives the
  terminal back once, there.
- Regaining the focus loads an image a frame showed again behind the
  placeholder, so its old copy may be deleted before the new one is
  sent; tmux had dropped it anyway. An image loaded ahead and never
  shown has sent nothing and stays.
- Entries load ahead for the body of a pane that shows: not for the `z`
  view, a one-line hint or a question without context. An entry loaded
  ahead before a resize lays out again when it is shown, and may show
  the placeholder then.
- Memory holds the viewers of at most three questions. Images keep
  their Kitty ids from their encoding on, also those loaded ahead and
  never sent; their deletes, when they go, reach a terminal that never
  had them.
- A shown image holds its file's bytes and its encoding, not its
  decoded picture; another size of the image in cells costs a decoding
  again. An image file
  changed on disk keeps showing what asqr read first.
- Writing an image to the terminal still happens while a frame is
  drawn; the workers do not shorten it.
