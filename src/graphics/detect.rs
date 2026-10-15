// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finding out at start what the terminal can show (spec section 7.7):
//! whether to ask it for its graphics protocol at all, what to do with
//! tmux's `allow-passthrough` around the query and for the run, and the
//! answers that may still arrive after it. The binary runs the query and
//! the tmux commands; everything they decide on is here.

#[cfg(unix)]
use std::io::{self, Write};
#[cfg(unix)]
use std::time::{Duration, Instant};

use super::{Graphics, Passthrough, Protocol};
#[cfg(unix)]
use crate::terminal_env::passthrough;
use crate::terminal_env::{TerminalEnv, TmuxState};

// ---------------------------------------------------------------------
// Whether to ask
// ---------------------------------------------------------------------

/// Whether the terminal may be asked for its graphics protocol, in the
/// terminal `env` names, where `tmux` is what tmux said about asqr's pane
/// if asqr runs in one and tmux answered. Of that, this needs the pane's
/// passthrough option, the number of clients attached to the session and
/// whether a client shows the pane.
///
/// Inside tmux the query only reaches a terminal through passthrough, and
/// only while a client is attached. With `on`, tmux passes sequences only
/// from a pane a client shows, and ratatui-image sets `on` for the pane
/// before it queries, also where the pane had `all`. A query that gets no
/// answer holds up the start for its whole wait, and an answer later than
/// that may reach asqr as typed keys ([`answers_may_follow_the_query`]),
/// so where none can come the query is skipped and images fall back to
/// half blocks (spec section 7.7).
///
/// A terminal that names tmux outside a tmux pane means a tmux that asqr
/// cannot ask, as after ssh from a tmux pane. ratatui-image wraps its
/// query for tmux on exactly this test. There `probe` finds out whether a
/// sequence sent through tmux's passthrough is answered, as
/// [`passthrough_answers`] does, and the terminal is queried only when it
/// is. A `screen` `TERM` gets the query unwrapped and without a probe.
pub fn may_query_protocol(
    env: &TerminalEnv,
    tmux: Option<&TmuxState>,
    probe: impl FnOnce() -> bool,
) -> bool {
    if !env.in_tmux_pane() {
        return !env.tmux_like_term() || probe();
    }
    let Some(state) = tmux else {
        return false;
    };
    let attached = state.clients().is_some_and(|clients| clients > 0);
    let passes = matches!(state.passthrough(), Some("on" | "all"));
    attached && passes && state.shown() == Some(true)
}

// ---------------------------------------------------------------------
// The pane's passthrough
// ---------------------------------------------------------------------

/// Whether asqr sets `allow-passthrough all` for its pane for the run:
/// under the Kitty protocol with its commands wrapped for tmux, inside a
/// tmux 3.4 or later whose pane has no `all` yet, as `tmux` says (spec
/// section 7.7, ADR 44). Transfers and deletes then reach the terminal
/// from a hidden pane too. Kitty images are drawn through placeholder
/// cells, which only the pane's own content places. Sixel and iTerm2
/// images are written where the cursor is, and from a hidden pane tmux
/// would paint them over the window that shows, so they stay at the
/// pane's own setting.
pub fn passthrough_all_for_images(graphics: &Graphics, tmux: Option<&TmuxState>) -> bool {
    let Some(state) = tmux else {
        return false;
    };
    graphics.protocol == Protocol::Kitty
        && graphics.tmux_wrapped
        && state.passthrough() != Some("all")
        && state.knows_passthrough_all()
}

