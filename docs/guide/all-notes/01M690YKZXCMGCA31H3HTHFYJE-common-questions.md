---
title: "Common questions"
tags: [guide/start]
site:
  order: 3
---
The questions people ask before and after their first session, each with
a short answer and the page that has the details.

## Do I need an MCP server?

No. Your agent works with asqr through shell commands and files: it writes
its questions into a JSON file, runs `asqr ask` and `asqr wait`, and reads
your answers as JSON. Any agent that can run commands and write files can
do this without configuration, and asqr has no MCP server. The wait works
under any time limit on the agent's commands: `asqr wait --timeout` ends
with exit code 12 while no answer is there, and the agent waits again.
[How an agent asks](01M68C6WTNJGM2A5WY0C3EYKRT-how-an-agent-asks.md) follows
the path.

## Do I have to install anything into my agent?

No. Tell your agent to use asqr. `asqr --help` tells it to read `asqr
skill`, which prints the skill of the asqr you run: the format, the
waiting and what to do with your answers.

> [!TIP]
> If your agent does not find its way, tell it to run `asqr skill`. You
> can also install the skill as a file with `asqr skill --install`; [The
> skill and other agents](01M68C6WTY7EK8Q9RB1MCWSFM3-the-skill-and-other-agents.md)
> says where and what that means for updates.

## Will my agent start using asqr on its own?

No. The skill tells your agent to use asqr only when you ask for it, for
example "ask me through asqr". Without such a request it asks you the way
it normally does.

## Which agents work with asqr?

Any agent that can run shell commands and write files. For its check of an
installed skill, asqr knows where Claude Code, Codex, Gemini CLI, GitHub
Copilot and Cursor keep skills.

## My agent asked, but nothing shows up

asqr and your agent use different queues. That happens when one of them
runs inside a tmux session and the other outside it, or in another tmux
session. While asqr has nothing else to say, its status line counts the
sessions waiting in queues no asqr watches, such as `1 session waits in
default   w  queues`, and `w` switches to that queue. Or start both the
same way. [Queues](01M68C1XV58STWHBH179QYXPZR-queues.md) explains which
queue each of them uses.

## What if asqr is not running when my agent asks?

The session waits in the queue, and `asqr ask` warns your agent that no
asqr is watching and tells it to ask you to start asqr. When you start
it, the session shows.

## Do I need tmux?

No. Outside tmux, asqr and your agents use the default queue. Inside tmux,
they use the queue of the tmux session, so several tmux sessions keep
their questions apart.

> [!TIP]
> In tmux, put `set -g allow-passthrough all` into your tmux
> configuration, so desktop notifications and images reach your terminal.
> [Notifications, the terminal and
> tmux](01M68C1XVP8WV9Z6NWRJ1BEWT8-notifications-the-terminal-and-tmux.md)
> has the details.

## Can I stop in the middle?

Yes. `q` quits asqr and keeps your answers as a draft, and asqr shows
them again when it shows the session the next time. [Review, submit and come back
later](01M68BT9BMFVCVEAACWR03B827-review-submit-and-come-back-later.md) says more.

## What if I do not want to answer a session?

On the review tab, type a reason into the Reject field and press `enter`.
Your agent gets the reason. If you answered in the chat instead, tell your
agent: it takes the session back with `asqr withdraw`, after checking with
you if you had started on it in asqr, and the session leaves your screen
with a notice. [Review, submit and come back
later](01M68BT9BMFVCVEAACWR03B827-review-submit-and-come-back-later.md) shows both.

## What does my agent get back?

Every answer, the questions you skipped, whether you left a default as it
was, and every note. [How an agent asks](01M68C6WTNJGM2A5WY0C3EYKRT-how-an-agent-asks.md)
shows a result.

## Can I see what I answered later?

Yes. Your answers go to your agent as a result file in the queue's
directory on your machine, and a finished session stays in the queue's
archive. `asqr history` lists the finished sessions, and `asqr replay
<REF>` shows one again, read-only, with your answers and the files it
showed. [Finished sessions: history and
replay](01M68KBBY1Y1YB4YFQ8WC37QP5-finished-sessions-history-and-replay.md)
explains both.

## Can several agents ask at once?

Yes. Sessions wait in the order they arrive, and asqr shows them one after
another. `L` lists the sessions waiting.

## Does asqr send anything over the network?

No. asqr works through files on your machine and has no networking.

## Can a script ask too?

Yes, with the same commands an agent uses. [How an agent
asks](01M68C6WTNJGM2A5WY0C3EYKRT-how-an-agent-asks.md) lists them.

## Does asqr delete anything?

Of your sessions, results and drafts, asqr deletes on its own only the
draft of a finished session, one you submitted or rejected or one the
asker took back. Everything else stays until you delete it with `asqr
prune --yes`. [Keeping files, compact and
prune](01M68KBBYC1XNESEFMG0QW8GW1-keeping-files-compact-and-prune.md) says
how.
