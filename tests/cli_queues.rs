// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr queues`: every queue, what it holds and who watches it.

mod common;

use std::os::unix::fs::PermissionsExt;

use asqr::pairing::TmuxPane;
use asqr::queue::{QueueLocation, lock_queue};
use common::{Sandbox, stderr, stdout};

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

// =========================================================================
// asqr queues remove
// =========================================================================

const SESSION: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
    "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}]}"#;

/// Asks `SESSION` into queue `name` and gives its id.
fn asked(sandbox: &Sandbox, name: &str) -> String {
    let file = sandbox.file(&format!("{name}-session.json"), SESSION);
    let output = sandbox
        .asqr()
        .args(["--queue", name, "ask"])
        .arg(&file)
        .output()
        .expect("runs");
    assert!(output.status.success(), "{output:?}");
    stdout(&output).trim().to_owned()
}

/// The named queue `name` of the sandbox, used once: an `ask` made it and
/// the session it asked is withdrawn, so its archive holds that session.
fn used(sandbox: &Sandbox, name: &str) -> QueueLocation {
    let id = asked(sandbox, name);
    sandbox
        .asqr()
        .args(["--queue", name, "withdraw", &id])
        .assert()
        .success();
    QueueLocation::named(&sandbox.data_dir(), name)
}

/// `asqr queues remove <args>` in `sandbox`, which must succeed.
fn removed(sandbox: &Sandbox, name: &str) {
    sandbox
        .asqr()
        .args(["queues", "remove", name])
        .assert()
        .success();
}

/// The queue names the listing `args` prints.
fn listed(sandbox: &Sandbox, args: &[&str]) -> Vec<String> {
    let output = sandbox.asqr().args(args).output().expect("runs");
    assert!(output.status.success(), "{output:?}");
    stdout(&output)
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

#[test]
fn remove_takes_a_queue_out_of_the_list_and_history_keeps_it() {
    let sandbox = Sandbox::new();
    used(&sandbox, "old");

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "old"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        stdout(&output),
        "removed queue old from the lists; its finished sessions stay in asqr history\n"
    );
    assert_eq!(listed(&sandbox, &["queues"]), ["default"]);
    assert_eq!(listed(&sandbox, &["queues", "--removed"]), ["old"]);
    let history = sandbox
        .asqr()
        .args(["history", "--all"])
        .output()
        .expect("runs");
    assert!(stdout(&history).contains("queue: old"), "{history:?}");
}

#[test]
fn remove_refuses_a_queue_in_which_sessions_wait_and_names_force() {
    let sandbox = Sandbox::new();
    asked(&sandbox, "weekly");
    asked(&sandbox, "weekly");

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "weekly"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "error: 2 sessions wait in queue weekly; `asqr queues remove --force weekly` removes it anyway and rejects them\n"
    );
    assert!(listed(&sandbox, &["queues"]).contains(&"weekly".to_owned()));
}

/// Asks into queue `name` a session that shows a file, and gives its id.
fn asked_showing(sandbox: &Sandbox, name: &str) -> String {
    let shown = sandbox.file("shown.txt", "shown\n");
    let file = sandbox.file(
        &format!("{name}-shown.json"),
        &SESSION.replace(
            r#""kind": "single","#,
            &format!(
                r#""kind": "single", "context": [{{"type": "code", "path": {:?}}}],"#,
                shown.to_str().expect("UTF-8 path")
            ),
        ),
    );
    let output = sandbox
        .asqr()
        .args(["--queue", name, "ask"])
        .arg(&file)
        .output()
        .expect("runs");
    assert!(output.status.success(), "{output:?}");
    stdout(&output).trim().to_owned()
}

/// The state the archived session of `queue` records for the file it
/// showed.
fn recorded_state(sandbox: &Sandbox, queue: &str) -> serde_json::Value {
    let archive = QueueLocation::named(&sandbox.data_dir(), queue).archive();
    let archived = std::fs::read_dir(archive)
        .expect("archive")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .expect("the session is archived");
    let session: serde_json::Value =
        serde_json::from_slice(&std::fs::read(archived).expect("reads")).expect("JSON");
    session["questions"][0]["context"][0]["shown"]["state"].clone()
}

