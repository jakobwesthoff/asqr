// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The commands in asqr's hints reach the queue the command worked on
//! (spec section 6.7).

mod common;

use common::{Sandbox, stderr};

const SESSION: &str =
    r#"{"asqr": 2, "id": "batch-01", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

/// The command the reminder after `ask` names, for a queue reached by
/// `asqr_prefix`.
fn wait_command(asqr_prefix: &str) -> String {
    format!("`{asqr_prefix} wait batch-01 --timeout 540`")
}

/// Runs `ask` on the example session with `args` before it and returns
/// its stderr.
fn ask(sandbox: &Sandbox, args: &[&str], environment: &[(&str, &str)]) -> String {
    let file = sandbox.file("session.json", SESSION);
    let mut command = sandbox.asqr();
    command.args(args).arg("ask").arg(&file);
    for (name, value) in environment {
        command.env(name, value);
    }
    let output = command.output().expect("runs");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    stderr(&output)
}

#[test]
fn names_plain_asqr_when_nothing_chose_the_queue() {
    let sandbox = Sandbox::new();

    let err = ask(&sandbox, &[], &[]);

    assert!(err.contains(&wait_command("asqr")), "{err}");
}

#[test]
fn carries_the_queue_flag() {
    let sandbox = Sandbox::new();

    let err = ask(&sandbox, &["--queue", "work"], &[]);

    assert!(err.contains(&wait_command("asqr --queue work")), "{err}");
}

#[test]
fn carries_a_queue_the_environment_chose_as_a_flag() {
    // The agent's next shell, or the person's terminal, may not have the
    // variable.
    let sandbox = Sandbox::new();

    let err = ask(&sandbox, &[], &[("ASQR_QUEUE", "work")]);

    assert!(err.contains(&wait_command("asqr --queue work")), "{err}");
}

#[test]
fn carries_a_directory_the_environment_chose_as_a_flag() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();

    let err = ask(
        &sandbox,
        &[],
        &[("ASQR_DIR", dir.to_str().expect("UTF-8 temp path"))],
    );

    let expected = wait_command(&format!("asqr --dir {}", dir.display()));
    assert!(err.contains(&expected), "{err}");
}

#[test]
fn carries_an_explicit_default_queue() {
    // Without the flag, the variable would choose another queue.
    let sandbox = Sandbox::new();

    let err = ask(&sandbox, &["--queue", "default"], &[("ASQR_QUEUE", "work")]);

    assert!(err.contains(&wait_command("asqr --queue default")), "{err}");
}

#[test]
fn carries_a_queue_name_that_starts_with_a_dash_with_an_equals_sign() {
    // `--queue -scratch` reads `-scratch` as flags.
    let sandbox = Sandbox::new();

    let err = ask(&sandbox, &["--queue=-scratch"], &[]);

    assert!(
        err.contains(&wait_command("asqr --queue=-scratch")),
        "{err}"
    );
}

#[test]
fn names_a_relative_queue_directory_by_its_absolute_path() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr()
        .args(["--dir", "queue", "ask"])
        .arg(&file)
        .current_dir(sandbox.home())
        .output()
        .expect("runs");

    // The process's working directory is the resolved one: on macOS the
    // temp directory under /var is /private/var.
    let absolute = sandbox
        .home()
        .canonicalize()
        .expect("the home exists")
        .join("queue");
    let expected = wait_command(&format!("asqr --dir {}", absolute.display()));
    let err = stderr(&output);
    assert!(err.contains(&expected), "{err}");
}

#[test]
fn quotes_a_queue_directory_with_a_space() {
    let sandbox = Sandbox::new();
    let dir = sandbox.home().join("my queue");

    let err = ask(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8 temp path")],
        &[],
    );

    let expected = wait_command(&format!("asqr --dir '{}'", dir.display()));
    assert!(err.contains(&expected), "{err}");
}
