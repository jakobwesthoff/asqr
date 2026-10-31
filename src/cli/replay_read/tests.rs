// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs;

use serde_json::{Value, json};
use ulid::Ulid;

use super::*;
use crate::format::{Answer, SessionResult, UnsentDraft};
use crate::keep::sha256_hex;
use crate::queue::{
    Archived, EntryKind, EntryName, EntryPlace, Month, NewEntry, QueueLocation, add_entries,
    archive_name, session_sha256,
};

// =========================================================
// Fixtures
// =========================================================

struct Scene {
    _scratch: tempfile::TempDir,
    location: QueueLocation,
}

fn scene() -> Scene {
    scene_in("default")
}

fn scene_in(queue: &str) -> Scene {
    let scratch = tempfile::tempdir().expect("temp dir");
    let location = QueueLocation::named(scratch.path(), queue);
    location.create_layout().expect("layout");
    Scene {
        _scratch: scratch,
        location,
    }
}

fn ulid() -> Ulid {
    Ulid::from_parts(1_788_000_000_000, 0x1234_5678_9abc)
}

/// A kept copy: the name `files/<name>` it is kept under, and its bytes.
fn copy(bytes: &[u8], extension: &str) -> (String, Vec<u8>) {
    (format!("{}.{extension}", sha256_hex(bytes)), bytes.to_vec())
}

fn kept(name: &str, original: &str, sha256: &str) -> Value {
    json!({"type": "image", "path": format!("files/{name}"), "shown": {
        "original_path": original, "state": "kept", "sha256": sha256,
        "size": 100, "modified": "2026-10-29T16:40:11Z", "kept_size": 40}})
}

/// A session with one question for each of `entries`, which it shows as
/// the question's context.
fn session_showing(entries: Vec<Value>) -> Value {
    let questions: Vec<Value> = entries
        .into_iter()
        .enumerate()
        .map(|(position, entry)| {
            json!({"id": format!("q{position}"), "text": "Which?", "kind": "text",
                   "context": [entry]})
        })
        .collect();
    json!({"asqr": 2, "id": "hero-2", "title": "Pick the hero", "questions": questions})
}

fn plain_session() -> Value {
    session_showing(vec![json!({"type": "code", "content": "x"})])
}

/// Archives `session` loose with `copies` under `archive/files/`, and puts
/// `result` into the outbox.
fn finish(
    scene: &Scene,
    session: &Value,
    copies: &[(String, Vec<u8>)],
    result: Option<SessionResult>,
) -> EntryRef {
    let bytes = serde_json::to_vec_pretty(session).expect("serialises");
    let path = scene
        .location
        .archive()
        .join(archive_name("hero-2", ulid(), Archived::Session));
    fs::write(&path, &bytes).expect("written");
    fs::create_dir_all(scene.location.kept_files()).expect("created");
    for (name, content) in copies {
        fs::write(scene.location.kept_files().join(name), content).expect("written");
    }
    if let Some(result) = result {
        fs::write(
            scene.location.outbox().join("hero-2.json"),
            serde_json::to_vec_pretty(&result).expect("serialises"),
        )
        .expect("written");
    }
    EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Loose(path),
    }
}

fn sha_of(session: &Value) -> String {
    session_sha256(&serde_json::to_vec_pretty(session).expect("serialises"))
}

fn submitted(session: &Value) -> SessionResult {
    SessionResult::submitted(
        "hero-2",
        &sha_of(session),
        "2026-08-29T10:39:00Z",
        Some("thanks".to_owned()),
        vec![Answer {
            selected: vec!["b".to_owned()],
            ..Answer::new("q0")
        }],
    )
}

fn read(entry: &EntryRef) -> Value {
    replay_json(&load_for_reading(entry).expect("loaded"))
}

/// The `files` array of the printed object.
fn files(printed: &Value) -> &Vec<Value> {
    printed["files"].as_array().expect("an array")
}

// =========================================================
// The object
// =========================================================

