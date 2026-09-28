// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr ask` (spec sections 3.5 and 8): validate a session file, make it
//! ready for the inbox and drop it there, then optionally wait for the
//! result.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::Exit;
use super::format::{print_errors, print_warnings, read_session_file};
use super::wait::wait_for_result;
use crate::format::{WarningKind, validate, warnings};
use crate::queue::{QueueLocation, drop_session, lock_holder};

pub(super) struct AskOptions {
    pub wait: bool,
    pub timeout: Option<Duration>,
    pub force: bool,
}

pub(super) fn run(location: &QueueLocation, file: &Path, options: AskOptions) -> Exit {
    let mut session_file = match read_session_file(file) {
        Ok(session_file) => session_file,
        Err(error) => return error.report(file),
    };

    // `ask` turns relative image paths into absolute ones itself, so only
    // the other warnings concern the asker.
    let found = warnings(&session_file.raw, &session_file.session, file.parent());
    print_warnings(
        found
            .iter()
            .filter(|warning| warning.kind != WarningKind::RelativeImagePath),
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
    make_images_absolute(raw, file);
    let mut bytes = serde_json::to_vec_pretty(raw).expect("a serde_json Value always serializes");
    bytes.push(b'\n');

    match drop_session(location, &id, &bytes, options.force) {
        Ok(dropped) => tracing::info!(id, ?dropped, "dropped session"),
        Err(error) => {
            tracing::warn!(id, %error, "ask refused");
            eprintln!("error: {error}");
            return Exit::Failure;
        }
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

/// Rewrites relative image paths against the directory of `file`, since a
/// relative path means nothing once the session sits in an inbox. The raw
/// JSON is edited, not the typed session, so unknown fields survive.
fn make_images_absolute(raw: &mut Value, file: &Path) {
    let base = file.parent().unwrap_or(Path::new(""));
    let questions = raw["questions"].as_array_mut().into_iter().flatten();
    for question in questions {
        make_image_absolute(question, base);
        let options = question["options"].as_array_mut().into_iter().flatten();
        for option in options {
            make_image_absolute(option, base);
        }
    }
}

/// Rewrites the `image` field of a question or an option, if it is there
/// and relative.
fn make_image_absolute(holder: &mut Value, base: &Path) {
    let Some(image) = holder["image"].as_str() else {
        return;
    };
    let image = Path::new(image);
    if image.is_relative()
        && let Ok(absolute) = std::path::absolute(base.join(image))
    {
        holder["image"] = Value::from(absolute.to_string_lossy().into_owned());
    }
}

/// The session queues either way, but the asker should tell the person to
/// start asqr (spec section 3.5).
fn warn_if_nobody_watches(location: &QueueLocation) {
    if matches!(lock_holder(location), Ok(None)) {
        let queue = location
            .name()
            .map_or_else(|| location.dir().display().to_string(), str::to_owned);
        eprintln!("warning: no asqr is watching queue {queue}; start `asqr` in another terminal");
    }
}
