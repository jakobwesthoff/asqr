// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs;

use serde_json::json;

use super::*;
use crate::format::{SessionResult, UnsentDraft};
use crate::queue::{
    Archived, EntryKind, EntryName, NewEntry, add_entries, archive_name, session_sha256,
};

// =========================================================
// Fixtures
// =========================================================

fn at(text: &str) -> SystemTime {
    SystemTime::from(text.parse::<jiff::Timestamp>().expect("a timestamp"))
}

fn ulid_at(text: &str, random: u128) -> Ulid {
    let millis = at(text)
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64;
    Ulid::from_parts(millis, random)
}

fn queue() -> (tempfile::TempDir, QueueLocation) {
    let scratch = tempfile::tempdir().expect("temp dir");
    let location = QueueLocation::at(scratch.path().join("queue"));
    location.create_layout().expect("layout");
    (scratch, location)
}

/// A root with the queues `names` under it, laid out.
fn root_with(names: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
    let scratch = tempfile::tempdir().expect("temp dir");
    let root = scratch.path().to_path_buf();
    for name in names {
        QueueLocation::named(&root, name)
            .create_layout()
            .expect("layout");
    }
    (scratch, root)
}

/// The text of a session titled `title`.
fn session(title: &str) -> String {
    json!({"asqr": 2, "id": "x", "title": title, "questions": []}).to_string()
}

/// A session that shows one file per entry of `states`, recorded in that
/// state (`None` for a place without a record).
fn session_showing(title: &str, states: &[Option<&str>]) -> String {
    let context: Vec<_> = states
        .iter()
        .map(|state| {
            let mut entry = json!({"type": "code", "path": "/tmp/x.rs"});
            if let Some(state) = state {
                entry["shown"] = json!({"original_path": "/tmp/x.rs", "state": state});
            }
            entry
        })
        .collect();
    json!({"asqr": 2, "id": "x", "title": title, "questions": [
        {"id": "q", "text": "?", "kind": "text", "context": context}
    ]})
    .to_string()
}

/// Archives `body` loose; the ULID is made of `when` and `random`.
fn archive_loose(location: &QueueLocation, id: &str, when: &str, random: u128, body: &str) -> Ulid {
    let ulid = ulid_at(when, random);
    let name = archive_name(id, ulid, Archived::Session);
    fs::write(location.archive().join(name), body).expect("written");
    ulid
}

/// Puts `body` into the bundle of its month as a session.
fn bundle_session(
    location: &QueueLocation,
    id: &str,
    when: &str,
    random: u128,
    body: &str,
) -> Ulid {
    let ulid = ulid_at(when, random);
    bundle_entry(location, EntryKind::Session, id, ulid, body);
    ulid
}

fn bundle_entry(location: &QueueLocation, kind: EntryKind, id: &str, ulid: Ulid, body: &str) {
    let name = EntryName {
        kind,
        id: id.to_owned(),
        ulid,
    };
    add_entries(
        location,
        Month::of(ulid),
        &[NewEntry::named(name, body.as_bytes().to_vec())],
        at("2026-12-01T00:00:00Z"),
    )
    .expect("bundled");
}

fn submitted(id: &str, session_text: &str, submitted_at: &str) -> String {
    let result = SessionResult::submitted(
        id,
        &session_sha256(session_text.as_bytes()),
        submitted_at,
        None,
        vec![],
    );
    serde_json::to_string_pretty(&result).expect("serialises")
}

fn put_in_outbox(location: &QueueLocation, id: &str, result: &str) {
    fs::write(location.outbox().join(format!("{id}.json")), result).expect("written");
}

/// A result `asqr ask --force` moved aside into the archive.
fn put_aside(location: &QueueLocation, id: &str, when: &str, random: u128, result: &str) {
    let name = archive_name(id, ulid_at(when, random), Archived::Result);
    fs::write(location.archive().join(name), result).expect("written");
}

fn listed(location: &QueueLocation) -> History {
    list_history(location, None, None).expect("listed")
}

fn ids(history: &History) -> Vec<&str> {
    history
        .entries
        .iter()
        .map(|entry| entry.entry.id.as_str())
        .collect()
}

