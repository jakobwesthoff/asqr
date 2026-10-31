# The asqr guide

This directory is the user guide of asqr, written as an
[ntropy](https://github.com/jakobwesthoff/ntropy) vault. The pages are the
notes in `all-notes/`, named `<id>-<title>.md`. The list below gives them in
reading order. The pictures are in `pictures/`.

## Pages

### Start

- [asqr](all-notes/01M68ASRHWD0MCGBJSFA6YXY6S-asqr.md)
- [Install and a first session](all-notes/01M68ASRJ527YVE41S9V50YXQW-install-and-a-first-session.md)

### Reference

- [Keys](all-notes/01M68AZYKWFPJAHTFZ0JPNNAK7-keys.md)

## As a website

```sh
ntropy --vault docs/guide -n site -o dist --strict
```

exports the guide as a static website. The vault is edited with ntropy:
after changing a note's file by hand, `ntropy --vault docs/guide reconcile`
brings file names and links up to date.
