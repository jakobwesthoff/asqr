// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr ask` (spec sections 5.5 and 6.3).

mod common;

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use asqr::format::{Answer, SessionResult, now_rfc3339};
use asqr::queue::{QueueLocation, finish, lock_queue, save_draft, session_sha256};
use common::{Sandbox, stderr, stdout};

const SESSION: &str =
    r#"{"asqr": 2, "id": "batch-01", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

fn inbox_file(sandbox: &Sandbox, id: &str) -> serde_json::Value {
    let path = sandbox.queue_dir().join("inbox").join(format!("{id}.json"));
    serde_json::from_slice(&std::fs::read(&path).expect("inbox file exists"))
        .expect("inbox file is JSON")
}

fn location(sandbox: &Sandbox) -> QueueLocation {
    QueueLocation::at(sandbox.queue_dir())
}

/// Answers the waiting session `id` the way the TUI will: through the
/// finish path, with a result over the exact inbox bytes.
fn answer(sandbox: &Sandbox, id: &str, result: impl Fn(&str) -> SessionResult) {
    let path = sandbox.queue_dir().join("inbox").join(format!("{id}.json"));
    let bytes = std::fs::read(&path).expect("session is waiting");
    finish(&location(sandbox), &path, &result(&session_sha256(&bytes))).expect("finished");
}

#[test]
fn drops_the_session_and_prints_its_id() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "batch-01\n");
    assert_eq!(inbox_file(&sandbox, "batch-01")["id"], "batch-01");
}

#[test]
fn warns_when_no_asqr_watches_the_queue() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert!(
        stderr(&output).contains(&nobody_watching(&sandbox)),
        "{}",
        stderr(&output)
    );
}

/// The warning `ask` prints when no asqr watches the sandbox queue.
fn nobody_watching(sandbox: &Sandbox) -> String {
    let queue = sandbox.queue_dir();
    format!(
        "warning: no asqr is watching queue {0}; tell the person to start `asqr --dir {0}` in a terminal of their own; the session waits in the queue until then\n",
        queue.display()
    )
}

#[test]
fn ask_prints_the_warning_before_the_wait_reminder() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(
        stderr(&output),
        nobody_watching(&sandbox) + &wait_reminder(&sandbox, "batch-01")
    );
}

/// The reminder `ask` prints after it dropped session `id` into the
/// sandbox queue without `--wait`.
fn wait_reminder(sandbox: &Sandbox, id: &str) -> String {
    format!(
        "hint: the person can answer any item with a note; wait for the result and read all of it: `asqr --dir {} wait {id} --timeout 540`, or a lower timeout if your command limit is shorter\n",
        sandbox.queue_dir().display()
    )
}

#[test]
fn ask_reminds_the_agent_to_wait_for_the_result() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(err.ends_with(&wait_reminder(&sandbox, "batch-01")), "{err}");
}

#[test]
fn ask_reminds_to_wait_after_an_info_only_session() {
    // The person can write a note on an info item too, so a session that
    // asks nothing still comes back with a result to read.
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "report.json",
        r#"{"asqr": 2, "id": "report", "questions": [{"id": "done", "text": "Done.", "kind": "info"}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0));
    let err = stderr(&output);
    assert!(err.ends_with(&wait_reminder(&sandbox, "report")), "{err}");
}

#[test]
fn ask_wait_prints_no_wait_reminder() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask", "--wait", "--timeout", "0"])
        .arg(&file)
        .output()
        .expect("runs");

    let err = stderr(&output);
    assert!(!err.contains("hint: the person can answer"), "{err}");
}

