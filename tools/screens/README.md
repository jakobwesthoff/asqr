# screens

A development tool of the asqr repository, not published: it draws
asqr's screens with asqr's own code and renders them into PNG pictures
for the documentation
([ADR 89](../../docs/adr/0089-draw-the-documentation-pictures-with-asqrs-own-rendering.md)).

Each screen is drawn through asqr's public API: an `App` with the scene's
sessions, keys through `App::handle`, and `asqr::tui::render::draw` into
a ratatui `TestBackend`. The cell buffer is rasterized straight into PNG
in Rust. One toolchain draws the pictures, and one command refreshes every
picture for a release. The pictures do not come from the insta snapshot
tests: accepting a snapshot never changes a picture.

## Running it

```sh
just screens                                     # every scene in tools/screens/scenes
just screens tools/screens/scenes/help.toml      # the scene files or directories given
just screens --out /tmp/pictures                 # elsewhere than target/screens
```

For each scene file `<name>.toml` the tool writes `<name>.png`, by default
to `target/screens/` of the workspace, and prints the path of every file
it writes. It also writes `index.md` there, which shows the pictures of
the run, each under its scene's caption, for a look at all of them at
once. An error names the scene file and what went wrong, and the tool
exits with 1.

## The scene format

One TOML file per picture. A field the format does not know is an error
that names it.

```toml
# What the picture shows; index.md shows it under the picture.
caption = "The queue switcher over a session, inside tmux"
# The terminal's size in cells.
columns = 80
rows = 24
# Where the context pane goes, as asqr's --layout: auto, beside or below.
# Optional, auto by default.
layout = "auto"
# The glyphs Talqi is drawn in on the empty queue: sextant, as asqr draws
# it in Ghostty, or quadrant. Optional, sextant by default.
emblem = "sextant"
# The queue asqr watches, by name. Optional; without it, or with
# "default", the default queue, which the empty queue does not name.
queue = "asqr-todos"
# The keys pressed, in order; a frame is drawn before the first key and
# after each one.
keys = ["w"]

# The sessions waiting, in queue order; the first one shows.
[[sessions]]
file = "../../../examples/sessions/release-checklist.json"

# Queues no asqr watches in which sessions wait, for the status line.
[[elsewhere]]
queue = "default"
waiting = 1

# What the queue switcher shows when a key opens it.
[switcher]
tmux_session = "asqr"
root = "/data/asqr"

[[switcher.queues]]
name = "asqr-todos"
waiting = 2
watched = true

[[switcher.taken]]
name = "ntropy"
tmux_session = "ntropy"
```

