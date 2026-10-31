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
    let (loaded, _) = load_for_reading(entry).expect("loaded");
    replay_json(&loaded, None)
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

// =========================================================
// Extracting into a directory
// =========================================================

/// The entry names below `dir`, sorted.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("listed")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Loads `entry` and writes it below `dir` the way the command does.
fn extract_into(entry: &EntryRef, dir: &Path) -> (LoadedReplay, Vec<KeptCopy>) {
    let (loaded, copies) = load_for_reading(entry).expect("loaded");
    write_extraction(dir, &loaded, &copies, &mut write_to_disk).expect("written");
    (loaded, copies)
}

/// A scene with one kept image and the result that answers it.
fn answered_image() -> (Scene, EntryRef, String, Vec<u8>) {
    let scene = scene();
    let (name, bytes) = copy(b"webp bytes", "webp");
    let session = session_showing(vec![kept(&name, "/tmp/x/hero.png", &"ab".repeat(32))]);
    let entry = finish(
        &scene,
        &session,
        &[(name.clone(), bytes.clone())],
        Some(submitted(&session)),
    );
    (scene, entry, name, bytes)
}

fn out_dir(scene: &Scene) -> PathBuf {
    scene._scratch.path().join("out")
}

#[test]
fn the_session_the_result_and_each_kept_copy_land_in_the_directory() {
    let (scene, entry, name, bytes) = answered_image();
    let out = out_dir(&scene);

    let (loaded, _) = extract_into(&entry, &out);

    assert_eq!(names_in(&out), ["files", "result.json", "session.json"]);
    assert_eq!(names_in(&out.join("files")), std::slice::from_ref(&name));
    assert_eq!(
        fs::read(out.join("files").join(&name)).expect("read"),
        bytes
    );
    assert!(name.starts_with(&sha256_hex(&bytes)), "{name}");
    let result: Value =
        serde_json::from_slice(&fs::read(out.join("result.json")).expect("read")).expect("JSON");
    assert_eq!(
        result,
        serde_json::to_value(loaded.result.expect("a result")).expect("plain data")
    );
}

#[test]
fn session_json_is_the_stored_session_and_its_file_paths_exist_below_the_directory() {
    let (scene, entry, _, _) = answered_image();
    let out = out_dir(&scene);

    let (loaded, _) = extract_into(&entry, &out);

    let written: Value =
        serde_json::from_slice(&fs::read(out.join("session.json")).expect("read")).expect("JSON");
    assert_eq!(written, loaded.raw);
    let path = written["questions"][0]["context"][0]["path"]
        .as_str()
        .expect("a path");
    assert!(path.starts_with("files/"), "{path}");
    assert!(out.join(path).is_file());
}

#[test]
fn two_files_kept_as_dat_with_different_bytes_are_two_files() {
    let scene = scene();
    let (first, first_bytes) = copy(b"all:\n\tcc a.c\n", "dat");
    let (second, second_bytes) = copy(b"all:\n\tcc b.c\n", "dat");
    let code = |name: &str, original: &str, sha: &str| {
        json!({"type": "code", "path": format!("files/{name}"), "shown": {
            "original_path": original, "state": "kept", "sha256": sha, "size": 14, "kept_size": 14}})
    };
    let session = session_showing(vec![
        code(&first, "/a/Makefile", &"aa".repeat(32)),
        code(&second, "/b/Makefile", &"bb".repeat(32)),
    ]);
    let entry = finish(
        &scene,
        &session,
        &[(first.clone(), first_bytes), (second.clone(), second_bytes)],
        None,
    );
    let out = out_dir(&scene);

    extract_into(&entry, &out);

    let mut expected = [first, second];
    expected.sort();
    assert_eq!(names_in(&out.join("files")), expected);
}

#[test]
fn a_file_shown_twice_is_written_once() {
    let scene = scene();
    let (name, bytes) = copy(b"the same bytes", "webp");
    let session = session_showing(vec![
        kept(&name, "/tmp/x/first.png", &"ab".repeat(32)),
        kept(&name, "/tmp/y/second.png", &"ab".repeat(32)),
    ]);
    let entry = finish(&scene, &session, &[(name.clone(), bytes)], None);
    let out = out_dir(&scene);

    let (_, copies) = extract_into(&entry, &out);

    assert_eq!(copies.len(), 1);
    assert_eq!(names_in(&out.join("files")), [name]);
}

