// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Withdrawing a session (spec section 5.10): the asker takes back a
//! session it dropped into the inbox. The session leaves the queue the way
//! a finished one does, with a result of the status `withdrawn`, so every
//! `asqr wait` for it ends and `asqr result` returns the same later.

use std::io;
use std::path::Path;

use thiserror::Error;

use super::drop::{check_drop, write_drop};
use super::finish::{ArchivedSession, archive_session, finish_steps, stem};
use super::{
    ArchivedStanding, DropError, Dropped, FinishError, Gate, QueueLocation, Step,
    archived_standing, archived_title, delete_draft, find_session_file, has_answered_draft,
    load_draft, lock_drafts, person_entered, read_if_present, save_draft, session_sha256,
};
use crate::format::{
    CarriedFrom, SessionResult, Status, UnsentDraft, now_rfc3339, parse_session, rejected_answers,
    same_session_id,
};

/// How a withdrawal ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Withdrawn {
    /// The session waited and is withdrawn now, with this result.
    Now(SessionResult),
    /// An earlier withdrawal of the session left this result.
    Already(SessionResult),
}

#[derive(Debug, Error)]
pub enum WithdrawError {
    #[error("session {0:?} is being answered; withdraw it anyway with --force")]
    BeingAnswered(String),

    #[error("session {0:?} is answered already; read the result with `asqr result {0}`")]
    Answered(String),

    #[error(
        "an unread result of an earlier session {0:?} is in the outbox; the asqr watching the queue archives this session unanswered (`asqr result {0}` shows that result)"
    )]
    UnreadResult(String),

    #[error("the result of {0} is gone; its session is archived")]
    ResultGone(String),

    #[error("no session {0:?} in this queue")]
    Unknown(String),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Withdraws session `id` from the queue at `location`. A session whose
/// draft holds an answer is withdrawn only with `force`.
pub fn withdraw_session(
    location: &QueueLocation,
    id: &str,
    force: bool,
) -> Result<Withdrawn, WithdrawError> {
    withdraw_session_with(location, id, force, Gate::default())
}

/// [`withdraw_session`] with the lock taken and observed through `gate`.
pub fn withdraw_session_with(
    location: &QueueLocation,
    id: &str,
    force: bool,
    gate: Gate<'_>,
) -> Result<Withdrawn, WithdrawError> {
    withdraw_session_archived(location, id, force, gate).map(|(withdrawn, _)| withdrawn)
}

/// [`withdraw_session_with`], also returning the archived session when this
/// withdrawal moved it there, so the files it showed can be recorded once
/// the lock is let go of.
pub fn withdraw_session_archived(
    location: &QueueLocation,
    id: &str,
    force: bool,
    mut gate: Gate<'_>,
) -> Result<(Withdrawn, Option<ArchivedSession>), WithdrawError> {
    // Looking at a queue creates nothing: a queue that is not there holds
    // no session, and a mistyped queue name must not leave a directory.
    if !location.dir().is_dir() {
        return Err(WithdrawError::Unknown(id.to_owned()));
    }
    // From the first look at the files to the last write, so the TUI
    // cannot save an answer into the draft after the check.
    let _lock = lock_drafts(location, gate.attempts)?;
    withdraw_locked(location, id, force, &mut gate.step)
        .map(|taken| (taken.withdrawn, taken.archived))
}

/// What a withdrawal under the drafts lock took from the queue.
struct Taken {
    withdrawn: Withdrawn,
    archived: Option<ArchivedSession>,
    /// The draft file of the session as it was before the withdrawal
    /// deleted it, where there was a readable one.
    draft: Option<SessionResult>,
    /// The title of the withdrawn session, where its file was at hand.
    title: Option<String>,
}

impl Taken {
    /// A withdrawal that found the session gone already: nothing was
    /// archived, and no file is at hand to read a draft or a title from.
    fn without_files(withdrawn: Withdrawn) -> Self {
        Taken {
            withdrawn,
            archived: None,
            draft: None,
            title: None,
        }
    }
}

/// The title of the session file `bytes`, where they hold a session.
fn title_of(bytes: &[u8]) -> Option<String> {
    parse_session(bytes).ok()?.1.title
}

