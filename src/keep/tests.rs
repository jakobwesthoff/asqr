// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::record::{RecordedSession, kept_file_names, record};
use super::snapshot::{Limits, Seen, Snapshot, snapshot, snapshot_with};
use super::store::{place, stage};
use super::*;

// =========================================================
// Fixtures
// =========================================================

fn code_session(path: &str) -> Vec<u8> {
    session_with_context(json!({"type": "code", "path": path}))
}

fn session_with_context(entry: Value) -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "asqr": 2,
        "id": "hero-2",
        "questions": [{"id": "q", "text": "?", "kind": "text", "context": [entry]}]
    }))
    .expect("serializes")
}

fn written(directory: &Path, name: &str, bytes: &[u8]) -> String {
    let path = directory.join(name);
    fs::write(&path, bytes).expect("writes");
    path.to_string_lossy().into_owned()
}

fn recorded(session: &[u8], mode: KeepFiles) -> (Value, RecordedSession) {
    let seen = snapshot(session, &Limits::VIEWERS);
    let recording = record(session, &seen, mode);
    let json = serde_json::from_slice(&recording.json).expect("the recording is JSON");
    (json, recording)
}

fn first_entry(session: &Value) -> &Value {
    &session["questions"][0]["context"][0]
}

fn copy_names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .map(|entries| {
            entries
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

// =========================================================
// Recording
// =========================================================

#[test]
fn a_shown_file_is_recorded_with_its_fingerprint_and_copied() {
    let directory = tempfile::tempdir().expect("temp dir");
    let bytes = b"fn main() {}\n";
    let original = written(directory.path(), "main.rs", bytes);
    let session = code_session(&original);

    let (json, recording) = recorded(&session, KeepFiles::Compact);

    let name = format!("{}.rs", sha256_hex(bytes));
    let entry = first_entry(&json);
    assert_eq!(entry["path"], format!("files/{name}"));
    let shown = &entry["shown"];
    assert_eq!(shown["original_path"], original.as_str());
    assert_eq!(shown["state"], "kept");
    assert_eq!(shown["sha256"], sha256_hex(bytes));
    assert_eq!(shown["size"], bytes.len());
    assert_eq!(shown["kept_size"], bytes.len());
    assert!(
        shown["modified"]
            .as_str()
            .is_some_and(|time| time.ends_with('Z'))
    );
    assert_eq!(json["archived"]["session_sha256"], sha256_hex(&session));
    assert_eq!(json["archived"]["keep"], "compact");

    // The copy hashes to its name and lands under it.
    assert_eq!(recording.copies.len(), 1);
    assert_eq!(recording.copies[0].name, name);
    let files = directory.path().join("archive/files");
    for copy in recording.copies {
        let staged = stage(&files, &copy.bytes).expect("stages");
        place(&files, &copy.name, staged).expect("places");
    }
    assert_eq!(fs::read(files.join(&name)).expect("copy exists"), bytes);
}

#[test]
fn off_records_the_fingerprint_and_copies_nothing() {
    let directory = tempfile::tempdir().expect("temp dir");
    let bytes = b"fn main() {}\n";
    let original = written(directory.path(), "main.rs", bytes);

    let (json, recording) = recorded(&code_session(&original), KeepFiles::Off);

    let entry = first_entry(&json);
    assert_eq!(entry["path"], original.as_str());
    assert_eq!(entry["shown"]["state"], "not_kept");
    assert_eq!(entry["shown"]["sha256"], sha256_hex(bytes));
    assert_eq!(entry["shown"]["size"], bytes.len());
    assert!(entry["shown"].get("kept_size").is_none());
    assert_eq!(json["archived"]["keep"], "off");
    assert!(recording.copies.is_empty());
}

#[test]
fn a_missing_file_is_recorded_as_missing() {
    let directory = tempfile::tempdir().expect("temp dir");
    let gone = directory.path().join("gone.png");
    let original = gone.to_string_lossy();

    let (json, recording) = recorded(&code_session(&original), KeepFiles::Compact);

    let shown = &first_entry(&json)["shown"];
    assert_eq!(shown["state"], "missing");
    assert_eq!(shown["original_path"], original.as_ref());
    assert!(shown.get("sha256").is_none());
    assert!(shown.get("size").is_none());
    assert_eq!(first_entry(&json)["path"], original.as_ref());
    assert!(recording.copies.is_empty());
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_recorded_with_its_reason() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "secret.txt", b"x");
    fs::set_permissions(&original, fs::Permissions::from_mode(0o000)).expect("chmod");
    if fs::File::open(&original).is_ok() {
        // Running as root: the mode does not keep the file closed.
        return;
    }

    let (json, _) = recorded(&code_session(&original), KeepFiles::Compact);

    let shown = &first_entry(&json)["shown"];
    assert_eq!(shown["state"], "unreadable");
    assert!(
        shown["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("ermission denied")),
        "{shown}"
    );
    assert!(shown.get("sha256").is_none());
}

#[cfg(unix)]
#[test]
fn a_fifo_is_recorded_as_unreadable_without_blocking() {
    let directory = tempfile::tempdir().expect("temp dir");
    let fifo = directory.path().join("pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo runs");
    assert!(made.success());
    let session = code_session(&fifo.to_string_lossy());

    // It runs apart, so a hang fails the test instead of stalling the run.
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(snapshot(&session, &Limits::VIEWERS));
    });
    let seen = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("reading a FIFO must not block");

    assert_eq!(
        seen.places[0].seen,
        Seen::Unreadable("not a regular file".to_owned())
    );
}

