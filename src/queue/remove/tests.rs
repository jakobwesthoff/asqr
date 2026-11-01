// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::format::Status;
use crate::queue::{
    drafts_lock_is_held, drop_session, list_queues, lock_holder, lock_queue, save_draft,
};

/// A session with a question without a default, `q`, and one whose default
/// is `y`, `r`.
const SESSION: &str = r#"{"asqr": 2, "questions": [
    {"id": "q", "text": "?", "kind": "single",
     "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]},
    {"id": "r", "text": "?", "kind": "single",
     "options": [{"id": "x", "label": "X"}, {"id": "y", "label": "Y", "default": true}]}]}"#;

const AT: &str = "2026-11-01T12:00:00+01:00";

fn root() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

/// The named queue `name` under `root`, used: its directories and its
/// `active` marker exist.
fn used(root: &Path, name: &str) -> QueueLocation {
    let location = QueueLocation::named(root, name);
    location.create_layout().expect("layout");
    location
}

fn asked(location: &QueueLocation, id: &str) {
    drop_session(location, id, SESSION.as_bytes(), false).expect("dropped");
}

fn archived_file(location: &QueueLocation) -> PathBuf {
    let path = location
        .archive()
        .join("old.01M6BG39EPAPHYHPJX7F8YBCMZ.json");
    fs::write(&path, SESSION).expect("archived");
    path
}

fn outbox_result(location: &QueueLocation, id: &str) -> SessionResult {
    let bytes = fs::read(location.outbox().join(format!("{id}.json"))).expect("result");
    serde_json::from_slice(&bytes).expect("parses")
}

fn write_result(location: &QueueLocation, id: &str, result: &SessionResult) {
    fs::write(
        location.outbox().join(format!("{id}.json")),
        serde_json::to_vec_pretty(result).expect("serializes"),
    )
    .expect("written");
}

fn in_use(root: &Path, name: &str) -> bool {
    list_queues(root)
        .expect("lists")
        .iter()
        .find(|summary| summary.name() == name)
        .is_some_and(|summary| summary.in_use())
}

/// The reason `--force` gives a session of queue `name`.
fn reason(name: &str) -> String {
    format!("the person removed queue {name} while this session waited in it")
}

/// The sessions `removed` rejected; it must have removed the queue.
fn rejected(removed: Removed) -> Vec<Rejection> {
    match removed {
        Removed::Now { rejected } => rejected,
        Removed::Already => panic!("removed now, not already"),
    }
}

// =========================================================================
// Removing
// =========================================================================

#[test]
fn removing_takes_the_marker_and_keeps_every_file() {
    let root = root();
    let old = used(root.path(), "old");
    let archived = archived_file(&old);
    fs::write(old.outbox().join("done.json"), "{}").expect("result");
    fs::write(old.drafts().join("left.json"), "{}").expect("draft");

    let removed = remove_queue(&old, false).expect("removes");

    assert_eq!(removed, Removed::Now { rejected: vec![] });
    assert!(!old.active_marker().exists());
    assert!(archived.is_file());
    assert!(old.outbox().join("done.json").is_file());
    assert!(old.drafts().join("left.json").is_file());
    assert!(!in_use(root.path(), "old"));
}

#[test]
fn refuses_a_queue_in_which_a_session_waits() {
    let root = root();
    let work = used(root.path(), "work");
    asked(&work, "first");

    let refused = remove_queue(&work, false);

    assert!(
        matches!(refused, Err(RemoveError::Waiting(1))),
        "{refused:?}"
    );
    assert!(work.active_marker().is_file());
    assert!(work.inbox().join("first.json").is_file());
}

#[test]
fn refuses_a_watched_queue_and_names_its_holder() {
    let root = root();
    let work = used(root.path(), "work");
    let _watching = lock_queue(&work, None).expect("an asqr watches it");

    let refused = remove_queue(&work, false);

    match refused {
        Err(RemoveError::Held(holder)) => assert_eq!(
            holder.text,
            format!(
                "pid {} on {}",
                std::process::id(),
                gethostname::gethostname().to_string_lossy()
            )
        ),
        other => panic!("refused as held, not {other:?}"),
    }
    assert!(work.active_marker().is_file());
}

