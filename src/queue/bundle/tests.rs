// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs;
use std::io::{Read, Write};

use super::*;
use crate::queue::archive_name;

fn queue() -> (tempfile::TempDir, QueueLocation) {
    let scratch = tempfile::tempdir().expect("temp dir");
    let location = QueueLocation::at(scratch.path().join("queue"));
    location.create_layout().expect("layout");
    (scratch, location)
}

/// A point in time from an RFC 3339 text.
fn at(text: &str) -> SystemTime {
    SystemTime::from(text.parse::<jiff::Timestamp>().expect("a timestamp"))
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64
}

fn ulid_at(text: &str) -> Ulid {
    Ulid::from_parts(millis(at(text)), 7)
}

fn entry(kind: EntryKind, id: &str, when: &str, bytes: &str) -> NewEntry {
    NewEntry {
        name: EntryName {
            kind,
            id: id.to_owned(),
            ulid: ulid_at(when),
        },
        bytes: bytes.as_bytes().to_vec(),
    }
}

const OCTOBER: Month = Month {
    year: 2026,
    month: 10,
};

fn bundle_bytes(location: &QueueLocation, month: Month) -> Vec<u8> {
    fs::read(bundle_path(location, month)).expect("bundle exists")
}

fn temp_files(location: &QueueLocation) -> Vec<String> {
    fs::read_dir(location.archive())
        .expect("readable")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with(".asqr-"))
        .collect()
}

/// Writes a bundle by hand with entries stored uncompressed and without
/// the time they were bundled, the way another tool would.
fn hand_made(location: &QueueLocation, month: Month, entries: &[(&str, &str)]) {
    let file = fs::File::create(bundle_path(location, month)).expect("created");
    let mut writer = zip::ZipWriter::new(file);
    for (name, content) in entries {
        let options =
            zip::write::SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file(*name, options).expect("started");
        writer.write_all(content.as_bytes()).expect("written");
    }
    writer.finish().expect("finished");
}

/// A session archived loose at `when`.
fn loose_session(location: &QueueLocation, id: &str, when: &str, text: &str) {
    let name = archive_name(id, ulid_at(when), Archived::Session);
    fs::write(location.archive().join(name), text).expect("written");
}

// -------------------------------------------------------------------------
// Months and names
// -------------------------------------------------------------------------

#[test]
fn the_month_of_a_ulid_is_its_utc_month() {
    assert_eq!(Month::of(ulid_at("2026-10-31T23:59:59.999Z")), OCTOBER);
    assert_eq!(
        Month::of(ulid_at("2026-11-01T00:00:00Z")),
        Month::new(2026, 11).expect("a month")
    );
    assert_eq!(
        Month::of(ulid_at("2026-12-31T23:59:59Z")),
        Month::new(2026, 12).expect("a month")
    );
    assert_eq!(
        Month::of(ulid_at("2027-01-01T00:00:00Z")),
        Month::new(2027, 1).expect("a month")
    );
}

#[test]
fn a_month_ends_where_the_next_begins() {
    assert_eq!(OCTOBER.end(), at("2026-11-01T00:00:00Z"));
    assert_eq!(OCTOBER.start(), at("2026-10-01T00:00:00Z"));
    let december = Month::new(2026, 12).expect("a month");
    assert_eq!(december.end(), at("2027-01-01T00:00:00Z"));
}

#[test]
fn months_print_and_parse_as_year_and_month() {
    assert_eq!(OCTOBER.to_string(), "2026-10");
    assert_eq!("2026-10".parse(), Ok(OCTOBER));
    for text in ["2026-1", "2026-13", "2026-00", "26-10", "2026_10", "x", ""] {
        assert!(text.parse::<Month>().is_err(), "{text:?}");
    }
}

