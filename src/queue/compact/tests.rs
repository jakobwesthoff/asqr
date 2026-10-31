// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs;

use ulid::Ulid;

use super::*;
use crate::format::SessionResult;
use crate::queue::{
    Body, Slot, add_entries, archive_name, bundle_path, list_entries, lock_queue, read_entry,
};

fn at(text: &str) -> SystemTime {
    SystemTime::from(text.parse::<jiff::Timestamp>().expect("a timestamp"))
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64
}

/// The moment compaction runs: everything before 2026-11-01 is due.
fn now() -> SystemTime {
    at("2026-12-01T00:00:00Z")
}

fn queue() -> (tempfile::TempDir, QueueLocation, QueueLock) {
    let scratch = tempfile::tempdir().expect("temp dir");
    let location = QueueLocation::at(scratch.path().join("queue"));
    location.create_layout().expect("layout");
    let lock = lock_queue(&location, None).expect("locked");
    (scratch, location, lock)
}

fn month(year: i32, month: u8) -> Month {
    Month::new(year, month).expect("a month")
}

/// A session archived loose at `when`; its path.
fn loose(location: &QueueLocation, id: &str, when: &str, kind: Archived) -> PathBuf {
    let name = archive_name(id, Ulid::from_parts(millis(at(when)), 3), kind);
    let path = location.archive().join(name);
    fs::write(&path, format!("content of {id} at {when}")).expect("written");
    path
}

/// A result in the outbox, finished at `when` by its `submitted_at`.
fn outbox_result(location: &QueueLocation, id: &str, when: &str) -> (PathBuf, Vec<u8>) {
    let result = SessionResult::submitted(id, &"0".repeat(64), when, None, Vec::new());
    let bytes = serde_json::to_vec_pretty(&result).expect("serialises");
    let path = location.outbox().join(format!("{id}.json"));
    fs::write(&path, &bytes).expect("written");
    (path, bytes)
}

fn entry_names(location: &QueueLocation, month: Month) -> Vec<String> {
    let mut names: Vec<_> = list_entries(location, month)
        .expect("listed")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    names
}

fn compact_now(location: &QueueLocation, lock: &QueueLock) -> CompactReport {
    compact(location, lock, now())
}

// -------------------------------------------------------------------------
// What is due
// -------------------------------------------------------------------------

#[test]
fn old_loose_entries_are_bundled_by_the_month_of_their_ulid_and_removed() {
    let (_scratch, location, lock) = queue();
    let first = loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    let second = loose(&location, "b", "2026-09-20T10:00:00Z", Archived::Session);

    let report = compact_now(&location, &lock);

    assert_eq!(
        report.bundled,
        [MonthReport {
            month: month(2026, 9),
            sessions: 2,
            results: 0,
            replaced: 0
        }]
    );
    assert!(
        !first.exists() && !second.exists(),
        "the loose files are gone"
    );
    assert_eq!(entry_names(&location, month(2026, 9)).len(), 2);
    assert_eq!(
        read_entry(
            &location,
            month(2026, 9),
            &format!(
                "sessions/a.{}.json",
                Ulid::from_parts(millis(at("2026-09-15T10:00:00Z")), 3)
            )
        )
        .expect("read"),
        b"content of a at 2026-09-15T10:00:00Z"
    );
}

#[test]
fn two_months_make_two_bundles() {
    let (_scratch, location, lock) = queue();
    loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    loose(&location, "b", "2026-10-15T10:00:00Z", Archived::Session);

    let report = compact_now(&location, &lock);

    assert_eq!(
        report
            .bundled
            .iter()
            .map(|done| done.month)
            .collect::<Vec<_>>(),
        [month(2026, 9), month(2026, 10)]
    );
    assert!(bundle_path(&location, month(2026, 9)).is_file());
    assert!(bundle_path(&location, month(2026, 10)).is_file());
}

#[test]
fn an_entry_just_older_than_30_days_goes() {
    let (_scratch, location, lock) = queue();
    let path = loose(&location, "a", "2026-10-31T23:59:59Z", Archived::Session);

    compact_now(&location, &lock);

    assert!(!path.exists());
}

