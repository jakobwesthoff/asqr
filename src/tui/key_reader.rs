// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reading the terminal's events on a thread of their own, which can
//! pause, so a program asqr hands the terminal to, the person's editor
//! (ADR 74), gets every key typed while it runs.

use std::io;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ratatui::crossterm::event::Event as TerminalEvent;

use super::Event;

/// Where the terminal's events come from: crossterm's `poll` and `read`
/// in a running asqr, a stand-in in tests.
pub trait EventSource: Send + 'static {
    /// Whether an event can be read, waiting up to `timeout` for one.
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
    /// The next event; called after `poll` said there is one.
    fn read(&mut self) -> io::Result<TerminalEvent>;
}

/// How long one `poll` of the reader waits before it looks whether it is
/// to pause.
pub const POLL_WAIT: Duration = Duration::from_millis(50);

/// The handle of the thread that reads the terminal's events and sends
/// them to the run loop.
///
/// crossterm's `poll` reads what the terminal sends into a buffer of its
/// own, so a thread that polls while another program reads the terminal
/// takes keys meant for that program. A paused thread calls neither
/// `poll` nor `read`.
///
/// A `poll` or `read` that fails, or panics, ends the thread, which then
/// sends [`Event::InputEnded`].
#[derive(Clone)]
pub struct KeyReader {
    shared: Shared,
    /// Ends a `poll` the thread waits in, where the source has a way to;
    /// crossterm's ends after [`POLL_WAIT`] anyway.
    interrupt: Arc<dyn Fn() + Send + Sync>,
}

type Shared = Arc<(Mutex<Gate>, Condvar)>;

#[derive(Debug, Default)]
struct Gate {
    state: Reading,
    /// The thread ended, so nothing reads the terminal any more.
    ended: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Reading {
    #[default]
    Running,
    /// A pause was asked for; the thread stops at its next look at the
    /// gate.
    PauseAsked,
    Paused,
}

/// Marks the thread ended when it returns, also by a panic, so a pause
/// waiting for it is let go. A thread that ends by a panic also tells the
/// run loop that nothing reads the terminal's input any more; one that
/// returns has told it already, or found it gone.
struct Ended {
    shared: Shared,
    sender: Sender<Event>,
}

impl Drop for Ended {
    fn drop(&mut self) {
        lock(&self.shared).ended = true;
        self.shared.1.notify_all();
        if std::thread::panicking() {
            let _ = self.sender.send(Event::InputEnded(
                "the thread reading the terminal panicked".to_owned(),
            ));
        }
    }
}

impl KeyReader {
    /// Starts reading `source` into `sender`.
    pub fn spawn(source: impl EventSource, sender: Sender<Event>) -> Self {
        Self::spawn_with_interrupt(source, sender, || {})
    }

    /// [`KeyReader::spawn`], calling `interrupt` whenever a pause is asked
    /// for, to end a `poll` the thread may wait in.
    pub fn spawn_with_interrupt(
        mut source: impl EventSource,
        sender: Sender<Event>,
        interrupt: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let shared = Shared::default();
        let gate = Arc::clone(&shared);
        std::thread::spawn(move || {
            let _ended = Ended {
                shared: Arc::clone(&gate),
                sender: sender.clone(),
            };
            // A terminal whose input fails once is not read again. The run
            // loop learns why and ends; a loop that is gone needs nothing.
            let input_ended = |error: io::Error| {
                let _ = sender.send(Event::InputEnded(error.to_string()));
            };
            loop {
                wait_at_the_gate(&gate);
                match source.poll(POLL_WAIT) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(error) => return input_ended(error),
                }
                let event = match source.read() {
                    Ok(event) => event,
                    Err(error) => return input_ended(error),
                };
                let event = match event {
                    TerminalEvent::Key(key) => Event::Key(key),
                    TerminalEvent::Resize(..) => Event::Resize,
                    TerminalEvent::FocusGained => Event::FocusGained,
                    TerminalEvent::Paste(text) => Event::Paste(text),
                    _ => continue,
                };
                if sender.send(event).is_err() {
                    return;
                }
            }
        });
        KeyReader {
            shared,
            interrupt: Arc::new(interrupt),
        }
    }