/// The withdrawal of [`withdraw_session_with`] once the drafts lock is
/// held, calling `step` after each of its steps.
fn withdraw_locked(
    location: &QueueLocation,
    id: &str,
    force: bool,
    step: &mut dyn FnMut(Step),
) -> Result<Taken, WithdrawError> {
    let Some(session) = find_session_file(&location.inbox(), id)? else {
        return gone(location, id).map(Taken::without_files);
    };
    let bytes = std::fs::read(&session)?;
    let sha256 = session_sha256(&bytes);

    // A result next to the waiting file is either the end of a finish or
    // a withdrawal that was interrupted after its first step, or one of an
    // earlier session under the same id.
    if let Some(path) = find_session_file(&location.outbox(), id)? {
        return match read_result(&path) {
            Some(result) if result.session_sha256.as_deref() == Some(sha256.as_str()) => {
                if result.status != Status::Withdrawn {
                    // The TUI's recovery completes the person's finish.
                    return Err(WithdrawError::Answered(id.to_owned()));
                }
                step(Step::Checked);
                // The interrupted withdrawal left the draft file, which
                // may hold more than the result's `draft` does.
                let draft = load_draft(location, id).ok().flatten();
                let archived = archive_session(location, &session, stem(&session))?.map(|path| {
                    ArchivedSession {
                        path,
                        bytes: bytes.clone(),
                    }
                });
                step(Step::Archived);
                delete_draft(location, id)?;
                step(Step::DraftDeleted);
                Ok(Taken {
                    withdrawn: Withdrawn::Already(result),
                    archived,
                    draft,
                    title: title_of(&bytes),
                })
            }
            _ => Err(WithdrawError::UnreadResult(id.to_owned())),
        };
    }

    if !force && has_answered_draft(location, id) {
        return Err(WithdrawError::BeingAnswered(id.to_owned()));
    }
    // Read before `finish_steps` deletes it: the replacement of a session
    // takes its draft over from here.
    let draft = load_draft(location, id).ok().flatten();
    let result = SessionResult::withdrawn(
        stem(&session),
        &sha256,
        &now_rfc3339(),
        unsent_draft(draft.as_ref(), &bytes),
    );
    step(Step::Checked);
    match finish_steps(location, &session, &result, step) {
        Ok(archived) => {
            tracing::info!(id, forced = force, "withdrew a session");
            let title = title_of(&bytes);
            let archived = archived.map(|path| ArchivedSession { path, bytes });
            Ok(Taken {
                withdrawn: Withdrawn::Now(result),
                archived,
                draft,
                title,
            })
        }
        // Another result landed after the check, from a writer that does
        // not take the lock; it decides what happened to the session.
        Err(FinishError::Conflict(_)) => gone(location, id).map(Taken::without_files),
        Err(FinishError::Io(error)) => Err(error.into()),
    }
}

// =========================================================================
// Replacing a session under another id
// =========================================================================

/// Why `ask --replaces` did not complete (spec section 5.5).
#[derive(Debug, Error)]
pub enum ReplaceError {
    /// The new session was refused; nothing changed.
    #[error(transparent)]
    Drop(DropError),

    /// The old session was not withdrawn; nothing changed.
    #[error(transparent)]
    Withdraw(WithdrawError),

    /// The old session is withdrawn, but the new one could not be dropped.
    #[error("{error}; {old} was withdrawn already, so the same command again drops the new one")]
    DropAfterWithdrawal { old: String, error: DropError },
}

/// Withdraws session `old` and drops `bytes` as session `id` in its place,
/// both under one hold of the drafts lock. Every refusal of either comes
/// before the first change, so a refused replacement leaves the queue as
/// it was. `force` stands for both: it withdraws `old` while the person is
/// answering it, and moves an unread result of `id` aside.
pub fn drop_replacing(
    location: &QueueLocation,
    old: &str,
    id: &str,
    bytes: &[u8],
    force: bool,
    gate: Gate<'_>,
) -> Result<(Withdrawn, Dropped), ReplaceError> {
    drop_replacing_archived(location, old, id, bytes, force, gate)
        .map(|(withdrawn, dropped, _)| (withdrawn, dropped))
}

/// [`drop_replacing`], also returning the archived session of `old` when
/// this call moved it there.
pub fn drop_replacing_archived(
    location: &QueueLocation,
    old: &str,
    id: &str,
    bytes: &[u8],
    force: bool,
    gate: Gate<'_>,
) -> Result<(Withdrawn, Dropped, Option<ArchivedSession>), ReplaceError> {
    drop_replacing_carrying(location, old, id, bytes, force, None, gate)
        .map(|replaced| (replaced.withdrawn, replaced.dropped, replaced.archived))
}

/// Makes the draft of the new session out of the draft of the one it
/// replaces, or none where nothing of it fits (spec section 4.3).
pub type Carry<'a> = &'a dyn Fn(&SessionResult) -> Option<SessionResult>;

/// How an `ask --replaces` ended.
#[derive(Debug)]
pub struct Replaced {
    pub withdrawn: Withdrawn,
    pub dropped: Dropped,
    /// The archived session of the old one, where this call moved it.
    pub archived: Option<ArchivedSession>,
    /// Whether a draft was written for the new session.
    pub carried: bool,
}

/// [`drop_replacing_archived`] that also writes the new session a draft
/// made by `carry` out of the draft of the old one (spec section 5.5),
/// before the session file itself, so the TUI finds the draft when the
/// session arrives.
pub fn drop_replacing_carrying(
    location: &QueueLocation,
    old: &str,
    id: &str,
    bytes: &[u8],
    force: bool,
    carry: Option<Carry<'_>>,
    mut gate: Gate<'_>,
) -> Result<Replaced, ReplaceError> {
    let refused = |error| ReplaceError::Drop(DropError::Io(error));
    // Made under the drafts lock, as `drop_session_with` makes them.
    let _lock = lock_drafts(location, gate.attempts).map_err(refused)?;
    location.create_layout().map_err(refused)?;
    let checked = check_drop(location, id, force).map_err(ReplaceError::Drop)?;
    // A waiting session of the new id is replaced in place, and a running
    // TUI rebuilds it from its own state, not from the draft file. An
    // answered draft of the new id belongs to a session of that id the
    // person started on before (spec section 5.4), so it stays, unless
    // an earlier run of this very replacement wrote it and ended before
    // the session file: that run's draft is the one to keep.
    let may_carry_at_all = carry.is_some() && !checked.waits();
    let resumed = may_carry_at_all
        && load_draft(location, id)
            .ok()
            .flatten()
            .and_then(|existing| existing.carried_from)
            .is_some_and(|from| same_session_id(&from.id, old));
    let may_carry = may_carry_at_all && !resumed && !has_answered_draft(location, id);
    let taken =
        withdraw_locked(location, old, force, &mut gate.step).map_err(ReplaceError::Withdraw)?;

    let mut carried = resumed;
    let mut wrote = false;
    if let (true, Some(carry)) = (may_carry, carry) {
        let source = draft_to_carry(location, old, &taken);
        if let Some(mut draft) = source.as_ref().and_then(carry) {
            draft.carried_from = Some(CarriedFrom {
                id: old.to_owned(),
                title: taken
                    .title
                    .clone()
                    .or_else(|| archived_title(location, old).ok().flatten()),
            });
            // A failed write costs the carry and nothing else: the old
            // session is withdrawn already, and its result holds the draft.
            match save_draft(location, &draft) {
                Ok(()) => {
                    (gate.step)(Step::DraftWritten);
                    carried = true;
                    wrote = true;
                }
                Err(error) => tracing::warn!(id, old, %error, "cannot write the carried draft"),
            }
        }
    }

    let dropped = write_drop(location, id, bytes, checked, &mut gate.step).map_err(|error| {
        // No draft is left for a session that never arrived.
        if wrote {
            let _ = delete_draft(location, id);
        }
        ReplaceError::DropAfterWithdrawal {
            old: old.to_owned(),
            error,
        }
    })?;
    Ok(Replaced {
        withdrawn: taken.withdrawn,
        dropped,
        archived: taken.archived,
        carried,
    })
}