#[test]
fn does_not_warn_while_an_asqr_holds_the_queue() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    let _watching = lock_queue(&location(&sandbox)).expect("this test plays the running asqr");

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert!(
        !stderr(&output).contains("no asqr is watching"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn assigns_a_ulid_when_the_session_has_no_id() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "session.json",
        r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    let id = stdout(&output).trim().to_owned();
    assert!(ulid::Ulid::from_string(&id).is_ok(), "{id}");
    assert_eq!(inbox_file(&sandbox, &id)["id"], id.as_str());
}

#[test]
fn makes_every_context_path_absolute_and_keeps_unknown_fields() {
    let sandbox = Sandbox::new();
    for file in ["q.png", "a.png", "notes.md", "fetch.rs", "old.rs", "new.rs"] {
        sandbox.file(&format!("session/files/{file}"), "content");
    }
    let file = sandbox.file(
        "session/session.json",
        r#"{"asqr": 2, "id": "ctx", "later": 1,
            "questions": [{"id": "q", "text": "?", "kind": "text",
                           "context": [{"type": "image", "path": "files/q.png"}]},
                          {"id": "s", "text": "?", "kind": "single",
                           "context": [{"type": "markdown", "path": "files/notes.md"}],
                           "options": [{"id": "a", "label": "A",
                                        "context": [{"type": "image", "path": "files/a.png"}]},
                                       {"id": "b", "label": "B",
                                        "context": [{"type": "diff", "old": {"path": "files/old.rs"},
                                                     "new": {"path": "files/new.rs"}}]},
                                       {"id": "c", "label": "C",
                                        "context": [{"type": "diff", "new": {"content": "inline"}}]},
                                       {"id": "d", "label": "D",
                                        "context": [{"type": "code", "path": "files/fetch.rs"}]}]}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let dropped = inbox_file(&sandbox, "ctx");
    let options = &dropped["questions"][1]["options"];
    for path in [
        &dropped["questions"][0]["context"][0]["path"],
        &dropped["questions"][1]["context"][0]["path"],
        &options[0]["context"][0]["path"],
        &options[1]["context"][0]["old"]["path"],
        &options[1]["context"][0]["new"]["path"],
        &options[3]["context"][0]["path"],
    ] {
        let path = path.as_str().expect("path");
        assert!(Path::new(path).is_absolute(), "{path}");
        assert!(Path::new(path).is_file(), "{path}");
    }
    // Fields that were not there stay away: no `old` on a new file, no
    // `context` or `options` where the asker wrote none.
    assert_eq!(
        options[2]["context"][0],
        serde_json::json!({"type": "diff", "new": {"content": "inline"}})
    );
    assert!(dropped["questions"][0].get("options").is_none());
    assert!(options[2].get("description").is_none());
    assert_eq!(dropped["later"], 1);
    assert!(
        stderr(&output).contains("warning: later: unknown field"),
        "{}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).contains("relative path"),
        "ask fixes relative paths itself"
    );
}

#[test]
fn ask_warns_that_note_false_is_ignored_and_drops_the_field() {
    // Every question takes a note (ADR 68). Without the field, an older
    // asqr watching the queue offers the note too.
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "quiet.json",
        r#"{"asqr": 2, "id": "quiet", "questions": [
            {"id": "q", "text": "?", "kind": "text", "note": false},
            {"id": "r", "text": "?", "kind": "text", "note": true},
            {"id": "s", "text": "?", "kind": "text"}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let warnings = stderr(&output);
    assert!(
        warnings.contains(
            "warning: questions[0].note: false is ignored; every question takes a note\n"
        ),
        "{warnings}"
    );
    assert!(!warnings.contains("questions[1].note"), "{warnings}");
    let dropped = inbox_file(&sandbox, "quiet");
    let questions = dropped["questions"].as_array().expect("questions");
    assert!(questions[0].get("note").is_none(), "{dropped}");
    assert_eq!(questions[1]["note"], true, "a true stays as written");
    assert!(questions[2].get("note").is_none(), "no field is added");
}

#[test]
fn warnings_name_a_missing_file_by_its_absolute_path() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "session/session.json",
        r#"{"asqr": 2, "id": "missing", "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "code", "path": "files/fetch.rs"}]}]}"#,
    );
    let directory = file.parent().expect("the session has a directory");

    // The session file given by its bare name, from its own directory,
    // as an agent working there writes it.
    let output = sandbox
        .asqr_in_queue(&["ask", "session.json"])
        .current_dir(directory)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let expected = directory
        .canonicalize()
        .expect("the directory exists")
        .join("files/fetch.rs");
    assert!(
        stderr(&output).contains(&format!(
            "warning: questions[0].context[0].path: file not found: {}\n",
            expected.display()
        )),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_question_without_context_gets_none_in_the_dropped_file() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "session.json",
        r#"{"asqr": 2, "id": "plain", "questions": [{"id": "q", "text": "?", "kind": "single",
            "options": [{"id": "a", "label": "A"}]}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let dropped = inbox_file(&sandbox, "plain");
    assert!(dropped["questions"][0].get("context").is_none());
    assert!(
        dropped["questions"][0]["options"][0]
            .get("context")
            .is_none()
    );
}

#[test]
fn refuses_a_session_of_format_version_1_and_points_to_the_skill() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "old.json",
        r#"{"asqr": 1, "id": "old", "questions": [{"id": "q", "text": "?", "kind": "text",
            "image": "/q.png"}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert_eq!(
        stderr(&output),
        "error: asqr: this asqr no longer supports format version 1; `asqr skill` describes the current format (version 2)\n"
    );
    assert!(!sandbox.queue_dir().join("inbox").join("old.json").exists());
}