#[test]
fn an_entry_just_younger_than_30_days_stays() {
    let (_scratch, location, lock) = queue();
    let path = loose(&location, "a", "2026-11-01T00:00:01Z", Archived::Session);

    let report = compact_now(&location, &lock);

    assert!(path.exists());
    assert_eq!(report, CompactReport::default());
}

#[test]
fn invalid_entries_go_as_sessions_and_results_moved_aside_as_replaced() {
    let (_scratch, location, lock) = queue();
    loose(
        &location,
        "invalid",
        "2026-09-15T10:00:00Z",
        Archived::Session,
    );
    loose(&location, "a", "2026-09-16T10:00:00Z", Archived::Result);

    let report = compact_now(&location, &lock);

    assert_eq!(
        (report.bundled[0].sessions, report.bundled[0].replaced),
        (1, 1)
    );
    let names = entry_names(&location, month(2026, 9));
    assert!(names[0].starts_with("replaced/a."), "{names:?}");
    assert!(names[1].starts_with("sessions/invalid."), "{names:?}");
}

#[test]
fn files_that_are_no_entries_stay() {
    let (_scratch, location, lock) = queue();
    let stray = location.archive().join("not-an-entry.txt");
    fs::write(&stray, "").expect("written");

    let report = compact_now(&location, &lock);

    assert!(stray.exists());
    assert_eq!(report, CompactReport::default());
}

// -------------------------------------------------------------------------
// Results in the outbox
// -------------------------------------------------------------------------

#[test]
fn an_old_outbox_result_moves_into_the_month_it_was_finished_in() {
    let (_scratch, location, lock) = queue();
    let (path, bytes) = outbox_result(&location, "done", "2026-09-30T23:30:00+00:00");

    let report = compact_now(&location, &lock);

    assert_eq!(report.bundled[0].month, month(2026, 9));
    assert_eq!(report.bundled[0].results, 1);
    assert!(!path.exists());
    let name = result_entry_name("done", &bytes, at("2026-09-30T23:30:00Z")).zip_name();
    assert_eq!(
        read_entry(&location, month(2026, 9), &name).expect("read"),
        bytes
    );
}

#[test]
fn a_result_without_a_finish_time_goes_by_its_modification_time() {
    let (_scratch, location, lock) = queue();
    let path = location.outbox().join("done.json");
    fs::write(&path, "{}").expect("written");
    fs::File::options()
        .write(true)
        .open(&path)
        .expect("opens")
        .set_modified(at("2026-08-15T12:00:00Z"))
        .expect("mtime set");

    let report = compact_now(&location, &lock);

    assert_eq!(report.bundled[0].month, month(2026, 8));
    assert!(!path.exists());
}

#[test]
fn a_younger_result_stays_in_the_outbox() {
    let (_scratch, location, lock) = queue();
    let (path, _) = outbox_result(&location, "done", "2026-11-15T10:00:00+00:00");

    let report = compact_now(&location, &lock);

    assert!(path.exists());
    assert_eq!(report, CompactReport::default());
}

#[test]
fn a_result_beside_an_inbox_session_of_its_id_stays_for_recovery() {
    let (_scratch, location, lock) = queue();
    let (path, _) = outbox_result(&location, "done", "2026-09-15T10:00:00+00:00");
    fs::write(location.inbox().join("done.json"), "{}").expect("written");

    let report = compact_now(&location, &lock);

    assert!(path.exists());
    assert_eq!(report.for_recovery, ["done"]);
    assert!(report.bundled.is_empty());
}

// -------------------------------------------------------------------------
// Repeating and resuming
// -------------------------------------------------------------------------

