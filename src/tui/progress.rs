// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! How far the session being answered is, reported to the terminal
//! through OSC 9;4 (spec section 7.9, ADR 73): the share of its
//! questions that have an answer, which terminals that know the sequence
//! show as a bar or on the tab.
//!
//! Terminals that do not know OSC 9;4 may show any OSC 9 as a desktop
//! notification, so asqr sends it only to terminals known to show it,
//! from the version that does. A terminal that clears the progress after
//! a while without an update gets it again every second while it shows.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::App;
use crate::terminal_env::{TerminalEnv, TmuxState, passthrough, version_at_least};

/// The share of the active session's questions that have an answer, in
/// percent and rounded down, or `None` where there is nothing to report:
/// no session waits, or the session asks nothing, as one of info items
/// only. An untouched default counts as answered and info items count
/// neither way, as in the review's counts. The review tab keeps the
/// share, so it reads 100 once every question is answered.
pub fn session_progress(app: &App) -> Option<u8> {
    let counts = app.active()?.counts();
    (counts.questions > 0).then(|| {
        u8::try_from(counts.answered * 100 / counts.questions)
            .expect("no more questions are answered than there are")
    })
}

/// Whether the terminal shows OSC 9;4, and whether it needs the progress
/// again every second to keep showing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressTerminal {
    /// The terminal is not known to show the progress; asqr sends none.
    None,
    /// The terminal shows the progress until it changes.
    Plain,
    /// The terminal clears the progress after a while without an update.
    KeepAlive,
}

/// What the terminal asqr runs in does with OSC 9;4, decided from `env`
/// and, inside a tmux pane, from what tmux said about it.
///
/// Inside tmux, the environment holds what the terminal tmux was started
/// from set, so the client's answer to XTVERSION decides, as for Talqi's
/// glyphs. With more than one client attached, the passthrough reaches
/// terminals the answer does not describe, so none gets the progress.
pub fn progress_terminal(env: &TerminalEnv, tmux: Option<&TmuxState>) -> ProgressTerminal {
    if !env.in_tmux_pane() {
        return from_env(env);
    }
    match tmux {
        Some(state) if state.clients() == Some(1) => state
            .termtype()
            .map_or(ProgressTerminal::None, from_xtversion),
        _ => ProgressTerminal::None,
    }
}

/// The first version of iTerm2 that shows OSC 9;4.
const ITERM2: &[u32] = &[3, 6, 6];
/// The first version of Ghostty that shows OSC 9;4.
const GHOSTTY: &[u32] = &[1, 2, 0];
/// The first version of kitty that shows OSC 9;4, as a percentage in the
/// tab's title. 0.38 drops the sequence; earlier versions show it as a
/// notification.
const KITTY: &[u32] = &[0, 39, 0];

/// [`progress_terminal`] for the terminal `env` names. The first rule
/// that names a terminal decides.
fn from_env(env: &TerminalEnv) -> ProgressTerminal {
    let set = |name: &str| env.set(name);
    // GNU screen and Zellij draw their own screen and pass on the
    // variables of whichever terminal they were started from.
    if set("TERM").is_some_and(|term| term.starts_with("screen")) || set("ZELLIJ").is_some() {
        return ProgressTerminal::None;
    }
    let at_least =
        |version: Option<&str>, floor| version_at_least(version.unwrap_or_default(), floor);
    // A terminal sets `TERM_PROGRAM` itself, while `WT_SESSION` may be left
    // from the terminal another one was started from.
    if let Some(program) = set("TERM_PROGRAM") {
        let version = set("TERM_PROGRAM_VERSION");
        return match program {
            "ghostty" if at_least(version, GHOSTTY) => ProgressTerminal::KeepAlive,
            "iTerm.app" if at_least(version, ITERM2) => ProgressTerminal::Plain,
            _ => ProgressTerminal::None,
        };
    }
    // iTerm2's variables come along over ssh, where `TERM_PROGRAM` stays
    // behind.
    if set("LC_TERMINAL") == Some("iTerm2") {
        return if at_least(set("LC_TERMINAL_VERSION"), ITERM2) {
            ProgressTerminal::Plain
        } else {
            ProgressTerminal::None
        };
    }
    // Windows Terminal names no version, so one before 1.6, the first that
    // shows the progress, is not told apart.
    if set("WT_SESSION").is_some() {
        return ProgressTerminal::Plain;
    }
    ProgressTerminal::None
}

