// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drafts (spec section 5.3): the answers to a session while it is being
//! answered, in `drafts/<id>.json`.

use std::io;

use super::atomic::write_atomically;
use super::{QueueLocation, find_session_file};
use crate::format::SessionResult;

pub fn load_draft(location: &QueueLocation, id: &str) -> io::Result<Option<SessionResult>> {
    let Some(path) = find_session_file(&location.drafts(), id)? else {
        return Ok(None);
    };
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Saves `draft` under its id. An existing draft of the same session keeps
/// its file name, whatever case the id is spelled in now.
pub fn save_draft(location: &QueueLocation, draft: &SessionResult) -> io::Result<()> {
    let path = match find_session_file(&location.drafts(), &draft.id)? {
        Some(existing) => existing,
        None => location.drafts().join(format!("{}.json", draft.id)),
    };
    let json = serde_json::to_vec_pretty(draft).map_err(io::Error::other)?;
    write_atomically(&path, &json)
}

/// Removes the draft of `id`, if there is one.
pub fn delete_draft(location: &QueueLocation, id: &str) -> io::Result<()> {
    match find_session_file(&location.drafts(), id)? {
        Some(path) => std::fs::remove_file(path),
        None => Ok(()),
    }
}

/// Whether the person entered anything: a selection, typed text or a note.
/// A selection counts even where it does not answer the question yet (a
/// `multi` below `min`), since it is still work that a replacement would
/// throw away.
pub fn draft_has_answer(draft: &SessionResult) -> bool {
    let filled =
        |text: &Option<String>| text.as_deref().is_some_and(|text| !text.trim().is_empty());
    draft
        .answers
        .iter()
        .any(|answer| !answer.selected.is_empty() || filled(&answer.custom) || filled(&answer.note))
}

/// Whether the draft of `id` holds an answer. A draft that cannot be read
/// counts as answered: refusing a replacement is better than losing work.
pub fn has_answered_draft(location: &QueueLocation, id: &str) -> bool {
    match load_draft(location, id) {
        Ok(draft) => draft.as_ref().is_some_and(draft_has_answer),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::format::Answer;

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path());
        location.create_layout().expect("layout");
        (scratch, location)
    }

    fn draft(id: &str, answers: Vec<Answer>) -> SessionResult {
        SessionResult::draft(id, "q1", answers)
    }

    #[test]
    fn saves_loads_and_deletes_a_draft() {
        let (_scratch, location) = queue();
        let saved = draft("batch-01", vec![Answer::new("q1")]);

        save_draft(&location, &saved).expect("draft is saved");
        assert_eq!(
            load_draft(&location, "BATCH-01").expect("readable"),
            Some(saved)
        );

        delete_draft(&location, "batch-01").expect("draft is deleted");
        assert_eq!(load_draft(&location, "batch-01").expect("readable"), None);
        delete_draft(&location, "batch-01").expect("deleting a missing draft is fine");
    }

    #[test]
    fn saving_keeps_the_spelling_of_an_existing_draft() {
        let (_scratch, location) = queue();
        save_draft(&location, &draft("Batch-01", Vec::new())).expect("saved");

        save_draft(&location, &draft("batch-01", vec![Answer::new("q1")])).expect("saved again");

        let names: Vec<_> = fs::read_dir(location.drafts())
            .expect("readable")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(names, ["Batch-01.json"]);
    }

    #[test]
    fn an_unparsable_draft_is_an_error() {
        let (_scratch, location) = queue();
        fs::write(location.drafts().join("broken.json"), "{").expect("written");

        let error = load_draft(&location, "broken").expect_err("broken draft fails");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_draft_has_an_answer_when_anything_was_entered() {
        let selected = Answer {
            selected: vec!["a".into()],
            ..Answer::new("q")
        };
        let typed = Answer {
            custom: Some("text".into()),
            ..Answer::new("q")
        };
        let noted = Answer {
            note: Some("why".into()),
            ..Answer::new("q")
        };
        let blank = Answer {
            custom: Some("  ".into()),
            note: Some("".into()),
            ..Answer::new("q")
        };

        for answer in [selected, typed, noted] {
            assert!(
                draft_has_answer(&draft("d", vec![answer.clone()])),
                "{answer:?}"
            );
        }
        assert!(!draft_has_answer(&draft("d", vec![blank])));
        assert!(!draft_has_answer(&draft("d", Vec::new())));
    }

    #[test]
    fn an_answered_draft_blocks_and_an_unreadable_one_counts_as_answered() {
        let (_scratch, location) = queue();
        assert!(!has_answered_draft(&location, "none"));

        save_draft(&location, &draft("empty", Vec::new())).expect("saved");
        assert!(!has_answered_draft(&location, "empty"));

        let noted = Answer {
            note: Some("why".into()),
            ..Answer::new("q")
        };
        save_draft(&location, &draft("started", vec![noted])).expect("saved");
        assert!(has_answered_draft(&location, "started"));

        // Losing someone's answers is worse than refusing a replacement.
        fs::write(location.drafts().join("broken.json"), "{").expect("written");
        assert!(has_answered_draft(&location, "broken"));
    }
}
