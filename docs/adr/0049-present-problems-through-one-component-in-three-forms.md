# 49. Present problems through one component in three forms

Date: 2026-10-16

## Status

Accepted

Amends [33. Show diffs as a unified diff computed with similar](0033-show-diffs-as-a-unified-diff-computed-with-similar.md)

Amends [34. Show code and Markdown entries through patine](0034-show-code-and-markdown-entries-through-patine.md)

Amends [38. Give notices kinds and confirm a finished session](0038-give-notices-kinds-and-confirm-a-finished-session.md)

Amends [46. Build and lay out context viewers on worker threads](0046-build-and-lay-out-context-viewers-on-worker-threads.md)

## Context

A context entry asqr could not show drew a placeholder of three lines
in the pane: a dim heading such as `code not shown`, the path wrapped at
the pane's edge, also inside a word, and a dim reason from the system or
the image decoder. A diff's placeholder did not say which side failed.
An invalid session showed only as a notice in the status line, its
validation errors joined with `; ` and cut at the line's edge, so the
whole reason showed nowhere in asqr. Under `NO_COLOR` the kinds of
notices looked alike (ADR 38).

Errors show in red with a mark, through one component of asqr's own.

## Decision

One component, the problem, shows what went wrong, in three forms: a
card in the context pane, a line in the status line and a dialog over
the screen. Its severity gives its mark and colour: `✗` in red for an
error, `!` in yellow for a warning.

- **Card.** A context entry that cannot be shown draws a card with a
  red rounded frame in the pane, in place of its content: `✗` and
  the reason in bold red, a blank line, the file name, and the folder
  on the lines after it. The folder shows the home folder (`HOME`) as
  `~` and wraps after a `/`; a part longer than a line breaks where the
  line ends. The path is never shortened. The card is at most 60
  columns wide and centred in the pane; a card taller than the pane
  starts at its top and scrolls. A diff's card puts `old side: ` or
  `new side: ` before the reason.
- **Reasons keep the wording of their source**: the system's or the image
  decoder's text, and asqr's own "relative path", "not a regular file",
  "binary file" and "image file larger than 64 MiB" (ADR 46). The
  original message also goes to the log when the viewer is built.
- **Header mark.** The pane's header shows a red `✗` before the entry's
  name when its viewer shows a card. The one-line hint, the tab bar and
  option rows show no mark: the pane always shows the card, so it is
  enough. The one-line hint of a terminal too small for the pane
  (ADR 30) shows none, as no viewer is built there (ADR 46).
- **Dialog.** An invalid session opens a dialog framed in yellow, the
  warning colour, that lists each of its errors on a line of its own.
  The next key closes it and does nothing else.
- **Status line.** An error notice or message starts with `✗`, a
  warning with `!`. `✗ rejected "<name>"` is the line of a rejected
  session (ADR 38).
- **Talqi** is not part of any problem form. He belongs to the empty
  queue and stays there under a notice of any kind; the dialog of an
  invalid session hides him while it is open.

The card shows the reason as the system, the image decoder or asqr's
own check wrote it. The log line of the card also names the cause in
one word: the system's error kind, such as `NotFound`; the image
decoder's kind of error, such as `Decoding` or `IoError(UnexpectedEof)`
for image data that ends early; or which of asqr's checks refused the
file, such as `Directory` or `NotRegular`, which read alike on the card.

## Consequences

- Under `NO_COLOR` an error and a warning in the status line differ by
  their mark; `✗ rejected` and an error notice both start with `✗` and
  differ by their words.
- The status line does not carry an invalid session's errors; the
  dialog does.
- A key closes the dialog only once a frame showed it. The message of a
  terminal too small for asqr leaves the dialog off the screen; a key
  there acts as it would without the dialog, and the dialog waits for
  the next frame with room for it. Otherwise such a key would close a
  dialog the person never saw.