#[test]
fn entry_names_round_trip_and_ids_with_dots_parse() {
    for (kind, prefix) in [
        (EntryKind::Session, "sessions"),
        (EntryKind::Result, "results"),
        (EntryKind::Replaced, "replaced"),
    ] {
        let name = EntryName {
            kind,
            id: "with.dots-1".to_owned(),
            ulid: ulid_at("2026-10-15T12:00:00Z"),
        };
        let zip_name = name.zip_name();
        assert!(
            zip_name.starts_with(&format!("{prefix}/with.dots-1.")),
            "{zip_name}"
        );
        assert_eq!(EntryName::parse(&zip_name), Some(name));
    }
}

#[test]
fn names_that_are_no_entries_do_not_parse() {
    for text in [
        "files/abc.png",
        "sessions/x.json",
        "sessions/x.notaulid.json",
        "other/x.01ARZ3NDEKTSV4RRFFQ69G5FAV.json",
        "x.01ARZ3NDEKTSV4RRFFQ69G5FAV.json",
        "sessions/.hidden.01ARZ3NDEKTSV4RRFFQ69G5FAV.json",
    ] {
        assert_eq!(EntryName::parse(text), None, "{text}");
    }
}

#[test]
fn a_result_entry_name_is_made_of_the_content_and_the_finish_time() {
    let finished = at("2026-10-15T12:00:00Z");
    let name = result_entry_name("round-1", b"{}", finished);

    assert_eq!(name, result_entry_name("round-1", b"{}", finished));
    assert_ne!(name, result_entry_name("round-1", b"{ }", finished));
    assert_eq!(name.kind, EntryKind::Result);
    assert_eq!(name.ulid.datetime(), finished);
    assert_eq!(Month::of(name.ulid), OCTOBER);
}

// -------------------------------------------------------------------------
// Writing
// -------------------------------------------------------------------------

#[test]
fn a_new_bundle_holds_the_added_entries_and_reads_them_back() {
    let (_scratch, location) = queue();
    let session = entry(
        EntryKind::Session,
        "batch-01",
        "2026-10-15T12:00:00Z",
        "the session",
    );
    let result = entry(
        EntryKind::Result,
        "batch-01",
        "2026-10-15T12:00:00Z",
        "the result",
    );

    let outcomes = add_entries(
        &location,
        OCTOBER,
        &[session.clone(), result.clone()],
        at("2026-11-20T08:00:00Z"),
    )
    .expect("added");

    assert_eq!(outcomes, [Added::Added, Added::Added]);
    let listed = list_entries(&location, OCTOBER).expect("listed");
    let names: Vec<_> = listed.iter().map(|entry| entry.name.clone()).collect();
    assert_eq!(names, [session.name.zip_name(), result.name.zip_name()]);
    assert_eq!(
        read_entry(&location, OCTOBER, &session.name.zip_name()).expect("read"),
        b"the session"
    );
    assert_eq!(
        read_entry(&location, OCTOBER, &result.name.zip_name()).expect("read"),
        b"the result"
    );
    assert_eq!(listed[0].parsed.as_ref(), Some(&session.name));
    assert!(temp_files(&location).is_empty());
}

#[test]
fn entries_are_deflated() {
    let (_scratch, location) = queue();
    let text = "a session ".repeat(2_000);
    let session = entry(
        EntryKind::Session,
        "batch-01",
        "2026-10-15T12:00:00Z",
        &text,
    );
    add_entries(
        &location,
        OCTOBER,
        std::slice::from_ref(&session),
        at("2026-11-20T08:00:00Z"),
    )
    .expect("added");

    let mut archive = open(&bundle_path(&location, OCTOBER)).expect("opens");
    let file = archive.by_name(&session.name.zip_name()).expect("entry");

    assert_eq!(file.compression(), CompressionMethod::Deflated);
    assert!(
        file.compressed_size() < file.size() / 10,
        "{}",
        file.compressed_size()
    );
}