/// The draft of the withdrawn session that the replacement takes over: the
/// draft file where the withdrawal read one, else, for a session withdrawn
/// before, what its result's `draft` holds. That holds less: no cleared
/// question, no own text that was unpicked, no selection below `min`
/// without a note, and no own text next to a pick. A session whose draft a
/// replacement carried already is replaced once only: a result's `draft`
/// may be stale by now.
fn draft_to_carry(location: &QueueLocation, old: &str, taken: &Taken) -> Option<SessionResult> {
    if let Some(draft) = &taken.draft {
        return Some(draft.clone());
    }
    let Withdrawn::Already(result) = &taken.withdrawn else {
        return None;
    };
    if carried_from_exists(location, old) {
        return None;
    }
    let unsent = result.draft.as_ref()?;
    let mut draft = SessionResult::draft(old, "", unsent.answers.clone());
    draft.note = unsent.note.clone();
    Some(draft)
}

/// Whether a draft in the queue continues the draft of session `old`.
fn carried_from_exists(location: &QueueLocation, old: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(location.drafts()) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<SessionResult>(&bytes).ok())
        .filter_map(|draft| draft.carried_from)
        .any(|from| same_session_id(&from.id, old))
}

/// Where a session stands whose file is not in the inbox, as far as a
/// withdrawal is concerned: withdrawn before, answered, or gone. The
/// result may have left the outbox for a month bundle.
fn gone(location: &QueueLocation, id: &str) -> Result<Withdrawn, WithdrawError> {
    let answered = |bytes: Option<&[u8]>| match bytes
        .and_then(|bytes| serde_json::from_slice::<SessionResult>(bytes).ok())
    {
        Some(result) if result.status == Status::Withdrawn => Ok(Withdrawn::Already(result)),
        _ => Err(WithdrawError::Answered(id.to_owned())),
    };
    if let Some(path) = find_session_file(&location.outbox(), id)? {
        match read_if_present(&path) {
            Ok(Some(bytes)) => return answered(Some(&bytes)),
            // Moved into a bundle since the listing: the archive has it.
            Ok(None) => {}
            Err(_) => return answered(None),
        }
    }
    match archived_standing(location, id).map_err(io::Error::other)? {
        ArchivedStanding::Result(bytes) => answered(Some(&bytes)),
        ArchivedStanding::Gone => Err(WithdrawError::ResultGone(id.to_owned())),
        ArchivedStanding::Unknown => Err(WithdrawError::Unknown(id.to_owned())),
    }
}