/// The passthrough that holds for the run, once the pane's setting is
/// settled: `all` where asqr set it (`set_all`) or found it, `on` as
/// tmux's answer says, and none outside tmux.
///
/// A terminal that names tmux outside a tmux pane, as after ssh from one,
/// is a tmux asqr cannot ask. Its pane passes sequences at least while it
/// shows, where the query was answered at all, and may drop them while it
/// is hidden, as `on` does.
pub fn settle_passthrough(
    env: &TerminalEnv,
    tmux: Option<&TmuxState>,
    set_all: bool,
) -> Passthrough {
    if set_all {
        return Passthrough::Always;
    }
    if !env.in_tmux_pane() {
        return if env.tmux_like_term() {
            Passthrough::WhileVisible
        } else {
            Passthrough::None
        };
    }
    match tmux.and_then(TmuxState::passthrough) {
        Some("all") => Passthrough::Always,
        Some("on") => Passthrough::WhileVisible,
        _ => Passthrough::None,
    }
}

/// Puts the pane's `allow-passthrough` back as it was before the `Picker`
/// was built: at once with [`PanePassthrough::restore`], or when dropped
/// after [`PanePassthrough::allow_all`] set `all` for the run. `tmux`
/// runs tmux with the arguments it gets and returns what it printed, or
/// `None` when it cannot run or fails.
///
/// ratatui-image 12.0.0-rc.2 runs `tmux set -p allow-passthrough on` in every
/// `Picker` constructor in a terminal that names tmux, which would
/// override the person's own setting for the pane on every start. The
/// same test decides here whether to take a snapshot, so this acts exactly
/// where the library does. A tmux that cannot show the option, with no
/// server or before 3.3, gets no snapshot and is left alone.
pub struct PanePassthrough<R: FnMut(&[&str]) -> Option<String>> {
    tmux: R,
    /// The pane's value before, `Some(None)` when the pane had none of its
    /// own and inherited it; `None` without a snapshot.
    before: Option<Option<String>>,
}

impl<R: FnMut(&[&str]) -> Option<String>> PanePassthrough<R> {
    /// Takes the snapshot, where `env` names a terminal ratatui-image
    /// takes for tmux.
    pub fn snapshot(env: &TerminalEnv, mut tmux: R) -> Self {
        let before = if env.tmux_like_term() {
            tmux(&["show-options", "-p", "-v", "allow-passthrough"]).map(|shown| {
                let value = shown.trim();
                (!value.is_empty()).then(|| value.to_owned())
            })
        } else {
            None
        };
        Self { tmux, before }
    }

    /// Sets `allow-passthrough all` for the pane until the drop restores
    /// it, and whether it did. Without a snapshot there is nothing to
    /// restore, and the pane is left as it is.
    pub fn allow_all(&mut self) -> bool {
        if self.before.is_none() {
            return false;
        }
        _ = (self.tmux)(&["set", "-p", "allow-passthrough", "all"]);
        true
    }

    /// Puts the pane's value back now; the drop then does nothing more.
    pub fn restore(&mut self) {
        // Unsetting lets the pane inherit from its window, session or the
        // global options again, as before.
        match self.before.take() {
            None => {}
            Some(None) => _ = (self.tmux)(&["set", "-pu", "allow-passthrough"]),
            Some(Some(value)) => _ = (self.tmux)(&["set", "-p", "allow-passthrough", &value]),
        }
    }
}

impl<R: FnMut(&[&str]) -> Option<String>> Drop for PanePassthrough<R> {
    fn drop(&mut self) {
        self.restore();
    }
}

// ---------------------------------------------------------------------
// Reading what the terminal answers
// ---------------------------------------------------------------------

/// What [`wait_for_input`] found.
#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
enum Ready {
    /// A byte can be read without blocking, or the input ended and a read
    /// returns at once with nothing.
    Input,
    /// The wait passed without either.
    Timeout,
    /// A signal, such as a resize, ended the wait early. Each caller
    /// decides what of its time is left.
    Interrupted,
}