/// [`progress_terminal`] for the terminal that answered XTVERSION with
/// `text`, such as `ghostty 1.2.0` or `kitty(0.39.0)`.
fn from_xtversion(text: &str) -> ProgressTerminal {
    let after = |prefix: &str| text.strip_prefix(prefix);
    if after("ghostty ").is_some_and(|version| version_at_least(version, GHOSTTY)) {
        return ProgressTerminal::KeepAlive;
    }
    let shows = after("iTerm2 ").is_some_and(|version| version_at_least(version, ITERM2))
        || after("kitty(").is_some_and(|version| version_at_least(version, KITTY));
    if shows {
        ProgressTerminal::Plain
    } else {
        ProgressTerminal::None
    }
}

/// The bytes that show `percent` as the progress, or remove it for
/// `None`, wrapped for tmux's passthrough when `tmux`. They end with ST,
/// as VTE takes no other end for OSC 9;4, and the remove names its state,
/// as iTerm2 3.6.6 fails on an OSC 9;4 without one.
pub fn progress_sequence(percent: Option<u8>, tmux: bool) -> Vec<u8> {
    let sequence = match percent {
        Some(percent) => format!("\x1b]9;4;1;{percent}\x1b\\"),
        None => "\x1b]9;4;0\x1b\\".to_owned(),
    };
    if tmux {
        passthrough(sequence.as_bytes())
    } else {
        sequence.into_bytes()
    }
}

// ---------------------------------------------------------------------
// Keeping the progress alive
// ---------------------------------------------------------------------

/// How often a terminal that clears the progress after a while gets it
/// again. Ghostty clears it after about 15 seconds and asks for an update
/// at least every second.
pub const KEEP_ALIVE_PERIOD: Duration = Duration::from_secs(1);

/// Waits for a period of the keep-alive.
pub trait Clock: Send + 'static {
    fn wait(&mut self, period: Duration);
}

/// The clock of a running asqr: it sleeps.
pub struct SleepClock;

impl Clock for SleepClock {
    fn wait(&mut self, period: Duration) {
        std::thread::sleep(period);
    }
}

#[derive(Debug, Default)]
struct Ticking {
    active: bool,
    /// How often the ticks were switched off. A period during which this
    /// changed ends without a tick, also when they were switched on again
    /// before it ended.
    stops: u64,
    /// The handle went, so the thread ends.
    ended: bool,
}

type Shared = Arc<(Mutex<Ticking>, Condvar)>;

/// The state of a keep-alive, also after a panic while it was locked: the
/// flags in it still hold.
fn lock(shared: &Shared) -> MutexGuard<'_, Ticking> {
    shared.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Calls a tick every [`KEEP_ALIVE_PERIOD`] on a thread of its own while
/// it is active, so the run loop resends the progress (ADR 73). The thread
/// ends when the handle is dropped or a tick says that nobody listens.
pub struct KeepAlive {
    shared: Shared,
}

impl KeepAlive {
    /// Starts the thread, inactive; `tick` returns `false` once nobody
    /// takes its ticks any more.
    pub fn spawn(mut clock: impl Clock, mut tick: impl FnMut() -> bool + Send + 'static) -> Self {
        let shared: Shared = Arc::default();
        let ticking = Arc::clone(&shared);
        std::thread::spawn(move || {
            loop {
                let stops = {
                    let (_, changed) = &*ticking;
                    let state = changed
                        .wait_while(lock(&ticking), |state| !state.active && !state.ended)
                        .unwrap_or_else(PoisonError::into_inner);
                    if state.ended {
                        return;
                    }
                    state.stops
                };
                clock.wait(KEEP_ALIVE_PERIOD);
                // Switched off while the period ran, the progress may be
                // gone; a tick would show it again.
                let state = lock(&ticking);
                if state.ended {
                    return;
                }
                if !state.active || state.stops != stops {
                    continue;
                }
                drop(state);
                if !tick() {
                    return;
                }
            }
        });
        KeepAlive { shared }
    }

    /// Whether the ticks come.
    pub fn is_active(&self) -> bool {
        lock(&self.shared).active
    }

    /// Lets the ticks come, or stops them, also the one of a period that
    /// has begun.
    pub fn set_active(&self, active: bool) {
        let mut state = lock(&self.shared);
        if state.active && !active {
            state.stops += 1;
        }
        state.active = active;
        drop(state);
        self.shared.1.notify_all();
    }
}

