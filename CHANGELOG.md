# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The empty queue and the terminal title name the queue asqr watches,
  unless it is the default queue: "Nothing to answer in queue
  asqr-todos." and `asqr · asqr-todos · 2 waiting`.
- `o` opens a code, Markdown or diff entry in the app the system has
  for its file type, through `open` on macOS and `xdg-open` elsewhere,
  as it opens an image. An entry with a `path` opens that file. Inline
  code goes into a file with the extension of its language, inline
  Markdown into a `.md` file, and a diff into a `.diff` file as a
  unified diff. These files go into a directory of asqr's own in the
  system's temp directory and stay after asqr quits; a start of asqr
  removes those whose run ended more than a day ago. `o` on an ANSI
  entry still opens nothing and says so. In the focused pane and the
  full-screen view, the key bar names `o  open` wherever `o` opens the
  entry.
- `asqr skill <part>` prints one part of the agent skill: `context`,
  `withdrawing` or `patterns`. An unknown part exits 2 and names the
  parts.

### Changed

- The agent skill tells agents to add an `info` item only where no
  question can carry what it says, and to keep info items short, one
  topic each, instead of one long text.
- The agent skill tells agents to write every question for a person who
  has not seen their work, saying what a thing is instead of referring
  to it by a name only they know.
- Only the first line of a default option's label, or of its
  description beside the labels, leaves room for `◀ default`. The lines
  below wrap at the full width instead of leaving an empty column under
  the marker.
- `o` on an entry whose `path` is relative, names no file or is no
  regular file, an image included, says why in a notice instead of
  handing the path to the program that opens it.
- When the program that opens an entry for `o` exits with an error, a
  notice says so and names its exit code, or that a signal ended it.
  That program now runs in a process group of its own, so a program it
  starts in the terminal does not take asqr's keys.
- The agent skill tells agents that every session, one of info items
  only too, comes back as a result to wait for and read in full, every
  note included, and to look at `asqr result` or `asqr status` before
  answering in the chat about something the person may have answered in
  asqr.
- The agent skill is a core, `SKILL.md`, and three parts under
  `references/`, which the core names at the step that needs them. The
  core starts by naming its last line, so an agent can tell whether it
  read all of it. `asqr skill --install <dir>` writes the parts to
  `<dir>/asqr/references/` too and replaces that directory whole, so
  parts an older asqr wrote disappear.

### Fixed

- `asqr validate` and `asqr ask` report a context file that turns into a
  FIFO while they check it as "not a file". Before, they waited until
  something wrote to the FIFO.
- A draft restored for a session file whose question no longer offers
  an own answer drops the own text it held for that question. Before,
  the text stayed in the answer without a row to edit or clear it, the
  review showed it and a submit sent it.
- A question the person changed and left empty counts as started: a
  cleared default, a pick cleared on a question without a default, and
  own text or a note typed and erased again. `asqr withdraw` and `asqr
  ask --replaces` refuse such a session without `--force`, and with it
  asqr tells the person the session was withdrawn; `asqr ask` of the
  same id refuses it, also with `--force`, and `asqr status` lists its
  draft under `draft_has_answers`. The draft marks such a question with
  `cleared: true`, which asqr 0.14.0 and earlier ignore. `enter` on an
  empty own-answer row of a `single` without a pick no longer counts as
  a change. Before, the session went without `--force`, and the person's
  choice with it.
- A session restored from its draft picks a default the file gained on
  a question the person never touched, as a replaced file does. Before,
  a restart left such a default unpicked on a question that had none. A
  cleared default in a draft from asqr 0.14.0 or earlier comes back
  selected.
- Each `o` left the finished program that opened the image behind as a
  process until asqr quit; asqr now waits for it.

## [0.14.0] - 2026-10-29

### Added

- `s` shows a `diff` entry side by side: the old text on the left, the
  new text on the right, each removed line next to an added one and
  blank rows where one side has more. `s` again shows it unified, which
  stays the default. It works in the focused context pane, in the
  full-screen view and on the question's rows, and holds for the entry
  while it shows. Switching keeps the line at the top in view. A pane
  without room for two columns of at least 8 characters each keeps the
  diff unified and says so; a diff switched side by side shows so again
  once the pane is wide enough.
- An option with `default: true` shows `◀ default` in bold cyan at the
  right end of its first line, also after another option is picked, so
  the person can tell the asker's recommendation from their own pick. A
  label or description whose first line would reach it wraps 11 columns
  earlier. On the row under the cursor it takes the focus fill like the
  rest of the row.
- The unchanged lines of a `diff` entry are highlighted in its language,
  like a `code` entry: the extension of `name` picks it, and an optional
  `language`, such as `tsx`, overrides it. The new text is highlighted
  from its start, so a hunk inside a comment or a string colours right.
  A diff whose two sides together exceed 1 MiB shows without
  highlighting. `asqr validate` and `asqr ask` no longer warn about
  `language` on a diff entry.
- A question of kind `info` shows its text and context and asks
  nothing: it takes no options, own answer or length, and the person may
  add a note to it. It has a tab like a question, marked with a dim `ℹ`,
  the hint "nothing to answer" under its text and one row, `next`, on
  which `enter` moves on. It counts neither as answered nor as skipped,
  in the review, the confirmation of a submit and the session list. A
  result lists an info item only when the person wrote a note on it, as
  `{"question": ..., "note": ...}`. A session may consist of info items
  only. asqr 0.13.1 and earlier refuse a session with an info item.
- `asqr withdraw <id>` takes back a session the asker dropped into the
  queue, so the person no longer sees it. It prints a result with the
  new status `withdrawn`, which every `asqr wait` for the session and a
  later `asqr result` print too, exiting with 14. A session the person
  has started answering is refused unless `--force`; the result of a
  forced withdrawal carries what they had entered in a separate `draft`
  field, never in `answers`. A default the person left untouched is not
  something they entered: it neither counts as starting to answer nor
  goes into `draft`. A session answered already is refused. A withdrawal
  without `--force` never takes a session whose draft holds an answer,
  also while the person is typing in it. asqr shows `↩ withdrawn
  "<title>" by the asker` when a session the person has had on screen or
  entered answers into is withdrawn, with `, with your answers so far`
  when the result carries them and `; your answers to it are dropped`
  when it does not. `asqr status` lists a withdrawn session under
  answered, with status `withdrawn`.
- `asqr ask --replaces <old-id> <file>` withdraws the old session and
  drops the new one in one step, for a session rebuilt and asked again
  under a new id. It refuses where `asqr withdraw` or `asqr ask` would,
  and a refusal changes nothing. `--force` takes the old session also
  while the person is answering it, and moves an unread result of the
  new id aside.