#[test]
fn adding_to_an_existing_bundle_keeps_every_entry_as_it_was_written() {
    let (_scratch, location) = queue();
    hand_made(
        &location,
        OCTOBER,
        &[("sessions/old.01ARZ3NDEKTSV4RRFFQ69G5FAV.json", "old")],
    );
    let fresh = entry(EntryKind::Session, "new", "2026-10-16T12:00:00Z", "new");

    add_entries(
        &location,
        OCTOBER,
        std::slice::from_ref(&fresh),
        at("2026-11-20T08:00:00Z"),
    )
    .expect("added");

    let mut archive = open(&bundle_path(&location, OCTOBER)).expect("opens");
    let old = archive
        .by_name("sessions/old.01ARZ3NDEKTSV4RRFFQ69G5FAV.json")
        .expect("kept");
    assert_eq!(
        old.compression(),
        CompressionMethod::Stored,
        "an entry that was there is copied, not compressed again"
    );
    drop(old);
    assert_eq!(archive.len(), 2);
    assert_eq!(
        read_entry(&location, OCTOBER, &fresh.name.zip_name()).expect("read"),
        b"new"
    );
}

#[test]
fn adding_an_entry_that_is_there_with_the_same_bytes_changes_nothing() {
    let (_scratch, location) = queue();
    let session = entry(
        EntryKind::Session,
        "batch-01",
        "2026-10-15T12:00:00Z",
        "the session",
    );
    add_entries(
        &location,
        OCTOBER,
        std::slice::from_ref(&session),
        at("2026-11-20T08:00:00Z"),
    )
    .expect("added");
    let before = bundle_bytes(&location, OCTOBER);

    let outcomes = add_entries(&location, OCTOBER, &[session], at("2026-12-01T08:00:00Z"))
        .expect("added again");

    assert_eq!(outcomes, [Added::AlreadyThere]);
    assert_eq!(bundle_bytes(&location, OCTOBER), before);
}

#[test]
fn an_entry_that_is_there_keeps_the_time_it_was_bundled_at() {
    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    let second = entry(EntryKind::Session, "two", "2026-10-16T12:00:00Z", "two");
    add_entries(
        &location,
        OCTOBER,
        std::slice::from_ref(&first),
        at("2026-11-20T08:00:00Z"),
    )
    .expect("added");

    add_entries(
        &location,
        OCTOBER,
        &[first.clone(), second.clone()],
        at("2026-12-01T09:30:15Z"),
    )
    .expect("added");

    let listed = list_entries(&location, OCTOBER).expect("listed");
    let bundled = |name: &EntryName| {
        listed
            .iter()
            .find(|entry| entry.name == name.zip_name())
            .and_then(|entry| entry.bundled_at)
    };
    assert_eq!(
        bundled(&first.name),
        Some(millis(at("2026-11-20T08:00:00Z")) / 1000),
        "the first value stays through a rewrite"
    );
    assert_eq!(
        bundled(&second.name),
        Some(millis(at("2026-12-01T09:30:15Z")) / 1000)
    );
}

#[test]
fn an_entry_with_the_same_name_and_other_bytes_is_a_conflict_and_changes_nothing() {
    let (_scratch, location) = queue();
    let session = entry(
        EntryKind::Session,
        "batch-01",
        "2026-10-15T12:00:00Z",
        "the session",
    );
    add_entries(
        &location,
        OCTOBER,
        std::slice::from_ref(&session),
        at("2026-11-20T08:00:00Z"),
    )
    .expect("added");
    let before = bundle_bytes(&location, OCTOBER);
    let other = NewEntry {
        bytes: b"another session".to_vec(),
        ..session
    };

    let outcomes =
        add_entries(&location, OCTOBER, &[other], at("2026-12-01T08:00:00Z")).expect("returns");

    assert_eq!(outcomes, [Added::Conflict]);
    assert_eq!(bundle_bytes(&location, OCTOBER), before);
}

