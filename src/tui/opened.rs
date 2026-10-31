// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The file `o` hands to the system's opener for the entry shown (spec
//! section 7.6, ADR 78): an entry's own file, or one asqr writes with the
//! entry's text.
//!
//! Written files go into a directory of their own for each run of asqr,
//! under one directory per user in the system's temp directory, and stay
//! after asqr quits: an editor or viewer that shows one would otherwise be
//! left with a deleted file, and an app may read its file only after asqr
//! quit. A start of asqr removes the directories of runs that ended a day
//! ago or longer.

use std::collections::HashMap;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use sha2::{Digest, Sha256};
use two_face::re_exports::syntect::parsing::SyntaxReference;

use super::render::{unified_text, unopenable};
use crate::format::{CodeEntry, ContextEntry, DiffEntry, MarkdownEntry};
use crate::syntaxes::SYNTAXES;

/// How long the files of a run that ended stay before a start of asqr
/// removes them. An app may still show such a file after asqr quit; the
/// lock already keeps the files of a run that goes on. The age counts from
/// the later of the run's last file and its end.
pub const KEEP_OPENED_FILES: Duration = Duration::from_secs(24 * 60 * 60);

/// The file in each run directory that the run holds an exclusive lock on
/// for its whole life, so another asqr's start sees that it goes on.
const LOCK: &str = "lock";

/// The prefix of the run directories under the base.
const RUN_PREFIX: &str = "run-";

/// How many characters of a title name a written file at most.
const NAME_CHARS: usize = 60;

/// The files asqr writes for the opener: a run directory of their own
/// under one directory per user in the system's temp directory, kept after
/// the run.
pub struct OpenedFiles {
    base: PathBuf,
    run: Option<Run>,
    /// How many directories the run holds one written file each in.
    count: usize,
    /// Each file written in this run, by its name and the SHA-256 of its
    /// bytes, so `o` on the same entry again opens the same file, as it
    /// does for an entry with a file of its own.
    written: HashMap<(String, [u8; 32]), PathBuf>,
}

/// The run directory and the lock the run holds on it.
struct Run {
    dir: PathBuf,
    // std opens files with close-on-exec, so the opener does not inherit
    // the lock and keep it after asqr quits.
    _lock: File,
}

impl Drop for Run {
    /// Sets the run directory's modification time to the run's end, so the
    /// day its files stay counts from then. A run that ends without this,
    /// as on a crash, counts from its last file.
    fn drop(&mut self) {
        let touched = File::open(&self.dir).and_then(|dir| dir.set_modified(SystemTime::now()));
        if let Err(error) = touched {
            tracing::warn!(%error, dir = %self.dir.display(), "could not mark the end of the run");
        }
    }
}

impl OpenedFiles {
    /// The files under the user's directory in the system's temp
    /// directory, after the directories of earlier runs that may go are
    /// removed.
    pub fn in_temp_dir() -> Self {
        Self::in_dir(base_dir())
    }

    /// The files under `base`, after the directories of earlier runs that
    /// may go are removed.
    pub fn in_dir(base: PathBuf) -> Self {
        let opened = Self::under(base);
        opened.sweep();
        opened
    }

    /// The files under `base`, which is made on the first file written.
    pub fn under(base: PathBuf) -> Self {
        OpenedFiles {
            base,
            run: None,
            count: 0,
            written: HashMap::new(),
        }
    }

    /// The file to hand to the opener for `entry`, and the name the status
    /// line gives it; or what the status line says instead.
    pub fn file_for(&mut self, entry: &ContextEntry) -> Result<(PathBuf, String), String> {
        match entry {
            ContextEntry::Image(image) => own_file(&image.path),
            ContextEntry::Code(CodeEntry {
                path: Some(path), ..
            })
            | ContextEntry::Markdown(MarkdownEntry {
                path: Some(path), ..
            }) => own_file(path),
            ContextEntry::Code(code) => {
                let content = code.content.as_deref().unwrap_or_default();
                self.write(code_file_name(code), content.as_bytes())
            }
            ContextEntry::Markdown(markdown) => {
                let content = markdown.content.as_deref().unwrap_or_default();
                self.write(markdown_file_name(markdown), content.as_bytes())
            }
            ContextEntry::Diff(diff) => {
                let (name, label) = diff_names(diff);
                let text = unified_text(diff, &label)
                    .map_err(|why| format!("cannot open {name}: {why}"))?;
                self.write(name, text.as_bytes())
            }
            // The session refuses `o` on an ANSI entry before it gets here.
            ContextEntry::Ansi(_) => Err("o does not open ANSI text".to_owned()),
        }
    }