#[test]
fn an_entry_that_comes_due_later_joins_the_months_bundle() {
    let (_scratch, location, lock) = queue();
    loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    compact(&location, &lock, at("2026-11-02T00:00:00Z"));
    loose(&location, "b", "2026-09-29T10:00:00Z", Archived::Session);

    compact(&location, &lock, at("2026-11-30T00:00:00Z"));

    assert_eq!(entry_names(&location, month(2026, 9)).len(), 2);
    assert_eq!(
        crate::queue::list_months(&location).expect("listed"),
        [month(2026, 9)]
    );
    let bundled: Vec<_> = list_entries(&location, month(2026, 9))
        .expect("listed")
        .into_iter()
        .map(|entry| entry.bundled_at)
        .collect();
    assert_eq!(
        bundled,
        [
            Some(millis(at("2026-11-02T00:00:00Z")) / 1000),
            Some(millis(at("2026-11-30T00:00:00Z")) / 1000)
        ],
        "the first entry keeps the time it was bundled at"
    );
}

#[test]
fn a_second_run_changes_nothing() {
    let (_scratch, location, lock) = queue();
    loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    compact_now(&location, &lock);
    let bundle = fs::read(bundle_path(&location, month(2026, 9))).expect("bundle");

    let report = compact_now(&location, &lock);

    assert_eq!(report, CompactReport::default());
    assert_eq!(
        fs::read(bundle_path(&location, month(2026, 9))).expect("bundle"),
        bundle
    );
}

#[test]
fn a_crash_after_the_rename_is_completed_by_the_next_run() {
    let (_scratch, location, lock) = queue();
    let session = loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    let (result, bytes) = outbox_result(&location, "b", "2026-09-16T10:00:00+00:00");
    // The bundle is in place, the loose files were not removed yet.
    add_entries(
        &location,
        month(2026, 9),
        &[
            NewEntry::named(
                EntryName {
                    kind: EntryKind::Session,
                    id: "a".to_owned(),
                    ulid: Ulid::from_parts(millis(at("2026-09-15T10:00:00Z")), 3),
                },
                fs::read(&session).expect("read"),
            ),
            NewEntry::named(
                result_entry_name("b", &bytes, at("2026-09-16T10:00:00Z")),
                bytes,
            ),
        ],
        at("2026-11-02T00:00:00Z"),
    )
    .expect("bundled");

    let report = compact_now(&location, &lock);

    assert!(!session.exists() && !result.exists());
    assert_eq!(
        entry_names(&location, month(2026, 9)).len(),
        2,
        "no duplicate"
    );
    assert_eq!(report.bundled[0].sessions + report.bundled[0].results, 2);
}

// -------------------------------------------------------------------------
// What is never deleted
// -------------------------------------------------------------------------

#[test]
fn an_entry_whose_name_holds_other_content_keeps_its_loose_file() {
    let (_scratch, location, lock) = queue();
    let path = loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    let other = loose(&location, "b", "2026-09-16T10:00:00Z", Archived::Session);
    add_entries(
        &location,
        month(2026, 9),
        &[NewEntry::named(
            EntryName {
                kind: EntryKind::Session,
                id: "a".to_owned(),
                ulid: Ulid::from_parts(millis(at("2026-09-15T10:00:00Z")), 3),
            },
            b"another content".to_vec(),
        )],
        at("2026-11-02T00:00:00Z"),
    )
    .expect("bundled");

    let report = compact_now(&location, &lock);

    assert!(path.exists(), "kept");
    assert!(!other.exists(), "the others go on");
    assert_eq!(report.problems.len(), 1);
    assert!(
        report.problems[0].text.contains("stays"),
        "{:?}",
        report.problems
    );
}

#[test]
fn a_bundle_that_cannot_be_read_is_skipped_and_the_other_months_go_on() {
    let (_scratch, location, lock) = queue();
    let broken = loose(&location, "a", "2026-08-15T10:00:00Z", Archived::Session);
    fs::write(bundle_path(&location, month(2026, 8)), "garbage").expect("written");
    let fine = loose(&location, "b", "2026-09-15T10:00:00Z", Archived::Session);

    let report = compact_now(&location, &lock);

    assert!(broken.exists());
    assert_eq!(
        fs::read(bundle_path(&location, month(2026, 8))).expect("bundle"),
        b"garbage"
    );
    assert!(!fine.exists());
    assert_eq!(report.problems[0].month, Some(month(2026, 8)));
    assert_eq!(report.bundled[0].month, month(2026, 9));
}