#[test]
fn a_queue_a_command_holds_is_refused_with_the_commands_text() {
    let root = root();
    let work = used(root.path(), "work");
    let _compacting = lock_queue_for(&work, 1, "asqr compact").expect("compact holds it");

    let refused = remove_queue(&work, false);

    match refused {
        Err(RemoveError::Held(holder)) => {
            assert!(
                holder.text.ends_with(", running asqr compact"),
                "{holder:?}"
            )
        }
        other => panic!("refused as held, not {other:?}"),
    }
}

#[test]
fn refuses_while_the_drafts_lock_stays_taken() {
    let root = root();
    let work = used(root.path(), "work");
    let _busy = lock_drafts(&work, 1).expect("an ask holds it");

    let refused = remove_queue_with(
        &work,
        false,
        Gate {
            attempts: 1,
            ..Gate::default()
        },
    );

    assert!(matches!(refused, Err(RemoveError::Busy)), "{refused:?}");
    assert!(work.active_marker().is_file());
}

#[test]
fn refuses_a_symbolic_link() {
    let root = root();
    let target = tempfile::tempdir().expect("elsewhere");
    fs::create_dir_all(root.path().join("queues")).expect("queues");
    std::os::unix::fs::symlink(target.path(), root.path().join("queues/linked")).expect("linked");
    fs::write(target.path().join("active"), "").expect("marker");

    let refused = remove_queue(&QueueLocation::named(root.path(), "linked"), false);

    assert!(matches!(refused, Err(RemoveError::Link)), "{refused:?}");
    assert!(target.path().join("active").is_file());
}

#[test]
fn a_link_to_another_queue_is_refused_and_that_queue_keeps_active() {
    let root = root();
    let work = used(root.path(), "work");
    std::os::unix::fs::symlink(work.dir(), root.path().join("queues/alias")).expect("linked");

    let refused = remove_queue(&QueueLocation::named(root.path(), "alias"), false);

    assert!(matches!(refused, Err(RemoveError::Link)), "{refused:?}");
    assert!(work.active_marker().is_file());
}

#[test]
fn the_default_queue_before_its_first_use_is_refused_as_the_default() {
    let root = root();

    let refused = remove_queue(&QueueLocation::named(root.path(), DEFAULT_QUEUE), false);

    assert!(matches!(refused, Err(RemoveError::Default)), "{refused:?}");
}

#[test]
fn a_link_to_the_default_queue_is_refused_as_a_link() {
    let root = root();
    let default = used(root.path(), DEFAULT_QUEUE);
    std::os::unix::fs::symlink(default.dir(), root.path().join("queues/main")).expect("linked");

    let refused = remove_queue(&QueueLocation::named(root.path(), "main"), false);

    assert!(matches!(refused, Err(RemoveError::Link)), "{refused:?}");
}

#[test]
fn a_queue_that_is_not_there_is_an_error_and_nothing_is_made() {
    let root = root();
    let ghost = QueueLocation::named(root.path(), "ghost");

    let refused = remove_queue(&ghost, false);

    assert!(matches!(refused, Err(RemoveError::NotThere)), "{refused:?}");
    assert!(!ghost.dir().exists());
}

#[test]
fn a_dir_queue_is_no_queue_to_remove() {
    let dir = tempfile::tempdir().expect("a directory of the person's");
    let location = QueueLocation::at(dir.path());
    location.create_layout().expect("layout");

    let refused = remove_queue(&location, false);

    assert!(matches!(refused, Err(RemoveError::NotThere)), "{refused:?}");
}

#[test]
fn a_file_where_a_queue_would_be_is_no_queue() {
    let root = root();
    fs::create_dir_all(root.path().join("queues")).expect("queues");
    fs::write(root.path().join("queues/stray"), "").expect("a file");

    let refused = remove_queue(&QueueLocation::named(root.path(), "stray"), false);

    assert!(matches!(refused, Err(RemoveError::NotThere)), "{refused:?}");
}

/// Runs `remove` with the directory `dir` read-only, and makes it writable
/// again before the test looks at the outcome.
fn with_read_only<T>(dir: &Path, mode: u32, remove: impl FnOnce() -> T) -> T {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(mode)).expect("permissions change");
    let outcome = remove();
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).expect("permissions back");
    outcome
}

