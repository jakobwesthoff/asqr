// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr withdraw` (spec sections 5.10 and 6.4).

mod common;

use asqr::format::{Answer, SessionResult, Status, now_rfc3339};
use asqr::queue::{Archived, QueueLocation, archive_name, finish, save_draft, session_sha256};
use common::{Sandbox, stderr, stdout};

fn location(sandbox: &Sandbox) -> QueueLocation {
    QueueLocation::at(sandbox.queue_dir())
}

/// Drops session `id` through `asqr ask`.
fn ask(sandbox: &Sandbox, id: &str) {
    let file = sandbox.file(
        &format!("{id}.json"),
        &format!(
            r#"{{"asqr": 2, "id": "{id}", "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#
        ),
    );
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
}

/// Gives the waiting session `id` a draft with a typed answer.
fn start_answering(sandbox: &Sandbox, id: &str) {
    let typed = Answer {
        custom: Some("half an answer".into()),
        ..Answer::new("q")
    };
    save_draft(
        &location(sandbox),
        &SessionResult::draft(id, "q", vec![typed]),
    )
    .expect("saved");
}

fn submit(sandbox: &Sandbox, id: &str) {
    let path = sandbox.queue_dir().join("inbox").join(format!("{id}.json"));
    let bytes = std::fs::read(&path).expect("session is waiting");
    let result = SessionResult::submitted(
        id,
        &session_sha256(&bytes),
        &now_rfc3339(),
        None,
        vec![Answer::skipped("q")],
    );
    finish(&location(sandbox), &path, &result).expect("finished");
}

fn withdraw(sandbox: &Sandbox, args: &[&str]) -> std::process::Output {
    let mut all = vec!["withdraw"];
    all.extend(args);
    sandbox.asqr_in_queue(&all).output().expect("runs")
}

#[test]
fn withdraw_prints_the_result_and_wait_and_result_end_with_14() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");

    let withdrawn = withdraw(&sandbox, &["batch"]);

    assert_eq!(withdrawn.status.code(), Some(0), "{}", stderr(&withdrawn));
    let result: SessionResult = serde_json::from_str(&stdout(&withdrawn)).expect("result JSON");
    assert_eq!(result.status, Status::Withdrawn);
    for command in ["wait", "result"] {
        let output = sandbox
            .asqr_in_queue(&[command, "batch"])
            .output()
            .expect("runs");
        assert_eq!(output.status.code(), Some(14), "{command}");
        assert_eq!(stdout(&output), stdout(&withdrawn), "{command}");
    }
}

#[test]
fn withdraw_refuses_a_session_being_answered_with_1() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    start_answering(&sandbox, "batch");

    let refused = withdraw(&sandbox, &["batch"]);

    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(stdout(&refused), "");
    assert_eq!(
        stderr(&refused),
        format!(
            "error: session \"batch\" is being answered; check with the person first, then `asqr --dir {} withdraw --force batch` withdraws it, and the result's `draft` holds what they entered so far\n",
            sandbox.queue_dir().display()
        )
    );
    assert!(sandbox.queue_dir().join("inbox/batch.json").is_file());
}

#[test]
fn withdraw_refuses_a_session_whose_only_change_is_a_cleared_default_with_1() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    let cleared = Answer {
        cleared: true,
        ..Answer::new("q")
    };
    save_draft(
        &location(&sandbox),
        &SessionResult::draft("batch", "q", vec![cleared]),
    )
    .expect("saved");

    let refused = withdraw(&sandbox, &["batch"]);

    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(sandbox.queue_dir().join("inbox/batch.json").is_file());
}

#[test]
fn withdraw_force_takes_it_anyway_with_the_unsent_draft() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    start_answering(&sandbox, "batch");

    let withdrawn = withdraw(&sandbox, &["batch", "--force"]);

    assert_eq!(withdrawn.status.code(), Some(0), "{}", stderr(&withdrawn));
    let result: SessionResult = serde_json::from_str(&stdout(&withdrawn)).expect("result JSON");
    assert_eq!(result.answers, Vec::<Answer>::new());
    assert_eq!(
        result.draft.expect("the unsent draft").answers[0]
            .custom
            .as_deref(),
        Some("half an answer")
    );
}

#[test]
fn withdraw_of_an_answered_session_is_refused_with_1() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    submit(&sandbox, "batch");

    let refused = withdraw(&sandbox, &["batch"]);

    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(
        stderr(&refused),
        "error: session \"batch\" is answered already; read the result with `asqr result batch`\n"
    );
}

#[test]
fn withdraw_twice_exits_0_and_says_so() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    let first = withdraw(&sandbox, &["batch"]);

    let second = withdraw(&sandbox, &["batch"]);

    assert_eq!(second.status.code(), Some(0));
    assert_eq!(stdout(&second), stdout(&first));
    assert_eq!(stderr(&second), "asqr: batch was withdrawn already\n");
}

