// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Advice for agents on stderr (spec section 6.7): the texts of the hints
//! and of the errors, warnings and notices that carry advice, and the one
//! function that prints them. stdout never changes for a hint. The one
//! exception is the hint about an older installed skill, whose text is in
//! [`super::skill_age`] with the search that finds the copies.

use std::fmt;
use std::io::Write;
use std::path::Path;

use crate::format::{SessionResult, Status};
use crate::queue::QueueLocation;

/// The timeout of every wait a hint names on its own: the one the skill's
/// example shows, so the hint and the skill teach the same command.
pub(super) const WAIT_TIMEOUT: u64 = 540;

/// The part of the skill that says how to use a withdrawn session's
/// `draft`, as `asqr skill <part>` names it.
const WITHDRAWING_PART: &str = "withdrawing";

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
    pub(super) fn hint(text: String) -> Self {
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

    fn error(text: String) -> Self {
        Line {
            prefix: "error",
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
            (true, Some(name)) => queue_command(name),
            (true, None) => format!("asqr --dir {}", shell_word(&dir, "<dir>")),
        };
        let queue = location
            .name()
            .map_or_else(|| dir.display().to_string(), str::to_owned);
        Invocation { command, queue }
    }
}

/// `asqr --queue <name>`, which reaches the named queue from anywhere.
fn queue_command(name: &str) -> String {
    // clap reads a value that starts with `-` as a flag, unless it is
    // joined to its flag.
    if name.starts_with('-') {
        format!("asqr --queue={name}")
    } else {
        format!("asqr --queue {name}")
    }
}