| Field | Meaning |
|---|---|
| `caption` | What the picture shows, for `index.md`. Required. |
| `columns`, `rows` | The terminal's size in cells. Required. asqr needs 60 by 15 at least; the empty queue shows Talqi from 32 rows on. |
| `layout` | `auto`, `beside` or `below`, as asqr's `--layout`. Default `auto`. |
| `emblem` | `sextant` or `quadrant`. Default `sextant`. |
| `queue` | The name of the queue asqr watches. Default: the default queue. |
| `keys` | The keys pressed, in order. Default: none. |
| `[[sessions]]` `file` | A session file, relative to the scene file. |
| `[[sessions]]` `id` | The session's id. Default: the session's own `id`, else the file's name without `.json`. |
| `[sessions.carried_from]` `id`, `title` | The earlier session whose draft the session continues (spec 4.3). The header says `continues your draft of "<title>"`, or names the id where there is no `title`. |
| `replaced` | The title of the session the person had open, which the scene's first session replaced by continuing its draft: the status line shows asqr's information notice for it (spec 7.9). The first session needs a `carried_from`. |
| `[[elsewhere]]` `queue`, `waiting` | A queue no asqr watches and how many sessions wait in it. |
| `[switcher]` `tmux_session` | The tmux session asqr runs in. Without it, asqr runs outside tmux. |
| `[switcher]` `root` | The directory of the named queues, where the switcher offers a new queue. Required when a row has a `name`. |
| `[[switcher.queues]]` `name` or `dir` | A named queue under `root`, or a `--dir` queue shown as written; one of the two. |
| `[[switcher.queues]]` `waiting`, `watched` | The sessions waiting in it (default 0) and whether this asqr watches it (default false). |
| `[[switcher.taken]]` `name`, `tmux_session` | A queue another asqr watches, and the tmux session it runs in. |
| `start_picker` | `true` draws asqr's start picker (spec 7.1) over the `[switcher]`'s queues instead of asqr's screen; the keys go to the picker. Needs a `[switcher]`. Default `false`. |
| `first_start` | `true` draws asqr's first-start screen (spec 7.1) instead of asqr's screen; the keys go to it. Needs a `[settings]` `path`, which its footer names, and `home` to show it with `~`. Default `false`. |
| `[[invalid]]` `id`, `errors` | A session file asqr answered with an error, and each problem it had, as the dialog shows them. |
| `[replay]` `reference`, `outcome`, `finished` | Shows the scene's one session as `asqr replay` shows a finished one (spec 7.11): the REF `asqr history` prints, how it ended (`sent`, `rejected`, `error`, `withdrawn` or `none`) and when, as the status line writes it, such as `Thu 29 Oct 16:41`. The scene lists exactly one `[[sessions]]`. |
| `[replay]` `result` | A result file in the result format, relative to the scene file, whose answers fill the session in: those of a sent or rejected result, the unsent draft of a withdrawn one. Examples are in `tools/screens/results/`. |
| `[replay]` `bundle`, `waiting` | The month bundle the session came from, such as `2026-10.zip`, and the sessions waiting in the queue, which the title bar counts. |
| `[[replay.files]]` | A file the session showed and what the archive recorded: `path` as the session file writes it, `original_path`, `state` (`kept`, `not_kept`, `missing`, `unreadable` or `not_recorded`), and where known `sha256`, `size`, `modified`, `kept_size`, `reason`, `copy` and `damaged`. A kept file is drawn from the file at `path`. |
| `image` | A picture, relative to the scene file, that a terminal with graphics shows where the screen shows an image entry: the file of an image entry of the scene's sessions, which the scene's keys bring up. The tool cannot tell which entry is on screen; a path that is no image entry of the scene's sessions is an error. See "Images" below. |
| `home` | The person's home folder, which asqr names `~` where a path lies below it. Without one nothing is named `~`. |
| `[settings]` `path` | The settings file's path, as the frame of the settings pane names it at its bottom edge. The tool reads the file at that path if there is one: a path in a folder that exists on no machine, such as `/home/someone/.config/asqr/settings.toml`, keeps the picture independent of the machine. |
| `[settings]` `context_held_by` | `flag` or `variable`: `Context` is held at the scene's `layout` as `--layout` or `ASQR_LAYOUT` holds it, which locks the row in the pane. |

The switcher shows its rows in the order the scene lists them; asqr lists
a `--dir` queue first, then the named queues by name.

### Keys

A string is one key: a single character (`"j"`, `"?"`, `"L"`, `"1"`) or a
name, `enter`, `esc`, `tab`, `shift-tab`, `space`, `backspace`,
`delete`, `up`, `down`, `left`, `right`, `home`, `end`, `pgup` or `pgdn`,
with `ctrl-` or `alt-` in front (`"ctrl-d"`, `"alt-x"`). `{ text = "..." }`
types each of its characters as a key of its own; it holds no control
character, which is pressed by its name instead. `{ paste = "..." }`
pastes its text.

A frame is drawn after every key, and what it measured goes back to the
app before the next one, as asqr's run loop does. A frame that shows
`loading…` in the context pane waits for the entry to load, up to 60
seconds, before the next key.

A key that would make asqr quit, open an entry, open an editor or switch
the queue is an error, and so is a key that takes a queue in the start
picker, and so is one that quits or presses the button of the first-start
screen: a picture cannot show what follows it. A key that finishes a
session removes it from the screen with the notice asqr shows (`✔ sent`,
`✗ rejected`) and writes no result. A key that saves a draft writes none;
the picture shows the answer all the same. A change in the settings pane
applies to the screen behind it and is not saved. `w` opens the switcher
with the scene's `[switcher]`, and closing it shows the scene's
`[[elsewhere]]` again.

### Sessions

A session is read as asqr reads a file in its inbox: relative paths in
its context entries are resolved against the session file's directory,
as `asqr ask` does, and the session is checked as a file named after its
id. A session that does not pass, two sessions with the same id, or a
scene `id` other than the session's own is an error. A session whose
`follows` names another session of the scene shows that one's title.

## Adding a scene