#[test]
fn an_entry_with_a_damaged_body_does_not_count_as_there() {
    let (_scratch, location) = queue();
    hand_made(
        &location,
        OCTOBER,
        &[("sessions/x.01ARZ3NDEKTSV4RRFFQ69G5FAV.json", "hello")],
    );
    let mut bytes = bundle_bytes(&location, OCTOBER);
    let body = bytes
        .windows(5)
        .position(|window| window == b"hello")
        .expect("stored body");
    bytes[body] = b'j';
    fs::write(bundle_path(&location, OCTOBER), bytes).expect("damaged");
    let session = NewEntry {
        name: EntryName::parse("sessions/x.01ARZ3NDEKTSV4RRFFQ69G5FAV.json").expect("a name"),
        bytes: b"hello".to_vec(),
    };

    let outcomes =
        add_entries(&location, OCTOBER, &[session], at("2026-12-01T08:00:00Z")).expect("returns");

    assert_eq!(outcomes, [Added::Conflict]);
}

#[test]
fn a_bundle_that_cannot_be_read_is_never_overwritten() {
    let (_scratch, location) = queue();
    fs::write(bundle_path(&location, OCTOBER), b"not a zip").expect("written");
    let session = entry(
        EntryKind::Session,
        "batch-01",
        "2026-10-15T12:00:00Z",
        "the session",
    );

    let added = add_entries(&location, OCTOBER, &[session], at("2026-11-20T08:00:00Z"));

    assert!(added.is_err(), "{added:?}");
    assert_eq!(bundle_bytes(&location, OCTOBER), b"not a zip");
    assert!(temp_files(&location).is_empty());
}

#[test]
fn the_old_bundle_is_whole_until_the_new_one_is_put_in_place() {
    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    add_entries(&location, OCTOBER, &[first], at("2026-11-20T08:00:00Z")).expect("added");
    let before = bundle_bytes(&location, OCTOBER);
    let second = entry(EntryKind::Session, "two", "2026-10-16T12:00:00Z", "two");
    let mut looked = false;

    add_entries_with(
        &location,
        OCTOBER,
        &[second],
        at("2026-12-01T08:00:00Z"),
        &mut || {
            looked = true;
            assert_eq!(bundle_bytes(&location, OCTOBER), before, "the old bundle");
            assert_eq!(temp_files(&location).len(), 1, "the new one is beside it");
            Ok(())
        },
    )
    .expect("added");

    assert!(looked, "the hook ran");
    assert_eq!(list_entries(&location, OCTOBER).expect("listed").len(), 2);
    assert!(temp_files(&location).is_empty());
}

#[test]
fn a_failing_hook_leaves_the_old_bundle_and_no_temp_file() {
    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    add_entries(&location, OCTOBER, &[first], at("2026-11-20T08:00:00Z")).expect("added");
    let before = bundle_bytes(&location, OCTOBER);
    let second = entry(EntryKind::Session, "two", "2026-10-16T12:00:00Z", "two");

    let added = add_entries_with(
        &location,
        OCTOBER,
        &[second],
        at("2026-12-01T08:00:00Z"),
        &mut || {
            assert_eq!(temp_files(&location).len(), 1, "the new bundle is written");
            Err(io::Error::other("stopped"))
        },
    );

    assert!(added.is_err());
    assert_eq!(bundle_bytes(&location, OCTOBER), before);
    assert!(temp_files(&location).is_empty());
}

#[test]
fn a_bundle_that_changed_after_it_was_prepared_is_no_longer_current() {
    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    add_entries(&location, OCTOBER, &[first], at("2026-11-20T08:00:00Z")).expect("added");
    let second = entry(EntryKind::Session, "two", "2026-10-16T12:00:00Z", "two");
    let prepared = prepare_bundle(&location, OCTOBER, &[second], at("2026-12-01T08:00:00Z"))
        .expect("prepared");
    assert!(
        prepared.still_current().expect("stat"),
        "nothing changed yet"
    );

    let third = entry(EntryKind::Session, "three", "2026-10-17T12:00:00Z", "three");
    add_entries(&location, OCTOBER, &[third], at("2026-12-01T09:00:00Z")).expect("added");

    assert!(!prepared.still_current().expect("stat"));
}