/// `path` as one word a shell reads back as the same path: as it is when
/// it holds only ASCII letters, digits and `_./-`, otherwise in single
/// quotes. A path that cannot be written that way, not UTF-8 or holding a
/// control character such as a newline, shows as `placeholder`.
pub(crate) fn shell_word(path: &Path, placeholder: &str) -> String {
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

/// `ask --replaces <old>` withdrew a session whose result holds a `draft`
/// (D2r): the person had started on it, and the new session starts empty.
pub(super) fn started_on(invocation: &Invocation, old: &str) -> Line {
    Line::hint(format!(
        "the person had started on {old}; what they entered is in the `draft` of its result: `{} result {old}`",
        invocation.command
    ))
}

/// A queue another asqr watches, as advice names it.
pub(super) struct WatchedQueue {
    pub name: String,
    /// The tmux session that asqr runs in, where tmux named one.
    pub session: Option<String>,
}

/// After `ask` dropped a session into a queue no asqr watches (A6): the
/// queues other asqr instances watch, and what the person can start. The
/// session waits there either way, and only the person starts asqr, so
/// the advice ends with that. `tmux_session` is the asker's tmux session
/// where tmux chose the queue: plain `asqr` in a window of that session
/// watches it.
pub(super) fn nobody_watching(
    invocation: &Invocation,
    watched: &[WatchedQueue],
    tmux_session: Option<&str>,
) -> Line {
    let queue = &invocation.queue;
    let named: Vec<String> = watched
        .iter()
        .map(|watched| match &watched.session {
            Some(session) => format!("{} (tmux session {session})", watched.name),
            None => watched.name.clone(),
        })
        .collect();
    let others = match named.split_last() {
        None => String::new(),
        Some((last, [])) => format!("asqr watches {last}; "),
        Some((last, others)) => format!("asqr watches {} and {last}; ", others.join(", ")),
    };
    let start = match tmux_session {
        Some(session) => format!(
            "`asqr` in a window of tmux session {session}, or `{}` in a terminal of their own",
            queue_command(queue)
        ),
        None => format!("`{}` in a terminal of their own", invocation.command),
    };
    Line::warning(format!(
        "no asqr is watching queue {queue}; {others}the session waits in {queue}: ask the person to start {start}; do not start it yourself"
    ))
}

/// After `ask` dropped a session into the queue tmux chose, which the asqr
/// of another tmux session watches (ADR 85): two session names that give
/// one queue name, or a pin left by an asqr that has quit.
pub(super) fn watched_from_another_session(
    invocation: &Invocation,
    watcher_session: &str,
    own_session: &str,
) -> Line {
    Line::warning(format!(
        "queue {} is watched by the asqr of tmux session {watcher_session:?}, not of this session ({own_session:?}); the session waits there: ask the person whether to answer it there, or to start `asqr --queue <another name>` in a window of this session; do not start it yourself",
        invocation.queue
    ))
}

/// `ASQR_QUEUE` or `ASQR_DIR` (`variable`) chose `queue` for `ask`, while
/// the asker's tmux session pins `pinned`, which `watched` says an asqr
/// watches. Only the agent's own environment has the variable, so the
/// agent passes the line on.
pub(super) fn environment_beside_pin(
    variable: &str,
    invocation: &Invocation,
    pinned: &str,
    watched: bool,
) -> Line {
    let pinned = if watched {
        format!("the asqr of this tmux session watches {pinned}")
    } else {
        format!("this tmux session's queue is {pinned}")
    };
    Line::hint(format!(
        "{variable} chose queue {}, but {pinned}; tell the person, who decides where you ask",
        invocation.queue
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

/// `ask` refused session `id` for an unread result under the same id
/// (A5). Reading it leaves it in the outbox, so the same `ask` would
/// refuse again: the ways past it are `--force` and a file without `id`.
/// Under `--replaces <old>`, `--force` would also withdraw `old` while the
/// person answers it, so it is named only as a warning.
pub(super) fn unread_result(invocation: &Invocation, id: &str, replaces: Option<&str>) -> Line {
    let read = format!(
        "an unread result for {id:?} is in the outbox; read it first with `{} result {id}`",
        invocation.command
    );
    Line::error(match replaces {
        None => format!(
            "{read}, then ask again with --force, which archives that result, or leave `id` out of the file to get a fresh id"
        ),
        Some(old) => format!(
            "{read}, then leave `id` out of the file to get a fresh id; --force would archive that result and also withdraw {old} even if the person is answering it"
        ),
    })
}

/// The command a refusal of a session being answered comes from.
pub(super) enum Refusing {
    Withdraw,
    AskReplaces,
}

/// Session `id` is being answered, so `withdraw` or `ask --replaces`
/// refused to take it away (D1). The text does not say that asqr tells
/// the person: their asqr shows a withdrawal only while it runs (spec
/// section 7.9).
pub(super) fn being_answered(invocation: &Invocation, id: &str, refusing: Refusing) -> Line {
    Line::error(match refusing {
        Refusing::Withdraw => format!(
            "session {id:?} is being answered; check with the person first, then `{} withdraw --force {id}` withdraws it, and the result's `draft` holds what they entered so far",
            invocation.command
        ),
        Refusing::AskReplaces => format!(
            "session {id:?} is being answered; check with the person first, then run this command again with --force, which withdraws it, and the `draft` of its result holds what they entered so far"
        ),
    })
}

// =========================================================================
// The hints of `wait` and `result`
// =========================================================================

/// Nothing in the queue and no other named queue has session `id` (W4):
/// the id is mistyped, or the session went to a queue directory, which
/// nothing searches.
pub(super) fn unknown_session(invocation: &Invocation, id: &str) -> Line {
    Line::error(format!(
        "no session {id:?} in queue {} or any other named queue; check the id with `{} status --since 1d`, and if you asked in a queue directory (--dir or ASQR_DIR), run this command again with that directory",
        invocation.queue, invocation.command
    ))
}

/// `wait` or `result` found session `id`, unknown in the queue it was run
/// on, in the one other named queue `found` (T2), and goes on there.
pub(super) fn found_elsewhere(invocation: &Invocation, id: &str, found: &str) -> Line {
    Line::warning(format!(
        "session {id} is in queue {found}, not in {}; using queue {found}",
        invocation.queue
    ))
}

/// Session `id`, unknown in the queue the command was run on, is in each
/// of the named queues `queues`.
pub(super) fn in_several_queues(id: &str, queues: &[String]) -> Line {
    Line::error(format!(
        "session {id:?} is in several queues: {}; run this command again with --queue and one of them",
        queues.join(", ")
    ))
}

/// `withdraw` found session `id` in another named queue only. A session
/// there may be another agent's, so `withdraw` takes nothing back on its
/// own.
pub(super) fn withdraw_elsewhere(invocation: &Invocation, id: &str, found: &str) -> Line {
    Line::error(format!(
        "session {id:?} is in queue {found}, not in {}; if it is yours, withdraw it with `{} withdraw {id}`",
        invocation.queue,
        queue_command(found)
    ))
}

/// `ask --replaces <old>` found `old` in another named queue only. The new
/// session belongs where the person watches now, so the old one is
/// withdrawn on its own and the new one asked without `--replaces`.
pub(super) fn replaces_elsewhere(invocation: &Invocation, old: &str, found: &str) -> Line {
    Line::error(format!(
        "session {old:?} is in queue {found}, not in {}; withdraw it with `{} withdraw {old}`, then ask without --replaces",
        invocation.queue,
        queue_command(found)
    ))
}

/// Prints the hints for `result` on stderr after its JSON, `printed`, went
/// to stdout. A result's JSON ends without a newline, so where a terminal,
/// `2>&1` or an agent's tool shows both streams as one text, the first
/// hint would follow the closing brace; an empty line first gives it a
/// line of its own.
pub(super) fn after_result(printed: &[u8], result: &SessionResult) {
    let lines = result_hints(result);
    if lines.is_empty() {
        return;
    }
    let mut stderr = std::io::stderr().lock();
    if !printed.ends_with(b"\n") {
        let _ = writeln!(stderr);
    }
    for line in &lines {
        print_to(&mut stderr, line);
    }
}

/// The hints a printed result gets, in the order they print.
fn result_hints(result: &SessionResult) -> Vec<Line> {
    outcome(result).into_iter().chain(notes(result)).collect()
}

/// What a result of a session that was not submitted means: a rejection
/// or a refusal (W3), or what a withdrawn session's `draft` is for (D2).
fn outcome(result: &SessionResult) -> Option<Line> {
    if result.status == Status::Withdrawn && result.draft.is_some() {
        return Some(Line::hint(format!(
            "`draft` holds what the person entered and never sent; these are not decisions, so use them only as presets in a replacement session (`asqr skill {WITHDRAWING_PART}` says how) and ignore them otherwise"
        )));
    }
    let text = match result.status {
        // `reason` is optional, and absent from the JSON when the person
        // gave none.
        Status::Cancelled if result.reason.is_some() => {
            "the person rejected the session; read `reason` for why, and treat the answers in it as never submitted, not as decisions"
        }
        Status::Cancelled => {
            "the person rejected the session without a reason; treat the answers in it as never submitted, not as decisions"
        }
        // An error result also comes from a newer asqr that dropped the
        // file's format version and from a file dropped by hand, so "fix
        // the file" comes first.
        Status::Error => {
            "the asqr watching the queue refused the session file; `error` says why; fix the file and ask again, or, if `error` names a kind or type that `asqr skill` describes, that asqr is older than this one: tell the person, and ask again once they updated or restarted it"
        }
        _ => return None,
    };
    Some(Line::hint(text.to_owned()))
}

/// The person wrote notes in a submitted or rejected result (W2), on the
/// session or on questions, named in session order. `reason`, `draft` and
/// `custom` do not count.
fn notes(result: &SessionResult) -> Option<Line> {
    let then = match result.status {
        Status::Submitted => "before you act on the answers",
        // A rejected result's answers were never submitted.
        Status::Cancelled => "before you go on",
        _ => return None,
    };
    let written =
        |note: &Option<String>| note.as_deref().is_some_and(|note| !note.trim().is_empty());
    let on_session = written(&result.note);
    let questions: Vec<String> = result
        .answers
        .iter()
        .filter(|answer| written(&answer.note))
        .map(|answer| format!("`{}`", answer.question))
        .collect();
    let count = usize::from(on_session) + questions.len();
    if count == 0 {
        return None;
    }
    const LISTED: usize = 5;
    let mut listed = questions
        .iter()
        .take(LISTED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if questions.len() > LISTED {
        listed += &format!(" and {} more", questions.len() - LISTED);
    }
    let place = match (on_session, questions.is_empty()) {
        (true, true) => "the session".to_owned(),
        (true, false) => format!("the session and on {listed}"),
        (false, _) => listed,
    };
    let (noun, pronoun) = if count == 1 {
        ("a note", "it")
    } else {
        ("notes", "them")
    };
    Some(Line::hint(format!(
        "the person wrote {noun} on {place}; read {pronoun} {then}"
    )))
}

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
    fn the_notes_hint_lists_five_questions_and_counts_the_rest() {
        let answers = (1..=7)
            .map(|n| crate::format::Answer {
                note: Some(format!("note {n}")),
                ..crate::format::Answer::new(format!("q{n}"))
            })
            .collect();
        let result =
            SessionResult::submitted("notes", "9f2c", "2026-10-30T12:00:00+01:00", None, answers);

        assert_eq!(
            notes(&result).expect("a hint").to_string(),
            "hint: the person wrote notes on `q1`, `q2`, `q3`, `q4`, `q5` and 2 more; read them before you act on the answers"
        );
    }

    #[test]
    fn every_part_a_hint_names_is_a_part_of_the_skill() {
        let withdrawn = SessionResult::withdrawn(
            "w",
            "9f2c",
            "2026-10-30T12:00:00+01:00",
            Some(crate::format::UnsentDraft::default()),
        );
        let hint = outcome(&withdrawn).expect("a hint").to_string();

        assert!(hint.contains(&format!("`asqr skill {WITHDRAWING_PART}`")));
        assert!(
            super::super::skill::PARTS
                .iter()
                .any(|(name, _)| *name == WITHDRAWING_PART)
        );
    }

    fn watched(name: &str, session: Option<&str>) -> WatchedQueue {
        WatchedQueue {
            name: name.into(),
            session: session.map(str::to_owned),
        }
    }

    #[test]
    fn the_warning_names_the_watched_queues_and_their_sessions() {
        let invocation = Invocation::new(&named("playground"), false);
        let others = [
            watched("asqr-todos", Some("asqr")),
            watched("ntropy", Some("ntropy")),
            watched("work", None),
        ];

        assert_eq!(
            nobody_watching(&invocation, &others, Some("playground")).to_string(),
            "warning: no asqr is watching queue playground; asqr watches asqr-todos (tmux session asqr), ntropy (tmux session ntropy) and work; the session waits in playground: ask the person to start `asqr` in a window of tmux session playground, or `asqr --queue playground` in a terminal of their own; do not start it yourself"
        );
    }

    #[test]
    fn outside_tmux_the_warning_asks_for_asqr_with_the_queue() {
        let invocation = Invocation::new(&named("work"), true);

        assert_eq!(
            nobody_watching(&invocation, &[watched("ntropy", None)], None).to_string(),
            "warning: no asqr is watching queue work; asqr watches ntropy; the session waits in work: ask the person to start `asqr --queue work` in a terminal of their own; do not start it yourself"
        );
    }

    #[test]
    fn without_other_watchers_the_warning_names_none() {
        let invocation = Invocation::new(&named("default"), false);

        assert_eq!(
            nobody_watching(&invocation, &[], None).to_string(),
            "warning: no asqr is watching queue default; the session waits in default: ask the person to start `asqr` in a terminal of their own; do not start it yourself"
        );
    }

    #[test]
    fn a_queue_watched_from_another_session_is_named_with_both_sessions() {
        let invocation = Invocation::new(&named("my-work"), false);

        assert_eq!(
            watched_from_another_session(&invocation, "my work", "my-work").to_string(),
            "warning: queue my-work is watched by the asqr of tmux session \"my work\", not of this session (\"my-work\"); the session waits there: ask the person whether to answer it there, or to start `asqr --queue <another name>` in a window of this session; do not start it yourself"
        );
    }

    #[test]
    fn a_variable_beside_another_pin_gets_a_hint() {
        let invocation = Invocation::new(&named("work"), true);

        assert_eq!(
            environment_beside_pin("ASQR_QUEUE", &invocation, "asqr-todos", true).to_string(),
            "hint: ASQR_QUEUE chose queue work, but the asqr of this tmux session watches asqr-todos; tell the person, who decides where you ask"
        );
        assert_eq!(
            environment_beside_pin("ASQR_QUEUE", &invocation, "asqr-todos", false).to_string(),
            "hint: ASQR_QUEUE chose queue work, but this tmux session's queue is asqr-todos; tell the person, who decides where you ask"
        );
    }

    #[test]
    fn the_wait_timeout_is_the_one_the_skill_shows() {
        let example = format!("asqr wait \"$id\" --timeout {WAIT_TIMEOUT}");

        assert!(super::super::skill::SKILL.contains(&example));
    }
}
