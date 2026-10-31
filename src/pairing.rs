// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pairing asqr with the agents of the same terminal session (ADR 85).
//! Inside tmux, the pane's tmux session decides the queue: the queue the
//! session's asqr pinned, else one named after the session. Outside tmux
//! nothing pairs; other multiplexers would be variants of
//! [`Multiplexer`].
//!
//! tmux is reached through [`Tmux`], so tests answer for it and never ask
//! the tmux they may run in.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::format::MAX_SESSION_ID_BYTES;
use crate::queue::{Holder, Paired};

/// The tmux session variable that holds the queue the session's asqr
/// watches. It is read from tmux only, never from the process
/// environment, where a copy inherited before a change would be stale.
pub const SESSION_QUEUE: &str = "ASQR_SESSION_QUEUE";

/// How long tmux gets to answer. A lookup takes about 5 ms, so a tmux
/// that does not answer within a hundred times that is taken as gone,
/// and no command waits on a hung tmux longer.
pub const TMUX_TIMEOUT: Duration = Duration::from_millis(500);

/// The variable through which a debug build takes another timeout than
/// [`TMUX_TIMEOUT`], in milliseconds. The tests' stand-in `tmux` is a
/// script that took 650 ms to start beside a second test run, longer than
/// tmux gets in use; the tests set the variable so the stand-in is not
/// given up on.
pub const TIMEOUT_VARIABLE: &str = "ASQR_TEST_TMUX_TIMEOUT_MS";

/// The longest timeout the variable may set. Beyond what `Instant` can add
/// to now, the wait would panic; ten minutes is far past any test.
const LONGEST_TIMEOUT_MS: u64 = 600_000;

/// How long tmux gets to answer given the value of [`TIMEOUT_VARIABLE`]:
/// the milliseconds it holds in a debug build, and [`TMUX_TIMEOUT`] when
/// it is unset, not a positive number of milliseconds, or in a release
/// build.
fn timeout_from(variable: Option<&OsStr>, debug_build: bool) -> Duration {
    if !debug_build {
        return TMUX_TIMEOUT;
    }
    variable
        .and_then(OsStr::to_str)
        .and_then(|milliseconds| milliseconds.parse::<u64>().ok())
        .filter(|milliseconds| *milliseconds > 0)
        .map_or(TMUX_TIMEOUT, |milliseconds| {
            Duration::from_millis(milliseconds.min(LONGEST_TIMEOUT_MS))
        })
}

/// Where this process runs, as far as pairing goes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Multiplexer {
    /// No multiplexer asqr pairs through.
    #[default]
    None,
    Tmux(TmuxPane),
}

/// What `TMUX` and `TMUX_PANE` say about the tmux pane of this process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPane {
    /// The server's socket, the first field of `TMUX`.
    pub socket: String,
    /// The pane, such as `%3`. Without it tmux would answer for the most
    /// recently active session, which may be another one.
    pub pane: Option<String>,
}

impl Multiplexer {
    /// The multiplexer the variables `TMUX` and `TMUX_PANE` name; empty
    /// ones count as unset.
    pub fn from_variables(tmux: Option<&str>, pane: Option<&str>) -> Self {
        match tmux.filter(|tmux| !tmux.is_empty()) {
            None => Multiplexer::None,
            Some(tmux) => Multiplexer::Tmux(TmuxPane {
                socket: tmux.split(',').next().unwrap_or_default().to_owned(),
                pane: pane.filter(|pane| !pane.is_empty()).map(str::to_owned),
            }),
        }
    }
}

/// Why tmux gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxFailure(pub String);

/// Runs a tmux command and returns what it printed.
pub trait Tmux {
    /// Runs `tmux` with `args`, on the server at `socket` where given.
    fn run(&self, socket: Option<&str>, args: &[&str]) -> Result<String, TmuxFailure>;
}

/// The `tmux` on `PATH`, given [`TMUX_TIMEOUT`] to answer.
#[derive(Debug, Clone)]
pub struct SystemTmux {
    program: PathBuf,
    timeout: Duration,
}