#[test]
fn a_busy_drafts_lock_skips_the_month() {
    let (_scratch, location, lock) = queue();
    let path = loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    let _held = lock_drafts(&location, 1).expect("locked");

    let report = compact_with(&location, &lock, now(), 1, &mut |_| {});

    assert!(path.exists());
    assert!(!bundle_path(&location, month(2026, 9)).exists());
    assert!(
        report.problems[0].text.contains("busy"),
        "{:?}",
        report.problems
    );
}

#[test]
fn a_loose_entry_pruned_during_compaction_is_not_bundled() {
    let (_scratch, location, lock) = queue();
    let path = loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);

    let report = compact_with(&location, &lock, now(), 1, &mut |_| {
        fs::remove_file(&path).expect("pruned");
    });

    assert!(!bundle_path(&location, month(2026, 9)).exists());
    assert!(
        report.problems[0].text.contains("changed"),
        "{:?}",
        report.problems
    );
}

#[test]
fn a_bundle_pruned_during_compaction_does_not_come_back() {
    let (_scratch, location, lock) = queue();
    loose(&location, "old", "2026-09-10T10:00:00Z", Archived::Session);
    compact(&location, &lock, at("2026-11-02T00:00:00Z"));
    let path = loose(&location, "new", "2026-09-20T10:00:00Z", Archived::Session);

    let report = compact_with(&location, &lock, now(), 1, &mut |month| {
        fs::remove_file(bundle_path(&location, month)).expect("pruned");
    });

    assert!(!bundle_path(&location, month(2026, 9)).exists());
    assert!(path.exists(), "left for the next run");
    assert!(
        report.problems[0].text.contains("changed"),
        "{:?}",
        report.problems
    );
}

#[test]
fn an_outbox_result_changed_during_compaction_stays() {
    let (_scratch, location, lock) = queue();
    let (path, _) = outbox_result(&location, "done", "2026-09-15T10:00:00+00:00");

    let report = compact_with(&location, &lock, now(), 1, &mut |_| {
        fs::write(&path, "changed").expect("rewritten");
    });

    assert_eq!(fs::read(&path).expect("kept"), b"changed");
    assert!(
        report.problems[0].text.contains("changed"),
        "{:?}",
        report.problems
    );
}

#[test]
fn an_inbox_session_that_arrives_during_compaction_keeps_the_result_for_recovery() {
    let (_scratch, location, lock) = queue();
    let (path, _) = outbox_result(&location, "done", "2026-09-15T10:00:00+00:00");
    fs::write(location.inbox().join("notes.txt"), "").expect("written");

    let report = compact_with(&location, &lock, now(), 1, &mut |_| {
        fs::write(location.inbox().join("done.json"), "{}").expect("written");
    });

    assert!(path.exists());
    assert!(report.bundled.is_empty());
}

#[test]
fn every_bundled_file_reads_back_identically() {
    let (_scratch, location, lock) = queue();
    let session = loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    let session_bytes = fs::read(&session).expect("read");
    let (_, result_bytes) = outbox_result(&location, "b", "2026-09-16T10:00:00+00:00");

    compact_now(&location, &lock);

    let listed = list_entries(&location, month(2026, 9)).expect("listed");
    let read = |prefix: &str| {
        let entry = listed
            .iter()
            .find(|entry| entry.name.starts_with(prefix))
            .expect("the entry");
        read_entry(&location, month(2026, 9), &entry.name).expect("read")
    };
    assert_eq!(read("sessions/a."), session_bytes);
    assert_eq!(read("results/b."), result_bytes);
}

// -------------------------------------------------------------------------
// Temp files
// -------------------------------------------------------------------------