- An image context entry takes an optional `caption`: Markdown shown
  under the image in the context pane, rendered like a Markdown entry.
  The image is scaled to the rows the caption leaves, so both show
  whole and nothing scrolls; a caption that leaves the image fewer than
  3 rows shows alone under a dim `z shows the image` and scrolls where
  it is taller than the pane. `z` from the pane shows the image alone;
  from the one-line hint it shows image and caption first, then the
  image alone. `asqr validate` and `asqr ask` warn about a caption above
  1 MiB.
- The agent skill suggests patterns for common kinds of task, such as
  comparing image variants, deciding before implementing and going
  through review findings. Each pattern names what a session can hold,
  as a starting point an agent adapts or leaves aside.
- asqr reports how much of the session being answered has an answer
  through OSC 9;4: Ghostty from 1.2.0 and iTerm2 from 3.6.6 show it as a
  bar in the window (iTerm2 from 3.7.0 also on the tab), Windows
  Terminal as a ring on the tab and a bar in the taskbar, and kitty from
  0.39.0 inside tmux in the tab's title. The share counts the questions
  with an answer, a default included, and reads 100 % on the review once
  all are answered. An empty queue shows none, and asqr removes it when
  it quits. Inside tmux it goes out in the passthrough wrapper while one
  client is attached. Other terminals get nothing, as several show any
  OSC 9 as a desktop notification.
- `ctrl-g` in a typed field or a note opens its text in the person's
  editor (`VISUAL`, then `EDITOR`, with arguments and quotes), as a
  Markdown file that names the session and the question below a cut
  line; the text above the line goes into the field when the editor
  exits. On a field left with `esc` it gives the field the keys again
  first. When no editor is found, the editor fails, or the cut line is
  gone, the field stays as it was and the status line says why; a text
  that would be lost stays in its file, which the status line names,
  also in the notice of a session withdrawn while the editor ran. A
  field's key bar names `ctrl-g  editor`, and the help names it on the
  line of `esc`. While the editor runs, ctrl-c there does not quit asqr.
  On Unix, after the editor exits, asqr drops what the terminal sends
  until 150 ms pass without input, at most for a second, so a late
  answer to the editor's queries does not become keys; keys typed in
  that moment are lost.

### Changed

- A removed line of a diff shows in a red fill and an added line in a
  green fill to the pane's edge, numbers and sign included, in place of
  red and green text. Changed lines show without syntax colours.
- A question of a `kind` asqr does not know is refused with an error
  that names the kind as not supported by this asqr and its version,
  lists the supported kinds, and says that a newer asqr may support it.
  New kinds are added within format version 2, as context entry types
  are, so an asqr that runs while a newer one is installed can meet one.
  `asqr validate` and `asqr ask` print it, and the dialog and the error
  result of an invalid session in the queue carry it. Before, the
  message was the parser's `unknown variant`.
- Every question and info item takes a note. Before, `"note": false`
  turned the question's note off, and `n` said that the question takes
  no note. An open note stays open on a replaced session file while its
  question shows, also where its row is gone.
- `pgup` and `pgdn` scroll the context pane, the full-screen view and
  the intro by their height less 2 lines, so the last 2 lines before the
  jump show again at the other end, as with Vim's `ctrl-f`; in a view of
  3 rows or fewer they move by 1 line. Before, they moved by the whole
  height.
- A `code`, `markdown`, `ansi` or `diff` entry, or an image's caption,
  of more than 500,000 lines shows its first 500,000, under a dim
  "first 500,000 lines shown" and above a dim "… the rest is cut after
  500,000 lines", from a file or inline; each side of a diff counts on
  its own. 8 MiB of empty lines took 828 MB in a code entry and takes
  64 MB now. `asqr validate` and `asqr ask` warn about such an entry.

### Deprecated

- The `note` field of a question. `"note": false` is still read but has
  no effect: `asqr validate` and `asqr ask` warn that it is ignored, the
  schema marks the field deprecated, and `asqr ask` drops it from the
  copy it puts into the inbox.

### Fixed

- `asqr ask` no longer replaces a waiting session whose draft asqr is
  saving at that moment: the look at the draft and the replacement run
  under a lock that asqr also takes while it saves a draft, finishes a
  session or answers an invalid one with an error. A change to a session
  whose file has left the inbox no longer saves a draft for it. When the
  lock stays taken for about 2 seconds, `ask` refuses with "the queue is
  busy", and asqr keeps the answers and says so.
- `asqr ask` replaces a waiting session whose draft holds only defaults
  the person left untouched, as after they only moved between its
  questions, and `asqr status` no longer reports such a draft under
  `draft_has_answers`. Before, moving to another question saved the
  default into the draft, and `ask` refused as if the person had
  started answering.
- A session file replaced under the same id, or a session restored from
  its draft, shows the defaults the file sets now on every question the
  person has not touched: a moved default moves the pick along with
  `◀ default`, an added one is picked, and a removed one is no longer
  picked. Before, the old default stayed picked and came back with
  `defaulted: true` while the marker stood on the new one, and a default
  added to a question that had none was not picked on a replaced file.
- `asqr ask` refusing a session the person is answering says to wait
  for its result, or to ask the person and withdraw it with
  `asqr withdraw --force <id>`. Before, it suggested another id, which
  leaves the old session in their queue.
- A session whose file changes on disk before the person has had it on
  screen, such as one waiting behind the session being answered, is
  reloaded without the message "<title> changed on disk and was
  reloaded", which would tell them nothing. Before, the message came
  along and stayed hidden under the confirmation of the session finished
  before it, and the next key cleared both.
- Every line of the question's area can be read on a small terminal. On
  the first row, `↑`/`k` scroll up through a question text that does not
  fit, and `↓`/`j` scroll back before the cursor moves; on the last row,
  `↓`/`j` reach the lines below it, such as a long note. An option whose
  label and description do not fit the area scrolls through them line by
  line before the cursor moves to the next or previous row, and moving
  up onto it shows its last lines; the line above and the line below it
  frame it, so `↑ more` and `↓ more` no longer cover the end of its first
  or last line. Any other key that acts on the question, such as `n`, a
  digit or `enter`, brings the row under the cursor back into view
  first; `?`, `L`, `i`, `o`, `z`, `s` and `q` leave the view where it
  is. Before, a long question text was cut off at its start, a tall
  option showed from its start and its later lines only once the cursor
  was on a later row, and those of the last option never.
