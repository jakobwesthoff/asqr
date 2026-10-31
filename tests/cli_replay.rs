// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr replay <ref>` resolves a reference to one finished session in
//! any queue under the data directory and opens it (spec section 6.5).
//! The terminal UI itself runs in `tui_pty.rs`.

mod common;

use std::fs;
use std::time::{Duration, SystemTime};

use asqr::queue::{Archived, QueueLocation, archive_name};
use common::{Sandbox, stderr};
use ulid::Ulid;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn session(title: &str) -> String {
    format!(
        r#"{{"asqr": 2, "id": "x", "title": "{title}", "questions": [{{"id": "q", "text": "?", "kind": "text"}}]}}"#
    )
}

/// A session archived loose `days` ago with the random part `random`; its
/// reference.
fn archive(location: &QueueLocation, id: &str, title: &str, days: u32, random: u128) -> String {
    location.create_layout().expect("layout");
    let millis = (SystemTime::now() - DAY * days)
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64;
    let ulid = Ulid::from_parts(millis, random);
    fs::write(
        location
            .archive()
            .join(archive_name(id, ulid, Archived::Session)),
        session(title),
    )
    .expect("written");
    let text = ulid.to_string();
    text[text.len() - 8..].to_owned()
}

fn replay(sandbox: &Sandbox, args: &[&str]) -> std::process::Output {
    sandbox.asqr().args(args).output().expect("runs")
}

