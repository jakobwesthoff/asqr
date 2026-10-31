---
title: "Keys"
tags: [guide/reference]
site:
  index: true
  label: Reference
  order: 5
---
Every key of asqr, by place. Inside asqr, `?` shows the keys that work
where the cursor is.

## Keys on a question

These keys work on the rows of a question and its options.

| Key | What it does |
|---|---|
| `↑`/`↓`, `k`/`j` | Move between the rows. An option too big for the screen scrolls first. On the first and the last row they scroll through the lines above or below the row before the cursor moves on. |
| `←`/`→`, `h`/`l` | Go to the previous and the next question. The review tab is the last one. |
| `enter` | In a single choice, picks the option and moves on to the next question. In a multiple choice and on an info item, moves on. |
| `space` | In a multiple choice, toggles the option. |
| `1`-`9` | In a single choice, picks that option and moves on. In a multiple choice, toggles it. |
| `backspace` | In a single choice, on the picked option's row, clears the pick, a default included. On any other row it does nothing. |
| `n` | Edits the question's note in its line. |
| `L` | Opens the list of waiting sessions. |
| `w` | Opens the queue switcher. |
| `,` | Opens the settings pane. |
| `i` | Shows the whole intro of the session. |
| `o`, `z`, `s` | `o` opens the entry shown in the app your system has for its file type. `z` shows the context pane full screen. `s` shows a diff side by side or in one column. |
| `tab`, `shift-tab` | Move into the context pane. `tab`, `shift-tab`, `esc` or `q` move back out. |
| `pgup`/`pgdn` | Scroll the context pane by a page less 2 lines, also while you type in a field, as long as a pane shows. |
| `ctrl-u`/`ctrl-d` | Scroll the context pane by half a page, also while you type in a field, as long as a pane shows. Without a pane, in a field, `ctrl-u` undoes and `ctrl-d` deletes. |
| `q`, `ctrl-c` | Quit asqr and keep your answers as a draft. In the context pane `q` goes back instead. |
| `?` | Shows the help. |

- In a multiple choice, a selection beyond `max` is refused with the
  message "at most N options".
- `enter` on the last question moves on to the review tab.
- `enter` on the option that is already picked keeps it picked.
- Inside a field, every key that produces text types, so `n`, `q` and
  the others type there until you press `esc`.

The pages that explain these keys: [answering a
session](01M6916HYT9Z4N422AX47A8JQH-answering-a-session.md) has the common
ones, `i` is on [the screen](01M68BT9AJHW7WX304EQKGBGF6-the-screen.md), `n`, `space`, the
digits and `backspace` are on
[questions and answers](01M68BT9AXJNJQR45K1FBR9VTG-questions-and-answers.md),
`tab`, `o`, `z` and `s` are on [context: files, diffs and
images](01M68BT9B5R7ASMVGW9SHK8MB2-context-files-diffs-and-images.md) and
[the context pane in detail](01M68BT9BDTGM96WSRXRSYFZKY-the-context-pane-in-detail.md),
`pgup`/`pgdn` and `ctrl-u`/`ctrl-d` are on
[less obvious keys](01M68BT9BTS0J2VR1Y5N4NMEVC-less-obvious-keys.md), `L`
and `w` are on [queues](01M68C1XV58STWHBH179QYXPZR-queues.md) and `,` is on
[settings](01M68C1XVEMX1D38GK6GY0V1NZ-settings.md).

## On the review tab

- `enter` on `[ Submit ]` submits the session.
- `enter` in the note to the asker submits the session with that note.
- `enter` in the reason field rejects the session with that reason.
- `enter` on a question in the list moves to that question.

## In the context pane

`tab` or `shift-tab` moves the focus into the pane wherever one shows,
also from a field. A focused pane takes every key:

| Key | What it does |
|---|---|
| `↑`/`↓`, `k`/`j` | Scroll by a line. |
| `←`/`→`, `h`/`l` | Scroll by 4 columns where the entry's lines are cut at the pane's edge. |
| `pgup`/`pgdn` | Scroll by a page less 2 lines. |
| `ctrl-u`/`ctrl-d` | Scroll by half a page. |
| `g`/`G`, `home`/`end` | Go to the top and to the bottom. |
| `z`, `o`, `s`, `?` | Act as they do on a question. |
| `ctrl-c` | Quits asqr. |
| `tab`, `shift-tab`, `esc`, `q` | Give the focus back to where it was. |

