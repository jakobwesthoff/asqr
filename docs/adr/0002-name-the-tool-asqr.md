# 2. Name the tool asqr

Date: 2026-09-27

## Status

Accepted

## Context

The tool needs a short name that works as a crate, a binary and a repo,
and that is free in the registries where it may be published.

## Decision

The tool is called `asqr`, pronounced "asker". The name keeps the q of
question and queue and drops the vowel in the style of Web 2.0 names such
as Flickr.

A check on 2026-09-27 found `asqr` free on crates.io (also `asqr-cli`
and `asqr-core`), npm, PyPI and Homebrew. The only prior use is the
GitHub repository `mediocre-softworks/asqr`, a Java project with one star
and no commit since 2019. ASQR is also an aerospace and defence acronym,
which is unrelated to software tools.

Rejected: `askq`, which was also free and is easier to read, and
`askqr`, `asquer` and `askquer`, which are harder to spell or read like
"QR".

## Consequences

The README opens with the pronunciation, so nobody reads "QR" into the
name.
