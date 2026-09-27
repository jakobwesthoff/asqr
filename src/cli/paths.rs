// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr paths` (spec section 3.2): where the queue and its files are, so
//! agents and scripts need not know the platform rules.

use std::path::{Path, PathBuf};

use super::Exit;
use crate::queue::{QueueLocation, platform_cache_dir};

/// The log file in the platform cache directory (ADR 15).
pub(crate) fn log_file() -> Option<PathBuf> {
    platform_cache_dir().map(|dir| dir.join("asqr.log"))
}

pub(super) fn run(location: &QueueLocation, json: bool) -> Exit {
    let text = |path: Option<&Path>| {
        path.map_or_else(|| "none".to_owned(), |path| path.display().to_string())
    };
    let entries = [
        ("root", text(location.root())),
        ("queue", text(Some(location.dir()))),
        ("inbox", text(Some(&location.inbox()))),
        ("outbox", text(Some(&location.outbox()))),
        ("drafts", text(Some(&location.drafts()))),
        ("archive", text(Some(&location.archive()))),
        ("lock", text(Some(&location.lock_file()))),
        ("log", text(log_file().as_deref())),
    ];

    if json {
        // `null` for what does not exist, so a script can test for it.
        let object: serde_json::Map<_, _> = entries
            .into_iter()
            .map(|(key, value)| {
                let value = if value == "none" {
                    serde_json::Value::Null
                } else {
                    value.into()
                };
                (key.to_owned(), value)
            })
            .collect();
        println!("{}", serde_json::Value::Object(object));
    } else {
        for (key, value) in entries {
            println!("{key}: {value}");
        }
    }
    Exit::Success
}