#[test]
fn a_sent_session_prints_the_fields_an_agent_reads() {
    let scene = scene();
    let (name, bytes) = copy(b"webp bytes", "webp");
    let session = session_showing(vec![kept(&name, "/tmp/x/hero.png", &"ab".repeat(32))]);
    let entry = finish(
        &scene,
        &session,
        &[(name, bytes)],
        Some(submitted(&session)),
    );

    let printed = read(&entry);

    let mut keys: Vec<&str> = printed
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "draft",
            "extracted_to",
            "files",
            "finished_at",
            "id",
            "queue",
            "ref",
            "result",
            "session",
            "status"
        ]
    );
    assert_eq!(printed["ref"], entry.reference());
    assert_eq!(printed["queue"], "default");
    assert_eq!(printed["id"], "hero-2");
    assert_eq!(printed["status"], "sent");
    assert_eq!(printed["draft"], false);
    assert_eq!(printed["extracted_to"], Value::Null);
    assert_eq!(printed["result"]["status"], "submitted");
    assert_eq!(printed["result"]["answers"][0]["selected"], json!(["b"]));
    let finished = printed["finished_at"].as_str().expect("a time");
    assert!(
        finished.ends_with('Z') && finished.parse::<jiff::Timestamp>().is_ok(),
        "{finished}"
    );
}

#[test]
fn a_session_without_a_result_prints_a_null_result_and_the_status_none() {
    let scene = scene();
    let entry = finish(&scene, &plain_session(), &[], None);

    let printed = read(&entry);

    assert_eq!(printed["result"], Value::Null);
    assert_eq!(printed["status"], "none");
    assert_eq!(printed["draft"], false);
}

#[test]
fn a_withdrawn_session_prints_its_unsent_draft_in_the_result() {
    let scene = scene();
    let session = plain_session();
    let unsent = UnsentDraft {
        note: Some("half way".to_owned()),
        answers: vec![Answer {
            selected: vec!["a".to_owned()],
            ..Answer::new("q0")
        }],
    };
    let result = SessionResult::withdrawn(
        "hero-2",
        &sha_of(&session),
        "2026-08-29T10:39:00Z",
        Some(unsent),
    );
    let entry = finish(&scene, &session, &[], Some(result));

    let printed = read(&entry);

    assert_eq!(printed["status"], "withdrawn");
    assert_eq!(printed["draft"], true);
    assert_eq!(printed["result"]["draft"]["note"], "half way");
    assert_eq!(
        printed["result"]["draft"]["answers"][0]["selected"],
        json!(["a"])
    );
}

#[test]
fn a_rejected_session_prints_the_status_rejected() {
    let scene = scene();
    let session = plain_session();
    let result = SessionResult::cancelled(
        "hero-2",
        &sha_of(&session),
        "2026-08-29T10:39:00Z",
        Some("not now".to_owned()),
        None,
        Vec::new(),
    );
    let entry = finish(&scene, &session, &[], Some(result));

    let printed = read(&entry);

    assert_eq!(printed["status"], "rejected");
    assert_eq!(printed["result"]["reason"], "not now");
}

#[test]
fn a_session_that_ended_in_an_error_prints_the_status_error() {
    let scene = scene();
    let session = plain_session();
    let result = SessionResult::error(
        "hero-2",
        &sha_of(&session),
        "2026-08-29T10:39:00Z",
        "the picture is gone",
    );
    let entry = finish(&scene, &session, &[], Some(result));

    let printed = read(&entry);

    assert_eq!(printed["status"], "error");
    assert_eq!(printed["result"]["error"], "the picture is gone");
}

#[test]
fn the_result_is_printed_as_asqr_reads_it_without_null_options() {
    let scene = scene();
    let session = plain_session();
    let entry = finish(
        &scene,
        &session,
        &[],
        Some(SessionResult::error(
            "hero-2",
            &sha_of(&session),
            "2026-08-29T10:39:00Z",
            "boom",
        )),
    );

    let printed = read(&entry);

    let result = printed["result"].as_object().expect("an object");
    assert!(result.values().all(|value| !value.is_null()), "{result:?}");
    assert!(!result.contains_key("note"), "{result:?}");
}

#[test]
fn the_queue_is_the_one_the_entry_was_found_in() {
    let scene = scene_in("work");
    let entry = finish(&scene, &plain_session(), &[], None);

    let printed = read(&entry);

    assert_eq!(printed["queue"], "work");
}

#[test]
fn a_queue_given_as_a_directory_is_named_by_its_absolute_path() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let location = QueueLocation::at(scratch.path().join("q"));
    location.create_layout().expect("layout");
    let scene = Scene {
        _scratch: scratch,
        location,
    };
    let entry = finish(&scene, &plain_session(), &[], None);

    let printed = read(&entry);

    assert_eq!(
        printed["queue"],
        scene.location.dir().display().to_string().as_str()
    );
}