#[test]
fn a_reference_that_is_too_short_is_a_usage_error() {
    let sandbox = Sandbox::new();

    let output = replay(&sandbox, &["replay", "9PA7"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "asqr: \"9PA7\" is too short; give at least 6 characters of the REF\n"
    );
}

#[test]
fn a_reference_that_is_too_long_is_a_usage_error() {
    let sandbox = Sandbox::new();

    let output = replay(&sandbox, &["replay", "9PA7KXZQ1"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "asqr: \"9PA7KXZQ1\" is too long; a REF has at most 8 characters\n"
    );
}

#[test]
fn a_letter_no_reference_holds_is_a_usage_error_naming_it() {
    let sandbox = Sandbox::new();

    let output = replay(&sandbox, &["replay", "9PA7KO"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "asqr: \"9PA7KO\" is no REF: no REF holds the letter O\n"
    );
}

#[test]
fn a_reference_that_ends_no_session_says_where_it_looked() {
    let sandbox = Sandbox::new();
    archive(
        &QueueLocation::named(&sandbox.data_dir(), "default"),
        "a",
        "A",
        1,
        0x1234_5678_9abc,
    );

    let output = replay(&sandbox, &["replay", "ZZZZZZ"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "asqr: no finished session ends in \"ZZZZZZ\" in any queue under the data directory; a queue given with --dir is only searched when named\n"
    );
}

#[test]
fn a_reference_that_ends_two_sessions_lists_them() {
    let sandbox = Sandbox::new();
    let root = sandbox.data_dir();
    // The last 30 bits are the same, the bits before them differ.
    let first = archive(
        &QueueLocation::named(&root, "default"),
        "a",
        "Alt rework 3",
        3,
        0x01_2345_6789,
    );
    let second = archive(
        &QueueLocation::named(&root, "work"),
        "b",
        "Release notes",
        9,
        0x02_2345_6789,
    );
    let shared = &first[2..];

    let output = replay(&sandbox, &["replay", shared]);

    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        format!("asqr: \"{shared}\" ends 2 finished sessions; give more of the REF:")
    );
    assert!(
        lines[1].starts_with(&format!("  {first}  default ")) && lines[1].ends_with("Alt rework 3"),
        "{text}"
    );
    assert!(
        lines[2].starts_with(&format!("  {second}  work    "))
            && lines[2].ends_with("Release notes"),
        "{text}"
    );
}

#[test]
fn a_session_found_needs_a_terminal() {
    let sandbox = Sandbox::new();
    let reference = archive(
        &QueueLocation::named(&sandbox.data_dir(), "work"),
        "a",
        "A",
        1,
        0x1234_5678_9abc,
    );

    let output = replay(&sandbox, &["replay", &reference.to_lowercase()]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "error: asqr needs a terminal; run it in a terminal of its own\n"
    );
}

#[test]
fn a_queue_outside_the_data_directory_is_searched_when_it_is_named() {
    let sandbox = Sandbox::new();
    let reference = archive(
        &QueueLocation::at(sandbox.queue_dir()),
        "a",
        "A",
        1,
        0x1234_5678_9abc,
    );

    let unnamed = replay(&sandbox, &["replay", &reference]);
    let named = sandbox
        .asqr_in_queue(&["replay", &reference])
        .output()
        .expect("runs");

    assert!(
        stderr(&unnamed).contains("no finished session ends in"),
        "{}",
        stderr(&unnamed)
    );
    assert!(
        stderr(&named).contains("needs a terminal"),
        "{}",
        stderr(&named)
    );
}

#[test]
fn an_unreadable_bundle_that_may_hold_the_entry_ends_the_replay_naming_it() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    location.create_layout().expect("layout");
    fs::write(location.archive().join("2026-01.zip"), "no zip").expect("written");

    let output = replay(&sandbox, &["replay", "ZZZZZZ"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with("error: cannot read "),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("2026-01.zip"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_session_that_cannot_be_shown_ends_the_replay_naming_the_file() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    for entry in fs::read_dir(location.archive()).expect("listed") {
        fs::write(entry.expect("entry").path(), "this is no session").expect("overwritten");
    }

    let output = replay(&sandbox, &["replay", &reference]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with("error: "),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("the archived session cannot be shown"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_replay_writes_nothing_into_the_queue() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    let listing = |dir: &std::path::Path| {
        let mut names: Vec<String> = walk(dir);
        names.sort();
        names
    };
    let before = listing(location.dir());

    replay(&sandbox, &["replay", &reference]);

    assert_eq!(listing(location.dir()), before);
}

fn walk(dir: &std::path::Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).expect("listed") {
        let path = entry.expect("entry").path();
        found.push(path.display().to_string());
        if path.is_dir() {
            found.extend(walk(&path));
        }
    }
    found
}

#[test]
fn a_session_found_beside_an_unreadable_bundle_warns_naming_the_bundle() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    fs::write(location.archive().join("2019-01.zip"), "no zip").expect("written");

    let output = replay(&sandbox, &["replay", &reference]);

    let text = stderr(&output);
    let warning = text.lines().next().expect("a line");
    assert!(
        warning.starts_with("warning: cannot read ") && warning.contains("2019-01.zip"),
        "{text}"
    );
    assert!(
        warning.ends_with("its sessions could not be searched for this REF"),
        "{text}"
    );
}

// =========================================================
// replay --json
// =========================================================

fn printed_object(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_str(&common::stdout(output)).expect("stdout is one JSON object")
}

#[test]
fn json_prints_the_session_as_one_object_without_a_terminal() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "work");
    let reference = archive(&location, "a", "A title", 1, 0x1234_5678_9abc);

    let output = replay(&sandbox, &["replay", &reference.to_lowercase(), "--json"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let printed = printed_object(&output);
    assert_eq!(printed["ref"], reference.as_str());
    assert_eq!(printed["queue"], "work");
    assert_eq!(printed["session"]["title"], "A title");
    assert_eq!(printed["status"], "none");
    assert!(common::stdout(&output).ends_with("}\n"));
    assert_eq!(stderr(&output), "");
}

#[test]
fn json_leaves_a_too_short_reference_a_usage_error_with_nothing_on_stdout() {
    let sandbox = Sandbox::new();

    let output = replay(&sandbox, &["replay", "9PA7", "--json"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "asqr: \"9PA7\" is too short; give at least 6 characters of the REF\n"
    );
    assert_eq!(common::stdout(&output), "");
}

#[test]
fn json_leaves_a_letter_no_reference_holds_a_usage_error_with_nothing_on_stdout() {
    let sandbox = Sandbox::new();

    let output = replay(&sandbox, &["replay", "9PA7KO", "--json"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "asqr: \"9PA7KO\" is no REF: no REF holds the letter O\n"
    );
    assert_eq!(common::stdout(&output), "");
}

#[test]
fn json_leaves_a_reference_that_ends_no_session_a_failure_with_nothing_on_stdout() {
    let sandbox = Sandbox::new();
    archive(
        &QueueLocation::named(&sandbox.data_dir(), "default"),
        "a",
        "A",
        1,
        0x1234_5678_9abc,
    );

    let output = replay(&sandbox, &["replay", "ZZZZZZ", "--json"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with("asqr: no finished session ends in \"ZZZZZZ\""),
        "{}",
        stderr(&output)
    );
    assert_eq!(common::stdout(&output), "");
}

#[test]
fn json_leaves_an_ambiguous_reference_a_failure_with_nothing_on_stdout() {
    let sandbox = Sandbox::new();
    let root = sandbox.data_dir();
    let first = archive(
        &QueueLocation::named(&root, "default"),
        "a",
        "Alt rework 3",
        3,
        0x01_2345_6789,
    );
    archive(
        &QueueLocation::named(&root, "work"),
        "b",
        "Release notes",
        9,
        0x02_2345_6789,
    );

    let output = replay(&sandbox, &["replay", &first[2..], "--json"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("finished sessions; give more of the REF:"),
        "{}",
        stderr(&output)
    );
    assert_eq!(common::stdout(&output), "");
}

#[test]
fn json_beside_an_unreadable_bundle_warns_naming_it_and_prints_the_session() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    fs::write(location.archive().join("2019-01.zip"), "no zip").expect("written");

    let output = replay(&sandbox, &["replay", &reference, "--json"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).starts_with("warning: cannot read ")
            && stderr(&output).contains("2019-01.zip"),
        "{}",
        stderr(&output)
    );
    assert_eq!(printed_object(&output)["ref"], reference.as_str());
}

#[test]
fn json_warns_once_about_a_bundle_both_the_search_and_the_pairing_could_not_read() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    fs::write(location.archive().join("2019-01.zip"), "no zip").expect("written");

    let output = replay(&sandbox, &["replay", &reference, "--json"]);

    let text = stderr(&output);
    assert_eq!(text.matches("2019-01.zip").count(), 1, "{text}");
}

#[test]
fn json_writes_nothing_into_the_queue_or_the_temp_directory() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    let queue_before = walk(location.dir());
    let temp = sandbox.temp_dir();

    let output = replay(&sandbox, &["replay", &reference, "--json"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(walk(location.dir()), queue_before);
    assert_eq!(walk(&temp), Vec::<String>::new());
}

#[test]
fn json_leaves_a_session_that_cannot_be_shown_a_failure_with_nothing_on_stdout() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    for entry in fs::read_dir(location.archive()).expect("listed") {
        fs::write(entry.expect("entry").path(), "this is no session").expect("overwritten");
    }

    let output = replay(&sandbox, &["replay", &reference, "--json"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("the archived session cannot be shown"),
        "{}",
        stderr(&output)
    );
    assert_eq!(common::stdout(&output), "");
}

// =========================================================
// replay --extract
// =========================================================

const IMAGE: &[u8] = b"pretend this is a WebP";

/// A session archived `days` ago that shows one kept image and one file
/// asqr has no record of, with the copy of the image in the archive; its
/// reference and the name of the copy.
fn archive_with_image(location: &QueueLocation, random: u128) -> (String, String) {
    location.create_layout().expect("layout");
    let millis = (SystemTime::now() - DAY)
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64;
    let ulid = Ulid::from_parts(millis, random);
    let name = format!("{}.webp", asqr::keep::sha256_hex(IMAGE));
    fs::create_dir_all(location.kept_files()).expect("created");
    fs::write(location.kept_files().join(&name), IMAGE).expect("written");
    let session = serde_json::json!({"asqr": 2, "id": "x", "title": "Pick", "questions": [
        {"id": "q1", "text": "?", "kind": "text", "context": [{"type": "image",
            "path": format!("files/{name}"),
            "shown": {"original_path": "/tmp/x/hero.png", "state": "kept",
                      "sha256": "ab".repeat(32), "size": 100, "kept_size": 22}}]},
        {"id": "q2", "text": "?", "kind": "text", "context": [
            {"type": "code", "path": "/tmp/x/old.rs"}]}
    ]});
    fs::write(
        location
            .archive()
            .join(archive_name("x", ulid, Archived::Session)),
        session.to_string(),
    )
    .expect("written");
    let text = ulid.to_string();
    (text[text.len() - 8..].to_owned(), name)
}

#[test]
fn extract_writes_the_session_and_the_kept_copy_and_says_what_it_left_out() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, name) = archive_with_image(&location, 0x1234_5678_9abc);
    let out = sandbox.home().join("out");

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            out.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        fs::read(out.join("files").join(&name)).expect("read"),
        IMAGE
    );
    assert!(out.join("session.json").is_file());
    assert!(!out.join("result.json").exists());
    let text = common::stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("extracted to ") && lines[0].ends_with("/out"),
        "{text}"
    );
    assert_eq!(
        lines[1..],
        [
            "session.json",
            "no result",
            "1 file in files/",
            "/tmp/x/old.rs: not_recorded"
        ]
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn extract_with_json_prints_only_the_object_naming_the_directory_and_the_copy() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, name) = archive_with_image(&location, 0x1234_5678_9abc);
    let out = sandbox.home().join("out");

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--json",
            "--extract",
            out.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let printed = printed_object(&output);
    let extracted_to = printed["extracted_to"].as_str().expect("a path");
    assert!(
        std::path::Path::new(extracted_to).is_absolute(),
        "{extracted_to}"
    );
    assert!(extracted_to.ends_with("/out"), "{extracted_to}");
    let first = &printed["files"][0];
    assert_eq!(first["original_path"], "/tmp/x/hero.png");
    let relative = first["extracted"].as_str().expect("a path");
    assert_eq!(relative, format!("files/{name}"));
    assert!(out.join(relative).is_file());
    assert_eq!(
        printed["session"]["questions"][0]["context"][0]["path"],
        "/tmp/x/hero.png"
    );
}

#[test]
fn extract_refuses_a_directory_with_something_in_it_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, _) = archive_with_image(&location, 0x1234_5678_9abc);
    let out = sandbox.home().join("out");
    fs::create_dir(&out).expect("made");
    fs::write(out.join(".keep"), "mine").expect("written");

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--json",
            "--extract",
            out.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("it is not empty"),
        "{}",
        stderr(&output)
    );
    assert_eq!(common::stdout(&output), "");
    assert_eq!(walk(&out), [out.join(".keep").display().to_string()]);
}

#[test]
fn extract_refuses_a_path_that_is_a_file() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, _) = archive_with_image(&location, 0x1234_5678_9abc);
    let out = sandbox.file("out", "a file");

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            out.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("it is not a directory"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read_to_string(&out).expect("read"), "a file");
}

