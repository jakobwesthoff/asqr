// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The terminal around the TUI: the only code that talks to the real
//! terminal, the operating system's image viewer and the inbox watcher's
//! thread. Everything it wires together is tested in the library; what is
//! left here needs a terminal and is kept to the wiring.

use std::cell::{Cell, RefCell};
use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;

use anyhow::Context;
use asqr::cli::{Exit, Watch};
#[cfg(unix)]
use asqr::tui::render::passthrough_answers;
use asqr::tui::render::{
    Glyphs, TMUX_STATE_FORMAT, TerminalNames, View, kitty_deletes, may_query_protocol,
};
use asqr::tui::{AlertTerminal, Alerts, Event, Host, alert, run, start, watch_inbox};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    Event as TerminalEvent,
};
use ratatui::crossterm::terminal::SetTitle;
use ratatui::crossterm::{execute, queue, terminal};
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

    // asqr honours NO_COLOR itself, in the drawn buffer, and crossterm has
    // to write what is left there. crossterm 0.29 reads NO_COLOR too and
    // then writes every colour as an empty SGR, which the terminal reads as
    // a reset of everything: images lose their RGB colours, the Kitty
    // protocol the colour that names each image, and a cell that changes
    // colour loses the bold, dim or reversed set just before it.
    ratatui::crossterm::style::force_color_output(true);

    // `try_init` also installs the panic hook that gives the terminal back
    // before a panic message is printed. That hook knows nothing of focus
    // reports and bracketed paste, so the guard turns those off too, on
    // every return from this function from here on and on a panic.
    let mut terminal = ratatui::try_init().context("setting up the terminal")?;
    let title_pushed = Rc::new(Cell::new(false));
    let exit_deletes = Rc::new(RefCell::new(Vec::new()));
    let _guard = TerminalGuard {
        out: io::stdout(),
        restore: ratatui::restore,
        title_pushed: Rc::clone(&title_pushed),
        exit_deletes: Rc::clone(&exit_deletes),
    };
    // SIGTERM, SIGINT and SIGHUP would end the process without the guard.
    // Caught, they end the run as `q` does, and the guard runs. SIGKILL
    // cannot be caught and leaves the terminal as set up here.
    #[cfg(unix)]
    forward_quit_signals(sender.clone());

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
    // One question to tmux serves both the graphics query and the glyphs
    // of Talqi's emblem.
    let in_tmux = tmux.as_deref().is_some_and(|tmux| !tmux.is_empty());
    let tmux_state = if in_tmux { tmux_state() } else { None };
    let glyphs = Glyphs::for_terminal(|name| std::env::var(name).ok(), tmux_state.as_deref());
    tracing::info!(?glyphs, "glyphs for Talqi's emblem");
    // Every `Picker` constructor turns passthrough on for the pane; one
    // guard around both gives the pane its own setting back once the
    // picker is built (spec section 7.7).
    let picker = {
        let _passthrough = PassthroughRestore::snapshot(&names, run_tmux);
        if may_query_protocol(&names, || tmux_state.clone(), probe_passthrough) {
            Picker::from_query_stdio().unwrap_or_else(|error| {
                tracing::info!(%error, "no graphics protocol detected; using half blocks");
                Picker::halfblocks()
            })
        } else {
            tracing::info!("tmux cannot forward the graphics query; using half blocks");
            Picker::halfblocks()
        }
    };
    // Images are sized with this cell size for the whole run, so a report
    // of a cropped or shrunken image starts by comparing it with the cell
    // size the terminal reports on a resize.
    let font_size = picker.font_size();
    let kitty_tmux = picker.tmux_detected();
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
        title: watch.title(),
        title_pushed,
        terminal: AlertTerminal {
            term,
            tmux: tmux.is_some_and(|tmux| !tmux.is_empty()),
        },
        kitty_tmux,
    };
    let mut view = View::new(&queue, watch.layout(), picker).no_color(watch.no_color());
    let ran = run(
        &mut terminal,
        &mut app,
        &mut inbox,
        &mut view,
        &events,
        &mut host,
    );
    // The guard writes these while the alternate screen is still on, also
    // when the run ended in an error (ADR 42).
    exit_deletes.replace(kitty_deletes(&view.take_kitty_images(), kitty_tmux));
    ran
}