#[test]
fn a_bundle_that_was_deleted_after_it_was_prepared_is_no_longer_current() {
    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    add_entries(&location, OCTOBER, &[first], at("2026-11-20T08:00:00Z")).expect("added");
    let second = entry(EntryKind::Session, "two", "2026-10-16T12:00:00Z", "two");
    let prepared = prepare_bundle(&location, OCTOBER, &[second], at("2026-12-01T08:00:00Z"))
        .expect("prepared");

    fs::remove_file(bundle_path(&location, OCTOBER)).expect("pruned");

    assert!(!prepared.still_current().expect("stat"));
}

#[test]
fn a_bundle_that_appeared_after_it_was_prepared_is_no_longer_current() {
    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    let prepared =
        prepare_bundle(&location, OCTOBER, &[first], at("2026-12-01T08:00:00Z")).expect("prepared");
    assert!(prepared.still_current().expect("stat"), "still absent");

    hand_made(&location, OCTOBER, &[]);

    assert!(!prepared.still_current().expect("stat"));
}

#[test]
fn nothing_to_add_leaves_the_bundle_alone_and_writes_no_temp_file() {
    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    add_entries(
        &location,
        OCTOBER,
        std::slice::from_ref(&first),
        at("2026-11-20T08:00:00Z"),
    )
    .expect("added");

    let prepared =
        prepare_bundle(&location, OCTOBER, &[first], at("2026-12-01T08:00:00Z")).expect("prepared");

    assert_eq!(prepared.outcomes(), [Added::AlreadyThere]);
    assert!(temp_files(&location).is_empty());
    prepared.commit().expect("committed");
}

// -------------------------------------------------------------------------
// Listing
// -------------------------------------------------------------------------

#[test]
fn only_files_named_like_a_bundle_are_months() {
    let (_scratch, location) = queue();
    for name in [
        "2026-10.zip",
        "2026-09.zip",
        "2026-13.zip",
        "x.zip",
        "2026-1.zip",
        ".asqr-1.tmp",
        "2026-10.json",
    ] {
        fs::write(location.archive().join(name), "").expect("written");
    }

    assert_eq!(
        list_months(&location).expect("listed"),
        [Month::new(2026, 9).expect("a month"), OCTOBER]
    );
}

#[test]
fn entries_of_other_prefixes_are_listed_and_not_parsed() {
    let (_scratch, location) = queue();
    hand_made(&location, OCTOBER, &[("files/abc.png", "png")]);

    let listed = list_entries(&location, OCTOBER).expect("listed");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "files/abc.png");
    assert_eq!(listed[0].parsed, None);
    assert_eq!(listed[0].size, 3);
}

#[test]
fn bundled_results_are_listed_by_name_without_reading_their_content() {
    let (_scratch, location) = queue();
    hand_made(
        &location,
        OCTOBER,
        &[
            ("results/a.01ARZ3NDEKTSV4RRFFQ69G5FAV.json", "hello"),
            ("sessions/a.01ARZ3NDEKTSV4RRFFQ69G5FAV.json", "session"),
            ("replaced/a.01ARZ3NDEKTSV4RRFFQ69G5FAV.json", "replaced"),
        ],
    );
    let mut bytes = bundle_bytes(&location, OCTOBER);
    let body = bytes
        .windows(5)
        .position(|window| window == b"hello")
        .expect("stored body");
    bytes[body] = b'j';
    fs::write(bundle_path(&location, OCTOBER), bytes).expect("damaged");
    fs::write(location.archive().join("2026-11.zip"), "garbage").expect("written");

    let (found, unreadable) = bundled_results(&location).expect("listed");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name.id, "a");
    assert_eq!(found[0].month, OCTOBER);
    assert_eq!(unreadable.len(), 1, "the garbage bundle is reported");
}

// -------------------------------------------------------------------------
// What the archive says about a session
// -------------------------------------------------------------------------