1. Write `tools/screens/scenes/<name>.toml`. Take the sessions from
   `examples/sessions/`, and put new ones written for the guide there too,
   so the README's examples and the pictures share their sources.
2. Run `just screens tools/screens/scenes/<name>.toml` and look at
   `target/screens/<name>.png`.
3. `just check` draws every scene in `tools/screens/scenes` in its tests,
   so a scene that stops drawing fails the check.

## Refreshing the pictures for a release

The help and the empty queue show `asqr <version>`. After the version
bump, `just screens` draws every picture again with the new version.
The pictures the guide in `docs/guide` shows are drawn by
`just guide-pictures`, which writes every scene into
`docs/guide/pictures` and removes the `index.md` of the run.

## The look

The pictures show the terminal in these colours, Gruvbox Material Hard
dark by sainnhe (<https://github.com/sainnhe/gruvbox-material>, MIT):

| | Colour | ratatui |
|---|---|---|
| background | `#1d2021` | `Color::Reset` as background |
| foreground | `#d4be98` | `Color::Reset` as text colour |
| 0 | `#1d2021` | `Black` |
| 1 | `#ea6962` | `Red` |
| 2 | `#a9b665` | `Green` |
| 3 | `#d8a657` | `Yellow` |
| 4 | `#7daea3` | `Blue` |
| 5 | `#d3869b` | `Magenta` |
| 6 | `#89b482` | `Cyan` |
| 7 | `#d4be98` | `Gray` |
| 8 | `#eddeb5` | `DarkGray` |
| 9 to 15 | as 1 to 7 | `LightRed` to `White` |

ratatui's named colours are the 16 ANSI colours: SGR 30-37 and 40-47 take
entries 0 to 7, 90-97 and 100-107 entries 8 to 15. `Color::Indexed` 0 to
15 takes the palette, 16 to 231 xterm's colour cube and 232 to 255 its
grey ramp; `Color::Rgb` stays as it is.

Reversed text swaps the text colour and the background. Dim text is drawn
at half opacity over its background, Ghostty's default `faint-opacity`.
Glyphs are blended over the pixels below in sRGB values, as Ghostty blends
on macOS by default. A border of 10 pixels in the background colour
surrounds the screen: a window padding of 5 points, as Ghostty draws it on
a display of content scale 2.

Box drawing, block elements and the sextants are painted with Ghostty's
geometry (`src/font/sprite/draw/` in ghostty-org/ghostty at commit
fbed180a5f18): partial blocks of `round(size * fraction)` pixels,
quadrants and sextants whose halves and thirds meet without a gap, lines
2 pixels wide (4 for heavy ones, double ones two light lines with a gap
of 2), dashes that tile from cell to cell, and arcs and diagonals as
Ghostty curves them. Ghostty is under the MIT licence, kept in
`LICENSE-ghostty`. The arcs and the diagonals are anti-aliased by
distance to their path, so their edge pixels may differ a little from
Ghostty's.

## Images

asqr works out the cells an image takes from the picture's pixels, the
cell size and the room the viewer gives it, the same for every graphics
protocol (`Image::lay_out` in `src/graphics/image.rs`). The tool gives
asqr its own cell, 15 by 39 pixels, so the half blocks it draws take the
cells the image takes in a terminal with graphics.

A scene with `image` gets that picture where a terminal with graphics
shows it. The cells with an RGB background are the image's, as half
blocks colour every cell of an image in RGB and asqr draws its own
screen in the terminal's named colours; they must form one rectangle,
so a scene with an `ansi` entry in RGB colours or Talqi on the empty
queue cannot take an `image`. The tool clears those cells to the
terminal's background and paints the picture into their pixels as
ratatui-image sends it under the Kitty protocol: scaled with
`DynamicImage::resize` and asqr's `Triangle` filter into the cells,
proportions kept, at their top left, and blended by its alpha over the
background. A picture asqr enlarges three times or more goes to the
terminal at a divisor of the cell's pixels for the terminal to enlarge
(ADR 80); the tool scales it to the full pixels in one step.

## Fonts and licences

| Directory | Files | Source | Licence |
|---|---|---|---|
| `fonts/victor-mono/` | `VictorMono-SemiBold.ttf`, `VictorMono-Bold.ttf`, `VictorMono-SemiBoldItalic.ttf`, `VictorMono-BoldItalic.ttf` | Victor Mono 1.560 by Rune Bjørnerås, `VictorMonoAll.zip` from <https://rubjo.github.io/victor-mono/> | SIL Open Font License 1.1, no Reserved Font Name (`LICENSE.txt`) |
| `fonts/dejavu/` | `DejaVuSansMono.ttf`, `DejaVuSansMono-Bold.ttf`, `DejaVuSans.ttf` | DejaVu Fonts 2.37, `dejavu-fonts-ttf-2.37.zip` from <https://github.com/dejavu-fonts/dejavu-fonts/releases> | Bitstream Vera and Arev licences; DejaVu's changes are in the public domain (`LICENSE`) |
| `fonts/noto-sans-symbols-2/` | `NotoSansSymbols2-Regular.ttf` (unhinted) | Noto Sans Symbols 2 2.008, `NotoSansSymbols2-v2.008.zip` from <https://github.com/notofonts/symbols/releases> | SIL Open Font License 1.1, no Reserved Font Name (`OFL.txt`) |

Victor Mono draws the text, in SemiBold for regular text, Bold for bold,
SemiBold Italic and Bold Italic for italics. It is the upstream release,
not the Nerd Fonts build: the Nerd Fonts build adds icon sets under
licences of their own, and asqr draws no Nerd Font icon.

What Victor Mono lacks comes from the next face that has it, in this
order:

1. DejaVu Sans Mono, Bold in bold text: the box marks `☐ ☑ ☒`, the checks
   `✓ ✔ ✗`, `✎`, the pointer `❯`, `⏎` and `U+FFFD`;
2. DejaVu Sans: `ℹ`;
3. Noto Sans Symbols 2: the Control Pictures `␀` to `␡`, which code and
   diff entries show for control bytes.

Box drawing (U+2500 to U+257F), block elements (U+2580 to U+259F) and the
sextants (U+1FB00 to U+1FB3B) come from no font: they are painted cell by
cell, as Ghostty draws them, so lines and Talqi's cells meet without
seams.

The test `tests/glyph_coverage.rs` checks that these fonts cover every
glyph asqr draws. It reads the characters of the string and character
literals in asqr's code, following its module tree from `src/lib.rs` and
`src/main.rs` and leaving out what only tests build, and of the files
that code includes, such as Talqi's templates. It adds what asqr computes
(the Control Pictures and `U+FFFD`), what ratatui draws for asqr's
rounded borders, and printable ASCII. It leaves out the Kitty image
placeholder `U+10EEEE` and its diacritics (`src/graphics/placeholders.rs`):
under the Kitty protocol the terminal draws the image where they stand,
so they are never drawn from a font.

## The cell

Every face is drawn at 28 pixels per em. A cell is 15 pixels wide and 39
high, with the baseline at row 30, the underline at rows 32 and 33, a
strikethrough at rows 21 and 22, and box-drawing lines 2 pixels wide (4
for heavy ones). These are the numbers Ghostty computes for the Nerd
Fonts build of Victor Mono at 14 pt on a display of content scale 2,
written down as constants in `src/fonts.rs` with the rules they follow.
The upstream file gives the same numbers, which a test checks.

Victor Mono's glyphs start at their cell's left edge with their own
bearings and are not clipped to the cell, so an italic may reach into the
next one, as in Ghostty. A glyph from a fallback face stands on the same
baseline, centred on its cells, and is scaled down about the baseline
when it is wider than its cells. A character of two columns takes two
cells and paints its background over both. Variation selectors and
joiners inside a cell draw nothing; a combining mark is drawn over the
character before it.

## Limits

- Images in the context pane are drawn as half blocks, each cell two
  pixels of the image, sized for the picture's cells, unless the scene
  names its `image` (below).
- No ligatures: Victor Mono's ligatures, such as `->` or `!=`, show as
  their characters. Ghostty shapes them with the font features `calt`,
  `clig` and `liga`, which needs a text shaper.
- Glyphs are rasterized by ab_glyph, not by Core Text, so their strokes
  can look a little thinner or thicker than in Ghostty.
- Keys that quit, open an entry, open an editor, switch the queue or take
  a queue in the start picker are refused, and no draft or result is
  written.
- A session that follows one outside the scene shows that one's id as
  written, as the tool has no archive to find its title in.
- A character no committed font has is an error naming it and its cell.
- Two draws with the same build give the same bytes; a new version of a
  dependency, such as the PNG encoder, may change them.