    /// Writes a kept file of a replayed session under `name` in the run
    /// directory and returns its path, which stays readable after the
    /// archive changes and after asqr quits. The same name and bytes give
    /// the same file again.
    pub fn extract(&mut self, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let key = (name.to_owned(), Sha256::digest(bytes).into());
        if let Some(path) = self.written.get(&key).filter(|path| path.exists()) {
            return Ok(path.clone());
        }
        let path = self.write_new(name, bytes)?;
        self.written.insert(key, path.clone());
        Ok(path)
    }

    /// Writes `bytes` to a file named `name` in a directory of its own in
    /// the run directory, or finds the one written with them before.
    fn write(&mut self, name: String, bytes: &[u8]) -> Result<(PathBuf, String), String> {
        // A person who edited that file in their app gets their edits: the
        // app may still show it, and a second copy would leave two.
        match self.extract(&name, bytes) {
            Ok(path) => Ok((path, name)),
            Err(error) => Err(format!("cannot write {name} to open it: {error}")),
        }
    }

    fn write_new(&mut self, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let run = self.run_dir()?;
        self.count += 1;
        let dir = run.join(self.count.to_string());
        private_dir(&dir)?;
        let path = dir.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&path)?.write_all(bytes)?;
        Ok(path)
    }

    /// The run directory, made and locked on the first file written.
    fn run_dir(&mut self) -> io::Result<PathBuf> {
        if let Some(run) = &self.run {
            return Ok(run.dir.clone());
        }
        own_base(&self.base)?;
        let mut builder = tempfile::Builder::new();
        builder.prefix(RUN_PREFIX);
        #[cfg(unix)]
        builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
        let dir = builder.tempdir_in(&self.base)?.keep();
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(LOCK))?;
        // The directory's name is new, so no other process holds its lock.
        lock.lock()?;
        self.run = Some(Run {
            dir: dir.clone(),
            _lock: lock,
        });
        Ok(dir)
    }

    /// Removes the run directories under the base whose run ended more
    /// than [`KEEP_OPENED_FILES`] ago. Nothing here is the person's
    /// business, so failures only go to the log.
    pub(crate) fn sweep(&self) {
        let started = Instant::now();
        if let Err(error) = check_own(&self.base) {
            // Before the first file `o` writes, there is no base at all.
            if error.kind() != io::ErrorKind::NotFound {
                tracing::warn!(%error, "left the opened files' directory alone");
            }
            return;
        }
        let entries = match fs::read_dir(&self.base) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, "could not list the opened files' directory");
                return;
            }
        };
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with(RUN_PREFIX) {
                continue;
            }
            let dir = entry.path();
            if let Err(error) = sweep_run(&dir) {
                tracing::warn!(%error, dir = %dir.display(), "could not remove the files of an earlier run");
            }
        }
        tracing::debug!(took = ?started.elapsed(), "swept the opened files");
    }
}

/// Removes the run directory `dir` when its run ended more than
/// [`KEEP_OPENED_FILES`] ago: when its lock can be taken, or when it has
/// none. A run makes its directory before its lock file, so between the
/// two the directory has no lock; the age is checked first, and a
/// directory that young is never removed. The kernel releases a lock
/// however its process ends, so the files of a run that crashed go too.
fn sweep_run(dir: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(dir)?;
    if !metadata.is_dir() {
        return Ok(());
    }
    let age = metadata.modified()?.elapsed().unwrap_or_default();
    if age <= KEEP_OPENED_FILES {
        return Ok(());
    }
    // Opened for writing, without making it: some systems lock a file
    // only through a descriptor open for writing.
    let lock = match File::options().read(true).write(true).open(dir.join(LOCK)) {
        Ok(lock) => Some(lock),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Some(lock) = &lock {
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Ok(()),
            Err(fs::TryLockError::Error(error)) => return Err(error),
        }
    }
    fs::remove_dir_all(dir)
}

// ---------------------------------------------------------------------
// The directories
// ---------------------------------------------------------------------

