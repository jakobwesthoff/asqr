---
title: "Files, variables and the log"
tags: [guide/reference]
site:
  order: 3
---
Where asqr keeps its files, which environment variables it reads and what
it logs.

## The queue

A queue is a directory:

```
<queue>/
  inbox/<id>.json                  sessions waiting, written by askers
  outbox/<id>.json                 results, written by asqr, owned by the asker
  drafts/<id>.json                 answers in progress
  archive/<id>.<ulid>.json         finished sessions
  archive/<YYYY-MM>.zip            the month bundle of older entries
  archive/files/<sha256>.<ext>     files a finished session showed, kept by asqr
  lock                             the instance lock
  drafts.lock                      the drafts lock
```

asqr creates the four directories on first use. Finished sessions and
results older than 30 days are compressed into one zip per month in
`archive/`. The asqr that starts watching a queue does this, and
`asqr compact` does it on request. Nothing is deleted automatically,
apart from drafts of sessions that were finished. `asqr prune` deletes
bundles and archive entries of old months, or one finished session, with
the kept files only they showed. The page on
[keeping files](01M68KBBYC1XNESEFMG0QW8GW1-keeping-files-compact-and-prune.md)
says which files asqr keeps.

The default root is `~/Library/Application Support/asqr/` on macOS and
`$XDG_DATA_HOME/asqr/` on Linux, usually `~/.local/share/asqr/`. A named
queue lives in `<root>/queues/<name>/`, and the default queue is
`default`. `asqr paths` prints the root, the queue, its four directories,
the lock file, the log file and the settings file. `asqr paths --json`
prints them as the keys `root`, `queue`, `inbox`, `outbox`, `drafts`,
`archive`, `lock`, `log` and `settings`.

## The settings file

`settings.toml` lives in the config directory. The [settings page](01M68C1XVEMX1D38GK6GY0V1NZ-settings.md)
lists its four keys and explains the pane that writes them.

## Environment variables

| Variable | Effect |
|---|---|
| `ASQR_QUEUE` | The named queue, like `--queue`. |
| `ASQR_DIR` | The queue directory, like `--dir`. |
| `ASQR_LAYOUT` | Where the context pane goes: `auto`, `beside` or `below`, like `--layout`. |
| `ASQR_KEEP_FILES` | What asqr keeps of the files a finished session showed: `compact`, `exact` or `off`. It wins over the settings file, and a set value skips the first-start screen. |
| `ASQR_LOG` | Which events reach the log. |
| `NO_COLOR` | Any value but the empty one makes the TUI draw without colours. |
| `VISUAL`, `EDITOR` | The editor that `ctrl-g` opens. |

An empty variable counts as unset. A flag wins over a variable, and
`ASQR_QUEUE` and `ASQR_DIR` together are a usage error. asqr also reads
`XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_CONFIG_HOME` on Linux, `TMPDIR`
for the files `o` writes, and variables that name the terminal and tmux.

## The log

Commands log the sessions they drop, finish, archive or prune into
`asqr.log` in the platform's cache directory, `~/Library/Caches/asqr/` on
macOS. Draft saves are not logged. The log holds no text you typed: a line
about a session names ids, status and counts, never answers, notes or
drafts.

`asqr.log` rotates at 1 MiB. A process that opens the log and finds it at
or above that size moves it to `asqr-<UTC timestamp>.log` and starts a new
`asqr.log`. A later rotation compresses a moved log as gzip when no process
holds it and it was last written at least 24 hours before. Compressed logs
are never removed.

`ASQR_LOG` sets which events reach the log. A level, such as
`ASQR_LOG=debug`, applies to every event. Comma-separated `target=level`
directives set the level per target, and `asqr` is the target of asqr's own
events: `info,asqr=debug` adds asqr's debug events and keeps the rest at
`info`. Without `ASQR_LOG` the log takes `info` and above. asqr does not
read `RUST_LOG`.