#[test]
fn refuses_an_unknown_entry_type_as_one_a_newer_asqr_may_support() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "video.json",
        r#"{"asqr": 2, "id": "video", "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "video", "content": "x"}]}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert_eq!(
        stderr(&output),
        format!(
            r#"error: questions[0].context[0].type: "video" is not supported by this asqr (version {}), which supports "image", "code", "markdown", "diff" and "ansi"; a newer asqr may support it
"#,
            env!("CARGO_PKG_VERSION")
        )
    );
    assert!(
        !sandbox
            .queue_dir()
            .join("inbox")
            .join("video.json")
            .exists()
    );
}

#[test]
fn refuses_an_unknown_kind_as_one_a_newer_asqr_may_support() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "rank.json",
        r#"{"asqr": 2, "id": "rank", "questions": [{"id": "q", "text": "?", "kind": "rank"}]}"#,
    );

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert_eq!(
        stderr(&output),
        format!(
            r#"error: questions[0].kind: "rank" is not supported by this asqr (version {}), which supports "single", "multi", "text" and "info"; a newer asqr may support it
"#,
            env!("CARGO_PKG_VERSION")
        )
    );
    assert!(!sandbox.queue_dir().join("inbox").join("rank.json").exists());
}

#[test]
fn refuses_an_invalid_session() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("bad.json", r#"{"asqr": 3, "id": "bad", "questions": []}"#);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
    assert!(stderr(&output).contains("error: asqr: unsupported format version 3"));
    assert!(!sandbox.queue_dir().join("inbox").join("bad.json").exists());
}

#[test]
fn refuses_a_file_that_is_not_a_session() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("bad.json", "[]");

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(11));
}

#[test]
fn refuses_a_session_that_is_no_json_object_without_a_panic() {
    // serde reads the session's struct from an array of its fields in
    // their order, so this array passes the typed parse.
    let array = r#"[2, null, null, null, null, null, [{"id": "a", "text": "?", "kind": "text"}]]"#;
    for (name, content) in [("array", array), ("string", r#""session""#)] {
        let sandbox = Sandbox::new();
        let file = sandbox.file("bad.json", content);

        let output = sandbox
            .asqr_in_queue(&["ask"])
            .arg(&file)
            .output()
            .expect("runs");

        assert_eq!(
            output.status.code(),
            Some(11),
            "{name}: {}",
            stderr(&output)
        );
        assert!(
            stderr(&output).starts_with("error: not a session file:"),
            "{name}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn replaces_a_waiting_session_nobody_answered() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    let changed = sandbox.file("changed.json", &SESSION.replace("\"?\"", "\"changed?\""));

    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&changed)
        .assert()
        .success();

    assert_eq!(
        inbox_file(&sandbox, "batch-01")["questions"][0]["text"],
        "changed?"
    );
}

#[test]
fn ask_says_when_it_replaced_a_waiting_session() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(
        stderr(&output),
        "asqr: replaced the waiting session batch-01, in which the person had entered no answer yet; they see the new file in its place\n".to_owned()
            + &nobody_watching(&sandbox)
            + &wait_reminder(&sandbox, "batch-01")
    );
}

#[test]
fn ask_of_a_new_id_says_nothing_about_replacing() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    let err = stderr(&output);
    assert!(!err.contains("replaced the waiting session"), "{err}");
}

