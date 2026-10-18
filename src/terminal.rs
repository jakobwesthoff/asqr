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
use std::time::Instant;

use anyhow::Context;
use asqr::cli::{Exit, Watch};
use asqr::graphics::{
    Graphics, Images, KittyBook, KittyDeletes, KittyExit, PanePassthrough, QUERY_WAIT,
    answers_may_follow_the_query, may_query_protocol, passthrough_all_for_images,
    settle_passthrough,
};
#[cfg(unix)]
use asqr::graphics::{LATE_ANSWER_CAP, LATE_ANSWER_QUIET, discard_late_answers};
use asqr::terminal_env::{TerminalEnv, TmuxState};
use asqr::tui::render::{Glyphs, View, loader_panicked};
use asqr::tui::{Alerts, Event, Host, alert, run, start, watch_inbox};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    Event as TerminalEvent,
};
use ratatui::crossterm::terminal::SetTitle;
use ratatui::crossterm::{execute, queue, terminal};
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
    let _guard = TerminalGuard {
        out: io::stdout(),
        restore: ratatui::restore,
        title_pushed: Arc::clone(&title_pushed),
    };
    // SIGTERM, SIGINT and SIGHUP would end the process without the guard.
    // Caught, they end the run as `q` does, and the guard runs. SIGKILL
    // cannot be caught and leaves the terminal as set up here.
    #[cfg(unix)]
    forward_quit_signals(sender.clone());

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
        "graphics protocol detected"
    );
    log_window_size();

    let images = Images::new(graphics);
    hook_in_before_ratatui_on_panic(images.kitty_book().clone(), Arc::clone(&title_pushed));
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
        env,
    };
    let mut view = View::new(&queue, watch.layout(), images)
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
    run(
        &mut terminal,
        &mut app,
        &mut inbox,
        &mut view,
        &events,
        &mut host,
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

/// Gives back the title asqr pushed and turns focus reports and bracketed
/// paste off, before the alternate screen is left. On a panic the hook
/// and then the guard both run it; the title is given back once.
fn give_back_title_and_modes(out: &mut impl Write, title_pushed: &AtomicBool) {
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
        give_back_title_and_modes(&mut self.out, &self.title_pushed);
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
fn hook_in_before_ratatui_on_panic(book: KittyBook, title_pushed: Arc<AtomicBool>) {
    let ratatui_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A panic while loading a context entry is handed to the drawing
        // thread, which panics with it; the terminal is given back there,
        // once nothing draws any more.
        if loader_panicked(info) {
            return;
        }
        give_back_on_panic(&mut io::stdout(), &book, &title_pushed);
        ratatui_hook(info);
    }));
}

/// Deletes the Kitty images, those still shown included, gives the title
/// back and turns focus reports and bracketed paste off, on a panic.
///
/// The hook runs before the unwinding lets go of a lock, so it only tries
/// the book: after a panic while the book was locked, the exit guard
/// writes the deletes, once the alternate screen is gone already. What
/// the hook took, the exit guard does not write again.
fn give_back_on_panic(out: &mut impl Write, book: &KittyBook, title_pushed: &AtomicBool) {
    if let Some(deletes) = book.try_drain().filter(|deletes| !deletes.is_empty()) {
        let deleted = out
            .write_all(&deletes.to_bytes())
            .and_then(|()| out.flush());
        if let Err(error) = deleted {
            tracing::warn!(%error, "could not delete the images");
        }
    }
    give_back_title_and_modes(out, title_pushed);
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
    env: TerminalEnv,
}

impl Host for TerminalHost {
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

    fn open_image(&mut self, path: &str) -> io::Result<()> {
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

    use asqr::graphics::{Image, Protocol};
    use ratatui::layout::Size;

    use super::*;

    thread_local! {
        static RESTORED: Cell<bool> = const { Cell::new(false) };
    }

    fn note_restore() {
        RESTORED.set(true);
    }

    /// What dropping a guard writes, when asqr did or did not push the
    /// title.
    fn dropped(pushed: bool) -> String {
        let mut written = Vec::new();
        drop(TerminalGuard {
            out: &mut written,
            restore: note_restore,
            title_pushed: Arc::new(AtomicBool::new(pushed)),
        });
        String::from_utf8(written).expect("escape sequences are ASCII")
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

        give_back_on_panic(&mut written, &book, &pushed);
        give_back_on_panic(&mut written, &book, &pushed);

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

        give_back_on_panic(&mut &mut full[..], &book, &pushed);

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
        });
        assert!(RESTORED.get());
    }
}
