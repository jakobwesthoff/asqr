# The asqr guide

This directory is the user guide of asqr, written as an
[ntropy](https://github.com/jakobwesthoff/ntropy) vault. The pages are the
notes in `all-notes/`, named `<id>-<title>.md`. The list below gives them in
reading order. The pictures are in `pictures/`.

## Pages

### Start

- [asqr](all-notes/01M68ASRHWD0MCGBJSFA6YXY6S-asqr.md)
- [Get started](all-notes/01M68ASRJ527YVE41S9V50YXQW-get-started.md)
- [Common questions](all-notes/01M690YKZXCMGCA31H3HTHFYJE-common-questions.md)

### Answering

- [Answering a session](all-notes/01M6916HYT9Z4N422AX47A8JQH-answering-a-session.md)
- [Context: files, diffs and images](all-notes/01M68BT9B5R7ASMVGW9SHK8MB2-context-files-diffs-and-images.md)
- [Review, submit and come back later](all-notes/01M68BT9BMFVCVEAACWR03B827-review-submit-and-come-back-later.md)
- [The screen](all-notes/01M68BT9AJHW7WX304EQKGBGF6-the-screen.md), details of answering a session
- [Questions and answers](all-notes/01M68BT9AXJNJQR45K1FBR9VTG-questions-and-answers.md), details of answering a session
- [The context pane in detail](all-notes/01M68BT9BDTGM96WSRXRSYFZKY-the-context-pane-in-detail.md), details of the context pane
- [Less obvious keys](all-notes/01M68BT9BTS0J2VR1Y5N4NMEVC-less-obvious-keys.md), details of answering a session

### Your agent

- [What your agent can ask you](all-notes/01M68C6WV57K25ZD05875QJY73-what-your-agent-can-ask-you.md)
- [How an agent asks](all-notes/01M68C6WTNJGM2A5WY0C3EYKRT-how-an-agent-asks.md), details of what your agent writes and reads
- [The skill and other agents](all-notes/01M68C6WTY7EK8Q9RB1MCWSFM3-the-skill-and-other-agents.md), details of the skill

### Queues and settings

- [Queues](all-notes/01M68C1XV58STWHBH179QYXPZR-queues.md)
- [Settings](all-notes/01M68C1XVEMX1D38GK6GY0V1NZ-settings.md)
- [Finished sessions: history and replay](all-notes/01M68KBBY1Y1YB4YFQ8WC37QP5-finished-sessions-history-and-replay.md)
- [Notifications, the terminal and tmux](all-notes/01M68C1XVP8WV9Z6NWRJ1BEWT8-notifications-the-terminal-and-tmux.md), details of what asqr sends to your terminal

### Reference

- [Keys](all-notes/01M68AZYKWFPJAHTFZ0JPNNAK7-keys.md)
- [Commands](all-notes/01M68CASKFMEPMTN3JA6Z2DGCQ-commands.md)
- [Files, variables and the log](all-notes/01M68CASKVXFA54B7MJHTKFFGW-files-variables-and-the-log.md)
- [Keeping files, compact and prune](all-notes/01M68KBBYC1XNESEFMG0QW8GW1-keeping-files-compact-and-prune.md)
- [Limits](all-notes/01M68CASM4YTGM03NS3CKQS28G-limits.md)

## As a website

```sh
ntropy --vault docs/guide -n site -o dist --strict
```

exports the guide as a static website. The vault is edited with ntropy:
after changing a note's file by hand, `ntropy --vault docs/guide reconcile`
brings file names and links up to date.
