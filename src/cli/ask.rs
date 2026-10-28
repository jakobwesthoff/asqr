// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr ask` (spec sections 5.5 and 6.3): validate a session file, make it
//! ready for the inbox and drop it there, then optionally wait for the
//! result.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::Exit;
use super::format::{print_errors, print_warnings, read_session_file, session_dir};
use super::wait::wait_for_result;
use crate::format::{WarningKind, same_session_id, validate, warnings};
use crate::queue::{
    Dropped, Gate, QueueLocation, ReplaceError, WithdrawError, drop_replacing, drop_session,
    lock_holder,
};

pub(super) struct AskOptions {
    pub wait: bool,
    pub timeout: Option<Duration>,
    pub force: bool,
    /// The waiting session the new one takes the place of.
    pub replaces: Option<String>,
}

pub(super) fn run(location: &QueueLocation, file: &Path, options: AskOptions) -> Exit {
    let mut session_file = match read_session_file(file) {
        Ok(session_file) => session_file,
        Err(error) => return error.report(file),
    };

    // `ask` turns relative paths into absolute ones itself, so only the
    // other warnings concern the asker.
    let found = warnings(
        &session_file.raw,
        &session_file.session,
        session_dir(file).as_deref(),
    );
    print_warnings(
        found
            .iter()
            .filter(|warning| warning.kind != WarningKind::RelativePath),
    );
    if let Err(errors) = validate(&session_file.session, None) {
        print_errors(&errors);
        return Exit::ErrorResult;
    }

    let id = session_file
        .session
        .id
        .clone()
        .unwrap_or_else(|| ulid::Ulid::generate().to_string());
    let raw = &mut session_file.raw;
    raw["id"] = Value::from(id.as_str());
    make_paths_absolute(raw, file);
    let mut bytes = serde_json::to_vec_pretty(raw).expect("a serde_json Value always serializes");
    bytes.push(b'\n');

    let dropped = match options.replaces.as_deref() {
        None => drop_session(location, &id, &bytes, options.force).map_err(|error| {
            tracing::warn!(id, %error, "ask refused");
            eprintln!("error: {error}");
            Exit::Failure
        }),
        Some(old) => replace(location, old, &id, &bytes, options.force),
    };
    match dropped {
        Ok(dropped) => tracing::info!(id, ?dropped, "dropped session"),
        Err(exit) => return exit,
    }
    warn_if_nobody_watches(location);

    if options.wait {
        eprintln!("{id}");
        wait_for_result(location, &id, options.timeout)
    } else {
        println!("{id}");
        Exit::Success
    }
}

/// `ask --replaces <old>` (spec section 5.5): withdraws `old` and drops the
/// new session `id` in one step. Its refusals exit as those of `ask` and
/// `withdraw` do.
fn replace(
    location: &QueueLocation,
    old: &str,
    id: &str,
    bytes: &[u8],
    force: bool,
) -> Result<Dropped, Exit> {
    // `ask` replaces a waiting session of the same id by itself; naming it
    // again would withdraw the session the new file is to replace.
    if same_session_id(old, id) {
        eprintln!(
            "error: --replaces names the session's own id; ask replaces a waiting session of the same id without it"
        );
        return Err(Exit::Usage);
    }
    match drop_replacing(location, old, id, bytes, force, Gate::default()) {
        Ok((withdrawn, dropped)) => {
            tracing::info!(id, old, ?withdrawn, "replaced a session");
            Ok(dropped)
        }
        Err(error) => {
            tracing::warn!(id, old, %error, "ask --replaces refused");
            Err(match error {
                ReplaceError::Withdraw(
                    error @ (WithdrawError::ResultGone(_) | WithdrawError::Unknown(_)),
                ) => {
                    eprintln!("error: {error}");
                    Exit::UnknownId
                }
                ReplaceError::Withdraw(WithdrawError::Io(error)) => {
                    eprintln!("error: cannot withdraw {old}: {error}");
                    Exit::Failure
                }
                error => {
                    eprintln!("error: {error}");
                    Exit::Failure
                }
            })
        }
    }
}

/// Rewrites the relative paths of context entries against the directory
/// of `file`, since a relative path means nothing once the session sits in
/// an inbox. The raw JSON is edited, not the typed session, so unknown
/// fields survive.
fn make_paths_absolute(raw: &mut Value, file: &Path) {
    let base = file.parent().unwrap_or(Path::new(""));
    for question in array_mut(raw, "questions") {
        make_context_absolute(question, base);
        for option in array_mut(question, "options") {
            make_context_absolute(option, base);
        }
    }
}

/// The elements of the array in `field` of `object`, none when it is
/// missing. Indexing a `Value` mutably would insert a `null` for a missing
/// field, which the dropped file would then carry.
fn array_mut<'a>(object: &'a mut Value, field: &str) -> impl Iterator<Item = &'a mut Value> {
    object
        .get_mut(field)
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
}

/// Rewrites every path in the `context` of a question or an option: the
/// `path` of an image, code or Markdown entry and the paths of a diff's
/// sides.
fn make_context_absolute(holder: &mut Value, base: &Path) {
    for entry in array_mut(holder, "context") {
        make_path_absolute(entry, base);
        for side in ["old", "new"] {
            if let Some(side) = entry.get_mut(side) {
                make_path_absolute(side, base);
            }
        }
    }
}

/// Rewrites the `path` field of `object`, if it is there and relative.
fn make_path_absolute(object: &mut Value, base: &Path) {
    let Some(path) = object.get("path").and_then(Value::as_str) else {
        return;
    };
    let path = Path::new(path);
    if path.is_relative()
        && let Ok(absolute) = std::path::absolute(base.join(path))
    {
        object["path"] = Value::from(absolute.to_string_lossy().into_owned());
    }
}

/// The session queues either way, but the asker should tell the person to
/// start asqr (spec section 5.5).
fn warn_if_nobody_watches(location: &QueueLocation) {
    if matches!(lock_holder(location), Ok(None)) {
        let queue = location
            .name()
            .map_or_else(|| location.dir().display().to_string(), str::to_owned);
        eprintln!("warning: no asqr is watching queue {queue}; start `asqr` in another terminal");
    }
}