#[test]
fn a_force_that_stops_names_and_records_what_it_rejected_before() {
    let sandbox = Sandbox::new();
    let id = asked_showing(&sandbox, "work");
    let work = QueueLocation::named(&sandbox.data_dir(), "work");
    // A file dropped by hand at the same moment, which sorts after the ULID
    // `ask` gave, and which cannot be read.
    let locked = work.inbox().join("zz-locked.json");
    std::fs::write(&locked, SESSION).expect("dropped by hand");
    let time = std::fs::metadata(work.inbox().join(format!("{id}.json")))
        .and_then(|metadata| metadata.modified())
        .expect("a time");
    std::fs::File::options()
        .write(true)
        .open(&locked)
        .and_then(|file| file.set_modified(time))
        .expect("the same drop time");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("unreadable");

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "--force", "work"])
        .output()
        .expect("runs");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600))
        .expect("readable again");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "asqr: rejected {id} before stopping\n\
             error: cannot reject session zz-locked of queue work: Permission denied (os error 13); queue work stays in the lists\n"
        )
    );
    assert_eq!(recorded_state(&sandbox, "work"), "not_kept");
    assert!(listed(&sandbox, &["queues"]).contains(&"work".to_owned()));
}

#[test]
fn a_force_whose_marker_stays_names_and_records_what_it_rejected() {
    let sandbox = Sandbox::new();
    let id = asked_showing(&sandbox, "work");
    let work = QueueLocation::named(&sandbox.data_dir(), "work");
    // A refused removal leaves the queue's lock file, so the removal below
    // opens both lock files that exist and needs to write into the queue's
    // directory only to delete the marker.
    sandbox
        .asqr()
        .args(["queues", "remove", "work"])
        .assert()
        .code(1);
    std::fs::set_permissions(work.dir(), std::fs::Permissions::from_mode(0o500))
        .expect("read-only");

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "--force", "work"])
        .output()
        .expect("runs");
    std::fs::set_permissions(work.dir(), std::fs::Permissions::from_mode(0o755))
        .expect("writable again");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "asqr: rejected {id}\n\
             error: cannot take queue work out of the lists: Permission denied (os error 13); its waiting sessions are rejected, queue work stays in the lists\n"
        )
    );
    assert_eq!(recorded_state(&sandbox, "work"), "not_kept");
    assert!(work.active_marker().is_file());
}

#[test]
fn remove_force_rejects_the_waiting_session_and_wait_reads_why() {
    let sandbox = Sandbox::new();
    let shown = sandbox.file("shown.txt", "shown\n");
    let file = sandbox.file(
        "weekly-shown.json",
        &SESSION.replace(
            r#""kind": "single","#,
            &format!(
                r#""kind": "single", "context": [{{"type": "code", "path": {:?}}}],"#,
                shown.to_str().expect("UTF-8 path")
            ),
        ),
    );
    let ask = sandbox
        .asqr()
        .args(["--queue", "weekly", "ask"])
        .arg(&file)
        .output()
        .expect("runs");
    let id = stdout(&ask).trim().to_owned();

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "--force", "weekly"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        stdout(&output),
        "removed queue weekly from the lists and rejected its 1 waiting session; its finished sessions stay in asqr history\n"
    );
    let wait = sandbox
        .asqr()
        .args(["--queue", "weekly", "wait", &id, "--timeout", "0"])
        .output()
        .expect("runs");
    assert_eq!(wait.status.code(), Some(10), "{wait:?}");
    let result: serde_json::Value = serde_json::from_str(&stdout(&wait)).expect("JSON");
    assert_eq!(
        result["reason"],
        "the person removed queue weekly while this session waited in it"
    );
    let archived = std::fs::read_dir(QueueLocation::named(&sandbox.data_dir(), "weekly").archive())
        .expect("archive")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .expect("the session is archived");
    let session: serde_json::Value =
        serde_json::from_slice(&std::fs::read(archived).expect("reads")).expect("JSON");
    assert_eq!(
        session["questions"][0]["context"][0]["shown"]["state"],
        "not_kept"
    );
}

#[test]
fn remove_refuses_a_watched_queue_naming_its_tmux_session() {
    let sandbox = Sandbox::new();
    let work = used(&sandbox, "work");
    let pane = TmuxPane {
        socket: "/tmp/tmux-test/default".into(),
        pane: Some("%9".into()),
    };
    let _watching = lock_queue(&work, Some(&pane)).expect("this test plays the running asqr");

    let output = sandbox
        .asqr()
        .env("PATH", stand_in_tmux(&sandbox))
        .args(["queues", "remove", "work"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        format!(
            "error: queue work is watched by pid {} on {}, tmux session other work; switch that asqr to another queue or quit it first\n",
            std::process::id(),
            gethostname::gethostname().to_string_lossy()
        )
    );
}

#[test]
fn remove_of_a_queue_a_command_holds_says_it_is_busy() {
    let sandbox = Sandbox::new();
    let work = used(&sandbox, "work");
    let _compacting =
        asqr::queue::lock_queue_for(&work, 1, "asqr compact").expect("this test plays compact");

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "work"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        format!(
            "error: cannot remove queue work: it is busy (pid {} on {}, running asqr compact); try again in a moment\n",
            std::process::id(),
            gethostname::gethostname().to_string_lossy()
        )
    );
}

