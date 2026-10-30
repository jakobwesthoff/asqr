// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Advice for agents on stderr (spec section 6.7): the texts of the hints
//! and of the errors, warnings and notices that carry advice, and the one
//! function that prints them. stdout never changes for a hint.

use std::fmt;
use std::io::Write;
use std::path::Path;

use crate::queue::QueueLocation;

/// The timeout of every wait a hint names on its own: the one the skill's
/// example shows, so the hint and the skill teach the same command.
pub(super) const WAIT_TIMEOUT: u64 = 540;

// =========================================================================
// Lines and how they are printed
// =========================================================================

/// One line on stderr: a hint of its own, or an error, warning or notice
/// whose message carries advice.
pub(super) struct Line {
    prefix: &'static str,
    text: String,
}

impl Line {
    fn hint(text: String) -> Self {
        Line {
            prefix: "hint",
            text,
        }
    }

    fn notice(text: String) -> Self {
        Line {
            prefix: "asqr",
            text,
        }
    }

    fn warning(text: String) -> Self {
        Line {
            prefix: "warning",
            text,
        }
    }

    /// Prints the line on stderr. A stderr that cannot be written, such as
    /// a pipe closed by the reader, is ignored: the line is advice, and the
    /// command it follows has already done its work, such as dropping a
    /// session, which must not end in a panic.
    pub(super) fn print(&self) {
        print_to(&mut std::io::stderr().lock(), self);
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.prefix, self.text)
    }
}

fn print_to(out: &mut impl Write, line: &Line) {
    let _ = writeln!(out, "{line}");
}

// =========================================================================
// How a hint names the queue
// =========================================================================

/// How hints name the queue a command worked on.
pub(super) struct Invocation {
    /// The command that reaches the queue: `asqr`, `asqr --queue <name>`
    /// or `asqr --dir <path>`.
    command: String,
    /// The queue as messages name it: its name, or its absolute directory.
    queue: String,
}

impl Invocation {
    /// `chosen` says whether a flag or a variable chose the queue. Plain
    /// `asqr` reaches the queue only when neither did; otherwise the
    /// command carries the queue as a flag, also one a variable chose,
    /// since the agent's next shell, or the person's terminal, may not
    /// have the variable.
    pub(super) fn new(location: &QueueLocation, chosen: bool) -> Self {
        // The agent or the person may run the command elsewhere, so a
        // directory is named absolute. `absolute` fails only for an empty
        // path, which clap refuses, or a current directory that cannot be
        // read; the directory as given is the best name left then.
        let dir = std::path::absolute(location.dir()).unwrap_or_else(|_| location.dir().into());
        let command = match (chosen, location.name()) {
            (false, _) => "asqr".to_owned(),
            // clap reads a value that starts with `-` as a flag, unless it
            // is joined to its flag.
            (true, Some(name)) if name.starts_with('-') => format!("asqr --queue={name}"),
            (true, Some(name)) => format!("asqr --queue {name}"),
            (true, None) => format!("asqr --dir {}", shell_word(&dir, "<dir>")),
        };
        let queue = location
            .name()
            .map_or_else(|| dir.display().to_string(), str::to_owned);
        Invocation { command, queue }
    }
}

