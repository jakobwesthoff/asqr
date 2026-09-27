// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The log file (ADR 15). The TUI owns the terminal, so everything worth
//! keeping goes into `asqr.log` in the platform cache directory. Logging
//! is a convenience: when the file cannot be opened, the command runs
//! without it.

use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Mutex;

use super::paths::log_file;

/// Sends `tracing` events of this process to the log file, appending.
pub(super) fn init() {
    if let Some(path) = log_file() {
        let _ = try_init(&path);
    }
}

fn try_init(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    // Only fails when a subscriber is set already, which then keeps
    // logging.
    let _ = tracing_subscriber::fmt()
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .try_init();
    Ok(())
}