    /// Stops reading the terminal, and says whether the thread stopped
    /// within `deadline`. crossterm may wait in a `read` past its poll's
    /// wait, for the rest of a sequence that came in part; at the deadline
    /// the pause is called off and the thread reads on. A thread that
    /// ended reads nothing, which counts as stopped.
    pub fn pause(&self, deadline: Duration) -> bool {
        let mut gate = lock(&self.shared);
        if gate.ended {
            return true;
        }
        gate.state = Reading::PauseAsked;
        drop(gate);
        (self.interrupt)();
        let (_, changed) = &*self.shared;
        let (mut gate, _) = changed
            .wait_timeout_while(lock(&self.shared), deadline, |gate| {
                gate.state != Reading::Paused && !gate.ended
            })
            .unwrap_or_else(PoisonError::into_inner);
        if gate.state == Reading::Paused || gate.ended {
            return true;
        }
        // Called off under the lock the thread looks at the gate under, so
        // a thread that reaches it now reads on rather than parking for a
        // pause nobody waits for.
        gate.state = Reading::Running;
        false
    }

    /// Reads again after a [`KeyReader::pause`].
    pub fn resume(&self) {
        lock(&self.shared).state = Reading::Running;
        self.shared.1.notify_all();
    }
}

/// Parks the thread while a pause holds, saying so to the pause that asked
/// for it.
fn wait_at_the_gate(shared: &Shared) {
    let (_, changed) = &**shared;
    let mut gate = lock(shared);
    if gate.state == Reading::PauseAsked {
        gate.state = Reading::Paused;
        changed.notify_all();
    }
    let _running = changed
        .wait_while(gate, |gate| gate.state != Reading::Running)
        .unwrap_or_else(PoisonError::into_inner);
}

