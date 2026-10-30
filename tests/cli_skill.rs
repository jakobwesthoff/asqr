// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr skill` (spec section 9).

mod common;

use common::{Sandbox, stdout};

const SKILL: &str = include_str!("../skills/asqr/SKILL.md");

/// The parts of the skill, in the order `asqr skill --install` writes
/// them. Each is `references/<name>.md` beside `SKILL.md`.
const PART_NAMES: [&str; 3] = ["context", "withdrawing", "patterns"];

/// The skill's directory in the repository.
fn skill_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/asqr")
}

/// A part as the repository holds it, read at run time so the tests
/// compare the binary's output with the file in the repository.
fn part(name: &str) -> String {
    let file = skill_dir().join(format!("references/{name}.md"));
    std::fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("{} is part of the skill: {error}", file.display()))
}

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
fn the_skill_carries_the_version_of_this_asqr() {
    // Every copy of the skill, installed, packaged or saved from
    // `asqr skill`, names the asqr it belongs to. A release that bumps the
    // crate version fails here until the skill names the new one.
    let frontmatter = SKILL
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .expect("the skill has frontmatter")
        .0;
    let stamp = format!(
        "\nmetadata:\n  asqr-version: \"{}\"",
        env!("CARGO_PKG_VERSION")
    );

    assert!(frontmatter.contains(&stamp), "{frontmatter}");
}

#[test]
fn prints_each_part_of_the_skill() {
    for name in PART_NAMES {
        let output = Sandbox::new()
            .asqr()
            .args(["skill", name])
            .output()
            .expect("runs");

        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {}",
            common::stderr(&output)
        );
        assert_eq!(stdout(&output), part(name), "{name}");
    }
}

#[test]
fn the_skill_ships_exactly_its_named_parts() {
    // A file the system puts there, such as a `.DS_Store`, is not part of
    // the skill.
    let mut shipped: Vec<String> = std::fs::read_dir(skill_dir().join("references"))
        .expect("the skill has a references directory")
        .map(|entry| {
            entry
                .expect("readable")
                .file_name()
                .into_string()
                .expect("UTF-8")
        })
        .filter(|name| !name.starts_with('.'))
        .collect();
    shipped.sort();
    let mut named: Vec<String> = PART_NAMES.iter().map(|name| format!("{name}.md")).collect();
    named.sort();

    assert_eq!(shipped, named);
}

