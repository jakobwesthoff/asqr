# 100. Keep the files a finished session showed

Date: 2026-10-31

## Status

Accepted

Amends [9. Crate choices](0009-crate-choices.md):
`libwebp-sys` encodes the images asqr keeps, and builds libwebp from C

Amends [92. Keep settings in a file](0092-keep-settings-in-a-file.md):
`keep_files` is a fourth setting, and `ASQR_KEEP_FILES` holds it

Amends [96. Bundle finished sessions and results per month in a zip](0096-bundle-finished-sessions-and-results-per-month-in-a-zip.md):
the `files/` prefix holds kept files, and the compaction moves them with
the sessions that show them

Amends [97. Delete only through prune --older-than with long units, after a dry run](0097-delete-only-through-prune-older-than-with-long-units-after-a-dry-run.md):
a queue whose archive holds only an empty `files/` directory counts as
empty

## Context

- A session shows files by path: images, Markdown, code, ANSI text and
  diff sides. The archive kept the session but not those files, and many
  of them lie in temporary folders. Of 541 file references in 267
  archived sessions of the default queue, 229 (42%) were gone four weeks
  after the oldest session.
- The 245 files still there held 182 MB. 120 of them were images (73 PNG,
  47 JPEG) of 181 MB. On 127 session images, WebP at quality 90 came to
  about 19% of the originals' size.
- No pure-Rust lossy WebP encoder has a licence asqr can use: `image-webp`,
  which `image` builds in, encodes lossless only, and `zenwebp` is
  AGPL-3.0 or commercial. Lossless WebP alone came to 57% of the
  originals.

## Decision

### Modes

`KeepFiles` has three modes:

- `compact` (the default): images are kept as WebP copies, every other
  file as it is.
- `exact`: every file is kept byte for byte, images included.
- `off`: no file is kept. The size and SHA-256 of each shown file are
  still recorded.

### Processors

Before a shown file is kept, a `Processor` chosen by the kind of the entry
that showed it (image, code, Markdown, ANSI, diff side) and the mode turns
its bytes into what is kept. Under `compact` an image goes to `WebpImages`;
every other kind, and every kind under `exact`, goes to `AsIs`. The
encoder sits behind the `ImageEncoder` trait, so another encoder or format
replaces one type. Rejected: the WebP call inlined in the archiving code.

### Re-encoding an image

