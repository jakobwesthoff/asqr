Part of the asqr skill. Read SKILL.md first, which holds the rules for every session; `asqr skill` prints it.

# asqr: withdrawing and replacing a session

When you no longer need the answers to a session you asked, withdraw
it, so the person does not answer it for nothing:

```sh
asqr withdraw "$id"
```

- Withdraw when the questions are settled another way, such as when the
  person answered them in the chat. For a session you rebuilt, use
  `ask --replaces` (below).
- `withdraw` takes the session out of the person's queue and prints a
  result with `"status": "withdrawn"`. Every `asqr wait` for the
  session ends with exit code 14, your own in the background included,
  and `asqr result` returns the same result later. An asqr older than
  this skill does not know the status: its `wait` ends with exit 1 and
  "not a result file" instead.
- When the person has started answering, `withdraw` refuses with exit
  code 1. A default you set that they left as it is does not count as
  starting; one they cleared does. Ask them before you take the session
  away; `--force` withdraws it anyway. An asqr running at that moment
  tells them that you withdrew it; when none runs, the session leaves
  their queue without a word.
- The result of such a withdrawal carries what they had entered in
  `draft`: their `note` and the `answers` they had given so far. A
  question they only cleared, such as a default or a pick they removed,
  is not in it; when that is all they did, the result has no `draft`. These
  are not decisions: they never sent them, and they might have rejected
  the session in the end. Do not read them as answers. When you replace
  the session with `ask --replaces` (below), asqr puts what fits into
  the replacement as the person's own draft, so you set no `default`
  for it and quote no typed text. When you build no replacement, ignore
  them.
- When the person has answered already, `withdraw` refuses with exit
  code 1: read the answers with `asqr result <id>`.
- Withdrawing twice is fine: the second call prints the same result and
  exits 0.
- A rebuilt session under the same id needs no `withdraw`: `ask` it
  again, and it replaces the waiting one unless the person has started
  answering it. Questions they have not touched show the defaults of
  the new file. When it refuses because the person has started
  answering, stderr names the way on. After a withdrawal its result
  stays in the outbox, so asking the same id again needs `--force`.

To replace a session you rebuilt under a new id, ask the new one with
`--replaces`:

```sh
id=$(asqr ask questions.json --replaces "$old_id")
```

It withdraws the old session and drops the new one in one step, and
prints the new id. If the person had started on the old session, asqr
carries their draft over into the new one: it matches their answers,
typed text and notes to your new file by question id and option id, and
leaves out what no longer fits. A question you keep under its id keeps
what they entered; a question under a new id starts fresh, so give a
question whose meaning changed a new id, and keep the ids of the
questions and options whose earlier answers should stay. You write no
text into their fields, and there is no flag for it. The person sees a
note in the session's header that it continues their draft. The result
of the old session still holds their `draft`, with their answers and
notes only. The hint after `ask` says whether anything was carried. A
withdrawn session is replaced once: a second `--replaces` of the same
old id carries nothing while a draft that continues it exists, since
its result's `draft` may be stale by then.

`--replaces` refuses where `withdraw` or `ask` would refuse,
with the same exit codes; 13 means the old id is unknown. When it
refuses, nothing changes: the old session stays and the new one is not
dropped. If stderr says the old session was withdrawn already, run the
same command again: it finds the old one withdrawn and drops the new
one.

`--force` with `--replaces` also ends a session the person is in the
middle of: their answers so far go into the old result's `draft`, and
asqr carries over what fits. Use it only as the exception, after asking
them. It also moves an unread result of the new id aside;
reading a result with `asqr result` does not move it, so to get past an
old result of that id without `--force`, leave `id` out of the new file
and let `ask` assign a fresh one.

A replacement that carries a draft counts as started: `ask` of its id is
refused, also with `--force`, and replacing it again takes `--replaces`
with `--force`. A draft is carried only into a new session that does not
wait already, and not where an answered draft of that id exists.

`ask` refuses to replace a session the person has started answering, and
refuses while an unread result with the same id is in the outbox.
`--force` moves that result into the archive first. `ask --replaces`
refuses the same way for the old session, and its `--force` takes that
one too.

End of references/withdrawing.md.