#[test]
fn temp_files_older_than_24_hours_go_and_fresh_ones_stay() {
    let (_scratch, location, lock) = queue();
    let stale = location.archive().join(".asqr-stale.tmp");
    let fresh = location.archive().join(".asqr-fresh.tmp");
    let other = location.archive().join(".keep-me.tmp");
    for (path, modified) in [
        (&stale, "2026-11-29T23:00:00Z"),
        (&fresh, "2026-11-30T01:00:00Z"),
        (&other, "2026-01-01T00:00:00Z"),
    ] {
        fs::write(path, "").expect("written");
        fs::File::options()
            .write(true)
            .open(path)
            .expect("opens")
            .set_modified(at(modified))
            .expect("mtime set");
    }

    compact_now(&location, &lock);

    assert!(!stale.exists());
    assert!(fresh.exists());
    assert!(other.exists(), "only temp files of asqr's own name go");
}

#[test]
fn an_entry_that_only_conflicts_bundles_nothing() {
    let (_scratch, location, lock) = queue();
    let path = loose(&location, "a", "2026-09-15T10:00:00Z", Archived::Session);
    add_entries(
        &location,
        month(2026, 9),
        &[NewEntry::named(
            EntryName {
                kind: EntryKind::Session,
                id: "a".to_owned(),
                ulid: Ulid::from_parts(millis(at("2026-09-15T10:00:00Z")), 3),
            },
            b"another content".to_vec(),
        )],
        at("2026-11-02T00:00:00Z"),
    )
    .expect("bundled");

    let report = compact_now(&location, &lock);

    assert!(path.exists());
    assert!(report.bundled.is_empty());
    assert_eq!(report.problems.len(), 1);
}

#[test]
fn files_in_the_outbox_that_are_no_results_stay() {
    let (_scratch, location, lock) = queue();
    let stray = location.outbox().join("notes.txt");
    fs::write(&stray, "").expect("written");

    let report = compact_now(&location, &lock);

    assert!(stray.exists());
    assert_eq!(report, CompactReport::default());
}

#[cfg(unix)]
#[test]
fn a_queue_whose_archive_cannot_be_listed_is_a_problem_and_nothing_else() {
    use std::os::unix::fs::PermissionsExt;

    let (_scratch, location, lock) = queue();
    fs::set_permissions(location.archive(), fs::Permissions::from_mode(0o000)).expect("chmod");

    let report = compact_now(&location, &lock);

    fs::set_permissions(location.archive(), fs::Permissions::from_mode(0o755))
        .expect("chmod back, so the temp dir can go");
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.problems[0].month, None);
    assert!(report.bundled.is_empty());
}

#[test]
fn the_start_of_a_watch_leaves_what_it_cannot_bundle_and_goes_on() {
    let (_scratch, location, lock) = queue();
    let path = loose(&location, "a", "2020-09-15T10:00:00Z", Archived::Session);
    fs::write(bundle_path(&location, month(2020, 9)), "garbage").expect("written");

    compact_quietly(&location, &lock);

    assert!(path.exists());
}

#[cfg(unix)]
#[test]
fn an_outbox_file_that_vanished_since_the_listing_is_skipped() {
    let (_scratch, location, lock) = queue();
    // A link to nothing is listed and cannot be read, as a file that an
    // asker moved aside meanwhile.
    std::os::unix::fs::symlink("nowhere", location.outbox().join("gone.json")).expect("linked");

    let report = compact_now(&location, &lock);

    assert_eq!(report, CompactReport::default());
}

#[cfg(unix)]
#[test]
fn an_outbox_whose_files_cannot_be_examined_is_a_problem() {
    use std::os::unix::fs::PermissionsExt;

    let (_scratch, location, lock) = queue();
    outbox_result(&location, "done", "2026-09-15T10:00:00+00:00");
    fs::set_permissions(location.outbox(), fs::Permissions::from_mode(0o444)).expect("chmod");

    let report = compact_now(&location, &lock);

    fs::set_permissions(location.outbox(), fs::Permissions::from_mode(0o755))
        .expect("chmod back, so the temp dir can go");
    assert_eq!(report.problems.len(), 1);
    assert!(report.bundled.is_empty());
}

