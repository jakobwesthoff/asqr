// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The commands about the file format: `asqr new`, `asqr validate` and
//! `asqr schema`, plus reading a session file the way `ask` needs it too.

use std::path::Path;

use serde_json::Value;

use super::Exit;
use crate::format::{
    Session, ValidationError, Warning, result_schema, session_schema, validate, warnings,
};

/// A session file as read from disk, in both forms the commands need: the
/// raw JSON, which keeps unknown fields, and the typed session.
pub(super) struct SessionFile {
    pub raw: Value,
    pub session: Session,
}

pub(super) enum ReadError {
    Io(std::io::Error),
    NotASession(serde_json::Error),
}

impl ReadError {
    /// Reports the error on stderr and returns the exit it leads to: an
    /// unreadable file is asqr's failure, a file that is not a session is
    /// the asker's error.
    pub fn report(&self, path: &Path) -> Exit {
        match self {
            ReadError::Io(error) => {
                eprintln!("error: cannot read {}: {error}", path.display());
                Exit::Failure
            }
            ReadError::NotASession(error) => {
                eprintln!("error: not a session file: {error}");
                Exit::ErrorResult
            }
        }
    }
}

pub(super) fn read_session_file(path: &Path) -> Result<SessionFile, ReadError> {
    let bytes = std::fs::read(path).map_err(ReadError::Io)?;
    let raw: Value = serde_json::from_slice(&bytes).map_err(ReadError::NotASession)?;
    let session = serde_json::from_value(raw.clone()).map_err(ReadError::NotASession)?;
    Ok(SessionFile { raw, session })
}

pub(super) fn print_errors(errors: &[ValidationError]) {
    for error in errors {
        eprintln!("error: {error}");
    }
}

pub(super) fn print_warnings<'a>(found: impl IntoIterator<Item = &'a Warning>) {
    for warning in found {
        eprintln!("warning: {warning}");
    }
}

pub(super) fn run_validate(path: &Path) -> Exit {
    let file = match read_session_file(path) {
        Ok(file) => file,
        Err(error) => return error.report(path),
    };
    print_warnings(&warnings(&file.raw, &file.session, path.parent()));
    match validate(&file.session, None) {
        Ok(()) => {
            println!("{}: valid", path.display());
            Exit::Success
        }
        Err(errors) => {
            print_errors(&errors);
            Exit::ErrorResult
        }
    }
}

/// A session to start from: one question of each kind, and a fresh ULID
/// as the id, so it is ready for `asqr ask` once the texts are filled in.
pub(super) fn run_new() -> Exit {
    let skeleton = serde_json::json!({
        "asqr": 1,
        "id": ulid::Ulid::generate().to_string(),
        "title": "What this session is about",
        "questions": [
            {
                "id": "choice",
                "text": "Which option?",
                "kind": "single",
                "options": [
                    { "id": "a", "label": "Option A", "description": "What A means." },
                    { "id": "b", "label": "Option B" }
                ],
                "custom": true
            },
            {
                "id": "several",
                "text": "Which of these?",
                "kind": "multi",
                "options": [
                    { "id": "x", "label": "X" },
                    { "id": "y", "label": "Y" }
                ]
            },
            {
                "id": "free",
                "text": "Anything else?",
                "kind": "text"
            }
        ]
    });
    println!("{}", pretty(&skeleton));
    Exit::Success
}

pub(super) fn run_schema(result: bool) -> Exit {
    let schema = if result {
        result_schema()
    } else {
        session_schema()
    };
    println!("{}", pretty(&schema));
    Exit::Success
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("a serde_json Value always serializes")
}
