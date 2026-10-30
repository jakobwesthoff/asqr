// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr skill` (spec section 9): the agent skill compiled into the
//! binary, so `asqr skill` prints the skill of this asqr.

use std::io;
use std::path::Path;

use super::Exit;

/// The skill's core in the Agent Skills format: every rule an agent needs
/// on every session.
pub const SKILL: &str = include_str!("../../skills/asqr/SKILL.md");

/// The skill's parts by the name `asqr skill <name>` takes. Each is
/// `references/<name>.md` beside `SKILL.md`, which names it at the step
/// that needs it.
pub const PARTS: &[(&str, &str)] = &[
    (
        "context",
        include_str!("../../skills/asqr/references/context.md"),
    ),
    (
        "withdrawing",
        include_str!("../../skills/asqr/references/withdrawing.md"),
    ),
    (
        "patterns",
        include_str!("../../skills/asqr/references/patterns.md"),
    ),
];

pub(super) fn run(part: Option<&str>, install: Option<&Path>) -> Exit {
    match install {
        Some(root) => install_into(root),
        None => print(part),
    }
}

fn print(part: Option<&str>) -> Exit {
    let Some(name) = part else {
        print!("{SKILL}");
        return Exit::Success;
    };
    if let Some((_, text)) = PARTS.iter().find(|(known, _)| *known == name) {
        print!("{text}");
        return Exit::Success;
    }
    // An agent asks for a part because a skill named it. A name this asqr
    // does not know means that skill came from another version, so the
    // message points to the skill of this one.
    let names: Vec<&str> = PARTS.iter().map(|(known, _)| *known).collect();
    let (last, others) = names.split_last().expect("the skill has parts");
    eprintln!(
        "error: the asqr skill has no part \"{name}\"; its parts are {} and {last}",
        others.join(", ")
    );
    eprintln!(
        "The skill you read may belong to another version of asqr: `asqr skill` prints the one of this asqr."
    );
    Exit::Usage
}

fn install_into(root: &Path) -> Exit {
    // `root` is a skills root such as `.claude/skills`; each skill lives in
    // a directory named after it.
    let dir = root.join("asqr");
    match write_skill(&dir) {
        Ok(()) => Exit::Success,
        Err(error) => {
            eprintln!(
                "error: cannot install the skill into {}: {error}",
                dir.display()
            );
            Exit::Failure
        }
    }
}

/// Writes the core and every part into `dir`, naming each file on stdout
/// once it is written, so a failure half way leaves stdout naming what
/// was written.
fn write_skill(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let core = dir.join("SKILL.md");
    std::fs::write(&core, SKILL)?;
    println!("installed {}", core.display());

    // asqr owns `references/`: it is replaced whole, so a part an older
    // asqr wrote and this one no longer ships disappears. Any other file
    // in `dir` stays.
    let references = dir.join("references");
    match std::fs::remove_dir_all(&references) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    std::fs::create_dir(&references)?;
    for (name, text) in PARTS {
        let file = references.join(format!("{name}.md"));
        std::fs::write(&file, text)?;
        println!("installed {}", file.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_names_every_part_by_file_and_by_command() {
        // Hosts that load the skill directory follow the file; an agent
        // that only ran `asqr skill` has no file and needs the command.
        for (name, _) in PARTS {
            assert!(SKILL.contains(&format!("references/{name}.md")), "{name}");
            assert!(SKILL.contains(&format!("`asqr skill {name}`")), "{name}");
        }
    }

    #[test]
    fn every_part_the_core_links_is_a_part() {
        // A file name is the run of lowercase letters after `references/`
        // that ends in `.md`; `references/` alone, as the opening
        // paragraph names the directory, is followed by other text.
        let linked = SKILL
            .match_indices("references/")
            .filter_map(|(offset, _)| {
                let rest = &SKILL[offset + "references/".len()..];
                let name = rest.split(|c: char| !c.is_ascii_lowercase()).next()?;
                rest[name.len()..].starts_with(".md").then_some(name)
            });
        for name in linked {
            assert!(
                PARTS.iter().any(|(known, _)| *known == name),
                "the core links references/{name}.md, which is no part"
            );
        }
    }
}