impl Default for SystemTmux {
    fn default() -> Self {
        SystemTmux {
            program: PathBuf::from("tmux"),
            timeout: timeout_from(
                std::env::var_os(TIMEOUT_VARIABLE).as_deref(),
                cfg!(debug_assertions),
            ),
        }
    }
}

impl SystemTmux {
    /// `program` in place of `tmux`, given `timeout`, for the tests of
    /// what a missing, failing or hung tmux does.
    #[cfg(test)]
    fn at(program: impl Into<PathBuf>, timeout: Duration) -> Self {
        SystemTmux {
            program: program.into(),
            timeout,
        }
    }
}

impl Tmux for SystemTmux {
    fn run(&self, socket: Option<&str>, args: &[&str]) -> Result<String, TmuxFailure> {
        let mut command = Command::new(&self.program);
        if let Some(socket) = socket {
            command.arg("-S").arg(socket);
        }
        let mut child = command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => TmuxFailure("tmux is not installed".into()),
                _ => TmuxFailure(format!("cannot run tmux: {error}")),
            })?;
        // The pipes are read on threads of their own, so a tmux that
        // prints more than a pipe holds does not stop before it exits.
        let read = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut bytes);
                }
                bytes
            })
        };
        let stdout = read(child.stdout.take().map(|pipe| Box::new(pipe) as _));
        let stderr = read(child.stderr.take().map(|pipe| Box::new(pipe) as _));

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                // Gone past the deadline, or the wait failed: either way no
                // answer comes, and the child is not left behind.
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(TmuxFailure(format!(
                        "tmux did not answer within {} ms",
                        self.timeout.as_millis()
                    )));
                }
            }
        };
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        if status.success() {
            return Ok(String::from_utf8_lossy(&stdout).into_owned());
        }
        let message = String::from_utf8_lossy(&stderr);
        let first = message.lines().next().unwrap_or_default();
        Err(TmuxFailure(match status.code() {
            Some(code) => format!("tmux exited with {code}: {first}"),
            None => format!("tmux was ended by a signal: {first}"),
        }))
    }
}

/// What a tmux session says about its queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSession {
    pub name: String,
    /// The queue its asqr pinned, if any.
    pub pinned: Option<String>,
}

/// Asks tmux for the session of `pane` and its pinned queue.
///
/// One call answers both: the session's name, then every variable of the
/// session as `NAME=value`, a removed one as `-NAME`. Asking for the
/// variable by name would fail the whole call while it is unset.
pub fn ask_session(tmux: &dyn Tmux, pane: &TmuxPane) -> Result<TmuxSession, TmuxFailure> {
    let Some(id) = pane.pane.as_deref() else {
        return Err(TmuxFailure("TMUX_PANE is not set".into()));
    };
    let answer = tmux.run(
        None,
        &[
            "display-message",
            "-p",
            "-t",
            id,
            "#{session_name}",
            ";",
            "show-environment",
            "-t",
            id,
        ],
    )?;
    let mut lines = answer.lines();
    // tmux prints an empty name for a pane that is gone.
    let name = lines
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| TmuxFailure(format!("tmux knows no session of pane {id}")))?;
    let pinned = lines
        .filter_map(|line| line.strip_prefix(SESSION_QUEUE)?.strip_prefix('='))
        .next()
        .map(str::to_owned);
    Ok(TmuxSession {
        name: name.to_owned(),
        pinned,
    })
}

/// The queue named after the tmux session `name`, which tmux allows to
/// hold anything: each run of characters a queue name cannot hold becomes
/// one `-`, a leading `.` becomes `_`, and the name is cut at the longest
/// queue name.
pub fn queue_for_session(name: &str) -> String {
    let mut queue = String::with_capacity(name.len());
    let mut in_run = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
            queue.push(c);
            in_run = false;
        } else if !in_run {
            queue.push('-');
            in_run = true;
        }
    }
    if queue.starts_with('.') {
        queue.replace_range(..1, "_");
    }
    // Only ASCII is left, so any byte is a character boundary.
    queue.truncate(MAX_SESSION_ID_BYTES);
    queue
}