/// A bundle of October with `entries`, bundled at `when`.
fn bundled(location: &QueueLocation, when: &str, entries: &[NewEntry]) {
    add_entries(location, OCTOBER, entries, at(when)).expect("added");
}

#[test]
fn the_newest_bundled_result_of_an_id_answers() {
    let (_scratch, location) = queue();
    bundled(
        &location,
        "2026-11-20T08:00:00Z",
        &[
            entry(
                EntryKind::Result,
                "Round-1",
                "2026-10-10T12:00:00Z",
                "older",
            ),
            entry(
                EntryKind::Result,
                "round-1",
                "2026-10-12T12:00:00Z",
                "newer",
            ),
            entry(
                EntryKind::Result,
                "round-2",
                "2026-10-14T12:00:00Z",
                "other id",
            ),
        ],
    );

    assert_eq!(
        archived_standing(&location, "ROUND-1").expect("readable"),
        ArchivedStanding::Result(b"newer".to_vec())
    );
}

#[test]
fn a_session_archived_after_the_result_was_bundled_hides_it() {
    let (_scratch, location) = queue();
    bundled(
        &location,
        "2026-11-20T08:00:00Z",
        &[entry(
            EntryKind::Result,
            "round-1",
            "2026-10-12T12:00:00Z",
            "the result",
        )],
    );
    loose_session(&location, "round-1", "2026-11-25T10:00:00Z", "{}");

    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Gone
    );
}

#[test]
fn a_session_archived_before_the_result_was_bundled_still_shows_it() {
    let (_scratch, location) = queue();
    bundled(
        &location,
        "2026-11-20T08:00:00Z",
        &[entry(
            EntryKind::Result,
            "round-1",
            "2026-10-12T12:00:00Z",
            "the result",
        )],
    );
    // Archived unanswered beside the unread result, before it was bundled.
    loose_session(&location, "round-1", "2026-11-10T10:00:00Z", "{}");

    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Result(b"the result".to_vec())
    );
}

#[test]
fn a_bundled_session_after_the_result_was_bundled_hides_it_too() {
    let (_scratch, location) = queue();
    bundled(
        &location,
        "2026-10-20T08:00:00Z",
        &[entry(
            EntryKind::Result,
            "round-1",
            "2026-10-12T12:00:00Z",
            "the result",
        )],
    );
    let december = Month::new(2026, 12).expect("a month");
    add_entries(
        &location,
        december,
        &[entry(
            EntryKind::Session,
            "round-1",
            "2026-12-05T10:00:00Z",
            "{}",
        )],
        at("2027-02-01T00:00:00Z"),
    )
    .expect("added");

    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Gone
    );
}

#[test]
fn a_result_without_a_bundling_time_is_never_hidden() {
    let (_scratch, location) = queue();
    hand_made(
        &location,
        OCTOBER,
        &[(
            "results/round-1.01ARZ3NDEKTSV4RRFFQ69G5FAV.json",
            "the result",
        )],
    );
    loose_session(&location, "round-1", "2026-11-25T10:00:00Z", "{}");

    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Result(b"the result".to_vec())
    );
}

#[test]
fn a_session_without_any_result_is_gone() {
    let (_scratch, location) = queue();
    loose_session(&location, "round-1", "2026-10-12T12:00:00Z", "{}");

    assert_eq!(
        archived_standing(&location, "ROUND-1").expect("readable"),
        ArchivedStanding::Gone
    );
}

#[test]
fn a_bundled_session_without_any_result_is_gone() {
    let (_scratch, location) = queue();
    bundled(
        &location,
        "2026-11-20T08:00:00Z",
        &[entry(
            EntryKind::Session,
            "round-1",
            "2026-10-12T12:00:00Z",
            "{}",
        )],
    );

    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Gone
    );
}

#[test]
fn a_replaced_result_answers_nothing() {
    let (_scratch, location) = queue();
    bundled(
        &location,
        "2026-11-20T08:00:00Z",
        &[entry(
            EntryKind::Replaced,
            "round-1",
            "2026-10-12T12:00:00Z",
            "replaced",
        )],
    );

    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Gone
    );
}

