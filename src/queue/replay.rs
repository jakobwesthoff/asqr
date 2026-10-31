// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loading a finished session to show it again (ADR 101): the session as
//! the person saw it, the result that answers it and the files it showed,
//! put where the viewers can read them.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::PathBuf;

use serde_json::Value;
use thiserror::Error;

use super::bundle::{BundleError, BundleReader, EntryKind, EntryName, Month};
use super::history::{EntryPlace, EntryRef, HistoryEntry, paired_entry};
use crate::format::{
    Answer, MAX_IMAGE_BYTES, Session, SessionResult, Status, parse_session, validate,
};
use crate::keep::places::places_mut;
use crate::keep::record::is_kept_name;
use crate::keep::sha256_hex;
use crate::regular_file;

/// The state of a file whose path the archived session names without any
/// record of what it showed there.
pub const NOT_RECORDED: &str = "not_recorded";

/// What the archived session recorded about one file it showed, and what
/// became of the copy on load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayFile {
    /// Where the file lay when the session was finished.
    pub original_path: String,
    /// `kept`, `not_kept`, `missing`, `unreadable` or [`NOT_RECORDED`].
    pub state: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub modified: Option<String>,
    /// The size of the kept copy.
    pub kept_size: Option<u64>,
    /// Why the file is not kept, or could not be read.
    pub reason: Option<String>,
    /// Where the copy lay in the archive: `files/<name>`.
    pub copy: Option<String>,
    /// Why a kept file cannot be shown: its copy is gone or does not hash
    /// to its name.
    pub damaged: Option<String>,
}

impl ReplayFile {
    /// Whether a good copy of the file was written out on load, so that the
    /// path the session names it at holds what the session showed.
    pub fn is_shown(&self) -> bool {
        self.state == "kept" && self.damaged.is_none()
    }
}

/// A finished session ready to be shown.
#[derive(Debug, Clone)]
pub struct LoadedReplay {
    pub entry: HistoryEntry,
    /// The session with the paths of its kept files made absolute copies
    /// that outlive the archive.
    pub session: Session,
    pub result: Option<SessionResult>,
    /// The answers to fill the session with, in the form of a draft.
    pub draft: Option<SessionResult>,
    /// Each shown file, by the path the session names it at.
    pub files: HashMap<String, ReplayFile>,
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("{0}")]
    Bundle(#[from] BundleError),

    #[error("cannot read {place}: {source}")]
    Entry { place: String, source: io::Error },

    #[error("{place}: the archived session cannot be shown: {cause}")]
    Session { place: String, cause: String },
}

