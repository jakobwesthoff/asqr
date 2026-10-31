// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pairing with the tmux session (ADR 85), against a stand-in `tmux` on
//! `PATH`, so no test asks the tmux the tests may run in.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use asqr::pairing::TmuxPane;
use asqr::queue::{QueueLocation, lock_queue};
use assert_cmd::Command;
use common::{Sandbox, stdout};

/// The stand-in `tmux` of a sandbox: it notes every call in `calls` and
/// answers the session lookup with the session `TEST_SESSION` and, where
/// set, the pin `TEST_PIN`. `TEST_TMUX_EXIT` makes it fail, and
/// `TEST_TMUX_HANG` makes it never answer.
struct StandInTmux {
    bin: PathBuf,
    calls: PathBuf,
}

impl StandInTmux {
    fn new(sandbox: &Sandbox) -> Self {
        let bin = sandbox.home().join("bin");
        std::fs::create_dir_all(&bin).expect("bin dir");
        let calls = sandbox.home().join("tmux-calls");
        let script = bin.join("tmux");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n\
                 echo \"$*\" >> '{}'\n\
                 [ -n \"$TEST_TMUX_EXIT\" ] && exit \"$TEST_TMUX_EXIT\"\n\
                 [ -n \"$TEST_TMUX_HANG\" ] && exec sleep 60\n\
                 [ \"$1\" = -S ] && shift 2\n\
                 case \"$1\" in\n\
                 display-message)\n\
                   echo \"$TEST_SESSION\"\n\
                   [ -n \"$TEST_PIN\" ] && echo \"ASQR_SESSION_QUEUE=$TEST_PIN\"\n\
                   ;;\n\
                 list-panes) echo \"$TEST_PANES\" ;;\n\
                 esac\n\
                 exit 0\n",
                calls.display()
            ),
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        StandInTmux { bin, calls }
    }

    /// `asqr` inside pane `%3` of the stand-in's tmux, in session
    /// `session`.
    fn asqr(&self, sandbox: &Sandbox, session: &str) -> Command {
        let mut command = sandbox.asqr();
        let path = std::env::var("PATH").unwrap_or_default();
        command
            .env("PATH", format!("{}:{path}", self.bin.display()))
            .env("TMUX", "/tmp/tmux-test/default,1,0")
            .env("TMUX_PANE", "%3")
            .env("TEST_SESSION", session)
            .env_remove("TEST_PIN")
            .env_remove("TEST_TMUX_EXIT")
            .env_remove("TEST_TMUX_HANG");
        command
    }

    /// The calls the stand-in got, one line each.
    fn calls(&self) -> String {
        std::fs::read_to_string(&self.calls).unwrap_or_default()
    }
}

/// The queue directory `paths --json` names.
fn queue_of(command: &mut Command) -> (String, String) {
    let output = command.args(["paths", "--json"]).output().expect("runs");
    assert!(output.status.success(), "{output:?}");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    let queue = PathBuf::from(paths["queue"].as_str().expect("a queue"));
    (
        queue
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a name")
            .to_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn inside_tmux_asqr_uses_the_queue_named_after_the_session() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let (queue, _) = queue_of(&mut tmux.asqr(&sandbox, "my work"));

    assert_eq!(queue, "my-work");
    assert_eq!(
        tmux.calls(),
        "display-message -p -t %3 #{session_name} ; show-environment -t %3\n"
    );
}

#[test]
fn the_pin_wins_over_the_session_name() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let (queue, _) = queue_of(tmux.asqr(&sandbox, "asqr").env("TEST_PIN", "asqr-todos"));

    assert_eq!(queue, "asqr-todos");
}

/// A stand-in that never answers is given up on after the millisecond the
/// variable sets, so the queue falls back to the default however fast the
/// machine is. The sandbox's own timeout is 30 s, so a run well under that
/// shows the variable's timeout was the one used.
#[test]
fn the_timeout_the_test_variable_sets_is_the_one_tmux_gets() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);
    let started = std::time::Instant::now();

    let (queue, stderr) = queue_of(
        tmux.asqr(&sandbox, "my work")
            .env(asqr::pairing::TIMEOUT_VARIABLE, "1")
            .env("TEST_TMUX_HANG", "1"),
    );

    assert_eq!(queue, "default");
    assert_eq!(stderr, "");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "asqr waited {:?} for a tmux that never answers",
        started.elapsed()
    );
}