// -------------------------------------------------------------------------
// Kept files (spec section 5.1)
// -------------------------------------------------------------------------

/// Writes `bytes` as the loose kept file they hash to and returns its name.
fn kept(location: &QueueLocation, bytes: &[u8]) -> String {
    let name = format!("{}.webp", crate::keep::sha256_hex(bytes));
    let dir = location.archive().join("files");
    fs::create_dir_all(&dir).expect("files directory");
    fs::write(dir.join(&name), bytes).expect("written");
    name
}

fn kept_path(location: &QueueLocation, name: &str) -> PathBuf {
    location.archive().join("files").join(name)
}

/// An archived session that shows the kept files `names`, one image each.
fn showing(names: &[&str]) -> String {
    let context: Vec<_> = names
        .iter()
        .map(|name| serde_json::json!({"type": "image", "path": format!("files/{name}")}))
        .collect();
    serde_json::json!({
        "asqr": 2,
        "id": "a",
        "questions": [{"id": "q", "context": context}],
    })
    .to_string()
}

/// A session archived loose at `when` with `content`.
fn loose_showing(location: &QueueLocation, id: &str, when: &str, names: &[&str]) -> PathBuf {
    let name = archive_name(id, Ulid::from_parts(millis(at(when)), 3), Archived::Session);
    let path = location.archive().join(name);
    fs::write(&path, showing(names)).expect("written");
    path
}

fn has_entry(location: &QueueLocation, month: Month, name: &str) -> bool {
    entry_names(location, month).contains(&format!("files/{name}"))
}

#[test]
fn compaction_moves_the_kept_files_with_their_sessions() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    let session = loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);

    let report = compact_now(&location, &lock);

    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(
        read_entry(&location, month(2026, 9), &format!("files/{name}")).expect("read"),
        b"the picture"
    );
    assert!(!session.exists(), "the loose session is gone");
    assert!(
        !kept_path(&location, &name).exists(),
        "the loose copy is gone"
    );
}

#[test]
fn two_sessions_of_one_month_showing_one_file_bundle_it_once() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);
    loose_showing(&location, "b", "2026-09-16T10:00:00Z", &[&name]);

    let report = compact_now(&location, &lock);

    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.bundled[0].sessions, 2);
    let files: Vec<_> = entry_names(&location, month(2026, 9))
        .into_iter()
        .filter(|entry| entry.starts_with("files/"))
        .collect();
    assert_eq!(files, [format!("files/{name}")]);
}

#[test]
fn a_file_shown_by_two_months_lands_in_both_bundles() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);
    loose_showing(&location, "b", "2026-10-15T10:00:00Z", &[&name]);

    let report = compact_now(&location, &lock);

    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(has_entry(&location, month(2026, 9), &name));
    assert!(has_entry(&location, month(2026, 10), &name));
    assert!(
        !kept_path(&location, &name).exists(),
        "the loose copy goes once no loose session shows it"
    );
}

#[test]
fn a_file_a_loose_session_still_shows_stays_loose() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    loose_showing(&location, "old", "2026-09-15T10:00:00Z", &[&name]);
    let recent = loose_showing(&location, "new", "2026-11-20T10:00:00Z", &[&name]);

    compact_now(&location, &lock);

    assert!(has_entry(&location, month(2026, 9), &name));
    assert!(recent.exists());
    assert!(kept_path(&location, &name).exists());
}

#[test]
fn a_session_whose_kept_file_is_gone_is_bundled_with_a_problem() {
    let (_scratch, location, lock) = queue();
    let name = format!("{}.webp", crate::keep::sha256_hex(b"never written"));
    let session = loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);

    let report = compact_now(&location, &lock);

    assert!(!session.exists(), "the session is bundled");
    assert!(!has_entry(&location, month(2026, 9), &name));
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(
        report.problems[0].text.contains(&name),
        "{:?}",
        report.problems
    );
}