#[test]
fn refuses_while_the_session_is_being_answered() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    let started = Answer {
        custom: Some("half".into()),
        ..Answer::new("q")
    };
    save_draft(
        &location(&sandbox),
        &SessionResult::draft("batch-01", "q", vec![started]),
    )
    .expect("saved");

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "error: session \"batch-01\" is being answered; wait for its result, or ask the person and withdraw it with `asqr withdraw --force batch-01`\n"
    );
}

#[test]
fn refuses_an_unread_result_unless_forced() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    answer(&sandbox, "batch-01", |sha| {
        SessionResult::submitted("batch-01", sha, &now_rfc3339(), None, Vec::new())
    });

    let refused = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");
    let forced = sandbox
        .asqr_in_queue(&["ask", "--force"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("unread result"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(forced.status.code(), Some(0));
    assert_eq!(inbox_file(&sandbox, "batch-01")["id"], "batch-01");
}

#[test]
fn an_unread_result_refusal_names_the_ways_past_it() {
    // Reading the result leaves it in the outbox, so `ask` would refuse
    // again: the ways past it are `--force` and a fresh id.
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    answer(&sandbox, "batch-01", |sha| {
        SessionResult::submitted("batch-01", sha, &now_rfc3339(), None, Vec::new())
    });

    let refused = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(
        stderr(&refused),
        format!(
            "error: an unread result for \"batch-01\" is in the outbox; read it first with `asqr --dir {} result batch-01`, then ask again with --force, which archives that result, or leave `id` out of the file to get a fresh id\n",
            sandbox.queue_dir().display()
        )
    );
}

/// Starts `asqr ask --wait` in the background, answers the session once it
/// arrives, and returns what the waiting asker printed and how it exited.
fn ask_and_answer(result: impl Fn(&str) -> SessionResult) -> (Option<i32>, String, String) {
    ask_and_answer_session(SESSION, result)
}

/// [`ask_and_answer`] for the session file `session`, whose id must be
/// `batch-01`.
fn ask_and_answer_session(
    session: &str,
    result: impl Fn(&str) -> SessionResult,
) -> (Option<i32>, String, String) {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", session);
    let mut command = sandbox.spawnable_asqr();
    command
        .arg("--dir")
        .arg(sandbox.queue_dir())
        .args(["ask", "--wait", "--timeout", "20"])
        .arg(&file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().expect("asqr starts");

    let inbox = sandbox.queue_dir().join("inbox").join("batch-01.json");
    let started = Instant::now();
    while !inbox.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the session never arrived"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    answer(&sandbox, "batch-01", result);

    let output = child.wait_with_output().expect("asqr ends");
    (output.status.code(), stdout(&output), stderr(&output))
}

#[test]
fn waits_for_the_result_and_prints_only_the_result() {
    let (code, out, err) = ask_and_answer(|sha| {
        SessionResult::submitted(
            "batch-01",
            sha,
            &now_rfc3339(),
            None,
            vec![Answer::skipped("q")],
        )
    });

    assert_eq!(code, Some(0));
    let result: SessionResult = serde_json::from_str(&out).expect("stdout is the result JSON");
    assert_eq!(result.answers, [Answer::skipped("q")]);
    assert!(err.contains("batch-01"), "the id goes to stderr: {err}");
}

#[test]
fn a_rejected_session_exits_with_the_cancelled_code() {
    let (code, _, _) = ask_and_answer(|sha| {
        SessionResult::cancelled("batch-01", sha, &now_rfc3339(), None, None, Vec::new())
    });

    assert_eq!(code, Some(10));
}

#[test]
fn ask_wait_says_to_wait_again_after_its_timeout() {
    // Asking again would replace the session or be refused, so the way on
    // is `wait`.
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);

    let output = sandbox
        .asqr_in_queue(&["ask", "--wait", "--timeout", "1"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(12));
    let expected = format!(
        "\nbatch-01\nasqr: no result for batch-01 yet (timeout); the person has not answered yet, so wait again with `asqr --dir {} wait batch-01 --timeout 1`\n",
        sandbox.queue_dir().display()
    );
    let err = stderr(&output);
    assert!(err.ends_with(&expected), "{err}");
}

/// The example session with `count` fields asqr does not know, with or
/// without its `id`.
fn with_unknown_fields(count: usize, keep_id: bool) -> String {
    let fields: String = (0..count).map(|n| format!(r#""later{n}": 1, "#)).collect();
    let session = SESSION.replacen('{', &format!("{{{fields}"), 1);
    if keep_id {
        session
    } else {
        session.replace(r#""id": "batch-01", "#, "")
    }
}

/// The ULID among the lines of `text`, as `ask` prints the id it assigned.
fn assigned_id(text: &str) -> String {
    text.lines()
        .find(|line| ulid::Ulid::from_string(line).is_ok())
        .expect("an assigned id")
        .to_owned()
}

#[test]
fn ask_with_a_warning_offers_to_replace_or_wait() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", &with_unknown_fields(1, false));

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    let id = stdout(&output).trim().to_owned();
    let expected = format!(
        "hint: 1 warning above; if it matters to the person, fix the file and replace this session: `asqr --dir {0} ask {1} --replaces {id}`; otherwise wait for its result: `asqr --dir {0} wait {id} --timeout 540`\n",
        sandbox.queue_dir().display(),
        file.display()
    );
    let err = stderr(&output);
    assert!(err.ends_with(&expected), "{err}");
    assert!(!err.contains("hint: the person can answer"), "{err}");
}

#[test]
fn ask_with_warnings_and_an_id_offers_to_ask_again() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", &with_unknown_fields(2, true));

    let output = sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .output()
        .expect("runs");

    let expected = format!(
        "hint: 2 warnings above; if one matters to the person, fix the file, keep its `id`, and ask again: `asqr --dir {0} ask {1}`, which replaces this session while the person has not started on it; otherwise wait for its result: `asqr --dir {0} wait batch-01 --timeout 540`\n",
        sandbox.queue_dir().display(),
        file.display()
    );
    let err = stderr(&output);
    assert!(err.ends_with(&expected), "{err}");
}

#[test]
fn ask_wait_names_the_warnings_when_it_times_out() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", &with_unknown_fields(1, false));

    let output = sandbox
        .asqr_in_queue(&["ask", "--wait", "--timeout", "0"])
        .arg(&file)
        .output()
        .expect("runs");

    let err = stderr(&output);
    let id = assigned_id(&err);
    let expected = format!(
        "hint: 1 warning above; the session still waits; if it matters to the person, fix the file and replace it: `asqr --dir {} ask {} --replaces {id}`\nasqr: no result for {id} yet (timeout);",
        sandbox.queue_dir().display(),
        file.display()
    );
    assert!(err.contains(&expected), "{err}");
}

#[test]
fn ask_wait_with_an_id_names_the_warnings_when_it_times_out() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", &with_unknown_fields(2, true));

    let output = sandbox
        .asqr_in_queue(&["ask", "--wait", "--timeout", "0"])
        .arg(&file)
        .output()
        .expect("runs");

    let expected = format!(
        "hint: 2 warnings above; the session still waits; if one matters to the person, fix the file, keep its `id`, and ask again: `asqr --dir {} ask {}`, which replaces it\nasqr: no result for batch-01 yet (timeout);",
        sandbox.queue_dir().display(),
        file.display()
    );
    let err = stderr(&output);
    assert!(err.contains(&expected), "{err}");
}

#[test]
fn ask_wait_names_no_warnings_once_a_result_came() {
    // A finished session can no longer be replaced.
    let (code, _, err) = ask_and_answer_session(&with_unknown_fields(1, true), |sha| {
        SessionResult::submitted("batch-01", sha, &now_rfc3339(), None, Vec::new())
    });

    assert_eq!(code, Some(0));
    assert!(err.contains("warning: later0"), "{err}");
    assert!(!err.contains("warning above"), "{err}");
}

#[test]
fn stops_waiting_after_the_timeout() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("session.json", SESSION);
    let started = Instant::now();

    let output = sandbox
        .asqr_in_queue(&["ask", "--wait", "--timeout", "1"])
        .arg(&file)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(12));
    assert_eq!(stdout(&output), "");
    assert!(started.elapsed() >= Duration::from_secs(1));
}