/// Waits up to `wait` with `poll` for `input` to be readable, so no read
/// is left blocked when nothing comes.
#[cfg(unix)]
fn wait_for_input(input: impl std::os::fd::AsFd, wait: Duration) -> io::Result<Ready> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};

    let wait = Timespec::try_from(wait).map_err(io::Error::other)?;
    let mut fds = [PollFd::new(&input, PollFlags::IN)];
    match poll(&mut fds, Some(&wait)) {
        Ok(0) => Ok(Ready::Timeout),
        Ok(_) => Ok(Ready::Input),
        Err(rustix::io::Errno::INTR) => Ok(Ready::Interrupted),
        Err(error) => Err(error.into()),
    }
}

/// The answer to the device status report request `ESC [5n` that
/// [`passthrough_answers`] sends: the terminal is fine.
#[cfg(unix)]
const PROBE_ANSWER: &[u8] = b"\x1b[0n";

/// Writes a device status report request, `ESC [5n`, wrapped for tmux's
/// passthrough to `out`, and whether `input` brings the answer within
/// `patience`. The terminal outside answers only when the pane passes the
/// request through to it. It reads a byte at a time only once one is
/// there, so the bytes after the answer stay for the next reader. Bytes
/// before the answer, such as keys typed in the meantime, are lost.
/// `input` must be in raw mode, or a terminal hands the answer over only
/// with a line.
#[cfg(unix)]
pub fn passthrough_answers(
    out: &mut impl Write,
    input: impl std::os::fd::AsFd,
    patience: Duration,
) -> io::Result<bool> {
    out.write_all(&passthrough(b"\x1b[5n"))?;
    out.flush()?;
    let deadline = Instant::now() + patience;
    let mut seen = Vec::new();
    while !seen.ends_with(PROBE_ANSWER) {
        // Input that keeps coming, such as a paste, or signals that keep
        // ending the wait must not keep the probe reading past its
        // deadline.
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        match wait_for_input(&input, left)? {
            Ready::Input => {}
            Ready::Timeout => return Ok(false),
            Ready::Interrupted => continue,
        }
        let mut byte = [0];
        if rustix::io::read(&input, &mut byte)? == 0 {
            return Ok(false);
        }
        seen.push(byte[0]);
    }
    Ok(true)
}

/// Whether answers to the graphics query may still be on their way once
/// ratatui-image is done reading, so [`discard_late_answers`] has to run
/// before keys are read. ratatui-image reads up to the first status
/// report or its deadline; whatever comes after it reaches the key reader,
/// which takes a Kitty answer such as `ESC _Gi=31;OK ESC \` for keys, and
/// its digits pick options.
///
/// `found_capabilities` is whether the picker found any capability. A
/// picker without one got no usable answer, which every query that ran
/// into its deadline is, and its terminal may still answer. Inside tmux,
/// with more than one client attached, as `tmux` says, each client's
/// terminal answers the query tmux passed on, and only the first answer
/// was read. A lone terminal that answered sends nothing more, and its
/// start costs no wait.
pub fn answers_may_follow_the_query(found_capabilities: bool, tmux: Option<&TmuxState>) -> bool {
    let clients = tmux.and_then(TmuxState::clients).unwrap_or(0);
    !found_capabilities || clients > 1
}