- The character U+10EEEE, which the Kitty graphics protocol reserves for
  placing images, shows as `�` wherever text puts it on the screen, such
  as in a code or `ansi` entry, so no text can make the terminal draw an
  image's cells there. Answers keep the character as typed. Before, it
  reached the terminal as written.
- `backspace` on the picked option's row of a `single` question clears
  the pick, a default included, so the question comes back skipped,
  with its note if it has one; on any other row it does nothing. Own
  text typed before the option was picked stays in its field, unpicked,
  until `enter` on its row or an edit of it picks it again; a draft
  keeps it as `custom` with `custom_unpicked: true`, which asqr 0.13.1
  and earlier ignore. The key bar names `backspace  clear` there, where
  it has room, and the help lists it. `enter` on the option already
  picked keeps it. Before, a `single` question with a default could not
  be skipped, and one without a default could no longer be skipped once
  an option was picked.
- The review lists a `single` question's own text only where it is the
  answer. Before, it showed the text next to a chosen option, which the
  result does not send.
- Closing the help, the intro or the session list over an image under
  Sixel or iTerm2 draws the screen afresh without asking the terminal
  where its cursor is. Before, a terminal that did not answer within 2
  seconds ended asqr with an error.
- When asqr can no longer read the terminal's input, it gives the
  terminal back, names the error and exits with 1. Before, it stayed on
  screen and ignored every key.

## [0.13.1] - 2026-10-22

### Added

- While no session waits, the right end of the key bar shows asqr's
  version, such as `asqr 0.13.1`, dim, where the hints leave room, and
  the help shows it at the left of its bottom edge.

### Changed

- The general help names how far `ctrl-u` and `ctrl-d` scroll: its
  line reads `pgup/pgdn, ctrl-u/ctrl-d  scroll by page; by half a page`.
  Before, it read `pgup/pgdn  scroll the context pane, also
  ctrl-u/ctrl-d`. The key bar names `ctrl-u/ctrl-d  half page` after
  `pgup/pgdn  page` while the pane or the full-screen view has the
  focus, after `tab  context` while a pane shows without it, and after
  `pgup/pgdn  page` in the intro overlay while the intro is longer than
  the overlay. It is the first hint a narrow key bar leaves out.

## [0.13.0] - 2026-10-22

### Added

- A context entry of type `ansi` shows text in the colours and
  attributes its SGR escape sequences set, such as the output of
  `git diff --color=always` or a mockup of a terminal screen, from
  `path` or `content`. Named colours follow the terminal's theme; every
  other escape sequence is dropped. Lines do not wrap and have no line
  numbers. Under `NO_COLOR` the entry shows without any colour, RGB
  colours included. `asqr validate` and `asqr ask` warn about an `ansi`
  entry above 1 MiB, escape sequences included.

### Changed

- A context entry of a `type` asqr does not know is refused with an
  error that names the type as not supported by this asqr and its
  version, lists the supported types, and says that a newer asqr may
  support it. New entry types are added within format version 2, so an
  asqr that runs while a newer one is installed can meet one.
  `asqr validate` and `asqr ask` print it, and the dialog and the error
  result of an invalid session in the queue carry it. Before, the
  message was the parser's `unknown variant`.
- `ctrl-u` and `ctrl-d` scroll the context pane, the full-screen view
  and the intro by half a page, as they do in Vim and `less`; an odd
  height rounds down, as in Vim. Before, they scrolled a whole page, as
  `pgup` and `pgdn` still do. The help of the pane and of the
  full-screen view lists them on a line of their own.
- The result of a rejected session keeps the note to the whole session
  and the answers to the questions with an answer or a note, in the
  shape of a submitted result. An untouched default is in it with
  `"defaulted": true`. Before, the result carried only the reason, and
  the notes and answers typed before rejecting were lost.
- The context pane stands on a ledge apart from the question. Its header
  is a dim rule with the entry's name set into it in bold, `╭─ name ─────`
  beside the question and `── name ─────` below it. Beside the question
  a dim edge runs down the pane's left side to a bottom rule, and the
  content starts two columns right of it; below the question a blank row
  stands above the header. The position, such as `1–31 of 58 · tab`,
  moves from the header into the bottom rule, which runs solid without
  text when the entry fits. The focused pane fills its name and turns its
  rules yellow; before, its whole header row took the fill. The
  full-screen view shows the pane with the ledge it has beside the
  question.
- Option labels are Markdown like the intro, question texts and
  descriptions: inline code, emphasis and the rest render in the option
  row and in the review's summary. A label of more than one line moves
  the question's descriptions below the labels.
- A Markdown table that does not fit its width shows as records, one
  for each data row: each column on a line of its own, its name from
  the header row in bold and dim and then the cell's text, with a rule
  across the width between two records. The values stand beside the
  names, or, where that room would break a word, every value stands
  under its name, two columns in. A table shows as records as soon as
  a column would have to break a word, a URL included. Before, the
  word broke inside its cell, a table with more columns than the width
  holds fell apart into one letter per line, and one that did not fit
  even at one letter per column wrapped row by row behind `↪`, its
  border read as a quote bar (patine 1.8.0).
- The other dependencies are updated to their latest compatible
  versions, among them crc32fast 1.5.3 and plist 1.10.3.

### Fixed

- The dialog of an invalid session lists each context entry of type
  `text` on a line of its own, as it lists the other errors. Before,
  all such entries stood on one line, joined with `; `. The error result
  and the log still join them.
- An option label too long for its row wraps under its own start
  instead of ending at the edge of the question, where it was cut off
  without a mark. An option taller than the question's area shows from
  its start, as a long description does.
- A Markdown line that wraps behind a `↪` with one column left after
  it, such as a line nested deep in a Markdown entry, no longer runs a
  wide character past the edge of its space.
- A halfwidth sound mark such as `ﾞ` in Markdown counts as the column
  it takes on screen, and so does each letter of a ligature such as
  Lam-Alef `لا`, so a table's borders line up with its rows. Before, a
  cell of `ｶﾞ` three times put its row's right border three columns
  past the box, and a line of running text holding such text came out
  wider than the width and broke behind `↪` in the middle of a word,
  where patine now wraps it between words. A word that breaks in a
  record keeps each character whole, a flag emoji included; before, a
  flag in a table cell could be cut into its two letters
  (patine 1.8.0).

## [0.12.2] - 2026-10-19

### Changed

- asqr draws images with ratatui-image 12.0.0-rc.3. It writes the
  graphics query to the terminal and reads the answer through the
  terminal's file descriptors directly. The bytes it sends and the way
  it parses the answer stay the same.
