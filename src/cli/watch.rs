// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr` and `asqr watch` (spec section 7): everything the TUI needs
//! before it gets the terminal.

use super::{Exit, WatchArgs};
use crate::queue::{LockError, QueueLocation, QueueLock, lock_queue};
use crate::tui::Alerts;
use crate::tui::render::PaneLayout;

/// A queue ready to be answered: its layout exists and this process holds
/// its lock, until the value is dropped.
#[derive(Debug)]
pub struct Watch {
    location: QueueLocation,
    alerts: Alerts,
    layout: PaneLayout,
    /// Whether asqr sets the terminal title; `--no-title` switches it off.
    title: bool,
    /// Whether the TUI draws without colours (`NO_COLOR`).
    no_color: bool,
    _lock: QueueLock,
}

impl Watch {
    pub fn location(&self) -> &QueueLocation {
        &self.location
    }

    pub fn alerts(&self) -> Alerts {
        self.alerts
    }

    pub fn layout(&self) -> PaneLayout {
        self.layout
    }

    pub fn title(&self) -> bool {
        self.title
    }

    pub fn no_color(&self) -> bool {
        self.no_color
    }
}

pub(super) fn run(
    location: QueueLocation,
    args: &WatchArgs,
    no_color: bool,
    terminal_ui: impl FnOnce(Watch) -> Exit,
) -> Exit {
    // The watcher needs the inbox to exist before the first file does.
    if let Err(error) = location.create_layout() {
        eprintln!(
            "error: cannot set up the queue in {}: {error}",
            location.dir().display()
        );
        return Exit::Failure;
    }
    let lock = match lock_queue(&location) {
        Ok(lock) => lock,
        Err(error @ LockError::Held(_)) => {
            eprintln!("error: {error}");
            return Exit::Failure;
        }
        Err(LockError::Io(error)) => {
            eprintln!("error: cannot lock the queue: {error}");
            return Exit::Failure;
        }
    };
    terminal_ui(Watch {
        location,
        alerts: Alerts {
            notify: !args.no_notify,
            bell: !args.no_bell,
        },
        layout: args.layout.unwrap_or_default(),
        title: !args.no_title,
        no_color,
        _lock: lock,
    })
}