impl Drop for KeepAlive {
    fn drop(&mut self) {
        lock(&self.shared).ended = true;
        self.shared.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::SessionState;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    // -----------------------------------------------------------------
    // The share
    // -----------------------------------------------------------------

    /// An app with one session made of `questions`, a JSON list.
    fn app_with(questions: &str) -> App {
        let mut app = App::default();
        let session = serde_json::from_str(&format!(r#"{{"asqr": 2, "questions": {questions}}}"#))
            .expect("test session parses");
        app.add(SessionState::new("progress", session, None));
        app
    }

    fn single(id: &str, default: bool) -> String {
        format!(
            r#"{{"id": "{id}", "text": "?", "kind": "single",
                "options": [{{"id": "a", "label": "A", "default": {default}}}, {{"id": "b", "label": "B"}}]}}"#
        )
    }

    fn info(id: &str) -> String {
        format!(r#"{{"id": "{id}", "text": "Read this.", "kind": "info"}}"#)
    }

    fn press(app: &mut App, c: char) {
        app.handle(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }

    #[test]
    fn no_session_shows_no_progress() {
        assert_eq!(session_progress(&App::default()), None);
    }

    #[test]
    fn the_share_of_answered_questions_rounds_down() {
        let questions = [single("a", false), single("b", false), single("c", false)];
        let mut app = app_with(&format!("[{}]", questions.join(",")));
        assert_eq!(session_progress(&app), Some(0), "nothing answered yet");

        // `1` picks and moves on to the next question.
        press(&mut app, '1');

        assert_eq!(session_progress(&app), Some(33));
    }

    #[test]
    fn an_untouched_default_counts_as_answered() {
        let app = app_with(&format!("[{}, {}]", single("a", true), single("b", false)));
        assert_eq!(session_progress(&app), Some(50));
    }

    #[test]
    fn the_review_tab_keeps_the_session_s_share() {
        let mut app = app_with(&format!("[{}, {}]", single("a", false), single("b", false)));

        // Picking on the last question moves on to the review.
        press(&mut app, '1');
        press(&mut app, '2');

        assert!(app.active().expect("waiting").on_review());
        assert_eq!(session_progress(&app), Some(100));
    }

    #[test]
    fn a_session_of_info_items_only_shows_no_progress() {
        let app = app_with(&format!("[{}, {}]", info("i"), info("j")));
        assert_eq!(session_progress(&app), None);
    }

    #[test]
    fn info_items_count_neither_way() {
        let mut app = app_with(&format!(
            "[{}, {}, {}]",
            single("a", false),
            info("i"),
            single("b", false)
        ));
        press(&mut app, '1');
        assert_eq!(session_progress(&app), Some(50));
    }

    // -----------------------------------------------------------------
    // The bytes
    // -----------------------------------------------------------------

    fn text(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).expect("escape sequences are ASCII")
    }

    #[test]
    fn a_share_is_set_with_state_1_and_ends_with_st() {
        assert_eq!(
            text(progress_sequence(Some(33), false)),
            "\x1b]9;4;1;33\x1b\\"
        );
    }

    #[test]
    fn no_share_removes_the_progress_with_state_0() {
        assert_eq!(text(progress_sequence(None, false)), "\x1b]9;4;0\x1b\\");
    }

    #[test]
    fn inside_tmux_the_sequence_is_wrapped_for_passthrough() {
        assert_eq!(
            text(progress_sequence(Some(33), true)),
            "\x1bPtmux;\x1b\x1b]9;4;1;33\x1b\x1b\\\x1b\\"
        );
    }

    // -----------------------------------------------------------------
    // The terminals
    // -----------------------------------------------------------------

    use ProgressTerminal::{KeepAlive as Kept, None as Off, Plain};

    fn outside_tmux(vars: &[(&str, &str)]) -> ProgressTerminal {
        progress_terminal(&TerminalEnv::from_pairs(vars), None)
    }

    /// Inside a tmux pane whose one client answered XTVERSION with
    /// `termtype`; the outer environment names Ghostty, which must not
    /// count there.
    fn inside_tmux(termtype: &str, clients: u32) -> ProgressTerminal {
        let env = TerminalEnv::from_pairs(&[
            ("TMUX", "/tmp/tmux-501/default,1,0"),
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.2.0"),
        ]);
        let state = TmuxState::parse(&format!("on\t{clients}\t{termtype}\t1\t1\t0\t3.5"));
        progress_terminal(&env, Some(&state))
    }

    #[test]
    fn known_terminals_show_progress_from_their_environment() {
        let cases: [(&[(&str, &str)], ProgressTerminal); 5] = [
            (
                &[
                    ("TERM_PROGRAM", "ghostty"),
                    ("TERM_PROGRAM_VERSION", "1.2.0"),
                ],
                Kept,
            ),
            (
                &[
                    ("TERM_PROGRAM", "iTerm.app"),
                    ("TERM_PROGRAM_VERSION", "3.6.6"),
                ],
                Plain,
            ),
            (
                &[("LC_TERMINAL", "iTerm2"), ("LC_TERMINAL_VERSION", "3.7.5")],
                Plain,
            ),
            (
                &[("WT_SESSION", "0b0c5b5a-3c4e-4f6a-9d2e-1c2b3a4d5e6f")],
                Plain,
            ),
            (
                &[
                    ("TERM_PROGRAM", "ghostty"),
                    ("TERM_PROGRAM_VERSION", "1.3.1"),
                ],
                Kept,
            ),
        ];
        for (vars, expected) in cases {
            assert_eq!(outside_tmux(vars), expected, "{vars:?}");
        }
    }

    #[test]
    fn known_terminals_show_progress_from_their_xtversion_inside_tmux() {
        assert_eq!(inside_tmux("ghostty 1.2.0", 1), Kept);
        assert_eq!(inside_tmux("iTerm2 3.6.6", 1), Plain);
        assert_eq!(inside_tmux("kitty(0.39.0)", 1), Plain);
    }

    #[test]
    fn versions_before_the_first_that_shows_progress_show_none() {
        assert_eq!(
            outside_tmux(&[
                ("TERM_PROGRAM", "ghostty"),
                ("TERM_PROGRAM_VERSION", "1.1.3")
            ]),
            Off
        );
        assert_eq!(
            outside_tmux(&[
                ("TERM_PROGRAM", "iTerm.app"),
                ("TERM_PROGRAM_VERSION", "3.6.5")
            ]),
            Off
        );
        assert_eq!(inside_tmux("kitty(0.38.1)", 1), Off);
        assert_eq!(inside_tmux("iTerm2 3.6.5", 1), Off);
        assert_eq!(inside_tmux("ghostty 1.1.3", 1), Off);
    }

    #[test]
    fn terminals_without_a_known_version_show_none() {
        for vars in [
            &[("TERM_PROGRAM", "ghostty")][..],
            &[("TERM", "xterm-ghostty")],
            &[("TERM", "xterm-kitty"), ("KITTY_WINDOW_ID", "1")],
            &[("TERM_PROGRAM", "iTerm.app")],
            &[("LC_TERMINAL", "iTerm2")],
        ] {
            assert_eq!(outside_tmux(vars), Off, "{vars:?}");
        }
    }

    #[test]
    fn other_terminals_show_none() {
        for vars in [
            &[("TERM_PROGRAM", "WezTerm")][..],
            &[("TERM_PROGRAM", "vscode"), ("WT_SESSION", "x")],
            &[("TERM", "foot")],
            &[("VTE_VERSION", "8002")],
            &[("TERM", "xterm-256color")],
            &[],
        ] {
            assert_eq!(outside_tmux(vars), Off, "{vars:?}");
        }
        assert_eq!(inside_tmux("WezTerm 20240203-110809-5046fc22", 1), Off);
        assert_eq!(inside_tmux("", 1), Off);
    }

    #[test]
    fn screen_and_zellij_show_none() {
        let ghostty = [
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.2.0"),
        ];
        let mut in_screen = ghostty.to_vec();
        in_screen.push(("TERM", "screen-256color"));
        let mut in_zellij = ghostty.to_vec();
        in_zellij.push(("ZELLIJ", "0"));

        assert_eq!(outside_tmux(&in_screen), Off);
        assert_eq!(outside_tmux(&in_zellij), Off);
    }

    // -----------------------------------------------------------------
    // The keep-alive
    // -----------------------------------------------------------------

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};

    /// How long a test waits for the keep-alive's thread before it fails
    /// instead of hanging. A passing run never waits this long: every step
    /// it waits for is one the test let happen.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// A clock whose periods last until the test lets them pass. Each wait
    /// says so on `waits` first.
    struct GatedClock {
        waits: Sender<Duration>,
        passes: Receiver<()>,
    }

    impl Clock for GatedClock {
        fn wait(&mut self, period: Duration) {
            let _ = self.waits.send(period);
            let _ = self.passes.recv();
        }
    }

    /// A keep-alive on a gated clock, the channels the test drives it
    /// through, and whether its ticks are still taken.
    struct Driven {
        keep_alive: KeepAlive,
        waits: Receiver<Duration>,
        passes: Sender<()>,
        ticks: Receiver<()>,
        listening: Arc<AtomicBool>,
    }

    impl Driven {
        fn new() -> Self {
            let (waits_to, waits) = mpsc::channel();
            let (passes, passes_from) = mpsc::channel();
            let (ticks_to, ticks) = mpsc::channel();
            let listening = Arc::new(AtomicBool::new(true));
            let taken = Arc::clone(&listening);
            let keep_alive = KeepAlive::spawn(
                GatedClock {
                    waits: waits_to,
                    passes: passes_from,
                },
                move || {
                    let _ = ticks_to.send(());
                    taken.load(Ordering::SeqCst)
                },
            );
            Driven {
                keep_alive,
                waits,
                passes,
                ticks,
                listening,
            }
        }

        /// Waits until a period began, and says how long it is.
        fn period_begins(&self) -> Duration {
            self.waits
                .recv_timeout(PATIENCE)
                .expect("the keep-alive waits for a period")
        }

        fn let_the_period_pass(&self) {
            self.passes.send(()).expect("the clock waits");
        }

        fn tick_comes(&self) {
            self.ticks
                .recv_timeout(PATIENCE)
                .expect("the keep-alive ticks");
        }
    }

    #[test]
    fn the_first_tick_comes_a_period_after_the_keep_alive_is_switched_on() {
        let driven = Driven::new();

        driven.keep_alive.set_active(true);

        assert_eq!(driven.period_begins(), KEEP_ALIVE_PERIOD);
        assert_eq!(driven.ticks.try_recv(), Err(TryRecvError::Empty));
        driven.let_the_period_pass();
        driven.tick_comes();
    }

    #[test]
    fn an_active_keep_alive_ticks_once_per_period() {
        let driven = Driven::new();
        driven.keep_alive.set_active(true);

        for _ in 0..3 {
            driven.period_begins();
            driven.let_the_period_pass();
            driven.tick_comes();
        }
        // The thread ticks before it waits again, so a fourth period that
        // began shows that no other tick came.
        driven.period_begins();
        assert_eq!(driven.ticks.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn a_keep_alive_switched_off_during_a_period_does_not_tick() {
        let driven = Driven::new();
        driven.keep_alive.set_active(true);
        driven.period_begins();

        driven.keep_alive.set_active(false);
        driven.let_the_period_pass();
        driven.keep_alive.set_active(true);

        // The next period began, so the one switched off ended without a
        // tick.
        driven.period_begins();
        assert_eq!(driven.ticks.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn a_tick_nobody_takes_ends_the_keep_alive() {
        let driven = Driven::new();
        driven.listening.store(false, Ordering::SeqCst);
        driven.keep_alive.set_active(true);
        driven.period_begins();

        driven.let_the_period_pass();

        driven.tick_comes();
        // The thread ended and dropped its clock.
        assert_eq!(
            driven.waits.recv_timeout(PATIENCE),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn dropping_the_keep_alive_ends_its_thread() {
        let Driven {
            keep_alive, waits, ..
        } = Driven::new();

        drop(keep_alive);

        assert_eq!(
            waits.recv_timeout(PATIENCE),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn inside_tmux_the_client_s_terminal_decides_and_only_one_client() {
        assert_eq!(inside_tmux("ghostty 1.2.0", 2), Off, "two clients");
        assert_eq!(inside_tmux("ghostty 1.2.0", 0), Off, "no client");
        let env = TerminalEnv::from_pairs(&[
            ("TMUX", "/tmp/tmux-501/default,1,0"),
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.2.0"),
        ]);
        assert_eq!(progress_terminal(&env, None), Off, "tmux did not answer");
    }
}
