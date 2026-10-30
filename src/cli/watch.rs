// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr` and `asqr watch` (spec section 7): everything the TUI needs
//! before it gets the terminal.

use std::path::{Path, PathBuf};

use super::{Exit, WatchArgs};
use crate::pairing::{self, Multiplexer, Tmux, TmuxPane};
use crate::queue::{Holder, LockError, Origin, QueueLocation, QueueLock, lock_queue};
use crate::settings::{Loaded, SettingsStore};
use crate::tui::render::{PaneLayout, ReadingWidth};
use crate::tui::{Alerts, QueueChoice, queue_choice};

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
    /// The settings of this run, with what a flag or a variable holds.
    settings: SettingsStore,
    /// What was wrong with the settings file (see [`Loaded::warnings`]).
    warnings: Vec<String>,
    /// Whether asqr sets the terminal title; `--no-title` switches it off.
    title: bool,
    /// Whether the TUI draws without colours (`NO_COLOR`).
    no_color: bool,
    /// The home folder, which the TUI shows as `~` in paths.
    home: Option<PathBuf>,
    /// The pin to set once the TUI runs, inside tmux on a named queue.
    pin: Option<Pin>,
    /// The tmux pane asqr runs in, which a lock names.
    tmux: Option<TmuxPane>,
    /// Whether tmux answered at the start, so a switch may pin.
    tmux_answered: bool,
    /// Held until the TUI's host takes it over.
    lock: Option<QueueLock>,
    /// The queues of the start picker, where the TUI picks its queue
    /// first and holds no lock yet (ADR 86).
    pick: Option<QueueChoice>,
}

impl Watch {
    pub fn location(&self) -> &QueueLocation {
        &self.location
    }

    pub fn alerts(&self) -> Alerts {
        self.alerts
    }

    /// Where the context goes: the setting, or what a flag or a variable
    /// holds over it.
    pub fn layout(&self) -> PaneLayout {
        self.settings.effective().context
    }

    /// The widest the question's text gets.
    pub fn reading_width(&self) -> ReadingWidth {
        self.settings.effective().max_question_width
    }

    pub fn settings(&self) -> &SettingsStore {
        &self.settings
    }

    /// What was wrong with the settings file, for the status line.
    pub fn settings_warnings(&self) -> &[String] {
        &self.warnings
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

    /// The tmux pane a lock taken for a switch names (ADR 85).
    pub fn lock_pane(&self) -> Option<&TmuxPane> {
        self.tmux.as_ref()
    }

    /// The tmux pane whose session a switch to a named queue pins it in:
    /// none outside tmux, and none once tmux did not answer at the start.
    pub fn pin_pane(&self) -> Option<&TmuxPane> {
        self.tmux
            .as_ref()
            .filter(|pane| self.tmux_answered && pane.pane.is_some())
    }

    /// The queue's lock, which the TUI's host takes over for the run, so a
    /// switch can let go of it.
    pub fn take_lock(&mut self) -> Option<QueueLock> {
        self.lock.take()
    }

    /// The queues of the start picker, where the TUI shows it.
    pub fn pick(&self) -> Option<&QueueChoice> {
        self.pick.as_ref()
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
    loaded: Loaded,
    no_color: bool,
    home: Option<PathBuf>,
    pairing: Pairing<'_>,
    terminal_ui: impl FnOnce(Watch) -> Exit,
) -> Exit {
    let pane = match pairing.multiplexer {
        Multiplexer::Tmux(pane) => Some(pane),
        Multiplexer::None => None,
    };
    let alerts = Alerts {
        notify: !args.no_notify,
        bell: !args.no_bell,
    };
    // Outside tmux, with nothing chosen, the person picks the queue when a
    // queue besides `default` is there to pick; the TUI takes its lock
    // then (ADR 86).
    let pick = match (pane, pairing.origin, location.root()) {
        (None, Origin::Default, Some(root)) => start_pick(root, pairing.tmux, home.as_deref()),
        _ => None,
    };
    if pick.is_some() {
        return terminal_ui(Watch {
            location,
            alerts,
            settings: loaded.store,
            warnings: loaded.warnings,
            title: !args.no_title,
            no_color,
            home,
            pin: None,
            tmux: None,
            tmux_answered: false,
            lock: None,
            pick,
        });
    }
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
        alerts,
        settings: loaded.store,
        warnings: loaded.warnings,
        title: !args.no_title,
        no_color,
        home,
        pin,
        tmux: pane.cloned(),
        tmux_answered: pairing.origin != Origin::Unreachable,
        lock: Some(lock),
        pick: None,
    })
}

/// The queues of the start picker, when `root` holds a queue besides
/// `default` that no asqr watches.
fn start_pick(root: &Path, tmux: &dyn Tmux, home: Option<&Path>) -> Option<QueueChoice> {
    let choice = queue_choice(Some(root), None, None, None, tmux, home);
    choice
        .rows
        .iter()
        .any(|row| !row.location.is_default())
        .then_some(choice)
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
