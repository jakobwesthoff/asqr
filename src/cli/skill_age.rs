// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The hint about an installed agent skill that is older than this asqr
//! (spec sections 6.7 and 9): where installed copies are looked for, how
//! old a copy is, and the text of the hint. `ask` and `validate` make one
//! call, [`hint_if_older`]; everything else is here.

use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::hint::{Line, shell_word};
use crate::regular_file;

// =========================================================================
// Where installed copies are looked for
// =========================================================================

/// Skills directories of a project, relative to the current directory and
/// to every directory above it up to the git root. Each entry is a
/// location the documentation of an agent names.
const PROJECT_DIRS: &[&str] = &[
    // The shared directory several agents read: Codex
    // (https://learn.chatgpt.com/docs/build-skills), Gemini CLI
    // (https://geminicli.com/docs/cli/skills/), GitHub Copilot
    // (https://docs.github.com/en/copilot/concepts/customizations/agent-skills)
    // and Cursor (https://cursor.com/docs/skills).
    ".agents/skills",
    // Claude Code: https://code.claude.com/docs/en/skills
    ".claude/skills",
    // Gemini CLI: https://geminicli.com/docs/cli/skills/
    ".gemini/skills",
    // GitHub Copilot:
    // https://docs.github.com/en/copilot/concepts/customizations/agent-skills
    ".github/skills",
    // Cursor: https://cursor.com/docs/skills
    ".cursor/skills",
    // Cursor loads the Codex directory too: https://cursor.com/docs/skills
    ".codex/skills",
];

/// How much of a `SKILL.md` is read. The frontmatter is at the top, and the
/// core is longer than this, so a file is never read whole.
const PREFIX: usize = 16 * 1024;

/// Skills directories relative to the home directory.
const GLOBAL_DIRS: &[&str] = &[
    // The shared directory: Codex (https://learn.chatgpt.com/docs/build-skills),
    // Gemini CLI (https://geminicli.com/docs/cli/skills/), GitHub Copilot
    // and Cursor (see above).
    ".agents/skills",
    // Claude Code: https://code.claude.com/docs/en/skills
    ".claude/skills",
    // Gemini CLI: https://geminicli.com/docs/cli/skills/
    ".gemini/skills",
    // GitHub Copilot:
    // https://docs.github.com/en/copilot/concepts/customizations/agent-skills
    ".copilot/skills",
    // Cursor: https://cursor.com/docs/skills
    ".cursor/skills",
];

/// What a search needs to know, so tests can give it directories of their
/// own.
struct Search<'a> {
    cwd: &'a Path,
    home: Option<&'a Path>,
    /// The directory the walk up from `cwd` ends in, the git root. None
    /// searches `cwd` alone.
    stop: Option<&'a Path>,
    /// The version of the asqr that runs.
    running: &'a str,
}

/// Prints the hint when an installed copy of the skill is older than this
/// asqr. The directories come from the process: the current directory,
/// `HOME` and the git root above the current directory.
pub(super) fn hint_if_older() {
    if let Some(text) = hint_of_process() {
        Line::hint(text).print();
    }
}

/// The text of the hint for the directories of this process. A current
/// directory that cannot be read, such as one removed under the process,
/// leaves nothing to search from.
fn hint_of_process() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from);
    let stop = git_root(&cwd, None);
    check(&Search {
        cwd: &cwd,
        home: home.as_deref(),
        stop: stop.as_deref(),
        running: env!("CARGO_PKG_VERSION"),
    })
}

/// The nearest directory from `cwd` up that holds a `.git`, a directory in
/// a repository and a file in a linked worktree. The search does not go
/// above `ceiling`.
fn git_root(cwd: &Path, ceiling: Option<&Path>) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        // `symlink_metadata` also counts a `.git` that is a dangling
        // link: the directory is still where a repository was meant to
        // be.
        if std::fs::symlink_metadata(dir.join(".git")).is_ok() {
            return Some(dir.to_owned());
        }
        if Some(dir) == ceiling {
            break;
        }
    }
    None
}