#[test]
fn a_relative_path_is_recorded_as_unreadable() {
    let (json, _) = recorded(&code_session("files/q.png"), KeepFiles::Compact);

    let shown = &first_entry(&json)["shown"];
    assert_eq!(shown["state"], "unreadable");
    assert_eq!(shown["reason"], "relative path");
}

#[cfg(unix)]
#[test]
fn a_directory_and_dev_zero_are_unreadable() {
    let directory = tempfile::tempdir().expect("temp dir");
    for path in [
        directory.path().to_string_lossy().into_owned(),
        "/dev/zero".to_owned(),
    ] {
        let seen = snapshot(&code_session(&path), &Limits::VIEWERS);

        assert_eq!(
            seen.places[0].seen,
            Seen::Unreadable("not a regular file".to_owned()),
            "{path}"
        );
    }
}

#[test]
fn a_file_over_the_limit_is_not_kept_without_a_fingerprint() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "big.txt", b"0123456789");
    let session = code_session(&original);
    let limits = Limits {
        text: 4,
        ..Limits::VIEWERS
    };

    let seen = snapshot(&session, &limits);
    let recording = record(&session, &seen, KeepFiles::Compact);

    let json: Value = serde_json::from_slice(&recording.json).expect("JSON");
    let shown = &first_entry(&json)["shown"];
    assert_eq!(shown["state"], "not_kept");
    assert_eq!(shown["reason"], "larger than 4 bytes");
    assert_eq!(shown["size"], 10);
    assert!(shown.get("sha256").is_none());
    assert_eq!(first_entry(&json)["path"], original.as_str());
    assert!(recording.copies.is_empty());
}

#[test]
fn an_image_is_judged_by_the_image_limit_and_other_files_by_the_text_limit() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "big.bin", b"0123456789");
    let limits = Limits {
        image: 4,
        ..Limits::VIEWERS
    };

    let image = snapshot(
        &session_with_context(json!({"type": "image", "path": original})),
        &limits,
    );
    let code = snapshot(&code_session(&original), &limits);

    assert!(matches!(
        &image.places[0].seen,
        Seen::TooLarge { reason, .. } if reason == "larger than 4 bytes"
    ));
    assert!(matches!(code.places[0].seen, Seen::Read(_)));
}

#[test]
fn a_limit_in_whole_mebibytes_is_named_in_mebibytes() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(
        directory.path(),
        "big.txt",
        &vec![b'x'; 2 * 1024 * 1024 + 1],
    );
    let limits = Limits {
        text: 2 * 1024 * 1024,
        ..Limits::VIEWERS
    };

    let seen = snapshot(&code_session(&original), &limits);

    assert!(matches!(
        &seen.places[0].seen,
        Seen::TooLarge { reason, .. } if reason == "larger than 2 MiB"
    ));
}

#[test]
fn a_file_that_grows_while_it_is_read_is_unreadable() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "log.txt", b"short");

    let seen = snapshot_with(&code_session(&original), &Limits::VIEWERS, &|path| {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .and_then(|mut file| file.write_all(b" and longer"))
            .expect("appends");
    });

    assert_eq!(
        seen.places[0].seen,
        Seen::Unreadable("changed while it was read".to_owned())
    );
}

#[test]
fn two_sessions_showing_one_file_keep_it_once() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "shared.md", b"# shared\n");
    let files = directory.path().join("archive/files");

    for id in ["one", "two"] {
        let mut session: Value = serde_json::from_slice(&code_session(&original)).expect("JSON");
        session["id"] = json!(id);
        let session = serde_json::to_vec(&session).expect("serializes");
        let (_, recording) = recorded(&session, KeepFiles::Compact);
        for copy in recording.copies {
            let staged = stage(&files, &copy.bytes).expect("stages");
            place(&files, &copy.name, staged).expect("places");
        }
    }

    assert_eq!(
        copy_names(&files),
        [format!("{}.md", sha256_hex(b"# shared\n"))]
    );
}

