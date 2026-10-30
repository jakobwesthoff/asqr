// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The terminal around the TUI: the only code that talks to the real
//! terminal, to tmux, to the operating system's image viewer and to the
//! inbox watcher's thread. What it decides on, such as whether to ask the
//! terminal for its graphics protocol and what to do with tmux's
//! passthrough, is decided and tested in the library. What is left here
//! runs the terminal and those programs; its parts that write escape
//! sequences are tested against a byte buffer.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use anyhow::Context;
use asqr::cli::{Exit, Watch};
use asqr::graphics::{
    Graphics, Images, KittyBook, KittyDeletes, KittyExit, PanePassthrough, QUERY_WAIT,
    answers_may_follow_the_query, may_query_protocol, passthrough_all_for_images,
    settle_passthrough,
};
#[cfg(unix)]
use asqr::graphics::{LATE_ANSWER_CAP, LATE_ANSWER_QUIET, discard_late_answers};
use asqr::pairing::{SystemTmux, Tmux};
use asqr::terminal_env::{TerminalEnv, TmuxState};
use asqr::tui::render::{Glyphs, View, loader_panicked};
use asqr::tui::{
    Alerts, Event, EventSource, Host, KeepAlive, KeyReader, OpenedFiles, ProgressTerminal,
    SleepClock, alert, opener_failure, progress_sequence, progress_terminal, run, spawn_reaped,
    start, watch_inbox,
};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    Event as TerminalEvent,
};
use ratatui::crossterm::terminal::SetTitle;
use ratatui::crossterm::{cursor, execute, queue, terminal};
use ratatui_image::picker::cap_parser::QueryStdioOptions;
use ratatui_image::picker::{Capability, Picker, ProtocolType};

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
    // arrive unseen in between (spec section 5.4).
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

    let env = TerminalEnv::from_process();
    // Every `Picker` constructor turns passthrough on for the pane. This
    // guard gives the pane its own setting back once the picker is built,
    // or as asqr ends when the pane gets `all` for Kitty images (spec
    // section 8.9). Made first, it is dropped last, so the deletes of the
    // Kitty images on quitting still pass from a hidden pane.
    let mut pane = PanePassthrough::snapshot(&env, run_tmux);

    // `try_init` also installs the panic hook that gives the terminal back
    // before a panic message is printed. That hook knows nothing of focus
    // reports and bracketed paste, so the guard turns those off too, on
    // every return from this function from here on and on a panic. Once
    // the images exist, asqr's own hook runs before ratatui's.
    let mut terminal = ratatui::try_init().context("setting up the terminal")?;
    let title_pushed = Arc::new(AtomicBool::new(false));
    let progress = ShownProgress::new(env.in_tmux_pane());
    let _guard = TerminalGuard {
        out: io::stdout(),
        restore: ratatui::restore,
        title_pushed: Arc::clone(&title_pushed),
        progress: progress.clone(),
    };
    // SIGTERM, SIGINT and SIGHUP would end the process without the guard.
    // Caught, they end the run as `q` does, and the guard runs. SIGKILL
    // cannot be caught and leaves the terminal as set up here.
    let editing = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    forward_quit_signals(sender.clone(), Arc::clone(&editing));

    // The protocol query reads the terminal's answers from stdin, so it
    // runs before the thread that reads key events starts.
    // One question to tmux serves both the graphics query and the glyphs
    // of Talqi's emblem.
    let tmux = if env.in_tmux_pane() {
        tmux_state()
    } else {
        None
    };
    let glyphs = Glyphs::for_terminal(&env, tmux.as_ref());
    tracing::info!(?glyphs, "glyphs for Talqi's emblem");
    let mut graphics = if may_query_protocol(&env, tmux.as_ref()) {
        query_graphics(tmux.as_ref())
    } else {
        tracing::info!("tmux cannot forward the graphics query; using half blocks");
        Graphics::HALF_BLOCKS
    };
    let set_all = passthrough_all_for_images(&graphics, tmux.as_ref()) && pane.allow_all();
    if set_all {
        tracing::info!("allow-passthrough all for the pane while asqr runs");
    } else {
        pane.restore();
    }
    graphics.passthrough = settle_passthrough(&env, tmux.as_ref(), set_all);
    tracing::info!(
        protocol = ?graphics.protocol,
        cell = %format!("{}x{}", graphics.cell.width, graphics.cell.height),
        passthrough = ?graphics.passthrough,
        kitty_compression = graphics.kitty_compression,
        "graphics protocol detected"
    );
    log_window_size();

    let images = Images::new(graphics);
    hook_in_before_ratatui_on_panic(
        images.kitty_book().clone(),
        Arc::clone(&title_pushed),
        progress.clone(),
    );
    // What is left at the end is dropped in the reverse order it was made
    // in: the view first, whose images hand their ids on as they go, then
    // `_kitty_exit`, which deletes the images while the alternate screen is
    // on, then `_guard`, which leaves it, and last `pane`, which gives the
    // pane its passthrough back, so the deletes pass while it has `all`
    // (ADRs 42 and 44).
    let _kitty_exit = KittyExit::new(images.kitty_book().clone(), io::stdout());

    // Focus reports tell asqr when it comes back into view, so it can send
    // images again whose transfer tmux dropped while it was hidden (spec
    // section 8.8). tmux forwards them with its `focus-events` option.
    // Bracketed paste sends a paste as one event, so a line break in it is
    // no `enter` that picks, submits or rejects (spec section 7.7). The TUI
    // works without either, and a legacy Windows console refuses bracketed
    // paste, so a refusal is logged and the run goes on.
    if let Err(error) = execute!(io::stdout(), EnableFocusChange) {
        tracing::warn!(%error, "could not ask for focus reports");
    }
    if let Err(error) = execute!(io::stdout(), EnableBracketedPaste) {
        tracing::warn!(%error, "could not ask for bracketed paste");
    }

    let loaded = sender.clone();
    let ticks = sender.clone();
    let openers = sender.clone();
    // The thread reads for the life of the process and ends with it; it
    // pauses while another program has the terminal.
    let keys = KeyReader::spawn(CrosstermEvents, sender);

    let queue = match location.name() {
        Some(name) => name.to_owned(),
        None => location.dir().display().to_string(),
    };
    let mut host = TerminalHost {
        alerts: watch.alerts(),
        title: watch.title(),
        title_pushed,
        progress: ProgressOut::new(env.clone(), run_tmux, progress, ticks),
        env,
        keys,
        editing,
        openers,
    };
    let mut view = View::new(&queue, watch.layout(), images)
        .chosen_queue(!location.is_default())
        .no_color(watch.no_color())
        .home(watch.home().map(Path::to_path_buf))
        .glyphs(glyphs)
        .wake_on_load(move || {
            // The run loop is gone only as asqr quits.
            let _ = loaded.send(Event::Loaded);
        });
    // A debug build panics while loading the image the PTY tests name,
    // so they see what asqr gives back on a panic while loading an entry.
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("ASQR_TEST_PANIC_ON_LOAD") {
        view = view.panicking_on_load(path);
    }
    // The files `o` wrote in earlier runs that may go are removed here,
    // under the queue's lock.
    let mut opened = OpenedFiles::in_temp_dir();
    run(
        &mut terminal,
        &mut app,
        &mut inbox,
        &mut view,
        &events,
        &mut host,
        &mut opened,
    )
}