/// The text of the hint, none when no installed copy is older than
/// `search.running`.
fn check(search: &Search<'_>) -> Option<String> {
    let running = parse_version(search.running)?;
    let mut old = Vec::new();
    // Agents share one copy through symlinks, so a copy counts once, by
    // the real path of its file, under the first directory that reaches it.
    let mut seen = HashSet::new();
    let mut consider = |shown: Shown, dir: PathBuf| {
        let file = dir.join("SKILL.md");
        let (Some(from), Ok(real)) = (read_stamp(&file), file.canonicalize()) else {
            return;
        };
        if seen.insert(real) && is_older(from, running) {
            old.push(OldCopy { shown, from });
        }
    };
    let canonical_home = search.home.and_then(|home| home.canonicalize().ok());
    for (level, root) in project_roots(search.cwd, search.stop)
        .into_iter()
        .enumerate()
    {
        for entry in PROJECT_DIRS {
            // Where the copy is, as the hint names it: the directory of
            // the agent for the level the agent works in, otherwise by
            // the home directory when the copy is below it, and by its
            // full path when it is not.
            let shown = if level == 0 {
                Shown::Relative(entry.into())
            } else {
                let above = root.join(entry);
                let under_home = [search.home, canonical_home.as_deref()]
                    .into_iter()
                    .flatten()
                    .find_map(|home| above.strip_prefix(home).ok());
                match under_home {
                    Some(rest) => Shown::Home(rest.to_owned()),
                    None => Shown::Absolute(above),
                }
            };
            consider(shown, root.join(entry).join("asqr"));
        }
    }
    if let Some(home) = search.home {
        for entry in GLOBAL_DIRS {
            consider(Shown::Home(entry.into()), home.join(entry).join("asqr"));
        }
    }
    (!old.is_empty()).then(|| hint_text(&old, search.running))
}

/// The stamp of the `SKILL.md` at `file`. None when the file cannot be read
/// as a regular file, or its stamp is no version: such a copy is not
/// reported, since a hint about the check helps nobody.
fn read_stamp(file: &Path) -> Option<Stamp> {
    let mut prefix = Vec::new();
    // `open` does not wait for a writer on a FIFO.
    regular_file::open(file)
        .ok()?
        .take(PREFIX as u64)
        .read_to_end(&mut prefix)
        .ok()?;
    // Whatever follows the frontmatter is not read, so a byte that is no
    // UTF-8, or a character cut at the end of the prefix, must not hide
    // the copy.
    stamp_of(&String::from_utf8_lossy(&prefix))
}

/// `cwd` and each directory above it up to `stop`, nearest first. `cwd`
/// alone when there is no `stop`, or `stop` is not above it: without a
/// repository there is no root to end the walk at.
fn project_roots(cwd: &Path, stop: Option<&Path>) -> Vec<PathBuf> {
    let chain: Vec<&Path> = cwd.ancestors().collect();
    let end = stop
        .and_then(|stop| chain.iter().position(|dir| *dir == stop))
        .unwrap_or(0);
    chain[..=end].iter().map(|dir| dir.to_path_buf()).collect()
}

/// A copy without a stamp is older than any asqr.
fn is_older(copy: Stamp, running: Version) -> bool {
    match copy {
        Stamp::Unstamped => true,
        Stamp::Version(version) => version < running,
    }
}

// =========================================================================
// Versions
// =========================================================================

/// `asqr-version` of a copy: three numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

/// What a copy says about the asqr it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stamp {
    /// No `asqr-version`: installed by asqr 0.14 or earlier.
    Unstamped,
    Version(Version),
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// `MAJOR.MINOR.PATCH`, numbers only. A pre-release or build suffix after
/// the patch number does not count.
fn parse_version(text: &str) -> Option<Version> {
    let core = text.split(['-', '+']).next()?;
    // `u64::from_str` accepts a leading `+`, which is no digit.
    let number = |part: &str| {
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let mut parts = core.split('.');
    let version = Version(
        number(parts.next()?)?,
        number(parts.next()?)?,
        number(parts.next()?)?,
    );
    parts.next().is_none().then_some(version)
}

/// The stamp in the frontmatter of the text of a `SKILL.md`. None when the
/// frontmatter has an `asqr-version` that is no version.
///
/// The one key is read by hand: a YAML parser would be a dependency for
/// a single line, and the line is the same wherever it is nested.
fn stamp_of(text: &str) -> Option<Stamp> {
    let Some(value) = frontmatter(text).into_iter().find_map(|line| {
        line.trim_start()
            .strip_prefix("asqr-version:")
            .map(str::trim)
    }) else {
        return Some(Stamp::Unstamped);
    };
    let unquoted = ['"', '\'']
        .iter()
        .find_map(|quote| value.strip_prefix(*quote)?.strip_suffix(*quote))
        .unwrap_or(value);
    parse_version(unquoted).map(Stamp::Version)
}

/// The lines between the two `---` lines that open a text. A text that
/// does not open with one, or never closes it, has none.
fn frontmatter(text: &str) -> Vec<&str> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Vec::new();
    }
    let mut inside = Vec::new();
    for line in lines {
        if line == "---" {
            return inside;
        }
        inside.push(line);
    }
    Vec::new()
}

// =========================================================================
// The text of the hint
// =========================================================================

/// Where a copy is shown to be, which is also where `asqr skill --install`
/// takes it.
#[derive(Debug, PartialEq, Eq)]
enum Shown {
    /// Below the current directory: the skills directory as the table
    /// names it.
    Relative(PathBuf),
    /// Below the home directory: the path after it.
    Home(PathBuf),
    Absolute(PathBuf),
}