- The other dependencies are updated to their latest compatible
  versions, among them sha2 0.11.1, serde_json 1.0.154, schemars 1.2.3
  and signal-hook 0.4.5.

## [0.12.1] - 2026-10-16

### Changed

- The own answer's mark, `(•)` or `[x]`, is bold in the primary colour
  when the typed text counts, like a chosen option's mark, and bold
  under the cursor's fill too. Before, it kept the text colour.

### Fixed

- The logo at the top of the README shows on crates.io in a dark color
  scheme as well. Its dark variant was linked relative to the
  repository, which crates.io does not rewrite in a `<source>` element.
- A diff side cut at 8 MiB between the `\r` and the `\n` of a line end
  stops before that `\r`. Before, the note on line endings named a `CR`
  end the file does not use, as `old mixed CRLF and CR`, and said the
  line endings differ between two CRLF files.

## [0.12.0] - 2026-10-16

### Added

- The empty queue shows Talqi, asqr's mascot, above its sentence, as
  asqr's terminal emblem drawn in text cells of 24-bit colour. Terminals
  that draw sextant characters themselves, such as kitty, Ghostty,
  WezTerm, iTerm2 3.6.7 and later, foot, GNOME Terminal and Konsole,
  get sextants; all others get quadrant blocks. Under `NO_COLOR`, Talqi
  is the emblem's outline without any colour. Talqi needs a terminal of
  at least 32 rows and 44 columns, so a terminal of 80×24 shows the
  sentence alone.
- The terminal title shows how many sessions wait, as
  `asqr · 2 waiting`. On quit asqr clears its title and gives the
  terminal its title from before back where the terminal keeps a title
  stack. `--no-title` leaves the title alone.
- asqr honours `NO_COLOR`: set to any value but the empty one, the TUI
  draws without colours and keeps bold, dim, reversed, the marks and
  the pointers. Images keep their colours. Before, the terminal library
  took the colours itself, with them the images, and text lost its dim
  or reversed style where its colour changed.
- The status line confirms a submitted or rejected session, as
  `✔ sent "Release 0.2.0" · 1 answered, 2 skipped` in green or
  `✗ rejected "Release 0.2.0"`, with the id for a session without a
  title. The line stays until the next key or paste, also over the next
  session and over the empty queue. A name too long for the line is cut
  and ends in `…`, so the counts and the closing quote show whole.
- Markdown shows a task list item with `☐` for an open task and `☑` for
  a done one, in place of the bullet. A done task is dim, apart from the
  items nested under it. Table columns follow the alignment of their
  delimiter row (`:--`, `:-:`, `--:`). Both come with patine 1.7.0.
- A long word in a task item wraps under the item's text, as in any
  other list item.
- An invalid session opens a dialog framed in yellow that lists each of
  its errors on a line of its own; the next key closes it and does
  nothing else. Under the message of a terminal too small for asqr,
  which hides the dialog, a key acts as without it, and the dialog
  shows once the terminal has room. Before, the errors showed only in
  the status line, joined with `; ` and cut at its edge.

### Changed

- Context entries load apart from drawing the screen, so keys keep
  working while a large file is read and highlighted or an image is
  decoded and scaled. An entry that is not ready after 100 ms shows a
  dim `loading…` in the middle of the pane until it is, and while keys
  wait to be handled it shows at once, so holding `j` does not stop on
  each slow entry. Before, the screen froze until the entry was ready:
  23 seconds for the first 8 MiB of a log.
- While a question shows, asqr loads the entries of its other options,
  of the next question and of the one before ahead, and keeps what it
  loaded for those three questions only. Coming back to an entry
  outside them loads it again.
- A code entry longer than 1 MiB shows without syntax highlighting.
  In 0.11.0, highlighting the first 8 MiB of a log took 23 seconds.
- An image entry holds its file as asqr read it instead of its decoded
  pixels, and decodes it again only for another size of the image in
  cells. A shown 6000×6000 PNG
  held 144 MB of pixels before. An image file larger than 64 MiB is
  not read and shows a card that says so.
- A diff marks no words inside a changed line: the old version shows
  whole in red and the new one whole in green. Before, the changed
  words of a changed line showed reversed.
- The note of a diff whose sides end their lines differently names the
  ends of both, as `line endings differ: old CRLF, new LF`, and a side
  with ends of several kinds as `mixed CRLF and LF`.
- The TUI takes its colours from the roles of the design guide instead
  of reversing nearly everything. The row under the cursor, a field's
  head under it, the current entry of the session list and the focused
  context pane's header show a yellow fill, one at a time: while the
  context pane has the focus, the question's current row keeps only its
  pointer. The title line and the key bar lose their fills: `asqr` and
  the keys are bold cyan, the queue, the count of waiting sessions and
  the key descriptions dim. The
  current tab is bold cyan over a heavy cyan line, unanswered `☐` marks
  are dim, chosen option marks bold cyan and option numbers dim.
  `[ Submit ]` is always filled in cyan, with a yellow pointer when the
  cursor is on it. The help, the intro and the session list have rounded
  cyan frames with a blank column inside, which moves the help's keys
  and the session list's rows one column in.
- The status line colours each notice and message by its kind instead
  of showing all of them in yellow, an error after `✗` and a warning
  after `!`. Failures, such as a draft that
  cannot be saved or an inbox that cannot be read, are red. Refused keys
  and pastes and sessions set aside, such as an invalid session answered
  with an error, stay yellow. News, such as a session file reloaded
  after it changed, is in the text colour.
- A context entry asqr cannot show draws a card with a red rounded
  frame in the context pane: `✗` and the reason in bold red, then the
  file name and its folder, the home folder as `~`, wrapped after a `/`
  and never shortened. A card taller than the pane scrolls. A diff's
  card names the side that failed, and the pane's header shows a red `✗`
  before the entry's name. Before, a dim heading such as "code not
  shown" stood above the path and the reason.
- Inside tmux 3.4 or later, in kitty and Ghostty, asqr sets
  `allow-passthrough all` for its own pane while it runs and puts the
  pane's setting back when it quits. An image that arrives while asqr's
  window is in the background shows when you switch to it, and the
  deletes of its images reach the terminal from a hidden pane.
- Inside tmux, asqr asks the terminal for its graphics protocol only
  when a client shows asqr's pane; a pane in a background window, or
  one hidden by another pane's zoom, gets images as blocks at once.
  Before, asqr waited there for an answer tmux did not pass on.
- Images scaled to their cells mix the pixels they cover, so thin
  lines and small text stay visible. Before, each pixel took the
  nearest one and dropped the others.
