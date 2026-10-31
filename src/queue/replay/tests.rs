// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use serde_json::{Value, json};
use ulid::Ulid;

use super::*;
use crate::format::{Answer, UnsentDraft};
use crate::keep::sha256_hex;
use crate::queue::history::{EntryPlace, list_history};
use crate::queue::{
    Archived, EntryKind, EntryName, Month, NewEntry, QueueLocation, add_entries, archive_name,
    session_sha256,
};

// =========================================================
// Fixtures
// =========================================================

struct Scene {
    _scratch: tempfile::TempDir,
    location: QueueLocation,
    /// Where `extract` writes.
    shown: PathBuf,
}

fn scene() -> Scene {
    let scratch = tempfile::tempdir().expect("temp dir");
    let location = QueueLocation::at(scratch.path().join("queue"));
    location.create_layout().expect("layout");
    let shown = scratch.path().join("shown");
    fs::create_dir(&shown).expect("created");
    Scene {
        _scratch: scratch,
        location,
        shown,
    }
}

fn ulid() -> Ulid {
    Ulid::from_parts(1_788_000_000_000, 0x1234_5678_9abc)
}

/// A kept copy: its bytes and the name `files/<name>` it is kept under.
fn copy(bytes: &[u8], extension: &str) -> (String, Vec<u8>) {
    (format!("{}.{extension}", sha256_hex(bytes)), bytes.to_vec())
}

/// A session with one question that shows `entry`.
fn session_json(entry: Value) -> Value {
    json!({"asqr": 2, "id": "hero-2", "title": "Pick the hero", "questions": [
        {"id": "q1", "text": "Which?", "kind": "single",
         "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
         "context": [entry]},
        {"id": "q2", "text": "Why?", "kind": "text"}
    ]})
}

fn kept_image(name: &str, original: &str) -> Value {
    json!({"type": "image", "path": format!("files/{name}"), "shown": {
        "original_path": original, "state": "kept", "sha256": "ab".repeat(32),
        "size": 100, "modified": "2026-10-29T16:40:11Z", "kept_size": 40}})
}

