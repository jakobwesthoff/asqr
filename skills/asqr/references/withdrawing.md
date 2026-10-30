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
  the session in the end. Use them only as presets in a replacement
  session (`"default": true` on the options they picked, which asqr
  shows marked `◀ default`; say in the question's text that these are
  their earlier answers, so the marker does not read as your
  recommendation), where they confirm everything again. Typed text
  (`custom`) and notes have no preset in the format: quote them in the
  replacement question's text, such as "Earlier you wrote: ...", so the
  person can paste or retype them. When you build no replacement,
  ignore them.
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
prints the new id. It refuses where `withdraw` or `ask` would refuse,
with the same exit codes; 13 means the old id is unknown. When it
refuses, nothing changes: the old session stays and the new one is not
dropped. If stderr says the old session was withdrawn already, run the
same command again: it finds the old one withdrawn and drops the new
one.

`--force` with `--replaces` also ends a session the person is in the
middle of: their answers so far go into the old result's `draft`, and
they have to start over on the new one. Use it only as the exception,
after asking them. It also moves an unread result of the new id aside;
reading a result with `asqr result` does not move it, so to get past an
old result of that id without `--force`, leave `id` out of the new file
and let `ask` assign a fresh one.

`ask` refuses to replace a session the person has started answering, and
refuses while an unread result with the same id is in the outbox.
`--force` moves that result into the archive first. `ask --replaces`
refuses the same way for the old session, and its `--force` takes that
one too.

End of references/withdrawing.md.