#[test]
fn withdraw_of_an_unknown_id_exits_13() {
    let sandbox = Sandbox::new();
    std::fs::create_dir_all(sandbox.queue_dir()).expect("queue");

    let refused = withdraw(&sandbox, &["nothing"]);

    assert_eq!(refused.status.code(), Some(13));
    assert_eq!(stderr(&refused), no_session(&sandbox, "nothing"));
}

#[test]
fn withdraw_of_an_archived_session_exits_13() {
    let sandbox = Sandbox::new();
    let archive = sandbox.queue_dir().join("archive");
    std::fs::create_dir_all(&archive).expect("archive");
    let name = archive_name("old", ulid::Ulid::from_parts(1_000, 0), Archived::Session);
    std::fs::write(archive.join(name), "{}").expect("written");

    let refused = withdraw(&sandbox, &["old"]);

    assert_eq!(refused.status.code(), Some(13));
    assert_eq!(
        stderr(&refused),
        "error: the result of old is gone; its session is archived\n"
    );
}

#[test]
fn withdraw_of_an_unknown_queue_creates_nothing() {
    let sandbox = Sandbox::new();

    let refused = sandbox
        .asqr()
        .args(["--queue", "typo", "withdraw", "batch"])
        .output()
        .expect("runs");

    assert_eq!(refused.status.code(), Some(13));
    let queues = sandbox.home().join(".local/share/asqr/queues");
    assert!(!queues.join("typo").exists());
}

#[test]
fn withdraw_with_an_unread_result_of_an_earlier_session_exits_1() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    submit(&sandbox, "batch");
    // Dropped by hand next to the unread result, which `ask` refuses.
    std::fs::write(sandbox.queue_dir().join("inbox/batch.json"), "{}").expect("written");

    let refused = withdraw(&sandbox, &["batch"]);

    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).starts_with("error: an unread result of an earlier session \"batch\""),
        "{}",
        stderr(&refused)
    );
}

#[test]
fn withdraw_with_an_outbox_that_cannot_be_written_exits_1() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    let outbox = sandbox.queue_dir().join("outbox");
    std::fs::remove_dir(&outbox).expect("removed");
    std::fs::write(&outbox, "in the way").expect("written");

    let failed = withdraw(&sandbox, &["batch"]);

    assert_eq!(failed.status.code(), Some(1));
    assert!(
        stderr(&failed).starts_with("error: cannot withdraw batch: "),
        "{}",
        stderr(&failed)
    );
}

#[test]
fn asking_a_withdrawn_id_again_needs_force() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    withdraw(&sandbox, &["batch"]);
    let file = sandbox.file(
        "again.json",
        r#"{"asqr": 2, "id": "batch", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
    );

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
    assert_eq!(forced.status.code(), Some(0), "{}", stderr(&forced));
}

#[test]
fn status_lists_a_withdrawn_session_as_answered() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    withdraw(&sandbox, &["batch"]);

    let output = sandbox
        .asqr_in_queue(&["status", "--json"])
        .output()
        .expect("runs");

    let status: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    assert_eq!(
        status["answered"],
        serde_json::json!([{"id": "batch", "status": "withdrawn"}])
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

/// The hint after a withdrawn result that carries a `draft`, behind the
/// empty line that ends the result's JSON.
const DRAFT_HINT: &str = "\nhint: `draft` holds what the person entered and never sent; these are not decisions, so use them only as presets in a replacement session (`asqr skill withdrawing` says how) and ignore them otherwise\n";

#[test]
fn withdraw_force_points_at_the_draft() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    start_answering(&sandbox, "batch");

    let withdrawn = withdraw(&sandbox, &["batch", "--force"]);

    assert_eq!(withdrawn.status.code(), Some(0));
    assert_eq!(stderr(&withdrawn), DRAFT_HINT);
}

#[test]
fn withdrawing_again_names_the_draft_after_the_notice() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    start_answering(&sandbox, "batch");
    withdraw(&sandbox, &["batch", "--force"]);

    let again = withdraw(&sandbox, &["batch"]);

    assert_eq!(again.status.code(), Some(0));
    assert_eq!(
        stderr(&again),
        "asqr: batch was withdrawn already\n".to_owned() + DRAFT_HINT
    );
}

#[test]
fn wait_on_a_withdrawn_result_with_a_draft_points_at_it() {
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    start_answering(&sandbox, "batch");
    withdraw(&sandbox, &["batch", "--force"]);

    let output = sandbox
        .asqr_in_queue(&["wait", "batch"])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(14));
    assert_eq!(stderr(&output), DRAFT_HINT);
}

#[test]
fn a_withdrawal_whose_result_cannot_be_printed_fails_without_the_draft_hint() {
    // The hint points into a result the agent never got.
    let sandbox = Sandbox::new();
    ask(&sandbox, "batch");
    start_answering(&sandbox, "batch");
    let (reader, writer) = std::io::pipe().expect("a pipe");
    drop(reader);

    let output = sandbox
        .spawnable_asqr()
        .arg("--dir")
        .arg(sandbox.queue_dir())
        .args(["withdraw", "batch", "--force"])
        .stdout(writer)
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(&output), "");
}
