---
title: "A log test of the Markdown fallback fails depending on which tests run beside it"
kind: bug
component: tui
origin: implementation
---
# A log test of the Markdown fallback fails depending on which tests run beside it

Found on 2026-10-31 by the replay agent and reproduced by the lead on
main e473446: `cargo test --lib tui::render::text::tests::a_text_the_parser`
runs three tests, and
`a_text_the_parser_fails_on_is_logged_once_without_the_text`
(`src/tui/render/text.rs`, around line 295) fails 5 of 5 times with
`left: 0, right: 1` (no log line). Alone (`--exact`) and in the full
`just check` it passes.

## Problem

The test sets a thread-local subscriber with
`tracing::subscriber::with_default` and expects one log line. A
likely cause, not verified: `tracing` caches each callsite's interest
the first time the callsite runs. When another test thread runs the same
`warn!` first with no subscriber, the callsite is cached as never
enabled, and the thread-local subscriber of this test never sees the
event. Which test runs first decides the result, so the test passes or
fails by order.

## Impact

A red test run that has nothing to do with the change under test; the
full suite hides it today only through test order.

## Suggested fix

Call `tracing::callsite::rebuild_interest_cache()` after the subscriber
is set (inside `with_default`, before the loop), or capture the log in a
way that does not depend on callsite interest. Confirm with the
substring filter above, run 20 times.
