# 78. Open code, Markdown and diff entries with the system opener

Date: 2026-10-30

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md):
`o` opens the entry shown in the app for its file type, not only an
image

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md):
`o` opens code, Markdown and diff entries, and refuses only an ANSI
entry

## Context

- `o` opened the image shown with the system's opener. On a code,
  Markdown or diff entry it opened nothing and said "o opens images
  only".
- A code, Markdown or ANSI entry gives its text in the file at `path` or
  inline in `content`; each side of a diff does the same (spec section
  3.3).
- asqr started the opener and never waited for it, so every `o` left a
  finished process behind until asqr quit.

## Decision

- **The person's app:** `o` opens code, Markdown and diff entries through
  the system's opener, `open` on macOS and `xdg-open` elsewhere, as it
  opens images, so the app the person set up for the file type shows
  them. asqr stays on screen and does not hand the terminal over.
- **An entry's own file:** an entry with a `path` opens that file. The
  path goes through the rule every viewer follows first (spec section
  8.3), an image's included: a relative path, a missing file and a path
  that is no regular file open nothing, and the notice gives the reason
  the pane's card gives.
- **Written files:** inline content and every diff are written to a file
  first, with a fitting extension: a code entry its language's, a
  Markdown entry `.md`, a diff `.diff`, as a unified diff from the old
  and the new text.
- **Extensions from the highlighting:** a code file's name comes from
  the syntaxes the code viewer highlights with, two-face's set, which
  asqr loads through its own dependency on two-face at the version and
  with the features patine uses, so no other crate comes in. A title
  whose extension, or whole name, a syntax lists stays as written, where
  it fits the `language`. Otherwise the file gets the `language` itself
  where its syntax lists it as an extension (`yml`), else the first of
  the syntax's extensions made only of lowercase letters, digits, `+`,
  `_` and `-` (`rust` gives `rs`, `cmake` gives `cmake`), else `.txt`.
  An entry with a `path` opens whatever file it names, so the asker
  decides the file type there; a written file never takes an extension
  the highlighting does not know.
- **The diff's file:** its sides are read and compared as the pane reads
  and compares them, so the file holds the hunks the pane shows, with 3
  lines of context. The notes the pane shows about cut sides and line
  endings come first, each as a line of its own above the header, which
  `patch` and `git apply` pass over. The header names the file after the
  entry's `name` or `title` as `a/<name>` and `b/<name>`, a side the
  entry leaves out as `/dev/null`.
- **Where written files go:** into a directory of their own each, inside
  a directory for the run, inside one directory per user in the system's
  temp directory, `asqr-<uid>`, made so only its owner may enter it. An
  `asqr-<uid>` that is a link, belongs to another user or lets others in
  is not written into.
- **Written files outlive the run:** removing them when asqr quits would
  leave the person's editor or viewer with a deleted file, as VS Code and
  others mark the tab of a deleted file, and an app that reads its file
  late may not have read it yet.
- **Their removal:** each run holds an exclusive lock on `lock` in its
  directory for its whole life. When asqr starts its TUI, it removes a
  run's directory only when that is older than `KEEP_OPENED_FILES`, one
  day, and its lock can be taken, or it has no lock file. A run sets its
  directory's modification time when it ends, so the day counts from the
  later of its last file and its end. A running asqr's files stay
  however old they are, as its lock is held.
- **The same entry again:** `o` on an entry a run wrote a file for
  before opens that file again while it is there, as it opens an entry's
  own file again.
- **ANSI entries:** `o` refuses an ANSI entry and says "o does not open
  ANSI text", outside the full-screen view with "; z shows this entry
  full screen".
- **The opener's process:** asqr waits for it on a thread, so no finished
  opener stays behind, and starts it in a process group of its own, so a
  program it starts in the terminal does not take asqr's keys. An opener
  that exits with an error, or that a signal ends, shows a notice that
  says so.

## Consequences

- Written files stay until the first start of asqr's TUI that finds
  their run ended and its last file, or its end, more than a day ago,
  however long that is.
- `o` on a diff, or on a large inline entry, writes its file on the run
  loop's thread at the key press. A diff reads both sides there, each up
  to 8 MiB, and is computed within the 1 s that bounds every diff.
- The first `o` that needs an extension loads a second copy of the
  syntax set, as patine keeps its own private.