// =========================================================
// The list
// =========================================================

#[test]
fn a_loose_session_is_listed_with_its_title() {
    let (_scratch, location) = queue();
    archive_loose(
        &location,
        "hero-2",
        "2026-10-29T16:41:10Z",
        7,
        &session("Pick the hero"),
    );

    let history = listed(&location);

    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].entry.id, "hero-2");
    assert_eq!(history.entries[0].title.as_deref(), Some("Pick the hero"));
    assert!(matches!(
        history.entries[0].entry.place,
        EntryPlace::Loose(_)
    ));
}

#[test]
fn a_bundled_session_is_listed_with_its_place() {
    let (_scratch, location) = queue();
    bundle_session(
        &location,
        "hero-2",
        "2026-09-02T10:00:00Z",
        7,
        &session("Old pick"),
    );

    let history = listed(&location);

    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].title.as_deref(), Some("Old pick"));
    assert!(matches!(
        &history.entries[0].entry.place,
        EntryPlace::Bundled(month, name)
            if month.to_string() == "2026-09" && name.starts_with("sessions/hero-2.")
    ));
}

#[test]
fn an_entry_that_is_loose_and_bundled_is_listed_once() {
    let (_scratch, location) = queue();
    let body = session("Both places");
    let ulid = archive_loose(&location, "hero-2", "2026-09-02T10:00:00Z", 7, &body);
    bundle_entry(&location, EntryKind::Session, "hero-2", ulid, &body);

    let history = listed(&location);

    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.total, 1);
    assert!(matches!(
        history.entries[0].entry.place,
        EntryPlace::Loose(_)
    ));
}

#[test]
fn the_newest_entry_comes_first_whatever_the_place() {
    let (_scratch, location) = queue();
    bundle_session(
        &location,
        "bundled-new",
        "2026-10-02T10:00:00Z",
        1,
        &session("a"),
    );
    archive_loose(
        &location,
        "loose-old",
        "2026-09-02T10:00:00Z",
        2,
        &session("b"),
    );
    archive_loose(
        &location,
        "loose-new",
        "2026-10-03T10:00:00Z",
        3,
        &session("c"),
    );
    bundle_session(
        &location,
        "bundled-old",
        "2026-08-02T10:00:00Z",
        4,
        &session("d"),
    );

    assert_eq!(
        ids(&listed(&location)),
        ["loose-new", "bundled-new", "loose-old", "bundled-old"]
    );
}

#[test]
fn result_files_and_invalid_stems_are_not_sessions() {
    let (_scratch, location) = queue();
    put_aside(&location, "hero-2", "2026-10-01T10:00:00Z", 1, "{}");
    archive_loose(&location, "invalid", "2026-10-01T10:00:00Z", 2, "not json");
    fs::write(location.archive().join("notes.txt"), "x").expect("written");

    assert!(listed(&location).entries.is_empty());
}

#[test]
fn the_limit_cuts_the_list_and_the_total_keeps_counting() {
    let (_scratch, location) = queue();
    for (day, id) in [(1, "a"), (2, "b"), (3, "c")] {
        archive_loose(
            &location,
            id,
            &format!("2026-10-0{day}T10:00:00Z"),
            1,
            &session(id),
        );
    }

    let history = list_history(&location, Some(2), None).expect("listed");

    assert_eq!(ids(&history), ["c", "b"]);
    assert_eq!(history.total, 3);
}

#[test]
fn since_leaves_out_what_was_archived_before_it() {
    let (_scratch, location) = queue();
    for (day, id) in [(1, "a"), (2, "b"), (3, "c")] {
        archive_loose(
            &location,
            id,
            &format!("2026-10-0{day}T10:00:00Z"),
            1,
            &session(id),
        );
    }

    let history = list_history(&location, None, Some(at("2026-10-02T00:00:00Z"))).expect("listed");

    assert_eq!(ids(&history), ["c", "b"]);
    assert_eq!(history.total, 2);
}