impl Shown {
    /// The skills directory as the hint names the copy's place, with a
    /// character that would break the hint's line shown as U+FFFD.
    fn skills_dir(&self) -> String {
        let (prefix, path) = match self {
            Shown::Relative(path) => ("", path),
            Shown::Home(path) => ("~/", path),
            Shown::Absolute(path) => ("", path),
        };
        let text = format!("{prefix}{}", path.to_string_lossy());
        text.chars()
            .map(|c| if c.is_control() { '\u{fffd}' } else { c })
            .collect()
    }

    /// The copy's directory as the hint names it.
    fn name(&self) -> String {
        format!("{}/asqr", self.skills_dir())
    }

    /// The skills directory for `asqr skill --install`, written so a shell
    /// reads it back as the same directory. A path that cannot be written
    /// that way shows as `<dir>`.
    fn install_dir(&self) -> String {
        const UNWRITABLE: &str = "<dir>";
        match self {
            Shown::Relative(path) | Shown::Absolute(path) => shell_word(path, UNWRITABLE),
            // The shell expands `~/` only unquoted, so the prefix stands
            // outside the quotes the rest may need.
            Shown::Home(path) => match shell_word(path, UNWRITABLE) {
                word if word == UNWRITABLE => word,
                word => format!("~/{word}"),
            },
        }
    }
}

/// A copy older than this asqr.
struct OldCopy {
    shown: Shown,
    from: Stamp,
}

/// The text of the hint for `old`, which is not empty. This is the one
/// place the wording lives.
fn hint_text(old: &[OldCopy], running: &str) -> String {
    let from = |stamp: &Stamp| match stamp {
        Stamp::Version(version) => format!("asqr {version}"),
        Stamp::Unstamped => "an older asqr".to_owned(),
    };
    let install = |copy: &OldCopy| format!("`asqr skill --install {}`", copy.shown.install_dir());
    match old {
        [copy] => format!(
            "the asqr skill in {} is from {}, this asqr is {running}; read the current one with `asqr skill`, and ask the person whether to update the copy with {}",
            copy.shown.name(),
            from(&copy.from),
            install(copy)
        ),
        copies => {
            let named: Vec<String> = copies
                .iter()
                .map(|copy| format!("{} (from {})", copy.shown.name(), from(&copy.from)))
                .collect();
            let installs: Vec<String> = copies.iter().map(install).collect();
            format!(
                "the asqr skill copies in {} are older than this asqr, {running}; read the current skill with `asqr skill`, and ask the person whether to update them with {}",
                list(&named),
                list(&installs)
            )
        }
    }
}