/// The user's directory for written files in the system's temp
/// directory. A shared `/tmp` holds those of every user, so the name
/// carries the user's id.
fn base_dir() -> PathBuf {
    #[cfg(unix)]
    let name = format!("asqr-{}", rustix::process::getuid().as_raw());
    #[cfg(not(unix))]
    let name = "asqr".to_owned();
    std::env::temp_dir().join(name)
}

/// Makes `base`, or checks that the one there is asqr's own.
fn own_base(base: &Path) -> io::Result<()> {
    match private_dir(base) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => check_own(base),
        Err(error) => Err(error),
    }
}

/// Makes the directory `dir`, which only its owner may enter.
fn private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Refuses a `base` that another user could have made or may look into:
/// in a shared `/tmp` anyone can make the name first, or put a link there.
#[cfg(unix)]
fn check_own(base: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(base)?;
    let own = metadata.is_dir()
        && metadata.uid() == rustix::process::getuid().as_raw()
        && metadata.mode() & 0o077 == 0;
    if own {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} is not asqr's own directory",
            base.display()
        )))
    }
}

#[cfg(not(unix))]
fn check_own(base: &Path) -> io::Result<()> {
    let _ = base;
    Ok(())
}

// ---------------------------------------------------------------------
// The entry's own file
// ---------------------------------------------------------------------

/// The file at `path`, by the rule every viewer follows (spec section
/// 8.3), so `o` opens a path the pane can show and refuses one it shows a
/// card for, with the card's reason.
fn own_file(path: &str) -> Result<(PathBuf, String), String> {
    match unopenable(path) {
        None => Ok((PathBuf::from(path), path.to_owned())),
        Some(reason) => Err(format!("cannot open {path}: {reason}")),
    }
}

// ---------------------------------------------------------------------
// The names of written files
// ---------------------------------------------------------------------

/// `text` with every control character, line breaks and tabs included,
/// and every backslash turned into `_`, so it stays on one line of a diff's
/// header and names no other directory.
fn without_controls(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() || c == '\\' { '_' } else { c })
        .collect()
}