/// Asks the terminal for its graphics protocol, and discards the answers
/// that may come after the query read its own. Silence and an answer
/// without graphics both give half blocks; only reading or writing the
/// terminal fails, and gives them too.
fn query_graphics(tmux: Option<&TmuxState>) -> Graphics {
    let options = QueryStdioOptions {
        timeout_ms: i32::try_from(QUERY_WAIT.as_millis()).expect("5000 ms fit an i32"),
        ..QueryStdioOptions::default()
    };
    let started = Instant::now();
    let queried = Picker::from_query_stdio_with_options(options);
    let query_took = started.elapsed();
    let graphics = match &queried {
        Ok(picker) => {
            if let Some(reason) = half_blocks_reason(picker.protocol_type(), picker.capabilities())
            {
                tracing::info!(reason, "no graphics protocol detected; using half blocks");
            }
            Graphics::from_picker(picker)
        }
        Err(error) => {
            tracing::warn!(%error, "could not query the graphics protocol; using half blocks");
            Graphics::HALF_BLOCKS
        }
    };
    if answers_may_follow_the_query(query_took, tmux) {
        #[cfg(unix)]
        discard_late_answers_on_stdin();
    }
    graphics
}

/// Gives the terminal back when dropped: gives back the title asqr
/// pushed, turns focus reports and bracketed paste off, then calls
/// `restore`, which leaves raw mode and the alternate screen.
///
/// The guard covers a panic only because a panic unwinds and drops it. A
/// build with `panic = "abort"` would skip the drop and leave both modes
/// on in the shell. On a panic, ratatui's hook leaves the alternate screen
/// before the unwinding reaches the guard, so asqr's hook gives the title
/// back before it (see [`hook_in_before_ratatui_on_panic`]).
struct TerminalGuard<W: Write> {
    out: W,
    /// [`ratatui::restore`]; tests pass a function that only notes the call.
    restore: fn(),
    /// Whether [`write_title`] pushed the terminal's title, shared with
    /// the [`TerminalHost`] that writes the titles and the panic hook.
    title_pushed: Arc<AtomicBool>,
    /// The progress the [`TerminalHost`] shows, which goes on quitting.
    progress: ShownProgress,
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
fn write_title(out: &mut impl Write, title: &str, pushed: &AtomicBool) -> io::Result<()> {
    if !pushed.load(Ordering::Relaxed) {
        out.write_all(PUSH_TITLE)?;
        pushed.store(true, Ordering::Relaxed);
    }
    queue!(out, SetTitle(title))?;
    out.flush()
}

/// Removes the progress asqr shows, gives back the title asqr pushed and
/// turns focus reports and bracketed paste off, before the alternate
/// screen is left. On a panic the hook and then the guard both run it;
/// the progress and the title are given back once.
fn give_back_title_and_modes(
    out: &mut impl Write,
    title_pushed: &AtomicBool,
    progress: &ShownProgress,
) {
    progress.remove_once(out);
    // Only a push is popped. The empty title comes first: a terminal
    // without a title stack keeps it instead of asqr's, and one with a
    // stack then takes back the pushed title. The other way round, the
    // empty title would replace the title just taken back (ADR 40).
    if title_pushed.swap(false, Ordering::Relaxed) {
        let given_back = queue!(out, SetTitle(""))
            .and_then(|()| out.write_all(POP_TITLE))
            .and_then(|()| out.flush());
        if let Err(error) = given_back {
            tracing::warn!(%error, "could not give the terminal title back");
        }
    }
    // Left on, the shell would receive the reports as typed text, and its
    // pastes wrapped in the markers.
    if let Err(error) = execute!(out, DisableFocusChange) {
        tracing::warn!(%error, "could not turn focus reports off");
    }
    if let Err(error) = execute!(out, DisableBracketedPaste) {
        tracing::warn!(%error, "could not turn bracketed paste off");
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        give_back_title_and_modes(&mut self.out, &self.title_pushed, &self.progress);
        (self.restore)();
    }
}

/// Puts a panic hook in front of the one `ratatui::try_init` installed,
/// which leaves the alternate screen before the unwinding drops
/// [`KittyExit`] and [`TerminalGuard`]. What has to happen on that screen
/// happens here first (ADRs 40 and 42). The pane's passthrough is given
/// back during the unwinding, after both hooks, so the deletes pass while
/// it has `all` (ADR 44).
///
/// The hook stays for the rest of the process, as ratatui's does. After
/// the run the book is empty and the title given back, so it only turns
/// the modes off again.
fn hook_in_before_ratatui_on_panic(
    book: KittyBook,
    title_pushed: Arc<AtomicBool>,
    progress: ShownProgress,
) {
    let ratatui_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A panic while loading a context entry is handed to the drawing
        // thread, which panics with it; the terminal is given back there,
        // once nothing draws any more.
        if loader_panicked(info) {
            return;
        }
        give_back_on_panic(&mut io::stdout(), &book, &title_pushed, &progress);
        ratatui_hook(info);
    }));
}