#[test]
fn a_copy_that_differs_from_a_file_of_its_name_replaces_it() {
    let directory = tempfile::tempdir().expect("temp dir");
    let files = directory.path().join("files");
    fs::create_dir_all(&files).expect("creates");
    fs::write(files.join("damaged.txt"), b"damaged").expect("writes");

    let staged = stage(&files, b"whole").expect("stages");
    place(&files, "damaged.txt", staged).expect("places");

    assert_eq!(
        fs::read(files.join("damaged.txt")).expect("reads"),
        b"whole"
    );
    assert_eq!(copy_names(&files), ["damaged.txt"]);
}

#[test]
fn both_sides_of_a_diff_and_option_context_are_recorded() {
    let directory = tempfile::tempdir().expect("temp dir");
    let old = written(directory.path(), "old.rs", b"old\n");
    let new = written(directory.path(), "new.rs", b"new\n");
    let picture = written(directory.path(), "pick.png", b"not really a png");
    let session = serde_json::to_vec(&json!({
        "asqr": 2,
        "id": "s",
        "questions": [{
            "id": "q", "text": "?", "kind": "single",
            "context": [{"type": "diff", "old": {"path": old}, "new": {"path": new}}],
            "options": [{"id": "a", "label": "A",
                "context": [{"type": "image", "path": picture}]}]
        }]
    }))
    .expect("serializes");

    let (json, recording) = recorded(&session, KeepFiles::Compact);

    let diff = first_entry(&json);
    assert_eq!(diff["old"]["shown"]["state"], "kept");
    assert_eq!(diff["new"]["shown"]["state"], "kept");
    assert_eq!(
        diff["old"]["path"],
        format!("files/{}.rs", sha256_hex(b"old\n"))
    );
    let option = &json["questions"][0]["options"][0]["context"][0];
    assert_eq!(option["shown"]["state"], "kept");
    assert_eq!(recording.copies.len(), 3);
}

#[test]
fn inline_content_is_left_alone() {
    let session = session_with_context(json!({"type": "code", "content": "let x = 1;"}));

    let (json, recording) = recorded(&session, KeepFiles::Compact);

    assert_eq!(
        first_entry(&json),
        &json!({"type": "code", "content": "let x = 1;"})
    );
    assert!(recording.copies.is_empty());
}

#[test]
fn unknown_fields_survive() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "a.txt", b"a");
    let session = serde_json::to_vec(&json!({
        "asqr": 2, "id": "s", "x-extra": {"keep": "me"},
        "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "code", "path": original, "x-entry": 7}]}]
    }))
    .expect("serializes");

    let (json, _) = recorded(&session, KeepFiles::Compact);

    assert_eq!(json["x-extra"], json!({"keep": "me"}));
    assert_eq!(first_entry(&json)["x-entry"], 7);
}

#[test]
fn a_file_past_the_memory_cap_goes_to_the_worker_as_a_path() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "big.txt", b"twenty bytes of text");
    let session = code_session(&original);
    let limits = Limits {
        memory: 5,
        ..Limits::VIEWERS
    };

    let mut seen = snapshot(&session, &limits);
    assert_eq!(seen.places[0].seen, Seen::Deferred);
    assert_eq!(seen.places[0].path, original);

    seen.resolve_deferred(&limits);
    let Seen::Read(file) = &seen.places[0].seen else {
        panic!("the worker reads it: {:?}", seen.places[0].seen);
    };
    assert_eq!(file.sha256, sha256_hex(b"twenty bytes of text"));
}

#[test]
fn a_file_nobody_read_in_time_is_recorded_as_unreadable() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "big.txt", b"twenty bytes of text");
    let session = code_session(&original);
    let limits = Limits {
        memory: 5,
        ..Limits::VIEWERS
    };

    let seen = snapshot(&session, &limits);
    let recording = record(&session, &seen, KeepFiles::Compact);

    let json: Value = serde_json::from_slice(&recording.json).expect("JSON");
    assert_eq!(first_entry(&json)["shown"]["state"], "unreadable");
}

