---
title: "Check the README against what asqr does today and add what is missing"
kind: docs
component: project
origin: review
---
# Check the README against what asqr does today and add what is missing

Many features landed on 2026-10-30 and 2026-10-31, each with its spec
sections and CHANGELOG entry, but the README was not part of every item.
Found 2026-10-31: the README names `asqr history`, `asqr replay` and the
`keep_files` setting, but not that asqr bundles sessions, results and
kept files older than 30 days into one zip per month, nor `asqr compact`
or `asqr prune` (`--older-than` with d/w/mo/y, a dry run unless `--yes`,
`--session <ref>`). Other features of these two days may be missing or
described in an older state the same way.

## What to do

- Read the README against `asqr --help`, the spec, the CHANGELOG's
  `[Unreleased]` section and the ADRs 89 to 101, and list every feature
  a person using asqr meets that the README omits or describes wrongly
  (for example the log rotation and where old logs go, the archive
  bundles, `compact`, `prune`, keeping files, history and replay, the
  settings, the `--dir` path form, the size limits of the viewers).
- Add what is missing in the README's own voice and length: the README
  is an overview that points to `asqr --help`, the spec and, once
  merged, the guide; it is not a second spec.
- Every sentence checked against the spec or the code
  (`validated-docs`).
- The user guide on its own branch also links from the README; leave
  that link to the guide's merge.