#[test]
fn a_root_that_cannot_be_searched_is_an_error() {
    let root = root();
    let work = used(root.path(), "work");
    let queues = root.path().join("queues");

    let failed = with_read_only(&queues, 0o000, || remove_queue(&work, false));

    assert!(
        matches!(&failed, Err(RemoveError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied),
        "{failed:?}"
    );
}

#[test]
fn a_queue_lock_that_cannot_be_made_is_an_error() {
    let root = root();
    let work = used(root.path(), "work");

    let failed = with_read_only(work.dir(), 0o500, || remove_queue(&work, false));

    assert!(
        matches!(&failed, Err(RemoveError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied),
        "{failed:?}"
    );
    assert!(work.active_marker().is_file());
}

#[test]
fn a_drafts_lock_that_cannot_be_made_is_an_error() {
    let root = root();
    let work = used(root.path(), "work");
    // The queue lock file exists, as after any asqr watched the queue.
    drop(lock_queue(&work, None).expect("locks"));

    let failed = with_read_only(work.dir(), 0o500, || remove_queue(&work, false));

    assert!(
        matches!(&failed, Err(RemoveError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied),
        "{failed:?}"
    );
    assert!(work.active_marker().is_file());
}

#[test]
fn a_marker_that_cannot_be_deleted_is_an_error() {
    use std::os::unix::fs::PermissionsExt;

    let root = root();
    let work = used(root.path(), "work");
    let dir = work.dir().to_path_buf();

    let failed = remove_queue_with(
        &work,
        false,
        Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::Checked {
                    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500))
                        .expect("read-only");
                }
            }),
        },
    );
    fs::set_permissions(work.dir(), fs::Permissions::from_mode(0o755)).expect("writable again");

    assert!(
        matches!(&failed, Err(RemoveError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied),
        "{failed:?}"
    );
    assert!(work.active_marker().is_file());
}

#[test]
fn refuses_the_default_queue() {
    let root = root();
    let default = used(root.path(), DEFAULT_QUEUE);

    let refused = remove_queue(&default, false);

    assert!(matches!(refused, Err(RemoveError::Default)), "{refused:?}");
    assert!(default.active_marker().is_file());
}

#[test]
fn refuses_the_default_queue_spelled_otherwise_where_the_filesystem_ignores_case() {
    let root = root();
    used(root.path(), DEFAULT_QUEUE);
    let upper = QueueLocation::named(root.path(), "Default");
    // On a filesystem that keeps case apart, `Default` is no queue.
    let ignores_case = upper.dir().exists();

    let refused = remove_queue(&upper, false);

    if ignores_case {
        assert!(matches!(refused, Err(RemoveError::Default)), "{refused:?}");
    } else {
        assert!(matches!(refused, Err(RemoveError::NotThere)), "{refused:?}");
    }
}

#[test]
fn removing_a_removed_queue_says_so() {
    let root = root();
    let old = used(root.path(), "old");
    remove_queue(&old, false).expect("removes");

    assert_eq!(remove_queue(&old, false).expect("again"), Removed::Already);
}

// =========================================================================
// The locks
// =========================================================================

#[test]
fn the_removal_holds_both_locks_from_the_look_to_the_deletion() {
    let root = root();
    let old = used(root.path(), "old");
    let seen = RefCell::new(Vec::new());

    remove_queue_with(
        &old,
        false,
        Gate {
            attempts: 1,
            step: Box::new(|step| {
                if step == Step::Checked {
                    seen.borrow_mut().push((
                        drafts_lock_is_held(&old),
                        lock_holder(&old)
                            .expect("readable")
                            .map(|holder| holder.text),
                        old.active_marker().is_file(),
                    ));
                }
            }),
        },
    )
    .expect("removes");

    let seen = seen.into_inner();
    assert_eq!(seen.len(), 1, "the look at the inbox ran under the gate");
    let (drafts_held, holder, marked) = &seen[0];
    assert!(drafts_held, "the drafts lock is held");
    assert!(
        holder
            .as_deref()
            .is_some_and(|text| text.ends_with(", running asqr queues remove")),
        "{holder:?}"
    );
    assert!(marked, "the marker goes after the look");
    assert!(!old.active_marker().exists());
}

#[test]
fn an_ask_after_the_removal_marks_the_queue_again() {
    let root = root();
    let old = used(root.path(), "old");
    remove_queue(&old, false).expect("removes");

    asked(&old, "later");

    assert!(old.active_marker().is_file());
    assert!(in_use(root.path(), "old"));
}

#[test]
fn an_asqr_starting_after_the_removal_marks_the_queue_again() {
    let root = root();
    let old = used(root.path(), "old");
    remove_queue(&old, false).expect("removes");

    let _watching = lock_queue(&old, None).expect("an asqr starts on it");
    old.create_layout().expect("layout");

    assert!(old.active_marker().is_file());
}