#[test]
fn bytes_that_are_no_session_record_nothing() {
    for bytes in [&b"not json"[..], b"[1, 2]", b"{\"questions\": 3}"] {
        let seen = snapshot(bytes, &Limits::VIEWERS);
        assert_eq!(seen, Snapshot::default());

        let recording = record(bytes, &seen, KeepFiles::Compact);

        assert_eq!(recording.json, bytes);
        assert!(recording.copies.is_empty());
    }
}

#[test]
fn entries_that_show_no_file_are_skipped() {
    let session = serde_json::to_vec(&json!({
        "asqr": 2, "id": "s",
        "questions": [
            3,
            {"id": "q", "text": "?", "kind": "single", "tags": [1],
             "context": [
                {"type": "widget", "path": "/a"},
                5,
                {"type": "diff", "name": "x.rs", "old": {"content": "c"}, "new": {"path": 7}}],
             "options": "none"}
        ]
    }))
    .expect("serializes");

    assert_eq!(snapshot(&session, &Limits::VIEWERS), Snapshot::default());
    assert!(kept_file_names(&session).is_empty());
    assert!(kept_file_names(b"not json").is_empty());
}

#[test]
fn resolving_leaves_the_files_already_read_alone() {
    let directory = tempfile::tempdir().expect("temp dir");
    let small = written(directory.path(), "small.txt", b"ab");
    let big = written(directory.path(), "big.txt", b"twenty bytes of text");
    let session = serde_json::to_vec(&json!({
        "asqr": 2, "id": "s",
        "questions": [{"id": "q", "text": "?", "kind": "single", "options": [
            {"id": "a", "label": "A", "context": [{"type": "code", "path": small}]},
            {"id": "b", "label": "B", "context": [{"type": "code", "path": big}]}]}]
    }))
    .expect("serializes");
    let limits = Limits {
        memory: 5,
        ..Limits::VIEWERS
    };
    let mut seen = snapshot(&session, &limits);
    let before = seen.places[0].clone();

    seen.resolve_deferred(&limits);

    assert!(matches!(before.seen, Seen::Read(_)));
    assert_eq!(seen.places[0], before);
    assert!(matches!(seen.places[1].seen, Seen::Read(_)));
}

#[test]
fn a_name_that_cannot_be_read_is_an_error() {
    let directory = tempfile::tempdir().expect("temp dir");
    let files = directory.path().join("files");
    fs::create_dir_all(files.join("name.txt")).expect("creates");
    let staged = stage(&files, b"bytes").expect("stages");

    assert!(place(&files, "name.txt", staged).is_err());
}

#[test]
fn a_snapshot_of_another_session_records_nothing() {
    let directory = tempfile::tempdir().expect("temp dir");
    let one = code_session(&written(directory.path(), "one.txt", b"1"));
    let other = code_session(&written(directory.path(), "other.txt", b"2"));
    let seen = snapshot(&one, &Limits::VIEWERS);

    let recording = record(&other, &seen, KeepFiles::Compact);

    assert_eq!(recording.json, other);
    assert!(recording.copies.is_empty());
}

#[test]
fn an_archived_object_a_session_brought_along_is_replaced() {
    let directory = tempfile::tempdir().expect("temp dir");
    let original = written(directory.path(), "a.txt", b"a");
    let mut session: Value = serde_json::from_slice(&code_session(&original)).expect("JSON");
    session["archived"] = json!({"session_sha256": "forged", "keep": "off"});
    let session = serde_json::to_vec(&session).expect("serializes");

    let (json, _) = recorded(&session, KeepFiles::Compact);

    assert_eq!(json["archived"]["session_sha256"], sha256_hex(&session));
    assert_eq!(json["archived"]["keep"], "compact");
}

// =========================================================
// Which paths are kept files
// =========================================================

#[test]
fn only_well_formed_kept_names_are_listed() {
    let hash = "a".repeat(64);
    let good = format!("files/{hash}.webp");
    let session = serde_json::to_vec(&json!({
        "asqr": 2, "id": "s",
        "questions": [{"id": "q", "text": "?", "kind": "text", "context": [
            {"type": "image", "path": good},
            {"type": "image", "path": "files/../x"},
            {"type": "image", "path": format!("files/{}.png", "A".repeat(64))},
            {"type": "image", "path": format!("files/{hash}.PNG")},
            {"type": "image", "path": format!("files/{hash}.toolongext")},
            {"type": "image", "path": format!("files/{hash}")},
            {"type": "image", "path": format!("/abs/files/{hash}.png")},
            {"type": "image", "path": format!("files/{}.png", "a".repeat(63))},
            {"type": "image", "path": good},
        ]}]
    }))
    .expect("serializes");

    assert_eq!(
        kept_file_names(&session),
        [good.trim_start_matches("files/")]
    );
}