#[test]
fn a_loose_replaced_result_answers_nothing() {
    let (_scratch, location) = queue();
    let name = archive_name("round-1", ulid_at("2026-10-12T12:00:00Z"), Archived::Result);
    fs::write(location.archive().join(name), "replaced").expect("written");

    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Gone
    );
}

#[test]
fn an_id_the_archive_does_not_know_is_unknown() {
    let (_scratch, location) = queue();
    loose_session(&location, "other", "2026-10-12T12:00:00Z", "{}");
    bundled(
        &location,
        "2026-11-20T08:00:00Z",
        &[entry(
            EntryKind::Session,
            "other",
            "2026-10-12T12:00:00Z",
            "{}",
        )],
    );

    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Unknown
    );
}

#[test]
fn ids_with_dots_are_told_apart_from_their_prefixes() {
    let (_scratch, location) = queue();
    bundled(
        &location,
        "2026-11-20T08:00:00Z",
        &[entry(
            EntryKind::Result,
            "a.b",
            "2026-10-12T12:00:00Z",
            "dotted",
        )],
    );

    assert_eq!(
        archived_standing(&location, "a.b").expect("readable"),
        ArchivedStanding::Result(b"dotted".to_vec())
    );
    assert_eq!(
        archived_standing(&location, "a").expect("readable"),
        ArchivedStanding::Unknown
    );
}

#[test]
fn an_unreadable_bundle_is_an_error_only_when_nothing_else_knows_the_id() {
    let (_scratch, location) = queue();
    fs::write(bundle_path(&location, OCTOBER), "garbage").expect("written");

    assert!(archived_standing(&location, "round-1").is_err());

    loose_session(&location, "round-1", "2026-10-12T12:00:00Z", "{}");
    assert_eq!(
        archived_standing(&location, "round-1").expect("readable"),
        ArchivedStanding::Gone
    );
}

#[test]
fn the_newest_session_is_found_among_loose_entries_and_bundles() {
    let (_scratch, location) = queue();
    loose_session(&location, "round-1", "2026-10-10T12:00:00Z", "loose");
    bundled(
        &location,
        "2026-11-20T08:00:00Z",
        &[entry(
            EntryKind::Session,
            "Round-1",
            "2026-10-12T12:00:00Z",
            "bundled",
        )],
    );

    let (ulid, bytes) = newest_session(&location, "ROUND-1")
        .expect("listed")
        .expect("found");

    assert_eq!(bytes, b"bundled");
    assert_eq!(ulid, ulid_at("2026-10-12T12:00:00Z"));

    loose_session(&location, "round-1", "2026-10-14T12:00:00Z", "newest loose");
    let (_, bytes) = newest_session(&location, "round-1")
        .expect("listed")
        .expect("found");
    assert_eq!(bytes, b"newest loose");
}

#[test]
fn a_session_nobody_archived_has_no_newest_session() {
    let (_scratch, location) = queue();
    fs::write(bundle_path(&location, OCTOBER), "garbage").expect("written");
    loose_session(&location, "other", "2026-10-12T12:00:00Z", "{}");
    assert_eq!(newest_session(&location, "round-1").expect("listed"), None);
}

#[test]
fn reading_an_entry_checks_its_checksum() {
    let (_scratch, location) = queue();
    hand_made(
        &location,
        OCTOBER,
        &[("sessions/x.01ARZ3NDEKTSV4RRFFQ69G5FAV.json", "hello")],
    );
    let mut bytes = bundle_bytes(&location, OCTOBER);
    let body = bytes
        .windows(5)
        .position(|window| window == b"hello")
        .expect("stored body");
    bytes[body] = b'j';
    fs::write(bundle_path(&location, OCTOBER), bytes).expect("damaged");

    let read = read_entry(
        &location,
        OCTOBER,
        "sessions/x.01ARZ3NDEKTSV4RRFFQ69G5FAV.json",
    );

    assert!(read.is_err(), "{read:?}");
}