- An image the terminal's protocol fails to encode stays empty, and the
  log says why, once. Before, nothing said why, and asqr encoded it
  again on every frame.
- asqr waits up to 5 seconds for the terminal's answer to the graphics
  query, up from 2. Only a terminal that does not answer waits that
  long for the first frame.
- A terminal that does not answer the graphics query at all gets its
  images as blocks, also where its environment names a terminal with
  the iTerm2 protocol. Before, asqr used the iTerm2 protocol there when
  the terminal reported its size in pixels. This follows ratatui-image
  12.0.0-rc.2, which asqr now draws images with.
- Closing the help, the intro or the session list over an image redraws
  the whole screen only under Sixel and iTerm2, which need it. Half
  blocks and the Kitty protocol redraw only what changed.
- Regaining the focus reads and sends the images again only under the
  Kitty protocol behind tmux with `allow-passthrough on`, or behind a
  tmux asqr cannot ask, as over ssh, where tmux drops a transfer while
  asqr's pane is hidden. Elsewhere it no longer reads every image again.
- Over ssh from a tmux pane, asqr asks the terminal for its graphics
  protocol straight away, without first sending a status request
  through tmux's passthrough and waiting up to 300 ms for its answer.
  Where that tmux passes nothing on, the start waits the query's 5
  seconds before it shows images as blocks.
- asqr discards input after the graphics query only when the query got
  no answer in time, or inside tmux with several clients attached.
  Before, it also did so after a quick answer that named no graphics
  protocol, and a key typed in the first moments after the start was
  lost.
- The session's title moves into the title line next to `asqr`, with the
  queue and the count dim at its right end, as `default · 2 waiting`.
  `from` and `follows` share one dim line under it, and the intro hangs
  under the title in the text colour instead of dim, in at most two
  lines instead of three, with a blank line before the tabs. On a low
  terminal the header gives up that blank line, then intro lines, before
  the context pane shrinks to its one-line hint.
- `follows` in the header shows the title of the session it continues,
  as `follows "Review the retry change"`, taken from the waiting
  sessions or the queue's archive. Where asqr finds no such session, or
  it has no title, the header shows the id as before.
- A quote in Markdown shows its text in the style of the text around
  it, so emphasis shows inside the quote, behind a blue bar that is dim
  under `NO_COLOR`. Before, quoted text was dim and italic behind a dim
  bar.
- The agent skill names task lists among the Markdown asqr renders.
- A code entry of a log is highlighted in about a fifth of the time
  (patine 1.7.1).

### Fixed

- A Markdown table with a word wider than its column, such as a long
  URL, fits the pane: the word continues on the next line of its cell,
  a link's URL dim on each line, and words that fit stay whole. Before,
  such a word widened the table past the pane, every row wrapped behind
  `↪`, and the wrapped rows started with the table's border; since
  patine 1.7.1 the URL of any link in a cell did this. Running text
  still wraps a long word behind `↪`, and a table whose columns do not
  fit even at one character each wraps row by row (patine 1.7.2).
- Markdown table cells keep their inline styles: bold, italic,
  strikethrough, inline code and links show as in running text, and a
  header cell adds bold on top. A link shows its dimmed `(url)`, so a
  column of links is wider, and an image shows `[image: alt]` with its
  path, where its cell was empty. Cell text wraps like running text,
  apart from a word wider than its column (see above), and a run of
  spaces or inline HTML that is not drawn no longer
  widens a column. Before, table cells showed as plain text (patine
  1.7.1).
- In a Markdown list item, a list after the text of a quote gets the
  blank line, with the quote's bar, that it gets in any other quote,
  and a paragraph in an item that starts with an empty code block
  follows the bullet on the next line without a blank line (patine
  1.7.1).
- A code block or a nested list inside a Markdown quote sits inside the
  quote's bar, as `│   code`, where it showed as `  │ code` (patine
  1.7.0).
- On an 80×24 terminal, a session with `from`, `follows` and a long
  intro showed its context pane only as a one-line hint, as `follows`
  pushed the title onto a second line.
- The notes around a Markdown file cut at 8 MiB are cut at the pane's
  edge, as those of code and diffs are. Before, they wrapped onto
  further rows in a pane narrower than the note.
- A question with its context pane below shows its last line while the
  cursor is on its own answer and the own answer's label ends at the
  right edge. Before, the highlighted label pushed the answer's text one
  column right, which could take a row more than the question had room
  for, and the last line was cut.
- In kitty and Ghostty, asqr asks the terminal to delete an image of a
  context entry once the screen no longer shows it: after sending it
  again on regaining the focus, when its entry is replaced, when its
  session leaves, and on quitting, also when asqr panics. Inside tmux,
  regaining the focus writes the deletes again, as a hidden pane drops
  them. Before, every copy stayed in the terminal, in Ghostty up to its
  limit of 320 MB.
- On Unix, SIGTERM, SIGINT and SIGHUP quit asqr as `q` does, with exit
  code 0: it deletes its images, gives the title back, turns focus
  reports and bracketed paste off and leaves raw mode and the alternate
  screen. Before, a signal such as `kill <pid>` ended asqr at once and
  left the terminal as asqr had set it up. SIGKILL still does.
- When asqr panics, it gives the title back before it leaves the
  alternate screen, as on quitting. Before, it gave the title back on
  the main screen, after the alternate screen was left.
- In kitty and Ghostty, an image whose pane changes size is sent under
  a new id, and its old copy is deleted. Before, it was sent again
  under the same id, and Ghostty showed it blank while the new copy
  came in.
- In Ghostty under tmux, the help, the intro and the session list hide
  only the part of an image they cover. Before, the image vanished
  across its full width in every row an overlay covered. An image first
  sent while an overlay covered its top left corner, as after a resize,
  shows beside the overlay and whole once it closes; before, it stayed
  blank until it was sent again.
- After a graphics query the terminal left unanswered, the first key
  or paste reaches asqr. Before, the query went on reading and took it,
  and a paste lost its start marker, so its line breaks arrived as
  `enter`.
- An answer to the graphics query that arrives after asqr stopped
  waiting for it, and the further answers inside tmux with several
  clients attached, no longer reach asqr as keys. Before, their digits
  could pick an option.
- A context file swapped for a FIFO between asqr's check of its path
  and its opening shows the card of a path that is not a regular file.
  Before, asqr waited until something wrote to the FIFO.

## [0.11.0] - 2026-10-15

### Added