#[test]
fn a_conflicting_file_keeps_its_session_loose_and_out_of_the_bundle() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    // A bundle that holds other bytes under the file's name.
    add_entries(
        &location,
        month(2026, 9),
        &[NewEntry {
            slot: Slot::File(name.clone()),
            body: Body::Bytes(b"something else".to_vec()),
        }],
        at("2026-11-02T00:00:00Z"),
    )
    .expect("bundled");
    let conflicted = loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);
    let other = loose(&location, "b", "2026-09-16T10:00:00Z", Archived::Session);

    let report = compact_now(&location, &lock);

    assert!(conflicted.exists(), "the session stays loose");
    assert!(!other.exists(), "the others go on");
    let sessions: Vec<_> = entry_names(&location, month(2026, 9))
        .into_iter()
        .filter(|entry| entry.starts_with("sessions/"))
        .collect();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert!(sessions[0].starts_with("sessions/b."), "{sessions:?}");
    assert!(kept_path(&location, &name).exists());
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
}

#[test]
fn a_damaged_loose_kept_file_is_not_bundled() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    fs::write(kept_path(&location, &name), b"rotten").expect("damaged");
    let session = loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);

    let report = compact_now(&location, &lock);

    assert!(!session.exists(), "the session is bundled with the problem");
    assert!(!has_entry(&location, month(2026, 9), &name));
    assert_eq!(
        fs::read(kept_path(&location, &name)).expect("stays"),
        b"rotten"
    );
    assert!(
        report.problems[0].text.contains(&name),
        "{:?}",
        report.problems
    );
}

#[test]
fn a_kept_file_of_a_session_an_older_asqr_bundled_is_adopted_not_deleted() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    // An asqr that knew no kept files bundled the session alone.
    add_entries(
        &location,
        month(2026, 9),
        &[NewEntry::named(
            EntryName {
                kind: EntryKind::Session,
                id: "a".to_owned(),
                ulid: Ulid::from_parts(millis(at("2026-09-14T10:00:00Z")), 3),
            },
            showing(&[&name]).into_bytes(),
        )],
        at("2026-11-02T00:00:00Z"),
    )
    .expect("bundled");
    loose(&location, "b", "2026-09-16T10:00:00Z", Archived::Session);

    compact_now(&location, &lock);

    assert!(has_entry(&location, month(2026, 9), &name));
    assert!(
        kept_path(&location, &name).exists(),
        "a stray is never removed by compaction"
    );
}

#[test]
fn an_interrupted_compaction_finds_the_file_again() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    let session = loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);
    // The bundle is in place, the loose files were not removed yet.
    add_entries(
        &location,
        month(2026, 9),
        &[
            NewEntry::named(
                EntryName {
                    kind: EntryKind::Session,
                    id: "a".to_owned(),
                    ulid: Ulid::from_parts(millis(at("2026-09-15T10:00:00Z")), 3),
                },
                fs::read(&session).expect("read"),
            ),
            NewEntry::kept_file(&name, kept_path(&location, &name)).expect("a kept name"),
        ],
        at("2026-11-02T00:00:00Z"),
    )
    .expect("bundled");

    let report = compact_now(&location, &lock);

    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(!session.exists());
    assert!(!kept_path(&location, &name).exists());
    assert_eq!(entry_names(&location, month(2026, 9)).len(), 2);
}

#[test]
fn stale_temp_files_in_the_files_directory_are_swept() {
    let (_scratch, location, lock) = queue();
    let dir = location.archive().join("files");
    fs::create_dir_all(&dir).expect("files directory");
    let stale = dir.join(".asqr-stale.tmp");
    let fresh = dir.join(".asqr-fresh.tmp");
    for (path, modified) in [
        (&stale, "2026-11-29T23:00:00Z"),
        (&fresh, "2026-11-30T01:00:00Z"),
    ] {
        fs::write(path, "").expect("written");
        fs::File::options()
            .write(true)
            .open(path)
            .expect("opens")
            .set_modified(at(modified))
            .expect("mtime set");
    }

    compact_now(&location, &lock);

    assert!(!stale.exists());
    assert!(fresh.exists());
}