/// Archives `session` loose, with `copies` under `archive/files/`, and
/// puts the answering `result` into the outbox.
fn finish_loose(
    scene: &Scene,
    session: &Value,
    copies: &[(String, Vec<u8>)],
    result: Option<SessionResult>,
) -> EntryRef {
    let bytes = serde_json::to_vec_pretty(session).expect("serialises");
    let name = archive_name("hero-2", ulid(), Archived::Session);
    let path = scene.location.archive().join(name);
    fs::write(&path, &bytes).expect("written");
    if !copies.is_empty() {
        fs::create_dir_all(scene.location.kept_files()).expect("created");
    }
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

fn answered(session: &Value, answers: Vec<Answer>) -> SessionResult {
    let bytes = serde_json::to_vec_pretty(session).expect("serialises");
    SessionResult::submitted(
        "hero-2",
        &session_sha256(&bytes),
        "2026-08-29T10:39:00Z",
        Some("thanks".to_owned()),
        answers,
    )
}

/// An extractor that writes below `scene.shown`, each file in a directory
/// of its own, the way the opener's run directory does.
fn extractor(shown: &Path) -> impl FnMut(&str, &[u8]) -> io::Result<PathBuf> {
    let shown = shown.to_owned();
    let mut count = 0;
    move |name, bytes| {
        count += 1;
        let dir = shown.join(count.to_string());
        fs::create_dir(&dir)?;
        let path = dir.join(name);
        fs::write(&path, bytes)?;
        Ok(path)
    }
}

fn load(scene: &Scene, entry: &EntryRef) -> Result<LoadedReplay, ReplayError> {
    load_replay(entry, &mut extractor(&scene.shown))
}

fn first_image_path(loaded: &LoadedReplay) -> String {
    match &loaded.session.questions[0].context[0] {
        crate::format::ContextEntry::Image(image) => image.path.clone(),
        other => panic!("expected an image, got {other:?}"),
    }
}

// =========================================================
// The answers
// =========================================================

#[test]
fn a_replayed_session_pairs_the_result_it_was_answered_with() {
    let scene = scene();
    let session = session_json(json!({"type": "code", "content": "x"}));
    let answers = vec![Answer {
        selected: vec!["b".to_owned()],
        ..Answer::new("q1")
    }];
    let entry = finish_loose(
        &scene,
        &session,
        &[],
        Some(answered(&session, answers.clone())),
    );

    let loaded = load(&scene, &entry).expect("loaded");

    let result = loaded.result.expect("the result is found");
    assert_eq!(result.answers, answers);
    assert_eq!(result.note.as_deref(), Some("thanks"));
    let draft = loaded.draft.expect("the answers fill the session");
    assert_eq!(draft.answers, answers);
    assert_eq!(draft.note.as_deref(), Some("thanks"));
}

#[test]
fn a_skipped_question_stays_skipped_where_the_question_has_a_default() {
    let scene = scene();
    let mut session = session_json(json!({"type": "code", "content": "x"}));
    session["questions"][0]["options"][1]["default"] = json!(true);
    let skipped = vec![Answer::skipped("q1")];
    let entry = finish_loose(&scene, &session, &[], Some(answered(&session, skipped)));

    let loaded = load(&scene, &entry).expect("loaded");

    let draft = loaded.draft.expect("a draft");
    assert!(draft.answers[0].cleared, "{:?}", draft.answers[0]);
}

#[test]
fn a_withdrawn_session_shows_the_draft_the_person_had_not_sent() {
    let scene = scene();
    let session = session_json(json!({"type": "code", "content": "x"}));
    let unsent = UnsentDraft {
        note: Some("half way".to_owned()),
        answers: vec![Answer {
            selected: vec!["a".to_owned()],
            ..Answer::new("q1")
        }],
    };
    let bytes = serde_json::to_vec_pretty(&session).expect("serialises");
    let result = SessionResult::withdrawn(
        "hero-2",
        &session_sha256(&bytes),
        "2026-08-29T10:39:00Z",
        Some(unsent.clone()),
    );
    let entry = finish_loose(&scene, &session, &[], Some(result));

    let loaded = load(&scene, &entry).expect("loaded");

    let draft = loaded.draft.expect("the unsent draft");
    assert_eq!(draft.answers, unsent.answers);
    assert_eq!(draft.note, unsent.note);
}

#[test]
fn an_error_result_and_no_result_start_empty() {
    let scene = scene();
    let session = session_json(json!({"type": "code", "content": "x"}));
    let entry = finish_loose(&scene, &session, &[], None);

    let loaded = load(&scene, &entry).expect("loaded");

    assert!(loaded.result.is_none());
    assert!(loaded.draft.is_none());
}

// =========================================================
// The files
// =========================================================

#[test]
fn a_loose_kept_file_is_shown_from_the_scratch_copy() {
    let scene = scene();
    let (name, bytes) = copy(b"webp bytes", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let entry = finish_loose(&scene, &session, &[(name.clone(), bytes.clone())], None);

    let loaded = load(&scene, &entry).expect("loaded");

    let path = first_image_path(&loaded);
    assert!(Path::new(&path).is_absolute(), "{path}");
    assert!(
        path.starts_with(scene.shown.to_str().expect("UTF-8")),
        "{path}"
    );
    // The replay no longer depends on the archive after the copy.
    fs::remove_file(scene.location.kept_files().join(&name)).expect("removed");
    assert_eq!(fs::read(&path).expect("still readable"), bytes);
    let file = &loaded.files[&path];
    assert_eq!(file.state, "kept");
    assert_eq!(file.original_path, "/tmp/x/hero.png");
    assert_eq!(file.damaged, None);
}

#[test]
fn a_bundled_kept_image_is_extracted_from_its_bundle() {
    let scene = scene();
    let (name, bytes) = copy(b"bundled webp", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let month = Month::of(ulid());
    let session_name = EntryName {
        kind: EntryKind::Session,
        id: "hero-2".to_owned(),
        ulid: ulid(),
    };
    let zip_name = session_name.zip_name();
    add_entries(
        &scene.location,
        month,
        &[
            NewEntry::named(
                session_name,
                serde_json::to_vec_pretty(&session).expect("serialises"),
            ),
            NewEntry::kept_file(&name, {
                let path = scene.shown.join("source");
                fs::write(&path, &bytes).expect("written");
                path
            })
            .expect("a kept name"),
        ],
        SystemTime::now(),
    )
    .expect("bundled");
    let entry = EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Bundled(month, zip_name),
    };

    let loaded = load(&scene, &entry).expect("loaded");

    let path = first_image_path(&loaded);
    assert_eq!(fs::read(&path).expect("extracted"), bytes);
    assert!(!scene.location.kept_files().exists());
}

#[test]
fn a_kept_makefile_replays_under_its_own_name_so_it_highlights_as_make() {
    let scene = scene();
    let (name, bytes) = copy(b"all:\n\tcc x.c\n", "dat");
    let session = session_json(json!({"type": "code", "path": format!("files/{name}"),
        "shown": {"original_path": "/work/Makefile", "state": "kept", "sha256": "cd".repeat(32),
                  "size": 14, "kept_size": 14}}));
    let entry = finish_loose(&scene, &session, &[(name, bytes)], None);

    let loaded = load(&scene, &entry).expect("loaded");

    let path = match &loaded.session.questions[0].context[0] {
        crate::format::ContextEntry::Code(code) => code.path.clone().expect("a path"),
        other => panic!("expected code, got {other:?}"),
    };
    assert_eq!(Path::new(&path).file_name().expect("a name"), "Makefile");
}

#[test]
fn a_damaged_kept_copy_is_recorded_and_not_extracted() {
    let scene = scene();
    let (name, _) = copy(b"what the name promises", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let entry = finish_loose(
        &scene,
        &session,
        &[(name.clone(), b"something else".to_vec())],
        None,
    );

    let loaded = load(&scene, &entry).expect("loaded");

    let path = first_image_path(&loaded);
    assert_eq!(
        path,
        format!("files/{name}"),
        "the path stays relative, which no viewer opens"
    );
    assert_eq!(
        loaded.files[&path].damaged.as_deref(),
        Some("the kept copy is damaged")
    );
    assert_eq!(fs::read_dir(&scene.shown).expect("listed").count(), 0);
}

#[test]
fn a_kept_copy_the_archive_lacks_is_recorded_as_gone() {
    let scene = scene();
    let (name, _) = copy(b"never stored", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let entry = finish_loose(&scene, &session, &[], None);

    let loaded = load(&scene, &entry).expect("loaded");

    let path = first_image_path(&loaded);
    assert_eq!(
        loaded.files[&path].damaged.as_deref(),
        Some("the kept copy is missing from the archive")
    );
}

#[test]
fn a_path_outside_files_is_never_read() {
    let scene = scene();
    let secret = scene.shown.join("secret.png");
    fs::write(&secret, b"not for the replay").expect("written");
    let session = session_json(
        json!({"type": "image", "path": "files/../../shown/secret.png",
        "shown": {"original_path": "/tmp/x.png", "state": "kept", "sha256": "ab".repeat(32), "size": 1, "kept_size": 1}}),
    );
    let entry = finish_loose(&scene, &session, &[], None);

    let loaded = load(&scene, &entry).expect("loaded");

    let path = first_image_path(&loaded);
    assert_eq!(path, "files/../../shown/secret.png");
    assert!(loaded.files[&path].damaged.is_some());
    assert_eq!(
        fs::read_dir(&scene.shown).expect("listed").count(),
        1,
        "only the secret is there"
    );
}

#[test]
fn files_that_were_not_kept_keep_their_original_path_and_their_record() {
    let scene = scene();
    let session = session_json(json!({"type": "image", "path": "/tmp/gone.png",
        "shown": {"original_path": "/tmp/gone.png", "state": "missing"}}));
    let entry = finish_loose(&scene, &session, &[], None);

    let loaded = load(&scene, &entry).expect("loaded");

    assert_eq!(first_image_path(&loaded), "/tmp/gone.png");
    let file = &loaded.files["/tmp/gone.png"];
    assert_eq!(
        (file.state.as_str(), file.sha256.as_deref()),
        ("missing", None)
    );
}

// =========================================================
// What cannot be shown
// =========================================================

#[test]
fn an_unparseable_session_is_an_error_naming_the_entry() {
    let scene = scene();
    let path = scene
        .location
        .archive()
        .join(archive_name("hero-2", ulid(), Archived::Session));
    fs::write(&path, "this is no session").expect("written");
    let entry = EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Loose(path.clone()),
    };

    let error = load(&scene, &entry).expect_err("refused");

    let text = error.to_string();
    assert!(text.contains(&path.display().to_string()), "{text}");
    assert!(
        text.contains("the archived session cannot be shown"),
        "{text}"
    );
}

#[test]
fn an_unreadable_bundle_is_an_error_naming_it() {
    let scene = scene();
    let month = Month::of(ulid());
    fs::write(
        scene.location.archive().join(format!("{month}.zip")),
        "no zip",
    )
    .expect("written");
    let entry = EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Bundled(month, "sessions/hero-2.x.json".to_owned()),
    };

    let error = load(&scene, &entry).expect_err("refused");

    assert!(
        error.to_string().contains(&format!("{month}.zip")),
        "{error}"
    );
}

#[test]
fn loading_changes_nothing_in_the_queue() {
    let scene = scene();
    let (name, bytes) = copy(b"webp bytes", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let answers = vec![Answer::new("q1")];
    let entry = finish_loose(
        &scene,
        &session,
        &[(name, bytes)],
        Some(answered(&session, answers)),
    );
    let listing = |location: &QueueLocation| {
        let mut names: Vec<String> = walk(location.dir());
        names.sort();
        names
    };
    let before = listing(&scene.location);

    load(&scene, &entry).expect("loaded");

    assert_eq!(listing(&scene.location), before);
    assert_eq!(
        list_history(&scene.location, None, None)
            .expect("listed")
            .entries
            .len(),
        1
    );
}

fn walk(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).expect("listed") {
        let entry = entry.expect("entry");
        let path = entry.path();
        found.push(format!(
            "{} {}",
            path.display(),
            fs::metadata(&path).expect("metadata").len()
        ));
        if path.is_dir() {
            found.extend(walk(&path));
        }
    }
    found
}

// =========================================================
// Edges
// =========================================================

#[test]
fn a_file_without_a_record_is_recorded_as_not_recorded_and_never_read_from_disk() {
    let scene = scene();
    let session = session_json(json!({"type": "image", "path": "/tmp/old.png"}));
    let entry = finish_loose(&scene, &session, &[], None);

    let loaded = load(&scene, &entry).expect("loaded");

    let file = &loaded.files["/tmp/old.png"];
    assert_eq!(
        (file.state.as_str(), file.original_path.as_str()),
        ("not_recorded", "/tmp/old.png")
    );
    assert_eq!(file.copy, None);
}

#[test]
fn both_sides_of_a_diff_without_a_record_are_recorded_as_not_recorded() {
    let scene = scene();
    let session = session_json(json!({"type": "diff",
        "old": {"path": "/work/old.rs"}, "new": {"path": "/work/new.rs"}}));
    let entry = finish_loose(&scene, &session, &[], None);

    let loaded = load(&scene, &entry).expect("loaded");

    let mut states: Vec<_> = loaded
        .files
        .iter()
        .map(|(path, file)| (path.as_str(), file.state.as_str()))
        .collect();
    states.sort_unstable();
    assert_eq!(
        states,
        [
            ("/work/new.rs", "not_recorded"),
            ("/work/old.rs", "not_recorded")
        ]
    );
}

#[test]
fn a_path_without_a_record_next_to_a_kept_one_is_recorded_on_its_own() {
    let scene = scene();
    let (name, bytes) = copy(b"png", "webp");
    let session = session_json(json!({"type": "diff",
        "old": {"path": format!("files/{name}"), "shown": {
            "original_path": "/work/old.rs", "state": "kept", "kept_size": 3}},
        "new": {"path": "/work/new.rs"}}));
    let entry = finish_loose(&scene, &session, &[(name, bytes)], None);

    let loaded = load(&scene, &entry).expect("loaded");

    assert_eq!(loaded.files["/work/new.rs"].state, "not_recorded");
    assert_eq!(
        loaded
            .files
            .values()
            .filter(|file| file.state == "kept")
            .count(),
        1
    );
}

#[test]
fn a_session_that_parses_and_does_not_validate_is_an_error() {
    let scene = scene();
    let mut session = session_json(json!({"type": "code", "content": "x"}));
    session["questions"][1]["id"] = json!("q1");
    let entry = finish_loose(&scene, &session, &[], None);

    let error = load(&scene, &entry).expect_err("refused");

    assert!(error.to_string().contains("duplicate id"), "{error}");
}

#[test]
fn an_error_result_gives_no_answers() {
    let scene = scene();
    let session = session_json(json!({"type": "code", "content": "x"}));
    let bytes = serde_json::to_vec_pretty(&session).expect("serialises");
    let result = SessionResult::error(
        "hero-2",
        &session_sha256(&bytes),
        "2026-08-29T10:39:00Z",
        "bad",
    );
    let entry = finish_loose(&scene, &session, &[], Some(result));

    let loaded = load(&scene, &entry).expect("loaded");

    assert!(loaded.result.is_some());
    assert!(loaded.draft.is_none());
}

#[test]
fn a_session_the_compaction_moved_into_its_bundle_meanwhile_is_read_there() {
    let scene = scene();
    let session = session_json(json!({"type": "code", "content": "x"}));
    let name = EntryName {
        kind: EntryKind::Session,
        id: "hero-2".to_owned(),
        ulid: ulid(),
    };
    add_entries(
        &scene.location,
        Month::of(ulid()),
        &[NewEntry::named(
            name,
            serde_json::to_vec(&session).expect("serialises"),
        )],
        SystemTime::now(),
    )
    .expect("bundled");
    let gone = scene
        .location
        .archive()
        .join(archive_name("hero-2", ulid(), Archived::Session));
    let entry = EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Loose(gone),
    };

    let loaded = load(&scene, &entry).expect("found in the bundle");

    assert_eq!(loaded.session.title.as_deref(), Some("Pick the hero"));
}

#[test]
fn a_loose_session_that_is_nowhere_is_an_error_naming_its_path() {
    let scene = scene();
    let gone = scene
        .location
        .archive()
        .join(archive_name("hero-2", ulid(), Archived::Session));
    let entry = EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Loose(gone.clone()),
    };

    let error = load(&scene, &entry).expect_err("refused");

    assert!(
        error.to_string().contains(&gone.display().to_string()),
        "{error}"
    );
}

