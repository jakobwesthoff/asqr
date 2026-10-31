---
title: "Review and submit"
tags: [guide/answer]
site:
  order: 5
---
The last tab of a session is the review. You look at everything once,
then submit the answers or reject the session.

## The review tab

The review lists every question with its answer on one line, dim. A
single or multiple choice shows the labels of the chosen options. Your
own text and a typed answer show in quotes, with a line break shown as
`⏎`. `(default)` follows a default you did not touch, `-` stands for a
skipped question, and `+note` says the question has a note. Under the
list, the counts of answered, skipped and defaulted questions. Info items
are not counted.

![The review tab of an answered session: every answer on a line, the counts and the submit button](../pictures/review.png)

`enter` on a question in the list moves to that question. The review
tab opens with the cursor on `[ Submit ]`, and the keys are commands
there.

## Submit or reject

- `[ Submit ]`: `enter` submits the session.
- **Note to the asker**: a field for an optional note to the whole
  session. `enter` in it submits the session with that note.
- **Reject**: a field for an optional reason. `enter` in it rejects the
  session. The result keeps the session note and the questions that have
  an answer or a note.

There is no `n` on the review tab.

![The review tab with a note to the asker and a reason for rejecting typed in](../pictures/review-rejected-note.png)

## What happens next

The session leaves the screen. The next session in the queue takes over,
or the one before it when it was the last, or the empty screen when none
is left. The status line confirms what asqr wrote and names the session
by its title, or by its id when it has none:

```
✔ sent "Release 0.2.0" · 1 answered, 2 skipped
✗ rejected "Release 0.2.0"
```

The counts are those the review showed, a default counted as answered.
The notice stays until your next key, also over the next session.

![The notice after submitting a session, over the session that comes next](../pictures/sent-notice.png)

A submit that does not finish keeps the session and shows a notice of its
own. One such case is a session file that changed on disk after you
opened it: asqr does not send the answers, and loads the new version so
that you see it before you answer.

## Leaving without submitting

`q` quits asqr and keeps your answers. asqr writes them to a draft every
time they change while the session waits, and when it shows the session
again it restores them. It matches the answers by question and option, and
drops what no longer fits the session.

## When a session is invalid

A session file that fails validation never reaches the questions. asqr
answers the asker with an error result that names the problem, and shows
you a dialog. The dialog is framed in the warning colour. Its title names the session, and each
problem stands on a line of its own behind a `•`. Any key closes the dialog,
and it does nothing else.

![The dialog of a session file asqr answered with an error](../pictures/invalid-dialog.png)

The dialog also shows when no session is waiting.
