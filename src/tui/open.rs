// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Opening the entry shown in the app the system has for its file type
//! (spec section 7.6, `o`): the system's opener runs apart from asqr, and
//! what it says when it ends badly reaches the status line.

use std::io;
use std::process::{Command, ExitStatus};

/// Starts `command` and waits for it on a thread of its own, which hands
/// its status to `exited`. Returns the child's process id.
///
/// A child nobody waits for stays in the process table as a zombie until
/// asqr exits, one more for every `o`. On Unix the child also gets a
/// process group of its own: where no desktop app takes the file,
/// `xdg-open` may fall back to a program that runs in the terminal, and
/// outside the foreground group that program cannot read the keys asqr
/// reads.
pub fn spawn_reaped(
    command: &mut Command,
    exited: impl FnOnce(ExitStatus) + Send + 'static,
) -> io::Result<u32> {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(command, 0);
    let mut child = command.spawn()?;
    let pid = child.id();
    let waited = std::thread::Builder::new()
        .name("opener".to_owned())
        .spawn(move || match child.wait() {
            Ok(status) => exited(status),
            Err(error) => tracing::warn!(%error, "could not wait for the opener"),
        });
    // The opener runs either way, so a thread that could not start only
    // leaves its process unwaited for.
    if let Err(error) = waited {
        tracing::warn!(%error, "could not start the thread that waits for the opener");
    }
    Ok(pid)
}

/// What the status line says about an opener that ended with `status`
/// after it was given the file shown as `name`; nothing when it succeeded.
/// `open` and `xdg-open` exit with an error at once when no app is set up
/// for the file's type.
pub fn opener_failure(name: &str, status: ExitStatus) -> Option<String> {
    if status.success() {
        return None;
    }
    Some(match status.code() {
        Some(code) => format!("cannot open {name}: the opener exited with {code}"),
        None => format!("cannot open {name}: the opener was ended by a signal"),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::process::Stdio;
    use std::sync::mpsc;

    use rustix::process::{Pid, getpgid, test_kill_process};

    use super::*;

    /// Runs `command` through [`spawn_reaped`] and returns its pid and the
    /// status the waiting thread handed on. Receiving the status is the
    /// gate: the thread has waited for the child by then.
    fn reaped(command: &mut Command) -> (u32, ExitStatus) {
        let (sender, statuses) = mpsc::channel();
        let pid = spawn_reaped(command, move |status| {
            sender.send(status).expect("the test waits for the status");
        })
        .expect("the command starts");
        let status = statuses.recv().expect("the thread hands on the status");
        (pid, status)
    }

    fn pid(raw: u32) -> Pid {
        Pid::from_raw(i32::try_from(raw).expect("a pid fits an i32")).expect("a pid is not 0")
    }

    #[test]
    fn the_opener_is_waited_for_once_it_ends() {
        let (child, _) = reaped(&mut Command::new("true"));

        // A child that ended but was not waited for stays a zombie, which
        // `kill` with signal 0 still finds.
        assert_eq!(
            test_kill_process(pid(child)),
            Err(rustix::io::Errno::SRCH),
            "no process, not even a zombie, is left"
        );
    }

    #[test]
    fn the_opener_runs_in_a_process_group_of_its_own() {
        let (reader, writer) = io::pipe().expect("a pipe");
        let (sender, statuses) = mpsc::channel();
        let mut command = Command::new("sh");
        command.args(["-c", "read line"]).stdin(Stdio::from(reader));
        let child = spawn_reaped(&mut command, move |status| {
            sender.send(status).expect("the test waits for the status");
        })
        .expect("the command starts");

        // The child waits for a line until the pipe closes, so it is
        // still there to be asked.
        let group = getpgid(Some(pid(child))).expect("the child runs");
        drop(writer);
        statuses
            .recv()
            .expect("the child ends once the pipe closes");

        assert_ne!(group, getpgid(None).expect("asqr's own group"));
    }

    #[test]
    fn an_opener_that_fails_is_named_with_its_exit_code() {
        let (_, status) = reaped(&mut Command::new("false"));

        assert_eq!(
            opener_failure("plan.md", status),
            Some("cannot open plan.md: the opener exited with 1".to_owned())
        );
    }

    #[test]
    fn an_opener_that_succeeds_says_nothing() {
        let (_, status) = reaped(&mut Command::new("true"));

        assert_eq!(opener_failure("plan.md", status), None);
    }

    #[test]
    fn an_opener_ended_by_a_signal_says_so() {
        let (_, status) = reaped(Command::new("sh").args(["-c", "kill -9 $$"]));

        assert_eq!(
            opener_failure("plan.md", status),
            Some("cannot open plan.md: the opener was ended by a signal".to_owned())
        );
    }
}
