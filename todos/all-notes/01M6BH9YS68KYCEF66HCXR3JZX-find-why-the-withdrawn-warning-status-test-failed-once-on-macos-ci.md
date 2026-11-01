---
title: "Find why the withdrawn-warning status test failed once on macOS CI"
kind: bug
component: tui
origin: incident
tags: [testing]
---
# Find why the withdrawn-warning status test failed once on macOS CI

The unit test
`tui::inbox::tests::the_withdrawn_warning_shows_in_the_status_line_cut_before_its_quote`
(`src/tui/inbox.rs:2673`) failed once in CI on macOS while 3010 other
tests passed, on a commit that changed only a todo file.

## Problem

CI run 41305562461, job `check (macos-latest)`, commit 60163a2,
2026-11-01 12:16 UTC:

```
panicked at src/tui/inbox.rs:2684:9:
assertion `left == right` failed
  left: ""
 right: "! ↩ withdrawn \"Rele…\" by the asker, with your answers so far"
```

The status row was empty, so the app held no notice after the sync that
should have seen the withdrawal. The test presses `2` (a draft-worthy
answer), applies the effect, withdraws the session with `force`, syncs
once and draws. It passed 8 of 8 times run alone on macOS (2026-11-01),
and Ubuntu passed in the same run. Not reproduced yet; a race under the
load of the full suite is the suspicion, not established.

## Suggested approach

Reproduce first: run the test many times under the full suite's
parallel load (or with CPU stress), and read what `inbox.apply` and
`withdraw_session(.., true)` do with the draft and the inbox file, and
what `sync` needs to see to raise the withdrawn notice. Then fix the
race in the code if there is one, or the test if it assumes an order
the code does not promise.
