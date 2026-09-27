// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr skill` (spec section 9).

mod common;

use common::{Sandbox, stdout};

const SKILL: &str = include_str!("../skills/asqr/SKILL.md");

#[test]
fn prints_the_skill_of_this_version() {
    let output = Sandbox::new().asqr().arg("skill").output().expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), SKILL);
}

#[test]
fn the_skill_has_agent_skills_frontmatter() {
    assert!(
        SKILL.starts_with("---\nname: asqr\ndescription: "),
        "{SKILL}"
    );
}

#[test]
fn installs_the_skill_under_a_skills_root() {
    let sandbox = Sandbox::new();
    let root = sandbox.home().join("project/.claude/skills");

    let output = sandbox
        .asqr()
        .args(["skill", "--install"])
        .arg(&root)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    let installed = root.join("asqr/SKILL.md");
    assert_eq!(
        std::fs::read_to_string(&installed).expect("installed"),
        SKILL
    );
    assert_eq!(
        stdout(&output),
        format!("installed {}\n", installed.display())
    );
}

#[test]
fn installing_again_replaces_an_older_skill() {
    let sandbox = Sandbox::new();
    let root = sandbox.home().join("skills");
    sandbox.file("skills/asqr/SKILL.md", "an older version");

    sandbox
        .asqr()
        .args(["skill", "--install"])
        .arg(&root)
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(root.join("asqr/SKILL.md")).expect("installed"),
        SKILL
    );
}

#[test]
fn fails_when_the_skill_cannot_be_written() {
    let sandbox = Sandbox::new();
    let blocked = sandbox.file("blocked", "a file where the skills root should be");

    let output = sandbox
        .asqr()
        .args(["skill", "--install"])
        .arg(&blocked)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
}
