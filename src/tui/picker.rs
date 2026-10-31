// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The start picker (spec section 7.1, ADR 86): outside tmux, with no
//! queue chosen and several to choose from, asqr shows the queue
//! switcher's list before it takes a lock, and watches the queue picked.

use std::sync::mpsc::Receiver;

use anyhow::Context;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::KeyEventKind;

use super::render::draw_picker;
use super::{Event, Notice, REFUSED_NAME, Switcher, SwitcherKey, TOO_LONG_NAME, Tone};
use crate::queue::QueueLocation;

/// Shows `picker` until the person picks a queue that `take` takes, and
/// gives what `take` made of it, or `None` once the person quits. A queue
/// `take` refuses, with the notice it gives, leaves the picker open with
/// the notice in its status line. `version` is asqr's name and version,
/// which the key bar names.
///
/// The picker reads `events`, the run loop's channel, so a signal that
/// quits asqr quits the picker too.
pub fn pick_queue<B: Backend, T>(
    terminal: &mut Terminal<B>,
    mut picker: Switcher,
    events: &Receiver<Event>,
    version: &str,
    mut take: impl FnMut(&QueueLocation) -> Result<T, Notice>,
) -> anyhow::Result<Option<T>>
where
    B::Error: Send + Sync + 'static,
{
    let mut notice: Option<Notice> = None;
    loop {
        terminal
            .draw(|frame| draw_picker(frame, &picker, notice.as_ref(), version))
            .context("drawing the screen")?;
        let Ok(event) = events.recv() else {
            return Ok(None);
        };
        let answer = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => picker.handle(key),
            Event::Paste(text) => picker.paste(&text),
            Event::Quit => return Ok(None),
            Event::InputEnded(reason) => {
                return Err(anyhow::Error::msg(reason)).context("reading the terminal's input");
            }
            // A resize only draws again; nothing else concerns the picker.
            _ => continue,
        };
        // A notice lasts until the next key or paste, as in the run loop.
        notice = None;
        match answer {
            SwitcherKey::Stay | SwitcherKey::Close => {}
            SwitcherKey::Quit => return Ok(None),
            SwitcherKey::Refused => notice = Some(Notice::new(Tone::Warning, REFUSED_NAME)),
            SwitcherKey::TooLong => notice = Some(Notice::new(Tone::Warning, TOO_LONG_NAME)),
            SwitcherKey::Switch(target) => match take(&target) {
                Ok(taken) => return Ok(Some(taken)),
                Err(refusal) => notice = Some(refusal),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::tui::{Place, QueueChoice, QueueRow};

    fn row(name: &str) -> QueueRow {
        QueueRow {
            location: QueueLocation::named(Path::new("/data/asqr"), name),
            label: name.to_owned(),
            agent_dir: None,
            waiting: 0,
            watched: false,
        }
    }

    fn picker() -> Switcher {
        Switcher::picker(QueueChoice {
            rows: vec![row("asqr-todos"), row("default")],
            taken: Vec::new(),
            root: Some(PathBuf::from("/data/asqr")),
            place: Place::Outside,
        })
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// The picker over `events`, with `take` taking every queue but the
    /// ones `refused` names; what it gave and the screen it left.
    fn run_picker(
        events: Vec<Event>,
        refused: &[&str],
    ) -> (anyhow::Result<Option<String>>, Vec<String>, String) {
        let (sender, receiver) = mpsc::channel();
        for event in events {
            sender.send(event).expect("sent");
        }
        drop(sender);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut asked = Vec::new();
        let picked = pick_queue(&mut terminal, picker(), &receiver, "asqr 0.0.0", |target| {
            let name = target.name().expect("named").to_owned();
            asked.push(name.clone());
            if refused.contains(&name.as_str()) {
                Err(Notice::naming_queue(
                    Tone::Error,
                    "another asqr is watching queue ",
                    &name,
                    "",
                ))
            } else {
                Ok(name)
            }
        });
        let screen = terminal.backend().to_string();
        (picked, asked, screen)
    }

    #[test]
    fn enter_watches_the_queue_under_the_cursor() {
        let (picked, _, _) = run_picker(vec![key(KeyCode::Up), key(KeyCode::Enter)], &[]);

        assert_eq!(picked.expect("picks").as_deref(), Some("asqr-todos"));
    }

    #[test]
    fn a_queue_taken_meanwhile_leaves_the_picker_open_with_a_notice() {
        let (picked, asked, screen) = run_picker(vec![key(KeyCode::Enter)], &["default"]);

        assert_eq!(picked.expect("ends"), None, "the events ran out");
        assert_eq!(asked, ["default"]);
        assert!(
            screen.contains("✗ another asqr is watching queue default"),
            "{screen}"
        );
    }

    #[test]
    fn a_queue_taken_meanwhile_can_be_left_for_another() {
        let (picked, asked, _) = run_picker(
            vec![key(KeyCode::Enter), key(KeyCode::Up), key(KeyCode::Enter)],
            &["default"],
        );

        assert_eq!(picked.expect("picks").as_deref(), Some("asqr-todos"));
        assert_eq!(asked, ["default", "asqr-todos"]);
    }

    #[test]
    fn q_and_a_quit_signal_end_the_picker() {
        let (picked, asked, _) = run_picker(vec![key(KeyCode::Char('q'))], &[]);
        assert_eq!((picked.expect("ends"), asked.len()), (None, 0));

        let (picked, _, _) = run_picker(vec![Event::Quit, key(KeyCode::Enter)], &[]);
        assert_eq!(picked.expect("ends"), None);
    }

    #[test]
    fn a_refused_key_and_a_refused_paste_are_notices() {
        let (_, _, screen) = run_picker(
            vec![
                key(KeyCode::Down),
                key(KeyCode::Down),
                key(KeyCode::Char(' ')),
            ],
            &[],
        );
        assert!(screen.contains(&format!("! {REFUSED_NAME}")), "{screen}");

        let (_, _, screen) =
            run_picker(vec![key(KeyCode::Down), Event::Paste("a".repeat(201))], &[]);
        assert!(screen.contains(&format!("! {TOO_LONG_NAME}")), "{screen}");
    }

    #[test]
    fn the_end_of_the_input_is_an_error() {
        let (picked, _, _) =
            run_picker(vec![Event::InputEnded("the terminal is gone".into())], &[]);

        assert!(picked.is_err());
    }

    #[test]
    fn released_keys_and_other_events_do_nothing() {
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Enter,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        let (picked, asked, screen) = run_picker(
            vec![
                release,
                Event::Resize,
                Event::KeepAlive,
                Event::Loaded,
                Event::QueuesTick,
            ],
            &[],
        );

        assert_eq!((picked.expect("ends"), asked.len()), (None, 0));
        assert!(screen.contains("╭ Queues "), "{screen}");
    }
}