/// Pins `queue` in the tmux session of `pane`, for the agents of the
/// session and every later asqr there (ADR 85).
pub fn pin(tmux: &dyn Tmux, pane: &TmuxPane, queue: &str) -> Result<(), TmuxFailure> {
    let Some(id) = pane.pane.as_deref() else {
        return Err(TmuxFailure("TMUX_PANE is not set".into()));
    };
    tmux.run(None, &["set-environment", "-t", id, SESSION_QUEUE, queue])
        .map(|_| ())
}

/// The session of each pane of the tmux server at `socket`, by pane id.
pub fn sessions_of_panes(
    tmux: &dyn Tmux,
    socket: &str,
) -> Result<HashMap<String, String>, TmuxFailure> {
    let answer = tmux.run(
        Some(socket),
        &["list-panes", "-a", "-F", "#{pane_id} #{session_name}"],
    )?;
    // A pane id has no blank, a session name may.
    Ok(answer
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(pane, session)| (pane.to_owned(), session.to_owned()))
        .collect())
}

/// What the tmux session of `pane` says about its queue, for
/// [`QueueLocation::resolve_paired`](crate::queue::QueueLocation::resolve_paired).
/// A tmux that does not answer leaves asqr as outside tmux; the reason
/// goes into the log.
pub fn paired(tmux: &dyn Tmux, pane: &TmuxPane) -> Paired {
    paired_by(&ask_session(tmux, pane))
}

/// What the answer of [`ask_session`] says about the queue, as
/// [`paired`] gives it, for a caller that keeps the answer too.
pub fn paired_by(answer: &Result<TmuxSession, TmuxFailure>) -> Paired {
    match answer {
        Ok(TmuxSession {
            pinned: Some(pinned),
            ..
        }) => Paired::Pinned(pinned.clone()),
        Ok(TmuxSession { name, .. }) => Paired::Session(queue_for_session(name)),
        Err(TmuxFailure(reason)) => {
            tracing::info!(reason, "tmux did not answer; going on as outside tmux");
            Paired::Unreachable(reason.clone())
        }
    }
}

/// The tmux session of each watcher in `holders` that runs in a tmux
/// pane, by socket and pane: one call to each tmux server, however many
/// of its panes watch a queue. A server that does not answer names none.
pub fn sessions_of_watchers<'a>(
    holders: impl IntoIterator<Item = &'a Holder>,
    tmux: &dyn Tmux,
) -> WatcherSessions {
    let mut sessions = HashMap::new();
    for place in holders
        .into_iter()
        .filter_map(|holder| holder.tmux.as_ref())
    {
        sessions
            .entry(place.socket.clone())
            .or_insert_with(|| sessions_of_panes(tmux, &place.socket).unwrap_or_default());
    }
    WatcherSessions(sessions)
}

/// The tmux sessions [`sessions_of_watchers`] found.
#[derive(Debug, Default)]
pub struct WatcherSessions(HashMap<String, HashMap<String, String>>);