/// The last part of `title` as a file name: without controls, without
/// leading dots, at most [`NAME_CHARS`] characters. `None` when nothing is
/// left.
fn file_part(title: &str) -> Option<String> {
    let last = title.rsplit('/').next().unwrap_or_default();
    let cleaned = without_controls(last);
    let name: String = cleaned
        .trim_start_matches('.')
        .chars()
        .take(NAME_CHARS)
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The extension of the file name `name`, or the whole name when it has
/// none, as the code viewer reads it to pick a language.
fn kind_of(name: &str) -> &str {
    let path = Path::new(name);
    path.extension()
        .or_else(|| path.file_name())
        .and_then(|kind| kind.to_str())
        .unwrap_or(name)
}

fn listed(syntax: &SyntaxReference, kind: &str) -> bool {
    syntax
        .file_extensions
        .iter()
        .any(|listed| listed.eq_ignore_ascii_case(kind))
}

/// The name of the file for an inline code entry. A title the highlighting
/// knows by its extension, or by its whole name such as `Makefile`, stays as
/// written, where it fits the `language`. Otherwise the `language` gives the
/// extension, and what the highlighting does not know is text, so no
/// extension comes from text it does not know.
fn code_file_name(code: &CodeEntry) -> String {
    let title = code.title.as_deref().and_then(file_part);
    let language = code.language.as_deref().map(str::trim);
    let Some(language) = language.filter(|language| !language.is_empty()) else {
        return match title {
            Some(title) if SYNTAXES.find_syntax_by_extension(kind_of(&title)).is_some() => title,
            Some(title) => format!("{title}.txt"),
            None => "code.txt".to_owned(),
        };
    };
    let syntax = SYNTAXES
        .find_syntax_by_token(language)
        .or_else(|| SYNTAXES.find_syntax_by_name(language));
    if let (Some(title), Some(syntax)) = (&title, syntax)
        && listed(syntax, kind_of(title))
    {
        return title.clone();
    }
    let extension = syntax.and_then(|syntax| extension(language, syntax));
    let stem = title
        .as_deref()
        .and_then(|title| Path::new(title).file_stem()?.to_str())
        .unwrap_or("code");
    format!("{stem}.{}", extension.unwrap_or("txt"))
}

/// The extension a file in `language` gets: `language` itself where
/// `syntax` lists it, as `yml`; else the first of `syntax`'s extensions
/// that is one, as some list a whole file name first, such as
/// `CMakeLists.txt`.
fn extension<'a>(language: &'a str, syntax: &'a SyntaxReference) -> Option<&'a str> {
    if listed(syntax, language) {
        return Some(language);
    }
    syntax
        .file_extensions
        .iter()
        .map(String::as_str)
        .find(|extension| {
            extension
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+_-".contains(c))
        })
}

/// The name of the file for an inline Markdown entry: its title, with
/// `.md` unless it ends in it already.
fn markdown_file_name(markdown: &MarkdownEntry) -> String {
    let title = markdown.title.as_deref().and_then(file_part);
    match title {
        Some(title)
            if ["md", "markdown"].contains(&kind_of(&title).to_ascii_lowercase().as_str()) =>
        {
            title
        }
        Some(title) => format!("{title}.md"),
        None => "markdown.md".to_owned(),
    }
}

/// The name of a diff's file, after its `name` or `title`, and the label
/// its header gives the file the diff is about.
fn diff_names(diff: &DiffEntry) -> (String, String) {
    let named = diff.name.as_deref().or(diff.title.as_deref());
    let name = named
        .and_then(file_part)
        .unwrap_or_else(|| "diff".to_owned());
    let label = named
        .map(|named| without_controls(named).trim_start_matches('/').to_owned())
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| "file".to_owned());
    (format!("{name}.diff"), label)
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs::{self, File};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn an_extracted_file_is_written_once_in_the_run_directory() {
        let (_scratch, mut opened) = scratch();

        let first = opened.extract("Makefile", b"all:\n").expect("written");
        let again = opened.extract("Makefile", b"all:\n").expect("found");
        let other = opened.extract("Makefile", b"clean:\n").expect("written");

        assert_eq!(fs::read(&first).expect("readable"), b"all:\n");
        assert_eq!(first.file_name().expect("a name"), "Makefile");
        assert_eq!(first, again, "the same bytes give the same file");
        assert_ne!(
            first, other,
            "other bytes under one name get a file of their own"
        );
        assert!(first.is_absolute());
    }

    /// A scratch directory, and the files `o` writes under `asqr` in it.
    fn scratch() -> (tempfile::TempDir, OpenedFiles) {
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let opened = OpenedFiles::under(scratch.path().join("asqr"));
        (scratch, opened)
    }

    fn entry(json: &str) -> ContextEntry {
        serde_json::from_str(json).expect("test entry parses")
    }

    /// The file name `o` gives the entry `json` describes.
    fn written_name(json: &str) -> String {
        let (_scratch, mut opened) = scratch();
        let (path, name) = opened.file_for(&entry(json)).expect("written");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(name.as_str())
        );
        name
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("there").permissions().mode() & 0o777
    }

    /// Sets the modification time of the directory `dir` to `age` ago.
    fn age(dir: &Path, age: Duration) {
        File::open(dir)
            .and_then(|dir| dir.set_modified(SystemTime::now() - age))
            .expect("the time is set");
    }

    const TWO_DAYS: Duration = Duration::from_secs(2 * 24 * 60 * 60);

    /// The base under `scratch`, made as asqr makes it.
    fn private_base(scratch: &Path) -> PathBuf {
        let base = scratch.join("asqr");
        fs::create_dir(&base).expect("created");
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).expect("private");
        base
    }

    /// A run directory `name` under `base`, with an unlocked lock file where
    /// `lock` says so, last changed `old` ago.
    fn run_dir(base: &Path, name: &str, lock: bool, old: Duration) -> PathBuf {
        let dir = base.join(name);
        fs::create_dir_all(&dir).expect("created");
        if lock {
            fs::write(dir.join("lock"), "").expect("written");
        }
        age(&dir, old);
        dir
    }

    // ---------------------------------------------------------------------
    // Entries with a file of their own
    // ---------------------------------------------------------------------

    #[test]
    fn an_entry_with_a_file_opens_that_file_and_writes_nothing() {
        let (scratch, mut opened) = scratch();
        let file = scratch.path().join("plan.md");
        fs::write(&file, "# Plan").expect("written");
        let path = file.to_str().expect("UTF-8");

        for kind in ["image", "code", "markdown"] {
            let shown = entry(&format!(r#"{{"type": "{kind}", "path": "{path}"}}"#));

            assert_eq!(
                opened.file_for(&shown),
                Ok((file.clone(), path.to_owned())),
                "{kind}"
            );
        }
        assert!(!scratch.path().join("asqr").exists());
    }

    #[test]
    fn a_relative_path_is_not_opened() {
        let (_scratch, mut opened) = scratch();

        assert_eq!(
            opened.file_for(&entry(r#"{"type": "image", "path": "pics/a.png"}"#)),
            Err("cannot open pics/a.png: relative path".to_owned())
        );
    }

    #[test]
    fn a_missing_file_is_not_opened() {
        let (_scratch, mut opened) = scratch();

        let refused = opened.file_for(&entry(r#"{"type": "image", "path": "/nowhere/a.png"}"#));

        assert_eq!(
            refused,
            Err("cannot open /nowhere/a.png: No such file or directory (os error 2)".to_owned())
        );
    }

    #[test]
    fn a_directory_is_not_opened() {
        let (scratch, mut opened) = scratch();
        let dir = scratch.path().to_str().expect("UTF-8");

        assert_eq!(
            opened.file_for(&entry(&format!(r#"{{"type": "image", "path": "{dir}"}}"#))),
            Err(format!("cannot open {dir}: not a regular file"))
        );
    }

    #[test]
    fn an_ansi_entry_is_not_opened() {
        let (_scratch, mut opened) = scratch();

        assert_eq!(
            opened.file_for(&entry(r#"{"type": "ansi", "content": "x"}"#)),
            Err("o does not open ANSI text".to_owned())
        );
    }

    // ---------------------------------------------------------------------
    // Inline code
    // ---------------------------------------------------------------------

    #[test]
    fn inline_code_is_written_privately_with_its_language_s_extension() {
        let (scratch, mut opened) = scratch();

        let (path, name) = opened
            .file_for(&entry(
                r#"{"type": "code", "content": "fn main() {}\n", "language": "rust"}"#,
            ))
            .expect("written");

        assert_eq!(name, "code.rs");
        assert_eq!(fs::read(&path).expect("there"), b"fn main() {}\n");
        let n = path.parent().expect("its directory");
        let run = n.parent().expect("the run's directory");
        let base = run.parent().expect("the base");
        assert_eq!(base, scratch.path().join("asqr"));
        assert_eq!(
            [mode(&path), mode(n), mode(run), mode(base)],
            [0o600, 0o700, 0o700, 0o700]
        );
    }

    #[test]
    fn a_language_given_as_an_extension_keeps_it() {
        let json = r#"{"type": "code", "content": "a: 1", "language": "yml"}"#;
        assert_eq!(written_name(json), "code.yml");
    }

    #[test]
    fn a_language_takes_the_first_plain_extension_of_its_syntax() {
        let json = r#"{"type": "code", "content": "x", "language": "cmake"}"#;
        assert_eq!(written_name(json), "code.cmake");
    }

    #[test]
    fn a_syntax_whose_first_extension_is_a_file_name_takes_a_plain_one() {
        let json = r#"{"type": "code", "content": "FROM x", "language": "dockerfile"}"#;
        assert_eq!(written_name(json), "code.dockerfile");
    }

    #[test]
    fn an_unknown_language_writes_text() {
        let json = r#"{"type": "code", "content": "x", "language": "no-such-language"}"#;
        assert_eq!(written_name(json), "code.txt");
    }

    #[test]
    fn a_title_whose_extension_fits_the_language_stays_as_written() {
        let json = r#"{"type": "code", "content": "x", "language": "cpp", "title": "fetch.h"}"#;
        assert_eq!(written_name(json), "fetch.h");
    }

    #[test]
    fn a_title_without_the_language_s_extension_gets_it() {
        let json = r#"{"type": "code", "content": "x", "language": "rust", "title": "fetch"}"#;
        assert_eq!(written_name(json), "fetch.rs");
    }

    #[test]
    fn a_title_with_a_known_extension_names_the_file_as_written() {
        let json = r#"{"type": "code", "content": "x", "title": "fetch.h"}"#;
        assert_eq!(written_name(json), "fetch.h");
    }

    #[test]
    fn a_title_that_is_a_known_file_name_stays_as_written() {
        let json = r#"{"type": "code", "content": "all:", "title": "Makefile"}"#;
        assert_eq!(written_name(json), "Makefile");
    }

    #[test]
    fn a_title_without_a_known_kind_gets_text_s_extension() {
        let json = r#"{"type": "code", "content": "x", "title": "Release notes"}"#;
        assert_eq!(written_name(json), "Release notes.txt");
    }

    #[test]
    fn a_title_with_an_extension_no_syntax_knows_is_not_opened_as_its_type() {
        let json = r#"{"type": "code", "content": "x", "title": "x.webloc"}"#;
        assert_eq!(written_name(json), "x.webloc.txt");
    }

    #[test]
    fn code_without_title_or_language_is_text() {
        assert_eq!(
            written_name(r#"{"type": "code", "content": "x"}"#),
            "code.txt"
        );
    }

    #[test]
    fn a_title_names_one_file_in_the_run_s_directory() {
        let names = [
            (r#""../a/b""#, "b.txt"),
            (r#"".hidden""#, "hidden.txt"),
            (r#""a\tb""#, "a_b.txt"),
            (r#""a\\b""#, "a_b.txt"),
        ];
        for (title, name) in names {
            let json = format!(r#"{{"type": "code", "content": "x", "title": {title}}}"#);
            assert_eq!(written_name(&json), name, "{title}");
        }
        let long = "x".repeat(200);
        let json = format!(r#"{{"type": "code", "content": "x", "title": "{long}"}}"#);
        assert_eq!(written_name(&json), format!("{}.txt", "x".repeat(60)));
    }

    // ---------------------------------------------------------------------
    // Inline Markdown
    // ---------------------------------------------------------------------

    #[test]
    fn inline_markdown_is_written_as_md() {
        let (_scratch, mut opened) = scratch();

        let (path, name) = opened
            .file_for(&entry(
                r##"{"type": "markdown", "title": "Plan", "content": "# Plan"}"##,
            ))
            .expect("written");

        assert_eq!(name, "Plan.md");
        assert_eq!(fs::read_to_string(path).expect("there"), "# Plan");
    }

    #[test]
    fn a_markdown_title_that_ends_in_md_stays_as_written() {
        let json = r#"{"type": "markdown", "title": "plan.md", "content": "x"}"#;
        assert_eq!(written_name(json), "plan.md");
        assert_eq!(
            written_name(r#"{"type": "markdown", "content": "x"}"#),
            "markdown.md"
        );
    }

    // ---------------------------------------------------------------------
    // Diffs
    // ---------------------------------------------------------------------

    #[test]
    fn a_diff_is_written_as_a_unified_diff_named_after_its_file() {
        let (_scratch, mut opened) = scratch();

        let (path, name) = opened
            .file_for(&entry(
                r#"{"type": "diff", "name": "src/fetch.rs",
                    "old": {"content": "a\n"}, "new": {"content": "b\n"}}"#,
            ))
            .expect("written");

        assert_eq!(name, "fetch.rs.diff");
        assert_eq!(
            fs::read_to_string(path).expect("there"),
            "--- a/src/fetch.rs\n+++ b/src/fetch.rs\n@@ -1 +1 @@\n-a\n+b\n"
        );
    }

    #[test]
    fn a_name_with_a_line_break_stays_on_its_header_line() {
        let (_scratch, mut opened) = scratch();

        let (path, _) = opened
            .file_for(&entry(
                r#"{"type": "diff", "name": "a\nb.rs",
                    "old": {"content": "a\n"}, "new": {"content": "b\n"}}"#,
            ))
            .expect("written");

        let text = fs::read_to_string(path).expect("there");
        assert!(text.starts_with("--- a/a_b.rs\n+++ b/a_b.rs\n@@"), "{text}");
    }

    #[test]
    fn an_absolute_name_is_not_doubled_after_its_prefix() {
        let (_scratch, mut opened) = scratch();

        let (path, _) = opened
            .file_for(&entry(
                r#"{"type": "diff", "name": "/src/x.rs",
                    "old": {"content": "a\n"}, "new": {"content": "b\n"}}"#,
            ))
            .expect("written");

        let text = fs::read_to_string(path).expect("there");
        assert!(
            text.starts_with("--- a/src/x.rs\n+++ b/src/x.rs\n"),
            "{text}"
        );
    }

    #[test]
    fn a_diff_without_a_name_is_named_after_its_title_or_its_kind() {
        let json = r#"{"type": "diff", "title": "Fix", "new": {"content": "b\n"}}"#;
        assert_eq!(written_name(json), "Fix.diff");
        assert_eq!(
            written_name(r#"{"type": "diff", "new": {"content": "b\n"}}"#),
            "diff.diff"
        );
    }

    #[test]
    fn a_diff_whose_side_cannot_be_read_says_which() {
        let (_scratch, mut opened) = scratch();

        let refused = opened.file_for(&entry(
            r#"{"type": "diff", "name": "a.rs", "old": {"path": "/nowhere/a.rs"},
                "new": {"content": "b\n"}}"#,
        ));

        assert_eq!(
            refused,
            Err(
                "cannot open a.rs.diff: old side: No such file or directory (os error 2)"
                    .to_owned()
            )
        );
    }

    // ---------------------------------------------------------------------
    // Opening the same entry again
    // ---------------------------------------------------------------------

    #[test]
    fn the_same_entry_opens_the_file_written_for_it_before() {
        let (_scratch, mut opened) = scratch();
        let shown = entry(r#"{"type": "code", "content": "x", "language": "rust"}"#);

        let first = opened.file_for(&shown).expect("written");
        let second = opened.file_for(&shown).expect("written");

        assert_eq!(first, second);
        let run = first.0.parent().and_then(Path::parent).expect("the run");
        assert_eq!(
            fs::read_dir(run).expect("there").count(),
            2,
            "the lock and one directory"
        );
    }

    #[test]
    fn two_entries_of_one_name_get_a_file_each() {
        let (_scratch, mut opened) = scratch();

        let (first, _) = opened
            .file_for(&entry(
                r#"{"type": "code", "content": "a", "title": "x.rs"}"#,
            ))
            .expect("written");
        let (second, _) = opened
            .file_for(&entry(
                r#"{"type": "code", "content": "b", "title": "x.rs"}"#,
            ))
            .expect("written");

        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(first).expect("kept"), "a");
        assert_eq!(fs::read_to_string(second).expect("kept"), "b");
    }

    #[test]
    fn a_file_removed_since_is_written_again() {
        let (_scratch, mut opened) = scratch();
        let shown = entry(r#"{"type": "code", "content": "x"}"#);
        let (first, _) = opened.file_for(&shown).expect("written");
        fs::remove_file(&first).expect("removed");

        let (second, _) = opened.file_for(&shown).expect("written");

        assert_eq!(fs::read_to_string(second).expect("there"), "x");
    }

    // ---------------------------------------------------------------------
    // The directories
    // ---------------------------------------------------------------------

    #[test]
    fn the_files_stay_after_the_run_and_its_age_counts_from_its_end() {
        let (_scratch, mut opened) = scratch();
        let (path, _) = opened
            .file_for(&entry(r#"{"type": "code", "content": "x"}"#))
            .expect("written");
        let run = path
            .parent()
            .and_then(Path::parent)
            .expect("the run")
            .to_path_buf();
        age(&run, TWO_DAYS);

        drop(opened);

        assert!(path.exists());
        let modified = fs::metadata(&run)
            .and_then(|meta| meta.modified())
            .expect("a time");
        let since = modified.elapsed().unwrap_or_default();
        assert!(since < KEEP_OPENED_FILES, "{since:?}");
    }

    #[test]
    fn a_start_keeps_an_old_run_directory_whose_run_goes_on() {
        let (scratch, mut running) = scratch();
        let (path, _) = running
            .file_for(&entry(r#"{"type": "code", "content": "x"}"#))
            .expect("written");
        let run = path.parent().and_then(Path::parent).expect("the run");
        age(run, TWO_DAYS);

        OpenedFiles::under(scratch.path().join("asqr")).sweep();

        assert!(path.exists());
    }

    #[test]
    fn a_start_removes_an_old_run_directory_whose_lock_is_free() {
        let (scratch, starting) = scratch();
        let base = private_base(scratch.path());
        let ended = run_dir(&base, "run-ended", true, TWO_DAYS);

        starting.sweep();

        assert!(!ended.exists());
    }

    #[test]
    fn a_start_removes_an_old_run_directory_without_a_lock() {
        let (scratch, starting) = scratch();
        let base = private_base(scratch.path());
        let crashed = run_dir(&base, "run-crashed", false, TWO_DAYS);

        starting.sweep();

        assert!(!crashed.exists());
    }

    #[test]
    fn a_start_keeps_a_young_run_directory() {
        let (scratch, starting) = scratch();
        let base = private_base(scratch.path());
        let young = run_dir(&base, "run-young", true, Duration::ZERO);

        starting.sweep();

        assert!(young.exists());
    }

    #[test]
    fn a_start_leaves_what_is_no_run_directory_alone() {
        let (scratch, starting) = scratch();
        let base = private_base(scratch.path());
        let other = run_dir(&base, "other", false, TWO_DAYS);
        let target = run_dir(scratch.path(), "elsewhere", false, TWO_DAYS);
        let link = base.join("run-link");
        symlink(&target, &link).expect("linked");

        starting.sweep();

        assert!(other.exists());
        assert!(target.exists());
        assert!(fs::symlink_metadata(link).is_ok());
    }

    #[test]
    fn a_start_leaves_a_base_that_is_not_asqr_s_own_alone() {
        let (scratch, starting) = scratch();
        let base = private_base(scratch.path());
        let ended = run_dir(&base, "run-ended", true, TWO_DAYS);
        fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).expect("open");

        starting.sweep();

        assert!(ended.exists());
    }

    /// Sets the permission bits of `path` to `mode`.
    fn chmod(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("changed");
    }

    /// The sweep runs as the TUI starts, so a base it cannot list only
    /// goes to the log; a sweep that failed on it would panic here.
    #[test]
    fn a_base_the_start_cannot_list_does_not_stop_the_start() {
        let (scratch, starting) = scratch();
        let base = private_base(scratch.path());
        let ended = run_dir(&base, "run-ended", true, TWO_DAYS);
        chmod(&base, 0o300);

        starting.sweep();

        chmod(&base, 0o700);
        assert!(ended.exists());
    }

    #[test]
    fn a_run_directory_whose_lock_cannot_be_opened_stays() {
        let (scratch, starting) = scratch();
        let base = private_base(scratch.path());
        let ended = run_dir(&base, "run-ended", true, TWO_DAYS);
        chmod(&ended.join("lock"), 0o000);

        starting.sweep();

        assert!(ended.exists());
    }

    #[test]
    fn a_base_that_is_a_link_is_not_written_into() {
        let (scratch, mut opened) = scratch();
        let elsewhere = scratch.path().join("elsewhere");
        fs::create_dir(&elsewhere).expect("created");
        fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o700)).expect("private");
        let base = scratch.path().join("asqr");
        symlink(&elsewhere, &base).expect("linked");

        let refused = opened.file_for(&entry(r#"{"type": "code", "content": "x"}"#));

        assert_eq!(
            refused,
            Err(format!(
                "cannot write code.txt to open it: {} is not asqr's own directory",
                base.display()
            ))
        );
        assert_eq!(fs::read_dir(elsewhere).expect("there").count(), 0);
    }

    #[test]
    fn a_base_others_may_read_is_not_written_into() {
        let (scratch, mut opened) = scratch();
        let base = scratch.path().join("asqr");
        fs::create_dir(&base).expect("created");
        fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).expect("open");

        let refused = opened.file_for(&entry(r#"{"type": "code", "content": "x"}"#));

        assert_eq!(
            refused,
            Err(format!(
                "cannot write code.txt to open it: {} is not asqr's own directory",
                base.display()
            ))
        );
        assert_eq!(fs::read_dir(base).expect("there").count(), 0);
    }

    #[test]
    fn a_base_that_cannot_be_made_is_named() {
        let (scratch, _) = scratch();
        let file = scratch.path().join("a-file");
        fs::write(&file, "").expect("written");
        let mut opened = OpenedFiles::under(file.join("asqr"));

        let refused = opened.file_for(&entry(r#"{"type": "code", "content": "x"}"#));

        let refused = refused.expect_err("no directory under a file");
        assert!(
            refused.starts_with("cannot write code.txt to open it: "),
            "{refused}"
        );
    }
}
