// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drafts (spec section 4.3): the answers to a session while it is being
//! answered, in `drafts/<id>.json`.

use std::io;
use std::path::Path;

use super::atomic::write_atomically;
use super::{Gate, QueueLocation, Step, find_session_file, lock_drafts};
use crate::format::{Answer, SessionResult};

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

/// Saves `draft` while its session still waits in the inbox at
/// `session`, and says whether it did. A session that has left, such as one
/// withdrawn or taken back by hand, gets no draft, so none is left behind
/// for it. The check and the write run under the drafts lock (spec section
/// 4.3).
pub fn save_draft_while_waiting(
    location: &QueueLocation,
    session: &Path,
    draft: &SessionResult,
) -> io::Result<bool> {
    save_draft_while_waiting_with(location, session, draft, Gate::default())
}

/// [`save_draft_while_waiting`] with the lock taken and observed through
/// `gate`.
pub fn save_draft_while_waiting_with(
    location: &QueueLocation,
    session: &Path,
    draft: &SessionResult,
    mut gate: Gate<'_>,
) -> io::Result<bool> {
    let _lock = lock_drafts(location, gate.attempts)?;
    let waiting = session.try_exists()?;
    (gate.step)(Step::Checked);
    if !waiting {
        return Ok(false);
    }
    save_draft(location, draft)?;
    (gate.step)(Step::DraftWritten);
    Ok(true)
}

/// Removes the draft of `id`, if there is one.
pub fn delete_draft(location: &QueueLocation, id: &str) -> io::Result<()> {
    match find_session_file(&location.drafts(), id)? {
        Some(path) => std::fs::remove_file(path),
        None => Ok(()),
    }
}

/// Whether `answer` holds something the person entered: a selection they
/// made, typed text or a note that is not blank, or the `cleared` mark of
/// a question they changed and left empty (spec section 4.3). An
/// untouched default is the asker's preset, not the person's input. A
/// selection counts even where it does not answer the question yet (a
/// `multi` below `min`), since it is still work that a replacement would
/// throw away, and so is a cleared default, a cleared pick or text typed
/// and erased. A default picked again, a toggle or a note removes the
/// `defaulted` flag (section 4.2), so each of them counts.
pub fn person_entered(answer: &Answer) -> bool {
    (!answer.selected.is_empty() && !answer.defaulted)
        || filled(&answer.custom)
        || filled(&answer.note)
        || answer.cleared
}

/// Whether the person entered anything into `draft`: an answer that
/// [`person_entered`], or a note to the whole session. The reject reason
/// does not count (spec section 5.5).
pub fn draft_has_answer(draft: &SessionResult) -> bool {
    filled(&draft.note) || draft.answers.iter().any(person_entered)
}

fn filled(text: &Option<String>) -> bool {
    text.as_deref().is_some_and(|text| !text.trim().is_empty())
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
    fn a_draft_is_saved_under_the_drafts_lock_at_every_step() {
        let (_scratch, location) = queue();
        let session = location.inbox().join("batch-01.json");
        fs::write(&session, "{}").expect("written");
        let seen = std::cell::RefCell::new(Vec::new());

        let saved = save_draft_while_waiting_with(
            &location,
            &session,
            &draft("batch-01", vec![Answer::new("q1")]),
            crate::queue::recording_gate(&location, &seen),
        )
        .expect("saved");

        assert!(saved);
        assert_eq!(
            seen.into_inner(),
            [(Step::Checked, true), (Step::DraftWritten, true)]
        );
    }

    #[test]
    fn no_draft_is_saved_for_a_session_that_left() {
        let (_scratch, location) = queue();
        let gone = location.inbox().join("batch-01.json");

        let saved = save_draft_while_waiting(&location, &gone, &draft("batch-01", Vec::new()))
            .expect("no error");

        assert!(!saved);
        assert_eq!(load_draft(&location, "batch-01").expect("readable"), None);
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
    fn an_untouched_default_is_no_answer() {
        let defaulted = Answer {
            selected: vec!["a".into()],
            defaulted: true,
            ..Answer::new("q")
        };

        assert!(!draft_has_answer(&draft("d", vec![defaulted])));
    }

    #[test]
    fn a_cleared_question_counts_as_an_answer() {
        let cleared: Answer = serde_json::from_value(serde_json::json!({
            "question": "q",
            "cleared": true,
        }))
        .expect("a draft answer parses");

        assert!(draft_has_answer(&draft("d", vec![cleared])));
    }

    #[test]
    fn a_session_note_counts_and_a_reject_reason_does_not() {
        let noted = SessionResult {
            note: Some("approved if green".into()),
            ..draft("d", Vec::new())
        };
        let blank = SessionResult {
            note: Some("  ".into()),
            ..draft("d", Vec::new())
        };
        let rejecting = SessionResult {
            reason: Some("out of date".into()),
            ..draft("d", Vec::new())
        };

        assert!(draft_has_answer(&noted));
        assert!(!draft_has_answer(&blank));
        assert!(!draft_has_answer(&rejecting));
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
