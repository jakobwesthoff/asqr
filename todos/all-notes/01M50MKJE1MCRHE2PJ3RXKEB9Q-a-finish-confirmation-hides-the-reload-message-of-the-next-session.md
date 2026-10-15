---
title: "A finish confirmation hides the reload message of the next session"
kind: bug
component: tui
status: needs-discussion
impact: low
origin: implementation
tags: [ux]
---
# A finish confirmation hides the reload message of the next session

Found while implementing the finish confirmation (commit 52b816b,
2026-10-15). When the session that comes up after a submit or reject
carries a "changed on disk and was reloaded" message, that message is
never shown: the confirmation covers it, and the first key clears both.
Read in the code, not reproduced in a test yet.

## Problem

The status line shows one notice: the app's notice if there is one,
else the message of the session on screen (`status_line` in
`src/tui/render/screen.rs`). Sequence:

1. Session A is on screen, session B waits in the queue.
2. B's file changes on disk; `SessionState::replace_session`
   (`src/tui/session.rs`) gives B the message
   `B changed on disk and was reloaded` (Info).
3. A is submitted. B comes up; the status line shows the app's
   `✔ sent "A" · …` notice, which wins over B's message.
4. The next key: `App::handle` clears the app's notice, then
   `SessionState::handle` clears B's message before handling the key.

## Impact

A notice is lost without anyone having decided so. B was not on screen
before, so its reload may not matter to the user at that point.

## Open question

- Let the first key clear only the confirmation, so B's message shows
  afterwards until the next key.
- Or decide that a reload of a session not yet seen needs no message,
  and drop it in that case.
- Or keep the behaviour and record it in ADR 0038.
