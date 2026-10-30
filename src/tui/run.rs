// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The running app (spec section 7): draw, wait for the next event, act on
//! it, draw again. Events arrive through a channel that the binary feeds
//! from the terminal and the inbox watcher, and everything that reaches out
//! of the app goes through [`Host`], so tests run the loop against a test
//! backend.

use std::collections::VecDeque;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use anyhow::Context;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{KeyEvent, KeyEventKind};
use ratatui::layout::{Position, Rect};
use unicode_segmentation::UnicodeSegmentation;

use super::render::{Drawn, View, draw, picture, shorten_from_left};
use super::{
    App, Applied, EditRequest, Inbox, Notice, OpenedFiles, Tone, edited_text, editor_file,
    session_progress,
};
use crate::graphics::KittyDeletes;
use crate::queue::QueueLocation;

/// What wakes the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Key(KeyEvent),
    /// Text the person pasted, which a terminal with bracketed paste sends
    /// as a whole rather than as keys.
    Paste(String),
    /// The terminal changed its size; the next draw adapts.
    Resize,
    /// Something changed in the inbox.
    InboxChanged,
    /// The terminal has the focus again.
    FocusGained,
    /// A context entry finished loading apart from drawing; the next draw
    /// shows it (ADR 46).
    Loaded,
    /// SIGTERM, SIGINT or SIGHUP asked asqr to quit (spec section 7.1).
    Quit,
    /// A period of the keep-alive passed: the terminal clears the progress
    /// unless it gets it again (ADR 73).
    KeepAlive,
    /// The key reader ended, with the reason it gives: nothing reads the
    /// terminal's input any more.
    InputEnded(String),
    /// The system's opener, started for `o`, ended with an error; the
    /// status line shows what [`opener_failure`](super::opener_failure)
    /// says about it.
    OpenerFailed(String),
}

impl Event {
    /// Whether the event is what the person does: a key pressed, a paste,
    /// a resize or a quit. A draw with one of them waiting is replaced by
    /// the next at once, so it does not wait for its context entry. A key
    /// released or repeated counts as nothing, as the loop drops it.
    pub fn is_input(&self) -> bool {
        match self {
            Event::Key(key) => key.kind == KeyEventKind::Press,
            Event::Paste(_) | Event::Resize | Event::Quit => true,
            Event::InboxChanged
            | Event::FocusGained
            | Event::Loaded
            | Event::KeepAlive
            | Event::InputEnded(_)
            | Event::OpenerFailed(_) => false,
        }
    }
}

/// What the app asks of the world around it.
pub trait Host {
    /// Sessions arrived while asqr was running.
    fn alert(&mut self, arrived: &[String]);
    /// Open the file at `path` with the system's opener, in the app the
    /// system has for its type; the status line names it `name`.
    fn open(&mut self, path: &Path, name: &str) -> io::Result<()>;
    /// Show `title` as the terminal's title, unless the person switched
    /// titles off.
    fn title(&mut self, title: &str);
    /// Ask the terminal to delete the Kitty images `deletes` names, which
    /// the frame just written no longer shows (ADR 42).
    fn delete_images(&mut self, deletes: &KittyDeletes) -> io::Result<()>;
    /// Show `percent` as the terminal's progress, or remove the progress
    /// for `None`, where the terminal is known to show it (ADR 73).
    fn progress(&mut self, percent: Option<u8>);
    /// Hand the terminal to the person's editor on the file at `path`, and
    /// take it back once the editor exits (ADR 74). The error's kind says
    /// what went wrong: `NotFound` for no editor, `WouldBlock` for a
    /// terminal whose input could not be freed, `Other` for an editor that
    /// exited with an error.
    fn edit(&mut self, path: &Path) -> io::Result<()>;
}

/// Opens the field of `request` in the person's editor through `host` and
/// puts the text left above the file's cut line into it, or says in the
/// status line why it cannot (ADR 74). Says whether the editor had the
/// terminal, which is then drawn afresh.
///
/// The file is read by its path: an editor that saves into a new file and
/// renames it over the old one leaves the file asqr opened unchanged. Where
/// the text would be lost, as when the cut line is gone or the session
/// left while the editor ran, the file stays and the status line names it.
fn edit_in_editor(
    host: &mut impl Host,
    inbox: &mut Inbox,
    app: &mut App,
    request: &EditRequest,
) -> bool {
    let written = tempfile::Builder::new()
        .prefix("asqr-")
        .suffix(".md")
        .tempfile()
        .and_then(|mut file| {
            file.write_all(editor_file(request).as_bytes())?;
            file.flush()?;
            Ok(file)
        });
    let file = match written {
        Ok(file) => file,
        Err(error) => {
            app.notice(
                Tone::Error,
                format!("cannot write the file for the editor: {error}"),
            );
            return false;
        }
    };
    if let Err(error) = host.edit(file.path()) {
        app.notice(Tone::Error, editor_failure(&error));
        return true;
    }
    let read = fs::read(file.path()).and_then(|bytes| {
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    });
    let content = match read {
        Ok(content) => content,
        Err(error) => {
            app.notice(
                Tone::Error,
                format!("the field is unchanged: cannot read the editor's file: {error}"),
            );
            return true;
        }
    };
    let Some(text) = edited_text(&content) else {
        let kept = kept(file);
        app.notice(
            Tone::Warning,
            format!("{kept}the >8 line is gone, field unchanged"),
        );
        return true;
    };
    // A session withdrawn or finished elsewhere while the editor ran keeps
    // no draft, so the text stays in its file and not in the field, which
    // leaves with the session. The sync that sees the session leave shows
    // a notice of its own in the status line's one place, so the inbox
    // names the file there again.
    let id = &request.about.id;
    if !inbox.waiting(id) {
        let name = request.about.title.as_deref().unwrap_or(id);
        let kept = kept(file);
        app.show(Notice::naming(
            Tone::Warning,
            &format!("{kept}\""),
            name,
            "\" left while the editor ran",
        ));
        if !kept.is_empty() {
            inbox.keep_text_for(id, kept);
        }
        return true;
    }
    let effect = app.edited(&text);
    // The edited text asks for a draft save at most, which says nothing
    // back to the loop.
    inbox.apply(app, effect);
    true
}

/// Keeps `file` past the run, and says where it is as the start of a
/// message, or nothing where it cannot be kept. The status line is cut at
/// the screen's edge, so the path comes first and short: the system's temp
/// directory as `$TMPDIR` where that variable names it. The log has the
/// whole path.
fn kept(file: tempfile::NamedTempFile) -> String {
    match file.keep() {
        Ok((_, path)) => {
            tracing::info!(path = %path.display(), "kept the editor's file");
            let tmpdir = std::env::var_os("TMPDIR");
            format!("your text is in {}: ", shown_path(&path, tmpdir.as_deref()))
        }
        Err(error) => {
            tracing::warn!(%error, "could not keep the editor's file");
            String::new()
        }
    }
}

/// `path` as the status line names it: under `tmpdir`, the value of
/// `TMPDIR`, as `$TMPDIR/<name>`, otherwise whole.
fn shown_path(path: &Path, tmpdir: Option<&std::ffi::OsStr>) -> String {
    tmpdir
        .filter(|dir| !dir.is_empty())
        .and_then(|dir| path.strip_prefix(dir).ok())
        .map_or_else(
            || path.display().to_string(),
            |name| format!("$TMPDIR/{}", name.display()),
        )
}

/// What the status line says when the editor did not give a text back.
fn editor_failure(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "no editor found: set VISUAL or EDITOR".to_owned(),
        io::ErrorKind::WouldBlock => {
            "the editor did not start: the terminal's input is busy; press ctrl-g again".to_owned()
        }
        io::ErrorKind::Other => {
            "the field is unchanged: the editor exited with an error".to_owned()
        }
        _ => format!("the field is unchanged: cannot run the editor: {error}"),
    }
}

/// What the run loop last sent as the progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SentProgress {
    /// The terminal may show anything, as after a focus gain under tmux;
    /// the next pass sends what is current, a remove included.
    Unknown,
    /// The terminal shows no progress.
    Removed,
    Shown(u8),
}

impl SentProgress {
    fn of(percent: Option<u8>) -> Self {
        percent.map_or(SentProgress::Removed, SentProgress::Shown)
    }

    /// Whether `percent` has to go out to bring the terminal to it.
    fn differs_from(self, percent: Option<u8>) -> bool {
        self != SentProgress::of(percent)
    }
}

/// The most columns the queue takes in the terminal title. Terminals and
/// tmux cut a long tab title at their own width anyway, and 40 columns
/// leave room for `asqr · ` and ` · N waiting` in a tab of usual width.
const TITLE_QUEUE_COLUMNS: usize = 40;

/// The most bytes of a terminal title: Ghostty drops a title of 256 bytes
/// or more whole, the count included (ADR 40).
const TITLE_MAX_BYTES: usize = 255;

/// The terminal title for the state of `app`, so a tab in the background
/// tells how many sessions wait (spec section 7.9), and which queue they
/// wait in where `queue` names a chosen one (ADR 81).
pub fn terminal_title(app: &App, queue: Option<&str>) -> String {
    let count = format!("{} waiting", app.sessions().len());
    let Some(queue) = queue else {
        return format!("asqr · {count}");
    };
    // A BEL or an ESC from a `--dir` path would end the title's OSC
    // sequence early. Their pictures also take a column each, which the
    // cut to 40 columns has to count.
    let pictured: String = queue
        .chars()
        .map(|c| if c.is_control() { picture(c) } else { c })
        .collect();
    let shown = shorten_from_left(&pictured, TITLE_QUEUE_COLUMNS);
    // Combining marks take no column, so 40 columns can still hold more
    // bytes than Ghostty takes.
    let frame = format!("asqr ·  · {count}").len();
    let shown = within_bytes(&shown, TITLE_MAX_BYTES.saturating_sub(frame));
    format!("asqr · {shown} · {count}")
}

/// `text` in at most `room` bytes: whole where it fits, otherwise its end
/// behind `…`, taken grapheme by grapheme so no character is split.
fn within_bytes(text: &str, room: usize) -> String {
    if text.len() <= room {
        return text.to_owned();
    }
    let keep = room.saturating_sub('…'.len_utf8());
    let start = text
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|&index| text.len() - index <= keep)
        .unwrap_or(text.len());
    format!("…{}", &text[start..])
}

/// Reads the queue for the first time: what waits when asqr starts is
/// shown without an alert. Register the watcher before (spec section 5.4).
pub fn start(location: QueueLocation) -> anyhow::Result<(App, Inbox)> {
    let mut app = App::default();
    let mut inbox = Inbox::new(location);
    inbox.sync(&mut app).context("reading the inbox")?;
    Ok((app, inbox))
}

/// Clears the screen and forgets the last frame, so the next draw writes
/// every cell. A resize to the size the screen has does what
/// [`Terminal::clear`] does without asking the terminal where its cursor
/// is: a terminal that does not answer would end the run after the
/// query's wait, and the query's answer would compete with the key
/// reader for the terminal's input.
fn clear_screen<B: Backend>(terminal: &mut Terminal<B>) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    let size = terminal.size().context("measuring the screen")?;
    terminal
        .resize(Rect::from((Position::ORIGIN, size)))
        .context("clearing the screen")
}