/// The result at `path`, or `None` when it cannot be read as one.
fn read_result(path: &Path) -> Option<SessionResult> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// What the person entered into session `id` and never sent, in the shape
/// of a rejected result: the questions they answered or noted, and the
/// note to the whole session, trimmed. A rejected result keeps an
/// untouched default, flagged; here it goes, as it is the asker's own
/// preset and nothing the person entered (spec section 4.1). A question
/// marked `cleared` goes too: it is skipped without a note, which the
/// shape of a rejected result leaves out. None
/// without a readable draft, for a session file that is no valid session,
/// and for a draft that holds nothing else the person entered.
fn unsent_draft(draft: Option<&SessionResult>, bytes: &[u8]) -> Option<UnsentDraft> {
    let draft = draft?;
    let (_, session) = parse_session(bytes).ok()?;
    let note = draft
        .note
        .as_deref()
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .map(str::to_owned);
    let answers: Vec<_> = rejected_answers(&session, &draft.answers)
        .into_iter()
        .filter(person_entered)
        .collect();
    (note.is_some() || !answers.is_empty()).then_some(UnsentDraft { note, answers })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::format::{Answer, Status, UnsentDraft};
    use crate::queue::{
        Archived, Step, archive_name, drop_session, finish, load_draft, lock_drafts,
        parse_archive_name, recording_gate, save_draft, session_sha256,
    };

    const SESSION: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}]}"#;

    const AT: &str = "2026-10-28T10:00:00+01:00";

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        location.create_layout().expect("layout");
        (scratch, location)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .expect("readable")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    fn asked(location: &QueueLocation, id: &str) {
        drop_session(location, id, SESSION.as_bytes(), false).expect("dropped");
    }

    /// A draft of `id` in which the person picked `b` and wrote notes.
    fn started(location: &QueueLocation, id: &str) {
        save_draft(location, &started_draft(id)).expect("saved");
    }

    fn started_draft(id: &str) -> SessionResult {
        let mut draft = SessionResult::draft(
            id,
            "q",
            vec![Answer {
                selected: vec!["b".into()],
                note: Some("why b".into()),
                ..Answer::new("q")
            }],
        );
        draft.note = Some("  first thoughts  ".into());
        draft
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

    fn sha() -> String {
        session_sha256(SESSION.as_bytes())
    }

    /// A gate that writes `result` as the outbox result of `batch-01` right
    /// after the check, as a writer would that does not take the lock,
    /// such as the TUI of an earlier asqr.
    fn racing(location: &QueueLocation, result: SessionResult) -> Gate<'static> {
        let outbox = location.outbox();
        Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::Checked {
                    fs::write(
                        outbox.join("batch-01.json"),
                        serde_json::to_vec_pretty(&result).expect("serializes"),
                    )
                    .expect("written");
                }
            }),
        }
    }

    #[test]
    fn withdraws_a_waiting_session_nobody_has_answered() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        save_draft(
            &location,
            &SessionResult::draft("batch-01", "q", Vec::new()),
        )
        .expect("saved");

        let withdrawn = withdraw_session(&location, "batch-01", false).expect("withdrawn");

        let Withdrawn::Now(result) = withdrawn else {
            panic!("withdrawn now: {withdrawn:?}");
        };
        assert_eq!(
            (
                result.status,
                result.session_sha256.clone(),
                result.draft.clone()
            ),
            (Status::Withdrawn, Some(sha()), None)
        );
        assert_eq!(outbox_result(&location, "batch-01"), result);
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        let archived = names(&location.archive());
        let entry = parse_archive_name(&archived[0]).expect("archive name parses");
        assert_eq!(
            (entry.id.as_str(), entry.kind),
            ("batch-01", Archived::Session)
        );
        assert_eq!(load_draft(&location, "batch-01").expect("readable"), None);
    }

    #[test]
    fn keeps_the_spelling_of_the_inbox_file_in_the_result() {
        let (_scratch, location) = queue();
        asked(&location, "Batch-01");

        let withdrawn = withdraw_session(&location, "batch-01", false).expect("withdrawn");

        let Withdrawn::Now(result) = withdrawn else {
            panic!("withdrawn now: {withdrawn:?}");
        };
        assert_eq!(result.id, "Batch-01");
        assert_eq!(names(&location.outbox()), ["Batch-01.json"]);
    }

    #[test]
    fn refuses_while_the_draft_holds_an_answer() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        started(&location, "batch-01");

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(&refused, Err(WithdrawError::BeingAnswered(id)) if id == "batch-01"),
            "{refused:?}"
        );
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
        assert!(
            load_draft(&location, "batch-01")
                .expect("readable")
                .is_some()
        );
    }

    /// A draft of `id` in which the person only cleared the default of
    /// `q`.
    fn cleared_default(location: &QueueLocation, id: &str) {
        let cleared = Answer {
            cleared: true,
            ..Answer::new("q")
        };
        save_draft(location, &SessionResult::draft(id, "q", vec![cleared])).expect("saved");
    }

    #[test]
    fn refuses_while_the_draft_holds_a_cleared_default() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        cleared_default(&location, "batch-01");

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(&refused, Err(WithdrawError::BeingAnswered(id)) if id == "batch-01"),
            "{refused:?}"
        );
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
    }

    #[test]
    fn refuses_while_the_draft_cannot_be_read() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        fs::write(location.drafts().join("batch-01.json"), "{").expect("written");

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(refused, Err(WithdrawError::BeingAnswered(_))),
            "{refused:?}"
        );
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
    }

    #[test]
    fn force_withdraws_a_session_being_answered_with_its_unsent_draft() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        started(&location, "batch-01");

        let withdrawn = withdraw_session(&location, "batch-01", true).expect("withdrawn");

        let Withdrawn::Now(result) = withdrawn else {
            panic!("withdrawn now: {withdrawn:?}");
        };
        assert_eq!(result.answers, Vec::<Answer>::new(), "no answers, a draft");
        assert_eq!(
            result.draft,
            Some(UnsentDraft {
                note: Some("first thoughts".into()),
                answers: vec![Answer {
                    selected: vec!["b".into()],
                    note: Some("why b".into()),
                    ..Answer::new("q")
                }],
            })
        );
        assert_eq!(load_draft(&location, "batch-01").expect("readable"), None);
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
    }

    #[test]
    fn force_withdraws_a_session_whose_draft_cannot_be_read_without_a_draft() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        fs::write(location.drafts().join("batch-01.json"), "{").expect("written");

        let withdrawn = withdraw_session(&location, "batch-01", true).expect("withdrawn");

        assert!(
            matches!(&withdrawn, Withdrawn::Now(result) if result.draft.is_none()),
            "{withdrawn:?}"
        );
    }

    #[test]
    fn an_invalid_session_is_withdrawn_without_a_draft() {
        let (_scratch, location) = queue();
        fs::write(location.inbox().join("broken.json"), "{").expect("written");
        started(&location, "broken");

        let withdrawn = withdraw_session(&location, "broken", true).expect("withdrawn");

        assert!(
            matches!(&withdrawn, Withdrawn::Now(result) if result.draft.is_none()),
            "{withdrawn:?}"
        );
    }

    #[test]
    fn a_blank_draft_leaves_no_draft_in_the_result() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        let mut blank = SessionResult::draft("batch-01", "q", vec![Answer::new("q")]);
        blank.note = Some("   ".into());
        save_draft(&location, &blank).expect("saved");

        let withdrawn = withdraw_session(&location, "batch-01", false).expect("withdrawn");

        assert!(
            matches!(&withdrawn, Withdrawn::Now(result) if result.draft.is_none()),
            "{withdrawn:?}"
        );
    }

    #[test]
    fn a_forced_withdrawal_leaves_an_untouched_default_out_of_the_draft() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        let browsed = SessionResult::draft(
            "batch-01",
            "q",
            vec![Answer {
                selected: vec!["a".into()],
                defaulted: true,
                ..Answer::new("q")
            }],
        );
        save_draft(&location, &browsed).expect("saved");

        let withdrawn = withdraw_session(&location, "batch-01", true).expect("withdrawn");

        assert!(
            matches!(&withdrawn, Withdrawn::Now(result) if result.draft.is_none()),
            "{withdrawn:?}"
        );
    }

    #[test]
    fn a_second_withdrawal_reports_the_first() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        let first = withdraw_session(&location, "batch-01", false).expect("first");
        let Withdrawn::Now(first) = first else {
            panic!("the first withdrawal withdraws: {first:?}");
        };

        let second = withdraw_session(&location, "BATCH-01", false).expect("second");

        assert_eq!(second, Withdrawn::Already(first));
    }

    #[test]
    fn completes_an_interrupted_withdrawal() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        save_draft(
            &location,
            &SessionResult::draft("batch-01", "q", Vec::new()),
        )
        .expect("saved");
        let earlier = SessionResult::withdrawn("batch-01", &sha(), AT, None);
        write_result(&location, "batch-01", &earlier);
        let seen = RefCell::new(Vec::new());

        let withdrawn = withdraw_session_with(
            &location,
            "batch-01",
            false,
            recording_gate(&location, &seen),
        )
        .expect("completed");

        assert_eq!(withdrawn, Withdrawn::Already(earlier));
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        assert_eq!(load_draft(&location, "batch-01").expect("readable"), None);
        assert_eq!(
            seen.into_inner(),
            [
                (Step::Checked, true),
                (Step::Archived, true),
                (Step::DraftDeleted, true)
            ]
        );
    }

    #[test]
    fn refuses_a_session_answered_already() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        let path = location.inbox().join("batch-01.json");
        let submitted = SessionResult::submitted("batch-01", &sha(), AT, None, Vec::new());
        finish(&location, &path, &submitted).expect("finished");

        let refused = withdraw_session(&location, "batch-01", true);

        assert!(
            matches!(&refused, Err(WithdrawError::Answered(id)) if id == "batch-01"),
            "{refused:?}"
        );
        assert_eq!(outbox_result(&location, "batch-01"), submitted);
    }

    #[test]
    fn an_outbox_entry_that_cannot_be_read_counts_as_answered() {
        let (_scratch, location) = queue();
        // A directory in the place of the result file.
        fs::create_dir(location.outbox().join("batch-01.json")).expect("created");

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(refused, Err(WithdrawError::Answered(_))),
            "{refused:?}"
        );
    }

    #[test]
    fn refuses_while_an_interrupted_finish_waits() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        write_result(
            &location,
            "batch-01",
            &SessionResult::submitted("batch-01", &sha(), AT, None, Vec::new()),
        );

        let refused = withdraw_session(&location, "batch-01", true);

        assert!(
            matches!(refused, Err(WithdrawError::Answered(_))),
            "{refused:?}"
        );
        assert_eq!(
            names(&location.inbox()),
            ["batch-01.json"],
            "left to recovery"
        );
    }

    #[test]
    fn refuses_next_to_an_unread_result_of_an_earlier_session() {
        let (_scratch, location) = queue();
        let earlier = SessionResult::withdrawn("batch-01", "other bytes", AT, None);
        write_result(&location, "batch-01", &earlier);
        // Dropped by hand, so no ask refused it.
        fs::write(location.inbox().join("batch-01.json"), SESSION).expect("written");

        let refused = withdraw_session(&location, "batch-01", true);

        assert!(
            matches!(&refused, Err(WithdrawError::UnreadResult(id)) if id == "batch-01"),
            "{refused:?}"
        );
        assert_eq!(outbox_result(&location, "batch-01"), earlier);
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
    }

    #[test]
    fn an_unreadable_result_next_to_a_waiting_session_is_an_earlier_one() {
        let (_scratch, location) = queue();
        fs::write(location.outbox().join("batch-01.json"), "{").expect("written");
        fs::write(location.inbox().join("batch-01.json"), SESSION).expect("written");

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(refused, Err(WithdrawError::UnreadResult(_))),
            "{refused:?}"
        );
    }

    #[test]
    fn an_unreadable_result_without_a_session_counts_as_answered() {
        let (_scratch, location) = queue();
        fs::write(location.outbox().join("batch-01.json"), "{").expect("written");

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(refused, Err(WithdrawError::Answered(_))),
            "{refused:?}"
        );
    }

    #[test]
    fn an_archived_session_without_its_result_is_gone() {
        let (_scratch, location) = queue();
        let name = archive_name(
            "batch-01",
            ulid::Ulid::from_parts(1_000, 0),
            Archived::Session,
        );
        fs::write(location.archive().join(name), SESSION).expect("written");

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(&refused, Err(WithdrawError::ResultGone(id)) if id == "batch-01"),
            "{refused:?}"
        );
    }

    #[test]
    fn an_unknown_id_is_unknown() {
        let (_scratch, location) = queue();

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(&refused, Err(WithdrawError::Unknown(id)) if id == "batch-01"),
            "{refused:?}"
        );
    }

    #[test]
    fn a_queue_that_does_not_exist_is_left_uncreated() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("never-used"));

        let refused = withdraw_session(&location, "batch-01", false);

        assert!(
            matches!(refused, Err(WithdrawError::Unknown(_))),
            "{refused:?}"
        );
        assert!(!location.dir().exists());
    }

    #[test]
    fn a_finish_that_won_the_race_counts_as_answered() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        let submitted = SessionResult::submitted("batch-01", &sha(), AT, None, Vec::new());

        let refused = withdraw_session_with(
            &location,
            "batch-01",
            false,
            racing(&location, submitted.clone()),
        );

        assert!(
            matches!(refused, Err(WithdrawError::Answered(_))),
            "{refused:?}"
        );
        assert_eq!(outbox_result(&location, "batch-01"), submitted);
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
    }

    #[test]
    fn a_withdrawal_that_won_the_race_counts_as_already() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        let earlier = SessionResult::withdrawn("batch-01", &sha(), AT, None);

        let withdrawn = withdraw_session_with(
            &location,
            "batch-01",
            false,
            racing(&location, earlier.clone()),
        );

        assert_eq!(withdrawn.expect("already"), Withdrawn::Already(earlier));
    }

    #[test]
    fn a_withdrawal_holds_the_drafts_lock_at_every_step() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        let seen = RefCell::new(Vec::new());

        withdraw_session_with(
            &location,
            "batch-01",
            false,
            recording_gate(&location, &seen),
        )
        .expect("withdrawn");

        assert_eq!(
            seen.into_inner(),
            [
                (Step::Checked, true),
                (Step::ResultWritten, true),
                (Step::Archived, true),
                (Step::DraftDeleted, true)
            ]
        );
    }

    #[test]
    fn a_withdrawal_while_the_drafts_lock_is_busy_is_refused() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        let _held = lock_drafts(&location, 1).expect("locks");
        let gate = Gate {
            attempts: 1,
            step: Box::new(|step| panic!("no step while the lock is busy: {step:?}")),
        };

        let busy = withdraw_session_with(&location, "batch-01", false, gate);

        assert!(
            matches!(&busy, Err(WithdrawError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock),
            "{busy:?}"
        );
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
    }

    #[test]
    fn an_outbox_that_cannot_be_written_is_an_io_error() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        fs::remove_dir(location.outbox()).expect("removed");
        fs::write(location.outbox(), "in the way").expect("written");

        let failed = withdraw_session(&location, "batch-01", false);

        assert!(matches!(failed, Err(WithdrawError::Io(_))), "{failed:?}");
        assert_eq!(names(&location.inbox()), ["batch-01.json"]);
    }

    #[test]
    fn a_withdrawal_stopped_after_its_result_is_completed_by_the_next() {
        let (_scratch, location) = queue();
        asked(&location, "batch-01");
        fs::remove_dir(location.archive()).expect("removed");
        fs::write(location.archive(), "in the way").expect("written");

        let failed = withdraw_session(&location, "batch-01", false);

        assert!(matches!(failed, Err(WithdrawError::Io(_))), "{failed:?}");
        let written = outbox_result(&location, "batch-01");
        assert_eq!(written.status, Status::Withdrawn);

        fs::remove_file(location.archive()).expect("removed");
        fs::create_dir(location.archive()).expect("created");
        let completed = withdraw_session(&location, "batch-01", false).expect("completed");
        assert_eq!(completed, Withdrawn::Already(written));
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
    }

    // ---------------------------------------------------------------------
    // Replacing a session under another id
    // ---------------------------------------------------------------------

    const NEW: &[u8] = b"{\"asqr\": 2}";

    #[test]
    fn a_replacement_withdraws_the_old_and_drops_the_new_under_one_lock() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        let seen = RefCell::new(Vec::new());

        let (withdrawn, dropped) = drop_replacing(
            &location,
            "round-1",
            "round-2",
            NEW,
            false,
            recording_gate(&location, &seen),
        )
        .expect("replaced");

        assert!(matches!(withdrawn, Withdrawn::Now(_)), "{withdrawn:?}");
        assert_eq!(dropped, Dropped::New(location.inbox().join("round-2.json")));
        assert_eq!(names(&location.inbox()), ["round-2.json"]);
        assert_eq!(
            outbox_result(&location, "round-1").status,
            Status::Withdrawn
        );
        assert_eq!(
            seen.into_inner(),
            [
                (Step::Checked, true),
                (Step::ResultWritten, true),
                (Step::Archived, true),
                (Step::DraftDeleted, true),
                (Step::SessionWritten, true)
            ]
        );
    }

    #[test]
    fn a_refused_withdrawal_drops_nothing() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");

        let refused = drop_replacing(&location, "round-1", "round-2", NEW, false, Gate::default());

        assert!(
            matches!(
                refused,
                Err(ReplaceError::Withdraw(WithdrawError::BeingAnswered(_)))
            ),
            "{refused:?}"
        );
        assert_eq!(names(&location.inbox()), ["round-1.json"]);
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
    }

    /// The carry of a test: the old draft under the id `round-2`.
    fn as_round_two(old: &SessionResult) -> Option<SessionResult> {
        Some(SessionResult {
            id: "round-2".into(),
            ..old.clone()
        })
    }

    fn replace_carrying(
        location: &QueueLocation,
        carry: Carry<'_>,
        gate: Gate<'_>,
    ) -> Result<Replaced, ReplaceError> {
        drop_replacing_carrying(location, "round-1", "round-2", NEW, true, Some(carry), gate)
    }

    #[test]
    fn the_carried_draft_is_written_before_the_session_under_the_lock() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");
        let seen = RefCell::new(Vec::new());
        let gate = Gate {
            attempts: 1,
            step: Box::new(|step| {
                seen.borrow_mut().push((
                    step,
                    crate::queue::drafts_lock_is_held(&location),
                    location.inbox().join("round-2.json").exists(),
                    location.drafts().join("round-2.json").exists(),
                ));
            }),
        };

        let replaced = replace_carrying(&location, &as_round_two, gate).expect("replaced");

        assert!(replaced.carried);
        assert_eq!(
            seen.into_inner(),
            [
                (Step::Checked, true, false, false),
                (Step::ResultWritten, true, false, false),
                (Step::Archived, true, false, false),
                (Step::DraftDeleted, true, false, false),
                (Step::DraftWritten, true, false, true),
                (Step::SessionWritten, true, true, true),
            ]
        );
    }

    #[test]
    fn the_carried_draft_names_where_it_comes_from() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");

        replace_carrying(&location, &as_round_two, Gate::default()).expect("replaced");

        let draft = load_draft(&location, "round-2")
            .expect("readable")
            .expect("carried");
        assert_eq!(
            draft.carried_from,
            Some(CarriedFrom {
                id: "round-1".into(),
                title: None
            })
        );
    }

    #[test]
    fn nothing_is_written_where_the_carry_finds_nothing_to_carry() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");

        let replaced = replace_carrying(&location, &|_| None, Gate::default()).expect("replaced");

        assert!(!replaced.carried);
        assert_eq!(load_draft(&location, "round-2").expect("readable"), None);
    }

    #[test]
    fn a_drop_that_fails_after_the_carry_takes_the_carried_draft_with_it() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");
        let inbox = location.inbox();
        // A file dropped by hand at the moment the draft is written.
        let gate = Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::DraftWritten {
                    fs::write(inbox.join("round-2.json"), "{}").expect("written");
                }
            }),
        };

        let failed = replace_carrying(&location, &as_round_two, gate);

        assert!(
            matches!(failed, Err(ReplaceError::DropAfterWithdrawal { .. })),
            "{failed:?}"
        );
        assert_eq!(load_draft(&location, "round-2").expect("readable"), None);
    }

    #[test]
    fn a_drop_that_fails_without_a_carry_leaves_the_draft_of_the_new_id() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");
        let unanswered = SessionResult::draft("round-2", "q", Vec::new());
        save_draft(&location, &unanswered).expect("saved");
        let inbox = location.inbox();
        let gate = Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::DraftDeleted {
                    fs::write(inbox.join("round-2.json"), "{}").expect("written");
                }
            }),
        };

        let failed = replace_carrying(&location, &|_| None, gate);

        assert!(
            matches!(failed, Err(ReplaceError::DropAfterWithdrawal { .. })),
            "{failed:?}"
        );
        assert_eq!(
            load_draft(&location, "round-2").expect("readable"),
            Some(unanswered)
        );
    }

    #[test]
    fn a_carried_draft_that_cannot_be_written_costs_the_carry_only() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");
        let drafts = location.drafts();
        // A directory takes the place of the file after the checks, so
        // only the write of the draft fails.
        let gate = Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::DraftDeleted {
                    fs::create_dir(drafts.join("round-2.json")).expect("directory");
                }
            }),
        };

        let replaced = replace_carrying(&location, &as_round_two, gate).expect("replaced");

        assert!(!replaced.carried);
        assert_eq!(names(&location.inbox()), ["round-2.json"]);
        assert_eq!(
            outbox_result(&location, "round-1").status,
            Status::Withdrawn
        );
    }

    #[test]
    fn a_withdrawal_interrupted_after_its_result_still_hands_over_the_whole_draft() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        cleared_default(&location, "round-1");
        write_result(
            &location,
            "round-1",
            &SessionResult::withdrawn("round-1", &sha(), AT, None),
        );

        let replaced =
            replace_carrying(&location, &as_round_two, Gate::default()).expect("replaced");

        assert!(matches!(replaced.withdrawn, Withdrawn::Already(_)));
        let draft = load_draft(&location, "round-2")
            .expect("readable")
            .expect("carried");
        assert!(draft.answers[0].cleared, "{draft:?}");
    }

    #[test]
    fn a_session_withdrawn_long_ago_hands_over_what_its_result_holds() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");
        withdraw_session(&location, "round-1", true).expect("withdrawn");

        replace_carrying(&location, &as_round_two, Gate::default()).expect("replaced");

        let draft = load_draft(&location, "round-2")
            .expect("readable")
            .expect("carried");
        assert_eq!(draft.answers[0].selected, ["b"]);
        assert_eq!(draft.note.as_deref(), Some("first thoughts"));
    }

    #[test]
    fn the_rerun_after_a_crash_keeps_the_draft_the_first_run_carried() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");
        let inbox = location.inbox();
        // The first run ends right after the draft is written.
        let gate = Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::DraftWritten {
                    fs::write(inbox.join("round-2.json"), "{}").expect("written");
                }
            }),
        };
        replace_carrying(&location, &as_round_two, gate).expect_err("the drop fails");
        let carried = SessionResult {
            id: "round-2".into(),
            carried_from: Some(CarriedFrom {
                id: "round-1".into(),
                title: None,
            }),
            ..started_draft("round-1")
        };
        save_draft(&location, &carried).expect("the crash left the draft");
        fs::remove_file(location.inbox().join("round-2.json")).expect("removed");

        let replaced = replace_carrying(&location, &as_round_two, Gate::default()).expect("again");

        assert!(replaced.carried);
        assert_eq!(
            load_draft(&location, "round-2").expect("readable"),
            Some(carried)
        );
    }

    #[test]
    fn no_drafts_directory_holds_no_continuing_draft() {
        let (_scratch, location) = queue();
        fs::remove_dir(location.drafts()).expect("removed");

        assert!(!carried_from_exists(&location, "round-1"));
    }

    #[test]
    fn a_session_withdrawn_once_is_not_carried_into_a_second_replacement() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        started(&location, "round-1");
        replace_carrying(&location, &as_round_two, Gate::default()).expect("replaced");

        let again = drop_replacing_carrying(
            &location,
            "round-1",
            "round-3",
            NEW,
            false,
            Some(&as_round_two),
            Gate::default(),
        )
        .expect("replaced");

        assert!(!again.carried);
        assert_eq!(load_draft(&location, "round-3").expect("readable"), None);
    }

    #[test]
    fn a_session_that_waits_under_the_new_id_gets_no_carried_draft() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        asked(&location, "round-2");
        started(&location, "round-1");

        let replaced =
            replace_carrying(&location, &as_round_two, Gate::default()).expect("replaced");

        assert!(!replaced.carried);
        assert_eq!(load_draft(&location, "round-2").expect("readable"), None);
    }

    #[test]
    fn a_replacement_is_refused_while_the_old_draft_holds_a_cleared_default() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        cleared_default(&location, "round-1");

        let refused = drop_replacing(&location, "round-1", "round-2", NEW, false, Gate::default());

        assert!(
            matches!(
                refused,
                Err(ReplaceError::Withdraw(WithdrawError::BeingAnswered(_)))
            ),
            "{refused:?}"
        );
        assert_eq!(names(&location.inbox()), ["round-1.json"]);
    }

    #[test]
    fn a_refused_new_session_withdraws_nothing() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        fs::write(location.outbox().join("round-2.json"), "unread").expect("written");

        let refused = drop_replacing(&location, "round-1", "round-2", NEW, false, Gate::default());

        assert!(
            matches!(refused, Err(ReplaceError::Drop(DropError::UnreadResult(_)))),
            "{refused:?}"
        );
        assert_eq!(names(&location.inbox()), ["round-1.json"]);
        assert_eq!(names(&location.outbox()), ["round-2.json"]);
    }

    #[test]
    fn a_drop_that_loses_its_race_after_the_withdrawal_says_so() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        // A file dropped by hand under the new id between the withdrawal
        // and the drop, the one writer that does not take the lock.
        let inbox = location.inbox();
        let gate = Gate {
            attempts: 1,
            step: Box::new(move |step| {
                if step == Step::DraftDeleted {
                    fs::write(inbox.join("round-2.json"), "by hand").expect("written");
                }
            }),
        };

        let failed = drop_replacing(&location, "round-1", "round-2", NEW, false, gate);

        let Err(error @ ReplaceError::DropAfterWithdrawal { .. }) = failed else {
            panic!("the drop fails after the withdrawal: {failed:?}");
        };
        assert_eq!(
            error.to_string(),
            "another session \"round-2\" was dropped at the same moment; round-1 was withdrawn already, so the same command again drops the new one"
        );
        assert_eq!(
            outbox_result(&location, "round-1").status,
            Status::Withdrawn
        );
    }

    #[test]
    fn a_replacement_while_the_drafts_lock_is_busy_changes_nothing() {
        let (_scratch, location) = queue();
        asked(&location, "round-1");
        let _held = lock_drafts(&location, 1).expect("locks");
        let gate = Gate {
            attempts: 1,
            step: Box::new(|step| panic!("no step while the lock is busy: {step:?}")),
        };

        let busy = drop_replacing(&location, "round-1", "round-2", NEW, false, gate);

        assert!(
            matches!(&busy, Err(ReplaceError::Drop(DropError::Io(error))) if error.kind() == io::ErrorKind::WouldBlock),
            "{busy:?}"
        );
        assert_eq!(names(&location.inbox()), ["round-1.json"]);
    }

    #[test]
    fn errors_name_what_to_do() {
        assert_eq!(
            WithdrawError::BeingAnswered("b".into()).to_string(),
            "session \"b\" is being answered; withdraw it anyway with --force"
        );
        assert_eq!(
            WithdrawError::Answered("b".into()).to_string(),
            "session \"b\" is answered already; read the result with `asqr result b`"
        );
        assert_eq!(
            WithdrawError::UnreadResult("b".into()).to_string(),
            "an unread result of an earlier session \"b\" is in the outbox; the asqr watching the queue archives this session unanswered (`asqr result b` shows that result)"
        );
        assert_eq!(
            WithdrawError::ResultGone("b".into()).to_string(),
            "the result of b is gone; its session is archived"
        );
        assert_eq!(
            WithdrawError::Unknown("b".into()).to_string(),
            "no session \"b\" in this queue"
        );
    }
}
