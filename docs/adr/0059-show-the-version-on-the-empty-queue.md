# 59. Show the version on the empty queue

Date: 2026-10-22

## Status

Accepted

## Context

Apart from the error for a context entry of an unknown type, nothing on
asqr's screen names its version, so after an update the person cannot
tell whether the asqr in their terminal is the new one.

asqr has no start screen of its own: started with nothing waiting, it
shows the empty queue, with Talqi and the sentence "Nothing to answer.
New sessions show up here as soon as they arrive." (ADR 43). Its key bar
holds `q  quit`, or `any key  close` while the dialog of an invalid
session shows over it.

## Decision

- **Text:** `asqr <version>`, such as `asqr 0.13.1`, as `asqr
  --version` prints it, dim, in the secondary role (ADR 37).
- **Empty queue:** the version stands at the right end of the key bar,
  one blank from the edge, whenever the empty queue shows, the dialog
  of an invalid session over it included. It takes only the room the
  hints leave, with at least the two blanks after the last hint before
  it; it never takes a hint's place, and where the hints leave no room
  for it, it does not show.
- No start screen is added for it.

## Consequences

- On 60 columns the empty queue's key bar shows the version beside
  `q  quit`.
- The view the tests draw with names the version `asqr 0.0.0`, so the
  tests and snapshots do not change with a release; one test pins that
  the terminal's view names the package's version.