#[test]
fn an_unreadable_bundle_is_reported_and_the_rest_listed() {
    let (_scratch, location) = queue();
    archive_loose(
        &location,
        "hero-2",
        "2026-10-29T16:41:10Z",
        7,
        &session("Fine"),
    );
    fs::write(location.archive().join("2026-09.zip"), "this is no zip").expect("written");

    let history = listed(&location);

    assert_eq!(ids(&history), ["hero-2"]);
    assert_eq!(history.unreadable.len(), 1);
    assert!(history.unreadable[0].to_string().contains("2026-09.zip"));
}

#[test]
fn files_are_counted_by_what_the_session_recorded() {
    let (_scratch, location) = queue();
    let body = session_showing(
        "Files",
        &[
            Some("kept"),
            Some("kept"),
            Some("missing"),
            Some("unreadable"),
            Some("not_kept"),
            None,
        ],
    );
    archive_loose(&location, "hero-2", "2026-10-29T16:41:10Z", 7, &body);

    assert_eq!(
        listed(&location).entries[0].files,
        FileCounts {
            kept: 2,
            gone: 2,
            not_kept: 1
        }
    );
}

// =========================================================
// Pairing a session with its result
// =========================================================

fn only(history: &History) -> &HistoryEntry {
    assert_eq!(history.entries.len(), 1, "one entry: {history:?}");
    &history.entries[0]
}

#[test]
fn an_answered_session_shows_how_it_ended_and_when() {
    let (_scratch, location) = queue();
    let body = session("Pick");
    archive_loose(&location, "hero-2", "2026-10-29T16:41:10Z", 7, &body);
    put_in_outbox(
        &location,
        "hero-2",
        &submitted("hero-2", &body, "2026-10-29T16:41:07Z"),
    );

    let history = listed(&location);

    assert_eq!(only(&history).outcome, HistoryOutcome::Sent);
    assert_eq!(only(&history).finished, at("2026-10-29T16:41:07Z"));
}

#[test]
fn the_outcome_words_follow_the_result_status() {
    let (_scratch, location) = queue();
    let sha = |body: &str| session_sha256(body.as_bytes());
    let cases = [
        (
            "cancelled-1",
            SessionResult::cancelled(
                "cancelled-1",
                &sha(&session("c")),
                "2026-10-01T10:00:00Z",
                None,
                None,
                vec![],
            ),
            HistoryOutcome::Rejected,
        ),
        (
            "error-1",
            SessionResult::error(
                "error-1",
                &sha(&session("e")),
                "2026-10-01T10:00:00Z",
                "bad",
            ),
            HistoryOutcome::Error,
        ),
        (
            "withdrawn-1",
            SessionResult::withdrawn(
                "withdrawn-1",
                &sha(&session("w")),
                "2026-10-01T10:00:00Z",
                None,
            ),
            HistoryOutcome::Withdrawn,
        ),
    ];
    for (random, (id, result, _)) in cases.iter().enumerate() {
        let title = &id[..1];
        archive_loose(
            &location,
            id,
            "2026-10-01T10:00:05Z",
            random as u128,
            &session(title),
        );
        put_in_outbox(
            &location,
            id,
            &serde_json::to_string(result).expect("serialises"),
        );
    }

    let history = listed(&location);

    for (id, _, expected) in cases {
        let entry = history
            .entries
            .iter()
            .find(|entry| entry.entry.id == id)
            .expect("listed");
        assert_eq!(entry.outcome, expected, "{id}");
    }
}

#[test]
fn an_unanswered_session_shows_no_result() {
    let (_scratch, location) = queue();
    archive_loose(
        &location,
        "hero-2",
        "2026-10-29T16:41:10Z",
        7,
        &session("Pick"),
    );

    let history = listed(&location);

    assert_eq!(only(&history).outcome, HistoryOutcome::NoResult);
    assert_eq!(only(&history).finished, at("2026-10-29T16:41:10Z"));
}

#[test]
fn a_result_for_other_bytes_does_not_answer_the_session() {
    let (_scratch, location) = queue();
    archive_loose(
        &location,
        "hero-2",
        "2026-10-29T16:41:10Z",
        7,
        &session("Pick"),
    );
    put_in_outbox(
        &location,
        "hero-2",
        &submitted("hero-2", &session("Another round"), "2026-10-29T16:41:07Z"),
    );

    assert_eq!(only(&listed(&location)).outcome, HistoryOutcome::NoResult);
}