/// Loads the entry `entry`. `extract` writes a kept file under a name and
/// returns the absolute path it can be read at afterwards.
///
/// The copies are written out here, once, so the replay and the file `o`
/// hands to the opener do not depend on the archive afterwards, and a
/// bundle is not read again for every question. Only the files this
/// session shows are read.
pub fn load_replay(
    entry: &EntryRef,
    extract: &mut dyn FnMut(&str, &[u8]) -> io::Result<PathBuf>,
) -> Result<LoadedReplay, ReplayError> {
    let place = entry.place_text();
    let mut fetcher = Fetcher::new(entry);
    let bytes = fetcher.session()?;
    let (mut raw, _) = parse_session(&bytes).map_err(|error| ReplayError::Session {
        place: place.clone(),
        cause: error.to_string(),
    })?;

    let mut files = HashMap::new();
    for (_, object) in places_mut(&mut raw) {
        // A path without a record names a file asqr did not keep: showing
        // what lies there now would show something the session never did.
        let Some(shown) = object.get("shown").cloned() else {
            let path = object["path"].as_str().unwrap_or_default().to_owned();
            files.insert(
                path.clone(),
                ReplayFile {
                    original_path: path,
                    state: NOT_RECORDED.to_owned(),
                    sha256: None,
                    size: None,
                    modified: None,
                    kept_size: None,
                    reason: None,
                    copy: None,
                    damaged: None,
                },
            );
            continue;
        };
        let original = shown_text(&shown, "original_path")
            .or_else(|| object["path"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut file = ReplayFile {
            original_path: original.clone(),
            state: shown_text(&shown, "state").unwrap_or_default(),
            sha256: shown_text(&shown, "sha256"),
            size: shown["size"].as_u64(),
            modified: shown_text(&shown, "modified"),
            kept_size: shown["kept_size"].as_u64(),
            reason: shown_text(&shown, "reason"),
            copy: None,
            damaged: None,
        };
        if file.state == "kept" {
            let path = object["path"].as_str().unwrap_or_default().to_owned();
            file.copy = Some(path.clone());
            match path
                .strip_prefix("files/")
                .filter(|name| is_kept_name(name))
            {
                None => file.damaged = Some("the kept copy is not a name asqr keeps".to_owned()),
                Some(name) => match fetcher.file(name) {
                    Ok(copy) => {
                        let shown_as = shown_name(name, &original);
                        let at =
                            extract(&shown_as, &copy).map_err(|source| ReplayError::Entry {
                                place: format!("the scratch copy of {name}"),
                                source,
                            })?;
                        object["path"] = Value::String(at.to_string_lossy().into_owned());
                    }
                    Err(why) => file.damaged = Some(why),
                },
            }
        }
        let key = object["path"].as_str().unwrap_or_default().to_owned();
        files.insert(key, file);
    }

    let session: Session = serde_json::from_value(raw).map_err(|error| ReplayError::Session {
        place: place.clone(),
        cause: error.to_string(),
    })?;
    validate(&session, None).map_err(|errors| ReplayError::Session {
        place,
        cause: errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "),
    })?;

    let (paired, _) = paired_entry(entry).map_err(|source| ReplayError::Entry {
        place: entry.queue.dir().display().to_string(),
        source,
    })?;
    let draft = draft_of(&session, paired.result.as_ref());
    Ok(LoadedReplay {
        entry: paired.entry,
        session,
        result: paired.result,
        draft,
        files,
    })
}

fn shown_text(shown: &Value, field: &str) -> Option<String> {
    shown[field].as_str().map(str::to_owned)
}

/// The name a kept file is written out under. A file kept without an
/// extension asqr knows is named `.dat`, and the viewers pick a language
/// from the name, so the file goes out under the name it had.
fn shown_name(copy: &str, original_path: &str) -> String {
    let original = std::path::Path::new(original_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.chars().any(char::is_control));
    match original {
        Some(name) if copy.ends_with(".dat") => name.to_owned(),
        _ => copy.to_owned(),
    }
}

/// The answers a replay fills the session with: those of a result the
/// person sent or rejected, the draft a withdrawn result carries, and
/// nothing for the rest. A skipped answer is marked cleared, so a question
/// with a default does not show the default as picked.
fn draft_of(session: &Session, result: Option<&SessionResult>) -> Option<SessionResult> {
    let result = result?;
    let (answers, note, reason) = match result.status {
        Status::Submitted | Status::Cancelled => (
            result.answers.clone(),
            result.note.clone(),
            result.reason.clone(),
        ),
        Status::Withdrawn => {
            let unsent = result.draft.as_ref()?;
            (unsent.answers.clone(), unsent.note.clone(), None)
        }
        Status::Error | Status::Draft => return None,
    };
    let answers = answers
        .into_iter()
        .map(|answer| Answer {
            cleared: answer.cleared || answer.skipped,
            ..answer
        })
        .collect();
    let first = session
        .questions
        .first()
        .map_or("", |question| question.id.as_str());
    let mut draft = SessionResult::draft(&result.id, first, answers);
    draft.note = note;
    draft.reason = reason;
    Some(draft)
}

/// Reads what an entry refers to: its own session, and the kept files in
/// the loose directory or in the bundle of its month.
struct Fetcher<'a> {
    entry: &'a EntryRef,
    /// The bundle of the entry's month, opened on first use; `None` inside
    /// when it cannot be opened.
    bundle: Option<Option<BundleReader>>,
}

impl<'a> Fetcher<'a> {
    fn new(entry: &'a EntryRef) -> Self {
        Fetcher {
            entry,
            bundle: None,
        }
    }

    fn bundle(&mut self) -> Option<&mut BundleReader> {
        let location = &self.entry.queue;
        let month = Month::of(self.entry.ulid);
        let place = &self.entry.place;
        let month = match place {
            EntryPlace::Bundled(month, _) => *month,
            EntryPlace::Loose(_) => month,
        };
        self.bundle
            .get_or_insert_with(|| BundleReader::open(location, month).ok())
            .as_mut()
    }

    fn session(&mut self) -> Result<Vec<u8>, ReplayError> {
        match &self.entry.place {
            EntryPlace::Loose(path) => match std::fs::read(path) {
                Ok(bytes) => Ok(bytes),
                // The compaction moved it into its bundle meanwhile.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let name = EntryName {
                        kind: EntryKind::Session,
                        id: self.entry.id.clone(),
                        ulid: self.entry.ulid,
                    }
                    .zip_name();
                    let month = Month::of(self.entry.ulid);
                    Ok(BundleReader::open(&self.entry.queue, month)
                        .and_then(|mut reader| reader.read(&name))
                        .map_err(|_| ReplayError::Entry {
                            place: path.display().to_string(),
                            source: error,
                        })?)
                }
                Err(source) => Err(ReplayError::Entry {
                    place: path.display().to_string(),
                    source,
                }),
            },
            EntryPlace::Bundled(month, name) => {
                Ok(BundleReader::open(&self.entry.queue, *month)?.read(name)?)
            }
        }
    }

    /// The kept copy `name`, checked against the hash it is named by, or
    /// why it cannot be shown.
    fn file(&mut self, name: &str) -> Result<Vec<u8>, String> {
        let loose = self.entry.queue.kept_files().join(name);
        let bytes = match read_loose(&loose) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                let zip_name = format!("files/{name}");
                match self.bundle().map(|reader| reader.read(&zip_name)) {
                    Some(Ok(bytes)) => bytes,
                    Some(Err(BundleError::Zip {
                        source: zip::result::ZipError::FileNotFound,
                        ..
                    }))
                    | None => return Err("the kept copy is missing from the archive".to_owned()),
                    Some(Err(error)) => {
                        return Err(format!("the kept copy cannot be read: {error}"));
                    }
                }
            }
            Err(error) => return Err(format!("the kept copy cannot be read: {error}")),
        };
        let hash = name.split_once('.').map_or("", |(hash, _)| hash);
        if sha256_hex(&bytes) == hash {
            Ok(bytes)
        } else {
            Err("the kept copy is damaged".to_owned())
        }
    }
}

/// A loose kept file, `None` when it is not there. It is opened as a
/// regular file and read up to the most a kept image can be.
fn read_loose(path: &std::path::Path) -> io::Result<Option<Vec<u8>>> {
    let file = match regular_file::open(path) {
        Ok(file) => file,
        Err(regular_file::OpenError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(regular_file::OpenError::Io(error)) => return Err(error),
        Err(regular_file::OpenError::NotRegular { .. }) => {
            return Err(io::Error::other("not a regular file"));
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(io::Error::other("larger than a kept file can be"));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests;