- The format is read from the file's content, never its name. A WebP, a
  GIF and an animated PNG stay as they are, as does an image that is 0
  pixels or more than 16383 pixels wide or tall (libwebp's limit), and
  one whose decode needs more than 512 MiB.
- A picture of more than 32 Mpixel (32 x 1024 x 1024) stays as it is. The
  re-encoding holds the decoded picture at 4 bytes a pixel and, to
  compare, the decoded copy of the result, so 32 Mpixel is 256 MiB of
  pixels, the size of the budget for the files themselves. A 7680x4320
  picture (33.2 million pixels) is under the cap. A picture is converted
  to RGBA by moving its buffer where it can, not by copying it.
- Measured in a release build on a session with the six largest images of
  the 127-image corpus (31.4 MB): the keeping job peaked at 213 MB resident
  memory, against 256 MB before the job took its snapshot by value and
  converted in place.
- The encoder runs at quality 90. The result is decoded back and kept
  only if it has the original's dimensions, is smaller, and both the
  colour and the alpha channel have a PSNR above 30 dB against the
  original. Colour is compared after both pictures are laid over mid-grey
  (128), so the colour libwebp changes under a fully transparent pixel
  does not count. Alpha is compared on its own. Otherwise the original is
  kept.
- The wrapper returns libwebp's failure, such as a picture above its size
  limit, as an `Err`.

### Names

A kept file is named with the lowercased extension of its original name
when it is one to eight characters of `a-z0-9` and an image format
`image` knows, a syntax `two-face` highlights, or one of `txt md log diff
patch ansi` has it. Any other name gets `dat`. A re-encoded image gets
`webp`.

A kept file is named `<sha256>.<ext>` after the SHA-256 of its kept
bytes, so `shasum -a 256` checks it on any machine and a file shown by
several sessions is stored once per place. asqr already uses SHA-256 for
`session_sha256` in results. Rejected: BLAKE3 for the files, with or
without moving `session_sha256` to it. A reader accepts only names of the
form `files/<64 lowercase hex>.<1 to 8 of a-z0-9>`, so a path edited by
hand is never opened.

### Recording

The archived session is the JSON the person saw, edited as raw JSON so
unknown fields survive. It gains an `archived` object (the SHA-256 of the
session as it was in the inbox, which is what a result's `session_sha256`
names, and the mode) and a `shown` object on every entry or diff side
that names a file by path. `shown` holds the original path, a `state`
(`kept`, `not_kept`, `missing` or `unreadable`), the SHA-256, size and
modification time of the original, and for a kept file the size of the
copy. `path` points at the copy only where `state` is `kept`. The
fingerprint is recorded whether a copy is kept or not, so an archived
answer can still tell that a file is gone or changed. A session that
shows no file by path is archived as it is. The format is in spec
section 5.1.

Files are read the way the viewers read them: through
`regular_file::open`, which never waits for a FIFO or a device, and within
the viewers' limits (64 MiB for an image, 8 MiB for any other file). A
file over its limit, a missing file, a relative path or a file that
changes while it is read is recorded with its state and a reason, not
treated as an error. The snapshots of the jobs that wait for the
worker hold at most 256 MiB of file content together; a file that does not
fit what is left is not read then and is left to be read later by its path.

`asqr ask` removes `archived` and every `shown` from a session before it
enters the inbox, as asqr alone writes them.

### When files are read and kept

When the person finishes a session, the TUI thread reads every shown file into memory within the
limits above and hashes it, before the result is written, so an agent that
removes its temporary files after `asqr wait` returns cannot take them
away. At most 256 MiB is held across all jobs that wait for the worker,
and a job gives its share back as its files are done. A file that does not
fit what is left is read by the worker from its path, and an agent that
removes it first leaves it `missing`. Taking the share never waits, so
finishing a session never blocks the TUI thread. Under `off` no content is held at all: a file is hashed while it
is read, which `asqr withdraw` and `ask --replaces` do as well. The worker
handles one file at a time: it reads a file that was left to it when its
turn comes, processes it, writes its copy as a temp file and only then goes
on to the next, and the job gives the memory of a file back to the budget
as soon as that file is done. A job is consumed when it runs and a
processor takes the bytes of a file by value, so a file that is kept as it
is exists once in memory. Encoding and writing stay off the TUI thread: the result is
written, the session is archived by a plain rename and the draft deleted
as before, and then a job goes to one worker thread per run, which serves
every queue the run shows. Measured in a release build on the six largest
images of the 127-image corpus (31.4 MB, PNG and JPEG), reading and
hashing took 18 to 32 ms on the TUI thread before the result, and the
worker needed 3.0 s to encode them to 3.5 MB (11.2%).

The worker writes each copy as a synced temp file under
`archive/files/.asqr-*.tmp`. It then takes the drafts lock, checks that
the archived file still holds exactly the bytes it was archived with,
gives the copies their names and replaces the archived file with the
recording by one rename. The archived file therefore has two states,
plain and recorded. A copy lies under its name only once a recording that
refers to it was written, and placing the copies, the scans that look for
references and the removals all happen under the drafts lock. A busy
lock, a changed or missing archived file, a failing copy, a panic in a job
(caught and logged) or a crash leaves the plain session. What is lost then
are the copies and fingerprints of that session, and temp files that the
compaction's sweep removes. Quitting gives the terminal back, prints one
line on stderr that says how many finished sessions are still being kept,
and waits for the worker to run its queue empty for up to 10 seconds;
ctrl-c ends the wait. What is left is let go of, as a crash would. A
queue switch does not touch the worker, as a job carries its queue.

`asqr withdraw` and `ask --replaces` archive under their lock as before and
keep no copies, as only the asqr that shows the queue reads the settings.
After the lock is let go of they read the files and record each as
`not_kept`, then replace the archived file under the lock as the worker
does. A session completed by recovery gets a job whose files the worker
reads, as they may be gone. A session rejected as invalid is archived as
it is.

### Setting and variable

`keep_files` (`compact`, `exact` or `off`, default `compact`) is the
fourth setting of the settings file and the fourth row of the settings
pane, `Keep files`, with the values `compact copies`, `exact copies` and
`no copies`. A change applies to the next session the person finishes.
`ASQR_KEEP_FILES` sets it for the run and wins over the file, as
`ASQR_LAYOUT` does for `context`; the pane shows the row locked, dim and
marked `set by ASQR_KEEP_FILES`. An empty value counts as unset, and any
other value than the three is a usage error when asqr watches, as with
`ASQR_LAYOUT`. The other commands ignore the variable and the file.
Only the asqr that shows a queue reads the setting, which is why
`asqr withdraw` keeps no copies (above).

### Bundles and compaction

The compaction bundles the kept files with the sessions that show them,
as `files/<sha256>.<ext>` entries of the month of the session's ULID
(the prefix ADR 96 left for other kinds of entry). A bundle must be
readable alone, and `prune` deletes bundles whole, so a file shown in two
months is in both bundles.

- A file is streamed from `archive/files/` into the new bundle and hashed
  on the way. A file whose hash is not its name is not bundled; it stays
  loose and the compaction names it. Bundling it would seal the damage in
  and the loose copy would go.
- Before the bundle is built, `probe` looks up each file name in the
  existing bundle. A name the bundle holds with other content cannot be
  added, and one bundle build cannot leave a single session out, so the
  sessions that show that file are taken out of the build first and stay
  loose. A session whose file is missing or damaged on disk is bundled
  anyway, with the problem named in the output, as it still records the
  original path and the fingerprint of what was shown.
- Sessions an asqr without kept files bundled can show names the bundle
  lacks. The compaction adds those files when it rewrites that month.
  It never removes them afterwards.
- After the rename, under the drafts lock it holds, the compaction removes
  a loose kept file only if a session of this commit showed it and no
  loose session shows it. A file nobody shows (an orphan, left by a copy
  whose recording never landed) stays. `prune` removes those.
- The sweep of stale `.asqr-*.tmp` files covers `archive/files/`, where
  the worker writes its temp files.
- Measured in a release build: a month of 127 sessions that each show one
  of the 127 images of the corpus (191 MB, PNG and JPEG kept as they are,
  the size under `exact`) was bundled in 3.2 s with a peak of 11 MB of
  memory; the bundle is 197 MB, as Deflate does not shrink these images.

### libwebp

libwebp is C. `libwebp-sys` 0.14.4 builds libwebp 1.6.0 from the C
source that ships in the crate, with the `cc` crate, so a source build
needs a C compiler (the README says so). Its pre-generated bindings are
used, so building needs no libclang. asqr calls the encoder
(`WebPEncodeRGBA`) through a small wrapper, which copies the output into a
`Vec` and frees it with `WebPFree`; images are decoded through `image`.
The `webp` crate was not used: it pins `libwebp-sys` 0.9, which builds
libwebp 1.3.1, older than the release that fixed CVE-2023-4863. `check.yml`
builds both musl targets of the release with `--locked`.

Rejected: JPEG at quality 90 for opaque and lossless WebP for transparent
images (about 25% of the size, pure Rust); AVIF (12%, but `image` decodes
it only through dav1d, C); lossless WebP alone (57%); `libwebpop` 0.1.0,
released 2026-10-25, whose licence statement is unsure.