/// Deletes the Kitty images, those still shown included, removes the
/// progress, gives the title back and turns focus reports and bracketed
/// paste off, on a panic.
///
/// The hook runs before the unwinding lets go of a lock, so it only tries
/// the book: after a panic while the book was locked, the exit guard
/// writes the deletes, once the alternate screen is gone already. What
/// the hook took, the exit guard does not write again.
fn give_back_on_panic(
    out: &mut impl Write,
    book: &KittyBook,
    title_pushed: &AtomicBool,
    progress: &ShownProgress,
) {
    if let Some(deletes) = book.try_drain().filter(|deletes| !deletes.is_empty()) {
        let deleted = out
            .write_all(&deletes.to_bytes())
            .and_then(|()| out.flush());
        if let Err(error) = deleted {
            tracing::warn!(%error, "could not delete the images");
        }
    }
    give_back_title_and_modes(out, title_pushed, progress);
}

/// Whether asqr shows a progress, and the bytes that remove it, shared by
/// the [`ProgressOut`] that writes the progress, the guard and the panic
/// hook.
#[derive(Clone)]
struct ShownProgress {
    shown: Arc<AtomicBool>,
    remove: Arc<[u8]>,
}

impl ShownProgress {
    /// Nothing shown yet; the remove goes through tmux's passthrough when
    /// `tmux`, as the progress does.
    fn new(tmux: bool) -> Self {
        ShownProgress {
            shown: Arc::new(AtomicBool::new(false)),
            remove: progress_sequence(None, tmux).into(),
        }
    }

    /// Removes the progress, once, when asqr shows one.
    fn remove_once(&self, out: &mut impl Write) {
        if self.shown.swap(false, Ordering::Relaxed) {
            let removed = out.write_all(&self.remove).and_then(|()| out.flush());
            if let Err(error) = removed {
                tracing::warn!(%error, "could not remove the progress");
            }
        }
    }
}

/// Writes the progress to terminals known to show it (ADR 73).
struct ProgressOut {
    env: TerminalEnv,
    /// What the terminal does with the progress outside tmux, which the
    /// environment of the process decides once.
    outside: ProgressTerminal,
    /// [`run_tmux`]; tests pass a function that answers for tmux.
    ask_tmux: fn(&[&str]) -> Option<String>,
    shown: ShownProgress,
    /// Started for the first terminal that clears the progress after a
    /// while, and kept for the run.
    keep_alive: Option<KeepAlive>,
    /// Where the keep-alive's ticks go: the run loop.
    ticks: mpsc::Sender<Event>,
}

impl ProgressOut {
    fn new(
        env: TerminalEnv,
        ask_tmux: fn(&[&str]) -> Option<String>,
        shown: ShownProgress,
        ticks: mpsc::Sender<Event>,
    ) -> Self {
        ProgressOut {
            outside: progress_terminal(&env, None),
            env,
            ask_tmux,
            shown,
            keep_alive: None,
            ticks,
        }
    }

    /// What the terminal does with the progress right now. Inside tmux the
    /// client may have changed since the last write, as after a detach and
    /// an attach from another terminal, so tmux is asked again each time.
    fn terminal(&self) -> ProgressTerminal {
        if !self.env.in_tmux_pane() {
            return self.outside;
        }
        let state = (self.ask_tmux)(&["display-message", "-p", TmuxState::FORMAT])
            .map(|answer| TmuxState::parse(&answer));
        progress_terminal(&self.env, state.as_ref())
    }

    /// Shows `percent`, or removes the progress for `None`, where the
    /// terminal is known to show it. Where it is not, nothing is written,
    /// a remove neither, as the terminal may show any OSC 9 as a
    /// notification, and the progress counts as gone.
    fn write(&mut self, out: &mut impl Write, percent: Option<u8>) -> io::Result<()> {
        let terminal = self.terminal();
        if terminal == ProgressTerminal::None {
            self.shown.shown.store(false, Ordering::Relaxed);
            self.keep(false);
            return Ok(());
        }
        out.write_all(&progress_sequence(percent, self.env.in_tmux_pane()))?;
        out.flush()?;
        self.shown.shown.store(percent.is_some(), Ordering::Relaxed);
        self.keep(terminal == ProgressTerminal::KeepAlive && percent.is_some());
        Ok(())
    }

