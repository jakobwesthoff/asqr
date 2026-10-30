// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The hint about an installed skill that is older than asqr (spec
//! sections 6.7 and 9), as `asqr ask` and `asqr validate` print it. The
//! sandbox is the home directory and the current directory of every
//! command, so no test reads the skill directories of the machine or of
//! the checkout.

mod common;

use common::{Sandbox, stderr, stdout};

const SESSION: &str =
    r#"{"asqr": 2, "id": "batch-01", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

/// The skill's `SKILL.md` as an asqr of `version` installed it.
fn skill_of(version: &str) -> String {
    format!("---\nname: asqr\nmetadata:\n  asqr-version: \"{version}\"\n---\n\nBody\n")
}

/// The hint for one copy of asqr 0.1.0 in `place`, which `install` names.
fn hint(place: &str, install: &str) -> String {
    format!(
        "hint: the asqr skill in {place} is from asqr 0.1.0, this asqr is {}; read the current one with `asqr skill`, and ask the person whether to update the copy with `asqr skill --install {install}`",
        env!("CARGO_PKG_VERSION")
    )
}

fn lines(output: &std::process::Output) -> Vec<String> {
    stderr(output).lines().map(str::to_owned).collect()
}

#[test]
fn ask_names_an_older_copy_before_anything_else() {
    let sandbox = Sandbox::new();
    sandbox.file(".claude/skills/asqr/SKILL.md", &skill_of("0.1.0"));
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "batch-01\n");
    let lines = lines(&output);
    assert_eq!(
        lines[0],
        hint(".claude/skills/asqr", ".claude/skills"),
        "{lines:?}"
    );
    assert!(
        lines.last().is_some_and(|last| last.contains("asqr --dir")),
        "the wait hint stays the last line: {lines:?}"
    );
}

#[test]
fn validate_names_an_older_copy() {
    let sandbox = Sandbox::new();
    sandbox.file(".claude/skills/asqr/SKILL.md", &skill_of("0.1.0"));
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), format!("{}: valid\n", file.display()));
    assert_eq!(
        stderr(&output),
        format!("{}\n", hint(".claude/skills/asqr", ".claude/skills"))
    );
}

#[test]
fn the_hint_comes_before_the_errors_of_a_session_file() {
    let sandbox = Sandbox::new();
    sandbox.file(".claude/skills/asqr/SKILL.md", &skill_of("0.1.0"));
    let file = sandbox.file("session.json", r#"{"asqr": 2}"#);

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11), "{}", stderr(&output));
    let lines = lines(&output);
    assert_eq!(lines[0], hint(".claude/skills/asqr", ".claude/skills"));
    assert!(
        lines.len() > 1 && lines[1].starts_with("error:"),
        "{lines:?}"
    );
}

#[test]
fn a_copy_installed_by_this_asqr_gives_no_hint() {
    let sandbox = Sandbox::new();
    sandbox
        .asqr()
        .args(["skill", "--install", ".claude/skills"])
        .assert()
        .success();
    let file = sandbox.file("session.json", SESSION);

    let validated = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");
    let asked = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert!(
        !stderr(&validated).contains("hint:"),
        "{}",
        stderr(&validated)
    );
    assert!(
        !stderr(&asked).contains("the asqr skill in"),
        "{}",
        stderr(&asked)
    );
}

#[test]
fn without_a_copy_there_is_no_hint() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(stderr(&output), "");
}

#[test]
fn a_global_copy_is_shown_under_the_home_directory_also_when_it_is_the_current_directory() {
    let sandbox = Sandbox::new();
    // `.copilot/skills` is no project directory, so the copy is found as a
    // global one. The sandbox directory is where HOME points and also the
    // current directory, which the system may name by another path (macOS
    // has /var and /private/var).
    sandbox.file(".copilot/skills/asqr/SKILL.md", &skill_of("0.1.0"));
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(
        stderr(&output),
        format!("{}\n", hint("~/.copilot/skills/asqr", "~/.copilot/skills"))
    );
}

#[test]
fn a_copy_at_the_git_root_above_the_current_directory_is_found() {
    let sandbox = Sandbox::new();
    sandbox.file("project/.git", "gitdir: /elsewhere\n");
    sandbox.file("project/.claude/skills/asqr/SKILL.md", &skill_of("0.1.0"));
    // `.github/skills` is a project directory only, so this copy above the
    // root is not a global one and is not looked at.
    sandbox.file(".github/skills/asqr/SKILL.md", &skill_of("0.1.0"));
    let file = sandbox.file("session.json", SESSION);
    let current = sandbox.home().join("project/sub");
    std::fs::create_dir_all(&current).expect("a creatable directory");

    let output = sandbox
        .asqr()
        .current_dir(&current)
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(
        stderr(&output),
        format!(
            "{}\n",
            hint("~/project/.claude/skills/asqr", "~/project/.claude/skills")
        )
    );
}

#[test]
fn a_missing_home_leaves_the_project_copies() {
    let sandbox = Sandbox::new();
    sandbox.file(".claude/skills/asqr/SKILL.md", &skill_of("0.1.0"));
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr()
        .env_remove("HOME")
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stderr(&output),
        format!("{}\n", hint(".claude/skills/asqr", ".claude/skills"))
    );
}
