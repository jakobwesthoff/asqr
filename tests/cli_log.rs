// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The log file in the platform cache directory (ADR 15).

mod common;

use common::Sandbox;

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