    /// Lets the keep-alive's ticks come or stops them, starting it the
    /// first time it is needed.
    fn keep(&mut self, alive: bool) {
        if alive && self.keep_alive.is_none() {
            let ticks = self.ticks.clone();
            self.keep_alive = Some(KeepAlive::spawn(SleepClock, move || {
                ticks.send(Event::KeepAlive).is_ok()
            }));
        }
        if let Some(keep_alive) = &self.keep_alive {
            keep_alive.set_active(alive);
        }
    }
}

/// Sends [`Event::Quit`] on `sender` for every SIGTERM, SIGINT and SIGHUP.
/// signal-hook's handler only writes to a pipe; a thread reads it and sends
/// the event. In raw mode ctrl-c is a key, so a SIGINT comes from outside,
/// as from `kill`. A SIGHUP mostly means the terminal is gone; the guard's
/// writes then fail and are logged. When the handler cannot be installed,
/// asqr runs on, and a signal ends it at once.
///
/// While the person's editor has the terminal (`editing`), the tty is not
/// in raw mode, so ctrl-c there sends SIGINT to the editor and to asqr.
/// It is meant for the editor, so asqr lets it go (ADR 74).
#[cfg(unix)]
fn forward_quit_signals(sender: mpsc::Sender<Event>, editing: Arc<AtomicBool>) {
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
        for signal in signals.forever() {
            if quits(signal, editing.load(Ordering::Relaxed)) {
                let _ = sender.send(Event::Quit);
            }
        }
    });
}

/// Whether `signal` quits asqr, while the person's editor has the
/// terminal when `editing`. SIGTERM and SIGHUP do so after the editor
/// ended, as nothing else runs until then.
#[cfg(unix)]
fn quits(signal: i32, editing: bool) -> bool {
    !(editing && signal == signal_hook::consts::SIGINT)
}

// ---------------------------------------------------------------------
// Handing the terminal to the person's editor (ADR 74)
// ---------------------------------------------------------------------

/// How long `ctrl-g` waits for the key reader to stop reading the terminal
/// before it gives up on the editor.
const PAUSE_DEADLINE: Duration = Duration::from_secs(1);

/// Checks that the editor can start: one is found, and the key reader
/// stopped reading the terminal within `deadline`. Neither touches the
/// terminal. On `Ok` the reader is paused, and the caller resumes it.
fn ready_to_edit(
    find_editor: fn() -> io::Result<()>,
    keys: &KeyReader,
    deadline: Duration,
) -> io::Result<()> {
    if let Err(error) = find_editor() {
        tracing::info!(%error, "no editor found");
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    if keys.pause(deadline) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "the terminal's input is busy",
        ))
    }
}

/// What handing the terminal over needs from the operating system besides
/// the bytes written to it.
trait TtyModes {
    fn raw_mode(&mut self, on: bool) -> io::Result<()>;
    /// Notes the terminal's settings out of raw mode, those of before asqr.
    fn save_cooked(&mut self);
    /// Puts back what [`TtyModes::save_cooked`] noted. An editor that ended
    /// without giving the terminal back would otherwise leave its settings,
    /// and crossterm takes whatever is set when raw mode goes on for the
    /// settings to give back on quitting.
    fn restore_cooked(&mut self);
    /// Drops what the terminal sends until 150 ms pass without input, at
    /// most for a second: an answer to a query the editor sent the
    /// terminal that arrives after the editor exited would reach the key
    /// reader as keys. Keys typed meanwhile are dropped with it. Called in
    /// raw mode, where the terminal hands each byte over as it comes
    /// rather than a line at a time.
    fn discard_late_input(&mut self);
}

/// The terminal's modes through crossterm and termios.
#[derive(Default)]
struct RealTty {
    #[cfg(unix)]
    cooked: Option<rustix::termios::Termios>,
}

impl TtyModes for RealTty {
    fn raw_mode(&mut self, on: bool) -> io::Result<()> {
        if on {
            terminal::enable_raw_mode()
        } else {
            terminal::disable_raw_mode()
        }
    }

    fn save_cooked(&mut self) {
        #[cfg(unix)]
        match rustix::termios::tcgetattr(io::stdin()) {
            Ok(cooked) => self.cooked = Some(cooked),
            Err(error) => tracing::warn!(%error, "could not read the terminal's settings"),
        }
    }

    fn restore_cooked(&mut self) {
        #[cfg(unix)]
        if let Some(cooked) = &self.cooked {
            let restored = rustix::termios::tcsetattr(
                io::stdin(),
                rustix::termios::OptionalActions::Now,
                cooked,
            );
            if let Err(error) = restored {
                tracing::warn!(%error, "could not put the terminal's settings back");
            }
        }
    }

    // TODO: Windows drops nothing; `discard_late_answers` reads a Unix
    // file descriptor.
    fn discard_late_input(&mut self) {
        #[cfg(unix)]
        match discard_late_answers(io::stdin(), LATE_ANSWER_QUIET, LATE_ANSWER_CAP) {
            Ok(0) => {}
            Ok(bytes) => tracing::info!(bytes, "discarded input after the editor"),
            Err(error) => tracing::warn!(%error, "could not discard input after the editor"),
        }
    }
}

