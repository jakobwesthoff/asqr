// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The terminal around the TUI: the only code that talks to the real
//! terminal, the operating system's image viewer and the inbox watcher's
//! thread. Everything it wires together is tested in the library; what is
//! left here needs a terminal and is kept to the wiring.

use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use anyhow::Context;
use asqr::cli::{Exit, Watch};
use asqr::tui::render::{Images, View, may_query_protocol};
use asqr::tui::{Alerts, Event, Host, alert, run, start, watch_inbox};
use ratatui::crossterm::event::{self, Event as TerminalEvent};
use ratatui_image::picker::Picker;

/// Runs the TUI on the queue `watch` holds, in this process's terminal.
pub fn run_terminal_ui(watch: Watch) -> Exit {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("error: asqr needs a terminal; run it in a terminal of its own");
        return Exit::Failure;
    }
    match run_in_terminal(&watch) {
        Ok(()) => Exit::Success,
        Err(error) => {
            eprintln!("error: {error:#}");
            Exit::Failure
        }
    }
}

fn run_in_terminal(watch: &Watch) -> anyhow::Result<()> {
    let location = watch.location();
    let (sender, events) = mpsc::channel();

    // The watcher comes first and the first scan second, so no file can
    // arrive unseen in between (spec section 3.4).
    let inbox_sender = sender.clone();
    let _watcher = watch_inbox(location, move || {
        let _ = inbox_sender.send(Event::InboxChanged);
    })
    .context("watching the inbox")?;
    let (mut app, mut inbox) = start(location.clone())?;

    // `try_init` also installs the panic hook that gives the terminal back
    // before a panic message is printed.
    let mut terminal = ratatui::try_init().context("setting up the terminal")?;

    // The protocol query reads the terminal's answers from stdin, so it
    // runs before the thread that reads key events starts.
    let tmux = std::env::var("TMUX").ok();
    let picker = if may_query_protocol(tmux.as_deref(), tmux_passthrough) {
        Picker::from_query_stdio().unwrap_or_else(|error| {
            tracing::info!(%error, "no graphics protocol detected; using half blocks");
            Picker::halfblocks()
        })
    } else {
        tracing::info!("tmux passes no escape sequences through; using half blocks");
        Picker::halfblocks()
    };
    let mut images = Images::new(picker);

    // The thread blocks in `read` for the life of the process; it ends
    // with it.
    std::thread::spawn(move || {
        while let Ok(terminal_event) = event::read() {
            let event = match terminal_event {
                TerminalEvent::Key(key) => Event::Key(key),
                TerminalEvent::Resize(..) => Event::Resize,
                _ => continue,
            };
            if sender.send(event).is_err() {
                break;
            }
        }
    });

    let queue = match location.name() {
        Some(name) => name.to_owned(),
        None => location.dir().display().to_string(),
    };
    let mut host = TerminalHost {
        alerts: watch.alerts(),
        term: std::env::var("TERM").ok(),
    };
    let result = run(
        &mut terminal,
        &mut app,
        &mut inbox,
        &mut images,
        &View { queue: &queue },
        &events,
        &mut host,
    );
    ratatui::restore();
    result
}

/// The pane's `allow-passthrough` option, inherited values included.
fn tmux_passthrough() -> Option<String> {
    let output = Command::new("tmux")
        .args(["show-options", "-Apv", "allow-passthrough"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

struct TerminalHost {
    alerts: Alerts,
    term: Option<String>,
}

impl Host for TerminalHost {
    fn alert(&mut self, arrived: &[String]) {
        let mut stdout = io::stdout().lock();
        if let Err(error) = alert(&mut stdout, self.alerts, self.term.as_deref(), arrived) {
            tracing::warn!(%error, "could not write the alert");
        }
        let _ = stdout.flush();
    }

    fn open_image(&mut self, path: &str) -> io::Result<()> {
        let viewer = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        // The viewer must not write into the TUI's screen.
        Command::new(viewer)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
}