impl WatcherSessions {
    /// The tmux session `holder` runs in, where tmux named one.
    pub fn of(&self, holder: &Holder) -> Option<&str> {
        let place = holder.tmux.as_ref()?;
        self.0
            .get(&place.socket)?
            .get(&place.pane)
            .map(String::as_str)
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! A stand-in tmux for tests.

    use std::cell::RefCell;

    use super::*;

    /// Answers every call with `answer` and keeps the calls.
    pub(crate) struct StandIn {
        pub(crate) answer: Result<String, TmuxFailure>,
        pub(crate) calls: RefCell<Vec<Vec<String>>>,
    }

    impl StandIn {
        pub(crate) fn answering(answer: &str) -> Self {
            StandIn {
                answer: Ok(answer.to_owned()),
                calls: RefCell::new(Vec::new()),
            }
        }

        pub(crate) fn failing(reason: &str) -> Self {
            StandIn {
                answer: Err(TmuxFailure(reason.to_owned())),
                calls: RefCell::new(Vec::new()),
            }
        }

        pub(crate) fn calls(&self) -> Vec<Vec<String>> {
            self.calls.borrow().clone()
        }
    }

    impl Tmux for StandIn {
        fn run(&self, socket: Option<&str>, args: &[&str]) -> Result<String, TmuxFailure> {
            let mut call: Vec<String> = socket
                .map(|socket| vec!["-S".to_owned(), socket.to_owned()])
                .unwrap_or_default();
            call.extend(args.iter().map(|arg| (*arg).to_owned()));
            self.calls.borrow_mut().push(call);
            self.answer.clone()
        }
    }

    /// A stand-in that fails the test when it is asked at all.
    pub(crate) struct Unasked;

    impl Tmux for Unasked {
        fn run(&self, _socket: Option<&str>, args: &[&str]) -> Result<String, TmuxFailure> {
            panic!("tmux was asked: {args:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::StandIn;
    use super::*;

    fn pane(id: &str) -> TmuxPane {
        TmuxPane {
            socket: "/tmp/tmux-501/default".into(),
            pane: Some(id.into()),
        }
    }

    // -----------------------------------------------------------------
    // The variables
    // -----------------------------------------------------------------

    #[test]
    fn tmux_and_its_pane_come_from_the_variables() {
        assert_eq!(
            Multiplexer::from_variables(Some("/tmp/tmux-501/default,3614,4"), Some("%66")),
            Multiplexer::Tmux(pane("%66"))
        );
        assert_eq!(
            Multiplexer::from_variables(Some("/tmp/tmux-501/default,3614,4"), Some("")),
            Multiplexer::Tmux(TmuxPane {
                socket: "/tmp/tmux-501/default".into(),
                pane: None
            })
        );
        assert_eq!(
            Multiplexer::from_variables(Some(""), Some("%66")),
            Multiplexer::None
        );
        assert_eq!(Multiplexer::from_variables(None, None), Multiplexer::None);
    }

    // -----------------------------------------------------------------
    // The queue named after a session
    // -----------------------------------------------------------------

    #[test]
    fn a_session_name_that_is_a_valid_queue_name_stays() {
        assert_eq!(queue_for_session("asqr-todos"), "asqr-todos");
        assert_eq!(queue_for_session("0"), "0");
    }

    #[test]
    fn each_run_of_characters_a_queue_name_cannot_hold_becomes_one_dash() {
        assert_eq!(queue_for_session("my work"), "my-work");
        assert_eq!(queue_for_session("a:b"), "a-b");
        assert_eq!(queue_for_session("Über"), "-ber");
        assert_eq!(queue_for_session("x / y"), "x-y");
    }

    #[test]
    fn a_leading_dot_becomes_an_underscore() {
        assert_eq!(queue_for_session(".hidden"), "_hidden");
    }

    #[test]
    fn a_long_session_name_is_cut_at_the_longest_queue_name() {
        let queue = queue_for_session(&"ä".repeat(150).replace("ä", "äb"));

        assert_eq!(queue.len(), MAX_SESSION_ID_BYTES);
        assert!(crate::format::is_valid_session_id(&queue), "{queue}");
    }

    // -----------------------------------------------------------------
    // Asking tmux
    // -----------------------------------------------------------------

    #[test]
    fn the_session_and_its_pin_come_from_one_tmux_call() {
        let tmux = StandIn::answering(
            "my work\n-DISPLAY\nASQR_SESSION_QUEUE=asqr-todos\nSSH_AUTH_SOCK=/x\n",
        );

        let session = ask_session(&tmux, &pane("%3")).expect("tmux answers");

        assert_eq!(
            session,
            TmuxSession {
                name: "my work".into(),
                pinned: Some("asqr-todos".into())
            }
        );
        // tmux takes the separator of chained commands as an argument of
        // its own; the shell's `\;` would reach it as two characters.
        assert_eq!(
            tmux.calls(),
            [[
                "display-message",
                "-p",
                "-t",
                "%3",
                "#{session_name}",
                ";",
                "show-environment",
                "-t",
                "%3"
            ]]
        );
    }

    #[test]
    fn a_removed_pin_is_no_pin() {
        let tmux = StandIn::answering("work\n-ASQR_SESSION_QUEUE\n");

        let session = ask_session(&tmux, &pane("%3")).expect("tmux answers");

        assert_eq!(session.pinned, None);
    }

    #[test]
    fn an_empty_session_name_is_a_failure() {
        // tmux prints an empty name for a pane that is gone.
        let tmux = StandIn::answering("\n");

        assert!(ask_session(&tmux, &pane("%999")).is_err());
    }

    #[test]
    fn without_a_pane_tmux_is_not_asked() {
        let without = TmuxPane {
            socket: "/tmp/tmux-501/default".into(),
            pane: None,
        };

        assert!(ask_session(&testing::Unasked, &without).is_err());
    }

    #[test]
    fn the_pin_wins_over_the_session_name() {
        let tmux = StandIn::answering("my work\nASQR_SESSION_QUEUE=asqr-todos\n");

        assert_eq!(
            paired(&tmux, &pane("%3")),
            Paired::Pinned("asqr-todos".into())
        );
    }

    #[test]
    fn without_a_pin_the_session_name_names_the_queue() {
        let tmux = StandIn::answering("my work\n");

        assert_eq!(
            paired(&tmux, &pane("%3")),
            Paired::Session("my-work".into())
        );
    }

    #[test]
    fn a_tmux_that_does_not_answer_leaves_asqr_as_outside_tmux() {
        let tmux = StandIn::failing("tmux is not installed");

        assert_eq!(
            paired(&tmux, &pane("%3")),
            Paired::Unreachable("tmux is not installed".into())
        );
    }

    #[test]
    fn the_pin_is_set_in_the_session_of_the_pane() {
        let tmux = StandIn::answering("");

        pin(&tmux, &pane("%3"), "my-work").expect("pinned");

        assert_eq!(
            tmux.calls(),
            [[
                "set-environment",
                "-t",
                "%3",
                "ASQR_SESSION_QUEUE",
                "my-work"
            ]]
        );
    }

    #[test]
    fn a_pin_tmux_refuses_is_a_failure() {
        let tmux = StandIn::failing("tmux exited with 1: no server running");

        assert!(pin(&tmux, &pane("%3"), "my-work").is_err());
    }

    #[test]
    fn a_pane_without_an_id_is_not_pinned() {
        let without = TmuxPane {
            socket: "/tmp/tmux-501/default".into(),
            pane: None,
        };

        assert!(pin(&testing::Unasked, &without, "my-work").is_err());
    }

    #[test]
    fn the_sessions_of_the_panes_come_from_one_call_on_the_socket() {
        let tmux = StandIn::answering("%0 asqr\n%74 my work\n\n");

        let sessions = sessions_of_panes(&tmux, "/tmp/tmux-501/default").expect("tmux answers");

        assert_eq!(sessions.get("%74").map(String::as_str), Some("my work"));
        assert_eq!(sessions.get("%0").map(String::as_str), Some("asqr"));
        assert_eq!(sessions.len(), 2);
        assert_eq!(
            tmux.calls(),
            [[
                "-S",
                "/tmp/tmux-501/default",
                "list-panes",
                "-a",
                "-F",
                "#{pane_id} #{session_name}"
            ]]
        );
    }

    #[test]
    fn a_server_that_does_not_answer_gives_no_sessions() {
        let tmux = StandIn::failing("tmux exited with 1: no server running");

        assert!(sessions_of_panes(&tmux, "/tmp/tmux-501/default").is_err());
    }

    // -----------------------------------------------------------------
    // Running tmux
    // -----------------------------------------------------------------

    /// The timeout of the tests that run a real script, which beside a
    /// second test run may need longer than [`TMUX_TIMEOUT`] to start.
    const PATIENT: Duration = Duration::from_secs(30);

    /// An executable script with `body` in a fresh directory.
    fn script(body: &str) -> (tempfile::TempDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("tmux");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        (dir, path)
    }

    #[test]
    fn answers_what_tmux_printed() {
        let (_dir, program) = script(r#"echo "$*""#);
        let tmux = SystemTmux::at(program, PATIENT);

        assert_eq!(
            tmux.run(Some("/tmp/sock"), &["display-message", "-p", "x"]),
            Ok("-S /tmp/sock display-message -p x\n".to_owned())
        );
    }

    #[test]
    fn a_failing_tmux_is_a_failure_with_its_message() {
        let (_dir, program) = script("echo 'no server running' >&2\nexit 1");
        let tmux = SystemTmux::at(program, PATIENT);

        assert_eq!(
            tmux.run(None, &["display-message"]),
            Err(TmuxFailure("tmux exited with 1: no server running".into()))
        );
    }

    #[test]
    fn a_missing_tmux_is_a_failure() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tmux = SystemTmux::at(dir.path().join("tmux"), TMUX_TIMEOUT);

        assert_eq!(
            tmux.run(None, &["display-message"]),
            Err(TmuxFailure("tmux is not installed".into()))
        );
    }

    #[test]
    fn a_tmux_that_cannot_be_run_is_a_failure_with_the_reason() {
        // A directory where the program should be cannot be run.
        let dir = tempfile::tempdir().expect("temp dir");
        let tmux = SystemTmux::at(dir.path(), TMUX_TIMEOUT);

        let answer = tmux.run(None, &["display-message"]);

        let Err(TmuxFailure(reason)) = answer else {
            panic!("expected a failure, got {answer:?}");
        };
        assert!(reason.starts_with("cannot run tmux: "), "{reason}");
    }

    #[test]
    fn a_tmux_ended_by_a_signal_is_a_failure() {
        let (_dir, program) = script("kill -9 $$");
        let tmux = SystemTmux::at(program, PATIENT);

        assert_eq!(
            tmux.run(None, &["display-message"]),
            Err(TmuxFailure("tmux was ended by a signal: ".into()))
        );
    }

    #[test]
    fn a_tmux_that_hangs_is_given_up_after_the_timeout() {
        let (_dir, program) = script("exec sleep 30");
        let tmux = SystemTmux::at(program, Duration::from_millis(100));
        let started = Instant::now();

        let answer = tmux.run(None, &["display-message"]);

        assert_eq!(
            answer,
            Err(TmuxFailure("tmux did not answer within 100 ms".into()))
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn the_system_tmux_is_tmux_on_the_path_with_the_timeout() {
        let tmux = SystemTmux::default();

        // A run that exports the variable, as the tests of the binary do,
        // changes the timeout, not the program.
        assert_eq!(
            (tmux.program, tmux.timeout),
            (
                PathBuf::from("tmux"),
                timeout_from(
                    std::env::var_os(TIMEOUT_VARIABLE).as_deref(),
                    cfg!(debug_assertions)
                )
            )
        );
    }

    fn timeout_given(variable: &str) -> Duration {
        timeout_from(Some(OsStr::new(variable)), true)
    }

    #[test]
    fn a_debug_build_takes_the_milliseconds_of_the_variable() {
        assert_eq!(timeout_given("30000"), Duration::from_secs(30));
        assert_eq!(timeout_given("1"), Duration::from_millis(1));
    }

    #[test]
    fn a_value_beyond_ten_minutes_counts_as_ten_minutes() {
        assert_eq!(
            timeout_given("18446744073709551615"),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn a_release_build_ignores_the_variable() {
        assert_eq!(timeout_from(Some(OsStr::new("30000")), false), TMUX_TIMEOUT);
    }

    #[test]
    fn a_variable_that_is_no_positive_number_of_milliseconds_is_ignored() {
        for value in ["", "0", "abc", "-1", "1.5", "30 s"] {
            assert_eq!(timeout_given(value), TMUX_TIMEOUT, "{value:?}");
        }
        assert_eq!(timeout_from(None, true), TMUX_TIMEOUT);
    }
}