/// Gives the terminal to `editor` and takes it back once it returned,
/// whatever it returned. The terminal is given back as asqr found it,
/// focus reports and bracketed paste off and the cursor showing, and set
/// up again as at start; the next draw hides the cursor. What the terminal
/// sends just after the editor is dropped. A step that fails
/// is logged and the others go on, so the terminal is never left half set
/// up.
fn hand_over(
    out: &mut impl Write,
    modes: &mut impl TtyModes,
    editor: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let given_up = execute!(
        out,
        DisableFocusChange,
        DisableBracketedPaste,
        cursor::Show,
        terminal::LeaveAlternateScreen
    );
    if let Err(error) = given_up {
        tracing::warn!(%error, "could not give the screen to the editor");
    }
    if let Err(error) = modes.raw_mode(false) {
        tracing::warn!(%error, "could not leave raw mode for the editor");
    }
    modes.save_cooked();

    let edited = editor();

    modes.restore_cooked();
    if let Err(error) = modes.raw_mode(true) {
        tracing::warn!(%error, "could not enter raw mode after the editor");
    }
    let taken_back = execute!(
        out,
        terminal::EnterAlternateScreen,
        EnableBracketedPaste,
        EnableFocusChange
    );
    if let Err(error) = taken_back {
        tracing::warn!(%error, "could not take the screen back from the editor");
    }
    // Last, once raw mode is on and before the key reader goes on, which
    // would take what the editor's queries got back for keys.
    modes.discard_late_input();
    edited
}

/// The terminal's events as crossterm reads them, for the [`KeyReader`].
struct CrosstermEvents;

impl EventSource for CrosstermEvents {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        event::poll(timeout)
    }

    fn read(&mut self) -> io::Result<TerminalEvent> {
        let event = event::read()?;
        if let TerminalEvent::Resize(..) = event {
            log_window_size();
        }
        Ok(event)
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

/// Why the queried picker draws images in half blocks, for the log, or
/// `None` when it speaks a graphics protocol.
///
/// A picker without any capability got no answer it could use: none
/// within the query's wait, or one without a cell size, which the
/// graphics protocols need. With capabilities, the terminal answered and
/// named none of the protocols, and the environment hinted at none.
fn half_blocks_reason(protocol: ProtocolType, capabilities: &[Capability]) -> Option<&'static str> {
    if protocol != ProtocolType::Halfblocks {
        return None;
    }
    Some(if capabilities.is_empty() {
        "the terminal answered the graphics query without a cell size, or not at all"
    } else {
        "the terminal named no graphics protocol asqr speaks"
    })
}

/// What [`may_query_protocol`] and [`Glyphs::for_terminal`] need to know
/// from tmux.
fn tmux_state() -> Option<TmuxState> {
    run_tmux(&["display-message", "-p", TmuxState::FORMAT]).map(|answer| TmuxState::parse(&answer))
}

/// Runs `tmux` with `args` and returns what it printed, or `None` when it
/// cannot run, fails, as without a server, or does not answer in time, so
/// a hung tmux cannot hold up the start.
fn run_tmux(args: &[&str]) -> Option<String> {
    SystemTmux::default().run(None, args).ok()
}

/// Discards the answers to the graphics query that are still arriving,
/// before the key reader starts and takes them for keys. Raw mode is on,
/// so the terminal hands each byte over as it comes.
#[cfg(unix)]
fn discard_late_answers_on_stdin() {
    match discard_late_answers(io::stdin(), LATE_ANSWER_QUIET, LATE_ANSWER_CAP) {
        Ok(0) => {}
        Ok(bytes) => tracing::info!(bytes, "discarded late answers to the graphics query"),
        Err(error) => {
            tracing::warn!(%error, "could not discard late answers to the graphics query")
        }
    }
}

struct TerminalHost {
    alerts: Alerts,
    /// Off under `--no-title`, which writes no title byte at all: no
    /// title, and so no push the guard would pop.
    title: bool,
    title_pushed: Arc<AtomicBool>,
    progress: ProgressOut,
    env: TerminalEnv,
    /// Paused while the person's editor has the terminal.
    keys: KeyReader,
    /// Set while the person's editor has the terminal, for the thread
    /// that turns signals into quits.
    editing: Arc<AtomicBool>,
    /// Where the threads that wait for the system's opener report its
    /// failures: the run loop.
    openers: mpsc::Sender<Event>,
}

impl Host for TerminalHost {
    fn edit(&mut self, path: &Path) -> io::Result<()> {
        ready_to_edit(|| edit::get_editor().map(drop), &self.keys, PAUSE_DEADLINE)?;
        // Ticks would only pile up while the run loop waits here.
        self.progress.keep(false);
        self.editing.store(true, Ordering::Relaxed);
        let edited = hand_over(&mut io::stdout(), &mut RealTty::default(), || {
            edit::edit_file(path)
        });
        self.editing.store(false, Ordering::Relaxed);
        self.keys.resume();
        edited
    }

    fn progress(&mut self, percent: Option<u8>) {
        let mut stdout = io::stdout().lock();
        if let Err(error) = self.progress.write(&mut stdout, percent) {
            tracing::warn!(%error, "could not write the progress");
        }
    }

