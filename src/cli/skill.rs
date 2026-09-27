// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr skill` (spec section 9): the agent skill compiled into the
//! binary, so the installed skill always matches the installed asqr.

use std::path::Path;

use super::Exit;

/// The skill in the Agent Skills format.
pub const SKILL: &str = include_str!("../../skills/asqr/SKILL.md");

pub(super) fn run(install: Option<&Path>) -> Exit {
    let Some(root) = install else {
        print!("{SKILL}");
        return Exit::Success;
    };
    // `root` is a skills root such as `.claude/skills`; each skill lives in
    // a directory named after it.
    let dir = root.join("asqr");
    let file = dir.join("SKILL.md");
    match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, SKILL)) {
        Ok(()) => {
            println!("installed {}", file.display());
            Exit::Success
        }
        Err(error) => {
            eprintln!(
                "error: cannot install the skill into {}: {error}",
                dir.display()
            );
            Exit::Failure
        }
    }
}