#[cfg(unix)]
#[test]
fn extract_refuses_a_symbolic_link_to_an_empty_directory() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, _) = archive_with_image(&location, 0x1234_5678_9abc);
    let real = sandbox.home().join("real");
    fs::create_dir(&real).expect("made");
    let link = sandbox.home().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("linked");

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            link.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("it is a symbolic link"),
        "{}",
        stderr(&output)
    );
    assert_eq!(walk(&real), Vec::<String>::new());
}

#[test]
fn extract_refuses_the_empty_inbox_of_the_queue_and_leaves_it_empty() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, _) = archive_with_image(&location, 0x1234_5678_9abc);

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            location.inbox().to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("lies inside"),
        "{}",
        stderr(&output)
    );
    assert_eq!(walk(&location.inbox()), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn extract_refuses_a_symbolic_link_to_the_inbox_of_the_queue() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, _) = archive_with_image(&location, 0x1234_5678_9abc);
    let link = sandbox.home().join("link");
    std::os::unix::fs::symlink(location.inbox(), &link).expect("linked");

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            link.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(walk(&location.inbox()), Vec::<String>::new());
}

#[test]
fn extract_refuses_the_current_directory_even_when_it_is_empty() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, _) = archive_with_image(&location, 0x1234_5678_9abc);
    let here = sandbox.home().join("here");
    fs::create_dir(&here).expect("made");

    let output = sandbox
        .asqr()
        .current_dir(&here)
        .args(["replay", &reference, "--extract", "."])
        .output()
        .expect("runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("current directory"),
        "{}",
        stderr(&output)
    );
    assert!(here.is_dir());
    assert_eq!(walk(&here), Vec::<String>::new());
}

