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
#[cfg(unix)]
use asqr::tui::render::passthrough_answers;
use asqr::tui::render::{TerminalNames, View, may_query_protocol};
use asqr::tui::{AlertTerminal, Alerts, Event, Host, alert, run, start, watch_inbox};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    Event as TerminalEvent,
};
use ratatui::crossterm::{execute, terminal};
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
    // before a panic message is printed. That hook knows nothing of focus
    // reports and bracketed paste, so the guard turns those off too, on
    // every way out of this function from here on.
    let mut terminal = ratatui::try_init().context("setting up the terminal")?;
    let _guard = TerminalGuard {
        out: io::stdout(),
        restore: ratatui::restore,
    };

    // The protocol query reads the terminal's answers from stdin, so it
    // runs before the thread that reads key events starts.
    let tmux = std::env::var("TMUX").ok();
    let term = std::env::var("TERM").ok();
    let term_program = std::env::var("TERM_PROGRAM").ok();
    let names = TerminalNames {
        tmux: tmux.as_deref(),
        term: term.as_deref(),
        term_program: term_program.as_deref(),
    };
    let picker = if may_query_protocol(&names, tmux_state, probe_passthrough) {
        Picker::from_query_stdio().unwrap_or_else(|error| {
            tracing::info!(%error, "no graphics protocol detected; using half blocks");
            Picker::halfblocks()
        })
    } else {
        tracing::info!("tmux or screen cannot forward the graphics query; using half blocks");
        Picker::halfblocks()
    };
    // Images are sized with this cell size for the whole run, so a report
    // of a cropped or shrunken image starts by comparing it with the cell
    // size the terminal reports on a resize.
    let font_size = picker.font_size();
    tracing::info!(
        protocol = ?picker.protocol_type(),
        cell = %format!("{}x{}", font_size.width, font_size.height),
        "graphics protocol detected"
    );
    log_window_size();

    // Focus reports tell asqr when it comes back into view, so it can send
    // images again whose transfer tmux dropped while it was hidden (spec
    // section 7.7). tmux forwards them with its `focus-events` option.
    // Bracketed paste sends a paste as one event, so a line break in it is
    // no `enter` that picks, submits or rejects (spec section 7.3). The TUI
    // works without either, and a legacy Windows console refuses bracketed
    // paste, so a refusal is logged and the run goes on.
    if let Err(error) = execute!(io::stdout(), EnableFocusChange) {
        tracing::warn!(%error, "could not ask for focus reports");
    }
    if let Err(error) = execute!(io::stdout(), EnableBracketedPaste) {
        tracing::warn!(%error, "could not ask for bracketed paste");
    }

    // The thread blocks in `read` for the life of the process; it ends
    // with it.
    std::thread::spawn(move || {
        while let Ok(terminal_event) = event::read() {
            let event = match terminal_event {
                TerminalEvent::Key(key) => Event::Key(key),
                TerminalEvent::Resize(..) => {
                    log_window_size();
                    Event::Resize
                }
                TerminalEvent::FocusGained => Event::FocusGained,
                TerminalEvent::Paste(text) => Event::Paste(text),
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
        terminal: AlertTerminal {
            term,
            tmux: tmux.is_some_and(|tmux| !tmux.is_empty()),
        },
    };
    run(
        &mut terminal,
        &mut app,
        &mut inbox,
        &mut View::new(&queue, watch.layout(), picker),
        &events,
        &mut host,
    )
}

/// Gives the terminal back when dropped: turns focus reports and bracketed
/// paste off, then calls `restore`, which leaves raw mode and the
/// alternate screen.
///
/// The guard covers a panic only because a panic unwinds and drops it. A
/// build with `panic = "abort"` would skip the drop and leave both modes
/// on in the shell.
struct TerminalGuard<W: Write> {
    out: W,
    /// [`ratatui::restore`]; tests pass a function that only notes the call.
    restore: fn(),
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        // Left on, the shell would receive the reports as typed text, and
        // its pastes wrapped in the markers.
        if let Err(error) = execute!(self.out, DisableFocusChange) {
            tracing::warn!(%error, "could not turn focus reports off");
        }
        if let Err(error) = execute!(self.out, DisableBracketedPaste) {
            tracing::warn!(%error, "could not turn bracketed paste off");
        }
        (self.restore)();
    }
}

/// Logs the terminal's size in cells and pixels, and the cell size they
/// give, to set against the cell size the graphics query found at start.
/// Terminals that report no pixel size log zeros.
fn log_window_size() {
    match terminal::window_size() {
        Ok(size) => {
            let cell = if size.columns > 0 && size.rows > 0 {
                format!("{}x{}", size.width / size.columns, size.height / size.rows)
            } else {
                "unknown".to_owned()
            };
            tracing::info!(
                cells = %format!("{}x{}", size.columns, size.rows),
                pixels = %format!("{}x{}", size.width, size.height),
                %cell,
                "terminal size"
            );
        }
        Err(error) => tracing::info!(%error, "terminal size unknown"),
    }
}

/// What [`may_query_protocol`] needs to know from tmux.
fn tmux_state() -> Option<String> {
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "#{allow-passthrough} #{session_attached}",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// How long [`probe_passthrough`] waits for the answer: long enough for a
/// round trip over ssh, short enough not to hold up the start noticeably.
#[cfg(unix)]
const PROBE_PATIENCE: std::time::Duration = std::time::Duration::from_millis(300);

/// Whether a sequence sent through tmux's passthrough gets an answer, for
/// [`may_query_protocol`] in a tmux or screen it cannot ask. Raw mode is
/// on, as `ratatui::try_init` turned it on.
#[cfg(unix)]
fn probe_passthrough() -> bool {
    passthrough_answers(&mut io::stdout(), io::stdin(), PROBE_PATIENCE).unwrap_or_else(|error| {
        tracing::warn!(%error, "could not probe tmux passthrough");
        false
    })
}

/// Without `poll` on stdin, a probe could leave a read blocked, so the
/// query is skipped.
#[cfg(not(unix))]
fn probe_passthrough() -> bool {
    false
}

struct TerminalHost {
    alerts: Alerts,
    terminal: AlertTerminal,
}

impl Host for TerminalHost {
    fn alert(&mut self, arrived: &[String]) {
        let mut stdout = io::stdout().lock();
        if let Err(error) = alert(&mut stdout, self.alerts, &self.terminal, arrived) {
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static RESTORED: Cell<bool> = const { Cell::new(false) };
    }

    fn note_restore() {
        RESTORED.set(true);
    }

    // The panic path itself is not run here: it needs a real terminal and
    // a panicking TUI. What it relies on is that unwinding drops the guard,
    // and the drop is what this test runs.
    #[test]
    fn dropping_the_guard_turns_paste_and_focus_reports_off_and_restores() {
        let mut written = Vec::new();
        drop(TerminalGuard {
            out: &mut written,
            restore: note_restore,
        });
        let written = String::from_utf8(written).expect("escape sequences are ASCII");
        assert!(
            written.contains("\x1b[?2004l"),
            "bracketed paste off: {written:?}"
        );
        assert!(
            written.contains("\x1b[?1004l"),
            "focus reports off: {written:?}"
        );
        assert!(RESTORED.get());
    }

    #[test]
    fn a_terminal_that_refuses_the_sequences_is_still_restored() {
        // A full buffer fails every write with `WriteZero`.
        let mut full: [u8; 0] = [];
        drop(TerminalGuard {
            out: &mut full[..],
            restore: note_restore,
        });
        assert!(RESTORED.get());
    }
}
