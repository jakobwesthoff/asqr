# The asqr guide

This directory is the user guide of asqr, written as an
[ntropy](https://github.com/jakobwesthoff/ntropy) vault. The pages are the
notes in `all-notes/`, named `<id>-<title>.md`. The list below gives them in
reading order. The pictures are in `pictures/`.

## Pages

### Start

- [asqr](all-notes/01M68ASRHWD0MCGBJSFA6YXY6S-asqr.md)
- [Install and a first session](all-notes/01M68ASRJ527YVE41S9V50YXQW-install-and-a-first-session.md)

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

### Reference

- [Keys](all-notes/01M68AZYKWFPJAHTFZ0JPNNAK7-keys.md)

## As a website

```sh
ntropy --vault docs/guide -n site -o dist --strict
```

exports the guide as a static website. The vault is edited with ntropy:
after changing a note's file by hand, `ntropy --vault docs/guide reconcile`
brings file names and links up to date.
