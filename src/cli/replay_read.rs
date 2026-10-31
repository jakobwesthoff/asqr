// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr replay <ref> --json` (spec section 6.5): a finished session, its
//! result and the files it showed as data for an agent, where the terminal
//! UI is for the person.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::Exit;
use super::history::{finished_at, queue_label, status_json};
use super::plain::plain_line;
use crate::keep::places::places_mut;
use crate::queue::{BundleError, EntryRef, LoadedReplay, ReplayError, ReplayFile, load_replay};

/// A kept file read out of the archive: its name there and its bytes.
pub(super) type KeptCopy = (String, Vec<u8>);

/// Loads `entry` for reading. Nothing is written: the copies of the kept
/// files are checked against their hashes and, with `keep_copies`,
/// collected, each once. The session names them as `files/<name>`.
pub(super) fn load_for_reading(
    entry: &EntryRef,
    keep_copies: bool,
) -> Result<(LoadedReplay, Vec<KeptCopy>), ReplayError> {
    let mut copies: Vec<KeptCopy> = Vec::new();
    let loaded = load_replay(entry, &mut |copy_name, _, bytes| {
        if keep_copies && !copies.iter().any(|(name, _)| name == copy_name) {
            copies.push((copy_name.to_owned(), bytes.to_vec()));
        }
        Ok(PathBuf::from(format!("files/{copy_name}")))
    })?;
    Ok((loaded, copies))
}

/// What the command line asked `replay` to do with the session.
pub(super) struct ReadRequest<'a> {
    pub json: bool,
    pub extract: Option<&'a Path>,
    /// Directories the extraction must not lie in.
    pub keep_out: Vec<PathBuf>,
}

/// Reads the session out: prints the JSON object, writes the directory, or
/// both. A bundle that could not be read is warned about by the search, so
/// the pairing's warning for the same bundle is left out.
///
/// Nothing is written before the directory is allowed and the session has
/// loaded, so a refusal or an unreadable session leaves no trace.
pub(super) fn run_read(
    entry: &EntryRef,
    searched: &[BundleError],
    request: &ReadRequest<'_>,
) -> Exit {
    let target = match request.extract {
        None => None,
        Some(dir) => match prepare_target(dir, &request.keep_out) {
            Ok(target) => Some(target),
            Err(why) => {
                eprintln!("error: {why}");
                return Exit::Failure;
            }
        },
    };
    let (loaded, copies) = match load_for_reading(entry, target.is_some()) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("error: {error}");
            return Exit::Failure;
        }
    };
    let warned: Vec<String> = searched.iter().map(ToString::to_string).collect();
    for error in &loaded.unreadable {
        if !warned.contains(&error.to_string()) {
            eprintln!("warning: {error}");
        }
    }
    if let Some(target) = &target
        && let Err(error) = write_extraction(target, &loaded, &copies, &mut write_to_disk)
    {
        eprintln!(
            "error: cannot write {}: {error}",
            plain_line(&target.display().to_string())
        );
        return Exit::Failure;
    }
    if request.json {
        println!("{}", replay_json(&loaded, target.as_deref()));
    } else if let Some(target) = &target {
        print!("{}", summary(target, &loaded, &copies));
    }
    Exit::Success
}

/// Parses `--extract`: a directory name, which cannot be empty.
pub(super) fn parse_directory(input: &str) -> Result<PathBuf, String> {
    if input.is_empty() {
        Err("the directory must be named".to_owned())
    } else {
        Ok(PathBuf::from(input))
    }
}

// =========================================================
// Where an extraction may go
// =========================================================

