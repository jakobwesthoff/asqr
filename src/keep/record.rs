// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The archived session that records the files it showed (spec section
//! 5.1): its paths point at the kept copies, and each shown file gets a
//! `shown` object with the original path and a fingerprint.

use serde_json::{Map, Value, json};

use super::places::places_mut;
use super::processor::processor_for;
use super::snapshot::{FileBytes, Seen, Snapshot};
use super::{KeepFiles, sha256_hex};

/// A file to keep: its bytes, under the name they hash to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    /// `<sha256>.<extension>`, without the `files/` directory.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The session as it is archived once the files are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedSession {
    pub json: Vec<u8>,
    pub copies: Vec<Copy>,
}

/// Records `snapshot`, taken of the raw `session`, as `mode` keeps files.
/// Processing the files, an image's encoding above all, happens here.
///
/// A session that shows no file, or that is no JSON object, comes back as
/// it is: there is nothing to record, and a session without an `archived`
/// object is paired with its result by the hash of its own bytes.
pub fn record(session: &[u8], snapshot: &Snapshot, mode: KeepFiles) -> RecordedSession {
    let unchanged = || RecordedSession {
        json: session.to_vec(),
        copies: Vec::new(),
    };
    let Ok(mut raw) = serde_json::from_slice::<Value>(session) else {
        return unchanged();
    };
    let mut places = places_mut(&mut raw);
    let same_places = places.len() == snapshot.places.len()
        && places
            .iter()
            .zip(&snapshot.places)
            .all(|((_, object), seen)| object["path"] == seen.path.as_str());
    if places.is_empty() || !same_places {
        return unchanged();
    }

    let mut copies: Vec<Copy> = Vec::new();
    for ((kind, object), place) in places.iter_mut().zip(&snapshot.places) {
        let mut shown = Map::new();
        shown.insert("original_path".to_owned(), json!(place.path));
        match &place.seen {
            Seen::Read(file) if mode != KeepFiles::Off => {
                let processed = processor_for(*kind, mode).process(&file.bytes, &place.path);
                let name = format!("{}.{}", sha256_hex(&processed.bytes), processed.extension);
                shown.insert("state".to_owned(), json!("kept"));
                fingerprint(&mut shown, file);
                shown.insert("kept_size".to_owned(), json!(processed.bytes.len()));
                object["path"] = json!(format!("files/{name}"));
                if !copies.iter().any(|copy| copy.name == name) {
                    copies.push(Copy {
                        name,
                        bytes: processed.bytes,
                    });
                }
            }
            Seen::Read(file) => {
                shown.insert("state".to_owned(), json!("not_kept"));
                fingerprint(&mut shown, file);
            }
            Seen::TooLarge {
                size,
                modified,
                reason,
            } => {
                shown.insert("state".to_owned(), json!("not_kept"));
                shown.insert("size".to_owned(), json!(size));
                insert_modified(&mut shown, modified);
                shown.insert("reason".to_owned(), json!(reason));
            }
            Seen::Missing => {
                shown.insert("state".to_owned(), json!("missing"));
            }
            Seen::Unreadable(reason) => {
                shown.insert("state".to_owned(), json!("unreadable"));
                shown.insert("reason".to_owned(), json!(reason));
            }
            Seen::Deferred => {
                shown.insert("state".to_owned(), json!("unreadable"));
                shown.insert("reason".to_owned(), json!("not read in time"));
            }
        }
        object["shown"] = Value::Object(shown);
    }
    drop(places);

    raw["archived"] = json!({
        "session_sha256": sha256_hex(session),
        "keep": mode.to_string(),
    });
    let mut json = serde_json::to_vec_pretty(&raw).expect("a serde_json Value always serializes");
    json.push(b'\n');
    RecordedSession { json, copies }
}

/// The fields every file that was read has: the fingerprint of the
/// original bytes, their size and the file's modification time.
fn fingerprint(shown: &mut Map<String, Value>, file: &FileBytes) {
    shown.insert("sha256".to_owned(), json!(file.sha256));
    shown.insert("size".to_owned(), json!(file.bytes.len()));
    insert_modified(shown, &file.modified);
}

fn insert_modified(shown: &mut Map<String, Value>, modified: &Option<String>) {
    if let Some(modified) = modified {
        shown.insert("modified".to_owned(), json!(modified));
    }
}

/// The names under `files/` that the archived `session` refers to, each
/// once. Only names of the form `files/<64 lowercase hex>.<1 to 8 of
/// a-z0-9>` count, so a path written by hand never makes asqr open a file
/// outside the kept ones.
pub fn kept_file_names(session: &[u8]) -> Vec<String> {
    let Ok(mut raw) = serde_json::from_slice::<Value>(session) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for (_, object) in places_mut(&mut raw) {
        let name = object["path"]
            .as_str()
            .and_then(|path| path.strip_prefix("files/"))
            .filter(|name| is_kept_name(name));
        if let Some(name) = name
            && !names.iter().any(|known| known == name)
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// Whether `name` is `<64 lowercase hex>.<1 to 8 of a-z0-9>`, the name of
/// a kept file.
pub fn is_kept_name(name: &str) -> bool {
    let Some((hash, extension)) = name.split_once('.') else {
        return false;
    };
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && (1..=8).contains(&extension.len())
        && extension
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}