#[test]
fn an_unreachable_tmux_uses_the_default_queue_without_a_word() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let (queue, stderr) = queue_of(tmux.asqr(&sandbox, "asqr").env("TEST_TMUX_EXIT", "1"));

    assert_eq!(queue, "default");
    assert_eq!(stderr, "");
}

#[test]
fn asqr_queue_set_never_asks_tmux() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let (queue, _) = queue_of(tmux.asqr(&sandbox, "asqr").env("ASQR_QUEUE", "work"));

    assert_eq!(queue, "work");
    assert_eq!(tmux.calls(), "");
}

#[test]
fn asqr_skill_never_asks_tmux() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let output = tmux
        .asqr(&sandbox, "asqr")
        .arg("skill")
        .output()
        .expect("runs");

    assert!(output.status.success());
    assert_eq!(tmux.calls(), "");
}

#[test]
fn a_start_without_a_terminal_does_not_pin() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let output = tmux.asqr(&sandbox, "my work").output().expect("runs");

    assert!(!output.status.success(), "no terminal to run in");
    assert!(
        !tmux.calls().contains("set-environment"),
        "{}",
        tmux.calls()
    );
}

#[test]
fn the_refusal_names_the_tmux_session_of_the_asqr_watching() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);
    // The queue of tmux session `my work`, which an asqr in another
    // session watches.
    let output = tmux
        .asqr(&sandbox, "my work")
        .args(["paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    let location = QueueLocation::at(paths["queue"].as_str().expect("a queue"));
    let pane = TmuxPane {
        socket: "/tmp/tmux-test/default".into(),
        pane: Some("%9".into()),
    };
    let _watching = lock_queue(&location, Some(&pane)).expect("this test plays the running asqr");

    let output = tmux
        .asqr(&sandbox, "my work")
        .env("TEST_PANES", "%9 other work")
        .output()
        .expect("runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "another asqr is watching this queue (pid {} on ",
            std::process::id()
        )),
        "{stderr}"
    );
    assert!(stderr.contains(", tmux session other work)"), "{stderr}");
}

// ---------------------------------------------------------------------
// What `ask` says about who watches
// ---------------------------------------------------------------------

const SESSION: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

/// The named queue `name` of the sandbox, watched by "another asqr" (this
/// test, holding its lock) in pane `%9` of the stand-in's tmux server.
fn watched_from_pane_9(sandbox: &Sandbox, name: &str) -> asqr::queue::QueueLock {
    let output = sandbox
        .asqr()
        .args(["--queue", name, "paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    let location = QueueLocation::at(paths["queue"].as_str().expect("a queue"));
    location.create_layout().expect("layout");
    let pane = TmuxPane {
        socket: "/tmp/tmux-test/default".into(),
        pane: Some("%9".into()),
    };
    lock_queue(&location, Some(&pane)).expect("this test plays the running asqr")
}

/// What `ask` prints on stderr inside tmux session `session`, with the
/// variables in `environment` set.
fn ask_in(
    sandbox: &Sandbox,
    tmux: &StandInTmux,
    session: &str,
    environment: &[(&str, &str)],
) -> String {
    let file = sandbox.file("session.json", SESSION);
    let mut command = tmux.asqr(sandbox, session);
    command.arg("ask").arg(file);
    for (name, value) in environment {
        command.env(name, value);
    }
    let output = command.output().expect("runs");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn inside_tmux_the_warning_asks_for_asqr_in_a_window_of_the_session() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);
    let _watching = watched_from_pane_9(&sandbox, "asqr-todos");

    let stderr = ask_in(&sandbox, &tmux, "playground", &[("TEST_PANES", "%9 asqr")]);

    assert!(
        stderr.starts_with(
            "warning: no asqr is watching queue playground; asqr watches asqr-todos (tmux session asqr); the session waits in playground: ask the person to start `asqr` in a window of tmux session playground, or `asqr --queue playground` in a terminal of their own; do not start it yourself\n"
        ),
        "{stderr}"
    );
}

#[test]
fn ask_warns_when_the_session_queue_is_watched_from_another_session() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);
    let _watching = watched_from_pane_9(&sandbox, "my-work");

    let stderr = ask_in(&sandbox, &tmux, "my-work", &[("TEST_PANES", "%9 my work")]);

    assert!(
        stderr.starts_with(
            "warning: queue my-work is watched by the asqr of tmux session \"my work\", not of this session (\"my-work\"); the session waits there: ask the person whether to answer it there, or to start `asqr --queue <another name>` in a window of this session; do not start it yourself\n"
        ),
        "{stderr}"
    );
}

