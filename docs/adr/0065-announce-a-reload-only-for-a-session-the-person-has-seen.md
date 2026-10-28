# 65. Announce a reload only for a session the person has seen

Date: 2026-10-28

## Status

Accepted

Amends [38. Give notices kinds and confirm a finished session](0038-give-notices-kinds-and-confirm-a-finished-session.md)

## Context

A session file that changes on disk is reloaded, and the message
"<title> changed on disk and was reloaded" says so. A session waiting
behind the one on screen gets the message as well.

After a submit or reject, the next session takes over, and the
confirmation of the finished session stays until the next key or paste,
also over the next session (ADR 38). The status line shows the app's
notice over a session's message, and the next key clears both. The
reload message of the session that takes over is therefore never shown.

## Decision

A session gets the reload message only when the person has seen it. The
person never had any other session on screen, so "was reloaded" tells
them nothing about it.

A session counts as seen from the first frame that draws it as the
active session, at asqr's minimum size or more, its tab or its
full-screen view, with no overlay (the help, the intro, the session
list) and no invalid session's dialog over it. Once seen, it stays seen
while it waits, also across reloads of its file.

A session can be active and not yet seen. One sync can take the session
on screen out, as when its file is gone, archived or invalid, and reload
the session that takes over before a frame shows it. A session can also
take over under the session list or the dialog. Counting frames, not
the time a session is active, leaves such a session unseen.

## Consequences

- A session reloaded before the person has seen it changes its content
  without a message.
- A session reloaded after it showed, while the confirmation of a
  finished session still covers its message, loses the message with the
  next key, which clears both.