// =========================================================
// The session
// =========================================================

#[test]
fn the_session_is_printed_as_stored_with_the_paths_the_asker_wrote() {
    let scene = scene();
    let (name, bytes) = copy(b"webp bytes", "webp");
    let mut session = session_showing(vec![kept(&name, "/tmp/x/hero.png", &"ab".repeat(32))]);
    session["archived"] = json!({"session_sha256": "ab", "keep": "compact"});
    session["questions"][0]["from_a_newer_asqr"] = json!({"kept": true});
    let entry = finish(&scene, &session, &[(name, bytes)], None);

    let printed = read(&entry);

    let mut expected = session.clone();
    expected["questions"][0]["context"][0]["path"] = json!("/tmp/x/hero.png");
    assert_eq!(printed["session"], expected);
}

#[test]
fn a_title_with_control_characters_and_quotes_round_trips() {
    let scene = scene();
    let mut session = plain_session();
    session["title"] = json!("Pick \"one\"\n\u{1b}[2J\ttab");
    let entry = finish(&scene, &session, &[], None);

    let text = read(&entry).to_string();

    let parsed: Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(parsed["session"]["title"], "Pick \"one\"\n\u{1b}[2J\ttab");
    assert!(!text.contains('\u{1b}') && !text.contains('\n'), "{text:?}");
}

#[test]
fn a_bundled_session_prints_like_a_loose_one() {
    let scene = scene();
    let (name, bytes) = copy(b"bundled webp", "webp");
    let session = session_showing(vec![kept(&name, "/tmp/x/hero.png", &"ab".repeat(32))]);
    let month = Month::of(ulid());
    let session_name = EntryName {
        kind: EntryKind::Session,
        id: "hero-2".to_owned(),
        ulid: ulid(),
    };
    let zip_name = session_name.zip_name();
    let source = scene.location.dir().join("source");
    fs::write(&source, &bytes).expect("written");
    add_entries(
        &scene.location,
        month,
        &[
            NewEntry::named(
                session_name,
                serde_json::to_vec_pretty(&session).expect("serialises"),
            ),
            NewEntry::kept_file(&name, source).expect("a kept name"),
        ],
        std::time::SystemTime::now(),
    )
    .expect("bundled");
    let entry = EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Bundled(month, zip_name),
    };

    let printed = read(&entry);

    assert_eq!(files(&printed)[0]["state"], "kept");
    assert_eq!(files(&printed)[0]["problem"], Value::Null);
}

// =========================================================
// The files
// =========================================================

#[test]
fn a_kept_file_prints_what_the_session_recorded_of_it() {
    let scene = scene();
    let (name, bytes) = copy(b"webp bytes", "webp");
    let session = session_showing(vec![kept(&name, "/tmp/x/hero.png", &"ab".repeat(32))]);
    let entry = finish(&scene, &session, &[(name, bytes)], None);

    let printed = read(&entry);

    assert_eq!(
        files(&printed),
        &[json!({
            "original_path": "/tmp/x/hero.png",
            "state": "kept",
            "sha256": "ab".repeat(32),
            "size": 100,
            "modified": "2026-10-29T16:40:11Z",
            "kept_size": 40,
            "reason": null,
            "problem": null,
            "extracted": null,
        })]
    );
}

