// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The running app (spec section 7): draw, wait for the next event, act on
//! it, draw again. Events arrive through a channel that the binary feeds
//! from the terminal and the inbox watcher, and everything that reaches out
//! of the app goes through [`Host`], so tests run the loop against a test
//! backend.

use std::sync::mpsc::Receiver;

use anyhow::Context;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{KeyEvent, KeyEventKind};

use super::render::{Images, View, draw};
use super::{App, Applied, Inbox};
use crate::queue::QueueLocation;

/// What wakes the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Key(KeyEvent),
    /// The terminal changed its size; the next draw adapts.
    Resize,
    /// Something changed in the inbox.
    InboxChanged,
}

/// What the app asks of the world around it.
pub trait Host {
    /// Sessions arrived while asqr was running.
    fn alert(&mut self, arrived: &[String]);
    /// Open the image at `path` in the system viewer.
    fn open_image(&mut self, path: &str);
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
    images: &mut Images,
    view: &View,
    events: &Receiver<Event>,
    host: &mut impl Host,
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    loop {
        terminal
            .draw(|frame| draw(frame, app, view, images))
            .context("drawing the screen")?;
        let Ok(event) = events.recv() else {
            return Ok(());
        };
        match event {
            // Terminals that report releases and repeats (the kitty
            // keyboard protocol, Windows) would otherwise act twice.
            Event::Key(key) if key.kind != KeyEventKind::Press => {}
            Event::Key(key) => {
                let effect = app.handle(key);
                match inbox.apply(app, effect) {
                    Applied::Continue => {}
                    Applied::Quit => return Ok(()),
                    Applied::OpenImage(path) => host.open_image(&path),
                }
            }
            Event::Resize => {}
            // The inbox coming back or not is out of asqr's hands; the
            // notice says why nothing changes, and every later change
            // tries again.
            Event::InboxChanged => match inbox.sync(app) {
                Ok(synced) => {
                    if !synced.arrived.is_empty() {
                        host.alert(&synced.arrived);
                    }
                }
                Err(error) => app.notice(format!("cannot read the inbox: {error}")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::mpsc;

    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEventState, KeyModifiers};
    use ratatui_image::picker::Picker;

    use super::*;
    use crate::format::{SessionResult, Status};

    const SESSION: &str = r#"{"asqr": 1, "questions": [{"id": "q", "text": "Pick one", "kind": "single",
        "image": "/nowhere/cat.png",
        "options": [{"id": "a", "label": "Apples"}, {"id": "b", "label": "Bananas"}]}]}"#;

    #[derive(Default)]
    struct Recorder {
        alerts: Vec<Vec<String>>,
        opened: Vec<String>,
    }

    impl Host for Recorder {
        fn alert(&mut self, arrived: &[String]) {
            self.alerts.push(arrived.to_vec());
        }

        fn open_image(&mut self, path: &str) {
            self.opened.push(path.to_owned());
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
            let (sender, receiver) = mpsc::channel();
            for event in events {
                sender.send(event).expect("sent");
            }
            drop(sender);
            let mut images = Images::new(Picker::halfblocks());
            run(
                &mut self.terminal,
                app,
                inbox,
                &mut images,
                &View { queue: "test" },
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
            .run(vec![
                key(KeyCode::Char('2')),
                key(KeyCode::Char('j')),
                key(KeyCode::Enter),
            ])
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
    fn o_asks_the_host_to_open_the_image() {
        let mut harness = Harness::new();
        harness.put("batch-01");

        harness.run(vec![key(KeyCode::Char('o'))]).expect("runs");

        assert_eq!(harness.host.opened, ["/nowhere/cat.png"]);
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
}