#[test]
fn a_withdrawn_session_with_a_draft_is_marked() {
    let (_scratch, location) = queue();
    let with_draft = session("Has a draft");
    let without = session("Has none");
    archive_loose(
        &location,
        "with-draft",
        "2026-10-29T16:41:10Z",
        1,
        &with_draft,
    );
    archive_loose(&location, "without", "2026-10-29T16:41:11Z", 2, &without);
    let draft = UnsentDraft {
        note: Some("half way".to_owned()),
        ..UnsentDraft::default()
    };
    for (id, body, draft) in [
        ("with-draft", &with_draft, Some(draft)),
        ("without", &without, None),
    ] {
        let result = SessionResult::withdrawn(
            id,
            &session_sha256(body.as_bytes()),
            "2026-10-29T16:41:05Z",
            draft,
        );
        put_in_outbox(
            &location,
            id,
            &serde_json::to_string(&result).expect("serialises"),
        );
    }

    let history = listed(&location);

    let mark = |id: &str| {
        history
            .entries
            .iter()
            .find(|entry| entry.entry.id == id)
            .expect("listed")
            .has_draft
    };
    assert!(mark("with-draft"));
    assert!(!mark("without"));
}

#[test]
fn a_result_in_another_month_pairs_by_session_sha256() {
    let (_scratch, location) = queue();
    let body = session("Crossed midnight");
    // Archived in October; the result was finished and bundled in September.
    bundle_session(&location, "hero-2", "2026-10-01T00:00:03Z", 7, &body);
    let result = submitted("hero-2", &body, "2026-09-30T23:59:58Z");
    bundle_entry(
        &location,
        EntryKind::Result,
        "hero-2",
        ulid_at("2026-09-30T23:59:58Z", 9),
        &result,
    );

    assert_eq!(only(&listed(&location)).outcome, HistoryOutcome::Sent);
}

#[test]
fn two_sessions_of_one_id_pair_with_their_own_results() {
    let (_scratch, location) = queue();
    let first = session("First round");
    let second = session("Second round");
    archive_loose(&location, "hero-2", "2026-10-01T10:00:05Z", 1, &first);
    archive_loose(&location, "hero-2", "2026-10-02T10:00:05Z", 2, &second);
    put_aside(
        &location,
        "hero-2",
        "2026-10-02T09:00:00Z",
        3,
        &submitted("hero-2", &first, "2026-10-01T10:00:01Z"),
    );
    let cancelled = SessionResult::cancelled(
        "hero-2",
        &session_sha256(second.as_bytes()),
        "2026-10-02T10:00:01Z",
        None,
        None,
        vec![],
    );
    put_in_outbox(
        &location,
        "hero-2",
        &serde_json::to_string(&cancelled).expect("serialises"),
    );

    let history = listed(&location);

    assert_eq!(history.entries[0].title.as_deref(), Some("Second round"));
    assert_eq!(history.entries[0].outcome, HistoryOutcome::Rejected);
    assert_eq!(history.entries[1].title.as_deref(), Some("First round"));
    assert_eq!(history.entries[1].outcome, HistoryOutcome::Sent);
}

#[test]
fn two_sessions_with_identical_bytes_each_show_their_own_answers() {
    let (_scratch, location) = queue();
    let body = session("Asked twice, the same");
    archive_loose(&location, "hero-2", "2026-10-01T10:00:05Z", 1, &body);
    archive_loose(&location, "hero-2", "2026-10-02T10:00:05Z", 2, &body);
    // The first answer was moved aside when the session was asked again.
    let sha = session_sha256(body.as_bytes());
    let cancelled =
        SessionResult::cancelled("hero-2", &sha, "2026-10-01T10:00:01Z", None, None, vec![]);
    put_aside(
        &location,
        "hero-2",
        "2026-10-02T09:59:00Z",
        3,
        &serde_json::to_string(&cancelled).expect("serialises"),
    );
    put_in_outbox(
        &location,
        "hero-2",
        &submitted("hero-2", &body, "2026-10-02T10:00:01Z"),
    );

    let history = listed(&location);

    assert_eq!(history.entries[0].outcome, HistoryOutcome::Sent);
    assert_eq!(history.entries[0].finished, at("2026-10-02T10:00:01Z"));
    assert_eq!(history.entries[1].outcome, HistoryOutcome::Rejected);
    assert_eq!(history.entries[1].finished, at("2026-10-01T10:00:01Z"));
}

