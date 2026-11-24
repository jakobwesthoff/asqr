---
title: "Keep agents from losing the notes of info-only and already read sessions"
kind: improvement
component: project
origin: incident
---
# Keep agents from losing the notes of info-only and already read sessions

The skill already says that every session comes back as a result, an
info-only one too, that `asqr wait` belongs to every session and that
every note is read before the matter counts as done (skills/asqr,
"Asking and waiting"). An agent still lost two questions of the person
in one conductor run of the Torchsnap GPUI rewrite (2026-11-23/24),
because a note in a result is easy to read past when the agent expects
no answer. Jakob asked on 2026-11-24 to extend the skill so this cannot
happen again.

## The two cases

1. **Info-only session, no wait started.** Session
   `01M87BMN410GH6MXRJ1EMA18S9` (2026-11-24, one `info` item explaining
   an architecture test). The agent asked it and moved on without
   `asqr wait`, since there was nothing to answer. The person answered
   19:07 with a note on the info item: "Sollte man nicht hier viel eher
   auf dem Rust Token streamen oder auf irgendwas anderem arbeiten als
   mit reinen Strings. Gibt es dafür nicht sinnvolle Best Practices,
   sowas zu tun?" The agent learned of it only about an hour later, when
   the person asked in the chat why that question had no session.
2. **Result read, note not acted on.** Session
   `01M855ARS2Z4HFZ1H7Y55B71W1` (2026-11-23 22:38, info items explaining
   where a gadget's inline view appears). The agent fetched the result
   and saved it, but the session carried no choice, so the one note
   ("geht auch inline und zusätzlich normale results?" on the code
   example of the first place) never got an answer. Found the next
   evening only because the person asked whether other questions had
   been left.

In both cases the person had no sign that their question was lost.

## Proposal

Skill text (the core of this todo):

- Move the rule up and make it a checklist step, not a paragraph: after
  `asqr ask`, always start `asqr wait` in the background, whether the
  session asks anything or not.
- Say plainly that a note on an info item is often a question, and that
  a session counts as handled only when every note has a reply (in a
  follow-up session or in the chat).
- Add a closing check before the agent ends a turn or a task: `asqr
  status --json --since <age>` and read every answered session whose
  notes have not been answered.

Tool support, worth discussing (it makes the check reliable instead of
relying on the agent's memory across compactions):

- `asqr status` reports answered sessions whose result nobody fetched
  yet (asqr already knows when `wait` or `result` delivered a result).
- A way for the agent to mark a session's notes as handled, so `status`
  can list "answered, notes not handled" after a compaction or restart.
- `asqr ask` on an info-only session prints a hint that notes may come
  back and a wait is due, like the existing hint about notes on
  `wait`.

## Open questions

- Is the tracking of handled notes worth a format or queue change, or
  is the skill text plus the existing `status --since` enough?