#[test]
fn ask_warns_when_the_pinned_queue_is_watched_from_another_session() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);
    let _watching = watched_from_pane_9(&sandbox, "work");

    let stderr = ask_in(
        &sandbox,
        &tmux,
        "asqr",
        &[("TEST_PIN", "work"), ("TEST_PANES", "%9 other")],
    );

    assert!(
        stderr.starts_with("warning: queue work is watched by the asqr of tmux session \"other\", not of this session (\"asqr\");"),
        "{stderr}"
    );
}

#[test]
fn the_asqr_of_the_own_session_gets_no_warning() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);
    let _watching = watched_from_pane_9(&sandbox, "asqr");

    let stderr = ask_in(&sandbox, &tmux, "asqr", &[("TEST_PANES", "%9 asqr")]);

    assert!(
        stderr.starts_with("hint: the person can answer"),
        "{stderr}"
    );
}

#[test]
fn asqr_queue_beside_another_pin_gets_a_hint() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let unwatched = ask_in(
        &sandbox,
        &tmux,
        "asqr",
        &[("ASQR_QUEUE", "work"), ("TEST_PIN", "asqr-todos")],
    );
    let _watching = watched_from_pane_9(&sandbox, "asqr-todos");
    let watched = ask_in(
        &sandbox,
        &tmux,
        "asqr",
        &[("ASQR_QUEUE", "work"), ("TEST_PIN", "asqr-todos")],
    );

    assert!(
        unwatched.contains("\nhint: ASQR_QUEUE chose queue work, but this tmux session's queue is asqr-todos; tell the person, who decides where you ask\n"),
        "{unwatched}"
    );
    assert!(
        watched.contains("\nhint: ASQR_QUEUE chose queue work, but the asqr of this tmux session watches asqr-todos; tell the person, who decides where you ask\n"),
        "{watched}"
    );
}

#[test]
fn asqr_queue_equal_to_the_pin_gets_no_hint() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let stderr = ask_in(
        &sandbox,
        &tmux,
        "asqr",
        &[("ASQR_QUEUE", "work"), ("TEST_PIN", "work")],
    );

    assert!(!stderr.contains("chose queue"), "{stderr}");
}

#[test]
fn without_a_pin_asqr_queue_gets_no_hint() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let stderr = ask_in(&sandbox, &tmux, "asqr", &[("ASQR_QUEUE", "work")]);

    assert!(!stderr.contains("chose queue"), "{stderr}");
}

#[test]
fn an_unreachable_tmux_with_asqr_queue_set_asks_into_that_queue_without_a_hint() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let stderr = ask_in(
        &sandbox,
        &tmux,
        "asqr",
        &[("ASQR_QUEUE", "work"), ("TEST_TMUX_EXIT", "1")],
    );

    assert!(
        stderr.starts_with("warning: no asqr is watching queue work;"),
        "{stderr}"
    );
    assert!(!stderr.contains("chose queue"), "{stderr}");
    assert!(!stderr.contains("default"), "{stderr}");
}

#[test]
fn after_an_unreachable_tmux_the_advice_names_queue_default() {
    let sandbox = Sandbox::new();
    let tmux = StandInTmux::new(&sandbox);

    let stderr = ask_in(&sandbox, &tmux, "asqr", &[("TEST_TMUX_EXIT", "1")]);

    assert!(
        stderr
            .contains("ask the person to start `asqr --queue default` in a terminal of their own"),
        "{stderr}"
    );
    assert!(stderr.contains("`asqr --queue default wait "), "{stderr}");
}
