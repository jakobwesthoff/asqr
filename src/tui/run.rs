// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The running app (spec section 7): draw, wait for the next event, act on
//! it, draw again. Events arrive through a channel that the binary feeds
//! from the terminal and the inbox watcher, and everything that reaches out
//! of the app goes through [`Host`], so tests run the loop against a test
//! backend.

use std::io;
use std::sync::mpsc::Receiver;

use anyhow::Context;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{KeyEvent, KeyEventKind};

use super::render::{Drawn, View, draw};
use super::{App, Applied, Inbox, Tone};
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
    /// SIGTERM, SIGINT or SIGHUP asked asqr to quit (spec section 7).
    Quit,
}

/// What the app asks of the world around it.
pub trait Host {
    /// Sessions arrived while asqr was running.
    fn alert(&mut self, arrived: &[String]);
    /// Open the image at `path` in the system viewer.
    fn open_image(&mut self, path: &str) -> io::Result<()>;
    /// Show `title` as the terminal's title, unless the person switched
    /// titles off.
    fn title(&mut self, title: &str);
    /// Ask the terminal to delete the Kitty images `ids` names, which the
    /// frame just written no longer shows (ADR 42).
    fn delete_images(&mut self, ids: &[u32]);
}

/// The terminal title for the state of `app`, so a tab in the background
/// tells how many sessions wait (spec section 7.5).
pub fn terminal_title(app: &App) -> String {
    format!("asqr · {} waiting", app.sessions().len())
}

/// Reads the queue for the first time: what waits when asqr starts is
/// shown without an alert. Register the watcher before (spec section 3.4).
pub fn start(location: QueueLocation) -> anyhow::Result<(App, Inbox)> {
    let mut app = App::default();
    let mut inbox = Inbox::new(location);
    inbox.sync(&mut app).context("reading the inbox")?;
    Ok((app, inbox))
}