#[test]
fn a_session_whose_result_was_moved_aside_by_force_still_shows_its_answers() {
    let (_scratch, location) = queue();
    let body = session("Replaced answer");
    archive_loose(&location, "hero-2", "2026-10-01T10:00:05Z", 1, &body);
    put_aside(
        &location,
        "hero-2",
        "2026-10-02T09:00:00Z",
        2,
        &submitted("hero-2", &body, "2026-10-01T10:00:01Z"),
    );

    assert_eq!(only(&listed(&location)).outcome, HistoryOutcome::Sent);
}

#[test]
fn a_bundled_result_moved_aside_by_force_still_shows_its_answers() {
    let (_scratch, location) = queue();
    let body = session("Replaced answer");
    bundle_session(&location, "hero-2", "2026-09-01T10:00:05Z", 1, &body);
    bundle_entry(
        &location,
        EntryKind::Replaced,
        "hero-2",
        ulid_at("2026-09-02T09:00:00Z", 2),
        &submitted("hero-2", &body, "2026-09-01T10:00:01Z"),
    );

    assert_eq!(only(&listed(&location)).outcome, HistoryOutcome::Sent);
}

#[test]
fn a_session_carrying_its_own_archived_object_pairs_by_its_bytes() {
    let (_scratch, location) = queue();
    // The recording replaced the file: its own bytes differ from what the
    // result hashed, which `archived.session_sha256` still holds.
    let inbox_bytes = session("As the person saw it");
    let recorded = json!({
        "asqr": 2, "id": "x", "title": "As the person saw it", "questions": [],
        "archived": {"session_sha256": session_sha256(inbox_bytes.as_bytes()), "keep": "compact"}
    })
    .to_string();
    archive_loose(&location, "hero-2", "2026-10-01T10:00:05Z", 1, &recorded);
    put_in_outbox(
        &location,
        "hero-2",
        &submitted("hero-2", &inbox_bytes, "2026-10-01T10:00:01Z"),
    );

    assert_eq!(only(&listed(&location)).outcome, HistoryOutcome::Sent);
}

#[test]
fn an_id_is_compared_ignoring_case() {
    let (_scratch, location) = queue();
    let body = session("Pick");
    archive_loose(&location, "Hero-2", "2026-10-01T10:00:05Z", 1, &body);
    put_in_outbox(
        &location,
        "hero-2",
        &submitted("hero-2", &body, "2026-10-01T10:00:01Z"),
    );

    assert_eq!(only(&listed(&location)).outcome, HistoryOutcome::Sent);
}

// =========================================================
// The reference
// =========================================================

/// The last eight characters of the ULID's text.
fn reference_of(ulid: Ulid) -> String {
    let text = ulid.to_string();
    text[text.len() - 8..].to_owned()
}

fn found(resolved: Resolved) -> EntryRef {
    match resolved.resolution {
        Resolution::Found(entry) => entry,
        other => panic!("expected a match, got {other:?}"),
    }
}

#[test]
fn the_reference_is_the_last_eight_characters_uppercase() {
    let (_scratch, location) = queue();
    let ulid = archive_loose(
        &location,
        "hero-2",
        "2026-10-29T16:41:10Z",
        0x1234_5678_9abc,
        &session("a"),
    );

    let entry = listed(&location).entries.remove(0).entry;

    assert_eq!(entry.reference(), reference_of(ulid));
    assert_eq!(entry.reference().len(), 8);
    assert_eq!(entry.reference(), entry.reference().to_uppercase());
}