#[test]
fn a_session_without_a_result_writes_no_result_json() {
    let scene = scene();
    let entry = finish(&scene, &plain_session(), &[], None);
    let out = out_dir(&scene);

    extract_into(&entry, &out);

    assert_eq!(names_in(&out), ["files", "session.json"]);
}

#[test]
fn a_damaged_copy_is_not_written() {
    let scene = scene();
    let (name, _) = copy(b"what the name promises", "webp");
    let session = session_showing(vec![kept(&name, "/tmp/x/hero.png", &"ab".repeat(32))]);
    let entry = finish(
        &scene,
        &session,
        &[(name, b"something else".to_vec())],
        None,
    );
    let out = out_dir(&scene);

    extract_into(&entry, &out);

    assert_eq!(names_in(&out.join("files")), Vec::<String>::new());
}

#[test]
fn a_missing_parent_is_created() {
    let scene = scene();
    let entry = finish(&scene, &plain_session(), &[], None);
    let out = out_dir(&scene).join("deep").join("er");

    extract_into(&entry, &out);

    assert!(out.join("session.json").is_file());
}

#[cfg(unix)]
#[test]
fn the_directories_are_private_to_the_user_and_so_are_the_files() {
    use std::os::unix::fs::PermissionsExt;
    let (scene, entry, name, _) = answered_image();
    let out = out_dir(&scene).join("deep").join("er");

    extract_into(&entry, &out);

    let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode(&out), 0o700);
    assert_eq!(mode(out.parent().expect("a parent")), 0o700);
    assert_eq!(mode(&out.join("files")), 0o700);
    assert_eq!(mode(&out.join("session.json")), 0o600);
    assert_eq!(mode(&out.join("files").join(name)), 0o600);
}

#[test]
fn an_existing_empty_directory_is_filled_and_no_staging_directory_stays() {
    let (scene, entry, _, _) = answered_image();
    let out = out_dir(&scene);
    fs::create_dir(&out).expect("made");

    extract_into(&entry, &out);

    assert!(out.join("session.json").is_file());
    assert_eq!(names_in(out.parent().expect("a parent")), ["out", "queues"]);
}

#[test]
fn a_write_that_fails_leaves_no_staging_directory_and_no_created_parent() {
    let (scene, entry, _, _) = answered_image();
    let out = out_dir(&scene).join("deep").join("er");
    let (loaded, copies) = load_for_reading(&entry).expect("loaded");
    let mut calls = 0;

    let error = write_extraction(&out, &loaded, &copies, &mut |path, bytes| {
        calls += 1;
        if calls == 2 {
            return Err(io::Error::other("disk full"));
        }
        write_to_disk(path, bytes)
    })
    .expect_err("refused");

    assert!(error.to_string().contains("disk full"), "{error}");
    assert_eq!(
        names_in(out_dir(&scene).parent().expect("a parent")),
        ["queues"]
    );
}

#[test]
fn a_write_that_fails_leaves_an_existing_empty_directory_empty() {
    let (scene, entry, _, _) = answered_image();
    let out = out_dir(&scene);
    fs::create_dir(&out).expect("made");
    let (loaded, copies) = load_for_reading(&entry).expect("loaded");

    write_extraction(&out, &loaded, &copies, &mut |_, _| {
        Err(io::Error::other("disk full"))
    })
    .expect_err("refused");

    assert_eq!(names_in(&out), Vec::<String>::new());
    assert_eq!(names_in(out.parent().expect("a parent")), ["out", "queues"]);
}

#[test]
fn the_summary_lists_what_was_written_and_each_file_without_a_good_copy() {
    let scene = scene();
    let (name, bytes) = copy(b"webp bytes", "webp");
    let (damaged, _) = copy(b"what the name promises", "webp");
    let session = session_showing(vec![
        kept(&name, "/tmp/x/hero.png", &"ab".repeat(32)),
        kept(&damaged, "/tmp/x/broken.png", &"cd".repeat(32)),
        json!({"type": "code", "path": "/tmp/x/gone\n.md", "shown": {
            "original_path": "/tmp/x/gone\n.md", "state": "missing", "reason": "no such file"}}),
        json!({"type": "code", "path": "/tmp/x/old.rs"}),
    ]);
    let entry = finish(
        &scene,
        &session,
        &[(name, bytes), (damaged, b"something else".to_vec())],
        Some(submitted(&session)),
    );
    let (loaded, copies) = load_for_reading(&entry).expect("loaded");
    let out = Path::new("/out/dir");

    let text = summary(out, &loaded, &copies);

    assert_eq!(
        text,
        "extracted to /out/dir\n\
         session.json\n\
         result.json\n\
         1 file in files/\n\
         /tmp/x/broken.png: the kept copy is damaged\n\
         /tmp/x/gone\u{fffd}.md: gone\n\
         /tmp/x/old.rs: not_recorded\n"
    );
}