#[test]
fn a_bundle_error_names_the_bundle() {
    let (_scratch, location) = queue();
    fs::write(bundle_path(&location, OCTOBER), "garbage").expect("written");

    let error = list_entries(&location, OCTOBER).expect_err("unreadable");

    assert!(error.to_string().contains("2026-10.zip"), "{error}");
}

#[test]
fn a_read_that_runs_while_an_entry_moves_into_a_bundle_never_misses_it() {
    let (_scratch, location) = queue();
    let session = entry(EntryKind::Session, "round-1", "2026-10-12T12:00:00Z", "{}");
    loose_session(&location, "round-1", "2026-10-12T12:00:00Z", "{}");
    let mut seen = Vec::new();

    add_entries_with(
        &location,
        OCTOBER,
        &[session],
        at("2026-11-20T08:00:00Z"),
        &mut || {
            // The new bundle is written, the old state is still in place.
            seen.push(archived_standing(&location, "round-1").expect("readable"));
            Ok(())
        },
    )
    .expect("added");
    // The loose file is removed after the rename.
    fs::remove_file(location.archive().join(archive_name(
        "round-1",
        ulid_at("2026-10-12T12:00:00Z"),
        Archived::Session,
    )))
    .expect("removed");
    seen.push(archived_standing(&location, "round-1").expect("readable"));

    assert_eq!(seen, [ArchivedStanding::Gone, ArchivedStanding::Gone]);
}

#[test]
fn a_rewrite_keeps_the_bundling_time_of_the_entries_it_copies() {
    // Pins that the raw copy keeps the bundling time of the entries it
    // copies over.
    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    add_entries(
        &location,
        OCTOBER,
        std::slice::from_ref(&first),
        at("2026-11-20T08:00:00Z"),
    )
    .expect("added");
    let second = entry(EntryKind::Session, "two", "2026-10-16T12:00:00Z", "two");
    add_entries(&location, OCTOBER, &[second], at("2026-12-01T08:00:00Z")).expect("added");

    let mut archive = open(&bundle_path(&location, OCTOBER)).expect("opens");
    let mut file = archive.by_name(&first.name.zip_name()).expect("entry");
    let mut text = String::new();
    file.read_to_string(&mut text).expect("read");
    assert_eq!(text, "one");
    assert_eq!(
        bundled_at_of(&file),
        Some(millis(at("2026-11-20T08:00:00Z")) / 1000)
    );
}

#[cfg(unix)]
#[test]
fn a_bundle_in_an_unreadable_archive_cannot_be_prepared() {
    use std::os::unix::fs::PermissionsExt;

    let (_scratch, location) = queue();
    let first = entry(EntryKind::Session, "one", "2026-10-15T12:00:00Z", "one");
    fs::set_permissions(location.archive(), fs::Permissions::from_mode(0o000)).expect("chmod");

    let prepared = prepare_bundle(&location, OCTOBER, &[first], at("2026-12-01T08:00:00Z"));

    fs::set_permissions(location.archive(), fs::Permissions::from_mode(0o755))
        .expect("chmod back, so the temp dir can go");
    assert!(prepared.is_err());
}

#[test]
fn an_entry_with_another_timestamp_field_has_no_bundling_time() {
    let (_scratch, location) = queue();
    let file = fs::File::create(bundle_path(&location, OCTOBER)).expect("created");
    let mut writer = zip::ZipWriter::new(file);
    let mut options = zip::write::FullFileOptions::default();
    options
        .add_extra_field(0x5455, [1, 0, 0, 0, 1], false)
        .expect("a standard timestamp field");
    writer
        .start_file("sessions/x.01ARZ3NDEKTSV4RRFFQ69G5FAV.json", options)
        .expect("started");
    writer.finish().expect("finished");

    let listed = list_entries(&location, OCTOBER).expect("listed");

    assert_eq!(listed[0].bundled_at, None);
}