/// The gate, also after a panic while it was locked.
fn lock(shared: &Shared) -> MutexGuard<'_, Gate> {
    shared.0.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::mpsc::{self, Receiver};

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    /// How long a test waits for the reader before it fails instead of
    /// hanging. A passing run never waits this long.
    const PATIENCE: Duration = Duration::from_secs(5);

    // -----------------------------------------------------------------
    // A stand-in for the terminal's input
    // -----------------------------------------------------------------

    /// What the reader and whoever else reads the terminal share.
    #[derive(Default)]
    struct Tty {
        /// Input nobody took yet.
        queued: VecDeque<TerminalEvent>,
        /// The reader waits in `poll`.
        polling: bool,
        /// Input handed to the reader waiting in `poll`.
        handed: Option<TerminalEvent>,
        /// The reader's `poll` is to give up, as crossterm's does when its
        /// timeout runs out.
        interrupted: bool,
        /// The reader's thread ended and dropped its source.
        dropped: bool,
        /// The next `poll` fails, or panics.
        failing: Option<Failure>,
    }

    #[derive(Clone, Copy)]
    enum Failure {
        Error,
        Panic,
        /// The poll says there is an event, and reading it fails.
        Read,
    }

    /// The terminal's input as the test sees it. Input goes to whoever
    /// asked for it first: the reader waiting in `poll` takes it at once,
    /// so an editor reading the terminal gets only what the reader leaves.
    #[derive(Clone, Default)]
    struct FakeTty(Arc<(Mutex<Tty>, Condvar)>);

    impl FakeTty {
        fn lock(&self) -> MutexGuard<'_, Tty> {
            self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// Waits while `waiting` holds for the terminal, failing after a
        /// while instead of hanging.
        fn wait_while(&self, mut waiting: impl FnMut(&mut Tty) -> bool) {
            let (tty, changed) = &*self.0;
            let (_tty, timeout) = changed
                .wait_timeout_while(
                    tty.lock().unwrap_or_else(PoisonError::into_inner),
                    PATIENCE,
                    |tty| waiting(tty),
                )
                .unwrap_or_else(PoisonError::into_inner);
            assert!(!timeout.timed_out(), "the reader never got there");
        }

        /// The person types `key`.
        fn push(&self, key: char) {
            let event = TerminalEvent::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            let mut tty = self.lock();
            if tty.polling && tty.handed.is_none() {
                tty.handed = Some(event);
            } else {
                tty.queued.push_back(event);
            }
            drop(tty);
            self.0.1.notify_all();
        }

        /// What an editor reading the terminal now gets.
        fn try_take(&self) -> Option<char> {
            match self.lock().queued.pop_front()? {
                TerminalEvent::Key(key) => key.code.as_char(),
                _ => None,
            }
        }

        fn wait_for_the_reader_to_poll(&self) {
            self.wait_while(|tty| !tty.polling);
        }

        fn wait_for_the_reader_to_end(&self) {
            self.wait_while(|tty| !tty.dropped);
        }

        /// Ends the reader's `poll` without input.
        fn interrupt(&self) {
            let mut tty = self.lock();
            if tty.polling {
                tty.interrupted = true;
            }
            drop(tty);
            self.0.1.notify_all();
        }

        fn fail_next_poll(&self, failure: Failure) {
            self.lock().failing = Some(failure);
            self.0.1.notify_all();
        }

        /// The source the reader reads.
        fn source(&self) -> FakeSource {
            FakeSource {
                tty: self.clone(),
                read: None,
            }
        }
    }

    struct FakeSource {
        tty: FakeTty,
        /// What the last `poll` found, for `read`.
        read: Option<TerminalEvent>,
    }

    impl EventSource for FakeSource {
        fn poll(&mut self, _timeout: Duration) -> io::Result<bool> {
            let (tty, changed) = &*self.tty.0;
            let mut state = tty.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(event) = state.queued.pop_front() {
                self.read = Some(event);
                return Ok(true);
            }
            state.polling = true;
            changed.notify_all();
            let mut state = changed
                .wait_while(state, |tty| {
                    tty.handed.is_none() && !tty.interrupted && tty.failing.is_none()
                })
                .unwrap_or_else(PoisonError::into_inner);
            state.polling = false;
            match state.failing.take() {
                Some(Failure::Error) => return Err(io::Error::other("the terminal is gone")),
                Some(Failure::Panic) => {
                    drop(state);
                    panic!("the source panics");
                }
                Some(Failure::Read) => return Ok(true),
                None => {}
            }
            if std::mem::take(&mut state.interrupted) {
                return Ok(false);
            }
            self.read = state.handed.take();
            Ok(true)
        }

        fn read(&mut self) -> io::Result<TerminalEvent> {
            self.read
                .take()
                .ok_or_else(|| io::Error::other("read without a poll"))
        }
    }

    impl Drop for FakeSource {
        fn drop(&mut self) {
            self.tty.lock().dropped = true;
            self.tty.0.1.notify_all();
        }
    }

    // -----------------------------------------------------------------
    // The reader
    // -----------------------------------------------------------------

    /// A reader of `tty` whose pause interrupts the stand-in's `poll`, and
    /// the run loop's end of the channel.
    fn reading(tty: &FakeTty) -> (KeyReader, Receiver<Event>) {
        let (sender, events) = mpsc::channel();
        let interrupt = tty.clone();
        let reader =
            KeyReader::spawn_with_interrupt(tty.source(), sender, move || interrupt.interrupt());
        (reader, events)
    }

    fn typed(events: &Receiver<Event>) -> Option<char> {
        match events.recv_timeout(PATIENCE).expect("an event arrives") {
            Event::Key(key) => key.code.as_char(),
            _ => None,
        }
    }

    #[test]
    fn a_paused_reader_leaves_the_terminal_s_input_to_the_editor() {
        let tty = FakeTty::default();
        let (reader, events) = reading(&tty);
        tty.wait_for_the_reader_to_poll();

        assert!(reader.pause(PATIENCE), "paused");
        // The editor runs: the person types, and the editor reads it.
        tty.push('e');
        let editor_got = tty.try_take();
        reader.resume();
        tty.push('x');

        assert_eq!(editor_got, Some('e'));
        assert_eq!(typed(&events), Some('x'), "the reader reads again");
    }

    #[test]
    fn keys_after_resume_reach_the_app_in_order() {
        let tty = FakeTty::default();
        let (reader, events) = reading(&tty);
        tty.wait_for_the_reader_to_poll();
        assert!(reader.pause(PATIENCE));
        reader.resume();

        for key in ['a', 'b', 'c'] {
            tty.push(key);
        }

        assert_eq!(
            [typed(&events), typed(&events), typed(&events)],
            [Some('a'), Some('b'), Some('c')]
        );
    }

    #[test]
    fn a_pause_gives_up_at_its_deadline_and_the_reader_reads_on() {
        let tty = FakeTty::default();
        let (sender, events) = mpsc::channel();
        // The reader's `poll` waits and nothing interrupts it, as when
        // crossterm waits for the rest of a sequence that came in part.
        let reader = KeyReader::spawn(tty.source(), sender);
        tty.wait_for_the_reader_to_poll();

        assert!(!reader.pause(Duration::ZERO), "not paused");
        tty.push('x');

        assert_eq!(typed(&events), Some('x'));
    }

    #[test]
    fn a_pause_returns_once_the_reader_ended_on_an_error() {
        let tty = FakeTty::default();
        let (sender, _events) = mpsc::channel();
        let reader = KeyReader::spawn(tty.source(), sender);
        tty.wait_for_the_reader_to_poll();

        tty.fail_next_poll(Failure::Error);
        tty.wait_for_the_reader_to_end();

        assert!(reader.pause(PATIENCE), "nothing reads any more");
    }

    #[test]
    fn the_reader_ends_when_an_event_cannot_be_read() {
        let tty = FakeTty::default();
        let (sender, _events) = mpsc::channel();
        let reader = KeyReader::spawn(tty.source(), sender);
        tty.wait_for_the_reader_to_poll();

        tty.fail_next_poll(Failure::Read);
        tty.wait_for_the_reader_to_end();

        assert!(reader.pause(PATIENCE), "nothing reads any more");
    }

    #[test]
    fn a_pause_returns_once_the_reader_panicked() {
        let tty = FakeTty::default();
        let (sender, _events) = mpsc::channel();
        let reader = KeyReader::spawn(tty.source(), sender);
        tty.wait_for_the_reader_to_poll();

        tty.fail_next_poll(Failure::Panic);
        tty.wait_for_the_reader_to_end();

        assert!(reader.pause(PATIENCE), "nothing reads any more");
    }

    /// What the run loop gets once the reader's `poll` failed as `failure`.
    fn last_word_after(failure: Failure) -> Event {
        let tty = FakeTty::default();
        let (_reader, events) = reading(&tty);
        tty.wait_for_the_reader_to_poll();

        tty.fail_next_poll(failure);

        events.recv_timeout(PATIENCE).expect("an event arrives")
    }

    #[test]
    fn a_failed_poll_tells_the_run_loop_that_input_ended() {
        assert_eq!(
            last_word_after(Failure::Error),
            Event::InputEnded("the terminal is gone".to_owned())
        );
    }

    #[test]
    fn a_failed_read_tells_the_run_loop_that_input_ended() {
        assert_eq!(
            last_word_after(Failure::Read),
            Event::InputEnded("read without a poll".to_owned())
        );
    }

    #[test]
    fn a_panicking_poll_tells_the_run_loop_that_input_ended() {
        assert_eq!(
            last_word_after(Failure::Panic),
            Event::InputEnded("the thread reading the terminal panicked".to_owned())
        );
    }

    #[test]
    fn the_reader_ends_when_the_run_loop_is_gone() {
        let tty = FakeTty::default();
        let (reader, events) = reading(&tty);
        tty.wait_for_the_reader_to_poll();

        drop(events);
        tty.push('x');

        tty.wait_for_the_reader_to_end();
        drop(reader);
    }

    #[test]
    fn events_that_are_no_input_are_skipped() {
        let tty = FakeTty::default();
        let (_reader, events) = reading(&tty);
        tty.wait_for_the_reader_to_poll();

        {
            let mut state = tty.lock();
            state.queued.push_back(TerminalEvent::FocusLost);
            state.queued.push_back(TerminalEvent::Resize(80, 24));
            state.queued.push_back(TerminalEvent::FocusGained);
            state
                .queued
                .push_back(TerminalEvent::Paste("pasted".to_owned()));
        }
        tty.push('x');

        let received: Vec<Event> = (0..4)
            .map(|_| events.recv_timeout(PATIENCE).expect("an event arrives"))
            .collect();
        assert_eq!(
            received,
            [
                Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
                Event::Resize,
                Event::FocusGained,
                Event::Paste("pasted".to_owned()),
            ]
        );
    }
}