#[test]
fn six_characters_resolve_in_any_case() {
    let (_scratch, root) = root_with(&["default"]);
    let location = QueueLocation::named(&root, "default");
    let ulid = archive_loose(
        &location,
        "hero-2",
        "2026-10-29T16:41:10Z",
        0x1234_5678_9abc,
        &session("a"),
    );
    let reference = reference_of(ulid);

    for text in [
        reference[2..].to_owned(),
        reference[2..].to_lowercase(),
        reference.to_lowercase(),
    ] {
        let entry = found(resolve_reference(Some(&root), None, &text).expect("resolved"));
        assert_eq!(entry.id, "hero-2", "{text}");
    }
}

#[test]
fn five_are_too_short() {
    let (_scratch, root) = root_with(&["default"]);

    let resolved = resolve_reference(Some(&root), None, "9PA7K").expect("resolved");

    assert_eq!(resolved.resolution, Resolution::TooShort);
}

#[test]
fn nine_are_too_long() {
    let (_scratch, root) = root_with(&["default"]);

    let resolved = resolve_reference(Some(&root), None, "9PA7KXZQ1").expect("resolved");

    assert_eq!(resolved.resolution, Resolution::TooLong);
}

#[test]
fn a_letter_no_ulid_holds_matches_nothing_and_is_named() {
    let (_scratch, root) = root_with(&["default"]);

    let resolved = resolve_reference(Some(&root), None, "9PA7KXZO").expect("resolved");

    assert_eq!(resolved.resolution, Resolution::NotAReference('O'));
}

#[test]
fn no_entry_ending_in_the_text_is_no_match() {
    let (_scratch, root) = root_with(&["default"]);
    let location = QueueLocation::named(&root, "default");
    archive_loose(
        &location,
        "hero-2",
        "2026-10-29T16:41:10Z",
        0x1234_5678_9abc,
        &session("a"),
    );

    let resolved = resolve_reference(Some(&root), None, "ZZZZZZ").expect("resolved");

    assert_eq!(resolved.resolution, Resolution::NoMatch);
}

#[test]
fn a_unique_match_in_another_queue_is_found() {
    let (_scratch, root) = root_with(&["default", "work"]);
    let work = QueueLocation::named(&root, "work");
    let ulid = archive_loose(
        &work,
        "hero-2",
        "2026-10-29T16:41:10Z",
        0x1234_5678_9abc,
        &session("a"),
    );
    let reference = reference_of(ulid);

    let entry = found(resolve_reference(Some(&root), None, &reference).expect("resolved"));

    assert_eq!(entry.queue.name(), Some("work"));
    assert_eq!(entry.id, "hero-2");
}

#[test]
fn an_entry_in_a_bundle_is_found_by_its_name_alone() {
    let (_scratch, root) = root_with(&["default"]);
    let location = QueueLocation::named(&root, "default");
    let ulid = bundle_session(
        &location,
        "hero-2",
        "2026-09-02T10:00:00Z",
        0x1234_5678_9abc,
        &session("a"),
    );
    let reference = reference_of(ulid);

    let entry = found(resolve_reference(Some(&root), None, &reference).expect("resolved"));

    assert!(matches!(entry.place, EntryPlace::Bundled(..)));
}

#[test]
fn a_queue_given_as_a_directory_is_searched_when_it_is_passed() {
    let (_scratch, root) = root_with(&["default"]);
    let (_other, outside) = queue();
    let ulid = archive_loose(
        &outside,
        "hero-2",
        "2026-10-29T16:41:10Z",
        0x1234_5678_9abc,
        &session("a"),
    );
    let reference = reference_of(ulid);

    assert_eq!(
        resolve_reference(Some(&root), None, &reference)
            .expect("resolved")
            .resolution,
        Resolution::NoMatch
    );
    assert_eq!(
        found(resolve_reference(Some(&root), Some(&outside), &reference).expect("resolved")).queue,
        outside
    );
}