/// Pins that compaction leaves a kept file alone that no session shows.
#[test]
fn compaction_removes_no_orphan() {
    let (_scratch, location, lock) = queue();
    let shown = kept(&location, b"the picture");
    let orphan = kept(&location, b"nobody shows this");
    loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&shown]);

    compact_now(&location, &lock);

    assert!(kept_path(&location, &orphan).exists());
    assert!(!has_entry(&location, month(2026, 9), &orphan));
}

#[test]
fn a_month_whose_only_session_is_held_back_writes_no_bundle_of_its_own() {
    let (_scratch, location, lock) = queue();
    let name = kept(&location, b"the picture");
    add_entries(
        &location,
        month(2026, 9),
        &[NewEntry {
            slot: Slot::File(name.clone()),
            body: Body::Bytes(b"something else".to_vec()),
        }],
        at("2026-11-02T00:00:00Z"),
    )
    .expect("bundled");
    let before = fs::read(bundle_path(&location, month(2026, 9))).expect("bundle");
    let held = loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);

    let report = compact_now(&location, &lock);

    assert!(report.bundled.is_empty());
    assert!(held.exists());
    assert_eq!(
        fs::read(bundle_path(&location, month(2026, 9))).expect("bundle"),
        before
    );
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn a_loose_kept_file_that_cannot_be_opened_is_named_and_not_bundled() {
        let (_scratch, location, lock) = queue();
        let name = kept(&location, b"the picture");
        let path = kept_path(&location, &name);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("chmod");
        let session = loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);

        let report = compact_now(&location, &lock);

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod back");
        assert!(!session.exists(), "the session is bundled");
        assert!(!has_entry(&location, month(2026, 9), &name));
        assert!(path.exists());
        assert!(
            report.problems[0].text.contains("stays and is not bundled"),
            "{:?}",
            report.problems
        );
    }

    #[test]
    fn a_kept_name_that_is_a_directory_is_named_and_not_bundled() {
        let (_scratch, location, lock) = queue();
        let name = format!("{}.webp", crate::keep::sha256_hex(b"a directory"));
        fs::create_dir_all(kept_path(&location, &name)).expect("a directory by that name");
        let session = loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);

        let report = compact_now(&location, &lock);

        assert!(!session.exists());
        assert!(
            report.problems[0].text.contains("stays and is not bundled"),
            "{:?}",
            report.problems
        );
    }

    #[test]
    fn an_unreadable_loose_session_keeps_every_loose_kept_file() {
        let (_scratch, location, lock) = queue();
        let name = kept(&location, b"the picture");
        loose_showing(&location, "old", "2026-09-15T10:00:00Z", &[&name]);
        let recent = loose_showing(&location, "new", "2026-11-20T10:00:00Z", &[]);
        fs::set_permissions(&recent, fs::Permissions::from_mode(0o000)).expect("chmod");

        let report = compact_now(&location, &lock);

        fs::set_permissions(&recent, fs::Permissions::from_mode(0o644)).expect("chmod back");
        assert!(has_entry(&location, month(2026, 9), &name));
        assert!(
            kept_path(&location, &name).exists(),
            "not knowing what loose sessions show, nothing is removed"
        );
        assert!(
            report.problems[0].text.contains("loose kept files stay"),
            "{:?}",
            report.problems
        );
    }

    #[test]
    fn a_loose_kept_file_that_cannot_be_removed_is_named() {
        let (_scratch, location, lock) = queue();
        let name = kept(&location, b"the picture");
        loose_showing(&location, "a", "2026-09-15T10:00:00Z", &[&name]);
        let dir = location.kept_files();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("chmod");

        let report = compact_now(&location, &lock);

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("chmod back");
        assert!(has_entry(&location, month(2026, 9), &name));
        assert!(
            report.problems[0].text.contains("cannot remove"),
            "{:?}",
            report.problems
        );
    }
}