// -------------------------------------------------------------------------
// --replaces
// -------------------------------------------------------------------------

/// The session `round-1` waiting, and the file of `round-2`, which is to
/// replace it.
fn round_one_waiting(sandbox: &Sandbox) -> std::path::PathBuf {
    let first = sandbox.file("round-1.json", &SESSION.replace("batch-01", "round-1"));
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&first)
        .assert()
        .success();
    sandbox.file("round-2.json", &SESSION.replace("batch-01", "round-2"))
}

fn replace(sandbox: &Sandbox, file: &Path, args: &[&str]) -> std::process::Output {
    let mut all = vec!["ask"];
    all.extend(args);
    sandbox
        .asqr_in_queue(&all)
        .arg(file)
        .output()
        .expect("runs")
}

fn waiting(sandbox: &Sandbox) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(sandbox.queue_dir().join("inbox"))
        .expect("inbox")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".json"))
        .collect();
    names.sort();
    names
}

fn result_status(sandbox: &Sandbox, id: &str) -> Option<String> {
    let path = sandbox
        .queue_dir()
        .join("outbox")
        .join(format!("{id}.json"));
    let bytes = std::fs::read(path).ok()?;
    let result: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    result["status"].as_str().map(str::to_owned)
}

fn start_answering(sandbox: &Sandbox, id: &str) {
    let started = Answer {
        custom: Some("half".into()),
        ..Answer::new("q")
    };
    save_draft(
        &location(sandbox),
        &SessionResult::draft(id, "q", vec![started]),
    )
    .expect("saved");
}