/// Reads and discards what `input` brings until `quiet` passes without a
/// byte, or `cap` passes in all, and returns how many bytes it discarded.
/// It reads only what is there, so it never blocks in a read. A key typed
/// while it runs is discarded with the answers.
#[cfg(unix)]
pub fn discard_late_answers(
    input: impl std::os::fd::AsFd,
    quiet: Duration,
    cap: Duration,
) -> io::Result<usize> {
    let deadline = Instant::now() + cap;
    let mut discarded = 0;
    loop {
        let wait = quiet.min(deadline.saturating_duration_since(Instant::now()));
        if wait.is_zero() {
            return Ok(discarded);
        }
        match wait_for_input(&input, wait)? {
            Ready::Input => {}
            Ready::Timeout => return Ok(discarded),
            // The quiet time starts again; the cap still holds.
            Ready::Interrupted => continue,
        }
        let mut chunk = [0; 256];
        match rustix::io::read(&input, &mut chunk)? {
            0 => return Ok(discarded),
            read => discarded += read,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // Whether to ask
    // -----------------------------------------------------------------

    /// The variables of a terminal that is not tmux.
    const PLAIN: &[(&str, &str)] = &[("TERM", "xterm-ghostty"), ("TERM_PROGRAM", "ghostty")];

    /// `PLAIN` in a tmux pane.
    const IN_PANE: &[(&str, &str)] = &[
        ("TMUX", "/tmp/tmux-501/default,1234,0"),
        ("TERM", "xterm-ghostty"),
        ("TERM_PROGRAM", "ghostty"),
    ];

    /// The variables of a tmux that asqr cannot ask without `TMUX`, as
    /// after ssh from a tmux pane, which carries `TERM` along but not
    /// `TMUX`.
    const MULTIPLEXED: &[(&str, &str)] = &[("TERM", "tmux-256color")];

    /// The fields after the client's terminal in what tmux prints for
    /// [`TmuxState::FORMAT`]: whether the pane's window is the current one
    /// of its session, whether the pane is the window's current one,
    /// whether the window is zoomed, and tmux's version.
    const SHOWN: &str = "1\t1\t0\t3.5";

    /// What tmux prints for [`TmuxState::FORMAT`] with passthrough
    /// `passthrough`, `clients` attached, no answer to XTVERSION and the
    /// pane shown as `shown` says.
    fn state(passthrough: &str, clients: &str, shown: &str) -> Option<String> {
        Some(format!("{passthrough}\t{clients}\t\t{shown}\n"))
    }

    /// The decision in the environment `vars`, where tmux said `state`
    /// about the pane and the probe never runs.
    fn may_query(vars: &[(&str, &str)], state: Option<String>) -> bool {
        let state = state.as_deref().map(TmuxState::parse);
        may_query_protocol(&TerminalEnv::from_pairs(vars), state.as_ref(), || {
            unreachable!("no probe where tmux can be asked or is not there")
        })
    }

    /// The decision in the environment `vars` without a tmux pane, where
    /// the probe gets the `answered` it is given, if it runs.
    fn probed(vars: &[(&str, &str)], answered: bool) -> bool {
        may_query_protocol(&TerminalEnv::from_pairs(vars), None, || answered)
    }

    #[test]
    fn outside_tmux_the_terminal_is_asked() {
        assert!(may_query(PLAIN, None));
        assert!(may_query(&[], None));
    }

    /// ratatui-image sends the query unwrapped there, and GNU screen, or a
    /// tmux reached over ssh with such a `TERM`, answers it, so a probe
    /// would only cost time and keys.
    #[test]
    fn a_screen_term_without_tmux_is_asked_without_a_probe() {
        assert!(may_query(&[("TERM", "screen-256color")], None));
    }

    #[test]
    fn a_tmux_without_its_variable_is_asked_once_the_probe_is_answered() {
        assert!(probed(MULTIPLEXED, true));
    }

    #[test]
    fn a_tmux_without_its_variable_is_not_asked_without_an_answer() {
        assert!(!probed(MULTIPLEXED, false));
    }

    #[test]
    fn inside_tmux_only_with_passthrough_and_a_client() {
        assert!(may_query(IN_PANE, state("on", "1", SHOWN)));
        assert!(may_query(IN_PANE, state("all", "2", SHOWN)));
        assert!(!may_query(IN_PANE, state("off", "1", SHOWN)));
        assert!(!may_query(IN_PANE, state("on", "0", SHOWN)), "detached");
        let tmux_term = [
            ("TMUX", "/tmp/tmux-501/default,1234,0"),
            ("TERM", "tmux-256color"),
        ];
        assert!(
            may_query(&tmux_term, state("on", "1", SHOWN)),
            "the pane decides, whatever TERM says"
        );
    }

    /// With `on`, tmux passes nothing through from a pane no client
    /// shows, and the query would wait in vain. A pane set to `all` gets
    /// `on` from ratatui-image for the query, so it has to show too.
    #[test]
    fn inside_tmux_only_from_a_pane_that_shows() {
        let hidden = "0\t1\t0\t3.5";
        assert!(!may_query(IN_PANE, state("on", "1", hidden)));
        assert!(!may_query(IN_PANE, state("all", "1", hidden)));
    }

    #[test]
    fn inside_tmux_anything_unexpected_counts_as_no() {
        assert!(!may_query(IN_PANE, None));
        assert!(!may_query(IN_PANE, Some(String::new())));
        assert!(!may_query(IN_PANE, Some("on".into())));
        assert!(!may_query(IN_PANE, state("on", "many", SHOWN)));
        assert!(
            !may_query(IN_PANE, Some("on 1\n".into())),
            "blanks do not separate the fields"
        );
        assert!(
            !may_query(IN_PANE, Some("on\t1\t\n".into())),
            "with on, a pane not known to show"
        );
    }

    // -----------------------------------------------------------------
    // The pane's passthrough
    // -----------------------------------------------------------------

    /// Under the Kitty protocol inside tmux 3.4 or later, asqr's pane gets
    /// `all`, so transfers and deletes pass from a hidden pane too. Sixel
    /// and iTerm2 images are written where the cursor is, and from a
    /// hidden pane they would paint over the window that shows.
    #[test]
    fn kitty_images_inside_tmux_from_3_4_get_passthrough_all() {
        let kitty = Graphics {
            protocol: Protocol::Kitty,
            tmux_wrapped: true,
            ..Graphics::HALF_BLOCKS
        };
        let all = |graphics: Graphics, state: Option<String>| {
            let state = state.as_deref().map(TmuxState::parse);
            passthrough_all_for_images(&graphics, state.as_ref())
        };
        assert!(all(kitty, state("on", "1", SHOWN)));
        assert!(!all(kitty, state("on", "1", "1\t1\t0\t3.3a")), "before 3.4");
        for protocol in [Protocol::HalfBlocks, Protocol::Sixel, Protocol::Iterm2] {
            let graphics = Graphics { protocol, ..kitty };
            assert!(!all(graphics, state("on", "1", SHOWN)), "{protocol:?}");
        }
        let unwrapped = Graphics {
            tmux_wrapped: false,
            ..kitty
        };
        assert!(
            !all(unwrapped, state("on", "1", SHOWN)),
            "Kitty commands not wrapped for tmux"
        );
        assert!(!all(kitty, state("all", "1", SHOWN)), "all already");
        assert!(!all(kitty, None), "tmux not asked");
    }

    #[test]
    fn the_passthrough_for_the_run_is_the_one_the_pane_has_at_last() {
        let settled = |vars: &[(&str, &str)], state: Option<String>, set_all: bool| {
            let state = state.as_deref().map(TmuxState::parse);
            settle_passthrough(&TerminalEnv::from_pairs(vars), state.as_ref(), set_all)
        };

        assert_eq!(
            settled(IN_PANE, state("on", "1", SHOWN), true),
            Passthrough::Always,
            "set by asqr"
        );
        assert_eq!(
            settled(IN_PANE, state("all", "1", SHOWN), false),
            Passthrough::Always,
            "found"
        );
        assert_eq!(
            settled(IN_PANE, state("on", "1", SHOWN), false),
            Passthrough::WhileVisible
        );
        assert_eq!(
            settled(IN_PANE, state("off", "1", SHOWN), false),
            Passthrough::None
        );
        assert_eq!(settled(IN_PANE, None, false), Passthrough::None);
        assert_eq!(settled(PLAIN, None, false), Passthrough::None);
        assert_eq!(
            settled(MULTIPLEXED, None, false),
            Passthrough::WhileVisible,
            "a tmux asqr cannot ask"
        );
    }

    /// What the run does with a [`PanePassthrough`] between its snapshot
    /// and its drop.
    #[derive(Clone, Copy)]
    enum Between {
        Nothing,
        AllowAll,
        Restore,
    }

    const SHOW: &str = "show-options -p -v allow-passthrough";

    /// The tmux commands a [`PanePassthrough`] runs from its snapshot to
    /// its drop in the environment `vars`, where `show-options` answers
    /// `shown`, and whether `allow_all` said it set `all`.
    fn passthrough_commands_with(
        vars: &[(&str, &str)],
        shown: Option<&str>,
        between: Between,
    ) -> (Vec<String>, bool) {
        let commands = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = std::rc::Rc::clone(&commands);
        let mut pane =
            PanePassthrough::snapshot(&TerminalEnv::from_pairs(vars), move |args: &[&str]| {
                recorded.borrow_mut().push(args.join(" "));
                if args[0] == "show-options" {
                    shown.map(str::to_owned)
                } else {
                    Some(String::new())
                }
            });
        let set_all = match between {
            Between::Nothing => false,
            Between::AllowAll => pane.allow_all(),
            Between::Restore => {
                pane.restore();
                false
            }
        };
        drop(pane);
        (commands.take(), set_all)
    }

    /// The tmux commands a [`PanePassthrough`] runs from its snapshot to
    /// its drop with nothing in between.
    fn passthrough_commands(vars: &[(&str, &str)], shown: Option<&str>) -> Vec<String> {
        passthrough_commands_with(vars, shown, Between::Nothing).0
    }

    /// Under the Kitty protocol inside tmux 3.4 or later, the pane gets
    /// `all` for the run and its own value back at the end.
    #[test]
    fn a_pane_set_to_all_gets_its_own_value_back() {
        assert_eq!(
            passthrough_commands_with(MULTIPLEXED, Some(""), Between::AllowAll),
            (
                vec![
                    SHOW.to_owned(),
                    "set -p allow-passthrough all".to_owned(),
                    "set -pu allow-passthrough".to_owned()
                ],
                true
            )
        );
        assert_eq!(
            passthrough_commands_with(MULTIPLEXED, Some("on\n"), Between::AllowAll).0,
            [
                SHOW,
                "set -p allow-passthrough all",
                "set -p allow-passthrough on"
            ]
        );
    }

    /// Without a value to put back, asqr does not set `all` either.
    #[test]
    fn a_pane_without_a_snapshot_is_not_set_to_all() {
        assert_eq!(
            passthrough_commands_with(MULTIPLEXED, None, Between::AllowAll),
            (vec![SHOW.to_owned()], false)
        );
        assert_eq!(
            passthrough_commands_with(PLAIN, Some(""), Between::AllowAll),
            (Vec::new(), false)
        );
    }

    /// Restored once the picker is built, the pane is not restored again
    /// when the guard goes.
    #[test]
    fn a_restored_pane_is_restored_once() {
        assert_eq!(
            passthrough_commands_with(MULTIPLEXED, Some("off"), Between::Restore).0,
            [SHOW, "set -p allow-passthrough off"]
        );
    }

    #[test]
    fn a_pane_without_its_own_passthrough_gets_it_unset_again() {
        assert_eq!(
            passthrough_commands(MULTIPLEXED, Some("")),
            [SHOW, "set -pu allow-passthrough"]
        );
    }

    #[test]
    fn a_pane_with_its_own_passthrough_gets_that_value_back() {
        assert_eq!(
            passthrough_commands(
                &[("TERM", "xterm-256color"), ("TERM_PROGRAM", "tmux")],
                Some("off\n")
            ),
            [SHOW, "set -p allow-passthrough off"]
        );
    }

    #[test]
    fn a_tmux_that_cannot_show_the_option_is_left_alone() {
        assert_eq!(passthrough_commands(MULTIPLEXED, None), [SHOW]);
    }

    #[test]
    fn outside_what_ratatui_image_takes_for_tmux_tmux_is_not_run() {
        assert_eq!(
            passthrough_commands(&[("TERM", "screen-256color")], Some("off")),
            Vec::<String>::new()
        );
    }

    // -----------------------------------------------------------------
    // Reading what the terminal answers
    // -----------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn the_probe_asks_through_tmux_passthrough_and_reads_only_up_to_the_answer() {
        use std::io::Read;

        let (mut input, mut terminal) = std::io::pipe().expect("a pipe");
        terminal
            .write_all(b"typed\x1b[0nafter")
            .expect("the pipe takes it");
        drop(terminal);
        let mut sent = Vec::new();

        let answered = passthrough_answers(&mut sent, &input, Duration::from_secs(5));

        assert!(answered.expect("the probe runs"));
        assert_eq!(sent, b"\x1bPtmux;\x1b\x1b[5n\x1b\\");
        let mut rest = String::new();
        input.read_to_string(&mut rest).expect("the pipe reads");
        assert_eq!(rest, "after", "what comes after the answer stays");
    }

    #[cfg(unix)]
    #[test]
    fn the_probe_gives_up_without_an_answer() {
        let (input, mut terminal) = std::io::pipe().expect("a pipe");
        terminal.write_all(b"typed").expect("the pipe takes it");

        let answered = passthrough_answers(&mut Vec::new(), &input, Duration::from_millis(50));
        assert!(!answered.expect("the probe runs"), "silence");

        drop(terminal);
        let answered = passthrough_answers(&mut Vec::new(), &input, Duration::from_secs(5));
        assert!(!answered.expect("the probe runs"), "the input ended");
    }

    #[cfg(unix)]
    #[test]
    fn the_probe_reads_nothing_past_its_deadline() {
        use std::io::Read;

        let (mut input, mut terminal) = std::io::pipe().expect("a pipe");
        terminal.write_all(b"\x1b[0n").expect("the pipe takes it");
        drop(terminal);

        let answered = passthrough_answers(&mut Vec::new(), &input, Duration::ZERO);

        assert!(!answered.expect("the probe runs"));
        let mut rest = Vec::new();
        input.read_to_end(&mut rest).expect("the pipe reads");
        assert_eq!(rest, b"\x1b[0n", "the answer stays unread");
    }

    /// A terminal's answer to the graphics query, as kitty gives it.
    #[cfg(unix)]
    const KITTY_ANSWER: &[u8] = b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n";

    /// Writes each of `bursts` to a new pipe after its delay, counted
    /// from the one before, on a thread of its own. Returns the pipe's
    /// reading end and the thread, which closes the writing end when done.
    #[cfg(unix)]
    fn answering(
        bursts: Vec<(Duration, &'static [u8])>,
    ) -> (std::io::PipeReader, std::thread::JoinHandle<()>) {
        let (input, mut terminal) = std::io::pipe().expect("a pipe");
        let writer = std::thread::spawn(move || {
            for (delay, burst) in bursts {
                std::thread::sleep(delay);
                terminal.write_all(burst).expect("the pipe takes it");
            }
        });
        (input, writer)
    }

    /// What is left in `input` once its writer is done.
    #[cfg(unix)]
    fn left_over(mut input: std::io::PipeReader, writer: std::thread::JoinHandle<()>) -> Vec<u8> {
        use std::io::Read;

        writer.join().expect("the writer finishes");
        let mut rest = Vec::new();
        input.read_to_end(&mut rest).expect("the pipe reads");
        rest
    }

    /// With several tmux clients showing the pane, every client's terminal
    /// answers the query. The query reads up to the first status report,
    /// and the answers of the other two follow it, one after the other.
    #[cfg(unix)]
    #[test]
    fn the_answers_of_further_clients_are_discarded() {
        let (input, writer) = answering(vec![
            (Duration::ZERO, KITTY_ANSWER),
            (Duration::from_millis(50), KITTY_ANSWER),
        ]);

        let discarded =
            discard_late_answers(&input, Duration::from_millis(400), Duration::from_secs(5));

        assert_eq!(discarded.expect("the pipe reads"), 2 * KITTY_ANSWER.len());
        assert_eq!(left_over(input, writer), b"");
    }

    /// An answer that comes after the query gave up is discarded while it
    /// comes within the quiet time; a key typed after that stays.
    #[cfg(unix)]
    #[test]
    fn an_answer_after_the_query_gave_up_is_discarded_and_later_keys_stay() {
        let (input, writer) = answering(vec![
            (Duration::from_millis(100), KITTY_ANSWER),
            (Duration::from_millis(1500), b"q"),
        ]);

        let discarded =
            discard_late_answers(&input, Duration::from_millis(400), Duration::from_secs(5));

        assert_eq!(discarded.expect("the pipe reads"), KITTY_ANSWER.len());
        assert_eq!(left_over(input, writer), b"q");
    }

    /// Input that never pauses, such as a long paste, ends the discarding
    /// at the cap, and so do a closed input and one that never brings a
    /// byte.
    #[cfg(unix)]
    #[test]
    fn discarding_ends_at_its_cap_at_the_end_of_input_and_without_input() {
        let (input, writer) = answering(vec![(Duration::from_millis(20), b"x"); 100]);
        let started = Instant::now();
        let discarded = discard_late_answers(
            &input,
            Duration::from_millis(400),
            Duration::from_millis(300),
        );
        assert!(discarded.expect("the pipe reads") > 0);
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
        assert!(!left_over(input, writer).is_empty(), "the rest stays");

        let (input, terminal) = std::io::pipe().expect("a pipe");
        drop(terminal);
        let discarded =
            discard_late_answers(&input, Duration::from_secs(5), Duration::from_secs(5));
        assert_eq!(discarded.expect("the pipe reads"), 0, "the input ended");

        let (input, mut terminal) = std::io::pipe().expect("a pipe");
        terminal.write_all(b"q").expect("the pipe takes it");
        let discarded = discard_late_answers(&input, Duration::from_secs(5), Duration::ZERO);
        assert_eq!(discarded.expect("the pipe reads"), 0, "no time at all");
        let mut left = [0];
        rustix::io::read(&input, &mut left).expect("the pipe reads");
        assert_eq!(&left, b"q", "what came stays");

        let started = Instant::now();
        let discarded =
            discard_late_answers(&input, Duration::from_millis(50), Duration::from_secs(5));
        assert_eq!(discarded.expect("the pipe reads"), 0, "silence");
        assert!(started.elapsed() >= Duration::from_millis(50));
    }

    /// The wait reports each of its three outcomes.
    #[cfg(unix)]
    #[test]
    fn the_wait_tells_input_from_silence() {
        let (input, mut terminal) = std::io::pipe().expect("a pipe");
        assert_eq!(
            wait_for_input(&input, Duration::from_millis(10)).expect("polls"),
            Ready::Timeout
        );
        terminal.write_all(b"q").expect("the pipe takes it");
        assert_eq!(
            wait_for_input(&input, Duration::from_secs(5)).expect("polls"),
            Ready::Input
        );
    }

    /// A terminal that gave no usable answer may still send one after the
    /// query's deadline, and every further tmux client's terminal sends
    /// one of its own. A lone terminal that answered sends nothing more.
    #[test]
    fn answers_may_follow_without_capabilities_or_with_several_clients() {
        let follow = |found: bool, state: Option<&str>| {
            answers_may_follow_the_query(found, state.map(TmuxState::parse).as_ref())
        };
        assert!(follow(false, None));
        assert!(follow(false, Some("on\t1\t\n")));
        assert!(follow(true, Some("all\t2\t\n")));
        assert!(!follow(true, None));
        assert!(!follow(true, Some("on\t1\tkitty\n")));
        assert!(!follow(true, Some("on\tmany\t\n")));
    }
}
