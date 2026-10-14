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

use super::render::{View, draw};
use super::{App, Applied, Inbox};
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
}

/// What the app asks of the world around it.
pub trait Host {
    /// Sessions arrived while asqr was running.
    fn alert(&mut self, arrived: &[String]);
    /// Open the image at `path` in the system viewer.
    fn open_image(&mut self, path: &str) -> io::Result<()>;
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
    loop {
        // An overlay closed over an image leaves its text on the image
        // unless the whole screen is drawn afresh (ADR 31).
        if view.must_clear(app) {
            terminal.clear().context("clearing the screen")?;
        }
        terminal
            .draw(|frame| draw(frame, app, view))
            .context("drawing the screen")?;
        // How far the intro overlay and the context pane can scroll depends
        // on the size they were drawn at, and where `↑`/`↓` go in a field
        // on where its rows break. A frame without a tab, as below the
        // minimum size, says nothing about the pane: handing it on would
        // end the pane's focus and the full-screen view, and the session
        // shown at the next tab may be another one.
        app.set_intro_page(view.intro_page());
        if let Some(page) = view.pane_page() {
            app.set_pane_page(page);
        }
        // A field entered from above finds its first row only in its first
        // draw, and its text cursor goes there; that draw showed it
        // elsewhere.
        if app.set_field_shape(view.field_shape()) {
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
            // Coming back into view is when a transfer lost in the
            // background can be repeated.
            Event::FocusGained => {
                view.forget_images();
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
                    Err(error) => app.notice(format!("cannot read the inbox: {error}")),
                }
                continue;
            }
        };
        match inbox.apply(app, effect) {
            Applied::Continue => {}
            Applied::Quit => return Ok(()),
            Applied::OpenImage(path) => {
                if let Err(error) = host.open_image(&path) {
                    app.notice(format!("cannot open {path}: {error}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::mpsc;

    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEventState, KeyModifiers};
    use ratatui::style::{Color, Modifier};
    use ratatui_image::picker::Picker;

    use super::*;
    use crate::format::{SessionResult, Status};
    use crate::tui::render::PaneLayout;

    const SESSION: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Pick one", "kind": "single",
        "context": [{"type": "image", "path": "/nowhere/cat.png"}],
        "options": [{"id": "a", "label": "Apples"}, {"id": "b", "label": "Bananas"}]}]}"#;

    #[derive(Default)]
    struct Recorder {
        alerts: Vec<Vec<String>>,
        opened: Vec<String>,
        /// Opening an image fails with this, when set.
        viewer_missing: bool,
    }

    impl Host for Recorder {
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
            let mut view = View::new("test", PaneLayout::Auto, Picker::halfblocks());
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
        assert!(harness.screen().contains("Nothing to answer"));
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
        let mut view = View::new("test", PaneLayout::Auto, Picker::halfblocks());
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

    #[test]
    fn the_intro_scrolls_as_far_as_the_last_draw_showed() {
        let harness = &mut Harness::new();
        let intro: Vec<String> = (1..=30).map(|n| format!("- line {n}")).collect();
        let session = SESSION.replacen(
            r#"{"asqr": 2,"#,
            &format!(r#"{{"asqr": 2, "intro": {:?},"#, intro.join("\n")),
            1,
        );
        fs::write(harness.location.inbox().join("long.json"), session).expect("written");

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

        harness.run(vec![key(KeyCode::Char('o'))]).expect("runs");

        assert!(
            harness
                .screen()
                .contains("cannot open /nowhere/cat.png: no viewer"),
            "{}",
            harness.screen()
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
        let mut view = View::new("test", PaneLayout::Auto, Picker::halfblocks());

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
    }

    // -----------------------------------------------------------------
    // Clearing the screen after an overlay over an image
    // -----------------------------------------------------------------

    /// A test backend that logs each clear of the screen and each frame
    /// flushed, so a test sees which frame a clear came before.
    struct Logging {
        backend: TestBackend,
        log: Vec<&'static str>,
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
            self.log.push("clear");
            self.backend.clear()
        }

        fn clear_region(
            &mut self,
            clear_type: ratatui::backend::ClearType,
        ) -> Result<(), Self::Error> {
            self.log.push("clear");
            self.backend.clear_region(clear_type)
        }

        fn size(&self) -> Result<ratatui::layout::Size, Self::Error> {
            self.backend.size()
        }

        fn window_size(&mut self) -> Result<ratatui::backend::WindowSize, Self::Error> {
            self.backend.window_size()
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.log.push("frame");
            self.backend.flush()
        }
    }

    /// The clears and frames of a run over `events` with `session` waiting.
    fn logged(session: &str, events: Vec<Event>) -> Vec<&'static str> {
        let harness = Harness::new();
        fs::write(harness.location.inbox().join("s.json"), session).expect("written");
        let (mut app, mut inbox) = start(harness.location.clone()).expect("started");
        let mut view = View::new("test", PaneLayout::Auto, Picker::halfblocks());
        let mut terminal = Terminal::new(Logging {
            backend: TestBackend::new(80, 24),
            log: Vec::new(),
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
        terminal.backend().log.clone()
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
}