// =========================================================================
// --force
// =========================================================================

#[test]
fn force_rejects_a_waiting_session_with_the_answers_of_its_draft() {
    let root = root();
    let work = used(root.path(), "work");
    asked(&work, "first");
    let picked = Answer {
        selected: vec!["b".into()],
        note: Some("why b".into()),
        ..Answer::new("q")
    };
    let untouched = Answer {
        selected: vec!["y".into()],
        defaulted: true,
        ..Answer::new("r")
    };
    let mut draft = SessionResult::draft("first", "q", vec![picked.clone(), untouched.clone()]);
    draft.note = Some("  first thoughts  ".into());
    save_draft(&work, &draft).expect("saved");

    let rejected = rejected(remove_queue(&work, true).expect("removes"));

    assert_eq!(rejected.len(), 1);
    assert_eq!(
        (rejected[0].id.as_str(), rejected[0].how),
        ("first", Ended::Cancelled)
    );
    let archived = rejected[0].archived.as_ref().expect("archived");
    assert_eq!(archived.bytes, fs::read(&archived.path).expect("reads"));
    let result = outbox_result(&work, "first");
    assert_eq!(result.status, Status::Cancelled);
    assert_eq!(result.reason.as_deref(), Some(reason("work").as_str()));
    assert_eq!(result.note.as_deref(), Some("first thoughts"));
    assert_eq!(result.answers, vec![picked, untouched]);
    assert!(!work.inbox().join("first.json").exists());
    assert!(!work.drafts().join("first.json").exists());
    assert!(!work.active_marker().exists());
}

#[test]
fn force_without_a_draft_rejects_without_answers() {
    let root = root();
    let work = used(root.path(), "work");
    asked(&work, "first");

    remove_queue(&work, true).expect("removes");

    let result = outbox_result(&work, "first");
    assert_eq!(result.status, Status::Cancelled);
    assert_eq!((result.note, result.answers), (None, vec![]));
}

#[test]
fn force_rejects_an_invalid_session_without_answers() {
    let root = root();
    let work = used(root.path(), "work");
    fs::write(work.inbox().join("broken.json"), "{ not a session").expect("dropped by hand");
    let mut draft = SessionResult::draft("broken", "q", vec![]);
    draft.note = Some("a note".into());
    save_draft(&work, &draft).expect("saved");

    remove_queue(&work, true).expect("removes");

    let result = outbox_result(&work, "broken");
    assert_eq!(result.status, Status::Cancelled);
    assert_eq!(result.reason.as_deref(), Some(reason("work").as_str()));
    assert_eq!(
        result.session_sha256.as_deref(),
        Some(session_sha256(b"{ not a session").as_str())
    );
    assert_eq!((result.note, result.answers), (None, vec![]));
}

#[test]
fn force_rejects_a_session_that_does_not_validate_without_its_draft() {
    let root = root();
    let work = used(root.path(), "work");
    // Parses, and fails validation: a session needs a question.
    let bytes = r#"{"asqr": 2, "questions": []}"#;
    fs::write(work.inbox().join("empty.json"), bytes).expect("dropped by hand");
    let mut draft = SessionResult::draft("empty", "q", vec![Answer::new("q")]);
    draft.note = Some("a note".into());
    save_draft(&work, &draft).expect("saved");

    remove_queue(&work, true).expect("removes");

    let result = outbox_result(&work, "empty");
    assert_eq!(result.status, Status::Cancelled);
    assert_eq!((result.note, result.answers), (None, vec![]));
}

#[test]
fn a_session_that_leaves_the_inbox_meanwhile_is_left_to_whoever_took_it() {
    let root = root();
    let work = used(root.path(), "work");
    asked(&work, "first");
    let inbox = work.inbox();

    let removed = remove_queue_with(
        &work,
        true,
        Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::Checked {
                    // Taken by hand after the look at the inbox.
                    let _ = fs::remove_file(inbox.join("first.json"));
                }
            }),
        },
    );

    assert_eq!(rejected(removed.expect("removes")), vec![]);
    assert!(!work.outbox().join("first.json").exists());
    assert!(!work.active_marker().exists());
}