    fn alert(&mut self, arrived: &[String]) {
        let mut stdout = io::stdout().lock();
        if let Err(error) = alert(&mut stdout, self.alerts, &self.env, arrived) {
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

    fn delete_images(&mut self, deletes: &KittyDeletes) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        stdout.write_all(&deletes.to_bytes())?;
        stdout.flush()
    }

    fn open(&mut self, path: &Path, name: &str) -> io::Result<()> {
        // A debug build panics here when the PTY tests ask it to, so they
        // see what asqr gives back on a panic in its run loop.
        #[cfg(debug_assertions)]
        if std::env::var_os("ASQR_TEST_PANIC_ON_OPEN").is_some() {
            panic!("ASQR_TEST_PANIC_ON_OPEN is set");
        }
        let viewer = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let failures = self.openers.clone();
        let name = name.to_owned();
        // The viewer must not write into the TUI's screen.
        let mut command = Command::new(viewer);
        command
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spawn_reaped(&mut command, move |status| {
            if let Some(failure) = opener_failure(&name, status) {
                // The run loop is gone only as asqr quits.
                let _ = failures.send(Event::OpenerFailed(failure));
            }
        })
        .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use asqr::graphics::{Image, Protocol};
    use ratatui::layout::Size;

    use super::*;

    thread_local! {
        static RESTORED: Cell<bool> = const { Cell::new(false) };
    }

    // -----------------------------------------------------------------
    // Handing the terminal to the editor
    // -----------------------------------------------------------------

    /// What a hand-over did, in order: the bytes written, and the calls on
    /// the terminal's modes.
    type Steps = Rc<RefCell<Vec<String>>>;

    struct LoggedOut(Steps);

    impl Write for LoggedOut {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .borrow_mut()
                .push(String::from_utf8_lossy(bytes).into_owned());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct LoggedModes(Steps);

    impl TtyModes for LoggedModes {
        fn raw_mode(&mut self, on: bool) -> io::Result<()> {
            let step = if on { "<raw on>" } else { "<raw off>" };
            self.0.borrow_mut().push(step.to_owned());
            // Fails as a terminal that went away does; the hand-over goes on.
            Err(io::Error::other("no terminal"))
        }

        fn save_cooked(&mut self) {
            self.0.borrow_mut().push("<save>".to_owned());
        }

        fn restore_cooked(&mut self) {
            self.0.borrow_mut().push("<restore>".to_owned());
        }

        fn discard_late_input(&mut self) {
            self.0.borrow_mut().push("<discard>".to_owned());
        }
    }

    #[test]
    fn the_terminal_is_given_to_the_editor_and_taken_back_after_it() {
        let steps = Steps::default();
        let ran = Rc::clone(&steps);

        let edited = hand_over(
            &mut LoggedOut(Rc::clone(&steps)),
            &mut LoggedModes(Rc::clone(&steps)),
            || {
                ran.borrow_mut().push("<editor>".to_owned());
                Err(io::Error::other("exited with an error"))
            },
        );

        assert_eq!(
            edited.map_err(|error| error.to_string()),
            Err("exited with an error".to_owned()),
            "the editor's outcome"
        );
        assert_eq!(
            steps.borrow().concat(),
            "\x1b[?1004l\x1b[?2004l\x1b[?25h\x1b[?1049l<raw off><save><editor>\
             <restore><raw on>\x1b[?1049h\x1b[?2004h\x1b[?1004h<discard>"
        );
    }

    #[test]
    fn a_terminal_that_refuses_the_bytes_still_gets_the_editor() {
        let ran = Cell::new(false);
        // A full buffer fails every write with `WriteZero`.
        let mut full: [u8; 0] = [];

        let edited = hand_over(
            &mut &mut full[..],
            &mut LoggedModes(Steps::default()),
            || {
                ran.set(true);
                Ok(())
            },
        );

        assert!(edited.is_ok() && ran.get());
    }

    /// A source of terminal events whose `poll` says that it began on its
    /// sender and waits until its receiver's sender sends or goes, as
    /// crossterm waits for the rest of a sequence.
    struct Waiting(mpsc::Sender<()>, mpsc::Receiver<()>);

    impl EventSource for Waiting {
        fn poll(&mut self, _timeout: Duration) -> io::Result<bool> {
            let _ = self.0.send(());
            self.1
                .recv()
                .map(|()| false)
                .map_err(|_| io::Error::other("the terminal is gone"))
        }

        fn read(&mut self) -> io::Result<TerminalEvent> {
            Err(io::Error::other("nothing to read"))
        }
    }

    fn found() -> io::Result<()> {
        Ok(())
    }

    fn not_found() -> io::Result<()> {
        Err(io::Error::other("no editor on the path"))
    }

    /// A key reader inside a `poll` that waits, the sender that ends that
    /// `poll`, and the receiver of the polls that begin.
    fn reader_in_a_poll() -> (KeyReader, mpsc::Sender<()>, mpsc::Receiver<()>) {
        let (began, polls) = mpsc::channel();
        let (input, waiting) = mpsc::channel();
        let (sender, _events) = mpsc::channel();
        let keys = KeyReader::spawn(Waiting(began, waiting), sender);
        polls.recv().expect("the reader polls");
        (keys, input, polls)
    }

    #[test]
    fn no_editor_is_said_before_the_key_reader_is_asked() {
        let (keys, input, _polls) = reader_in_a_poll();

        let ready = ready_to_edit(not_found, &keys, Duration::ZERO);

        assert_eq!(
            ready.map_err(|error| error.kind()),
            Err(io::ErrorKind::NotFound)
        );
        drop(input);
    }

    #[test]
    fn a_key_reader_that_does_not_stop_in_time_is_a_busy_terminal() {
        let (keys, input, _polls) = reader_in_a_poll();

        let ready = ready_to_edit(found, &keys, Duration::ZERO);

        assert_eq!(
            ready.map_err(|error| error.kind()),
            Err(io::ErrorKind::WouldBlock)
        );
        drop(input);
    }

    #[test]
    fn a_key_reader_that_stopped_is_ready_for_the_editor() {
        let (keys, input, _polls) = reader_in_a_poll();
        // The terminal goes, and the reader with it: nothing reads the
        // terminal any more.
        drop(input);

        assert!(ready_to_edit(found, &keys, Duration::from_secs(5)).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn ctrl_c_meant_for_the_editor_does_not_quit_asqr() {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

        assert!(!quits(SIGINT, true), "SIGINT while editing");
        assert!(quits(SIGINT, false));
        assert!(quits(SIGTERM, true) && quits(SIGHUP, true));
    }

    fn note_restore() {
        RESTORED.set(true);
    }

    /// What dropping a guard writes, when asqr did or did not push the
    /// title.
    fn dropped(pushed: bool) -> String {
        dropped_showing(pushed, false)
    }

    /// What dropping a guard writes, when asqr did or did not push the
    /// title and did or did not show a progress.
    fn dropped_showing(pushed: bool, progress: bool) -> String {
        let mut written = Vec::new();
        let shown = ShownProgress::new(false);
        shown.shown.store(progress, Ordering::Relaxed);
        drop(TerminalGuard {
            out: &mut written,
            restore: note_restore,
            title_pushed: Arc::new(AtomicBool::new(pushed)),
            progress: shown,
        });
        String::from_utf8(written).expect("escape sequences are ASCII")
    }

    const REMOVE_PROGRESS: &str = "\x1b]9;4;0\x1b\\";

    #[test]
    fn the_guard_removes_the_progress_only_after_one_was_shown() {
        assert!(
            dropped_showing(false, true).starts_with(REMOVE_PROGRESS),
            "the progress goes first"
        );
        assert!(!dropped_showing(false, false).contains("]9;4"));
    }

    #[test]
    fn a_panic_removes_the_progress_once() {
        let (book, _shown) = book_with_an_image();
        let pushed = AtomicBool::new(false);
        let progress = ShownProgress::new(false);
        progress.shown.store(true, Ordering::Relaxed);
        let mut written = Vec::new();

        give_back_on_panic(&mut written, &book, &pushed, &progress);
        give_back_on_panic(&mut written, &book, &pushed, &progress);

        let written = String::from_utf8(written).expect("escape sequences are ASCII");
        assert_eq!(written.matches(REMOVE_PROGRESS).count(), 1, "{written:?}");
    }

    #[test]
    fn a_progress_that_cannot_be_removed_is_not_tried_again() {
        let progress = ShownProgress::new(false);
        progress.shown.store(true, Ordering::Relaxed);
        // A full buffer fails every write with `WriteZero`.
        let mut full: [u8; 0] = [];

        progress.remove_once(&mut &mut full[..]);

        assert!(!progress.shown.load(Ordering::Relaxed));
    }

    // -----------------------------------------------------------------
    // Writing the progress
    // -----------------------------------------------------------------

    thread_local! {
        /// How often the stand-in tmux was asked.
        static ASKED: Cell<usize> = const { Cell::new(0) };
    }

    /// tmux's answer for a pane whose one client is Ghostty 1.2.0 the
    /// first time it is asked, and WezTerm, which shows no progress, from
    /// then on, as after an attach from another terminal.
    fn ghostty_then_wezterm(_: &[&str]) -> Option<String> {
        let asked = ASKED.get();
        ASKED.set(asked + 1);
        let termtype = if asked == 0 {
            "ghostty 1.2.0"
        } else {
            "WezTerm 20240203-110809-5046fc22"
        };
        Some(format!("on\t1\t{termtype}\t1\t1\t0\t3.5\n"))
    }

    fn no_tmux(_: &[&str]) -> Option<String> {
        None
    }

    /// A writer of the progress for a terminal whose environment holds
    /// `vars`, and the channel its keep-alive ticks into.
    fn progress_out(
        vars: &[(&str, &str)],
        ask_tmux: fn(&[&str]) -> Option<String>,
    ) -> (ProgressOut, mpsc::Receiver<Event>) {
        let env = TerminalEnv::from_pairs(vars);
        let (ticks, ticked) = mpsc::channel();
        let shown = ShownProgress::new(env.in_tmux_pane());
        (ProgressOut::new(env, ask_tmux, shown, ticks), ticked)
    }

    fn written_by(out: &mut ProgressOut, percent: Option<u8>) -> String {
        let mut written = Vec::new();
        out.write(&mut written, percent).expect("written");
        String::from_utf8(written).expect("escape sequences are ASCII")
    }

    #[test]
    fn a_terminal_that_shows_progress_gets_it_and_its_removal() {
        let (mut out, _ticked) = progress_out(&[("WT_SESSION", "1")], no_tmux);

        assert_eq!(written_by(&mut out, Some(40)), "\x1b]9;4;1;40\x1b\\");
        assert!(out.shown.shown.load(Ordering::Relaxed));
        assert_eq!(written_by(&mut out, None), REMOVE_PROGRESS);
        assert!(!out.shown.shown.load(Ordering::Relaxed));
        assert!(out.keep_alive.is_none(), "Windows Terminal keeps it");
    }

    #[test]
    fn a_terminal_not_known_to_show_progress_gets_nothing() {
        let (mut out, _ticked) = progress_out(&[("TERM_PROGRAM", "WezTerm")], no_tmux);

        assert_eq!(written_by(&mut out, Some(40)), "");
        assert_eq!(written_by(&mut out, None), "");
        assert!(!out.shown.shown.load(Ordering::Relaxed));
    }

    #[test]
    fn ghostty_keeps_the_progress_alive_while_one_shows() {
        let ghostty = [
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.2.0"),
        ];
        let (mut out, _ticked) = progress_out(&ghostty, no_tmux);

        written_by(&mut out, Some(40));
        assert!(out.keep_alive.as_ref().is_some_and(KeepAlive::is_active));
        written_by(&mut out, None);
        assert!(
            out.keep_alive
                .as_ref()
                .is_some_and(|keep| !keep.is_active())
        );
    }

    #[test]
    fn inside_tmux_the_client_is_asked_before_each_write() {
        ASKED.set(0);
        let (mut out, _ticked) = progress_out(
            &[("TMUX", "/tmp/tmux-501/default,1,0")],
            ghostty_then_wezterm,
        );

        assert_eq!(
            written_by(&mut out, Some(40)),
            "\x1bPtmux;\x1b\x1b]9;4;1;40\x1b\x1b\\\x1b\\"
        );
        assert!(out.keep_alive.as_ref().is_some_and(KeepAlive::is_active));

        assert_eq!(written_by(&mut out, Some(60)), "", "WezTerm attached");
        assert_eq!(ASKED.get(), 2);
        assert!(
            !out.shown.shown.load(Ordering::Relaxed),
            "nothing to remove on quitting"
        );
        assert!(
            out.keep_alive
                .as_ref()
                .is_some_and(|keep| !keep.is_active())
        );
    }

    #[test]
    fn a_progress_the_terminal_refuses_is_an_error() {
        let (mut out, _ticked) = progress_out(&[("WT_SESSION", "1")], no_tmux);
        let mut full: [u8; 0] = [];

        assert!(out.write(&mut &mut full[..], Some(40)).is_err());
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
        let pushed = AtomicBool::new(false);

        write_title(&mut written, "asqr · 1 waiting", &pushed).expect("written");
        write_title(&mut written, "asqr · 0 waiting", &pushed).expect("written");

        assert_eq!(
            String::from_utf8(written).expect("UTF-8"),
            "\x1b[22;0t\x1b]0;asqr · 1 waiting\x07\x1b]0;asqr · 0 waiting\x07"
        );
        assert!(pushed.load(Ordering::Relaxed));
    }

    #[test]
    fn a_push_the_terminal_refused_is_not_popped() {
        let mut full: [u8; 0] = [];
        let pushed = AtomicBool::new(false);

        assert!(write_title(&mut &mut full[..], "asqr · 1 waiting", &pushed).is_err());

        assert!(!pushed.load(Ordering::Relaxed));
    }

    /// The book of a Kitty image laid out and still shown, and the image.
    fn book_with_an_image() -> (KittyBook, Image) {
        let images = Images::new(Graphics {
            protocol: Protocol::Kitty,
            ..Graphics::HALF_BLOCKS
        });
        // The image as a file holds it: the binary's tests see only what
        // the library offers outside its own tests.
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(20, 40)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("PNG takes 8-bit RGBA, written into memory");
        let mut image = images.decode(png, "red.png").expect("a PNG decodes");
        image.lay_out(Size::new(2, 2), true);
        (images.kitty_book().clone(), image)
    }

    /// The panic hook deletes the images still shown, then gives the
    /// title back and turns the modes off. Run again, as the guard's drop
    /// runs after it, only the modes are turned off again.
    #[test]
    fn a_panic_deletes_the_images_and_gives_the_title_back_once() {
        let (book, _shown) = book_with_an_image();
        let pushed = AtomicBool::new(true);
        let mut written = Vec::new();

        give_back_on_panic(&mut written, &book, &pushed, &ShownProgress::new(false));
        give_back_on_panic(&mut written, &book, &pushed, &ShownProgress::new(false));

        let written = String::from_utf8(written).expect("escape sequences are ASCII");
        let modes = "\x1b[?1004l\x1b[?2004l";
        let given_back = format!("\x1b]0;\x07\x1b[23;0t{modes}{modes}");
        let delete = written
            .strip_suffix(&given_back)
            .unwrap_or_else(|| panic!("the title once, the modes twice: {written:?}"));
        assert!(
            delete.starts_with("\x1b_Ga=d,d=I,i=") && delete.matches("\x1b_G").count() == 1,
            "one delete first: {written:?}"
        );
    }

    #[test]
    fn a_panic_hook_that_cannot_write_goes_on() {
        let (book, _shown) = book_with_an_image();
        let pushed = AtomicBool::new(true);
        // A full buffer fails every write with `WriteZero`.
        let mut full: [u8; 0] = [];

        give_back_on_panic(
            &mut &mut full[..],
            &book,
            &pushed,
            &ShownProgress::new(false),
        );

        assert!(book.drain().is_empty(), "the deletes were taken");
        assert!(
            !pushed.load(Ordering::Relaxed),
            "the pop is not tried twice"
        );
    }

    /// A picker that found no graphics protocol says why in the log: with
    /// no capability at all the terminal gave no usable answer, which
    /// includes a timeout and an answer without a cell size.
    #[test]
    fn the_log_says_why_images_are_half_blocks() {
        assert_eq!(
            half_blocks_reason(ProtocolType::Halfblocks, &[]),
            Some("the terminal answered the graphics query without a cell size, or not at all")
        );
        assert_eq!(
            half_blocks_reason(
                ProtocolType::Halfblocks,
                &[Capability::CellSize(Some((10, 20)))]
            ),
            Some("the terminal named no graphics protocol asqr speaks")
        );
        for protocol in [
            ProtocolType::Kitty,
            ProtocolType::Sixel,
            ProtocolType::Iterm2,
        ] {
            assert_eq!(half_blocks_reason(protocol, &[]), None, "{protocol:?}");
        }
    }

    #[test]
    fn a_terminal_that_refuses_the_sequences_is_still_restored() {
        // A full buffer fails every write with `WriteZero`.
        let mut full: [u8; 0] = [];
        drop(TerminalGuard {
            out: &mut full[..],
            restore: note_restore,
            title_pushed: Arc::new(AtomicBool::new(true)),
            progress: ShownProgress::new(false),
        });
        assert!(RESTORED.get());
    }
}