/// `items` as prose: "a", "a and b", "a, b and c".
fn list(items: &[String]) -> String {
    match items.split_last().expect("a hint names at least one copy") {
        (only, []) => only.clone(),
        (last, others) => format!("{} and {last}", others.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: &str = "0.15.0";

    // =====================================================================
    // Helpers
    // =====================================================================

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temp dir")
    }

    /// The text of a `SKILL.md` that asqr installed, with `stamp` as its
    /// `asqr-version`, or without one as an asqr of 0.14 or earlier wrote
    /// it.
    fn skill_md(stamp: Option<&str>) -> String {
        match stamp {
            Some(version) => format!(
                "---\nname: asqr\ndescription: Ask.\nmetadata:\n  asqr-version: \"{version}\"\n---\n\nBody\n"
            ),
            None => "---\nname: asqr\ndescription: Ask.\n---\n\nBody\n".to_owned(),
        }
    }

    /// Installs a copy under `base/entry/asqr` and returns its directory.
    fn put(base: &Path, entry: &str, stamp: Option<&str>) -> PathBuf {
        let dir = base.join(entry).join("asqr");
        std::fs::create_dir_all(&dir).expect("a creatable directory");
        std::fs::write(dir.join("SKILL.md"), skill_md(stamp)).expect("a writable file");
        dir
    }

    fn run(cwd: &Path, home: Option<&Path>, stop: Option<&Path>) -> Option<String> {
        check(&Search {
            cwd,
            home,
            stop,
            running: RUNNING,
        })
    }

    /// The text of the hint for one copy installed by `0.14.0` under
    /// `.claude/skills` of the current directory.
    const ONE_OLD_COPY: &str = "the asqr skill in .claude/skills/asqr is from asqr 0.14.0, this asqr is 0.15.0; read the current one with `asqr skill`, and ask the person whether to update the copy with `asqr skill --install .claude/skills`";

    // =====================================================================
    // What is reported
    // =====================================================================

    #[test]
    fn a_project_copy_from_an_older_asqr_is_named_with_its_version() {
        let project = temp();
        put(project.path(), ".claude/skills", Some("0.14.0"));

        let hint = run(project.path(), None, Some(project.path()));

        assert_eq!(hint.as_deref(), Some(ONE_OLD_COPY));
    }

    #[test]
    fn a_copy_without_a_stamp_is_from_an_older_asqr() {
        let project = temp();
        put(project.path(), ".claude/skills", None);

        let hint = run(project.path(), None, Some(project.path()));

        assert_eq!(
            hint.as_deref(),
            Some(
                "the asqr skill in .claude/skills/asqr is from an older asqr, this asqr is 0.15.0; read the current one with `asqr skill`, and ask the person whether to update the copy with `asqr skill --install .claude/skills`"
            )
        );
    }

    #[test]
    fn a_global_copy_is_shown_under_the_home_directory() {
        let home = temp();
        let project = temp();
        put(home.path(), ".copilot/skills", Some("0.14.0"));

        let hint = run(project.path(), Some(home.path()), Some(project.path()));

        assert_eq!(
            hint.as_deref(),
            Some(
                "the asqr skill in ~/.copilot/skills/asqr is from asqr 0.14.0, this asqr is 0.15.0; read the current one with `asqr skill`, and ask the person whether to update the copy with `asqr skill --install ~/.copilot/skills`"
            )
        );
    }

    #[test]
    fn several_old_copies_go_into_one_hint() {
        let home = temp();
        let project = temp();
        put(project.path(), ".claude/skills", Some("0.14.0"));
        put(home.path(), ".agents/skills", None);

        let hint = run(project.path(), Some(home.path()), Some(project.path()));

        assert_eq!(
            hint.as_deref(),
            Some(
                "the asqr skill copies in .claude/skills/asqr (from asqr 0.14.0) and ~/.agents/skills/asqr (from an older asqr) are older than this asqr, 0.15.0; read the current skill with `asqr skill`, and ask the person whether to update them with `asqr skill --install .claude/skills` and `asqr skill --install ~/.agents/skills`"
            )
        );
    }

    #[test]
    fn three_old_copies_are_listed_with_commas_and_an_and() {
        let old = |dir: &str, from| OldCopy {
            shown: Shown::Relative(dir.into()),
            from,
        };
        let copies = [
            old(".agents/skills", Stamp::Unstamped),
            old(".claude/skills", Stamp::Version(Version(0, 14, 0))),
            old(".codex/skills", Stamp::Version(Version(0, 9, 2))),
        ];

        assert_eq!(
            hint_text(&copies, RUNNING),
            "the asqr skill copies in .agents/skills/asqr (from an older asqr), .claude/skills/asqr (from asqr 0.14.0) and .codex/skills/asqr (from asqr 0.9.2) are older than this asqr, 0.15.0; read the current skill with `asqr skill`, and ask the person whether to update them with `asqr skill --install .agents/skills`, `asqr skill --install .claude/skills` and `asqr skill --install .codex/skills`"
        );
    }

    #[test]
    fn every_directory_of_both_tables_is_searched() {
        for entry in PROJECT_DIRS {
            let project = temp();
            put(project.path(), entry, Some("0.14.0"));

            let hint = run(project.path(), None, Some(project.path()));

            let named = format!("the asqr skill in {entry}/asqr is from asqr 0.14.0");
            assert!(
                hint.as_deref().is_some_and(|hint| hint.starts_with(&named)),
                "project {entry}: {hint:?}"
            );
        }
        for entry in GLOBAL_DIRS {
            let home = temp();
            let project = temp();
            put(home.path(), entry, Some("0.14.0"));

            let hint = run(project.path(), Some(home.path()), Some(project.path()));

            let named = format!("the asqr skill in ~/{entry}/asqr is from asqr 0.14.0");
            assert!(
                hint.as_deref().is_some_and(|hint| hint.starts_with(&named)),
                "global {entry}: {hint:?}"
            );
        }
    }

    #[test]
    fn without_a_home_only_project_copies_are_searched() {
        let project = temp();
        put(project.path(), ".claude/skills", Some("0.14.0"));

        let hint = run(project.path(), None, Some(project.path()));

        assert_eq!(hint.as_deref(), Some(ONE_OLD_COPY));
    }

    // =====================================================================
    // The versions that count as older
    // =====================================================================

    #[test]
    fn a_copy_of_the_running_version_gives_no_hint() {
        let project = temp();
        put(project.path(), ".claude/skills", Some(RUNNING));

        assert_eq!(run(project.path(), None, Some(project.path())), None);
    }

    #[test]
    fn a_newer_copy_gives_no_hint() {
        let project = temp();
        put(project.path(), ".claude/skills", Some("0.16.0"));

        assert_eq!(run(project.path(), None, Some(project.path())), None);
    }

    #[test]
    fn a_copy_with_a_stamp_that_is_no_version_gives_no_hint() {
        let project = temp();
        put(project.path(), ".claude/skills", Some("banana"));

        assert_eq!(run(project.path(), None, Some(project.path())), None);
    }

    #[test]
    fn a_current_copy_does_not_hide_an_older_one() {
        let project = temp();
        put(project.path(), ".agents/skills", Some(RUNNING));
        put(project.path(), ".claude/skills", Some("0.14.0"));

        let hint = run(project.path(), None, Some(project.path()));

        assert_eq!(hint.as_deref(), Some(ONE_OLD_COPY));
    }

    #[test]
    fn versions_compare_by_number_and_not_by_text() {
        assert!(Version(0, 9, 2) < Version(0, 14, 0));
        assert_eq!(parse_version("0.9.2"), Some(Version(0, 9, 2)));
        assert_eq!(parse_version("0.14.0"), Some(Version(0, 14, 0)));
    }

    #[test]
    fn a_pre_release_or_build_suffix_does_not_count() {
        assert_eq!(parse_version("0.15.0-rc.1"), Some(Version(0, 15, 0)));
        assert_eq!(parse_version("0.15.0+abc"), Some(Version(0, 15, 0)));
    }

    #[test]
    fn a_text_that_is_no_three_part_version_is_refused() {
        for text in [
            "", "banana", "0.14", "0.14.0.1", "0.-1.0", "0.+1.0", "0..0", "v0.14.0",
        ] {
            assert_eq!(parse_version(text), None, "{text:?}");
        }
    }

    // =====================================================================
    // The stamp in a SKILL.md
    // =====================================================================

    fn version_of(text: &str) -> Option<Stamp> {
        stamp_of(text)
    }

    const V014: Option<Stamp> = Some(Stamp::Version(Version(0, 14, 0)));

    #[test]
    fn the_stamp_is_read_under_metadata() {
        assert_eq!(version_of(&skill_md(Some("0.14.0"))), V014);
    }

    #[test]
    fn a_stamp_in_single_quotes_counts() {
        assert_eq!(version_of("---\nasqr-version: '0.14.0'\n---\n"), V014);
    }

    #[test]
    fn a_stamp_without_quotes_counts() {
        assert_eq!(version_of("---\nasqr-version: 0.14.0\n---\n"), V014);
    }

    #[test]
    fn a_stamp_at_the_top_level_counts() {
        assert_eq!(
            version_of("---\nname: asqr\nasqr-version: \"0.14.0\"\n---\n"),
            V014
        );
    }

    #[test]
    fn a_stamp_in_a_file_with_windows_line_endings_counts() {
        assert_eq!(
            version_of("---\r\nmetadata:\r\n  asqr-version: \"0.14.0\"\r\n---\r\n\r\nBody\r\n"),
            V014
        );
    }

    #[test]
    fn a_frontmatter_without_a_stamp_is_unstamped() {
        assert_eq!(version_of(&skill_md(None)), Some(Stamp::Unstamped));
    }

    #[test]
    fn a_text_without_frontmatter_is_unstamped() {
        assert_eq!(version_of("# asqr\n"), Some(Stamp::Unstamped));
    }

    #[test]
    fn a_frontmatter_that_never_ends_is_unstamped() {
        assert_eq!(
            version_of("---\nasqr-version: \"0.14.0\"\n"),
            Some(Stamp::Unstamped)
        );
    }

    #[test]
    fn a_line_in_the_body_is_no_stamp() {
        assert_eq!(
            version_of("---\nname: asqr\n---\n\nasqr-version: \"0.14.0\"\n"),
            Some(Stamp::Unstamped)
        );
    }

    #[test]
    fn another_key_that_starts_alike_is_no_stamp() {
        assert_eq!(
            version_of("---\nasqr-version-of-the-docs: \"0.14.0\"\n---\n"),
            Some(Stamp::Unstamped)
        );
    }

    #[test]
    fn a_stamp_that_is_no_version_is_refused() {
        assert_eq!(version_of("---\nasqr-version: \"banana\"\n---\n"), None);
        assert_eq!(version_of("---\nasqr-version: \"\"\n---\n"), None);
    }

    #[test]
    fn the_skill_this_asqr_ships_carries_its_own_version() {
        let running = parse_version(env!("CARGO_PKG_VERSION")).expect("a crate version");

        assert_eq!(
            stamp_of(super::super::skill::SKILL),
            Some(Stamp::Version(running))
        );
    }

    // =====================================================================
    // How a copy is shown
    // =====================================================================

    fn one_copy(shown: Shown) -> String {
        hint_text(
            &[OldCopy {
                shown,
                from: Stamp::Unstamped,
            }],
            RUNNING,
        )
    }

    #[test]
    fn a_path_with_a_space_is_quoted_in_the_install_command() {
        let hint = one_copy(Shown::Absolute("/work/my project/.claude/skills".into()));

        assert!(hint.starts_with("the asqr skill in /work/my project/.claude/skills/asqr is"));
        assert!(hint.ends_with("`asqr skill --install '/work/my project/.claude/skills'`"));
    }

    #[test]
    fn the_tilde_stays_outside_the_quotes_of_a_home_path() {
        let hint = one_copy(Shown::Home("my project/.claude/skills".into()));

        assert!(hint.ends_with("`asqr skill --install ~/'my project/.claude/skills'`"));
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_no_utf_8_shows_as_the_placeholder_in_the_install_command() {
        use std::os::unix::ffi::OsStrExt;

        let odd = PathBuf::from(std::ffi::OsStr::from_bytes(
            b"/work/gr\xf6\xdfe/.claude/skills",
        ));
        for shown in [Shown::Absolute(odd.clone()), Shown::Home(odd)] {
            assert!(one_copy(shown).ends_with("`asqr skill --install <dir>`"));
        }
    }

    #[test]
    fn a_control_character_in_a_path_cannot_break_the_line() {
        let hint = one_copy(Shown::Absolute("/work/a\nb/.claude/skills".into()));

        assert!(!hint.contains('\n'), "{hint:?}");
        assert!(
            hint.contains("/work/a\u{fffd}b/.claude/skills/asqr is"),
            "{hint:?}"
        );
        assert!(hint.ends_with("`asqr skill --install <dir>`"), "{hint:?}");
    }

    #[test]
    fn a_relative_directory_that_starts_with_a_dash_is_not_read_as_a_flag() {
        let hint = one_copy(Shown::Relative("-odd/skills".into()));

        assert!(
            hint.ends_with("`asqr skill --install ./-odd/skills`"),
            "{hint}"
        );
    }

    #[test]
    fn a_project_copy_above_the_current_directory_is_shown_absolute() {
        let outer = temp();
        let project = outer.path().join("project");
        let sub = project.join("sub");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        let home = temp();
        put(&project, ".claude/skills", Some("0.14.0"));

        let hint = run(&sub, Some(home.path()), Some(&project)).expect("a hint");

        let at = project.join(".claude/skills");
        assert!(
            hint.starts_with(&format!(
                "the asqr skill in {}/asqr is from asqr 0.14.0",
                at.display()
            )),
            "{hint}"
        );
        assert!(
            hint.ends_with(&format!("`asqr skill --install {}`", at.display())),
            "{hint}"
        );
    }

    #[test]
    fn a_project_copy_above_the_current_directory_under_home_is_shown_with_a_tilde() {
        let home = temp();
        let project = home.path().join("project");
        let sub = project.join("sub");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        put(&project, ".claude/skills", Some("0.14.0"));

        let hint = run(&sub, Some(home.path()), Some(&project)).expect("a hint");

        assert!(
            hint.starts_with("the asqr skill in ~/project/.claude/skills/asqr is from asqr 0.14.0"),
            "{hint}"
        );
        assert!(
            hint.ends_with("`asqr skill --install ~/project/.claude/skills`"),
            "{hint}"
        );
    }

    // =====================================================================
    // Where the walk goes
    // =====================================================================

    #[test]
    fn a_copy_above_the_git_root_is_not_found() {
        let outer = temp();
        let project = outer.path().join("project");
        let sub = project.join("sub");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        put(outer.path(), ".claude/skills", Some("0.14.0"));

        assert_eq!(run(&sub, None, Some(&project)), None);
    }

    #[test]
    fn without_a_git_root_only_the_current_directory_is_searched() {
        let project = temp();
        let sub = project.path().join("sub");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        put(project.path(), ".claude/skills", Some("0.14.0"));

        assert_eq!(run(&sub, None, None), None);
    }

    #[test]
    fn a_stop_that_is_no_ancestor_of_the_current_directory_searches_it_alone() {
        let project = temp();
        let sub = project.path().join("sub");
        let elsewhere = temp();
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        put(project.path(), ".claude/skills", Some("0.14.0"));

        assert_eq!(run(&sub, None, Some(elsewhere.path())), None);
    }

    #[test]
    fn the_current_directory_is_searched_before_those_above() {
        let project = temp();
        let sub = project.path().join("sub");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        put(project.path(), ".agents/skills", Some("0.13.0"));
        put(&sub, ".claude/skills", Some("0.14.0"));

        let hint = run(&sub, None, Some(project.path())).expect("a hint");

        let here = hint.find(".claude/skills/asqr (from asqr 0.14.0)");
        let above = hint.find("/.agents/skills/asqr (from asqr 0.13.0)");
        assert!(here.is_some() && above.is_some() && here < above, "{hint}");
    }

    #[test]
    fn a_git_directory_marks_the_root() {
        let root = temp();
        let sub = root.path().join("a/b");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        std::fs::create_dir(root.path().join(".git")).expect("a creatable directory");

        assert_eq!(
            git_root(&sub, Some(root.path())),
            Some(root.path().to_owned())
        );
    }

    #[test]
    fn a_git_file_of_a_linked_worktree_marks_the_root() {
        let root = temp();
        let sub = root.path().join("a/b");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        std::fs::write(
            root.path().join(".git"),
            "gitdir: /elsewhere/.git/worktrees/x\n",
        )
        .expect("a writable file");

        assert_eq!(
            git_root(&sub, Some(root.path())),
            Some(root.path().to_owned())
        );
    }

    #[test]
    fn the_nearest_git_root_wins() {
        let outer = temp();
        let inner = outer.path().join("inner");
        let sub = inner.join("sub");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        std::fs::create_dir(outer.path().join(".git")).expect("a creatable directory");
        std::fs::create_dir(inner.join(".git")).expect("a creatable directory");

        assert_eq!(git_root(&sub, Some(outer.path())), Some(inner));
    }

    #[test]
    fn the_git_search_does_not_go_above_its_ceiling() {
        let outer = temp();
        let inner = outer.path().join("inner");
        std::fs::create_dir_all(&inner).expect("a creatable directory");
        std::fs::create_dir(outer.path().join(".git")).expect("a creatable directory");

        assert_eq!(git_root(&inner, Some(&inner)), None);
    }

    #[test]
    fn a_git_file_stops_the_walk_below_a_repository() {
        let outer = temp();
        let root = outer.path().join("worktree");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).expect("a creatable directory");
        std::fs::create_dir(outer.path().join(".git")).expect("a creatable directory");
        std::fs::write(root.join(".git"), "gitdir: /elsewhere\n").expect("a writable file");
        put(&root, ".claude/skills", Some("0.14.0"));
        put(outer.path(), ".agents/skills", Some("0.13.0"));

        let stop = git_root(&sub, Some(outer.path()));
        let hint = run(&sub, None, stop.as_deref()).expect("a hint");

        assert!(
            hint.contains("/.claude/skills/asqr is from asqr 0.14.0"),
            "{hint}"
        );
        assert!(!hint.contains(".agents"), "{hint}");
    }

    // =====================================================================
    // One copy, however many ways lead to it
    // =====================================================================

    #[cfg(unix)]
    #[test]
    fn a_copy_reached_through_a_symlink_is_named_once() {
        let project = temp();
        let real = put(project.path(), ".agents/skills", Some("0.14.0"));
        std::fs::create_dir_all(project.path().join(".claude/skills"))
            .expect("a creatable directory");
        std::os::unix::fs::symlink(&real, project.path().join(".claude/skills/asqr"))
            .expect("a symlink");

        let hint = run(project.path(), None, Some(project.path()));

        let hint = hint.expect("a hint");
        assert!(
            hint.starts_with("the asqr skill in .agents/skills/asqr is from"),
            "{hint}"
        );
        assert!(!hint.contains(".claude"), "{hint}");
    }

    #[cfg(unix)]
    #[test]
    fn a_skills_directory_that_is_a_symlink_is_named_once() {
        let project = temp();
        put(project.path(), ".agents/skills", Some("0.14.0"));
        std::fs::create_dir_all(project.path().join(".claude")).expect("a creatable directory");
        std::os::unix::fs::symlink(
            project.path().join(".agents/skills"),
            project.path().join(".claude/skills"),
        )
        .expect("a symlink");

        let hint = run(project.path(), None, Some(project.path())).expect("a hint");

        assert!(
            hint.starts_with("the asqr skill in .agents/skills/asqr is from"),
            "{hint}"
        );
        assert!(!hint.contains(".claude"), "{hint}");
    }

    #[cfg(unix)]
    #[test]
    fn a_skill_file_shared_by_two_copies_is_named_once() {
        let project = temp();
        let real = put(project.path(), ".agents/skills", Some("0.14.0"));
        let other = project.path().join(".claude/skills/asqr");
        std::fs::create_dir_all(&other).expect("a creatable directory");
        std::os::unix::fs::symlink(real.join("SKILL.md"), other.join("SKILL.md"))
            .expect("a symlink");

        let hint = run(project.path(), None, Some(project.path())).expect("a hint");

        assert!(
            hint.starts_with("the asqr skill in .agents/skills/asqr is from"),
            "{hint}"
        );
        assert!(!hint.contains(".claude"), "{hint}");
    }

    #[test]
    fn two_separate_copies_of_one_version_are_both_named() {
        let project = temp();
        put(project.path(), ".agents/skills", Some("0.14.0"));
        put(project.path(), ".claude/skills", Some("0.14.0"));

        let hint = run(project.path(), None, Some(project.path())).expect("a hint");

        assert!(hint.contains(".agents/skills/asqr (from"), "{hint}");
        assert!(hint.contains(".claude/skills/asqr (from"), "{hint}");
    }

    #[test]
    fn a_project_that_is_the_home_directory_names_a_copy_once() {
        let home = temp();
        put(home.path(), ".claude/skills", Some("0.14.0"));

        let hint = run(home.path(), Some(home.path()), Some(home.path())).expect("a hint");

        assert!(
            hint.starts_with("the asqr skill in .claude/skills/asqr is from"),
            "{hint}"
        );
        assert!(!hint.contains("copies"), "{hint}");
    }

    // =====================================================================
    // What cannot be read gives no hint
    // =====================================================================

    #[test]
    fn a_directory_without_a_skill_file_gives_no_hint() {
        let project = temp();
        std::fs::create_dir_all(project.path().join(".claude/skills/asqr"))
            .expect("a creatable directory");

        assert_eq!(run(project.path(), None, Some(project.path())), None);
    }

    #[test]
    fn a_skill_file_that_is_a_directory_gives_no_hint() {
        let project = temp();
        std::fs::create_dir_all(project.path().join(".claude/skills/asqr/SKILL.md"))
            .expect("a creatable directory");

        assert_eq!(run(project.path(), None, Some(project.path())), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_leads_nowhere_gives_no_hint() {
        let project = temp();
        std::fs::create_dir_all(project.path().join(".claude/skills"))
            .expect("a creatable directory");
        std::os::unix::fs::symlink(
            project.path().join("missing"),
            project.path().join(".claude/skills/asqr"),
        )
        .expect("a symlink");

        assert_eq!(run(project.path(), None, Some(project.path())), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_skill_file_that_links_to_itself_gives_no_hint() {
        let project = temp();
        let dir = project.path().join(".claude/skills/asqr");
        std::fs::create_dir_all(&dir).expect("a creatable directory");
        std::os::unix::fs::symlink(dir.join("SKILL.md"), dir.join("SKILL.md")).expect("a symlink");

        assert_eq!(run(project.path(), None, Some(project.path())), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_skill_file_without_read_permission_gives_no_hint() {
        use std::os::unix::fs::PermissionsExt;

        let project = temp();
        let dir = put(project.path(), ".claude/skills", Some("0.14.0"));
        let file = dir.join("SKILL.md");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000))
            .expect("changeable permissions");
        // A superuser reads it anyway, and then the copy is old.
        let readable = std::fs::File::open(&file).is_ok();

        let hint = run(project.path(), None, Some(project.path()));

        assert_eq!(hint.is_some(), readable);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_as_skill_file_does_not_block_the_check() {
        let project = temp();
        let dir = project.path().join(".claude/skills/asqr");
        std::fs::create_dir_all(&dir).expect("a creatable directory");
        let made = std::process::Command::new("mkfifo")
            .arg(dir.join("SKILL.md"))
            .status()
            .expect("mkfifo runs");
        assert!(made.success());

        let path = project.path().to_owned();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(run(&path, None, Some(&path)));
        });
        let hint = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the check must not block on a FIFO");

        assert_eq!(hint, None);
    }

    // =====================================================================
    // How much of a file is read
    // =====================================================================

    #[test]
    fn a_stamp_is_found_when_a_multibyte_character_ends_the_prefix_read() {
        let project = temp();
        let dir = project.path().join(".claude/skills/asqr");
        std::fs::create_dir_all(&dir).expect("a creatable directory");
        // The arrow takes three bytes and starts one byte before the end
        // of the prefix.
        let mut text = skill_md(Some("0.14.0"));
        text.push_str(&"a".repeat(PREFIX - text.len() - 1));
        text.push('\u{2192}');
        text.push_str("tail");
        std::fs::write(dir.join("SKILL.md"), text).expect("a writable file");

        let hint = run(project.path(), None, Some(project.path()));

        assert_eq!(hint.as_deref(), Some(ONE_OLD_COPY));
    }

    #[test]
    fn a_stamp_is_found_when_the_body_is_no_utf_8() {
        let project = temp();
        let dir = project.path().join(".claude/skills/asqr");
        std::fs::create_dir_all(&dir).expect("a creatable directory");
        let mut bytes = skill_md(Some("0.14.0")).into_bytes();
        bytes.extend_from_slice(b"gr\xf6\xdfe");
        std::fs::write(dir.join("SKILL.md"), bytes).expect("a writable file");

        let hint = run(project.path(), None, Some(project.path()));

        assert_eq!(hint.as_deref(), Some(ONE_OLD_COPY));
    }

    #[test]
    fn a_stamp_after_the_prefix_is_not_read() {
        let project = temp();
        let dir = project.path().join(".claude/skills/asqr");
        std::fs::create_dir_all(&dir).expect("a creatable directory");
        let mut text = String::from("---\nname: asqr\n");
        text.push_str(&"# padding\n".repeat(PREFIX / 10 + 1));
        text.push_str("asqr-version: \"0.14.0\"\n---\n");
        std::fs::write(dir.join("SKILL.md"), text).expect("a writable file");

        let hint = run(project.path(), None, Some(project.path())).expect("a hint");

        assert!(hint.contains("is from an older asqr"), "{hint}");
    }
}
