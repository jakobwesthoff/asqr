Part of the asqr skill. Read SKILL.md first, which holds the rules for every session; `asqr skill` prints it.

# asqr: context entries

This part holds:

- the entry types `image`, `code`, `markdown`, `ansi` and `diff`
- how asqr shows a diff, a code entry and a Markdown entry
- the size limits of an entry
- how long a file an entry shows must exist
- the `image` field questions and options no longer have

Every entry has a `type` and an optional `title`:

- `{ "type": "image", "path": "..." }`: an image file. asqr shows PNG,
  JPEG, GIF (first frame only), WebP, BMP, TIFF, ICO, TGA, PNM, QOI,
  DDS, OpenEXR, HDR and farbfeld. It cannot show SVG or AVIF, so
  convert those to PNG first. The person only sees an error card with
  the reason for an image asqr cannot show, and `validate` and `ask`
  warn about it. asqr scales an image up to fill the pane, apart from
  the rows a caption takes, which suits most images: the person sees
  them larger when the space is there. Add `"enlarge": false` only when
  the exact pixel size matters, such as pixel art, a screenshot whose
  size is the point, or an icon at its real size; the image then keeps
  its natural size and only shrinks when it does not fit.
  Add `"caption"` with one short line of Markdown, two at most, that
  says only what matters about the image, such as `"caption": "Draft
  3: warmer palette, thicker outline"`. The person sees it under the
  image, which takes the rows the caption leaves, so a longer caption
  makes the image smaller, and one that leaves it too little room
  hides it from the pane until the person presses `z`. Do not put a
  prompt paragraph or a list into a caption: longer material, such as
  the full prompt or a list of notes, goes into the question's `text`
  or another context entry. `z` shows the image alone where the pane
  shows. A caption follows the rules of a Markdown entry (a single
  newline joins the lines). Other entry types ignore a caption, and
  `validate` and `ask` warn about it as an unknown field.
- `{ "type": "code", "path": "..." }` or `{ "type": "code", "content":
  "..." }`: text shown exactly as written and highlighted, like a
  fenced code block: source code, a log, terminal output, a
  configuration file. `language`, such as `rust`, picks the
  highlighting; without it the extension of `path`, then of `title`,
  decides. A Markdown file as `code` shows its source, which is what
  you want when the Markdown itself is under review.
- `{ "type": "markdown", "path": "..." }` or `{ "type": "markdown",
  "content": "..." }`: Markdown the person reads rendered, such as
  notes, a plan or a draft section of documentation. A Markdown entry
  follows CommonMark, unlike the question text: a single newline joins
  the lines (a blank line starts a paragraph, a trailing backslash
  breaks the line), and raw HTML is left out, so put `<` into code
  spans, such as `` `Vec<String>` ``.
- `{ "type": "ansi", "path": "..." }` or `{ "type": "ansi",
  "content": "..." }`: text in the colours its own escape sequences
  set, such as the output of a command run with its colours or a
  mockup of a terminal screen. asqr shows the colours, bold, dim,
  italic, underline, reversed and crossed out that SGR sequences
  (`ESC [ … m`) set, and drops every other escape sequence. Cursor
  movement is dropped too: the output of a full-screen program, which
  places its text by moving the cursor, does not show as on its
  screen; capture a command that prints line by line. Ask your tool
  for its colours, as `git diff --color=always` does. In JSON, write
  the escape character as `\u001b`, as in
  `"\u001b[31mfailed\u001b[0m"`; a file at `path` holds the raw bytes.
  The named colours and the first 16 of the 256 (`30` to `37`, `90` to
  `97`, `38;5;0` to `38;5;15`, and their backgrounds) follow the
  person's terminal theme; colours 16 to 255 and RGB colours show as
  given, so draw a mockup in the named colours. Under `NO_COLOR` the
  person sees the text without any colour. Long lines do not wrap, and
  there are no line numbers.
- Choose by what the person should see: the text as written is
  `code`, rendered prose is `markdown`, text in the colours you or your
  tool chose is `ansi`. None of them is the place for your explanation
  of the question; that goes into the question's `text`.