#[test]
fn ask_replaces_withdraws_the_old_session_and_drops_the_new_one() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);

    let output = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "round-2\n");
    assert_eq!(waiting(&sandbox), ["round-2.json"]);
    assert_eq!(
        result_status(&sandbox, "round-1").as_deref(),
        Some("withdrawn")
    );
}

#[test]
fn ask_replaces_refuses_while_the_old_session_is_being_answered() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);
    start_answering(&sandbox, "round-1");

    let output = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "error: session \"round-1\" is being answered; withdraw it anyway with --force\n"
    );
    assert_eq!(waiting(&sandbox), ["round-1.json"]);
}

#[test]
fn ask_replaces_with_force_takes_a_session_being_answered() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);
    start_answering(&sandbox, "round-1");

    let output = replace(&sandbox, &second, &["--replaces", "round-1", "--force"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(waiting(&sandbox), ["round-2.json"]);
    assert_eq!(
        result_status(&sandbox, "round-1").as_deref(),
        Some("withdrawn")
    );
}

#[test]
fn ask_replaces_force_archives_an_unread_result_of_the_new_id() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);
    std::fs::write(sandbox.queue_dir().join("outbox/round-2.json"), "{}").expect("written");

    let refused = replace(&sandbox, &second, &["--replaces", "round-1"]);
    let forced = replace(&sandbox, &second, &["--replaces", "round-1", "--force"]);

    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("unread result for \"round-2\""),
        "{}",
        stderr(&refused)
    );
    assert_eq!(forced.status.code(), Some(0), "{}", stderr(&forced));
    assert_eq!(waiting(&sandbox), ["round-2.json"]);
    assert_eq!(result_status(&sandbox, "round-2"), None, "moved aside");
}

#[test]
fn ask_replaces_an_unread_result_refusal_warns_about_force() {
    // Here `--force` would also withdraw the old session while the person
    // answers it, so it is named only as a warning.
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);
    std::fs::write(sandbox.queue_dir().join("outbox/round-2.json"), "{}").expect("written");

    let refused = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(
        stderr(&refused),
        format!(
            "error: an unread result for \"round-2\" is in the outbox; read it first with `asqr --dir {} result round-2`, then leave `id` out of the file to get a fresh id; --force would archive that result and also withdraw round-1 even if the person is answering it\n",
            sandbox.queue_dir().display()
        )
    );
}

#[test]
fn ask_replaces_refuses_an_answered_old_session() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);
    answer(&sandbox, "round-1", |sha| {
        SessionResult::submitted("round-1", sha, &now_rfc3339(), None, Vec::new())
    });

    let output = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("is answered already"),
        "{}",
        stderr(&output)
    );
    assert_eq!(waiting(&sandbox), Vec::<String>::new());
}

#[test]
fn ask_replaces_an_unknown_id_exits_13_without_dropping() {
    let sandbox = Sandbox::new();
    let second = sandbox.file("round-2.json", &SESSION.replace("batch-01", "round-2"));

    let output = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(13));
    assert_eq!(stderr(&output), no_session(&sandbox, "round-1"));
    assert_eq!(waiting(&sandbox), Vec::<String>::new());
}