#[test]
fn an_unknown_part_is_a_usage_error_that_names_the_parts() {
    let output = Sandbox::new()
        .asqr()
        .args(["skill", "nonsense"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    let stderr = common::stderr(&output);
    for name in PART_NAMES {
        assert!(stderr.contains(name), "{stderr}");
    }
    assert!(stderr.contains("`asqr skill`"), "{stderr}");
}

#[test]
fn a_part_and_install_together_are_a_usage_error() {
    let sandbox = Sandbox::new();
    let root = sandbox.home().join("skills");

    let output = sandbox
        .asqr()
        .args(["skill", "context", "--install"])
        .arg(&root)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = common::stderr(&output);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
    assert!(!root.exists(), "nothing is installed");
}

#[test]
fn installs_the_skill_and_its_parts_under_a_skills_root() {
    let sandbox = Sandbox::new();
    let root = sandbox.home().join("project/.claude/skills");

    let output = sandbox
        .asqr()
        .args(["skill", "--install"])
        .arg(&root)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    let core = root.join("asqr/SKILL.md");
    assert_eq!(std::fs::read_to_string(&core).expect("installed"), SKILL);
    let mut lines = format!("installed {}\n", core.display());
    for name in PART_NAMES {
        let file = root.join(format!("asqr/references/{name}.md"));
        assert_eq!(
            std::fs::read_to_string(&file).expect("installed"),
            part(name),
            "{name}"
        );
        lines += &format!("installed {}\n", file.display());
    }
    assert_eq!(stdout(&output), lines);
}

#[test]
fn installing_again_replaces_every_part_and_leaves_other_files() {
    let sandbox = Sandbox::new();
    let root = sandbox.home().join("skills");
    sandbox.file(
        "skills/asqr/references/old.md",
        "a part an older asqr wrote",
    );
    sandbox.file("skills/asqr/references/context.md", "an older version");
    let notes = sandbox.file("skills/asqr/notes.md", "the person's own file");

    sandbox
        .asqr()
        .args(["skill", "--install"])
        .arg(&root)
        .assert()
        .success();

    assert!(!root.join("asqr/references/old.md").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("asqr/references/context.md")).expect("installed"),
        part("context")
    );
    assert_eq!(
        std::fs::read_to_string(notes).expect("kept"),
        "the person's own file"
    );
}

#[test]
fn fails_when_the_parts_cannot_be_written() {
    let sandbox = Sandbox::new();
    let root = sandbox.home().join("skills");
    sandbox.file("skills/asqr/references", "a file where the parts should go");

    let output = sandbox
        .asqr()
        .args(["skill", "--install"])
        .arg(&root)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    let stderr = common::stderr(&output);
    assert!(stderr.contains("cannot install the skill"), "{stderr}");
}

#[test]
fn the_skill_starts_by_naming_its_last_line() {
    let body = SKILL
        .split_once("\n---\n\n")
        .expect("the frontmatter ends")
        .1;
    let first_paragraph = body.split("\n\n").next().expect("a first paragraph");

    assert!(
        flat(first_paragraph).contains("`End of the asqr skill.`"),
        "{first_paragraph}"
    );
    assert_eq!(SKILL.lines().last(), Some("End of the asqr skill."));
}

#[test]
fn each_part_names_the_core_first_and_ends_with_its_own_line() {
    for name in PART_NAMES {
        let text = part(name);

        assert!(
            text.starts_with("Part of the asqr skill. Read SKILL.md first"),
            "{name}"
        );
        assert_eq!(
            text.lines().last(),
            Some(format!("End of references/{name}.md.").as_str()),
            "{name}"
        );
    }
}

#[test]
fn each_part_stays_under_30000_bytes() {
    // Claude Code's Bash tool shows up to roughly 30,000 characters of
    // output inline, so `asqr skill <part>` arrives whole.
    for name in PART_NAMES {
        let size = part(name).len();
        assert!(size < 30_000, "{name} is {size} bytes");
    }
}

#[test]
fn the_core_fits_in_17980_bytes() {
    // The core arrives whole where Claude Code's Bash tool shows up to
    // roughly 30,000 characters of output inline. The cap is the size of
    // the core with every rule stated once. The example session, the exit
    // codes and the rules on info items, on writing for a person who has
    // not seen your work and on waiting for and reading every result keep
    // it above 16,000 bytes.
    let size = SKILL.len();
    assert!(size <= 17_980, "the core is {size} bytes");
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

/// The example session the skill teaches with: its JSON block that holds
/// `"questions"`. The result example comes before it.
fn example_session() -> &'static str {
    SKILL
        .split("```json\n")
        .skip(1)
        .map(|block| &block[..block.find("```").expect("the block ends")])
        .find(|block| block.contains("\"questions\""))
        .expect("the skill shows an example session")
}

#[test]
fn the_skill_teaches_asking_before_the_session_format() {
    // An agent that stops reading early has then seen how to ask, wait
    // and read the result, and has not yet seen the format it writes in,
    // so it cannot believe it is done.
    let at = |heading: &str| {
        SKILL
            .find(&format!("\n## {heading}\n"))
            .unwrap_or_else(|| panic!("the skill has a section {heading:?}"))
    };

    assert!(at("Asking and waiting") < at("Exit codes"));
    assert!(at("Exit codes") < at("Reading the result"));
    assert!(at("Reading the result") < at("The session file: every field you write"));
}

#[test]
fn the_example_session_validates_without_warnings() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("example.json", example_session());

    let output = sandbox
        .asqr()
        .arg("validate")
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", common::stderr(&output));
    assert_eq!(common::stderr(&output), "", "no warnings");
}

#[test]
fn the_skill_explains_every_exit_code() {
    for code in ["0", "1", "2", "10", "11", "12", "13", "14"] {
        assert!(
            SKILL.contains(&format!("| {code} |")),
            "exit code {code} is missing from the skill's table"
        );
    }
}