#[test]
fn two_matches_are_ambiguous_with_both_listed() {
    let (_scratch, root) = root_with(&["default", "work"]);
    // The last 30 bits are the same, the 10 bits before them differ.
    archive_loose(
        &QueueLocation::named(&root, "default"),
        "one",
        "2026-10-29T16:41:10Z",
        0x01_2345_6789,
        &session("a"),
    );
    let second = archive_loose(
        &QueueLocation::named(&root, "work"),
        "two",
        "2026-10-30T16:41:10Z",
        0x02_2345_6789,
        &session("b"),
    );
    let shared = reference_of(second)[2..].to_owned();

    let resolved = resolve_reference(Some(&root), None, &shared).expect("resolved");

    let Resolution::Ambiguous(entries) = resolved.resolution else {
        panic!("expected ambiguity, got {:?}", resolved.resolution);
    };
    let mut found: Vec<_> = entries.iter().map(|entry| entry.id.as_str()).collect();
    found.sort_unstable();
    assert_eq!(found, ["one", "two"]);
}

#[test]
fn an_unreadable_bundle_is_reported_with_the_resolution() {
    let (_scratch, root) = root_with(&["default"]);
    let location = QueueLocation::named(&root, "default");
    fs::write(location.archive().join("2026-09.zip"), "this is no zip").expect("written");

    let resolved = resolve_reference(Some(&root), None, "ABCDEFGH").expect("resolved");

    assert_eq!(resolved.resolution, Resolution::NoMatch);
    assert_eq!(resolved.unreadable.len(), 1);
}

// =========================================================
// Entries that do not fit
// =========================================================

#[test]
fn a_bundled_session_of_an_invalid_stem_is_not_listed() {
    let (_scratch, location) = queue();
    bundle_session(
        &location,
        "invalid",
        "2026-09-02T10:00:00Z",
        1,
        "not a session",
    );

    assert!(listed(&location).entries.is_empty());
}

#[test]
fn a_draft_is_no_answer() {
    let (_scratch, location) = queue();
    let body = session("Pick");
    archive_loose(&location, "hero-2", "2026-10-29T16:41:10Z", 7, &body);
    let draft = SessionResult::draft("hero-2", "q", vec![]);
    let mut value = serde_json::to_value(draft).expect("serialises");
    value["session_sha256"] = json!(session_sha256(body.as_bytes()));
    value["submitted_at"] = json!("2026-10-29T16:41:07Z");
    put_in_outbox(&location, "hero-2", &value.to_string());

    assert_eq!(only(&listed(&location)).outcome, HistoryOutcome::NoResult);
}

#[test]
fn a_session_that_cannot_be_read_is_listed_without_its_content() {
    let (_scratch, location) = queue();
    let ulid = ulid_at("2026-10-29T16:41:10Z", 7);
    // A directory where the file should be.
    fs::create_dir(
        location
            .archive()
            .join(archive_name("hero-2", ulid, Archived::Session)),
    )
    .expect("created");

    let history = listed(&location);

    assert_eq!(only(&history).title, None);
    assert_eq!(only(&history).outcome, HistoryOutcome::NoResult);
}

#[test]
fn a_result_that_cannot_be_read_answers_nothing() {
    let (_scratch, location) = queue();
    archive_loose(
        &location,
        "hero-2",
        "2026-10-29T16:41:10Z",
        7,
        &session("Pick"),
    );
    fs::create_dir(location.outbox().join("hero-2.json")).expect("created");

    assert_eq!(only(&listed(&location)).outcome, HistoryOutcome::NoResult);
}

#[test]
fn a_state_asqr_does_not_know_counts_as_no_file() {
    let (_scratch, location) = queue();
    let body = session_showing("Files", &[Some("kept"), Some("from-the-future")]);
    archive_loose(&location, "hero-2", "2026-10-29T16:41:10Z", 7, &body);

    assert_eq!(
        only(&listed(&location)).files,
        FileCounts {
            kept: 1,
            gone: 0,
            not_kept: 0
        }
    );
}

#[test]
fn a_session_that_is_no_json_shows_no_files() {
    let (_scratch, location) = queue();
    archive_loose(&location, "hero-2", "2026-10-29T16:41:10Z", 7, "not json");

    let history = listed(&location);

    assert_eq!(only(&history).files, FileCounts::default());
    assert_eq!(only(&history).title, None);
}