/// Runs the app until the person quits or the events end. `o` writes the
/// files it opens into `opened`.
pub fn run<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    inbox: &mut Inbox,
    view: &mut View,
    events: &Receiver<Event>,
    host: &mut impl Host,
    opened: &mut OpenedFiles,
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    // Every key draws again, but the title goes to the terminal only when
    // its text changes. No title is empty, so the first pass sends one.
    let mut sent_title = String::new();
    // The progress goes out only when it changes too. The terminal shows
    // none before asqr sends one, so a queue that starts empty sends
    // nothing.
    let mut sent_progress = SentProgress::Removed;
    // The events taken from the channel before they are handled, one a
    // pass with a draw between, as each key may depend on what the draw
    // before it measured.
    let mut ahead: VecDeque<Event> = VecDeque::new();
    // The person's editor had the terminal, so the next frame is drawn
    // whole (ADR 74).
    let mut redraw = false;
    loop {
        let title = terminal_title(app, view.named_queue());
        if title != sent_title {
            host.title(&title);
            sent_title = title;
        }
        let progress = session_progress(app);
        if sent_progress.differs_from(progress) {
            host.progress(progress);
            sent_progress = SentProgress::of(progress);
        }
        // A draw with input waiting behind it does not wait for its entry
        // (ADR 46). All that arrived is looked at, as the load threads
        // send their wake-ups into the same channel as the keys and one
        // in front would hide a key behind it. A wake-up only asks for
        // the draw that comes anyway, so it goes. Keep-alives pile up
        // while the loop waits on the host, as for the editor, and one
        // sends what they all would. Everything else stays, the end of
        // the input included.
        while let Ok(event) = events.try_recv() {
            let piled_up = event == Event::KeepAlive && ahead.contains(&Event::KeepAlive);
            if event != Event::Loaded && !piled_up {
                ahead.push_back(event);
            }
        }
        view.set_input_pending(ahead.iter().any(Event::is_input));
        // The editor left the screen to asqr to draw (ADR 74), and an
        // overlay closed over an image leaves its text on the image unless
        // the whole screen is drawn afresh (ADR 31).
        if std::mem::take(&mut redraw) || view.must_clear(app) {
            clear_screen(terminal)?;
        }
        let mut drawn = Drawn::default();
        // A frame that sends a Kitty image is logged with its bytes, the
        // time of the whole draw, which includes the wait for the image's
        // layout, and the time from the drawn buffer to the flushed write,
        // which is what tmux slows.
        let started = Instant::now();
        let mut built = started;
        terminal
            .draw(|frame| {
                drawn = draw(frame, app, view);
                built = Instant::now();
            })
            .context("drawing the screen")?;
        if drawn.image_sent > 0 {
            tracing::info!(
                bytes = drawn.image_sent,
                took = ?started.elapsed(),
                written = ?built.elapsed(),
                "frame sent a Kitty image"
            );
        }
        // The frame is flushed and shows no Kitty image that went, so the
        // terminal may let go of them (ADR 42). A frame that failed returned
        // above and left them in the book, for the deletes on quitting.
        let book = view.kitty_book();
        let deletes = book.after_frame();
        if !deletes.is_empty()
            && let Err(error) = host.delete_images(&deletes)
        {
            tracing::warn!(%error, "could not delete the images");
            book.put_back(deletes);
        }
        // How far the intro overlay and the context pane can scroll depends
        // on the size they were drawn at, and where `↑`/`↓` go in a field
        // on where its rows break. A frame without a tab, as below the
        // minimum size, says nothing about the intro or the pane: handing
        // it on would end the pane's focus and the full-screen view, and the
        // session shown at the next tab may be another one. How far the
        // keys move the question's area for the row under the cursor is
        // known from a frame that drew the tab; one below the minimum size
        // drops it, as the keys still reach the question there.
        if let Some(page) = drawn.intro_page {
            app.set_intro_page(page);
        }
        if let Some(page) = drawn.pane_page {
            app.set_pane_page(page);
        }
        if let Some(cut) = drawn.row_cut {
            app.set_row_cut(cut);
        }
        app.set_dialog_shown(drawn.invalid_dialog);
        app.set_session_shown(drawn.session_shown);
        // A field entered from above finds its first row only in its first
        // draw, and its text cursor goes there; that draw showed it
        // elsewhere.
        if app.set_field_shape(drawn.field_shape) {
            continue;
        }
        // A keep-alive only sends the progress again, which needs no
        // frame, so the loop takes the next event without drawing.
        let event = loop {
            let event = match ahead.pop_front() {
                Some(event) => event,
                None => match events.recv() {
                    Ok(event) => event,
                    Err(_) => return Ok(()),
                },
            };
            if event != Event::KeepAlive {
                break event;
            }
            if let SentProgress::Shown(percent) = sent_progress {
                host.progress(Some(percent));
            }
        };
        let effect = match event {
            // Terminals that report releases and repeats (the kitty
            // keyboard protocol, Windows) would otherwise act twice.
            Event::Key(key) if key.kind != KeyEventKind::Press => continue,
            Event::Key(key) => app.handle(key),
            Event::Paste(text) => app.paste(&text),
            // A keep-alive was taken above and never gets here.
            Event::Resize | Event::Loaded | Event::KeepAlive => continue,
            // The opener ends on a thread of its own, after the key that
            // started it; only its failure comes back.
            Event::OpenerFailed(failure) => {
                app.notice(Tone::Error, failure);
                continue;
            }
            // The caller gives the terminal back, as after `q`.
            Event::Quit => return Ok(()),
            // The other senders keep the channel open, so without this
            // asqr would stay on screen and ignore every key. The caller
            // gives the terminal back and exits with 1.
            Event::InputEnded(reason) => {
                return Err(anyhow::Error::msg(reason)).context("reading the terminal's input");
            }
            // Coming back into view is when a transfer lost in the
            // background can be repeated, where one can be lost at all,
            // the progress included: tmux drops passthrough from a pane no
            // client shows (spec section 8.9).
            Event::FocusGained => {
                view.focus_gained();
                sent_progress = SentProgress::Unknown;
                continue;
            }
            // The inbox coming back or not is out of asqr's hands; the
            // notice says why nothing changes, and every later change
            // tries again.
            Event::InboxChanged => {
                match inbox.sync(app) {
                    Ok(synced) => {
                        if !synced.arrived.is_empty() {
                            host.alert(&synced.arrived);
                        }
                    }
                    Err(error) => {
                        app.notice(Tone::Error, format!("cannot read the inbox: {error}"))
                    }
                }
                continue;
            }
        };
        match inbox.apply(app, effect) {
            Applied::Continue => {}
            Applied::Quit => return Ok(()),
            Applied::Edit(request) => {
                // Nothing else runs while the editor has the terminal:
                // what arrived meanwhile waits in the channel, so the text
                // goes into the field `ctrl-g` came from.
                if edit_in_editor(host, inbox, app, &request) {
                    view.screen_regained();
                    redraw = true;
                    // The editor may have set a title, or a progress, of
                    // its own.
                    sent_title.clear();
                    sent_progress = SentProgress::Unknown;
                }
            }
            Applied::Open(entry) => match opened.file_for(&entry) {
                Ok((path, name)) => {
                    if let Err(error) = host.open(&path, &name) {
                        app.notice(Tone::Error, format!("cannot open {name}: {error}"));
                    }
                }
                Err(refusal) => app.notice(Tone::Error, refusal),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::mpsc;

    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEventState, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::format::{SessionResult, Status};
    use crate::graphics::testing::{kitty_ids_on_screen, transmission};
    use crate::graphics::{Graphics, Images, Passthrough, Protocol};
    use crate::tui::render::{PaneLayout, test_support};

    const SESSION: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Pick one", "kind": "single",
        "context": [{"type": "image", "path": "/nowhere/cat.png"}],
        "options": [{"id": "a", "label": "Apples"}, {"id": "b", "label": "Bananas"}]}]}"#;

    #[derive(Default)]
    struct Recorder {
        alerts: Vec<Vec<String>>,
        /// The paths the opener got, in order.
        opened: Vec<String>,
        /// What each of them held as the opener got it.
        contents: Vec<Vec<u8>>,
        titles: Vec<String>,
        /// Opening a file fails with this, when set.
        viewer_missing: bool,
        /// Writing deletes fails, as writing to a terminal that went away
        /// does, when set.
        refuse_deletes: bool,
        /// Where the Kitty images the loop deletes go, among the frames a
        /// [`Logging`] backend notes.
        log: Log,
        /// Every progress the loop sent, in order.
        progress: Vec<Option<u8>>,
        /// What the person's editor does with the file it gets; without
        /// one, it exits at once and leaves the file as it was.
        editor: Option<Editor>,
        /// The files the editor got, in order.
        edited: Vec<PathBuf>,
    }

    /// A stand-in for the person's editor.
    type Editor = Box<dyn FnMut(&Path) -> io::Result<()>>;

    impl Host for Recorder {
        fn progress(&mut self, percent: Option<u8>) {
            self.progress.push(percent);
        }

        fn edit(&mut self, path: &Path) -> io::Result<()> {
            self.edited.push(path.to_path_buf());
            match &mut self.editor {
                Some(editor) => editor(path),
                None => Ok(()),
            }
        }

        fn delete_images(&mut self, deletes: &KittyDeletes) -> io::Result<()> {
            if self.refuse_deletes {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "terminal gone"));
            }
            self.log
                .borrow_mut()
                .push(Step::Deleted(deletes.ids().to_vec()));
            Ok(())
        }

        fn title(&mut self, title: &str) {
            self.titles.push(title.to_owned());
        }

        fn alert(&mut self, arrived: &[String]) {
            self.alerts.push(arrived.to_vec());
        }

        fn open(&mut self, path: &Path, _name: &str) -> io::Result<()> {
            if self.viewer_missing {
                return Err(io::Error::new(io::ErrorKind::NotFound, "no viewer"));
            }
            self.opened.push(path.display().to_string());
            self.contents.push(fs::read(path).unwrap_or_default());
            Ok(())
        }
    }

    struct Harness {
        scratch: tempfile::TempDir,
        location: QueueLocation,
        terminal: Terminal<TestBackend>,
        host: Recorder,
        /// Where `o` writes, inside the scratch directory, so no test
        /// writes into the system's temp directory.
        opened: OpenedFiles,
    }

    impl Harness {
        fn new() -> Self {
            let scratch = tempfile::tempdir().expect("temp dir");
            let location = QueueLocation::at(scratch.path().join("queue"));
            location.create_layout().expect("layout");
            let opened = OpenedFiles::under(scratch.path().join("opened"));
            Harness {
                scratch,
                location,
                terminal: Terminal::new(TestBackend::new(80, 24)).expect("test terminal"),
                host: Recorder::default(),
                opened,
            }
        }

        fn put(&self, id: &str) {
            fs::write(self.location.inbox().join(format!("{id}.json")), SESSION).expect("written");
        }

        /// Puts a session whose question shows `entry`, an entry as JSON.
        fn put_showing(&self, id: &str, entry: &str) {
            let session =
                SESSION.replace(r#"{"type": "image", "path": "/nowhere/cat.png"}"#, entry);
            fs::write(self.location.inbox().join(format!("{id}.json")), session).expect("written");
        }

        /// Writes `bytes` to the file `name` in the scratch directory and
        /// returns its path.
        fn scratch_file(&self, name: &str, bytes: &[u8]) -> String {
            let path = self.scratch.path().join(name);
            fs::write(&path, bytes).expect("written");
            path.to_str().expect("UTF-8").to_owned()
        }

        /// Starts the app, then runs it over `events`; the loop ends when
        /// they run out unless a key quits first.
        fn run(&mut self, events: Vec<Event>) -> anyhow::Result<()> {
            let (mut app, mut inbox) = start(self.location.clone())?;
            self.run_started(&mut app, &mut inbox, events)
        }

        fn run_started(
            &mut self,
            app: &mut App,
            inbox: &mut Inbox,
            events: Vec<Event>,
        ) -> anyhow::Result<()> {
            let mut view = View::for_tests("test", PaneLayout::Auto, Images::half_blocks());
            self.run_with_view(app, inbox, &mut view, events)
        }

        /// Like [`Harness::run_started`], keeping the view, and with it the
        /// images loaded so far, from one run to the next.
        fn run_with_view(
            &mut self,
            app: &mut App,
            inbox: &mut Inbox,
            view: &mut View,
            events: Vec<Event>,
        ) -> anyhow::Result<()> {
            let (sender, receiver) = mpsc::channel();
            for event in events {
                sender.send(event).expect("sent");
            }
            drop(sender);
            run(
                &mut self.terminal,
                app,
                inbox,
                view,
                &receiver,
                &mut self.host,
                &mut self.opened,
            )
        }

        fn screen(&self) -> String {
            let buffer = self.terminal.backend().buffer();
            buffer
                .content
                .chunks(buffer.area.width as usize)
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn result(&self, id: &str) -> SessionResult {
            let path = self.location.outbox().join(format!("{id}.json"));
            serde_json::from_slice(&fs::read(path).expect("result")).expect("parses")
        }
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn shows_what_waits_and_quits_on_q_without_alerting() {
        let mut harness = Harness::new();
        harness.put("batch-01");

        harness
            .run(vec![key(KeyCode::Char('q')), key(KeyCode::Char('j'))])
            .expect("runs");

        assert!(
            harness.screen().contains("Pick one"),
            "{}",
            harness.screen()
        );
        assert!(
            harness.host.alerts.is_empty(),
            "waiting sessions are no news"
        );
    }

    /// A frame of the loop shows the active session, so its reload says
    /// so; the session waiting behind it was never on screen and reloads
    /// without a word.
    #[test]
    fn a_reload_names_itself_only_for_a_session_already_shown() {
        let mut harness = Harness::new();
        harness.put("a");
        harness.put("b");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        harness
            .run_started(&mut app, &mut inbox, vec![])
            .expect("runs");
        let inbox_dir = harness.location.inbox();
        let rewrite = |id: &str, text: &str| {
            let path = inbox_dir.join(format!("{id}.json"));
            fs::write(path, SESSION.replace("Pick one", text)).expect("written");
        };

        rewrite("a", "Pick again");
        harness
            .run_started(&mut app, &mut inbox, vec![Event::InboxChanged])
            .expect("runs");
        let reloaded = "a changed on disk and was reloaded";
        assert_eq!(
            app.active()
                .and_then(crate::tui::SessionState::message)
                .map(|message| message.text.as_str()),
            Some(reloaded)
        );
        assert!(harness.screen().contains(reloaded), "{}", harness.screen());

        rewrite("b", "Pick later");
        harness
            .run_started(&mut app, &mut inbox, vec![Event::InboxChanged])
            .expect("runs");
        assert_eq!(app.sessions()[1].session().questions[0].text, "Pick later");
        assert_eq!(app.sessions()[1].message(), None);
    }

    // -----------------------------------------------------------------
    // A question taller than the area on a small terminal
    // -----------------------------------------------------------------

    /// A harness on a terminal of 60×15, the smallest asqr draws, with
    /// `session` waiting and the app started on it. Without header text
    /// the question's area has 10 rows there.
    fn small_with(session: &serde_json::Value) -> (Harness, App, Inbox) {
        let mut harness = Harness::new();
        harness.terminal = Terminal::new(TestBackend::new(60, 15)).expect("test terminal");
        let path = harness.location.inbox().join("tall.json");
        fs::write(path, session.to_string()).expect("written");
        let (app, inbox) = start(harness.location.clone()).expect("started");
        (harness, app, inbox)
    }

    /// `count` words `<prefix>001`, `<prefix>002` and so on, each of which
    /// shows whole or not at all, so a word cut by the area's edge or
    /// covered by a marker counts as not shown.
    fn words(prefix: char, count: usize) -> String {
        (1..=count)
            .map(|n| format!("{prefix}{n:03}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The words of [`words`] with `prefix` that `screen` shows whole.
    fn shown_words(screen: &str, prefix: char) -> BTreeSet<String> {
        screen
            .split_whitespace()
            .filter(|word| {
                word.len() == 4
                    && word.starts_with(prefix)
                    && word[1..].chars().all(|c| c.is_ascii_digit())
            })
            .map(str::to_owned)
            .collect()
    }

    /// One question whose options are `options`, a JSON list, below the
    /// text `text`.
    fn question(text: &str, options: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"asqr": 2, "questions": [
            {"id": "q", "text": text, "kind": "single", "options": options}]})
    }

    /// Presses `code` once and returns the screen drawn after it.
    fn press(harness: &mut Harness, app: &mut App, inbox: &mut Inbox, code: KeyCode) -> String {
        harness
            .run_started(app, inbox, vec![key(code)])
            .expect("runs");
        harness.screen()
    }

    #[test]
    fn down_reaches_the_tail_of_the_last_option_taller_than_the_area() {
        let options = serde_json::json!([{"id": "a", "label": words('w', 300)}]);
        let (mut harness, mut app, mut inbox) = small_with(&question("Which one?", options));

        let mut screen = String::new();
        for _ in 0..60 {
            screen = press(&mut harness, &mut app, &mut inbox, KeyCode::Char('j'));
        }

        assert!(
            shown_words(&screen, 'w').contains("w300"),
            "the label's last word shows:\n{screen}"
        );
        assert!(!screen.contains("↓ more"), "{screen}");
    }

    /// Keys still reach the question while the terminal is too small to
    /// draw it, so what the last drawn frame measured goes with it.
    #[test]
    fn a_frame_too_small_drops_the_view_of_a_tall_option() {
        let options = serde_json::json!([
            {"id": "a", "label": words('w', 300)},
            {"id": "b", "label": "Other"}]);
        let (mut harness, mut app, mut inbox) = small_with(&question("Which one?", options));
        press(&mut harness, &mut app, &mut inbox, KeyCode::Char('j'));
        assert_eq!(app.active().expect("waiting").row(), 0, "j scrolled");

        harness.terminal = Terminal::new(TestBackend::new(40, 10)).expect("test terminal");
        press(&mut harness, &mut app, &mut inbox, KeyCode::Char('j'));

        assert_eq!(app.active().expect("waiting").row(), 1);
    }

    #[test]
    fn down_shows_every_line_of_a_tall_option_before_the_next_one() {
        let options = serde_json::json!([
            {"id": "a", "label": words('w', 300)},
            {"id": "b", "label": "Other"}]);
        let (mut harness, mut app, mut inbox) = small_with(&question("Which one?", options));
        harness
            .run_started(&mut app, &mut inbox, vec![])
            .expect("runs");

        let mut seen = shown_words(&harness.screen(), 'w');
        for _ in 0..60 {
            let screen = press(&mut harness, &mut app, &mut inbox, KeyCode::Char('j'));
            if app.active().expect("waiting").row() == 1 {
                break;
            }
            seen.extend(shown_words(&screen, 'w'));
        }

        let missing: Vec<String> = shown_words(&words('w', 300), 'w')
            .difference(&seen)
            .cloned()
            .collect();
        assert_eq!(missing, Vec::<String>::new(), "words never shown whole");
    }

    #[test]
    fn up_onto_a_tall_option_shows_its_end_and_goes_back_through_it() {
        let options = serde_json::json!([
            {"id": "a", "label": words('w', 300)},
            {"id": "b", "label": "Other"}]);
        let (mut harness, mut app, mut inbox) = small_with(&question("Which one?", options));
        // Down to the second option and no further: below it, `j` would
        // scroll on to the notes line, and the next `k` would scroll back.
        for _ in 0..60 {
            press(&mut harness, &mut app, &mut inbox, KeyCode::Char('j'));
            if app.active().expect("waiting").row() == 1 {
                break;
            }
        }
        assert_eq!(app.active().expect("waiting").row(), 1);

        let screen = press(&mut harness, &mut app, &mut inbox, KeyCode::Char('k'));
        assert_eq!(app.active().expect("waiting").row(), 0);
        assert!(
            shown_words(&screen, 'w').contains("w300"),
            "moving up shows the option's end:\n{screen}"
        );

        let mut screen = screen;
        for _ in 0..60 {
            screen = press(&mut harness, &mut app, &mut inbox, KeyCode::Char('k'));
        }
        assert!(
            shown_words(&screen, 'w').contains("w001"),
            "up goes back to the option's start:\n{screen}"
        );
    }

    #[test]
    fn up_on_the_first_option_reaches_the_start_of_a_long_question_text() {
        let options = serde_json::json!([{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]);
        let (mut harness, mut app, mut inbox) = small_with(&question(&words('t', 200), options));

        let mut screen = String::new();
        for _ in 0..40 {
            screen = press(&mut harness, &mut app, &mut inbox, KeyCode::Up);
        }

        assert!(
            shown_words(&screen, 't').contains("t001"),
            "the text's first word shows:\n{screen}"
        );
        assert_eq!(app.active().expect("waiting").row(), 0);
    }

    #[test]
    fn down_scrolls_back_through_the_question_text_before_moving_on() {
        let options = serde_json::json!([{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]);
        let (mut harness, mut app, mut inbox) = small_with(&question(&words('t', 200), options));
        for _ in 0..40 {
            press(&mut harness, &mut app, &mut inbox, KeyCode::Char('k'));
        }

        press(&mut harness, &mut app, &mut inbox, KeyCode::Char('j'));

        assert_eq!(
            app.active().expect("waiting").row(),
            0,
            "the first `j` scrolls back instead of moving on"
        );
    }

    #[test]
    fn up_in_the_answer_of_a_text_question_reaches_the_start_of_its_text() {
        let session = serde_json::json!({"asqr": 2, "questions": [
            {"id": "q", "text": words('t', 200), "kind": "text"}]});
        let (mut harness, mut app, mut inbox) = small_with(&session);

        let mut screen = String::new();
        for _ in 0..40 {
            screen = press(&mut harness, &mut app, &mut inbox, KeyCode::Up);
        }
        assert!(
            shown_words(&screen, 't').contains("t001"),
            "the text's first word shows:\n{screen}"
        );

        let screen = press(&mut harness, &mut app, &mut inbox, KeyCode::Char('x'));
        assert!(screen.contains("✎ x"), "typing shows the field:\n{screen}");
    }

    #[test]
    fn down_on_the_last_option_reaches_the_end_of_its_note() {
        let options = serde_json::json!([{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]);
        let (mut harness, mut app, mut inbox) = small_with(&question(&words('t', 200), options));
        let ctrl_j = Event::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        let mut typed = vec![key(KeyCode::Char('j')), key(KeyCode::Char('n'))];
        for line in 1..=6 {
            if line > 1 {
                typed.push(ctrl_j.clone());
            }
            typed.extend(format!("n{line:03}").chars().map(|c| key(KeyCode::Char(c))));
        }
        typed.push(key(KeyCode::Esc));
        harness
            .run_started(&mut app, &mut inbox, typed)
            .expect("runs");

        let mut screen = String::new();
        for _ in 0..20 {
            screen = press(&mut harness, &mut app, &mut inbox, KeyCode::Char('j'));
        }

        assert!(
            shown_words(&screen, 'n').contains("n006"),
            "the note's last line shows:\n{screen}"
        );
    }

    #[test]
    fn a_session_arriving_while_running_is_shown_and_announced() {
        let mut harness = Harness::new();
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");

        harness.put("late");
        harness
            .run_started(
                &mut app,
                &mut inbox,
                vec![Event::Resize, Event::InboxChanged],
            )
            .expect("runs");

        assert_eq!(harness.host.alerts, [vec!["late".to_owned()]]);
        assert!(harness.screen().contains("Pick one"));
    }

    #[test]
    fn the_title_counts_the_waiting_sessions() {
        let harness = Harness::new();
        let titled = |sessions: usize| {
            for n in 0..sessions {
                harness.put(&format!("batch-{n}"));
            }
            let (app, _) = start(harness.location.clone()).expect("started");
            terminal_title(&app, None)
        };

        assert_eq!(titled(0), "asqr · 0 waiting");
        assert_eq!(titled(1), "asqr · 1 waiting");
        assert_eq!(titled(3), "asqr · 3 waiting");
    }

    #[test]
    fn the_title_is_sent_when_the_count_changes_and_only_then() {
        let mut harness = Harness::new();
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");

        // `2` picks and moves on to the review, where `enter` submits.
        harness.put("late");
        harness
            .run_started(
                &mut app,
                &mut inbox,
                vec![
                    Event::Resize,
                    Event::InboxChanged,
                    Event::Resize,
                    key(KeyCode::Char('2')),
                    key(KeyCode::Enter),
                    Event::Resize,
                ],
            )
            .expect("runs");

        assert_eq!(
            harness.host.titles,
            ["asqr · 0 waiting", "asqr · 1 waiting", "asqr · 0 waiting"]
        );
    }

    #[test]
    fn the_title_names_a_chosen_queue_and_not_the_default() {
        let app = App::default();

        assert_eq!(terminal_title(&app, None), "asqr · 0 waiting");
        assert_eq!(
            terminal_title(&app, Some("asqr-todos")),
            "asqr · asqr-todos · 0 waiting"
        );
    }

    #[test]
    fn a_long_queue_is_cut_to_40_columns_in_the_title() {
        let name = format!("{}{}", "a".repeat(31), "b".repeat(39));

        assert_eq!(
            terminal_title(&App::default(), Some(&name)),
            format!("asqr · …{} · 0 waiting", "b".repeat(39))
        );
    }

    /// The title goes out inside an OSC sequence, which a BEL or an ESC
    /// in a `--dir` path would end.
    #[test]
    fn control_characters_in_the_queue_show_as_pictures() {
        let app = App::default();

        assert_eq!(
            terminal_title(&app, Some("/tmp/a\u{7}b\u{1b}c\u{85}")),
            "asqr · /tmp/a␇b␛c\u{FFFD} · 0 waiting"
        );
        assert_eq!(
            terminal_title(&app, Some(&"\u{7}".repeat(50))),
            format!("asqr · …{} · 0 waiting", "␇".repeat(39)),
            "each picture takes a column"
        );
    }

    /// Ghostty drops a title of 256 bytes or more. A grapheme of a letter
    /// and 100 combining accents takes one column and 201 bytes.
    #[test]
    fn the_title_stays_under_256_bytes() {
        let accented = format!("e{}", "\u{301}".repeat(100)).repeat(5);
        let long_path = format!("/{}", "ü/".repeat(200));
        let app = App::default();

        for queue in [accented.as_str(), &long_path] {
            let title = terminal_title(&app, Some(queue));
            assert!(title.len() < 256, "{} bytes: {title:?}", title.len());
            assert!(title.starts_with("asqr · …"), "{title:?}");
            assert!(title.ends_with(" · 0 waiting"), "{title:?}");
        }
    }

    #[test]
    fn the_loop_titles_a_chosen_queue() {
        let mut harness = Harness::new();
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view =
            View::for_tests("pty", PaneLayout::Auto, Images::half_blocks()).chosen_queue(true);

        harness.put("late");
        harness
            .run_with_view(
                &mut app,
                &mut inbox,
                &mut view,
                vec![Event::Resize, Event::InboxChanged, Event::Resize],
            )
            .expect("runs");

        assert_eq!(
            harness.host.titles,
            ["asqr · pty · 0 waiting", "asqr · pty · 1 waiting"]
        );
    }

    /// A load that finishes after the draw that asked for it wakes the
    /// loop, which draws the entry then.
    #[test]
    fn a_finished_load_draws_the_entry() {
        let mut harness = Harness::new();
        let session = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Pick one", "kind": "single",
            "context": [{"type": "code", "content": "fn main() {}"}],
            "options": [{"id": "a", "label": "Apples"}]}]}"#;
        fs::write(harness.location.inbox().join("coded.json"), session).expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let (sender, receiver) = mpsc::channel();
        let woken = sender.clone();
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::half_blocks())
            .waiting(std::time::Duration::ZERO)
            .wake_on_load(move || {
                let _ = woken.send(Event::Loaded);
                let _ = woken.send(Event::Quit);
            });
        // A run that is never woken ends here, and fails on its frame
        // instead of hanging.
        let watchdog = sender.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let _ = watchdog.send(Event::Quit);
        });
        drop(sender);

        run(
            &mut harness.terminal,
            &mut app,
            &mut inbox,
            &mut view,
            &receiver,
            &mut harness.host,
            &mut harness.opened,
        )
        .expect("runs");

        assert!(
            harness.screen().contains("1 fn main() {}"),
            "{}",
            harness.screen()
        );
    }

    /// Starts the harness's app on a session whose question shows a code
    /// entry, and runs it over `events` with a view that skips the wait
    /// for an entry while input waits, as outside tests. Says how many
    /// times the draws waited.
    fn waits_over(events: Vec<Event>) -> usize {
        let mut harness = Harness::new();
        let session = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Pick one", "kind": "single",
            "context": [{"type": "code", "content": "fn main() {}"}],
            "options": [{"id": "a", "label": "Apples"}]}]}"#;
        fs::write(harness.location.inbox().join("coded.json"), session).expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::half_blocks())
            .skipping_waits_for_input();
        harness
            .run_with_view(&mut app, &mut inbox, &mut view, events)
            .expect("runs");
        view.waits()
    }

    /// Holding a key over slow entries would wait for each; a draw that
    /// input follows at once shows `loading…` instead.
    #[test]
    fn queued_input_draws_without_waiting() {
        assert_eq!(
            waits_over(vec![Event::Resize, Event::Resize, Event::Quit]),
            0
        );
    }

    /// A finished load wakes the loop through the channel the keys come
    /// in, so a wake-up in front of a key must not hide it.
    #[test]
    fn a_key_behind_a_wake_up_still_skips_the_wait() {
        let x = key(KeyCode::Char('x'));
        assert_eq!(waits_over(vec![Event::Loaded, x.clone(), Event::Quit]), 0);
        assert_eq!(waits_over(vec![Event::InboxChanged, x, Event::Quit]), 0);
    }

    #[test]
    fn a_frame_with_nothing_pending_waits_for_its_entry() {
        assert_eq!(waits_over(vec![]), 1);
    }

    #[test]
    fn only_what_the_person_does_counts_as_input() {
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
        };
        let cases = [
            (key(KeyCode::Char('x')), true),
            (Event::Paste("pasted".to_owned()), true),
            (Event::Resize, true),
            (Event::Quit, true),
            (Event::Key(release), false),
            (Event::InboxChanged, false),
            (Event::FocusGained, false),
            (Event::Loaded, false),
            (Event::InputEnded("the terminal is gone".to_owned()), false),
            (Event::OpenerFailed("cannot open x".to_owned()), false),
        ];
        for (event, input) in cases {
            assert_eq!(event.is_input(), input, "{event:?}");
        }
    }

    // -----------------------------------------------------------------
    // The end of the terminal's input
    // -----------------------------------------------------------------

    #[test]
    fn the_run_ends_with_an_error_when_input_ended() {
        let mut harness = Harness::new();
        harness.put("s");

        let ended = harness.run(vec![Event::InputEnded("the terminal is gone".to_owned())]);

        assert_eq!(
            ended.map_err(|error| format!("{error:#}")),
            Err("reading the terminal's input: the terminal is gone".to_owned())
        );
    }

    /// The events taken ahead of a draw drop wake-ups and a second
    /// keep-alive; the end of the input is neither.
    #[test]
    fn input_ended_behind_wake_ups_still_ends_the_run() {
        let mut harness = Harness::new();
        harness.put("s");

        let ended = harness.run(vec![
            Event::KeepAlive,
            Event::Loaded,
            Event::InputEnded("the terminal is gone".to_owned()),
            Event::KeepAlive,
        ]);

        assert_eq!(
            ended.map_err(|error| format!("{error:#}")),
            Err("reading the terminal's input: the terminal is gone".to_owned())
        );
    }

    // -----------------------------------------------------------------
    // The progress (ADR 73)
    // -----------------------------------------------------------------

    /// Two questions to pick from, the first option of each with `1`.
    const TWO_QUESTIONS: &str = r#"{"asqr": 2, "questions": [
        {"id": "q1", "text": "First?", "kind": "single", "options": [{"id": "a", "label": "A"}]},
        {"id": "q2", "text": "Second?", "kind": "single", "options": [{"id": "a", "label": "A"}]}]}"#;

    fn two_questions() -> Harness {
        let harness = Harness::new();
        fs::write(harness.location.inbox().join("two.json"), TWO_QUESTIONS).expect("written");
        harness
    }

    #[test]
    fn the_progress_is_sent_when_it_changes_and_only_then() {
        let mut harness = two_questions();

        // `1` picks and moves on, the second time to the review, where
        // `enter` submits; the resizes draw without a change.
        harness
            .run(vec![
                Event::Resize,
                key(KeyCode::Char('1')),
                Event::Resize,
                key(KeyCode::Char('1')),
                Event::Resize,
                key(KeyCode::Enter),
                Event::Resize,
            ])
            .expect("runs");

        assert_eq!(harness.host.progress, [Some(0), Some(50), Some(100), None]);
    }

    #[test]
    fn an_empty_queue_sends_no_progress() {
        let mut harness = Harness::new();

        harness.run(vec![Event::Resize]).expect("runs");

        assert_eq!(harness.host.progress, []);
    }

    #[test]
    fn gaining_the_focus_sends_the_progress_again() {
        let mut harness = two_questions();

        harness.run(vec![Event::FocusGained]).expect("runs");

        assert_eq!(harness.host.progress, [Some(0), Some(0)]);
    }

    #[test]
    fn the_progress_goes_when_the_session_leaves() {
        let mut harness = two_questions();
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        harness
            .run_started(&mut app, &mut inbox, vec![])
            .expect("runs");

        fs::remove_file(harness.location.inbox().join("two.json")).expect("removed");
        harness
            .run_started(&mut app, &mut inbox, vec![Event::InboxChanged])
            .expect("runs");

        assert_eq!(harness.host.progress, [Some(0), Some(0), None]);
    }

    #[test]
    fn a_keep_alive_sends_the_progress_again() {
        let mut harness = two_questions();

        harness.run(vec![Event::KeepAlive]).expect("runs");

        assert_eq!(harness.host.progress, [Some(0), Some(0)]);
    }

    #[test]
    fn keep_alives_waiting_together_send_the_progress_once() {
        let mut harness = two_questions();

        harness
            .run(vec![Event::KeepAlive, Event::KeepAlive, Event::KeepAlive])
            .expect("runs");

        assert_eq!(harness.host.progress, [Some(0), Some(0)]);
    }

    #[test]
    fn a_keep_alive_without_a_progress_sends_nothing() {
        let mut harness = Harness::new();

        harness.run(vec![Event::KeepAlive]).expect("runs");

        assert_eq!(harness.host.progress, []);
    }

    #[test]
    fn a_keep_alive_draws_no_frame() {
        let session = SESSION.replace(
            r#"{"type": "image", "path": "/nowhere/cat.png"}"#,
            r#"{"type": "code", "content": "fn main() {}"}"#,
        );

        let log = logged(&session, Images::half_blocks(), vec![Event::KeepAlive]);

        assert_eq!(log, ["frame"]);
    }

    // -----------------------------------------------------------------
    // ctrl-g: the field in the person's editor (ADR 74)
    // -----------------------------------------------------------------

    /// One `text` question, whose field has the keys from the start.
    const TEXT_QUESTION: &str = r#"{"asqr": 2, "title": "Release", "questions": [
        {"id": "q", "text": "What changed?", "kind": "text"}]}"#;

    fn text_question() -> Harness {
        let harness = Harness::new();
        fs::write(harness.location.inbox().join("text.json"), TEXT_QUESTION).expect("written");
        harness
    }

    fn ctrl(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    /// An editor that puts `text` on top of the file, saving the way vim
    /// can: into a new file renamed over the old one, so only the path
    /// reaches what it wrote.
    fn writing(text: &'static str) -> Editor {
        Box::new(move |path: &Path| {
            let file = fs::read_to_string(path)?;
            let saved = path.with_extension("saved");
            fs::write(&saved, format!("{text}\n{file}"))?;
            fs::rename(saved, path)
        })
    }

    /// An editor that fails with an error of `kind`.
    fn failing(kind: io::ErrorKind) -> Editor {
        Box::new(move |_: &Path| Err(io::Error::new(kind, "it failed")))
    }

    /// The status line of `harness` after a run with `editor` on the text
    /// question, and the run's harness.
    fn status_after(editor: Editor) -> (String, Harness) {
        let mut harness = text_question();
        harness.host.editor = Some(editor);
        harness.run(vec![ctrl('g')]).expect("runs");
        let screen = harness.screen();
        let rows: Vec<&str> = screen.lines().collect();
        (rows[rows.len() - 2].trim_end().to_owned(), harness)
    }

    #[test]
    fn ctrl_g_puts_the_text_the_editor_wrote_into_the_field() {
        let mut harness = text_question();
        harness.host.editor = Some(writing("from the editor"));

        // `enter` moves on to the review, where it submits.
        harness
            .run(vec![ctrl('g'), key(KeyCode::Enter), key(KeyCode::Enter)])
            .expect("runs");

        assert_eq!(harness.host.edited.len(), 1);
        assert_eq!(
            harness.result("text").answers[0].custom.as_deref(),
            Some("from the editor")
        );
    }

    #[test]
    fn the_editor_s_file_names_the_question_below_the_cut_line() {
        let seen = Rc::new(RefCell::new(String::new()));
        let mut harness = text_question();
        let copy = Rc::clone(&seen);
        harness.host.editor = Some(Box::new(move |path: &Path| {
            *copy.borrow_mut() = fs::read_to_string(path)?;
            Ok(())
        }));

        harness.run(vec![ctrl('g')]).expect("runs");

        let path = &harness.host.edited[0];
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("md"));
        assert!(!path.exists(), "the file goes once the text is back");
        assert!(
            seen.borrow()
                .contains("# Session: Release (text)\n# Question 1 of 1: q\n"),
            "{}",
            seen.borrow()
        );
    }

    #[test]
    fn an_editor_left_unchanged_leaves_the_field_and_says_so() {
        let (status, _) = status_after(Box::new(|_: &Path| Ok(())));
        assert_eq!(status, "the field is unchanged");
    }

    #[test]
    fn no_editor_is_named_in_the_status_line() {
        let (status, _) = status_after(failing(io::ErrorKind::NotFound));
        assert_eq!(status, "✗ no editor found: set VISUAL or EDITOR");
    }

    #[test]
    fn a_busy_terminal_is_named_in_the_status_line() {
        let (status, _) = status_after(failing(io::ErrorKind::WouldBlock));
        assert_eq!(
            status,
            "✗ the editor did not start: the terminal's input is busy; press ctrl-g again"
        );
    }

    #[test]
    fn an_editor_that_exits_with_an_error_leaves_the_field() {
        let (status, harness) = status_after(failing(io::ErrorKind::Other));
        assert_eq!(
            status,
            "✗ the field is unchanged: the editor exited with an error"
        );
        assert!(!harness.host.edited[0].exists(), "the file goes");
    }

    #[test]
    fn an_editor_that_cannot_run_says_why() {
        let (status, _) = status_after(failing(io::ErrorKind::PermissionDenied));
        assert_eq!(
            status,
            "✗ the field is unchanged: cannot run the editor: it failed"
        );
    }

    #[test]
    fn a_file_the_editor_took_away_leaves_the_field() {
        let (status, _) = status_after(Box::new(|path: &Path| fs::remove_file(path)));
        assert!(
            status.starts_with("✗ the field is unchanged: cannot read the editor's file: "),
            "{status}"
        );
    }

    #[test]
    fn a_file_that_is_no_utf_8_leaves_the_field() {
        let (status, _) = status_after(Box::new(|path: &Path| fs::write(path, [0xff, 0xfe])));
        assert!(
            status.starts_with("✗ the field is unchanged: cannot read the editor's file: "),
            "{status}"
        );
    }

    #[test]
    fn a_file_without_its_cut_line_leaves_the_field_and_is_kept() {
        let (status, harness) =
            status_after(Box::new(|path: &Path| fs::write(path, "all of it\n")));

        let kept = &harness.host.edited[0];
        assert_eq!(
            status,
            format!(
                "! your text is in {}: the >8 line is gone, field unchanged",
                as_shown(kept)
            )
        );
        assert_eq!(fs::read_to_string(kept).expect("kept"), "all of it\n");
        fs::remove_file(kept).expect("cleaned up");
    }

    /// `path` as the status line names it in this process.
    fn as_shown(path: &Path) -> String {
        shown_path(path, std::env::var_os("TMPDIR").as_deref())
    }

    #[test]
    fn a_kept_file_under_tmpdir_is_named_by_the_variable() {
        let dir = std::ffi::OsStr::new("/var/folders/p2/r4y13yq/T/");
        assert_eq!(
            shown_path(
                Path::new("/var/folders/p2/r4y13yq/T/asqr-E0yXnT.md"),
                Some(dir)
            ),
            "$TMPDIR/asqr-E0yXnT.md"
        );
    }

    #[test]
    fn a_kept_file_elsewhere_is_named_whole() {
        let path = Path::new("/tmp/asqr-E0yXnT.md");
        assert_eq!(shown_path(path, None), "/tmp/asqr-E0yXnT.md");
        assert_eq!(
            shown_path(path, Some(std::ffi::OsStr::new(""))),
            "/tmp/asqr-E0yXnT.md",
            "an empty TMPDIR"
        );
        assert_eq!(
            shown_path(path, Some(std::ffi::OsStr::new("/var/tmp"))),
            "/tmp/asqr-E0yXnT.md"
        );
    }

    #[test]
    fn a_session_that_left_while_the_editor_ran_keeps_the_text_in_its_file() {
        let mut harness = text_question();
        let inbox = harness.location.inbox().join("text.json");
        let mut write = writing("written meanwhile");
        harness.host.editor = Some(Box::new(move |path: &Path| {
            fs::remove_file(&inbox)?;
            write(path)
        }));

        harness.run(vec![ctrl('g')]).expect("runs");

        let kept = &harness.host.edited[0];
        let screen = harness.screen();
        assert!(
            screen.contains(&format!(
                "! your text is in {}: \"Release\" left while the editor ran",
                as_shown(kept)
            )),
            "{screen}"
        );
        assert!(
            fs::read_to_string(kept)
                .expect("kept")
                .starts_with("written meanwhile\n"),
        );
        fs::remove_file(kept).expect("cleaned up");
    }

    /// The status line on the screen of `harness`.
    fn status_line(harness: &Harness) -> String {
        let screen = harness.screen();
        let rows: Vec<&str> = screen.lines().collect();
        rows[rows.len() - 2].trim_end().to_owned()
    }

    #[test]
    fn a_withdrawal_while_the_editor_ran_names_the_kept_file() {
        let mut harness = text_question();
        let location = harness.location.clone();
        let mut write = writing("written meanwhile");
        harness.host.editor = Some(Box::new(move |path: &Path| {
            crate::queue::withdraw_session(&location, "text", true).map_err(io::Error::other)?;
            write(path)
        }));

        // The watcher's sync of the withdrawal waits in the channel while
        // the editor runs.
        harness
            .run(vec![ctrl('g'), Event::InboxChanged])
            .expect("runs");

        let kept = &harness.host.edited[0];
        assert_eq!(
            status_line(&harness),
            format!(
                "! your text is in {}: \"Release\" withdrawn by the asker",
                as_shown(kept)
            )
        );
        fs::remove_file(kept).expect("cleaned up");
    }

    #[test]
    fn the_text_of_a_session_that_left_does_not_go_into_its_field() {
        let mut harness = text_question();
        let inbox_file = harness.location.inbox().join("text.json");
        let mut write = writing("written meanwhile");
        harness.host.editor = Some(Box::new(move |path: &Path| {
            fs::remove_file(&inbox_file)?;
            write(path)
        }));
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");

        harness
            .run_started(&mut app, &mut inbox, vec![ctrl('g')])
            .expect("runs");

        let state = app.active().expect("shown until the next sync");
        assert_eq!(state.answers()[0].custom, None);
        fs::remove_file(&harness.host.edited[0]).expect("cleaned up");
    }

    /// A session file away from the inbox while the editor ran and back
    /// before the next sync is the same session again: a later withdrawal
    /// of it says nothing of the file kept back then.
    #[test]
    fn a_session_back_in_the_inbox_drops_the_kept_file_from_its_notice() {
        let mut harness = text_question();
        let inbox_file = harness.location.inbox().join("text.json");
        let aside = harness.location.dir().join("aside.json");
        let (from, to) = (inbox_file.clone(), aside.clone());
        let mut write = writing("written meanwhile");
        harness.host.editor = Some(Box::new(move |path: &Path| {
            fs::rename(&from, &to)?;
            write(path)
        }));
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        harness
            .run_started(&mut app, &mut inbox, vec![ctrl('g')])
            .expect("runs");

        fs::rename(&aside, &inbox_file).expect("moved back");
        harness
            .run_started(&mut app, &mut inbox, vec![Event::InboxChanged])
            .expect("runs");
        crate::queue::withdraw_session(&harness.location, "text", false).expect("withdrawn");
        harness
            .run_started(&mut app, &mut inbox, vec![Event::InboxChanged])
            .expect("runs");

        assert_eq!(
            status_line(&harness),
            "↩ withdrawn \"Release\" by the asker"
        );
        fs::remove_file(&harness.host.edited[0]).expect("cleaned up");
    }

    #[test]
    fn the_screen_is_drawn_whole_after_the_editor_without_asking_for_the_cursor() {
        let harness = text_question();
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::half_blocks());
        let log = Log::default();
        let mut backend = Logging::new(&log);
        backend.refuse_cursor_query = true;
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let (sender, receiver) = mpsc::channel();
        sender.send(ctrl('g')).expect("sent");
        drop(sender);

        run(
            &mut terminal,
            &mut app,
            &mut inbox,
            &mut view,
            &receiver,
            &mut Recorder::default(),
            &mut OpenedFiles::under(harness.scratch.path().join("opened")),
        )
        .expect("runs without asking for the cursor");

        let steps = log.borrow();
        let kinds: Vec<&str> = steps
            .iter()
            .map(|step| match step {
                Step::Clear => "clear",
                Step::Frame(_) => "frame",
                Step::Deleted(_) => "delete",
            })
            .collect();
        assert_eq!(kinds, ["frame", "clear", "frame"]);
    }

    #[test]
    fn the_title_and_the_progress_go_out_again_after_the_editor() {
        let mut harness = text_question();

        harness.run(vec![ctrl('g')]).expect("runs");

        assert_eq!(
            harness.host.titles,
            ["asqr · 1 waiting", "asqr · 1 waiting"]
        );
        assert_eq!(harness.host.progress, [Some(0), Some(0)]);
    }

    #[test]
    fn images_are_sent_again_after_the_editor() {
        let mut watched = Watched::new();
        watched.put("a", PICTURE, [255, 0, 0]);
        let path = watched.harness.location.inbox().join("a.json");
        let session = fs::read_to_string(&path).expect("written");
        fs::write(
            &path,
            session.replacen(
                r#""kind": "single","#,
                r#""kind": "single", "custom": true,"#,
                1,
            ),
        )
        .expect("written");
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::kitty());

        // `jj` reaches the own answer, whose field `ctrl-g` opens.
        watched.run(
            &mut app,
            &mut inbox,
            &mut view,
            vec![key(KeyCode::Char('j')), key(KeyCode::Char('j')), ctrl('g')],
        );

        let steps = watched.steps();
        let shown: Vec<&BTreeSet<u32>> = steps
            .iter()
            .filter_map(|step| match step {
                Step::Frame(shown) => Some(shown),
                _ => None,
            })
            .collect();
        let first = shown.first().expect("frames");
        let last = shown.last().expect("frames");
        assert_eq!((first.len(), last.len()), (1, 1), "{steps:?}");
        assert!(
            first.is_disjoint(last),
            "sent again under a new id: {steps:?}"
        );
        assert!(
            steps.contains(&Step::Deleted(first.iter().copied().collect())),
            "the old one is deleted: {steps:?}"
        );
    }

    #[test]
    fn a_change_without_new_sessions_is_not_announced() {
        let mut harness = Harness::new();
        harness.put("batch-01");

        harness.run(vec![Event::InboxChanged]).expect("runs");

        assert!(harness.host.alerts.is_empty());
    }

    #[test]
    fn answering_writes_the_result() {
        let mut harness = Harness::new();
        harness.put("batch-01");

        harness
            .run(vec![key(KeyCode::Char('2')), key(KeyCode::Enter)])
            .expect("runs");

        let result = harness.result("batch-01");
        assert_eq!(result.status, Status::Submitted);
        assert_eq!(result.answers[0].selected, ["b"]);
        // The confirmation stays in the status line, above the key bar,
        // over the empty queue.
        let screen = harness.screen();
        let rows: Vec<&str> = screen.lines().collect();
        assert!(screen.contains("Nothing to answer"), "{screen}");
        assert!(
            rows[rows.len() - 2].starts_with("✔ sent \"batch-01\" · 1 answered, 0 skipped"),
            "{screen}"
        );
    }

    #[test]
    fn only_key_presses_count() {
        let mut harness = Harness::new();
        harness.put("batch-01");
        let mut release = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        let mut repeat = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        repeat.kind = KeyEventKind::Repeat;
        repeat.state = KeyEventState::NONE;

        harness
            .run(vec![Event::Key(release), Event::Key(repeat)])
            .expect("runs");

        assert!(
            fs::read_dir(harness.location.drafts())
                .expect("readable")
                .next()
                .is_none(),
            "a release picks nothing"
        );
    }

    #[test]
    fn a_pasted_line_break_in_the_session_note_submits_nothing() {
        let mut harness = Harness::new();
        harness.put("batch-01");

        // `2` picks and moves on to the review, which opens on Submit; the
        // session note is the row below.
        harness
            .run(vec![
                key(KeyCode::Char('2')),
                key(KeyCode::Down),
                Event::Paste("a\nb".to_owned()),
            ])
            .expect("runs");

        assert!(
            !harness.location.outbox().join("batch-01.json").exists(),
            "no result was written"
        );
        let draft = crate::queue::load_draft(&harness.location, "batch-01")
            .expect("readable")
            .expect("a draft");
        assert_eq!(draft.note.as_deref(), Some("a\nb"));
    }

    #[test]
    fn a_field_entered_from_above_shows_its_cursor_on_its_first_row_at_once() {
        let harness = &mut Harness::new();
        let session = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Name it", "kind": "single",
            "custom": true, "options": [{"id": "a", "label": "Apples"}]}]}"#;
        fs::write(harness.location.inbox().join("own.json"), session).expect("written");
        // Thirty words of four columns wrap onto three rows after the head
        // `❯ ( ) ✎ Own answer: `, the first ending with the blank after
        // `w14`.
        let words: String = (1..=30).map(|n| format!("w{n:02} ")).collect();
        let mut events = vec![key(KeyCode::Char('j'))];
        events.extend(words.trim_end().chars().map(|c| key(KeyCode::Char(c))));
        events.extend([key(KeyCode::Up), key(KeyCode::Up), key(KeyCode::Up)]);
        events.push(key(KeyCode::Down));

        harness.run(events).expect("runs");

        let buffer = harness.terminal.backend().buffer();
        let cursor: Vec<(u16, u16)> = (4..buffer.area.height - 2)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].modifier.contains(Modifier::REVERSED))
            .collect();
        let screen = harness.screen();
        let first = screen
            .lines()
            .position(|row| row.contains("w01"))
            .expect("the answer shows");
        let first = u16::try_from(first).expect("a row");
        assert_eq!(cursor, [(76, first)], "{screen}");
        assert_eq!(buffer[(75, first)].symbol(), "4", "{screen}");
    }

    #[test]
    fn o_asks_the_host_to_open_the_image() {
        let mut harness = Harness::new();
        let image = harness.scratch_file("cat.png", b"not decoded here");
        harness.put_showing(
            "batch-01",
            &format!(r#"{{"type": "image", "path": "{image}"}}"#),
        );

        harness.run(vec![key(KeyCode::Char('o'))]).expect("runs");

        assert_eq!(harness.host.opened, [image]);
    }

    #[test]
    fn o_writes_inline_code_to_a_file_and_opens_it() {
        let mut harness = Harness::new();
        harness.put_showing(
            "coded",
            r#"{"type": "code", "content": "fn main() {}\n", "language": "rust"}"#,
        );

        harness.run(vec![key(KeyCode::Char('o'))]).expect("runs");

        let [opened] = &harness.host.opened[..] else {
            panic!("one file opened: {:?}", harness.host.opened);
        };
        assert!(opened.ends_with("/code.rs"), "{opened}");
        assert_eq!(harness.host.contents, [b"fn main() {}\n".to_vec()]);
        assert!(Path::new(opened).exists(), "the file stays after the run");
    }

    #[test]
    fn a_written_file_that_cannot_be_opened_is_named() {
        let mut harness = Harness::new();
        harness.host.viewer_missing = true;
        harness.put_showing("coded", r#"{"type": "code", "content": "x"}"#);

        harness.run(vec![key(KeyCode::Char('o'))]).expect("runs");

        assert_eq!(status_line(&harness), "✗ cannot open code.txt: no viewer");
    }

    #[test]
    fn o_on_an_image_at_a_relative_path_opens_nothing_and_says_why() {
        let mut harness = Harness::new();
        harness.put_showing("pictured", r#"{"type": "image", "path": "pics/cat.png"}"#);

        harness.run(vec![key(KeyCode::Char('o'))]).expect("runs");

        assert_eq!(harness.host.opened, Vec::<String>::new());
        assert_eq!(
            status_line(&harness),
            "✗ cannot open pics/cat.png: relative path"
        );
    }

    #[test]
    fn tab_focuses_the_pane_the_last_draw_showed() {
        let mut harness = Harness::new();
        harness.put("batch-01");

        // The focused pane takes the `2`; back on the question, enter
        // picks the first option.
        harness
            .run(vec![
                key(KeyCode::Tab),
                key(KeyCode::Char('2')),
                key(KeyCode::Esc),
                key(KeyCode::Enter),
                key(KeyCode::Enter),
            ])
            .expect("runs");

        assert_eq!(harness.result("batch-01").answers[0].selected, ["a"]);
    }

    #[test]
    fn a_frame_too_small_hands_no_pane_to_the_next_session() {
        let mut harness = Harness::new();
        harness.put("a-with-context");
        let without_context = SESSION.replace(
            r#""context": [{"type": "image", "path": "/nowhere/cat.png"}],"#,
            "",
        );
        fs::write(
            harness.location.inbox().join("b-without-context.json"),
            without_context,
        )
        .expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::half_blocks());
        harness
            .run_with_view(&mut app, &mut inbox, &mut view, vec![])
            .expect("runs");

        // The session with the pane goes while the screen is too small.
        // `tab` then finds no pane in the one that takes its place, and
        // `2` picks there instead of going to a pane.
        harness.terminal.backend_mut().resize(40, 10);
        fs::remove_file(harness.location.inbox().join("a-with-context.json")).expect("removed");
        harness
            .run_with_view(
                &mut app,
                &mut inbox,
                &mut view,
                vec![
                    Event::InboxChanged,
                    key(KeyCode::Tab),
                    key(KeyCode::Char('2')),
                ],
            )
            .expect("runs");
        harness.terminal.backend_mut().resize(80, 24);
        harness
            .run_with_view(&mut app, &mut inbox, &mut view, vec![key(KeyCode::Enter)])
            .expect("runs");

        assert_eq!(
            harness.result("b-without-context").answers[0].selected,
            ["b"]
        );
    }

    /// Under the message of a terminal too small for asqr, the dialog of
    /// an invalid session is not on the screen: `ctrl-c` quits and leaves
    /// it for the next start's frame. Once a frame shows it, a key closes
    /// it.
    #[test]
    fn a_key_under_the_too_small_message_leaves_the_dialog() {
        let mut harness = Harness::new();
        fs::write(harness.location.inbox().join("garbage.json"), "not json").expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        assert!(app.invalid_shown().is_some(), "the scan found it");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::half_blocks());
        let ctrl_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

        harness.terminal.backend_mut().resize(40, 10);
        harness
            .run_with_view(&mut app, &mut inbox, &mut view, vec![ctrl_c])
            .expect("runs");
        assert!(
            app.invalid_shown().is_some(),
            "ctrl-c closed a dialog no frame showed"
        );

        harness.terminal.backend_mut().resize(80, 24);
        harness
            .run_with_view(
                &mut app,
                &mut inbox,
                &mut view,
                vec![key(KeyCode::Char('x'))],
            )
            .expect("runs");
        assert_eq!(app.invalid_shown(), None, "the frame showed it");
    }

    /// `SESSION` with an intro of thirty bullet lines.
    fn with_long_intro() -> String {
        let intro: Vec<String> = (1..=30).map(|n| format!("- line {n}")).collect();
        SESSION.replacen(
            r#"{"asqr": 2,"#,
            &format!(r#"{{"asqr": 2, "intro": {:?},"#, intro.join("\n")),
            1,
        )
    }

    #[test]
    fn a_frame_too_small_keeps_how_far_the_intro_scrolls() {
        let harness = &mut Harness::new();
        fs::write(
            harness.location.inbox().join("long.json"),
            with_long_intro(),
        )
        .expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::half_blocks());
        harness
            .run_with_view(
                &mut app,
                &mut inbox,
                &mut view,
                vec![key(KeyCode::Char('i')), key(KeyCode::Char('G'))],
            )
            .expect("runs");

        // The too-small frame shows no intro, so `k` still scrolls within
        // what the last full frame showed: one line up from the end.
        harness.terminal.backend_mut().resize(40, 10);
        harness
            .run_with_view(
                &mut app,
                &mut inbox,
                &mut view,
                vec![key(KeyCode::Char('k'))],
            )
            .expect("runs");
        harness.terminal.backend_mut().resize(80, 24);
        harness
            .run_with_view(&mut app, &mut inbox, &mut view, vec![])
            .expect("runs");

        let screen = harness.screen();
        assert!(screen.contains("9–29 of 30"), "{screen}");
    }

    #[test]
    fn the_intro_scrolls_as_far_as_the_last_draw_showed() {
        let harness = &mut Harness::new();
        fs::write(
            harness.location.inbox().join("long.json"),
            with_long_intro(),
        )
        .expect("written");

        harness
            .run(vec![key(KeyCode::Char('i')), key(KeyCode::Char('G'))])
            .expect("runs");

        // 24 rows leave 23 above the key bar and 21 inside the frame.
        let screen = harness.screen();
        assert!(screen.contains("• line 30"), "{screen}");
        assert!(screen.contains("10–30 of 30"), "{screen}");
    }

    #[test]
    fn an_image_that_cannot_be_opened_is_a_notice() {
        let mut harness = Harness::new();
        harness.host.viewer_missing = true;
        let image = harness.scratch_file("cat.png", b"not decoded here");
        harness.put_showing(
            "batch-01",
            &format!(r#"{{"type": "image", "path": "{image}"}}"#),
        );
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");

        harness
            .run_started(&mut app, &mut inbox, vec![key(KeyCode::Char('o'))])
            .expect("runs");

        // The scratch directory's path is longer than the status line.
        assert_eq!(
            app.current_notice().map(|notice| notice.text.as_str()),
            Some(format!("cannot open {image}: no viewer").as_str())
        );
        assert_eq!(
            app.current_notice().map(|notice| notice.tone),
            Some(Tone::Error)
        );
    }

    #[test]
    fn an_opener_that_failed_is_a_notice() {
        let mut harness = Harness::new();
        harness.put("batch-01");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let failed = "cannot open /pictures/cat.png: the opener exited with 1";

        harness
            .run_started(
                &mut app,
                &mut inbox,
                vec![Event::OpenerFailed(failed.to_owned())],
            )
            .expect("runs");

        assert_eq!(status_line(&harness), format!("✗ {failed}"));
        assert_eq!(
            app.current_notice().map(|notice| notice.tone),
            Some(Tone::Error)
        );
    }

    /// Whether any cell of the screen is drawn in a colour of `hue`, as
    /// the half blocks of an image are. Half blocks blend the image's edge
    /// with the background, so a plain red image shows in shades of red.
    fn shows_hue(terminal: &Terminal<TestBackend>, hue: fn(u8, u8, u8) -> bool) -> bool {
        let buffer = terminal.backend().buffer();
        buffer.content.iter().any(|cell| {
            [cell.fg, cell.bg]
                .into_iter()
                .any(|colour| matches!(colour, Color::Rgb(r, g, b) if hue(r, g, b)))
        })
    }

    /// Half blocks are drawn into the cells with every frame, and no
    /// transfer of theirs can be lost, so regaining the focus reads
    /// nothing again.
    #[test]
    fn gaining_focus_keeps_half_block_images() {
        let red: fn(u8, u8, u8) -> bool = |r, g, b| r > 0 && g == 0 && b == 0;
        let blue: fn(u8, u8, u8) -> bool = |r, g, b| r == 0 && g == 0 && b > 0;
        let mut harness = Harness::new();
        let picture = harness.location.dir().join("picture.png");
        let paint = |rgb: [u8; 3]| {
            image::RgbImage::from_pixel(8, 8, image::Rgb(rgb))
                .save(&picture)
                .expect("image written");
        };
        paint([255, 0, 0]);
        let session = SESSION.replace("/nowhere/cat.png", picture.to_str().expect("utf-8 path"));
        fs::write(harness.location.inbox().join("pictured.json"), session).expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::half_blocks());

        harness
            .run_with_view(&mut app, &mut inbox, &mut view, vec![])
            .expect("runs");
        assert!(shows_hue(&harness.terminal, red), "{}", harness.screen());

        // A changed file would show a new read: the image as loaded stays
        // in use.
        paint([0, 0, 255]);
        harness
            .run_with_view(
                &mut app,
                &mut inbox,
                &mut view,
                vec![Event::Resize, Event::FocusGained],
            )
            .expect("runs");
        assert!(shows_hue(&harness.terminal, red), "{}", harness.screen());
        assert!(!shows_hue(&harness.terminal, blue), "{}", harness.screen());
    }

    /// Only tmux with `allow-passthrough on` drops a Kitty transfer, while
    /// asqr's pane is hidden. There the images are read and sent again
    /// under new ids when asqr regains the focus; elsewhere the terminal
    /// still has them, and they stay as they are (ADR 24).
    #[test]
    fn gaining_focus_sends_kitty_images_again_only_where_tmux_drops_them() {
        for (passthrough, again) in [
            (Passthrough::WhileVisible, true),
            (Passthrough::None, false),
            (Passthrough::Always, false),
        ] {
            let mut watched = Watched::new();
            watched.put("a", PICTURE, [255, 0, 0]);
            let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
            let mut view =
                View::for_tests("test", PaneLayout::Auto, Images::kitty_through(passthrough));

            watched.run(&mut app, &mut inbox, &mut view, vec![Event::FocusGained]);

            let shown: Vec<BTreeSet<u32>> = watched
                .steps()
                .into_iter()
                .filter_map(|step| match step {
                    Step::Frame(shown) => Some(shown),
                    _ => None,
                })
                .collect();
            let expected = if again { [1, 2] } else { [1, 1] };
            assert_eq!(
                shown,
                expected.map(|id| BTreeSet::from([id])),
                "{passthrough:?}"
            );
            let deleted: Vec<Step> = watched
                .steps()
                .into_iter()
                .filter(|step| matches!(step, Step::Deleted(_)))
                .collect();
            let expected = if again {
                vec![Step::Deleted(vec![1])]
            } else {
                Vec::new()
            };
            assert_eq!(deleted, expected, "{passthrough:?}");
        }
    }

    #[test]
    fn an_inbox_that_cannot_be_read_on_start_is_an_error() {
        let harness = Harness::new();
        fs::remove_dir(harness.location.inbox()).expect("removed");
        fs::write(harness.location.inbox(), "in the way").expect("written");

        let error = start(harness.location.clone()).expect_err("fails");

        assert!(
            format!("{error:#}").contains("reading the inbox"),
            "{error:#}"
        );
    }

    #[test]
    fn an_inbox_that_becomes_unreadable_is_a_notice() {
        let mut harness = Harness::new();
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");

        fs::remove_dir(harness.location.inbox()).expect("removed");
        fs::write(harness.location.inbox(), "in the way").expect("written");
        harness
            .run_started(&mut app, &mut inbox, vec![Event::InboxChanged])
            .expect("keeps running");

        assert!(
            harness.screen().contains("cannot read the inbox"),
            "{}",
            harness.screen()
        );
        assert_eq!(
            app.current_notice().map(|notice| notice.tone),
            Some(Tone::Error)
        );
    }

    // -----------------------------------------------------------------
    // Clearing the screen after an overlay over an image
    // -----------------------------------------------------------------

    /// What a run did to the terminal, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Step {
        Clear,
        /// A frame flushed, with the Kitty ids of the images it shows.
        Frame(BTreeSet<u32>),
        /// The host deleted these Kitty images.
        Deleted(Vec<u32>),
    }

    type Log = Rc<RefCell<Vec<Step>>>;

    /// A test backend that logs each clear of the screen and each frame
    /// flushed into a log it may share with a [`Recorder`], so a test sees
    /// which frame a clear or a delete came after.
    struct Logging {
        backend: TestBackend,
        log: Log,
        /// Flushing a frame fails, as writing to a terminal that went
        /// away does, when set.
        refuse_flush: bool,
        /// Asking for the cursor's position fails, as it does in a
        /// terminal that never answers the query, when set.
        refuse_cursor_query: bool,
    }

    impl Logging {
        fn new(log: &Log) -> Self {
            Logging {
                backend: TestBackend::new(80, 24),
                log: Rc::clone(log),
                refuse_flush: false,
                refuse_cursor_query: false,
            }
        }
    }

    /// `result` of the test backend, which never fails, as the I/O result
    /// the logging backend gives where a real terminal could fail.
    fn infallible<T>(result: Result<T, std::convert::Infallible>) -> io::Result<T> {
        result.map_err(|never| match never {})
    }

    impl Backend for Logging {
        type Error = io::Error;

        fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
        {
            infallible(self.backend.draw(content))
        }

        fn hide_cursor(&mut self) -> io::Result<()> {
            infallible(self.backend.hide_cursor())
        }

        fn show_cursor(&mut self) -> io::Result<()> {
            infallible(self.backend.show_cursor())
        }

        fn get_cursor_position(&mut self) -> io::Result<ratatui::layout::Position> {
            if self.refuse_cursor_query {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the cursor position could not be read",
                ));
            }
            infallible(self.backend.get_cursor_position())
        }

        fn set_cursor_position<P: Into<ratatui::layout::Position>>(
            &mut self,
            position: P,
        ) -> io::Result<()> {
            infallible(self.backend.set_cursor_position(position))
        }

        fn clear(&mut self) -> io::Result<()> {
            self.log.borrow_mut().push(Step::Clear);
            infallible(self.backend.clear())
        }

        fn clear_region(&mut self, clear_type: ratatui::backend::ClearType) -> io::Result<()> {
            self.log.borrow_mut().push(Step::Clear);
            infallible(self.backend.clear_region(clear_type))
        }

        fn size(&self) -> io::Result<ratatui::layout::Size> {
            infallible(self.backend.size())
        }

        fn window_size(&mut self) -> io::Result<ratatui::backend::WindowSize> {
            infallible(self.backend.window_size())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.refuse_flush {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "terminal gone"));
            }
            let shown = kitty_ids_on_screen(self.backend.buffer());
            self.log.borrow_mut().push(Step::Frame(shown));
            infallible(self.backend.flush())
        }
    }

    /// The clears and frames of a run over `events` with `session` waiting,
    /// its images made by `images`.
    fn logged(session: &str, images: Images, events: Vec<Event>) -> Vec<&'static str> {
        logged_in(session, images, events, false).expect("runs")
    }

    /// [`logged`] in a terminal that answers no query for the cursor's
    /// position where `refuse_cursor_query` is set, failing as the run
    /// fails.
    fn logged_in(
        session: &str,
        images: Images,
        events: Vec<Event>,
        refuse_cursor_query: bool,
    ) -> anyhow::Result<Vec<&'static str>> {
        let harness = Harness::new();
        fs::write(harness.location.inbox().join("s.json"), session).expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, images);
        let log = Log::default();
        let mut backend = Logging::new(&log);
        backend.refuse_cursor_query = refuse_cursor_query;
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let (sender, receiver) = mpsc::channel();
        for event in events {
            sender.send(event).expect("sent");
        }
        drop(sender);
        run(
            &mut terminal,
            &mut app,
            &mut inbox,
            &mut view,
            &receiver,
            &mut Recorder::default(),
            &mut OpenedFiles::under(harness.scratch.path().join("opened")),
        )?;
        let steps = log.borrow();
        Ok(steps
            .iter()
            .map(|step| match step {
                Step::Clear => "clear",
                Step::Frame(_) => "frame",
                Step::Deleted(_) => "delete",
            })
            .collect())
    }

    #[test]
    fn closing_an_overlay_over_an_image_clears_the_screen_before_the_next_frame() {
        let session = SESSION.replacen(r#"{"asqr": 2,"#, r#"{"asqr": 2, "intro": "Fruit","#, 1);
        let overlays = [
            ("help", key(KeyCode::Char('?')), key(KeyCode::Char('x'))),
            ("intro", key(KeyCode::Char('i')), key(KeyCode::Esc)),
            ("session list", key(KeyCode::Char('L')), key(KeyCode::Esc)),
        ];

        for protocol in [Protocol::Sixel, Protocol::Iterm2] {
            for (overlay, open, close) in overlays.clone() {
                let images = Images::new(Graphics {
                    protocol,
                    ..Graphics::HALF_BLOCKS
                });
                let log = logged(&session, images, vec![open, close, Event::Resize]);

                assert_eq!(
                    log,
                    ["frame", "frame", "clear", "frame", "frame"],
                    "{protocol:?}, {overlay}: the first frame without it is drawn afresh, \
                     and only that one"
                );
            }
        }
    }

    /// The clears and frames of opening and closing the help over an image
    /// under `protocol` in a terminal that does not answer the query for
    /// the cursor's position, as crossterm fails it after 2 seconds, or
    /// the error that ended the run.
    fn overlay_closed_without_cursor_query(
        protocol: Protocol,
    ) -> Result<Vec<&'static str>, String> {
        let images = Images::new(Graphics {
            protocol,
            ..Graphics::HALF_BLOCKS
        });
        logged_in(
            SESSION,
            images,
            vec![key(KeyCode::Char('?')), key(KeyCode::Char('x'))],
            true,
        )
        .map_err(|error| format!("{error:#}"))
    }

    #[test]
    fn closing_an_overlay_over_a_sixel_image_does_not_ask_for_the_cursor() {
        assert_eq!(
            overlay_closed_without_cursor_query(Protocol::Sixel),
            Ok(vec!["frame", "frame", "clear", "frame"])
        );
    }

    #[test]
    fn closing_an_overlay_over_an_iterm2_image_does_not_ask_for_the_cursor() {
        assert_eq!(
            overlay_closed_without_cursor_query(Protocol::Iterm2),
            Ok(vec!["frame", "frame", "clear", "frame"])
        );
    }

    /// Half blocks and the Kitty protocol leave no cell to ratatui that it
    /// would skip, so the frame after the overlay writes the cells it
    /// covered again.
    #[test]
    fn closing_an_overlay_over_a_half_block_or_kitty_image_keeps_the_screen() {
        for images in [Images::half_blocks(), Images::kitty()] {
            let protocol = images.graphics().protocol;

            let log = logged(
                SESSION,
                images,
                vec![key(KeyCode::Char('?')), key(KeyCode::Char('x'))],
            );

            assert_eq!(log, ["frame", "frame", "frame"], "{protocol:?}");
        }
    }

    #[test]
    fn closing_an_overlay_without_an_image_on_screen_keeps_the_screen() {
        let session = SESSION.replace(
            r#"{"type": "image", "path": "/nowhere/cat.png"}"#,
            r#"{"type": "code", "content": "fn main() {}"}"#,
        );
        let sixel = Images::new(Graphics {
            protocol: Protocol::Sixel,
            ..Graphics::HALF_BLOCKS
        });

        let log = logged(
            &session,
            sixel,
            vec![key(KeyCode::Char('?')), key(KeyCode::Char('x'))],
        );

        assert_eq!(log, ["frame", "frame", "frame"]);
    }

    // -----------------------------------------------------------------
    // Deleting Kitty images
    // -----------------------------------------------------------------

    /// A queue whose run is logged, frames and deletes in one log, on a
    /// terminal that speaks the Kitty protocol.
    struct Watched {
        harness: Harness,
        terminal: Terminal<Logging>,
        log: Log,
    }

    impl Watched {
        fn new() -> Self {
            let log = Log::default();
            let mut harness = Harness::new();
            harness.host.log = Rc::clone(&log);
            Watched {
                harness,
                terminal: Terminal::new(Logging::new(&log)).expect("test terminal"),
                log,
            }
        }

        /// Writes a session `id` whose question shows `context`, in which
        /// `{picture}` stands for a picture of one `colour` written into
        /// the queue's directory.
        fn put(&self, id: &str, context: &str, colour: [u8; 3]) {
            let name = format!("{id}-{}-{}-{}.png", colour[0], colour[1], colour[2]);
            let picture = self.harness.location.dir().join(name);
            image::RgbImage::from_pixel(8, 8, image::Rgb(colour))
                .save(&picture)
                .expect("image written");
            let context = context.replace("{picture}", picture.to_str().expect("utf-8 path"));
            let session =
                SESSION.replace(r#"{"type": "image", "path": "/nowhere/cat.png"}"#, &context);
            fs::write(
                self.harness.location.inbox().join(format!("{id}.json")),
                session,
            )
            .expect("written");
        }

        fn remove(&self, id: &str) {
            fs::remove_file(self.harness.location.inbox().join(format!("{id}.json")))
                .expect("removed");
        }

        fn run(&mut self, app: &mut App, inbox: &mut Inbox, view: &mut View, events: Vec<Event>) {
            self.try_run(app, inbox, view, events).expect("runs");
        }

        fn try_run(
            &mut self,
            app: &mut App,
            inbox: &mut Inbox,
            view: &mut View,
            events: Vec<Event>,
        ) -> anyhow::Result<()> {
            let (sender, receiver) = mpsc::channel();
            for event in events {
                sender.send(event).expect("sent");
            }
            drop(sender);
            run(
                &mut self.terminal,
                app,
                inbox,
                view,
                &receiver,
                &mut self.harness.host,
                &mut self.harness.opened,
            )
        }

        fn steps(&self) -> Vec<Step> {
            self.log.borrow().clone()
        }
    }

    const PICTURE: &str = r#"{"type": "image", "path": "{picture}"}"#;

    /// The frame that sends a Kitty image logs the bytes of the
    /// transmission, how long the frame took and how long writing it took,
    /// so the log tells what showing an image cost. The test's view waits
    /// for the image to load, so the run's one frame sends it.
    #[test]
    fn a_frame_that_sends_a_kitty_image_logs_its_bytes_and_times() {
        let mut watched = Watched::new();
        watched.put("only", PICTURE, [255, 0, 0]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::kitty());

        let log = test_support::logged(|| {
            watched.run(&mut app, &mut inbox, &mut view, Vec::new());
        });

        let sent = transmission(watched.terminal.backend().backend.buffer()).len();
        let lines: Vec<&str> = log
            .lines()
            .filter(|line| line.contains("frame sent a Kitty image"))
            .collect();
        assert_eq!(lines.len(), 1, "{log}");
        assert!(lines[0].contains(&format!("bytes={sent} ")), "{log}");
        assert!(
            lines[0].contains(" took=") && lines[0].contains(" written="),
            "{log}"
        );
    }

    /// A frame that shows a Kitty image sent before sends nothing, and
    /// logs nothing about it: a resize to the same size keeps the
    /// encoding.
    #[test]
    fn a_frame_that_shows_a_kitty_image_already_sent_logs_nothing() {
        let mut watched = Watched::new();
        watched.put("only", PICTURE, [255, 0, 0]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::kitty());

        let log = test_support::logged(|| {
            watched.run(&mut app, &mut inbox, &mut view, vec![Event::Resize]);
        });

        assert_eq!(log.matches("frame sent a Kitty image").count(), 1, "{log}");
    }

    #[test]
    fn finishing_the_only_session_deletes_its_image_after_the_empty_queue_frame() {
        let mut watched = Watched::new();
        watched.put("only", PICTURE, [255, 0, 0]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::kitty());

        // `2` picks and moves on to the review, where `enter` submits.
        watched.run(
            &mut app,
            &mut inbox,
            &mut view,
            vec![key(KeyCode::Char('2')), key(KeyCode::Enter)],
        );

        let steps = watched.steps();
        let Some(Step::Frame(shown)) = steps.first() else {
            panic!("a frame first: {steps:?}");
        };
        assert_eq!(shown.len(), 1, "the session's image: {steps:?}");
        let deleted: Vec<u32> = shown.iter().copied().collect();
        let at = steps
            .iter()
            .position(|step| *step == Step::Deleted(deleted.clone()))
            .expect("the image is deleted");
        let Step::Frame(before) = &steps[at - 1] else {
            panic!("a frame comes before the delete: {steps:?}");
        };
        assert!(before.is_disjoint(shown), "the frame of the empty queue");
        assert!(app.sessions().is_empty());
    }

    /// A signal ends the run where it is, as `q` does, and leaves the
    /// images the frame shows for the deletes on quitting (ADR 42).
    #[test]
    fn a_quit_event_ends_the_run_and_keeps_the_images_for_the_exit_deletes() {
        let mut watched = Watched::new();
        watched.put("only", PICTURE, [255, 0, 0]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::kitty());

        // Should the run go on past the quit, `2` picks and `enter`
        // submits.
        watched.run(
            &mut app,
            &mut inbox,
            &mut view,
            vec![Event::Quit, key(KeyCode::Char('2')), key(KeyCode::Enter)],
        );

        assert_eq!(app.sessions().len(), 1, "the session still waits");
        let steps = watched.steps();
        let [Step::Frame(shown)] = &steps[..] else {
            panic!("one frame and no delete: {steps:?}");
        };
        assert_eq!(shown.len(), 1, "the session's image: {steps:?}");
        assert_eq!(
            view.kitty_book().drain().ids(),
            shown.iter().copied().collect::<Vec<_>>(),
            "the exit deletes name the image still shown"
        );
    }

    /// A frame that could not be flushed may still be on the screen, so
    /// the images that went before it stay in the book, for the deletes
    /// on quitting.
    #[test]
    fn a_frame_that_fails_to_flush_keeps_the_images_that_went_for_the_exit() {
        let mut watched = Watched::new();
        watched.put("only", PICTURE, [255, 0, 0]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::kitty());
        watched.run(&mut app, &mut inbox, &mut view, vec![]);

        watched.remove("only");
        inbox.sync(&mut app).expect("synced");
        watched.terminal.backend_mut().refuse_flush = true;
        let ran = watched.try_run(&mut app, &mut inbox, &mut view, vec![]);

        assert!(ran.is_err(), "the frame failed");
        assert_eq!(
            view.kitty_book().drain().ids(),
            [1],
            "{:?}",
            watched.steps()
        );
    }

    /// A delete that could not be written stays in the book: it is written
    /// after the next frame, or on quitting.
    #[test]
    fn a_delete_that_could_not_be_written_is_written_after_the_next_frame() {
        let mut watched = Watched::new();
        watched.put("only", PICTURE, [255, 0, 0]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests("test", PaneLayout::Auto, Images::kitty());
        let (app, inbox, view) = (&mut app, &mut inbox, &mut view);
        watched.run(app, inbox, view, vec![]);

        watched.remove("only");
        watched.harness.host.refuse_deletes = true;
        watched.run(app, inbox, view, vec![Event::InboxChanged]);
        watched.harness.host.refuse_deletes = false;
        watched.run(app, inbox, view, vec![]);

        let steps = watched.steps();
        let deleted: Vec<&Step> = steps
            .iter()
            .filter(|step| matches!(step, Step::Deleted(_)))
            .collect();
        assert_eq!(deleted, [&Step::Deleted(vec![1])], "{steps:?}");
        assert_eq!(view.kitty_book().drain().ids(), [0_u32; 0], "written once");
    }

    /// Reviewer O's invariant: a delete written after a frame never names
    /// an image that frame or any later one shows. The run goes through
    /// every way an image of a context entry goes: regaining the focus, an
    /// entry replaced by another image, by another kind, by an image that
    /// cannot be read, a session that leaves while another waits, and the
    /// last one leaving.
    #[test]
    fn no_kitty_image_is_deleted_while_a_frame_shows_it() {
        let mut watched = Watched::new();
        watched.put("a", PICTURE, [255, 0, 0]);
        watched.put("b", PICTURE, [0, 0, 255]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests(
            "test",
            PaneLayout::Auto,
            Images::kitty_through(Passthrough::WhileVisible),
        );
        let (app, inbox, view) = (&mut app, &mut inbox, &mut view);

        watched.run(app, inbox, view, vec![Event::Resize, Event::FocusGained]);
        watched.put("a", PICTURE, [0, 255, 0]);
        watched.run(app, inbox, view, vec![Event::InboxChanged]);
        watched.put(
            "a",
            r#"{"type": "code", "content": "fn main() {}"}"#,
            [0; 3],
        );
        watched.run(app, inbox, view, vec![Event::InboxChanged]);
        watched.put("a", PICTURE, [255, 255, 0]);
        watched.run(app, inbox, view, vec![Event::InboxChanged]);
        watched.put(
            "a",
            r#"{"type": "image", "path": "/nowhere/cat.png"}"#,
            [0; 3],
        );
        watched.run(app, inbox, view, vec![Event::InboxChanged]);
        watched.put("a", PICTURE, [255, 0, 255]);
        watched.run(
            app,
            inbox,
            view,
            vec![Event::InboxChanged, Event::FocusGained],
        );
        watched.remove("a");
        watched.run(app, inbox, view, vec![Event::InboxChanged]);
        watched.remove("b");
        watched.run(app, inbox, view, vec![Event::InboxChanged, Event::Resize]);

        let steps = watched.steps();
        let frames: Vec<(usize, &BTreeSet<u32>)> = steps
            .iter()
            .enumerate()
            .filter_map(|(at, step)| match step {
                Step::Frame(shown) => Some((at, shown)),
                _ => None,
            })
            .collect();
        let mut deleted = BTreeSet::new();
        for (at, step) in steps.iter().enumerate() {
            let Step::Deleted(ids) = step else {
                continue;
            };
            let last_frame = frames
                .iter()
                .rposition(|&(frame, _)| frame < at)
                .expect("a delete follows a frame");
            for &(frame, shown) in &frames[last_frame..] {
                for id in ids {
                    assert!(
                        !shown.contains(id),
                        "{id} deleted at step {at} shows in the frame at step {frame}: {steps:?}"
                    );
                }
            }
            deleted.extend(ids.iter().copied());
        }

        // Every image a frame showed before the empty queue went, so the
        // run did go through each way above.
        let (_, last) = frames.last().expect("frames");
        let shown_before: BTreeSet<u32> = frames
            .iter()
            .flat_map(|(_, shown)| shown.iter().copied())
            .filter(|id| !last.contains(id))
            .collect();
        assert_eq!(deleted, shown_before, "{steps:?}");
        assert_eq!(
            shown_before.len(),
            7,
            "a, again on focus, green, yellow, magenta, again on focus, b: {steps:?}"
        );
        assert!(app.sessions().is_empty());
    }

    /// Inside tmux with `allow-passthrough on`, a delete written while the
    /// pane is hidden is lost. The deletes since the last focus gain are
    /// written again after the frame that follows the next one, once, with
    /// those of that frame.
    #[test]
    fn regaining_the_focus_writes_the_deletes_since_the_last_time_again() {
        let mut watched = Watched::new();
        watched.put("a", PICTURE, [255, 0, 0]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::for_tests(
            "test",
            PaneLayout::Auto,
            Images::kitty_through(Passthrough::WhileVisible),
        );
        let (app, inbox, view) = (&mut app, &mut inbox, &mut view);

        watched.run(app, inbox, view, vec![]);
        watched.put("a", PICTURE, [0, 255, 0]);
        watched.run(
            app,
            inbox,
            view,
            vec![Event::InboxChanged, Event::FocusGained, Event::FocusGained],
        );

        let steps = watched.steps();
        // Each run draws once before its first event, so a frame repeats.
        let mut shown: Vec<u32> = steps
            .iter()
            .filter_map(|step| match step {
                Step::Frame(shown) => shown.first().copied(),
                _ => None,
            })
            .collect();
        shown.dedup();
        let deleted: Vec<&Vec<u32>> = steps
            .iter()
            .filter_map(|step| match step {
                Step::Deleted(ids) => Some(ids),
                _ => None,
            })
            .collect();
        let [red, green, again, once_more] = shown[..] else {
            panic!("four frames with an image: {steps:?}");
        };
        assert_eq!(
            deleted,
            [&vec![red], &vec![red, green], &vec![again]],
            "{once_more} stays shown"
        );
    }
}