#[test]
fn the_summary_says_when_there_is_no_result() {
    let scene = scene();
    let entry = finish(&scene, &plain_session(), &[], None);
    let (loaded, copies) = load_for_reading(&entry).expect("loaded");

    let text = summary(Path::new("/out"), &loaded, &copies);

    assert_eq!(
        text,
        "extracted to /out\nsession.json\nno result\n0 files in files/\n"
    );
}

#[test]
fn json_names_the_directory_and_where_each_good_copy_lies_in_it() {
    let (scene, entry, name, _) = answered_image();
    let (loaded, _) = load_for_reading(&entry).expect("loaded");
    let out = out_dir(&scene);

    let printed = replay_json(&loaded, Some(&out));

    assert_eq!(printed["extracted_to"], out.display().to_string().as_str());
    assert_eq!(files(&printed)[0]["extracted"], format!("files/{name}"));
}

// =========================================================
// Where an extraction may go
// =========================================================

#[test]
fn an_absent_directory_may_be_extracted_into_and_is_named_absolutely() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let target = scratch.path().join("new").join("out");

    let resolved = prepare_target(&target, &[]).expect("allowed");

    assert!(resolved.is_absolute());
    assert!(resolved.ends_with("new/out"), "{resolved:?}");
}

#[test]
fn an_empty_directory_may_be_extracted_into() {
    let scratch = tempfile::tempdir().expect("temp dir");

    assert!(prepare_target(scratch.path(), &[]).is_ok());
}

#[test]
fn a_directory_with_a_file_is_refused() {
    let scratch = tempfile::tempdir().expect("temp dir");
    fs::write(scratch.path().join("notes.txt"), "x").expect("written");

    let why = prepare_target(scratch.path(), &[]).expect_err("refused");

    assert!(why.contains("is not empty"), "{why}");
}

#[test]
fn a_directory_with_only_a_hidden_file_is_not_empty() {
    let scratch = tempfile::tempdir().expect("temp dir");
    fs::write(scratch.path().join(".hidden"), "x").expect("written");

    let why = prepare_target(scratch.path(), &[]).expect_err("refused");

    assert!(why.contains("is not empty"), "{why}");
}

#[test]
fn a_path_that_is_a_file_is_refused() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let file = scratch.path().join("file");
    fs::write(&file, "x").expect("written");

    let why = prepare_target(&file, &[]).expect_err("refused");

    assert!(why.contains("is not a directory"), "{why}");
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_to_an_empty_directory_is_refused() {
    let scratch = tempfile::tempdir().expect("temp dir");
    fs::create_dir(scratch.path().join("real")).expect("made");
    let link = scratch.path().join("link");
    std::os::unix::fs::symlink(scratch.path().join("real"), &link).expect("linked");

    let why = prepare_target(&link, &[]).expect_err("refused");

    assert!(why.contains("is a symbolic link"), "{why}");
}

#[test]
fn a_directory_inside_a_kept_out_directory_is_refused_though_it_is_empty_or_absent() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let queue = scratch.path().join("queue");
    fs::create_dir_all(queue.join("inbox")).expect("made");

    let inside_existing = prepare_target(&queue.join("inbox"), std::slice::from_ref(&queue));
    let inside_absent = prepare_target(&queue.join("new"), std::slice::from_ref(&queue));
    let the_queue_itself = prepare_target(&queue, std::slice::from_ref(&queue));

    for result in [inside_existing, inside_absent, the_queue_itself] {
        let why = result.expect_err("refused");
        assert!(why.contains("lies inside"), "{why}");
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_into_a_kept_out_directory_is_refused() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let queue = scratch.path().join("queue");
    fs::create_dir_all(queue.join("inbox")).expect("made");
    let link = scratch.path().join("elsewhere");
    std::os::unix::fs::symlink(&queue, &link).expect("linked");

    let why = prepare_target(&link.join("new"), &[queue]).expect_err("refused");

    assert!(why.contains("lies inside"), "{why}");
}

#[test]
fn an_empty_directory_name_is_no_directory() {
    assert!(parse_directory("").is_err());
    assert_eq!(
        parse_directory("out").expect("parsed"),
        PathBuf::from("out")
    );
}