- Questions and options take `context`: material shown next to the
  question that helps to answer it. An entry is an image, code (text
  shown as written and highlighted, such as source, a log or a
  configuration file), Markdown (rendered) or a diff, which asqr
  computes from the old and the new text. Code and Markdown come from a
  file or inline in `content`. A question or option holds one entry.
  `"context": null` counts as no context, as `null` does for every
  other optional field.
  asqr reads only regular files, so a directory or a FIFO shows a
  placeholder instead of stalling the screen, and of a text file at
  most its first 8 MiB, under a dim "first 8 MiB shown" and above a dim
  "… the rest is cut after 8 MiB".
- A code entry shows highlighted in its language, through the Markdown
  renderer patine and the 8 colours of the terminal's palette, with line
  numbers in a dim gutter. The language comes from `language`,
  else from the extension or file name of `path` or `title`. Long lines
  do not wrap; the pane scrolls sideways, a `›` marks a line cut at the
  right edge and a `‹` a line scrolled past the start of its text.
- A Markdown entry shows rendered in patine's style and wrapped to the
  pane: headings, emphasis, inline code, lists, quotes, tables and
  highlighted code blocks. A line patine leaves wider than the pane,
  such as a long line of code, wraps, and each further row starts with a
  dim `↪`, indented like the line's quote bars, code block or list item
  text.
- A diff entry shows as a unified diff: hunks with 3 lines of context, a
  separator between hunks, the old and the new line number in a dim
  gutter, removed lines red and added lines green, and the changed words
  of a changed line marked. A diff without old text is a new file, one
  without new text a deleted one. Long lines do not wrap; the pane
  scrolls sideways, and `›` and `‹` mark cut lines as in a code entry.
  A wide character that would reach under a marker shows as blanks, so
  the marker stays visible. Lines compare without their line ends, and
  a dim "line endings differ" says when the two sides use different
  ones, such as `\r\n` and `\n`. Tabs expand to stops of 4 columns, and
  other control characters show as their pictures, such as `␛`. The
  agent skill describes what to put into `old`, `new`, `name` and
  `title`.
- An image entry takes `"enlarge": false` to keep the image at its
  natural size where it fits, in the pane and full screen, instead of
  scaling it up to fill the space. A larger image still shrinks. The
  agent skill tells agents to use it only where the exact pixel size
  matters.
- `asqr validate` and `asqr ask` warn about a relative or missing file
  in any context entry, and about an `image` field left over from format
  version 1, saying that an image is a context entry of type `image`.
  An entry of type `text` is an error that names `code` and `markdown`.
  A diff with neither `old` nor `new`, or with `path` or `content` on
  the entry itself, is an error that says what a diff takes: the text
  before and after the change, which asqr diffs itself.
- `asqr validate` and `asqr ask` warn about a code, Markdown or diff
  entry above 1 MiB. asqr still shows it.
- `asqr validate` and `asqr ask` warn when a side of a diff looks like a
  unified diff, which asqr would diff like any other text, and say to
  give the text before and after the change as `old` and `new`.
- `asqr validate` and `asqr ask` warn "not a file" about a context path
  that names a directory or another file that is not a regular file.
- The JSON Schemas state the format version as the constant `2`, allow
  one context entry per question or option, require exactly one of
  `path` and `content` in a code or Markdown entry and in each side of a
  diff, and require `old`, `new` or both in a diff. They describe
  `intro`, question `text` and option `description` as Markdown in
  which a single newline breaks the line and raw HTML shows as written.
- A note to the whole session: the review tab has a field for it below
  Submit, and `enter` in it submits. A submitted result carries it as a
  top-level `note`, the first new field of result format version 2. A
  rejected result never has one. The draft keeps the note, and a draft
  with a note counts as started, so `asqr ask` refuses to replace it.
- The agent skill tells agents to read the session note.
- An example session with diff, Markdown and code entries in
  `examples/sessions/retry-review.json`.
- `i` shows the session's whole intro in a frame of its own, which
  scrolls with `↑`/`↓`, `j`/`k`, `pgup`/`pgdn`, `ctrl-u`/`ctrl-d`,
  `g`/`G` and `home`/`end` when the intro is longer than the screen.
  `esc`, `q` and `i` close it.
- `tab` moves into the context pane, which then scrolls with `↑`/`↓`,
  `j`/`k`, `←`/`→`, `h`/`l`, `g`/`G`, `home`/`end`, `pgup`/`pgdn` and
  `ctrl-u`/`ctrl-d`; `tab`, `esc` or `q` move back to the question or
  the field being typed in, so a word typed after `tab` never quits.
  `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` scroll the pane from anywhere,
  also while typing in a field. Each entry keeps its
  scroll position while the person moves between options and
  questions. The key bar and the help show the pane's keys while it has
  the focus.
- Pasting into a field or a note inserts the text as a whole, through
  the terminal's bracketed paste. A line break in it no longer acts as
  `enter`, which picked an option, submitted the session from its note
  or rejected it. The pasted line breaks stay in the text, and `\r\n`
  or a lone `\r` arrive as `\n`. A tab becomes blanks up to the next
  multiple of 4 columns and other control characters are dropped, in a
  paste and in text restored from a draft, as a field shows neither. A
  paste on a field left with `esc` gives it the keys again and goes in;
  elsewhere a message says that a paste goes into a field. asqr turns
  bracketed paste off again when it quits or panics. A terminal that
  refuses bracketed paste, as a legacy Windows console does, runs asqr
  without it.
- `--layout auto|beside|below` and `ASQR_LAYOUT` choose where a
  question's context goes. `auto`, the default, puts it beside the
  question when the terminal's area looks landscape and below it when
  it looks portrait, and keeps what it shows while a resize stays near
  a square shape.

### Changed

- **Breaking:** session and result files use format version 2
  (`"asqr": 2`). asqr refuses a version 1 session with a message that
  points to `asqr skill`, which describes the new format. A version 1
  session still waiting in a queue gets an error result with that
  message. `wait` and `result` still print a version 1 result that is
  already in the outbox.
- `asqr ask` makes every path in a context entry absolute, as it did
  for images.
- `asqr new` writes format version 2 with a context entry, and without
  an id, which `asqr ask` assigns, as the agent skill says.
- An `asqr` version that is no unsigned integer, such as `2.0` or
  `"2"`, is refused as an unsupported format version with the value as
  written, instead of a parser error about its type.
- The agent skill describes format version 2 and context entries.
- A long own answer and a long reject reason wrap onto further lines
  under their start, like a note, instead of scrolling sideways while
  typed and being cut with `…` otherwise. The counter of a focused own
  answer moves to a line of its own after the text.