- Inline text goes into `content` in every entry, the sides of a diff
  included. The key `text` belongs to questions only.
- asqr shows U+10EEEE, the character the Kitty graphics protocol uses
  to place images, as `�` in every text, so it cannot place an image
  on the person's screen.
- `{ "type": "diff", "old": {...}, "new": {...} }`: a change, which asqr
  diffs itself. `old` is the text before the change and `new` the text
  after it, each `{ "path": "..." }` or `{ "content": "..." }`: two
  versions of the same text, never a unified diff of your own, which
  asqr would show as a changed file of `+` and `-` lines. `validate`
  and `ask` warn that a side "looks like a unified diff" when it holds
  one; give the two versions instead. The two
  versions can be whole files or the same excerpt of both; the line
  numbers then count from the excerpt's first line. Leave out
  `old` for a new file and `new` for a deleted one, never both: a
  diff needs at least one side. `name` is the
  file's name, such as `src/fetch.rs`, shown in the header when there
  is no `title`. Its extension picks the language the unchanged lines
  around the change are highlighted in; the removed and added lines
  show on a red and a green fill. `language`, such as `tsx`, overrides
  the language `name` gives: give it when `name` does not say the
  language you want, such as a `.txt` name whose text is Rust. Without
  `language`, the extension of `name` decides, or the whole name when
  it has none, as `Makefile` and `Dockerfile` do. Show a suggested
  change as a diff, not as a code block of the new version: only the
  diff shows what the change replaces. A new file shows as a diff of
  added lines only, without syntax colours; when the person should
  read new code highlighted, give it as a `code` entry. Give a `title`
  that says what the change does, such as "Retry with backoff".
- asqr shows a diff unified: only the changed lines with 3 unchanged
  lines around each change, a separator between such hunks, the old
  and the new line number of every line, removed lines in a red fill
  and added lines in a green fill across the pane, and the unchanged
  lines highlighted in the diff's language. The person can switch a
  diff to side by side, the old version on the left and the new one on
  the right with each removed line next to an added one; each side
  keeps its own line numbers, removed and added lines keep their red
  and green fill within their column, and unchanged lines their
  highlighting. A diff whose two versions
  together exceed 1 MiB shows without highlighting, and highlighting
  stops after line 50,000 of the new version. A long line does
  not wrap; the person scrolls sideways. Identical versions show "no
  changes".
- asqr shows a code entry highlighted, with line numbers in a gutter;
  highlighting stops after line 50,000, and the lines after it show plain
  below a dim note. Long lines do not wrap; the person scrolls sideways. A Markdown entry
  is rendered and wrapped to the pane: headings, emphasis, lists, task
  lists, quotes, tables and highlighted code blocks. A table too wide
  for the pane, or one that would have to break a word to fit, shows as
  records: a block of lines for each row, each column's name before its
  value. A long line of a code block wraps behind a `↪` marker. Name the
  language of a code block's fence so it is highlighted. A file asqr
  cannot read shows as an error card with the reason.
- `intro`, a question's `text` and an option's `label` and
  `description` show tables the same way: one that does not fit the
  space, or would have to break a word in it, shows as records. A line
  wider than the space there, such as a long line of code, wraps behind
  a `↪` marker.
- Keep a code, Markdown, ANSI or diff entry, and an image's caption,
  under 1 MiB (both sides of a diff together, escape sequences
  included); `validate` and `ask` warn above that. A Markdown entry or
  caption above 1 MiB is not rendered: asqr shows it as plain text.
  asqr shows at most the first 8 MiB of a file and the first 500,000
  lines of each text, each side of a diff on its own, with a note where
  it cuts, and `validate` and `ask` warn about more lines. Show an excerpt instead
  of a whole large file.
- The files an entry shows by path must exist until `asqr wait` returns.
  asqr reads them when the person finishes the session and copies them
  into its archive, so a temporary file may go after that. A file that
  is gone by then is recorded as gone.
- Questions and options have no `image` field. A file that still uses
  one gets a warning, and the image does not show.

End of references/context.md.