/// Gives the terminal back when dropped: deletes the Kitty images asqr
/// leaves, gives back the title asqr pushed, turns focus reports and
/// bracketed paste off, then calls `restore`, which leaves raw mode and
/// the alternate screen.
///
/// The guard covers a panic only because a panic unwinds and drops it. A
/// build with `panic = "abort"` would skip the drop and leave both modes
/// on in the shell. On a panic, ratatui's hook leaves the alternate screen
/// before the unwinding reaches the guard, so there the title is given
/// back on the main screen.
struct TerminalGuard<W: Write> {
    out: W,
    /// [`ratatui::restore`]; tests pass a function that only notes the call.
    restore: fn(),
    /// Whether [`write_title`] pushed the terminal's title, shared with
    /// the [`TerminalHost`] that writes the titles.
    title_pushed: Rc<Cell<bool>>,
    /// The commands that delete the Kitty images asqr still knows of, put
    /// here when the run ends. A panic leaves it empty.
    exit_deletes: Rc<RefCell<Vec<u8>>>,
}

/// Saves the window and the icon title on the terminal's title stack.
/// tmux ignores the push without the `;0`.
const PUSH_TITLE: &[u8] = b"\x1b[22;0t";
/// Takes back what [`PUSH_TITLE`] saved.
const POP_TITLE: &[u8] = b"\x1b[23;0t";

/// Writes `title` as the window and the icon title (OSC 0), after pushing
/// the terminal's own title before the first one. asqr calls it only once
/// the alternate screen is on, so the push, every title and the pop all
/// happen on that screen (ADR 40). tmux takes all of them for the pane
/// itself, so they go out plain there too, not through the passthrough.
fn write_title(out: &mut impl Write, title: &str, pushed: &Cell<bool>) -> io::Result<()> {
    if !pushed.get() {
        out.write_all(PUSH_TITLE)?;
        pushed.set(true);
    }
    queue!(out, SetTitle(title))?;
    out.flush()
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        // Ghostty keeps Kitty images per screen, and the alternate screen
        // with them after asqr leaves it, where a delete no longer reaches
        // them; erasing the screen skips images placed by placeholders. So
        // the deletes go first, while `restore` has not left it yet
        // (ADR 42).
        let deletes = self.exit_deletes.take();
        if !deletes.is_empty() {
            let deleted = self.out.write_all(&deletes).and_then(|()| self.out.flush());
            if let Err(error) = deleted {
                tracing::warn!(%error, "could not delete the images");
            }
        }
        // Only a push is popped, and before `restore` leaves the alternate
        // screen. The empty title comes first: a terminal without a title
        // stack keeps it instead of asqr's, and one with a stack then takes
        // back the pushed title. The other way round, the empty title
        // would replace the title just taken back (ADR 40).
        if self.title_pushed.get() {
            let given_back = queue!(self.out, SetTitle(""))
                .and_then(|()| self.out.write_all(POP_TITLE))
                .and_then(|()| self.out.flush());
            if let Err(error) = given_back {
                tracing::warn!(%error, "could not give the terminal title back");
            }
        }
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

/// Sends [`Event::Quit`] on `sender` for every SIGTERM, SIGINT and SIGHUP.
/// signal-hook's handler only writes to a pipe; a thread reads it and sends
/// the event. In raw mode ctrl-c is a key, so a SIGINT comes from outside,
/// as from `kill`. A SIGHUP mostly means the terminal is gone; the guard's
/// writes then fail and are logged. When the handler cannot be installed,
/// asqr runs on, and a signal ends it at once.
#[cfg(unix)]
fn forward_quit_signals(sender: mpsc::Sender<Event>) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

    let mut signals = match signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP]) {
        Ok(signals) => signals,
        Err(error) => {
            tracing::warn!(%error, "could not catch SIGTERM, SIGINT and SIGHUP");
            return;
        }
    };
    // The thread waits for the life of the process. A signal after the run
    // ended finds no receiver; the process is on its way out then.
    std::thread::spawn(move || {
        for _ in signals.forever() {
            let _ = sender.send(Event::Quit);
        }
    });
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

/// What [`may_query_protocol`] and [`Glyphs::for_terminal`] need to know
/// from tmux.
fn tmux_state() -> Option<String> {
    run_tmux(&["display-message", "-p", TMUX_STATE_FORMAT])
}

