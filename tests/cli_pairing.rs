// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pairing with the tmux session (ADR 85), against a stand-in `tmux` on
//! `PATH`, so no test asks the tmux the tests may run in.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use assert_cmd::Command;
use common::{Sandbox, stdout};

/// The stand-in `tmux` of a sandbox: it notes every call in `calls` and
/// answers the session lookup with the session `TEST_SESSION` and, where
/// set, the pin `TEST_PIN`. `TEST_TMUX_EXIT` makes it fail.
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
                 case \"$1\" in\n\
                 display-message)\n\
                   echo \"$TEST_SESSION\"\n\
                   [ -n \"$TEST_PIN\" ] && echo \"ASQR_SESSION_QUEUE=$TEST_PIN\"\n\
                   ;;\n\
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
            .env_remove("TEST_TMUX_EXIT");
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