#[test]
fn extract_with_an_empty_directory_name_is_a_usage_error() {
    let sandbox = Sandbox::new();

    let output = replay(&sandbox, &["replay", "ZZZZZZ", "--extract", ""]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(common::stdout(&output), "");
}

#[test]
fn extract_of_a_session_that_does_not_parse_leaves_an_absent_directory_absent() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    for entry in fs::read_dir(location.archive()).expect("listed") {
        fs::write(entry.expect("entry").path(), "this is no session").expect("overwritten");
    }
    let out = sandbox.home().join("out");

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            out.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(!out.exists());
    assert_eq!(common::stdout(&output), "");
}

#[test]
fn extract_of_a_session_that_parses_but_is_not_valid_leaves_the_directories_as_they_were() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    let invalid = r#"{"asqr": 2, "id": "x", "title": "A", "questions": [
        {"id": "q", "text": "?", "kind": "text"}, {"id": "q", "text": "?", "kind": "text"}]}"#;
    for entry in fs::read_dir(location.archive()).expect("listed") {
        fs::write(entry.expect("entry").path(), invalid).expect("overwritten");
    }
    let absent = sandbox.home().join("absent");
    let empty = sandbox.home().join("empty");
    fs::create_dir(&empty).expect("made");

    let into_absent = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            absent.to_str().expect("UTF-8"),
        ],
    );
    let into_empty = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            empty.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(
        into_absent.status.code(),
        Some(1),
        "{}",
        stderr(&into_absent)
    );
    assert!(
        stderr(&into_absent).contains("the archived session cannot be shown"),
        "{}",
        stderr(&into_absent)
    );
    assert_eq!(into_empty.status.code(), Some(1));
    assert!(!absent.exists());
    assert_eq!(walk(&empty), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn extract_into_a_place_that_cannot_be_written_fails_with_nothing_on_stdout() {
    use std::os::unix::fs::PermissionsExt;
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, _) = archive_with_image(&location, 0x1234_5678_9abc);
    let sealed = sandbox.home().join("sealed");
    fs::create_dir(&sealed).expect("made");
    fs::set_permissions(&sealed, fs::Permissions::from_mode(0o500)).expect("sealed");
    // A process that may write there anyway, such as root, cannot fail.
    if fs::File::create(sealed.join("probe")).is_ok() {
        return;
    }

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--json",
            "--extract",
            sealed.join("out").to_str().expect("UTF-8"),
        ],
    );

    fs::set_permissions(&sealed, fs::Permissions::from_mode(0o700)).expect("unsealed");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with("error: cannot write "),
        "{}",
        stderr(&output)
    );
    assert_eq!(common::stdout(&output), "");
    assert_eq!(walk(&sealed), Vec::<String>::new());
}

