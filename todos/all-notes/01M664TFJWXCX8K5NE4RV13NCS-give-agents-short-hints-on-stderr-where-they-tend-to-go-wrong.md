---
title: "Give agents short hints on stderr where they tend to go wrong"
kind: feature
component: cli
status: needs-discussion
origin: request
---
# Give agents short hints on stderr where they tend to go wrong

asqr's subcommands should print a one-line hint on stderr at the moments
an agent tends to go wrong: what to do next, how something works, or a
warning about a likely mistake. What programs read on stdout stays the
same. Jakob asked for this on 2026-10-30, after deciding that `asqr ask`
reminds the agent to wait for the result. The wording is not settled yet:
an adversarial review finds it before anything is built.

## Why

On 2026-10-29 an agent asked an info-only session, never waited for its
result, and missed a note the person had written on it. The skill held
the rule, but the agent never read that part. A hint arrives at the
moment the mistake happens, and the binary prints it whatever skill copy
the agent has. Several hints can also back up or replace rules in the
skill, which is being cut to a 16 KB core.

## Decided

- Jakob, 2026-10-30, asqr session 01M663RDEWA9W6SVK9KWFMC1ZV: `asqr ask`
  prints a reminder to wait for the result after every session it
  accepts, not only info-only ones (A1 below).
- Jakob, 2026-10-30, asqr session 01M664CT1H5WE3J1MT97JNYGDT: the hints
  below go into the wording review, and the skill gets one line telling
  agents that asqr prints hints and that they follow them.
- Lead decisions (2026-10-30), technical: hints go to stderr, as their
  own line with the prefix `hint:` for advice; errors and warnings that
  gain advice keep their own prefix (`error:`, `warning:`, `asqr:`). A
  hint prints every time its situation occurs, never once per day or
  per session, since an agent loses its context on compaction. No switch
  to turn hints off for now. All hints go through one print function in
  `src/cli/`, next to `print_warnings` in `src/cli/format.rs`.

## The hints (first drafts, wording open)

Facts as read on `main` at `ad47242`.

`asqr ask`:

- A1, session accepted without `--wait`: today only the id on stdout.
  Draft: `hint: wait for the result: asqr wait <id> --timeout 540`.
  Open: whether the binary names the skill's timeout, and whether an
  info-only session gets its own wording.
- A6, nobody watching (`warn_if_nobody_watches`, `src/cli/ask.rs`): today
  "start `asqr` in another terminal", which reads like an order to the
  agent. Draft: tell the person to start `asqr`; the session waits in the
  queue. Keep the phrase `no asqr is watching queue` stable (a test
  checks it).
- A5, refused for an unread result with the same id
  (`DropError::UnreadResult`, `src/queue/drop.rs`): today suggests
  `--force`. Draft: say that `--force` archives that result unread, and
  offer leaving `id` out of the file instead.
- A2, accepted with warnings: today the warnings scroll past. Draft:
  "N warnings above; fix the file and run asqr ask <file> --replaces
  <id>". Count after the `RelativePath` filter in `ask.rs`.
- A3, an `id` in the file replaced a waiting session (`Dropped::Replaced`,
  only logged today): a notice that the waiting session was replaced and
  that untouched questions show the new defaults.

`asqr wait` and `asqr result`:

- W2, a printed result holds notes (session `note`, answer `note`, or
  `reason`): draft "the person left notes (session note, 2 items); read
  them before you act". Only when notes exist.
- W1, timeout, exit 12: today "no result for <id> yet (timeout)". Draft:
  add that the person has not answered and to run `asqr wait <id>`
  again; `result` gets its own wording. Spec 6.4 quotes the message.
- W4, unknown id, exit 13: name the queue and point at `--queue`,
  `ASQR_QUEUE` and `asqr status --since`. Spec 6.4 quotes the message.
- W3, rejected or refused result (exit 10 or 11): say where the reason
  is, and that answers in a rejected result were not submitted.

`asqr withdraw`:

- D2, a forced withdrawal returned a `draft`: "these are not decisions;
  use them only as presets". Also on `wait`/`result` of such a result
  (exit 14).
- D1, refused while being answered (`WithdrawError::BeingAnswered`):
  today suggests `--force`. Draft: ask the person first; then
  `asqr withdraw --force <id>` withdraws it and tells them. The same
  variant feeds the `ask --replaces` refusal, so both change together.

Not picked: a hint on `ask --wait` without `--timeout`, on a valid file
with warnings in `validate`, and a tip after `skill --install`.

## Notes for the implementation

- Exact-stderr tests that change: `tests/cli_withdraw.rs`,
  `tests/cli_ask.rs` on the refusals; a clean `validate` must stay
  silent (`tests/cli_format.rs`, `tests/cli_skill.rs`).
- The skill's rules each hint backs up may shrink to one line; the skill
  split (todo "Make the skill keep an agent from leaving the result of
  an info-only session unread") changes where those rules live, so this
  work starts after it.
- Callers that merge stderr into stdout (`2>&1`) see hints in their
  data; they already see warnings there.

## Relations

- Depends on: [Make the skill keep an agent from leaving the result of an info-only session unread](01M63AVXW95AQA517AV8K35XGV-make-the-skill-keep-an-agent-from-leaving-the-result-of-an-info-only.md)