- The question and the review keep to a reading width of 100 columns
  on wide terminals. Beside the question, context takes the rest of the
  width from a terminal of 103 columns on; below it, context takes at
  least 8 rows and the question at most half the height. Below, the
  question takes the rows the tallest question with context in the
  session needs, so the context stays in place between questions, and
  typing moves it only when that question gains a line. Where neither
  fits, context shrinks to a line that names it, and `z` still shows an
  image full screen. A dim `↑ more` or `↓ more` on the first or last row
  of the question and the review says that rows are cut off there; it
  never covers the text cursor.
- Context shows in a pane whose first line names the entry by its
  title or file name and, when the entry is taller than the pane, says
  which of its lines show. While the pane does not have the focus and
  its entry is taller or wider than the pane, the header also names
  `tab`, as in `1–10 of 31 · tab`. Beside or below the question, an
  image gets one row less for that line, and an image smaller than the
  pane grows to fill it.
- `z` shows any context entry full screen, not only an image, and the
  one-line hint on small terminals offers it for every entry, as
  `esc, z: full screen` while a field or a note has the keys. The
  full-screen view takes every key, as the focused pane does: it
  scrolls with the pane's keys, the question's keys no longer act
  behind it, and `z`, `esc`, `tab` or `q` go back to where the focus
  was. Its key bar shows the pane's keys, and its help lists them with
  `z/tab/esc/q` to leave the full screen. A resize that passes through
  a size too small for asqr keeps the full-screen view and the pane's
  focus.
- `o` on a code, Markdown or diff entry, or where no entry shows, says
  why it opens nothing.
- `tab` and `shift-tab` no longer type a tab character into a field.
  While a context pane shows, `ctrl-u` and `ctrl-d` scroll it instead of
  undoing and deleting in a field.
- Every typed field takes several lines: the own answer, the session
  note and the reject reason now accept `ctrl-j` like a `text` answer
  and a question's note, and `enter` still picks, submits or rejects.
  In results, `custom`, the session `note` and `reason` may contain
  `\n`; the format stays version 2. The review shows a line
  break in an own answer as `⏎`. The key bars of these fields show
  `ctrl-j new line`, and the agent skill says that these fields may
  span several lines.
- `↑`/`↓` in a typed field move the text cursor through the rows the
  field shows, wrapped rows included, and leave the field only from its
  first or last row; `ctrl-p`/`ctrl-n` do the same. Moving down onto a
  field puts the text cursor at the end of its first row, moving up
  onto it at the end of its text. Before, they left the own answer, the
  `text` answer, the session note and the reject reason at once, so the
  cursor could not reach an earlier line of a `text` answer, and in a
  question's note they moved by the text's lines. The key bars of these
  fields drop their `↑/↓ leave` and `↑ back` hints, and the help states
  the rule.
- The key bar leaves out hints that do not fit whole instead of cutting
  the last one, and keeps `? help` and `q quit` longest.
- The help puts `space` and `1`-`9` on one line and `n` and `ctrl-j` on
  another, to make room for the context pane's keys.
- The agent skill explains that context shows in a pane of its own that
  the person scrolls and can show full screen, so long material belongs
  there rather than in the question text.
- The session title and intro wrap at the reading width instead of
  running to the edge. The title takes at most two lines, one on
  terminals under 24 rows. A cut intro ends in `… i: more`, or in
  `… esc, i: more` while a field, a note or the context pane has the
  keys.
- The agent skill says that a cut intro can be read with `i`.
- The agent skill explains that the intro shows in at most three lines
  and only identifies the session, and that everything needed to answer
  belongs in the question text.
- The intro, question texts and option descriptions render full
  Markdown in patine's style, as a Markdown entry does: headings,
  emphasis, inline code in yellow instead of reversed, lists, quotes,
  links, tables, rules and highlighted code blocks. Before, everything
  but bold, italic, inline code, lists and line breaks showed as its
  source. A line wider than its space, such as a long line of code,
  wraps, and each further row starts with a dim `↪`. A single newline
  still breaks the line, and raw HTML, such as `Vec<String>`, still
  shows as written.
- The agent skill says that the intro, question texts and option
  descriptions take full Markdown, that a single newline breaks the line
  and HTML shows as written there, that option descriptions should stay
  short, and that long code belongs in a `code` entry.
- The agent skill says that a Markdown entry follows CommonMark instead:
  a single newline joins the lines, a blank line starts a paragraph, a
  trailing backslash breaks the line, and raw HTML is left out, so `<`
  goes into code spans.
- A session file of the wrong shape names the field in its error, as
  in `not a session file: questions[1].context: invalid type: map,
  expected a sequence`. Before, the message named no question and no
  field. A `context` that is no list adds `context is a list with one
  entry: wrap the entry in [ ]`.

### Removed

- **Breaking:** the `image` field of questions and options. An image is
  a context entry: `"context": [{"type": "image", "path": ...}]`.
- The JSON Schemas of format version 1, `schema/session.v1.json` and
  `schema/result.v1.json`. `asqr schema` prints the version 2 schemas,
  which ship as `schema/session.v2.json` and `schema/result.v2.json`.

### Fixed

- When the help, the intro or the session list closes while an image
  shows, asqr clears the terminal and draws the whole screen again.
  With Sixel and the iTerm2 protocol, ratatui never rewrites the cells
  an image covers, so text of the overlay could stay on the image.
- The warnings of `asqr ask` and `asqr validate` name a missing context
  file by its absolute path when the session file was given by its bare
  name. Before, they printed the path as the session file wrote it.
- A session file replaced while asqr shows it keeps the question or
  review shown, the row, a field's text cursor, whether the field had
  the keys, an open note, the context pane's focus, the full-screen view
  and the scroll positions of the entries that did not change, and a
  message says that the file was reloaded. Before, the session started
  over at its first question without a word. An open intro overlay
  closes when the new file has no intro, where it stayed open with
  nothing in it and took every key.
- Removing a session while the session list was open on the last one
  left the cursor past the end, and `enter` showed "Nothing to answer"
  while sessions waited. The cursor stays on a session, and the list
  closes when none is left.
- `asqr ask` no longer adds `"options": null` to a `text` question in
  the copy it drops into the inbox.
- `asqr validate` and `asqr ask` refuse a session file that is no JSON
  object as not a session file (exit code 11). Before, a session written
  as an array of its fields in their order passed validation, and
  `asqr ask` crashed on it.
- The text cursor of a note or a `text` answer stays visible when the
  text fills its line exactly or the cursor sits on the space where a
  line breaks. Before, the cursor's cell fell past the end of the line
  and could not be seen.
- A word that changes its style midway, such as the word under the text
  cursor or a word with Markdown emphasis in only part of it, wraps as
  one word. Before, it could be split at the style change. A word that
  ends exactly at the end of a line also stays on that line.