/// The directory `dir` names, made absolute and without a trailing
/// separator, when a session may be extracted into it, else why not.
///
/// The directory is judged by where it really lies, symbolic links in the
/// directories above it resolved, so it cannot reach into `keep_out`
/// through one. A directory that is a link itself is refused whatever it
/// points at, since the staging directory is renamed onto the name and the
/// link would be replaced, not filled. The current directory and its
/// parents are refused for the same reason: the rename would leave the
/// caller's shell in a directory that no longer exists. The name that is
/// returned is the one given, not where links above it lead.
pub(super) fn prepare_target(dir: &Path, keep_out: &[PathBuf]) -> Result<PathBuf, String> {
    let shown = plain_line(&dir.display().to_string());
    let refuse = |why: &str| format!("cannot extract into {shown}: {why}");

    // A trailing separator would make the lookup of a link follow it, and
    // the components leave it out.
    let named: PathBuf = std::path::absolute(dir)
        .map_err(|error| refuse(&error.to_string()))?
        .components()
        .collect();
    let target = resolved(&named).map_err(|error| refuse(&error.to_string()))?;
    for forbidden in keep_out {
        let forbidden = resolved(forbidden).unwrap_or_else(|_| forbidden.clone());
        if target.starts_with(&forbidden) {
            return Err(refuse(
                "it lies inside a queue or the data directory of asqr",
            ));
        }
    }

    if let Ok(here) = std::env::current_dir().and_then(|here| here.canonicalize())
        && here.starts_with(&target)
    {
        return Err(refuse(
            "it is the current directory or one of its parents; name a new directory",
        ));
    }

    match std::fs::symlink_metadata(&named) {
        Ok(found) if found.is_symlink() => return Err(refuse("it is a symbolic link")),
        Ok(found) if !found.is_dir() => return Err(refuse("it is not a directory")),
        Ok(_) => {
            // A hidden file counts: the directory is for the session alone.
            let mut entries =
                std::fs::read_dir(&named).map_err(|error| refuse(&error.to_string()))?;
            if entries.next().is_some() {
                return Err(refuse("it is not empty"));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(refuse(&error.to_string())),
    }
    Ok(named)
}

/// `path` made absolute, with the links in the part that exists resolved
/// and the part that does not exist appended as written.
fn resolved(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut missing = Vec::new();
    let mut existing = absolute.as_path();
    loop {
        match existing.canonicalize() {
            Ok(mut real) => {
                real.extend(missing.iter().rev());
                return Ok(real);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let (Some(name), Some(parent)) = (existing.file_name(), existing.parent()) else {
                    return Err(error);
                };
                missing.push(name);
                existing = parent;
            }
            Err(error) => return Err(error),
        }
    }
}

// =========================================================
// Writing the directory
// =========================================================

/// What `write_extraction` writes a file with.
pub(super) type Sink<'a> = dyn FnMut(&Path, &[u8]) -> io::Result<()> + 'a;

/// The sink that writes to disk: a new file only the user can read.
pub(super) fn write_to_disk(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(bytes)
}

fn make_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// Writes the session, its result and the kept `copies` below `dir`.
///
/// Everything goes into a staging directory next to `dir`, which is renamed
/// onto `dir` when all is written. A failure removes the staging directory
/// and the parents this call created, so `dir` is either complete or as it
/// was. A race on an existing empty `dir` between the check and the rename
/// is accepted.
pub(super) fn write_extraction(
    dir: &Path,
    loaded: &LoadedReplay,
    copies: &[KeptCopy],
    sink: &mut Sink<'_>,
) -> io::Result<()> {
    let parent = dir
        .parent()
        .ok_or_else(|| io::Error::other("the directory has no parent"))?;
    let mut created: Vec<PathBuf> = Vec::new();
    let mut staging = None;

    let written = (|| {
        let mut missing = Vec::new();
        let mut above = parent;
        while !above.exists() {
            missing.push(above);
            above = above
                .parent()
                .ok_or_else(|| io::Error::other("no directory above the directory"))?;
        }
        for path in missing.into_iter().rev() {
            make_private_dir(path)?;
            created.push(path.to_path_buf());
        }

        let name = dir.file_name().unwrap_or_default().to_string_lossy();
        let stage = parent.join(format!(".{name}.asqr-{}", ulid::Ulid::generate()));
        make_private_dir(&stage)?;
        let stage = staging.insert(stage);
        make_private_dir(&stage.join("files"))?;

        sink(&stage.join("session.json"), &pretty(&loaded.raw))?;
        if let Some(result) = &loaded.result {
            let result = serde_json::to_value(result).expect("a result is plain data");
            sink(&stage.join("result.json"), &pretty(&result))?;
        }
        for (name, bytes) in copies {
            sink(&stage.join("files").join(name), bytes)?;
        }

        // Windows refuses to rename onto a directory, empty or not.
        #[cfg(not(unix))]
        if dir.exists() {
            std::fs::remove_dir(dir)?;
        }
        std::fs::rename(&*stage, dir)
    })();

    if written.is_err() {
        if let Some(stage) = &staging {
            let _ = std::fs::remove_dir_all(stage);
        }
        for path in created.iter().rev() {
            let _ = std::fs::remove_dir(path);
        }
    }
    written
}

fn pretty(value: &Value) -> Vec<u8> {
    let mut text = serde_json::to_vec_pretty(value).expect("JSON values serialise");
    text.push(b'\n');
    text
}

/// The lines `--extract` prints without `--json`: what was written, and
/// each file the session showed of which no copy was.
pub(super) fn summary(dir: &Path, loaded: &LoadedReplay, copies: &[KeptCopy]) -> String {
    let mut text = format!(
        "extracted to {}\nsession.json\n",
        plain_line(&dir.display().to_string())
    );
    text.push_str(if loaded.result.is_some() {
        "result.json\n"
    } else {
        "no result\n"
    });
    let noun = if copies.len() == 1 { "file" } else { "files" };
    text.push_str(&format!("{} {noun} in files/\n", copies.len()));
    for file in distinct_files(&loaded.shown)
        .into_iter()
        .filter(|file| !file.is_shown())
    {
        let why = file.damaged.as_deref().unwrap_or(state_word(file));
        text.push_str(&format!(
            "{}: {}\n",
            plain_line(&file.original_path),
            plain_line(why)
        ));
    }
    text
}

/// The object `replay --json` prints.
///
/// The session is the archived one as stored, so fields this asqr does not
/// know survive, but every file's `path` is the one the asker wrote: an
/// agent never sees the archive's names. The result is read through the
/// typed format, which leaves out what asqr does not know.
pub(super) fn replay_json(loaded: &LoadedReplay, extracted_to: Option<&Path>) -> Value {
    let entry = &loaded.entry;
    json!({
        "ref": entry.entry.reference(),
        "queue": queue_label(&entry.entry.queue),
        "id": entry.entry.id,
        "finished_at": finished_at(entry.finished),
        "status": status_json(entry.outcome),
        "draft": entry.has_draft,
        "session": session_with_original_paths(&loaded.raw),
        "result": loaded.result.as_ref().map(|result| {
            serde_json::to_value(result).expect("a result is plain data")
        }),
        "files": distinct_files(&loaded.shown)
            .into_iter()
            .map(|file| file_json(file, extracted_to.is_some()))
            .collect::<Vec<_>>(),
        "extracted_to": extracted_to.map(|dir| dir.to_string_lossy()),
    })
}

/// `raw` with the `path` of every place that records where its file lay
/// set to that original path.
fn session_with_original_paths(raw: &Value) -> Value {
    let mut session = raw.clone();
    for (_, place) in places_mut(&mut session) {
        let original = place["shown"]["original_path"].as_str().map(str::to_owned);
        if let Some(original) = original {
            place["path"] = Value::String(original);
        }
    }
    session
}

/// One file for each distinct original path and content, sorted by that
/// pair. A file shown at several places is one, and the first place that
/// names it decides what is said of it.
fn distinct_files(shown: &[ReplayFile]) -> Vec<&ReplayFile> {
    let mut distinct: Vec<&ReplayFile> = shown.iter().collect();
    distinct.sort_by(|a, b| (&a.original_path, &a.sha256).cmp(&(&b.original_path, &b.sha256)));
    distinct.dedup_by(|a, b| (&a.original_path, &a.sha256) == (&b.original_path, &b.sha256));
    distinct
}

/// The session records why a file is missing or unreadable, and both mean
/// the same to a reader: it was not there to keep.
fn state_word(file: &ReplayFile) -> &str {
    match file.state.as_str() {
        "missing" | "unreadable" => "gone",
        other => other,
    }
}

/// `extracting` says whether a directory was written, so that a good copy
/// has a place in it.
fn file_json(file: &ReplayFile, extracting: bool) -> Value {
    let extracted = file.copy.as_ref().filter(|_| extracting && file.is_shown());
    json!({
        "original_path": file.original_path,
        "state": state_word(file),
        "sha256": file.sha256,
        "size": file.size,
        "modified": file.modified,
        "kept_size": file.kept_size,
        "reason": file.reason,
        "problem": file.damaged,
        "extracted": extracted,
    })
}

#[cfg(test)]
mod tests;