#[test]
fn extract_writes_nothing_into_the_queue_or_the_temp_directory() {
    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let (reference, _) = archive_with_image(&location, 0x1234_5678_9abc);
    let queue_before = walk(location.dir());
    let out = sandbox.home().join("out");

    let output = replay(
        &sandbox,
        &[
            "replay",
            &reference,
            "--extract",
            out.to_str().expect("UTF-8"),
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(walk(location.dir()), queue_before);
    assert_eq!(walk(&sandbox.temp_dir()), Vec::<String>::new());
}

#[test]
fn json_alone_is_not_a_reference() {
    let sandbox = Sandbox::new();

    let output = replay(&sandbox, &["replay", "--json"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(common::stdout(&output), "");
}

/// A replay starts by removing the directories of earlier runs from the
/// temp directory. The sandbox must be the one it sweeps, never the
/// machine's.
#[cfg(unix)]
#[test]
fn a_replay_in_the_sandbox_sweeps_the_sandbox_temp_directory() {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};

    let sandbox = Sandbox::new();
    let location = QueueLocation::named(&sandbox.data_dir(), "default");
    let reference = archive(&location, "a", "A", 1, 0x1234_5678_9abc);
    let uid = fs::metadata(sandbox.home()).expect("metadata").uid();
    let base = sandbox.home().join("tmp").join(format!("asqr-{uid}"));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&base)
        .expect("made");
    let ended = base.join("run-ended");
    fs::create_dir(&ended).expect("made");
    fs::File::open(&ended)
        .expect("opens")
        .set_modified(SystemTime::now() - 2 * DAY)
        .expect("aged");

    replay(&sandbox, &["replay", &reference]);

    assert!(!ended.exists(), "the sandbox's temp directory was swept");
}