/// `path` as one word a shell reads back as the same path: as it is when
/// it holds only ASCII letters, digits and `_./-`, otherwise in single
/// quotes. A path that cannot be written that way, not UTF-8 or holding a
/// control character such as a newline, shows as `placeholder`.
fn shell_word(path: &Path, placeholder: &str) -> String {
    let Some(text) = path
        .to_str()
        .filter(|text| !text.contains(char::is_control))
    else {
        return placeholder.to_owned();
    };
    // A relative path that starts with `-` would read as a flag.
    let text = if path.is_relative() && text.starts_with('-') {
        format!("./{text}")
    } else {
        text.to_owned()
    };
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-');
    if !text.is_empty() && text.chars().all(plain) {
        text
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

// =========================================================================
// The hints of `ask`
// =========================================================================

/// After `ask` without `--wait` dropped session `id` (A1). Every session
/// gets it, one of info items only too, since the person can answer any
/// item with a note.
pub(super) fn wait_for_it(invocation: &Invocation, id: &str) -> Line {
    Line::hint(format!(
        "the person can answer any item with a note; wait for the result and read all of it: `{} wait {id} --timeout {WAIT_TIMEOUT}`, or a lower timeout if your command limit is shorter",
        invocation.command
    ))
}

/// After `ask` replaced the waiting session `id` with the new file (A3).
/// "No answer" rather than "not started": the person may have the session
/// open, and a reject reason counts as no answer.
pub(super) fn replaced_waiting(id: &str) -> Line {
    Line::notice(format!(
        "replaced the waiting session {id}, in which the person had entered no answer yet; they see the new file in its place"
    ))
}

/// After `ask` dropped a session into a queue no asqr watches (A6). The
/// session waits there either way, but only the person can start asqr.
pub(super) fn nobody_watching(invocation: &Invocation) -> Line {
    Line::warning(format!(
        "no asqr is watching queue {}; tell the person to start `{}` in a terminal of their own; the session waits in the queue until then",
        invocation.queue, invocation.command
    ))
}

/// After `ask` dropped session `id` from `file` and warned about the file
/// `count` times (A2), in place of [`wait_for_it`]. Some warnings are
/// harmless, so waiting is one of the two ways. `still_waiting` is the
/// form for `ask --wait` whose wait ran out: the agent reads the line only
/// then, and is already waiting.
pub(super) fn warnings_above(
    invocation: &Invocation,
    count: usize,
    file: &Path,
    id: &str,
    id_in_file: bool,
    still_waiting: bool,
) -> Line {
    let command = &invocation.command;
    let file = shell_word(file, "<file>");
    let warnings = if count == 1 {
        "1 warning above".to_owned()
    } else {
        format!("{count} warnings above")
    };
    let waits = if still_waiting {
        "the session still waits; "
    } else {
        ""
    };
    let which = if count == 1 { "it" } else { "one" };
    // A file with an `id` replaces its waiting session by being asked
    // again; without one, `ask` assigned a fresh id, so only `--replaces`
    // takes the place of this session.
    let fix = match (id_in_file, still_waiting) {
        (false, false) => {
            format!("fix the file and replace this session: `{command} ask {file} --replaces {id}`")
        }
        (false, true) => {
            format!("fix the file and replace it: `{command} ask {file} --replaces {id}`")
        }
        (true, false) => format!(
            "fix the file, keep its `id`, and ask again: `{command} ask {file}`, which replaces this session while the person has not started on it"
        ),
        (true, true) => format!(
            "fix the file, keep its `id`, and ask again: `{command} ask {file}`, which replaces it"
        ),
    };
    let otherwise = if still_waiting {
        String::new()
    } else {
        format!("; otherwise wait for its result: `{command} wait {id} --timeout {WAIT_TIMEOUT}`")
    };
    Line::hint(format!(
        "{warnings}; {waits}if {which} matters to the person, {fix}{otherwise}"
    ))
}

// =========================================================================
// The hints of `wait` and `result`
// =========================================================================

/// The command whose wait ran out.
pub(super) enum Caller {
    /// `wait` or `ask --wait`, with their `--timeout` in seconds.
    Wait(u64),
    /// `result`, which looks once.
    Result,
}

/// The session `id` still waits for the person when the caller's timeout
/// passed (W1). The way on is `wait`, also after `ask --wait`: asking again
/// would replace the session or be refused.
pub(super) fn no_result_yet(invocation: &Invocation, id: &str, caller: Caller) -> Line {
    let command = &invocation.command;
    // A timeout that ended with this line fit the caller's command limit,
    // so it is the one number known to work. A timeout of 0 only looks
    // once, so the hint names the skill's wait then.
    Line::notice(match caller {
        Caller::Wait(secs) if secs > 0 => format!(
            "no result for {id} yet (timeout); the person has not answered yet, so wait again with `{command} wait {id} --timeout {secs}`"
        ),
        Caller::Wait(_) => format!(
            "no result for {id} yet (timeout); the person has not answered yet, so wait again with `{command} wait {id} --timeout {WAIT_TIMEOUT}`, or a lower timeout if your command limit is shorter"
        ),
        Caller::Result => format!(
            "no result for {id} yet; the person has not answered yet, so wait for it with `{command} wait {id} --timeout {WAIT_TIMEOUT}`, or a lower timeout if your command limit is shorter"
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_line_prints_its_prefix_and_text_on_one_line() {
        let mut out = Vec::new();

        print_to(&mut out, &Line::hint("wait for it".into()));

        assert_eq!(out, b"hint: wait for it\n");
    }

    /// A writer whose every write fails, as a closed pipe does.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn a_line_that_cannot_be_written_is_dropped_without_a_panic() {
        print_to(&mut Closed, &Line::hint("wait for it".into()));
    }

    fn word(path: &str) -> String {
        shell_word(Path::new(path), "<file>")
    }

    #[test]
    fn a_plain_path_stands_as_it_is() {
        assert_eq!(word("/tmp/q-1/session_2.json"), "/tmp/q-1/session_2.json");
    }

    #[test]
    fn a_path_with_a_space_is_quoted() {
        assert_eq!(word("/tmp/my queue"), "'/tmp/my queue'");
    }

    #[test]
    fn a_quote_in_a_path_is_closed_escaped_and_reopened() {
        assert_eq!(word("/tmp/it's"), r"'/tmp/it'\''s'");
    }

    #[test]
    fn a_path_with_a_letter_beyond_ascii_is_quoted() {
        assert_eq!(word("/tmp/größe"), "'/tmp/größe'");
    }

    #[test]
    fn a_relative_path_that_starts_with_a_dash_gets_a_dot_slash() {
        assert_eq!(word("-v2.json"), "./-v2.json");
    }

    #[test]
    fn a_path_with_a_control_character_shows_as_the_placeholder() {
        assert_eq!(word("/tmp/a\nb"), "<file>");
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_no_utf_8_shows_as_the_placeholder() {
        use std::os::unix::ffi::OsStrExt;

        let latin1 = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/gr\xf6\xdfe"));
        assert_eq!(shell_word(latin1, "<dir>"), "<dir>");
    }

    #[test]
    fn the_empty_path_is_an_empty_quoted_word() {
        assert_eq!(word(""), "''");
    }

    fn command(location: &QueueLocation, chosen: bool) -> String {
        Invocation::new(location, chosen).command
    }

    fn named(name: &str) -> QueueLocation {
        let flags = crate::queue::Selection {
            queue: Some(name.into()),
            dir: None,
        };
        QueueLocation::resolve(
            flags,
            crate::queue::Selection::default(),
            Some(PathBuf::from("/data/asqr")),
        )
        .expect("a valid queue name")
    }

    #[test]
    fn a_queue_nothing_chose_is_reached_by_plain_asqr() {
        assert_eq!(command(&named("default"), false), "asqr");
    }

    #[test]
    fn a_chosen_queue_is_named_by_its_flag() {
        assert_eq!(command(&named("work"), true), "asqr --queue work");
        assert_eq!(command(&named("-scratch"), true), "asqr --queue=-scratch");
    }

    #[test]
    fn a_queue_directory_is_named_absolute() {
        let relative = QueueLocation::at("queues/work");
        let expected = std::env::current_dir()
            .expect("a current directory")
            .join("queues/work");

        assert_eq!(
            command(&relative, true),
            format!("asqr --dir {}", shell_word(&expected, "<dir>"))
        );
    }

    #[test]
    fn messages_name_a_queue_by_its_name_or_its_absolute_directory() {
        let relative = QueueLocation::at("queues/work");
        let expected = std::env::current_dir()
            .expect("a current directory")
            .join("queues/work");

        assert_eq!(Invocation::new(&named("work"), false).queue, "work");
        assert_eq!(
            Invocation::new(&relative, true).queue,
            expected.display().to_string()
        );
    }

    #[test]
    fn a_queue_directory_that_cannot_be_made_absolute_is_named_as_given() {
        assert_eq!(command(&QueueLocation::at(""), true), "asqr --dir ''");
    }

    #[test]
    fn the_warnings_hint_counts_one_warning_as_it_and_more_as_one() {
        let invocation = Invocation::new(&named("work"), true);
        let file = Path::new("session.json");

        let one = warnings_above(&invocation, 1, file, "batch-01", true, true).to_string();
        let two = warnings_above(&invocation, 2, file, "batch-01", false, false).to_string();

        assert_eq!(
            one,
            "hint: 1 warning above; the session still waits; if it matters to the person, fix the file, keep its `id`, and ask again: `asqr --queue work ask session.json`, which replaces it"
        );
        assert_eq!(
            two,
            "hint: 2 warnings above; if one matters to the person, fix the file and replace this session: `asqr --queue work ask session.json --replaces batch-01`; otherwise wait for its result: `asqr --queue work wait batch-01 --timeout 540`"
        );
    }

    #[test]
    fn the_wait_timeout_is_the_one_the_skill_shows() {
        let example = format!("asqr wait \"$id\" --timeout {WAIT_TIMEOUT}");

        assert!(super::super::skill::SKILL.contains(&example));
    }
}