#[test]
fn remove_of_a_queue_that_is_not_there_fails() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "ghost"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        format!(
            "error: no queue ghost under {}\n",
            sandbox.data_dir().display()
        )
    );
}

#[test]
fn remove_with_no_queue_name_is_a_usage_error() {
    let sandbox = Sandbox::new();

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "my work"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "error: invalid queue name \"my work\" (letters, digits, '-', '_' and '.', not starting with '.')\n"
    );
}

#[test]
fn remove_refuses_the_default_queue() {
    let sandbox = Sandbox::new();
    used(&sandbox, "default");

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "default"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "error: the default queue cannot be removed\n"
    );
}

#[test]
fn remove_of_a_removed_queue_says_so() {
    let sandbox = Sandbox::new();
    used(&sandbox, "old");
    removed(&sandbox, "old");

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "old"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "queue old is removed already\n");
}

#[test]
fn remove_says_the_name_as_the_directory_spells_it() {
    let sandbox = Sandbox::new();
    used(&sandbox, "work");
    // On a filesystem that keeps case apart, `Work` is no queue.
    let ignores_case = QueueLocation::named(&sandbox.data_dir(), "Work")
        .dir()
        .exists();

    let output = sandbox
        .asqr()
        .args(["queues", "remove", "Work"])
        .output()
        .expect("runs");

    if ignores_case {
        assert_eq!(
            stdout(&output),
            "removed queue work from the lists; its finished sessions stay in asqr history\n"
        );
    } else {
        assert_eq!(output.status.code(), Some(1), "{output:?}");
    }
}

#[test]
fn ask_into_a_removed_queue_lists_it_again() {
    let sandbox = Sandbox::new();
    used(&sandbox, "old");
    removed(&sandbox, "old");
    let id = asked(&sandbox, "old");
    sandbox
        .asqr()
        .args(["--queue", "old", "withdraw", &id])
        .assert()
        .success();

    assert_eq!(listed(&sandbox, &["queues"]), ["default", "old"]);
}

#[test]
fn a_queue_from_before_the_update_is_listed_only_while_a_session_waits() {
    let sandbox = Sandbox::new();
    // A queue as asqr 0.16.0 made it: its directories and no marker.
    let old = QueueLocation::named(&sandbox.data_dir(), "old");
    for dir in [old.inbox(), old.outbox(), old.drafts(), old.archive()] {
        std::fs::create_dir_all(dir).expect("dir");
    }
    assert_eq!(listed(&sandbox, &["queues"]), ["default"]);

    std::fs::write(old.inbox().join("dropped.json"), SESSION).expect("an old asqr asks");

    assert_eq!(listed(&sandbox, &["queues"]), ["default", "old"]);
}

#[test]
fn json_or_removed_with_remove_is_a_usage_error() {
    let sandbox = Sandbox::new();
    used(&sandbox, "old");

    for args in [
        ["queues", "--json", "remove", "old"],
        ["queues", "--removed", "remove", "old"],
        ["queues", "remove", "old", "--json"],
    ] {
        let output = sandbox.asqr().args(args).output().expect("runs");
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
    }
    assert_eq!(listed(&sandbox, &["queues"]), ["default", "old"]);
}

#[test]
fn queue_and_dir_choose_nothing_for_remove() {
    let sandbox = Sandbox::new();
    used(&sandbox, "old");
    used(&sandbox, "other");

    for args in [
        vec!["queues", "--queue", "other", "remove", "old"],
        vec!["--queue", "other", "queues", "remove", "other"],
    ] {
        let output = sandbox.asqr().args(&args).output().expect("runs");
        assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
    }
    let dir = sandbox.queue_dir();
    let output = sandbox
        .asqr()
        .args(["queues", "--dir"])
        .arg(&dir)
        .args(["remove", "old"])
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(stdout(&output), "queue old is removed already\n");
    assert_eq!(listed(&sandbox, &["queues"]), ["default"]);
}

#[test]
fn queues_removed_json_lists_the_removed_queues() {
    let sandbox = Sandbox::new();
    used(&sandbox, "old");
    removed(&sandbox, "old");

    let output = sandbox
        .asqr()
        .args(["queues", "--removed", "--json"])
        .output()
        .expect("runs");

    let queues: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    assert_eq!(
        queues,
        serde_json::json!([
            {"queue": "old", "waiting_total": 0, "answered_total": 1,
             "watched_by": null, "tmux_session": null},
        ])
    );
}