#[test]
fn a_result_that_cannot_be_written_stops_the_removal() {
    let root = root();
    let work = used(root.path(), "work");
    asked(&work, "first");

    let stopped = with_read_only(&work.outbox(), 0o500, || remove_queue(&work, true));

    match stopped {
        Err(RemoveError::Stopped { id, rejected, .. }) => {
            assert_eq!((id.as_str(), rejected.len()), ("first", 0));
        }
        other => panic!("stopped, not {other:?}"),
    }
    assert!(work.inbox().join("first.json").is_file());
    assert!(work.active_marker().is_file());
}

#[test]
fn force_completes_an_interrupted_finish_as_recovery_does() {
    let root = root();
    let work = used(root.path(), "work");
    asked(&work, "first");
    let answered = SessionResult::submitted(
        "first",
        &session_sha256(SESSION.as_bytes()),
        AT,
        None,
        vec![],
    );
    write_result(&work, "first", &answered);
    save_draft(&work, &SessionResult::draft("first", "q", vec![])).expect("saved");

    let rejected = rejected(remove_queue(&work, true).expect("removes"));

    assert_eq!(rejected[0].how, Ended::Completed);
    assert!(rejected[0].archived.is_some());
    assert_eq!(outbox_result(&work, "first").status, Status::Submitted);
    assert!(!work.drafts().join("first.json").exists());
}

#[test]
fn force_archives_a_session_next_to_an_unread_earlier_result() {
    let root = root();
    let work = used(root.path(), "work");
    let earlier = SessionResult::submitted("first", "other", AT, None, vec![]);
    write_result(&work, "first", &earlier);
    fs::write(work.inbox().join("first.json"), SESSION).expect("dropped by hand");
    save_draft(&work, &SessionResult::draft("first", "q", vec![])).expect("saved");

    let rejected = rejected(remove_queue(&work, true).expect("removes"));

    assert_eq!(rejected[0].how, Ended::ArchivedUnanswered);
    assert_eq!(outbox_result(&work, "first"), earlier);
    assert!(
        work.drafts().join("first.json").is_file(),
        "recovery keeps it"
    );
    assert!(!work.inbox().join("first.json").exists());
}

#[test]
fn a_result_written_after_the_look_archives_the_session_unanswered() {
    let root = root();
    let work = used(root.path(), "work");
    asked(&work, "first");
    save_draft(&work, &SessionResult::draft("first", "q", vec![])).expect("saved");
    let foreign = SessionResult::submitted("first", "other", AT, None, vec![]);
    let written = foreign.clone();
    let writer = work.clone();
    let mut checks = 0;

    let removed = remove_queue_with(
        &work,
        true,
        Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::Checked {
                    checks += 1;
                    // The second call comes after the look at the session's
                    // result; a writer that takes no lock writes then.
                    if checks == 2 {
                        write_result(&writer, "first", &written);
                    }
                }
            }),
        },
    );

    let rejected = rejected(removed.expect("removes"));
    assert_eq!(rejected[0].how, Ended::ArchivedUnanswered);
    assert_eq!(outbox_result(&work, "first"), foreign);
    assert!(work.drafts().join("first.json").is_file());
    assert!(!work.inbox().join("first.json").exists());
}

#[test]
fn force_stops_at_a_session_it_cannot_read_and_keeps_the_queue_in_use() {
    use std::os::unix::fs::PermissionsExt;

    let root = root();
    let work = used(root.path(), "work");
    asked(&work, "a-first");
    let locked = work.inbox().join("b-locked.json");
    fs::write(&locked, SESSION).expect("dropped by hand");
    // The inbox goes by drop time, then by id: the same time for both puts
    // `a-first` first.
    let time = fs::metadata(work.inbox().join("a-first.json"))
        .and_then(|metadata| metadata.modified())
        .expect("a time");
    fs::File::options()
        .write(true)
        .open(&locked)
        .and_then(|file| file.set_modified(time))
        .expect("time set");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("unreadable");

    let stopped = remove_queue(&work, true);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o600)).expect("readable again");

    match stopped {
        Err(RemoveError::Stopped { id, rejected, .. }) => {
            assert_eq!(id, "b-locked");
            assert_eq!(rejected.len(), 1);
            assert_eq!(rejected[0].id, "a-first");
            assert!(rejected[0].archived.is_some());
        }
        other => panic!("stopped, not {other:?}"),
    }
    assert_eq!(outbox_result(&work, "a-first").status, Status::Cancelled);
    assert!(work.active_marker().is_file());
    assert!(in_use(root.path(), "work"));
}
