# 8. TUI interaction model

Date: 2026-09-28

## Status

Accepted

Amended by [29. Let the person add a note to the whole session](0029-let-the-person-add-a-note-to-the-whole-session.md)

Amended by [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md)

Amended by [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [36. Treat every typed field alike](0036-treat-every-typed-field-alike.md)

## Context

The person answering has to get through many questions fast, often with
long option texts and sometimes with material beside the question. The
interaction rests on four ideas:

- the own answer as a row like the options, typed in place
- no separate submit key, since the last step should be the submission
- `enter`, not `space`, to choose on a single-choice question
- a TUI that adapts to the terminal size

On the review tab, submitting is what usually follows.

## Decision

- **Layout:** one full-width column. Under the header (ADR 47), a tab bar
  lists the questions, marked `☒` once answered or `☐`, with a review
  tab last. The question keeps to a reading width and shares the tab's
  area with a context pane where it has context (ADRs 30 and 31).
- **Navigation:** `←`/`→` and `h`/`l` switch questions. `↑`/`↓` and
  `j`/`k` move between rows.
- **Picking:** on `single`, `enter` or a digit picks and moves on to the
  next question. On `multi`, `space` or a digit toggles and `enter` moves
  on. The hint under a `multi` question, such as "pick between 2 and 3
  items", leaves the own answer out, since a person can add one anyway.
- **Typed fields:** the own answer is the last row and a live field,
  typed in its line without a frame. On `multi` it counts once it has
  text. A `text` answer and the note (`n`) are edited in place too.
  Inside a field the arrows move the text cursor and leave the field at
  its edges, and `esc` leaves it; how every typed field behaves is
  ADR 36.
- **Other keys:** `z` shows the context entry full screen and `o` opens
  an image in the system viewer (ADR 31); `tab` moves the focus into the
  context pane (ADR 31); `L` opens the list of waiting sessions; `i`
  shows the whole intro (ADR 30); `?` shows the help; `q` and `ctrl-c`
  quit and keep the draft.
- **Review tab:** it lists all answers and holds Submit, the session note
  (ADR 29) and Reject with a live reason field (ADR 13). It opens with
  the cursor on Submit. There is no submit key: `enter` on Submit is the
  submission, and submitting is never refused.
- **Length counter:** shows text (`112/125`, `!` above the target, `!!`
  above `warn`). Colour is only added on top. Input is never refused.
- **Drafts:** answers are saved continuously, so nothing is lost on quit
  or crash.
- **New sessions:** arrive with a desktop notification (OSC 9 or 777) and
  a terminal bell. Both can be switched off. Inside tmux the
  notification goes out in tmux's passthrough wrapper and needs
  passthrough for asqr's pane; without it only the bell reaches the
  person. A hint for this in the TUI is an open item (spec section 11).
- **Later:** `asqr answer <file>`, which answers one file in the current
  terminal without a queue.

Rejected:

- an own answer that needs `enter` before typing
- typing that selects the own answer on single-choice questions
- the review step with an extra shortcut, and submitting straight after
  the last answer without a review
- notes as a separate row
- a question list next to the tab bar

## Consequences

- The key map is fixed. A configurable key map can come later without
  breaking the file format.
- `enter` on the last question followed by `enter` on the review
  submits. That is the fast path.
- A person can move through every question with the arrows alone, text
  fields included.
- An asker can guide the length of an answer but not enforce it.