- Text with wide characters, such as Chinese, Japanese or Korean or
  most emoji, keeps to the screen's width. Each such character takes two
  columns, but asqr counted it as one, so a wrapped own answer, note,
  session note, reject reason or `text` answer ran past the edge, and a
  cut title or intro lost its `…` or `… i: more`. The same held for the
  tab bar, the review's labels and answers, the description column of
  options, the context pane's header and the queue path in the title
  bar. A wide character that does not fit at the end of a line now
  starts the next one. The text cursor shows on the whole character it
  sits on, an emoji with its skin tone included. In the context pane, a
  halfwidth sound mark such as `ﾞ` takes a column of its own, as
  terminals draw it.
- The `—` of an empty `text` answer and an empty note is dim, as it is
  in an empty own answer. Before, it showed in full brightness.
- The session schema requires at least one question, as validation
  does. Before, it accepted `"questions": []`, which `asqr validate`
  refuses.
- When asqr panics, it turns the terminal's focus reports off before
  it ends. Before, they stayed on, and the shell received them as typed
  text whenever its window gained or lost the focus. A signal that ends
  asqr, such as SIGTERM, still leaves them on. A terminal that refuses
  focus reports runs asqr without them. Before, asqr ended with an error
  and left the terminal in raw mode on its alternate screen.
- Run over ssh from a tmux pane without passthrough, asqr no longer
  asks the terminal for its graphics protocol and shows images as
  blocks. The question got no answer there, and the thread waiting for
  it took the first input that came. A first paste then lost its start
  marker, and its line breaks arrived as `enter`, which could pick an
  option or submit the session. Where `TERM` starts with `tmux`, or
  `TERM_PROGRAM` is `tmux`, without `TMUX`, asqr first sends a status
  request through tmux's passthrough and asks for the graphics protocol
  only when the answer comes within 300 ms, so tmux with passthrough on
  still shows images inline.
- Inside tmux, asqr gives its pane's `allow-passthrough` setting back
  once it has detected the graphics protocol, and unsets it when the
  pane had no setting of its own. Before, ratatui-image set it to `on`
  on every start, and it stayed on after asqr ended.
- An own answer of nothing but blanks or line breaks no longer shows
  as chosen. The tab bar, the review and the result already treated it
  as no answer.
- In a field, `pgup`/`pgdn`, `ctrl-v` and `alt-v` do nothing without a
  context pane. Before, they moved the text cursor to the start of the
  text.
- `shift` with a move in a field only moves the text cursor. Before, it
  selected text that asqr did not show, and the next typed key, paste
  or delete replaced it.
- A field or a note taller than the question's area keeps its text
  cursor in view, and on the text's last line the length counter below
  it too. Before, the question showed the field's start, and the cursor
  of a long `text` answer could stay below the screen while the person
  typed.

## [0.10.1] - 2026-10-14

### Added

- Every GitHub release carries prebuilt binaries for macOS (Apple
  silicon and Intel) and Linux (x86_64 and aarch64, statically linked).

## [0.10.0] - 2026-10-06

### Added

- `asqr status --since <age>` lists only sessions answered within that
  age, such as `2h`. `--limit <count>` sets how many answered sessions
  show, and `--all` lists every one.
- `asqr status --json` reports `answered_total`, the number of answered
  sessions before the limit cuts the list.

### Changed

- `asqr status` lists the 10 most recently answered sessions, newest
  first, instead of every result in the outbox in id order. The outbox
  keeps results until they are pruned, so the full list kept growing
  and cost an agent tokens on every call. Waiting sessions are still
  all listed.
- The agent skill explains the limit, `answered_total` and how to find
  a lost session with `--since`.

## [0.9.2] - 2026-10-03

### Fixed

- An option whose `description` is empty or only spaces shows again.
  Before, its whole row disappeared in wide terminals, and the person
  could not pick it.
- A paragraph after a quote, heading, code block, HTML block or
  horizontal rule starts on its own line after a blank line. Before, it
  was glued onto the last line of that element.
- Images in terminals with the Kitty protocol are no longer cropped or
  drawn too small when the cell size differs from the one at start:
  after a font size change, or in tmux with clients of different cell
  sizes attached.

### Changed

- The agent skill says that Markdown outside the supported subset, such
  as quotes, headings, links and code blocks, shows as literal source,
  and how to set a line apart instead.
- The log records the detected graphics protocol and cell size at start,
  and the terminal's size on every resize.

## [0.9.1] - 2026-09-29

### Added

- `asqr validate` and `asqr ask` warn about an image asqr cannot show,
  such as SVG or AVIF, and list the formats that work. Before, the
  person saw a placeholder and the asker never heard about it.
- The agent skill and the `image` field of the session schema name the
  supported image formats: PNG, JPEG, GIF (first frame only), WebP,
  BMP, TIFF, ICO, TGA, PNM, QOI, DDS, OpenEXR, HDR and farbfeld.

## [0.9.0] - 2026-09-28

The first release.

### Added

- A terminal UI (`asqr`, or `asqr watch`) that shows the sessions
  waiting in a queue and lets you answer them with the keyboard: a tab
  per question, a review tab to submit or reject the whole session, and
  a draft that keeps your answers when you quit.
- Question kinds: single choice, multiple choice with optional `min`
  and `max`, and typed answers. Choice questions can take your own
  answer instead of the given options, and a question takes a note
  unless the asker turns that off. Every question is optional.
- Option descriptions, preselected defaults, and length hints with a
  counter for typed answers. The intro, question texts and option
  descriptions support bold, italic, inline code, lists and line
  breaks.
- Images next to a question, or one per option that shows while the
  cursor is on it, drawn inline in terminals with the Kitty, iTerm2 or
  Sixel protocol and as blocks elsewhere. `o` opens an image in the
  system viewer, `z` shows it full screen.
- A desktop notification and the terminal bell when a session arrives
  (`--no-notify`, `--no-bell`), passed through tmux when it allows it.
- Commands for askers: `new`, `validate`, `ask`, `wait`, `result`,
  `status`, `prune`, `paths` and `schema`. `wait` and `result` exit
  with 0 for a submitted session, 10 for a rejected one, 11 for an
  invalid file, 12 while no result is there yet and 13 when none will
  come.
- Named queues (`--queue`, `ASQR_QUEUE`) and queues in any directory
  (`--dir`).
- Version 1 of the session and result format, with JSON Schemas in
  `schema/`.
- An agent skill built into the binary: `asqr skill` prints it,
  `asqr skill --install <dir>` installs it.