/// Runs `tmux` with `args` and returns what it printed, or `None` when it
/// cannot run or fails, as without a server.
fn run_tmux(args: &[&str]) -> Option<String> {
    let output = Command::new("tmux")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Puts the pane's `allow-passthrough` back as it was before the `Picker`
/// was built, when dropped.
///
/// ratatui-image 11.1.0 runs `tmux set -p allow-passthrough on` in every
/// `Picker` constructor when `TERM` starts with `tmux` or `TERM_PROGRAM`
/// is `tmux`, which would override the person's own setting for the pane
/// on every start. The same test decides here whether to take a snapshot,
/// so the guard acts exactly where the library does. A tmux that cannot
/// show the option, with no server or before 3.3, gets no snapshot and is
/// left alone.
struct PassthroughRestore<R: FnMut(&[&str]) -> Option<String>> {
    tmux: R,
    /// The pane's value before, `Some(None)` when the pane had none of its
    /// own and inherited it; `None` without a snapshot.
    before: Option<Option<String>>,
}

impl<R: FnMut(&[&str]) -> Option<String>> PassthroughRestore<R> {
    fn snapshot(names: &TerminalNames<'_>, mut tmux: R) -> Self {
        let tmux_like = names.term.is_some_and(|term| term.starts_with("tmux"))
            || names.term_program == Some("tmux");
        let before = if tmux_like {
            tmux(&["show-options", "-p", "-v", "allow-passthrough"]).map(|shown| {
                let value = shown.trim();
                (!value.is_empty()).then(|| value.to_owned())
            })
        } else {
            None
        };
        Self { tmux, before }
    }
}

impl<R: FnMut(&[&str]) -> Option<String>> Drop for PassthroughRestore<R> {
    fn drop(&mut self) {
        // Unsetting lets the pane inherit from its window, session or the
        // global options again, as before.
        match self.before.take() {
            None => {}
            Some(None) => _ = (self.tmux)(&["set", "-pu", "allow-passthrough"]),
            Some(Some(value)) => _ = (self.tmux)(&["set", "-p", "allow-passthrough", &value]),
        }
    }
}

/// How long [`probe_passthrough`] waits for the answer: long enough for a
/// round trip over ssh, short enough not to hold up the start noticeably.
#[cfg(unix)]
const PROBE_PATIENCE: std::time::Duration = std::time::Duration::from_millis(300);

/// Whether a sequence sent through tmux's passthrough gets an answer, for
/// [`may_query_protocol`] in a tmux it cannot ask. Raw mode is on, as
/// `ratatui::try_init` turned it on.
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
    /// Off under `--no-title`, which writes no title byte at all: no
    /// title, and so no push the guard would pop.
    title: bool,
    title_pushed: Rc<Cell<bool>>,
    terminal: AlertTerminal,
    /// Whether the picker found tmux, which wraps every Kitty command for
    /// its passthrough, the deletes as the transmissions.
    kitty_tmux: bool,
}

impl Host for TerminalHost {
    fn alert(&mut self, arrived: &[String]) {
        let mut stdout = io::stdout().lock();
        if let Err(error) = alert(&mut stdout, self.alerts, &self.terminal, arrived) {
            tracing::warn!(%error, "could not write the alert");
        }
        let _ = stdout.flush();
    }

    fn title(&mut self, title: &str) {
        if !self.title {
            return;
        }
        let mut stdout = io::stdout().lock();
        if let Err(error) = write_title(&mut stdout, title, &self.title_pushed) {
            tracing::warn!(%error, "could not set the terminal title");
        }
    }