Every other key does nothing there, so a word typed after `tab` never
quits asqr.

## In the full-screen view

`z` shows the pane's entry over the question. The view takes every key
as the focused pane does, so `enter`, the digits and `n` cannot reach
the question behind it. `z`, `esc`, `tab` and `q` leave it.

## In the help, the intro, the lists and the settings

- The help closes on any key.
- The whole intro (`i`) scrolls with `↑`/`↓` and `k`/`j` by line,
  `pgup`/`pgdn` by a page less 2 lines, `ctrl-u`/`ctrl-d` by half a page
  and `g`/`G` or `home`/`end` to the ends. `esc`, `q` and `i` close it,
  `ctrl-c` quits asqr, and every other key does nothing.
- The session list (`L`): `↑`/`↓` or `k`/`j` move, `enter` switches to
  the session, `esc` or `L` close the list and `q` quits asqr. Its key bar
  reads `↑/↓  move   enter  open   esc  close`.
- The queue switcher (`w`): `↑`/`↓` or `k`/`j` move, `enter` switches
  to the queue under the cursor, `esc`, `q` and `w` close it and
  `ctrl-c` quits asqr. On the new-queue row, letters, digits, `-`, `_`
  and `.` type, `backspace` removes the last character and `esc` stops
  typing.
- The settings pane (`,`): `↑`/`↓` or `k`/`j` move, `←`/`→` or `h`/`l`
  change the value and save it at once, `esc`, `q` and `,` close it and
  `ctrl-c` quits asqr.

## At the start

The [first-start screen](01M68C1XVEMX1D38GK6GY0V1NZ-settings.md): `↑`/`↓`
or `k`/`j` move, `enter` or `space` on a value picks it, and `enter` then
moves to the button. `enter` on `[ Save and start ]` writes the setting and
starts asqr. `q` and `ctrl-c` quit and write nothing. Every other key does
nothing, and a paste goes nowhere.

The [start picker](01M68C1XV58STWHBH179QYXPZR-queues.md): `↑`/`↓` move,
`enter` watches the queue under the cursor, and `q` and `ctrl-c` quit. On
the new-queue row `q` types, and `esc` stops typing there and does nothing
elsewhere.

## In a replay

A [replay](01M68KBBY1Y1YB4YFQ8WC37QP5-finished-sessions-history-and-replay.md)
shows a finished session, so most keys that change something do nothing.

- `space`, `1` to `9`, `n`, `backspace`, `L`, `w`, `,`, `ctrl-g`, typing and
  paste do nothing. No key saves a draft or sends a result.
- `enter` moves to the next question and, from the last, to `Result`.
- `d`, from the question or from the pane, opens the details of the file
  the entry shows. Any key closes them.
- The movement keys, `tab`, `o`, `z`, `s`, `i`, `?`, `q` and `ctrl-c` keep
  their meaning.

## In a field

The own-answer row, a text answer, a note, the session note and the
reject reason are fields. A field takes the keys as soon as the cursor
lands on it.

- `esc` leaves the field and the cursor stays. `enter` picks the own
  answer and moves on, as on an option.
- `ctrl-j` adds a line.
- `ctrl-g` opens the field in your editor.
- `←`/`→` and `ctrl-b`/`ctrl-f` move by a character. `ctrl-←`/`ctrl-→`
  and `alt-b`/`alt-f` move by a word. `home`/`end` and `ctrl-a`/`ctrl-e`
  move to the start and the end of the line.
- `↑`/`↓` and `ctrl-p`/`ctrl-n` move by a row of the field as shown. A
  field is left with `↑` only from its first row and with `↓` only from
  its last row.
- `backspace` and `ctrl-h` delete the character before the cursor,
  `delete` and `ctrl-d` the one under it. `ctrl-w`, `alt-h` and
  `alt-backspace` delete the word before it, `alt-d` and `alt-delete`
  the word after it, `ctrl-k` the rest of the line. `ctrl-y` puts back
  what the last word or line deletion took.
- `ctrl-u` undoes an edit and `ctrl-r` redoes it.
- There is no selection: `shift` with a move only moves.
