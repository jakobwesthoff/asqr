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

- [The screen](all-notes/01M68BT9AJHW7WX304EQKGBGF6-the-screen.md)
- [Questions and answers](all-notes/01M68BT9AXJNJQR45K1FBR9VTG-questions-and-answers.md)
- [The context pane](all-notes/01M68BT9B5R7ASMVGW9SHK8MB2-the-context-pane.md)
- [Looking at an entry](all-notes/01M68BT9BDTGM96WSRXRSYFZKY-looking-at-an-entry.md)
- [Review and submit](all-notes/01M68BT9BMFVCVEAACWR03B827-review-and-submit.md)
- [Less obvious keys](all-notes/01M68BT9BTS0J2VR1Y5N4NMEVC-less-obvious-keys.md)

### Queues and settings

- [Queues](all-notes/01M68C1XV58STWHBH179QYXPZR-queues.md)
- [Settings](all-notes/01M68C1XVEMX1D38GK6GY0V1NZ-settings.md)
- [Notifications, the terminal and tmux](all-notes/01M68C1XVP8WV9Z6NWRJ1BEWT8-notifications-the-terminal-and-tmux.md)
- [Finished sessions: history and replay](all-notes/01M68KBBY1Y1YB4YFQ8WC37QP5-finished-sessions-history-and-replay.md)

### Agents

- [How an agent asks](all-notes/01M68C6WTNJGM2A5WY0C3EYKRT-how-an-agent-asks.md)
- [The skill and other agents](all-notes/01M68C6WTY7EK8Q9RB1MCWSFM3-the-skill-and-other-agents.md)
- [What agents can ask](all-notes/01M68C6WV57K25ZD05875QJY73-what-agents-can-ask.md)

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