    fn delete_images(&mut self, ids: &[u32]) {
        let mut stdout = io::stdout().lock();
        let deleted = stdout
            .write_all(&kitty_deletes(ids, self.kitty_tmux))
            .and_then(|()| stdout.flush());
        if let Err(error) = deleted {
            tracing::warn!(%error, "could not delete the images");
        }
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
    /// What dropping a guard writes, when asqr did or did not push the
    /// title.
    fn dropped(pushed: bool) -> String {
        dropped_with(pushed, b"")
    }

    /// What dropping a guard writes, with `deletes` put there as the run
    /// ended.
    fn dropped_with(pushed: bool, deletes: &[u8]) -> String {
        let mut written = Vec::new();
        drop(TerminalGuard {
            out: &mut written,
            restore: note_restore,
            title_pushed: Rc::new(Cell::new(pushed)),
            exit_deletes: Rc::new(RefCell::new(deletes.to_vec())),
        });
        String::from_utf8(written).expect("escape sequences are ASCII")
    }

    #[test]
    fn the_guard_deletes_the_images_before_it_gives_the_terminal_back() {
        let deletes = kitty_deletes(&[7], false);

        let written = dropped_with(true, &deletes);

        assert!(
            written.starts_with("\x1b_Ga=d,d=I,i=7,q=2\x1b\\\x1b]0;\x07\x1b[23;0t"),
            "the deletes, then the title: {written:?}"
        );
        assert!(RESTORED.get());
    }

    #[test]
    fn dropping_the_guard_turns_paste_and_focus_reports_off_and_restores() {
        let written = dropped(false);
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
    fn the_guard_clears_and_pops_the_title_only_after_a_push() {
        let written = dropped(true);
        assert!(
            written.starts_with("\x1b]0;\x07\x1b[23;0t"),
            "the empty title, then the pop: {written:?}"
        );

        let written = dropped(false);
        assert!(
            !written.contains("\x1b]0;") && !written.contains("\x1b[23"),
            "no title bytes without a push: {written:?}"
        );
    }

    #[test]
    fn the_first_title_comes_after_a_push_and_the_others_alone() {
        let mut written = Vec::new();
        let pushed = Cell::new(false);

        write_title(&mut written, "asqr · 1 waiting", &pushed).expect("written");
        write_title(&mut written, "asqr · 0 waiting", &pushed).expect("written");

        assert_eq!(
            String::from_utf8(written).expect("UTF-8"),
            "\x1b[22;0t\x1b]0;asqr · 1 waiting\x07\x1b]0;asqr · 0 waiting\x07"
        );
        assert!(pushed.get());
    }

    #[test]
    fn a_push_the_terminal_refused_is_not_popped() {
        let mut full: [u8; 0] = [];
        let pushed = Cell::new(false);

        assert!(write_title(&mut &mut full[..], "asqr · 1 waiting", &pushed).is_err());

        assert!(!pushed.get());
    }

    /// The tmux commands a [`PassthroughRestore`] runs from its snapshot to
    /// its drop, where `show-options` answers `shown`.
    fn passthrough_commands(term: &str, term_program: &str, shown: Option<&str>) -> Vec<String> {
        let commands = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = std::rc::Rc::clone(&commands);
        let names = TerminalNames {
            tmux: None,
            term: Some(term),
            term_program: Some(term_program),
        };
        drop(PassthroughRestore::snapshot(
            &names,
            move |args: &[&str]| {
                recorded.borrow_mut().push(args.join(" "));
                if args[0] == "show-options" {
                    shown.map(str::to_owned)
                } else {
                    Some(String::new())
                }
            },
        ));
        commands.take()
    }

    const SHOW: &str = "show-options -p -v allow-passthrough";

    #[test]
    fn a_pane_without_its_own_passthrough_gets_it_unset_again() {
        assert_eq!(
            passthrough_commands("tmux-256color", "", Some("")),
            [SHOW, "set -pu allow-passthrough"]
        );
    }

    #[test]
    fn a_pane_with_its_own_passthrough_gets_that_value_back() {
        assert_eq!(
            passthrough_commands("xterm-256color", "tmux", Some("off\n")),
            [SHOW, "set -p allow-passthrough off"]
        );
    }

    #[test]
    fn a_tmux_that_cannot_show_the_option_is_left_alone() {
        assert_eq!(passthrough_commands("tmux", "", None), [SHOW]);
    }

    #[test]
    fn outside_what_ratatui_image_takes_for_tmux_tmux_is_not_run() {
        for (term, term_program) in [("xterm-ghostty", "ghostty"), ("screen-256color", "")] {
            assert_eq!(
                passthrough_commands(term, term_program, Some("off")),
                Vec::<String>::new(),
                "{term} {term_program}"
            );
        }
    }

    #[test]
    fn a_terminal_that_refuses_the_sequences_is_still_restored() {
        // A full buffer fails every write with `WriteZero`.
        let mut full: [u8; 0] = [];
        drop(TerminalGuard {
            out: &mut full[..],
            restore: note_restore,
            title_pushed: Rc::new(Cell::new(true)),
            exit_deletes: Rc::new(RefCell::new(kitty_deletes(&[7], false))),
        });
        assert!(RESTORED.get());
    }
}