#[test]
fn the_skill_applies_only_when_the_user_asks_for_asqr() {
    let description = SKILL
        .lines()
        .find_map(|line| line.strip_prefix("description: "))
        .expect("the frontmatter has a description");

    assert!(
        description.contains("Only use it when the user explicitly asks"),
        "{description}"
    );
}

// =========================================================
// Rules pinned by a phrase
// =========================================================

// A rewrite of the skill must not drop a rule an agent needs on every
// session. Each rule is pinned by a short phrase rather than a sentence,
// so rewording around it needs no change here. The skill is hard-wrapped,
// and a phrase may cross a line break, so phrases are looked for in the
// text with every run of whitespace joined into one space.

/// `text` with every run of whitespace, line breaks included, as one
/// space.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The skill's section under `## <heading>`, up to the next `## `
/// heading or the end.
fn section(heading: &str) -> &'static str {
    let start = SKILL
        .find(&format!("\n## {heading}\n"))
        .unwrap_or_else(|| panic!("the skill has a section {heading:?}"))
        + 1;
    let end = SKILL[start + 3..]
        .find("\n## ")
        .map_or(SKILL.len(), |offset| start + 3 + offset);
    &SKILL[start..end]
}

#[test]
fn asking_says_every_session_comes_back_as_a_result() {
    assert!(
        flat(section("Asking and waiting"))
            .contains("Every session you ask comes back as a result")
    );
}

#[test]
fn an_info_only_session_still_comes_back_with_the_notes() {
    assert!(flat(SKILL).contains("Its result still comes back with the person's notes"));
}

#[test]
fn the_core_keeps_the_key_rules() {
    // Rules an agent needs on every session and that no check of asqr
    // catches, so they stay in `SKILL.md`, not in a part.
    for anchor in [
        "Every session you ask comes back as a result",
        "Its result still comes back with the person's notes",
        "may have answered in asqr",
        "Read every note",
        "Most sessions need no info item",
        "Length is no reason for an info item",
        "short items in reading order",
        "made-up question",
        "has not seen your work",
        "holds no decisions",
    ] {
        assert!(
            flat(SKILL).contains(anchor),
            "the skill's core lost {anchor:?}"
        );
    }
}

#[test]
fn the_core_tells_agents_to_follow_asqrs_advice() {
    let asking = flat(section("Asking and waiting"));

    assert!(asking.contains("start with `hint:`"), "{asking}");
    assert!(asking.contains("Follow it"), "{asking}");
}

#[test]
fn the_skill_says_to_look_in_asqr_before_answering_in_the_chat() {
    assert!(flat(section("Asking and waiting")).contains("may have answered in asqr"));
}

// =========================================================
// The help points agents to the skill
// =========================================================

/// The line `asqr --help` shows agents. "To the last line" matches the
/// core, which names its last line.
const AGENT_LINE: &str = "Agents: run `asqr skill` and read its output to the last line before you ask; it describes this version of asqr";

/// What `asqr` prints for `args`.
fn help(args: &[&str]) -> String {
    let output = Sandbox::new().asqr().args(args).output().expect("runs");
    assert_eq!(output.status.code(), Some(0));
    stdout(&output)
}

#[test]
fn help_tells_agents_to_read_the_skill() {
    let expected = format!(
        "{}\n\n{AGENT_LINE}\n\nUsage:",
        env!("CARGO_PKG_DESCRIPTION")
    );

    let text = help(&["-h"]);

    assert!(text.starts_with(&expected), "{text}");
}

#[test]
fn long_help_tells_agents_to_read_the_skill() {
    let expected = format!(
        "{}\n\n{AGENT_LINE}\n\nUsage:",
        env!("CARGO_PKG_DESCRIPTION")
    );

    let text = help(&["--help"]);

    assert!(text.starts_with(&expected), "{text}");
}

#[test]
fn ask_help_tells_agents_to_read_the_skill() {
    let expected =
        format!("Drop a session file into the queue and print its id\n\n{AGENT_LINE}\n\nUsage:");

    let text = help(&["ask", "--help"]);

    assert!(text.starts_with(&expected), "{text}");
}

#[test]
fn ask_short_help_keeps_its_one_line() {
    let text = help(&["ask", "-h"]);

    assert!(
        text.starts_with("Drop a session file into the queue and print its id\n\nUsage:"),
        "{text}"
    );
}
