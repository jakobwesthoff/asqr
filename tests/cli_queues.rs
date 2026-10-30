// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr queues`: every queue, what it holds and who watches it.

mod common;

use std::os::unix::fs::PermissionsExt;

use asqr::pairing::TmuxPane;
use asqr::queue::{QueueLocation, lock_queue};
use common::{Sandbox, stdout};

/// The named queue `name` of the sandbox, created.
fn queue(sandbox: &Sandbox, name: &str) -> QueueLocation {
    let output = sandbox
        .asqr()
        .args(["--queue", name, "paths", "--json"])
        .output()
        .expect("runs");
    let paths: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    let location = QueueLocation::at(paths["queue"].as_str().expect("a queue"));
    location.create_layout().expect("layout");
    location
}

/// A stand-in `tmux` that names pane `%9` as part of session `other
/// work`, and the `PATH` that finds it first.
fn stand_in_tmux(sandbox: &Sandbox) -> String {
    let bin = sandbox.home().join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    let script = bin.join("tmux");
    std::fs::write(&script, "#!/bin/sh\necho '%9 other work'\n").expect("script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[test]
fn lists_every_queue_with_what_it_holds_and_who_watches_it() {
    let sandbox = Sandbox::new();
    let work = queue(&sandbox, "work");
    std::fs::write(work.inbox().join("a.json"), "{}").expect("written");
    std::fs::write(work.outbox().join("b.json"), "{}").expect("written");
    let watched = queue(&sandbox, "asqr-todos");
    let pane = TmuxPane {
        socket: "/tmp/tmux-test/default".into(),
        pane: Some("%9".into()),
    };
    let _watching = lock_queue(&watched, Some(&pane)).expect("this test plays the running asqr");

    let output = sandbox
        .asqr()
        .env("PATH", stand_in_tmux(&sandbox))
        .arg("queues")
        .output()
        .expect("runs");

    assert!(output.status.success(), "{output:?}");
    let host = gethostname::gethostname();
    assert_eq!(
        stdout(&output),
        format!(
            "QUEUE       WAITING  ANSWERED  WATCHED BY\n\
             asqr-todos        0         0  pid {} on {}, tmux session other work\n\
             default           0         0  nobody\n\
             work              1         1  nobody\n",
            std::process::id(),
            host.to_string_lossy()
        )
    );
}

#[test]
fn json_lists_the_same() {
    let sandbox = Sandbox::new();
    let work = queue(&sandbox, "work");
    std::fs::write(work.inbox().join("a.json"), "{}").expect("written");
    let _watching = lock_queue(&work, None).expect("this test plays the running asqr");

    let output = sandbox
        .asqr()
        .args(["queues", "--json"])
        .output()
        .expect("runs");

    let queues: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    assert_eq!(
        queues,
        serde_json::json!([
            {"queue": "default", "waiting_total": 0, "answered_total": 0,
             "watched_by": null, "tmux_session": null},
            {"queue": "work", "waiting_total": 1, "answered_total": 0,
             "watched_by": format!("pid {} on {}", std::process::id(),
                 gethostname::gethostname().to_string_lossy()),
             "tmux_session": null},
        ])
    );
}

#[test]
fn a_root_whose_queues_cannot_be_read_is_a_failure() {
    let sandbox = Sandbox::new();
    let work = queue(&sandbox, "work");
    let queues = work
        .dir()
        .parent()
        .expect("the queues directory")
        .to_path_buf();
    std::fs::set_permissions(&queues, std::fs::Permissions::from_mode(0o000))
        .expect("permissions change");

    let output = sandbox.asqr().arg("queues").output().expect("runs");
    std::fs::set_permissions(&queues, std::fs::Permissions::from_mode(0o755))
        .expect("permissions back");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with("error: cannot read the queues: "),
        "{output:?}"
    );
}

#[test]
fn a_watcher_outside_tmux_is_named_by_its_process() {
    let sandbox = Sandbox::new();
    let work = queue(&sandbox, "work");
    let _watching = lock_queue(&work, None).expect("this test plays the running asqr");

    let output = sandbox.asqr().arg("queues").output().expect("runs");

    let text = stdout(&output);
    let line = text
        .lines()
        .find(|line| line.starts_with("work"))
        .expect("listed");
    assert!(
        line.ends_with(&format!(
            "pid {} on {}",
            std::process::id(),
            gethostname::gethostname().to_string_lossy()
        )),
        "{line}"
    );
}
