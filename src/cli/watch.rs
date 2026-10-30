// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr` and `asqr watch` (spec section 7): everything the TUI needs
//! before it gets the terminal.

use std::path::{Path, PathBuf};

use super::{Exit, WatchArgs};
use crate::pairing::{self, Multiplexer, Tmux, TmuxPane};
use crate::queue::{Holder, LockError, Origin, QueueLocation, QueueLock, lock_queue};
use crate::tui::Alerts;
use crate::tui::render::PaneLayout;

/// The queue a TUI pins in its tmux session once it runs (ADR 85).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub pane: TmuxPane,
    pub queue: String,
}

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
    /// The home folder, which the TUI shows as `~` in paths.
    home: Option<PathBuf>,
    /// The pin to set once the TUI runs, inside tmux on a named queue.
    pin: Option<Pin>,
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

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// The queue to pin in the tmux session once the TUI runs. A start
    /// that fails before, as without a terminal, leaves the session's pin
    /// as it was, so its agents do not move to a queue nobody watches.
    pub fn pin(&self) -> Option<&Pin> {
        self.pin.as_ref()
    }
}

/// Where the TUI runs and how it got its queue, for the pane line of the
/// lock and the pin.
pub(super) struct Pairing<'a> {
    pub multiplexer: &'a Multiplexer,
    pub origin: Origin,
    pub tmux: &'a dyn Tmux,
}

pub(super) fn run(
    location: QueueLocation,
    args: &WatchArgs,
    no_color: bool,
    home: Option<PathBuf>,
    pairing: Pairing<'_>,
    terminal_ui: impl FnOnce(Watch) -> Exit,
) -> Exit {
    let pane = match pairing.multiplexer {
        Multiplexer::Tmux(pane) => Some(pane),
        Multiplexer::None => None,
    };
    // The lock comes before the layout: `prune` removes an unwatched
    // queue, and once this process holds the lock, the queue is watched.
    let lock = match lock_queue(&location, pane) {
        Ok(lock) => lock,
        Err(LockError::Held(holder)) => {
            eprintln!(
                "error: another asqr is watching this queue ({})",
                described(&holder, pairing.tmux)
            );
            return Exit::Failure;
        }
        Err(LockError::Io(error)) => {
            eprintln!("error: cannot lock the queue: {error}");
            return Exit::Failure;
        }
    };
    // The watcher needs the inbox to exist before the first file does.
    if let Err(error) = location.create_layout() {
        eprintln!(
            "error: cannot set up the queue in {}: {error}",
            location.dir().display()
        );
        return Exit::Failure;
    }
    // Only a named queue is pinned, and not after tmux failed to answer,
    // when setting the pin would only wait on it again.
    let pin = match (pane, location.name(), pairing.origin) {
        (_, _, Origin::Unreachable) => None,
        (Some(pane), Some(name), _) if pane.pane.is_some() => Some(Pin {
            pane: pane.clone(),
            queue: name.to_owned(),
        }),
        _ => None,
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
        home,
        pin,
        _lock: lock,
    })
}

/// The holder of a lock as a refusal names it: its pid and host, and its
/// tmux session where tmux still knows the pane.
fn described(holder: &Holder, tmux: &dyn Tmux) -> String {
    let session = holder.tmux.as_ref().and_then(|place| {
        pairing::sessions_of_panes(tmux, &place.socket)
            .ok()?
            .remove(&place.pane)
    });
    match session {
        Some(session) => format!("{}, tmux session {session}", holder.text),
        None => holder.text.clone(),
    }
}