#[test]
fn a_loose_session_that_cannot_be_read_is_an_error_naming_its_path() {
    let scene = scene();
    let path = scene
        .location
        .archive()
        .join(archive_name("hero-2", ulid(), Archived::Session));
    fs::create_dir(&path).expect("a directory where the file belongs");
    let entry = EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Loose(path.clone()),
    };

    let error = load(&scene, &entry).expect_err("refused");

    assert!(
        error.to_string().contains(&path.display().to_string()),
        "{error}"
    );
}

#[test]
fn a_kept_path_that_is_no_kept_name_is_recorded_as_such() {
    let scene = scene();
    let session = session_json(json!({"type": "image", "path": "files/not-a-hash.webp",
        "shown": {"original_path": "/tmp/x.png", "state": "kept", "size": 1, "kept_size": 1}}));
    let entry = finish_loose(&scene, &session, &[], None);

    let loaded = load(&scene, &entry).expect("loaded");

    assert_eq!(
        loaded.files["files/not-a-hash.webp"].damaged.as_deref(),
        Some("the kept copy is not a name asqr keeps")
    );
}

#[test]
fn a_kept_copy_that_cannot_be_read_is_recorded_with_the_reason() {
    let scene = scene();
    let (name, _) = copy(b"in a directory", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let entry = finish_loose(&scene, &session, &[], None);
    fs::create_dir_all(scene.location.kept_files().join(&name))
        .expect("a directory where the copy belongs");

    let loaded = load(&scene, &entry).expect("loaded");

    let reason = loaded.files[&format!("files/{name}")]
        .damaged
        .clone()
        .expect("recorded");
    assert!(
        reason.starts_with("the kept copy cannot be read"),
        "{reason}"
    );
}

#[test]
fn a_bundle_without_the_kept_copy_is_recorded_as_missing() {
    let scene = scene();
    let (name, _) = copy(b"not in the bundle", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let session_name = EntryName {
        kind: EntryKind::Session,
        id: "hero-2".to_owned(),
        ulid: ulid(),
    };
    let zip_name = session_name.zip_name();
    add_entries(
        &scene.location,
        Month::of(ulid()),
        &[NewEntry::named(
            session_name,
            serde_json::to_vec(&session).expect("serialises"),
        )],
        SystemTime::now(),
    )
    .expect("bundled");
    let entry = EntryRef {
        queue: scene.location.clone(),
        id: "hero-2".to_owned(),
        ulid: ulid(),
        place: EntryPlace::Bundled(Month::of(ulid()), zip_name),
    };

    let loaded = load(&scene, &entry).expect("loaded");

    assert_eq!(
        loaded.files[&format!("files/{name}")].damaged.as_deref(),
        Some("the kept copy is missing from the archive")
    );
}

#[test]
fn a_scratch_directory_that_cannot_be_written_ends_the_replay() {
    let scene = scene();
    let (name, bytes) = copy(b"webp bytes", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let entry = finish_loose(&scene, &session, &[(name, bytes)], None);

    let error =
        load_replay(&entry, &mut |_, _| Err(io::Error::other("disk full"))).expect_err("refused");

    assert!(error.to_string().contains("disk full"), "{error}");
}

#[test]
fn a_kept_copy_larger_than_a_kept_file_can_be_is_recorded_as_unreadable() {
    let scene = scene();
    let (name, _) = copy(b"claims to be small", "webp");
    let session = session_json(kept_image(&name, "/tmp/x/hero.png"));
    let entry = finish_loose(&scene, &session, &[], None);
    fs::create_dir_all(scene.location.kept_files()).expect("created");
    // A sparse file one byte past the limit costs no disk.
    let big = fs::File::create(scene.location.kept_files().join(&name)).expect("created");
    big.set_len(crate::format::MAX_IMAGE_BYTES + 1)
        .expect("sized");

    let loaded = load(&scene, &entry).expect("loaded");

    let reason = loaded.files[&format!("files/{name}")]
        .damaged
        .clone()
        .expect("recorded");
    assert!(
        reason.contains("larger than a kept file can be"),
        "{reason}"
    );
}
