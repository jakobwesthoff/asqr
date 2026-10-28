# 74. Edit a typed field in the person's editor with ctrl-g

Date: 2026-10-28

## Status

Accepted

Amends [36. Treat every typed field alike](0036-treat-every-typed-field-alike.md):
`ctrl-g` opens the focused field in the person's editor

## Context

Every typed field takes several lines (ADR 36): the own answer, the
answer of a `text` question, a question's note, the session note and the
reject reason. Longer answers and notes are easier to write in the
person's own editor than in a field of the TUI.

The TUI owns the terminal: raw mode, the alternate screen, focus reports
and bracketed paste, and a thread reads its keys through crossterm.
crossterm's `poll` reads what the terminal sends into a buffer of its
own, so a thread that polls while another program reads the terminal
takes keys meant for that program. crossterm can also wait inside a
`read` past its poll's timeout, for the rest of a sequence that came in
part.

The `edit` crate (0.1.5) finds the person's editor: `VISUAL`, then
`EDITOR`, then a list of common editors per platform, and it skips a
value that names no program it finds. Without its `quoted-env` feature it
splits the value at whitespace; with it, as a shell does, so a value with
arguments and quotes works. `edit_file` runs the editor on a file with
stdin, stdout and stderr inherited, and its error for an editor that
exits with an error is of kind `Other`; with no editor found it is
`NotFound`.

## Decision

- **The key:** `ctrl-g` while a typed field or a question's note has the
  keys opens it in the person's editor. On a field's row left with `esc`
  it gives the field the keys again, as a paste does there (spec section
  7.7), and opens it. Elsewhere, in the context pane, the full-screen
  view and over an overlay, it does nothing.
- **The editor:** found through the `edit` crate with `quoted-env`.
- **The file:** `asqr-XXXXXX.md` in the system's temp directory, git
  style: the field's text on top, a blank line, the cut line
  `# ------------------------ >8 ------------------------`, the line
  `# Do not change or remove the line above. Everything below it is
  ignored.`, then as `#` lines the session (its title and id, or its id),
  the question (`Question <n> of <count>: <header or id>`) and the field,
  and the question's text a line at a time. The review's fields name no
  question. What the session wrote is cleaned as a paste is.
- **Reading back:** by the file's path, as an editor may save into a new
  file renamed over the old one. Everything above the first line that is
  exactly the cut line goes into the field, without the line breaks at
  its end and cleaned as a paste; `#` lines above it stay. A text that
  differs from the field's only by the line breaks at its end leaves the
  field as it was, and the status line says so.
- **Failures:** no editor found, a key reader that does not stop within a
  second, an editor that exits with an error or cannot run, a file that
  cannot be written or read back, and a cut line that is gone each leave
  the field as it was, and the status line says why in asqr's own words.
  A field that holds the cut line is not opened, as its text would come
  back cut short.
- **Text that would be lost:** when the cut line is gone, and when the
  session left while the editor ran, asqr keeps the file and the status
  line names it first, the temp directory as `$TMPDIR` where that
  variable names it, as the status line is cut at the screen's edge. The
  log has the whole path. The text does not go into the field of a
  session that left. When the asker withdrew it, the notice of the
  withdrawal takes the status line's one place next, so it names the
  file first as well.
- **The hand-over:** asqr finds the editor and pauses the key reader
  before it touches the terminal; a reader that does not stop within a
  second calls the pause off. It turns focus reports and bracketed paste
  off, shows the cursor, leaves the alternate screen and raw mode, and
  notes the terminal's settings. After the editor it puts those settings
  back before raw mode goes on, as crossterm takes whatever is set then
  for the settings to give back on quitting, enters the alternate screen
  and turns paste and focus reports on. Its last step, in raw mode and on
  Unix, drops what the terminal sends until 150 ms pass without input, at
  most for a second, as an answer to a query the editor sent the terminal
  would reach the key reader as keys. Then the key reader goes on.
- **The key reader** reads on a thread that looks at a gate between its
  polls of 50 ms; a pause waits until the thread stopped at the gate,
  where it calls neither `poll` nor `read`.
- **Signals:** while the editor runs, SIGINT does not quit asqr: ctrl-c
  in an editor that leaves the terminal's signals on reaches both, and is
  meant for the editor. SIGTERM and SIGHUP quit once the editor exited.
- **After the editor:** the screen is drawn whole, by a resize to the
  size it has, which asks the terminal nothing; every image is sent
  again under new ids; the title and the progress go out again; the
  keep-alive of the progress rests while the editor runs.
- **Nothing else runs meanwhile:** the run loop waits for the editor, so
  what arrives in the queue waits in its channel, and the text goes into
  the field `ctrl-g` came from.
- **Key bar and help:** a field's and a note's key bar name `ctrl-g
  editor` last of their keys, so a narrow bar leaves it out before them;
  the general help names it on the line of `esc`, as `esc, ctrl-g  stop
  typing; edit the field in your editor`, and keeps its 13 lines.
- In ADR 36 this adds `ctrl-g` to the keys every typed field shares.

## Consequences

- An editor that returns at once, such as `code` without `-w`, leaves
  the field unchanged.
- A kill while the editor runs ends asqr only once the editor exited.
- Keys typed between `ctrl-g` and the pause reach asqr after the editor.
- After the editor, on Unix, asqr waits at least 150 ms and at most a
  second, and drops what the terminal sends then, answers to the
  editor's queries and keys typed alike.
- On 80 columns beside a context pane, a field's key bar shows `ctrl-g
  editor` and leaves out `tab  context`; on 60 columns it leaves out
  `ctrl-g  editor`.