#[test]
fn ask_replaces_an_old_session_whose_result_is_gone_exits_13() {
    let sandbox = Sandbox::new();
    let second = sandbox.file("round-2.json", &SESSION.replace("batch-01", "round-2"));
    let archive = sandbox.queue_dir().join("archive");
    std::fs::create_dir_all(&archive).expect("archive");
    let name = asqr::queue::archive_name(
        "round-1",
        ulid::Ulid::from_parts(1_000, 0),
        asqr::queue::Archived::Session,
    );
    std::fs::write(archive.join(name), "{}").expect("written");

    let output = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(13));
    assert_eq!(
        stderr(&output),
        "error: the result of round-1 is gone; its session is archived\n"
    );
    assert_eq!(waiting(&sandbox), Vec::<String>::new());
}

#[test]
fn ask_replaces_an_already_withdrawn_session_drops_the_new_one() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);
    sandbox
        .asqr_in_queue(&["withdraw", "round-1"])
        .assert()
        .success();

    let output = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(waiting(&sandbox), ["round-2.json"]);
}

#[test]
fn ask_replaces_its_own_id_is_a_usage_error() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);

    let output = replace(&sandbox, &second, &["--replaces", "ROUND-2"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "error: --replaces names the session's own id; ask replaces a waiting session of the same id without it\n"
    );
    assert_eq!(waiting(&sandbox), ["round-1.json"]);
}

#[test]
fn ask_replaces_an_invalid_file_leaves_the_old_waiting() {
    let sandbox = Sandbox::new();
    round_one_waiting(&sandbox);
    let broken = sandbox.file("broken.json", r#"{"asqr": 2, "questions": []}"#);

    let output = replace(&sandbox, &broken, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(11));
    assert_eq!(waiting(&sandbox), ["round-1.json"]);
    assert_eq!(result_status(&sandbox, "round-1"), None);
}

#[test]
fn ask_replaces_with_an_outbox_that_cannot_be_read_fails() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);
    let outbox = sandbox.queue_dir().join("outbox");
    std::fs::remove_dir(&outbox).expect("removed");
    std::fs::write(&outbox, "in the way").expect("written");

    let output = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(waiting(&sandbox), ["round-1.json"]);
}

#[test]
fn ask_replaces_an_old_session_that_cannot_be_read_fails_and_drops_nothing() {
    let sandbox = Sandbox::new();
    let second = sandbox.file("round-2.json", &SESSION.replace("batch-01", "round-2"));
    // A directory under the old session's name: it is found, and reading
    // it fails.
    std::fs::create_dir_all(sandbox.queue_dir().join("inbox/round-1.json")).expect("created");

    let output = replace(&sandbox, &second, &["--replaces", "round-1"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with("error: cannot withdraw round-1: "),
        "{}",
        stderr(&output)
    );
    assert_eq!(waiting(&sandbox), ["round-1.json"]);
}

#[test]
fn ask_replaces_with_wait_waits_for_the_new_one() {
    let sandbox = Sandbox::new();
    let second = round_one_waiting(&sandbox);

    let output = replace(
        &sandbox,
        &second,
        &["--replaces", "round-1", "--wait", "--timeout", "0"],
    );

    assert_eq!(output.status.code(), Some(12));
    assert!(
        stderr(&output).contains(&format!(
            "\nround-2\nasqr: no result for round-2 yet (timeout); the person has not answered yet, so wait again with `asqr --dir {} wait round-2 --timeout 540`, or a lower timeout if your command limit is shorter\n",
            sandbox.queue_dir().display()
        )),
        "{}",
        stderr(&output)
    );
}

/// The refusal for session `id`, which the sandbox queue does not know.
fn no_session(sandbox: &Sandbox, id: &str) -> String {
    let queue = sandbox.queue_dir();
    format!(
        "error: no session \"{id}\" in queue {0}; check the id with `asqr --dir {0} status --since 1d`, and if you asked in another queue (--queue, --dir, ASQR_QUEUE or ASQR_DIR), run this command again with that queue\n",
        queue.display()
    )
}