/// Runs the app until the person quits or the events end.
pub fn run<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    inbox: &mut Inbox,
    view: &mut View,
    events: &Receiver<Event>,
    host: &mut impl Host,
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    // Every key draws again, but the title goes to the terminal only when
    // its text changes. No title is empty, so the first pass sends one.
    let mut sent_title = String::new();
    // Inside tmux with `allow-passthrough on`, a delete written while the
    // pane is hidden is lost, as a transfer is (ADR 24). The deletes since
    // the last focus gain are written again after the frame that follows
    // the next one, and only then forgotten (ADR 42).
    let mut deleted_since_focus: Vec<u32> = Vec::new();
    let mut focus_regained = false;
    loop {
        let title = terminal_title(app);
        if title != sent_title {
            host.title(&title);
            sent_title = title;
        }
        // An overlay closed over an image leaves its text on the image
        // unless the whole screen is drawn afresh (ADR 31).
        if view.must_clear(app) {
            terminal.clear().context("clearing the screen")?;
        }
        let mut drawn = Drawn::default();
        terminal
            .draw(|frame| drawn = draw(frame, app, view))
            .context("drawing the screen")?;
        // The frame is flushed and shows no Kitty image that went, so the
        // terminal may let go of them (ADR 42).
        let mut deletes = std::mem::take(&mut drawn.deleted_images);
        if std::mem::take(&mut focus_regained) {
            deletes.splice(0..0, deleted_since_focus.drain(..));
        } else {
            deleted_since_focus.extend(&deletes);
        }
        if !deletes.is_empty() {
            host.delete_images(&deletes);
        }
        // How far the intro overlay and the context pane can scroll depends
        // on the size they were drawn at, and where `↑`/`↓` go in a field
        // on where its rows break. A frame without a tab, as below the
        // minimum size, says nothing about the intro or the pane: handing
        // it on would end the pane's focus and the full-screen view, and the
        // session shown at the next tab may be another one.
        if let Some(page) = drawn.intro_page {
            app.set_intro_page(page);
        }
        if let Some(page) = drawn.pane_page {
            app.set_pane_page(page);
        }
        // A field entered from above finds its first row only in its first
        // draw, and its text cursor goes there; that draw showed it
        // elsewhere.
        if app.set_field_shape(drawn.field_shape) {
            continue;
        }
        let Ok(event) = events.recv() else {
            return Ok(());
        };
        let effect = match event {
            // Terminals that report releases and repeats (the kitty
            // keyboard protocol, Windows) would otherwise act twice.
            Event::Key(key) if key.kind != KeyEventKind::Press => continue,
            Event::Key(key) => app.handle(key),
            Event::Paste(text) => app.paste(&text),
            Event::Resize => continue,
            // The caller gives the terminal back, as after `q`.
            Event::Quit => return Ok(()),
            // Coming back into view is when a transfer lost in the
            // background can be repeated.
            Event::FocusGained => {
                view.forget_images();
                focus_regained = true;
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
            Applied::OpenImage(path) => {
                if let Err(error) = host.open_image(&path) {
                    app.notice(Tone::Error, format!("cannot open {path}: {error}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::fs;
    use std::rc::Rc;
    use std::sync::mpsc;

    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEventState, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::format::{SessionResult, Status};
    use crate::graphics::Images;
    use crate::graphics::testing::kitty_ids_on_screen;
    use crate::tui::render::PaneLayout;

    const SESSION: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Pick one", "kind": "single",
        "context": [{"type": "image", "path": "/nowhere/cat.png"}],
        "options": [{"id": "a", "label": "Apples"}, {"id": "b", "label": "Bananas"}]}]}"#;

    #[derive(Default)]
    struct Recorder {
        alerts: Vec<Vec<String>>,
        opened: Vec<String>,
        titles: Vec<String>,
        /// Opening an image fails with this, when set.
        viewer_missing: bool,
        /// Where the Kitty images the loop deletes go, among the frames a
        /// [`Logging`] backend notes.
        log: Log,
    }

    impl Host for Recorder {
        fn delete_images(&mut self, ids: &[u32]) {
            self.log.borrow_mut().push(Step::Deleted(ids.to_vec()));
        }

        fn title(&mut self, title: &str) {
            self.titles.push(title.to_owned());
        }

        fn alert(&mut self, arrived: &[String]) {
            self.alerts.push(arrived.to_vec());
        }

        fn open_image(&mut self, path: &str) -> io::Result<()> {
            if self.viewer_missing {
                return Err(io::Error::new(io::ErrorKind::NotFound, "no viewer"));
            }
            self.opened.push(path.to_owned());
            Ok(())
        }
    }

    struct Harness {
        _scratch: tempfile::TempDir,
        location: QueueLocation,
        terminal: Terminal<TestBackend>,
        host: Recorder,
    }

    impl Harness {
        fn new() -> Self {
            let scratch = tempfile::tempdir().expect("temp dir");
            let location = QueueLocation::at(scratch.path().join("queue"));
            location.create_layout().expect("layout");
            Harness {
                _scratch: scratch,
                location,
                terminal: Terminal::new(TestBackend::new(80, 24)).expect("test terminal"),
                host: Recorder::default(),
            }
        }

        fn put(&self, id: &str) {
            fs::write(self.location.inbox().join(format!("{id}.json")), SESSION).expect("written");
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
            let mut view = View::new("test", PaneLayout::Auto, Images::half_blocks());
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
            terminal_title(&app)
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
        harness.put("batch-01");

        harness.run(vec![key(KeyCode::Char('o'))]).expect("runs");

        assert_eq!(harness.host.opened, ["/nowhere/cat.png"]);
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
        let mut view = View::new("test", PaneLayout::Auto, Images::half_blocks());
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
        let mut view = View::new("test", PaneLayout::Auto, Images::half_blocks());
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
        harness.put("batch-01");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");

        harness
            .run_started(&mut app, &mut inbox, vec![key(KeyCode::Char('o'))])
            .expect("runs");

        assert!(
            harness
                .screen()
                .contains("cannot open /nowhere/cat.png: no viewer"),
            "{}",
            harness.screen()
        );
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

    #[test]
    fn gaining_focus_sends_the_images_again() {
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
        let mut view = View::new("test", PaneLayout::Auto, Images::half_blocks());

        harness
            .run_with_view(&mut app, &mut inbox, &mut view, vec![])
            .expect("runs");
        assert!(shows_hue(&harness.terminal, red), "{}", harness.screen());

        // A changed file makes a new transfer visible: the image as loaded
        // stays in use until focus comes back.
        paint([0, 0, 255]);
        harness
            .run_with_view(&mut app, &mut inbox, &mut view, vec![Event::Resize])
            .expect("runs");
        assert!(
            shows_hue(&harness.terminal, red),
            "a redraw alone reuses the image"
        );

        harness
            .run_with_view(&mut app, &mut inbox, &mut view, vec![Event::FocusGained])
            .expect("runs");
        assert!(shows_hue(&harness.terminal, blue), "{}", harness.screen());
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
    }

    impl Backend for Logging {
        type Error = <TestBackend as Backend>::Error;

        fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
        {
            self.backend.draw(content)
        }

        fn hide_cursor(&mut self) -> Result<(), Self::Error> {
            self.backend.hide_cursor()
        }

        fn show_cursor(&mut self) -> Result<(), Self::Error> {
            self.backend.show_cursor()
        }

        fn get_cursor_position(&mut self) -> Result<ratatui::layout::Position, Self::Error> {
            self.backend.get_cursor_position()
        }

        fn set_cursor_position<P: Into<ratatui::layout::Position>>(
            &mut self,
            position: P,
        ) -> Result<(), Self::Error> {
            self.backend.set_cursor_position(position)
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.log.borrow_mut().push(Step::Clear);
            self.backend.clear()
        }

        fn clear_region(
            &mut self,
            clear_type: ratatui::backend::ClearType,
        ) -> Result<(), Self::Error> {
            self.log.borrow_mut().push(Step::Clear);
            self.backend.clear_region(clear_type)
        }

        fn size(&self) -> Result<ratatui::layout::Size, Self::Error> {
            self.backend.size()
        }

        fn window_size(&mut self) -> Result<ratatui::backend::WindowSize, Self::Error> {
            self.backend.window_size()
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            let shown = kitty_ids_on_screen(self.backend.buffer());
            self.log.borrow_mut().push(Step::Frame(shown));
            self.backend.flush()
        }
    }

    /// The clears and frames of a run over `events` with `session` waiting.
    fn logged(session: &str, events: Vec<Event>) -> Vec<&'static str> {
        let harness = Harness::new();
        fs::write(harness.location.inbox().join("s.json"), session).expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view = View::new("test", PaneLayout::Auto, Images::half_blocks());
        let log = Log::default();
        let mut terminal = Terminal::new(Logging {
            backend: TestBackend::new(80, 24),
            log: Rc::clone(&log),
        })
        .expect("test terminal");
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
        )
        .expect("runs");
        let steps = log.borrow();
        steps
            .iter()
            .map(|step| match step {
                Step::Clear => "clear",
                Step::Frame(_) => "frame",
                Step::Deleted(_) => "delete",
            })
            .collect()
    }

    #[test]
    fn closing_an_overlay_over_an_image_clears_the_screen_before_the_next_frame() {
        let session = SESSION.replacen(r#"{"asqr": 2,"#, r#"{"asqr": 2, "intro": "Fruit","#, 1);
        let overlays = [
            ("help", key(KeyCode::Char('?')), key(KeyCode::Char('x'))),
            ("intro", key(KeyCode::Char('i')), key(KeyCode::Esc)),
            ("session list", key(KeyCode::Char('L')), key(KeyCode::Esc)),
        ];

        for (overlay, open, close) in overlays {
            let log = logged(&session, vec![open, close, Event::Resize]);

            assert_eq!(
                log,
                ["frame", "frame", "clear", "frame", "frame"],
                "{overlay}: the first frame without it is drawn afresh, and only that one"
            );
        }
    }

    #[test]
    fn closing_an_overlay_without_an_image_on_screen_keeps_the_screen() {
        let session = SESSION.replace(
            r#"{"type": "image", "path": "/nowhere/cat.png"}"#,
            r#"{"type": "code", "content": "fn main() {}"}"#,
        );

        let log = logged(
            &session,
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
                terminal: Terminal::new(Logging {
                    backend: TestBackend::new(80, 24),
                    log: Rc::clone(&log),
                })
                .expect("test terminal"),
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
            )
            .expect("runs");
        }

        fn steps(&self) -> Vec<Step> {
            self.log.borrow().clone()
        }
    }

    const PICTURE: &str = r#"{"type": "image", "path": "{picture}"}"#;

    #[test]
    fn finishing_the_only_session_deletes_its_image_after_the_empty_queue_frame() {
        let mut watched = Watched::new();
        watched.put("only", PICTURE, [255, 0, 0]);
        let (mut app, mut inbox) = start(watched.harness.location.clone()).expect("started");
        let mut view = View::new("test", PaneLayout::Auto, Images::kitty());

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
        let mut view = View::new("test", PaneLayout::Auto, Images::kitty());

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
            view.take_kitty_images(),
            shown.iter().copied().collect::<Vec<_>>(),
            "the exit deletes name the image still shown"
        );
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
        let mut view = View::new("test", PaneLayout::Auto, Images::kitty());
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
        let mut view = View::new("test", PaneLayout::Auto, Images::kitty());
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