#[test]
fn a_missing_and_an_unreadable_file_are_both_gone_with_the_recorded_reason() {
    let scene = scene();
    let session = session_showing(vec![
        json!({"type": "code", "path": "/tmp/x/gone.md", "shown": {
            "original_path": "/tmp/x/gone.md", "state": "missing", "reason": "no such file"}}),
        json!({"type": "code", "path": "/tmp/x/locked.md", "shown": {
            "original_path": "/tmp/x/locked.md", "state": "unreadable",
            "reason": "permission denied"}}),
    ]);
    let entry = finish(&scene, &session, &[], None);

    let printed = read(&entry);

    let seen: Vec<(&str, &str, &str)> = files(&printed)
        .iter()
        .map(|file| {
            (
                file["original_path"].as_str().expect("text"),
                file["state"].as_str().expect("text"),
                file["reason"].as_str().expect("text"),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("/tmp/x/gone.md", "gone", "no such file"),
            ("/tmp/x/locked.md", "gone", "permission denied"),
        ]
    );
    assert_eq!(files(&printed)[0]["sha256"], Value::Null);
}

#[test]
fn a_file_asqr_did_not_keep_is_not_kept_and_one_without_a_record_is_not_recorded() {
    let scene = scene();
    let session = session_showing(vec![
        json!({"type": "code", "path": "/tmp/x/big.log", "shown": {
            "original_path": "/tmp/x/big.log", "state": "not_kept", "sha256": "cd".repeat(32),
            "size": 900, "reason": "larger than the limit"}}),
        json!({"type": "code", "path": "/tmp/x/old.rs"}),
    ]);
    let entry = finish(&scene, &session, &[], None);

    let printed = read(&entry);

    assert_eq!(files(&printed)[0]["state"], "not_kept");
    assert_eq!(files(&printed)[0]["reason"], "larger than the limit");
    assert_eq!(files(&printed)[1]["original_path"], "/tmp/x/old.rs");
    assert_eq!(files(&printed)[1]["state"], "not_recorded");
    assert_eq!(files(&printed)[1]["sha256"], Value::Null);
}

#[test]
fn a_damaged_copy_prints_the_problem_and_is_not_extracted() {
    let scene = scene();
    let (name, _) = copy(b"what the name promises", "webp");
    let session = session_showing(vec![kept(&name, "/tmp/x/hero.png", &"ab".repeat(32))]);
    let entry = finish(
        &scene,
        &session,
        &[(name, b"something else".to_vec())],
        None,
    );

    let printed = read(&entry);

    assert_eq!(files(&printed)[0]["state"], "kept");
    assert_eq!(files(&printed)[0]["problem"], "the kept copy is damaged");
    assert_eq!(files(&printed)[0]["extracted"], Value::Null);
}

#[test]
fn the_files_are_sorted_by_original_path_and_hash_whatever_order_the_session_names_them() {
    let scene = scene();
    let session = session_showing(vec![
        json!({"type": "diff",
               "old": {"path": "/tmp/b.rs"},
               "new": {"path": "/tmp/a.rs"}}),
        json!({"type": "code", "path": "/tmp/0.rs"}),
    ]);
    let entry = finish(&scene, &session, &[], None);

    let printed = read(&entry);

    let paths: Vec<&str> = files(&printed)
        .iter()
        .map(|file| file["original_path"].as_str().expect("text"))
        .collect();
    assert_eq!(paths, ["/tmp/0.rs", "/tmp/a.rs", "/tmp/b.rs"]);
}

#[test]
fn identical_bytes_at_two_originals_are_two_files() {
    let scene = scene();
    let (name, bytes) = copy(b"the same bytes", "webp");
    let sha = "ab".repeat(32);
    let session = session_showing(vec![
        kept(&name, "/tmp/x/first.png", &sha),
        kept(&name, "/tmp/y/second.png", &sha),
    ]);
    let entry = finish(&scene, &session, &[(name, bytes)], None);

    let printed = read(&entry);

    let paths: Vec<&str> = files(&printed)
        .iter()
        .map(|file| file["original_path"].as_str().expect("text"))
        .collect();
    assert_eq!(paths, ["/tmp/x/first.png", "/tmp/y/second.png"]);
}

#[test]
fn one_original_shown_with_two_contents_is_two_files() {
    let scene = scene();
    let (first, first_bytes) = copy(b"before", "webp");
    let (second, second_bytes) = copy(b"after", "webp");
    let session = session_showing(vec![
        kept(&first, "/tmp/x/hero.png", &"bb".repeat(32)),
        kept(&second, "/tmp/x/hero.png", &"aa".repeat(32)),
    ]);
    let entry = finish(
        &scene,
        &session,
        &[(first, first_bytes), (second, second_bytes)],
        None,
    );

    let printed = read(&entry);

    let hashes: Vec<&str> = files(&printed)
        .iter()
        .map(|file| file["sha256"].as_str().expect("text"))
        .collect();
    assert_eq!(hashes, ["aa".repeat(32), "bb".repeat(32)]);
}

#[test]
fn one_original_shown_twice_with_the_same_content_is_one_file() {
    let scene = scene();
    let (name, bytes) = copy(b"the same bytes", "webp");
    let sha = "ab".repeat(32);
    let session = session_showing(vec![
        kept(&name, "/tmp/x/hero.png", &sha),
        kept(&name, "/tmp/x/hero.png", &sha),
    ]);
    let entry = finish(&scene, &session, &[(name, bytes)], None);

    let printed = read(&entry);

    assert_eq!(files(&printed).len(), 1);
}
