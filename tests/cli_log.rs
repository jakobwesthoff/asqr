// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The log file in the platform cache directory (ADR 15).

mod common;

use asqr::format::{Answer, SessionResult};
use asqr::queue::{QueueLocation, save_draft};
use common::{Sandbox, stderr, stdout};

/// A filter in `ASQR_LOG` that does not parse leaves the command and its
/// output alone (ADR 83). The warning goes into the log instead.
#[test]
fn an_asqr_log_that_does_not_parse_warns_in_the_log_only() {
    let sandbox = Sandbox::new();
    let plain = sandbox.asqr_in_queue(&["paths"]).output().expect("runs");

    let filtered = sandbox
        .asqr_in_queue(&["paths"])
        .env("ASQR_LOG", "asqr=loud")
        .output()
        .expect("runs");

    assert!(filtered.status.success());
    assert_eq!(stdout(&filtered), stdout(&plain));
    assert_eq!(stderr(&filtered), stderr(&plain));
    let log = std::fs::read_to_string(sandbox.log_path()).expect("the log file exists");
    let warning = log
        .lines()
        .find(|line| line.contains("ASQR_LOG"))
        .unwrap_or_else(|| panic!("no warning about ASQR_LOG in {log:?}"));
    assert!(warning.contains("WARN"), "{warning}");
    assert!(warning.contains(r#"value="asqr=loud""#), "{warning}");
}

#[test]
fn commands_log_what_they_changed() {
    let sandbox = Sandbox::new();
    let file = sandbox.file(
        "session.json",
        r#"{"asqr": 2, "id": "logged", "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
    );

    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();

    let log = std::fs::read_to_string(sandbox.log_path()).expect("the log file exists");
    assert!(log.contains("dropped session"), "{log}");
    assert!(log.contains("logged"), "{log}");
}

#[test]
fn an_unwritable_log_does_not_stop_the_command() {
    let sandbox = Sandbox::new();
    let log = sandbox.log_path();
    // Asking for the path already opened the log; a directory takes its
    // place.
    std::fs::remove_file(&log).expect("the log file was created");
    std::fs::create_dir_all(&log).expect("directory in the way");

    sandbox.asqr_in_queue(&["status"]).assert().success();
}

#[test]
fn an_unusable_cache_directory_does_not_stop_the_command() {
    let sandbox = Sandbox::new();
    let cache = sandbox
        .log_path()
        .parent()
        .expect("the log has a directory")
        .to_path_buf();
    std::fs::remove_dir_all(&cache).expect("cache directory removed");
    std::fs::write(&cache, "a file where the cache directory should be").expect("written");

    sandbox.asqr_in_queue(&["status"]).assert().success();
}

const MEBIBYTE: usize = 1 << 20;

/// The files in the directory of the log, sorted, but for the rotation
/// lock.
fn files_beside(log: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(log.parent().expect("the log has a directory"))
        .expect("listed")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != "asqr.rotate.lock")
        .collect();
    names.sort();
    names
}

fn decoded(path: &std::path::Path) -> Vec<u8> {
    use std::io::Read;

    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).expect("opened"))
        .read_to_end(&mut bytes)
        .expect("gunzipped");
    bytes
}

#[test]
fn a_log_of_one_mebibyte_rotates_when_a_command_starts() {
    let sandbox = Sandbox::new();
    // Asking for the path runs a command too, so it comes before the log
    // grows.
    let log = sandbox.log_path();
    let full = vec![b'x'; MEBIBYTE];
    std::fs::write(&log, &full).expect("written");

    sandbox.asqr_in_queue(&["status"]).assert().success();

    let files = files_beside(&log);
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(files.contains(&"asqr.log".to_owned()), "{files:?}");
    // The rotation that moves a log leaves it plain.
    let moved = files
        .iter()
        .find(|name| name.starts_with("asqr-") && name.ends_with(".log"))
        .unwrap_or_else(|| panic!("no moved log in {files:?}"));
    assert_eq!(
        std::fs::read(log.with_file_name(moved)).expect("read"),
        full
    );
    assert!(std::fs::metadata(&log).expect("the new log").len() < MEBIBYTE as u64);
    // `asqr paths` keeps naming the current file.
    assert_eq!(sandbox.log_path(), log);
}

#[test]
fn a_plain_log_a_day_old_is_compressed_by_the_next_rotation() {
    let sandbox = Sandbox::new();
    let log = sandbox.log_path();
    let earlier = log.with_file_name("asqr-20260101T000000Z.log");
    std::fs::write(&earlier, b"an earlier log\n").expect("written");
    let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);
    std::fs::File::options()
        .write(true)
        .open(&earlier)
        .expect("opened")
        .set_modified(two_days_ago)
        .expect("time set");
    std::fs::write(&log, vec![b'x'; MEBIBYTE]).expect("written");

    sandbox.asqr_in_queue(&["status"]).assert().success();

    assert!(!earlier.exists());
    assert_eq!(
        decoded(&earlier.with_file_name("asqr-20260101T000000Z.log.gz")),
        b"an earlier log\n"
    );
}

/// The session `round-1` waits and the person has started on it: a draft
/// with a custom answer and a note, both text they typed. Returns the file
/// of `round-2`, which replaces `round-1`.
fn started_on_round_one(sandbox: &Sandbox) -> std::path::PathBuf {
    let session = |id: &str| {
        format!(
            r#"{{"asqr": 2, "id": "{id}", "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#
        )
    };
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(sandbox.file("round-1.json", &session("round-1")))
        .assert()
        .success();
    let typed = Answer {
        custom: Some("half-typed secret".into()),
        ..Answer::new("q")
    };
    let mut draft = SessionResult::draft("round-1", "q", vec![typed]);
    draft.note = Some("a private note".into());
    save_draft(&QueueLocation::at(sandbox.queue_dir()), &draft).expect("saved");
    sandbox.file("round-2.json", &session("round-2"))
}

#[test]
fn replacing_a_session_does_not_log_what_the_person_typed() {
    let sandbox = Sandbox::new();
    let second = started_on_round_one(&sandbox);

    sandbox
        .asqr_in_queue(&["ask", "--replaces", "round-1", "--force"])
        .arg(&second)
        .assert()
        .success();

    let log = std::fs::read_to_string(sandbox.log_path()).expect("the log file exists");
    assert!(log.contains("replaced a session"), "{log}");
    assert!(!log.contains("half-typed secret"), "{log}");
    assert!(!log.contains("a private note"), "{log}");
}

#[test]
fn the_line_of_a_replaced_session_names_ids_status_and_counts() {
    let sandbox = Sandbox::new();
    let second = started_on_round_one(&sandbox);

    sandbox
        .asqr_in_queue(&["ask", "--replaces", "round-1", "--force"])
        .arg(&second)
        .assert()
        .success();

    let log = std::fs::read_to_string(sandbox.log_path()).expect("the log file exists");
    let line = log
        .lines()
        .find(|line| line.contains("replaced a session"))
        .unwrap_or_else(|| panic!("no replaced-a-session line in {log:?}"));
    for field in [
        r#"id="round-2""#,
        r#"old="round-1""#,
        r#"outcome="now""#,
        "status=Withdrawn",
        "answers=0",
        "draft_answers=1",
    ] {
        assert!(line.contains(field), "{field} missing from {line}");
    }
}
