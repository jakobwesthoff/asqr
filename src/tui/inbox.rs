// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Keeps the app in step with the queue's files (spec sections 5.4 to 5.8):
//! sessions that arrive, change or disappear in the inbox, invalid files,
//! and the files the app's effects write, drafts and finished sessions.
//!
//! Every sync scans the whole inbox instead of following single watcher
//! events. An inbox holds a handful of files, and a full scan cannot miss
//! a rename or a burst of events the way event bookkeeping can.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use super::{App, AppEffect, EditRequest, Invalid, Notice, Outcome, SessionState, Tone};
use crate::format::{
    ParseError, SessionResult, Status, is_valid_session_id, now_rfc3339, parse_session,
    same_session_id, validate,
};
use crate::queue::{
    DRAFTS_LOCK_ATTEMPTS, Finished, Gate, QueueLocation, Recovery, Rejected, Waiting,
    archive_invalid_stem, archived_title, draft_has_answer, find_session_file,
    finish_unchanged_with, load_draft, recover_with, reject_invalid_with,
    save_draft_while_waiting_with, scan_inbox, session_sha256,
};

/// A session the app shows, with the bytes it was read from.
#[derive(Debug)]
struct Known {
    id: String,
    path: PathBuf,
    sha256: String,
}

/// What a sync found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Synced {
    /// Ids of sessions that are new to the app, in queue order.
    pub arrived: Vec<String>,
}

/// What the running app does after an effect was applied.
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    Continue,
    Quit,
    OpenImage(String),
    /// Open a field in the person's editor (ADR 74).
    Edit(EditRequest),
}

#[derive(Debug)]
pub struct Inbox {
    location: QueueLocation,
    known: Vec<Known>,
    ignored: HashSet<PathBuf>,
    /// How often the drafts lock is tried; tests that hold it lower this,
    /// so the busy path needs no waiting.
    lock_attempts: u32,
}

impl Inbox {
    pub fn new(location: QueueLocation) -> Self {
        Inbox {
            location,
            known: Vec::new(),
            ignored: HashSet::new(),
            lock_attempts: DRAFTS_LOCK_ATTEMPTS,
        }
    }

    /// The gate for an operation under the drafts lock.
    fn gate(&self) -> Gate<'static> {
        Gate {
            attempts: self.lock_attempts,
            ..Gate::default()
        }
    }

    fn known(&self, id: &str) -> Option<usize> {
        self.known
            .iter()
            .position(|known| same_session_id(&known.id, id))
    }

    /// Whether the file of session `id` is still in the inbox now; it may
    /// have left since the last sync, as when the asker withdrew it.
    pub fn waiting(&self, id: &str) -> bool {
        self.known(id)
            .is_some_and(|index| self.known[index].path.exists())
    }

    fn forget(&mut self, app: &mut App, id: &str) {
        if let Some(index) = self.known(id) {
            self.known.remove(index);
        }
        app.remove(id);
    }

    /// Forgets session `id`, which left the inbox, and tells the person
    /// when the asker withdrew it (spec section 7.9). Only a session the
    /// app shows gets the notice: one it never showed, such as one
    /// withdrawn while asqr was not running, leaves without a word.
    fn leave(&mut self, app: &mut App, id: &str) {
        let notice = self.withdrawn_notice(app, id);
        self.forget(app, id);
        if let Some(notice) = notice {
            app.show(notice);
        }
    }

    /// The notice for session `id` when its result says the asker withdrew
    /// it. The notice says what became of what the person entered: the
    /// result's `draft` took it to the asker, or nothing did, as when the
    /// person typed while the withdrawal ran.
    fn withdrawn_notice(&self, app: &App, id: &str) -> Option<Notice> {
        let state = app
            .sessions()
            .iter()
            .find(|state| same_session_id(state.id(), id))?;
        let path = find_session_file(&self.location.outbox(), id).ok()??;
        let result: SessionResult = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if result.status != Status::Withdrawn {
            return None;
        }
        let (tone, after) = if result.draft.is_some() {
            (Tone::Warning, "\" by the asker, with your answers so far")
        } else if draft_has_answer(&state.to_draft()) {
            (
                Tone::Warning,
                "\" by the asker; your answers to it are dropped",
            )
        } else {
            (Tone::Info, "\" by the asker")
        };
        Some(Notice::naming(tone, "↩ withdrawn \"", state.name(), after))
    }

    /// Brings `app` in line with the inbox. A failure with a single file
    /// becomes a notice; only an unreadable inbox is an error.
    pub fn sync(&mut self, app: &mut App) -> io::Result<Synced> {
        // Recovery runs on every sync, not only at the start: it is also
        // what archives a session dropped by hand next to an unread result
        // under its id (spec section 5.7).
        for recovery in recover_with(&self.location, self.gate())? {
            match recovery {
                Recovery::Completed(id) => {
                    tracing::info!(%id, "completed an interrupted finish");
                    self.leave(app, &id);
                }
                Recovery::Conflict(id) => {
                    tracing::warn!(%id, "archived a session that collided with an unread result");
                    self.forget(app, &id);
                    app.notice(Tone::Warning, format!(
                        "{id} was archived unanswered: an unread result with that id is in the outbox"
                    ));
                }
            }
        }

        let scan = scan_inbox(&self.location)?;
        // Each ignored file is logged once per run: a `.DS_Store` stays
        // for good and would otherwise be logged on every change.
        for path in scan.ignored {
            if self.ignored.insert(path.clone()) {
                tracing::info!(path = %path.display(), "ignored a file in the inbox");
            }
        }
        for path in &scan.invalid_stems {
            tracing::warn!(path = %path.display(), "archived a file whose name is no session id");
            if let Err(error) = archive_invalid_stem(&self.location, path) {
                app.notice(
                    Tone::Error,
                    format!("cannot archive {}: {error}", path.display()),
                );
            }
        }

        let gone: Vec<String> = self
            .known
            .iter()
            .filter(|known| {
                !scan
                    .sessions
                    .iter()
                    .any(|waiting| same_session_id(&waiting.id, &known.id))
            })
            .map(|known| known.id.clone())
            .collect();
        for id in gone {
            tracing::info!(%id, "session left the inbox");
            self.leave(app, &id);
        }

        let mut synced = Synced::default();
        let mut loaded = Vec::new();
        for waiting in scan.sessions {
            match self.load(app, &waiting, &mut synced) {
                Ok(true) => loaded.push(waiting.id),
                Ok(false) => {}
                Err(error) => app.notice(
                    Tone::Error,
                    format!("cannot read {}: {error}", waiting.path.display()),
                ),
            }
        }
        // Only once every waiting file is loaded, so a follow-up that is
        // older than the session it follows still finds it waiting.
        for id in loaded {
            let title = self.follows_title(app, &id);
            app.set_follows_title(&id, title);
        }
        Ok(synced)
    }

    /// Loads one waiting file into the app, unless the app shows exactly
    /// these bytes already. Says whether the app took a new state for it.
    fn load(&mut self, app: &mut App, waiting: &Waiting, synced: &mut Synced) -> io::Result<bool> {
        let bytes = match std::fs::read(&waiting.path) {
            // Finished or taken back between the scan and now; the next
            // sync sees it gone.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            other => other?,
        };
        let sha256 = session_sha256(&bytes);
        let index = self.known(&waiting.id);
        if index.is_some_and(|index| self.known[index].sha256 == sha256) {
            return Ok(false);
        }

        // Each error stays apart up to the dialog, which lists them; the
        // result and the log join them.
        let session = parse_session(&bytes)
            .map_err(|error| match error {
                ParseError::Refused(errors) => errors.iter().map(ToString::to_string).collect(),
                other => vec![other.to_string()],
            })
            .and_then(|(_, session)| match validate(&session, Some(&waiting.id)) {
                Ok(()) => Ok(session),
                Err(errors) => Err(errors.iter().map(ToString::to_string).collect()),
            });
        let session = match session {
            Ok(session) => session,
            Err(errors) => {
                self.forget(app, &waiting.id);
                self.reject(app, waiting, &bytes, &errors);
                return Ok(false);
            }
        };

        // A draft that cannot be read only loses the answers it held; the
        // session itself can still be answered.
        let draft = load_draft(&self.location, &waiting.id).unwrap_or_else(|error| {
            tracing::warn!(id = %waiting.id, %error, "ignored an unreadable draft");
            None
        });
        app.add(SessionState::new(
            waiting.id.clone(),
            session,
            draft.as_ref(),
        ));
        let known = Known {
            id: waiting.id.clone(),
            path: waiting.path.clone(),
            sha256,
        };
        match index {
            Some(index) => self.known[index] = known,
            None => {
                synced.arrived.push(waiting.id.clone());
                self.known.push(known);
            }
        }
        Ok(true)
    }

    /// The title of the session that session `id`'s `follows` names (spec
    /// section 7.2): a waiting session's, else the one archived last under
    /// that id. A session that names its own id can only mean an earlier
    /// round in the archive. A waiting session without a title names
    /// nothing, as it is the one meant.
    ///
    /// A value that is no valid id never matches a file, so the archive is
    /// not listed for it. An archive that cannot be listed costs only the
    /// title.
    fn follows_title(&self, app: &App, id: &str) -> Option<String> {
        let follows = app
            .sessions()
            .iter()
            .find(|state| same_session_id(state.id(), id))?
            .session()
            .follows
            .as_deref()
            .filter(|follows| is_valid_session_id(follows))?;
        let waiting = app
            .sessions()
            .iter()
            .find(|state| same_session_id(state.id(), follows) && !same_session_id(state.id(), id));
        if let Some(state) = waiting {
            return state.session().title.clone();
        }
        archived_title(&self.location, follows).unwrap_or_else(|error| {
            tracing::warn!(%id, %follows, %error, "cannot read the archive for a follow-up's title");
            None
        })
    }

    fn reject(&mut self, app: &mut App, waiting: &Waiting, bytes: &[u8], errors: &[String]) {
        let id = &waiting.id;
        let message = errors.join("; ");
        tracing::warn!(%id, %message, "rejected an invalid session");
        match reject_invalid_with(
            &self.location,
            &waiting.path,
            bytes,
            &message,
            &now_rfc3339(),
            self.gate(),
        ) {
            // The dialog lists the errors and the status line says what
            // happened while it shows, so no notice repeats it.
            Ok(Rejected::ErrorResult) => app.report_invalid(Invalid {
                id: id.clone(),
                errors: errors.to_vec(),
            }),
            Ok(Rejected::Conflict) => app.notice(Tone::Warning, format!(
                "{id} is invalid and was archived without a result, since an unread result with \
                 that id is in the outbox: {message}"
            )),
            Err(error) => app.notice(Tone::Error, format!("cannot reject the invalid {id}: {error}")),
            // The file left or changed while it was validated, as when the
            // asker withdrew or replaced it; the next sync sees what is
            // there now.
            Ok(Rejected::Left) => tracing::info!(%id, "an invalid session left before its rejection"),
        }
    }

    /// Carries out what a key asked for. Failures become notices, so a
    /// full disk or a vanished file never ends the app with answers lost.
    pub fn apply(&mut self, app: &mut App, effect: AppEffect) -> Applied {
        match effect {
            AppEffect::None => {}
            AppEffect::SaveDraft(draft) => {
                // A session that left the inbox gets no draft; the sync the
                // watcher sends next takes it out of the app.
                let Some(index) = self.known(&draft.id) else {
                    return Applied::Continue;
                };
                let session = &self.known[index].path;
                if let Err(error) =
                    save_draft_while_waiting_with(&self.location, session, &draft, self.gate())
                {
                    app.notice(
                        Tone::Error,
                        format!("cannot save the draft of {}: {error}", draft.id),
                    );
                }
            }
            AppEffect::Finish { id, outcome } => self.finish(app, &id, outcome),
            AppEffect::Quit => return Applied::Quit,
            AppEffect::OpenImage(path) => return Applied::OpenImage(path),
            AppEffect::Edit(request) => return Applied::Edit(request),
        }
        Applied::Continue
    }

    fn finish(&mut self, app: &mut App, id: &str, outcome: Outcome) {
        let Some(index) = self.known(id) else {
            return;
        };
        let known = &self.known[index];

        // The confirmation is worded before the finish, as `forget` takes
        // the session's state, and with it its title and counts, out of
        // the app.
        let confirmation = app
            .sessions()
            .iter()
            .find(|state| same_session_id(state.id(), id))
            .map(|state| confirmation(state, &outcome));

        let at = now_rfc3339();
        let result = match outcome {
            Outcome::Submitted { answers, note } => {
                SessionResult::submitted(&known.id, &known.sha256, &at, note, answers)
            }
            Outcome::Rejected {
                reason,
                note,
                answers,
            } => SessionResult::cancelled(&known.id, &known.sha256, &at, reason, note, answers),
        };
        // The result has to answer the bytes the person saw. A file that
        // changed since the last sync is loaded again instead, so the
        // person sees the new version before answering it. The sync runs
        // once the finish let go of the drafts lock, which its recovery
        // takes again.
        let finished = finish_unchanged_with(
            &self.location,
            &known.path,
            &known.sha256,
            &result,
            self.gate(),
        );
        match finished {
            Ok(Finished::Changed) => {
                app.notice(
                    Tone::Warning,
                    format!("{id} changed on disk and was not sent; check the new version"),
                );
                // The status line holds one notice, so an inbox that cannot
                // be read replaces the warning with its error.
                if let Err(error) = self.sync(app) {
                    app.notice(Tone::Error, format!("cannot read the inbox: {error}"));
                }
            }
            Ok(Finished::Done) => {
                tracing::info!(%id, status = ?result.status, "finished a session");
                self.forget(app, id);
                // Only a key or a paste clears a notice, so the
                // confirmation stays over the next session or the empty
                // queue (ADR 38).
                if let Some(confirmation) = confirmation {
                    app.show(confirmation);
                }
            }
            Err(error) => app.notice(Tone::Error, format!("cannot finish {id}: {error}")),
        }
    }
}

/// The status line's word that `state` was finished as `outcome`, since
/// the session leaves the screen at once (spec section 7.8). A sent
/// session is a success and counts its answers as the review showed
/// them; a rejected one is plain news (ADR 38). The name stays apart from
/// the rest, as the status line cuts it to keep the rest whole.
fn confirmation(state: &SessionState, outcome: &Outcome) -> Notice {
    let name = state.name();
    match outcome {
        Outcome::Submitted { .. } => {
            let counts = state.counts();
            let after = format!(
                "\" · {} answered, {} skipped",
                counts.answered, counts.skipped
            );
            Notice::naming(Tone::Success, "✔ sent \"", name, &after)
        }
        Outcome::Rejected { .. } => Notice::naming(Tone::Info, "✗ rejected \"", name, "\""),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::format::{Answer, Status};
    use crate::queue::{
        Archived, Withdrawn, archive_name, drop_session, parse_archive_name, save_draft,
    };

    const SESSION: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}]}"#;

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        location.create_layout().expect("layout");
        (scratch, location)
    }

    fn put(location: &QueueLocation, name: &str, text: &str) -> PathBuf {
        let path = location.inbox().join(name);
        fs::write(&path, text).expect("written");
        path
    }

    fn ids(app: &App) -> Vec<&str> {
        app.sessions().iter().map(SessionState::id).collect()
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

    fn result(location: &QueueLocation, id: &str) -> SessionResult {
        let bytes = fs::read(location.outbox().join(format!("{id}.json"))).expect("result");
        serde_json::from_slice(&bytes).expect("result parses")
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// Asserts that the app shows a notice of `tone` whose text passes
    /// `says`.
    #[track_caller]
    fn assert_notice(app: &App, tone: Tone, says: impl Fn(&str) -> bool) {
        let notice = app.current_notice().expect("a notice is shown");
        assert!(notice.tone == tone && says(&notice.text), "{notice:?}");
    }

    #[test]
    fn a_sync_adds_waiting_sessions_in_queue_order_and_reports_them() {
        let (_scratch, location) = queue();
        drop_session(&location, "first", SESSION.as_bytes(), false).expect("dropped");
        drop_session(&location, "second", SESSION.as_bytes(), false).expect("dropped");
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        let synced = inbox.sync(&mut app).expect("synced");

        assert_eq!(synced.arrived, ["first", "second"]);
        assert_eq!(ids(&app), ["first", "second"]);
        assert_eq!(inbox.sync(&mut app).expect("synced"), Synced::default());
        assert_eq!(ids(&app), ["first", "second"], "nothing is added twice");
    }

    #[test]
    fn a_session_starts_from_its_draft() {
        let (_scratch, location) = queue();
        drop_session(&location, "batch", SESSION.as_bytes(), false).expect("dropped");
        let mut answer = Answer::new("q");
        answer.selected = vec!["b".into()];
        save_draft(&location, &SessionResult::draft("batch", "q", vec![answer])).expect("saved");
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        assert_eq!(app.active().expect("active").answers()[0].selected, ["b"]);
    }

    #[test]
    fn an_unreadable_draft_is_ignored() {
        let (_scratch, location) = queue();
        drop_session(&location, "batch", SESSION.as_bytes(), false).expect("dropped");
        fs::write(location.drafts().join("batch.json"), "{").expect("written");
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        assert_eq!(ids(&app), ["batch"]);
        assert!(
            app.active().expect("active").answers()[0]
                .selected
                .is_empty()
        );
    }

    #[test]
    fn a_changed_file_replaces_the_session_keeping_its_answers() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location), App::default());
        inbox.sync(&mut app).expect("synced");
        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);

        fs::write(&path, SESSION.replace(r#""?""#, r#""Which one?""#)).expect("written");
        let synced = inbox.sync(&mut app).expect("synced");

        assert!(synced.arrived.is_empty(), "a replacement is no arrival");
        let state = app.active().expect("active");
        assert_eq!(state.session().questions[0].text, "Which one?");
        assert_eq!(state.answers()[0].selected, ["b"]);
    }

    #[test]
    fn a_session_that_left_the_inbox_leaves_the_app() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", SESSION);
        put(&location, "other.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location), App::default());
        inbox.sync(&mut app).expect("synced");

        fs::remove_file(path).expect("removed");
        inbox.sync(&mut app).expect("synced");

        assert_eq!(ids(&app), ["other"]);
    }

    #[test]
    fn an_invalid_session_gets_an_error_result_and_a_dialog() {
        let (_scratch, location) = queue();
        put(
            &location,
            "broken.json",
            &SESSION.replace(r#""kind": "single""#, r#""kind": "single", "min": 2"#),
        );
        put(&location, "garbage.json", "not json");
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        let synced = inbox.sync(&mut app).expect("synced");

        assert!(synced.arrived.is_empty() && app.sessions().is_empty());
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        let broken = result(&location, "broken");
        assert_eq!(broken.status, Status::Error);
        assert!(
            broken
                .error
                .as_deref()
                .is_some_and(|error| error.contains("questions[0]")),
            "the error names the field: {:?}",
            broken.error
        );
        let garbage = result(&location, "garbage");
        assert!(
            garbage
                .error
                .as_deref()
                .is_some_and(|e| e.starts_with("not a session file")),
            "{:?}",
            garbage.error
        );
        // The dialog says it, so no notice repeats it. The files load in
        // the order they came in, and both came at once, so by id.
        assert_eq!(app.current_notice(), None);
        let shown = app.invalid_shown().expect("a dialog");
        assert_eq!(shown.id, "broken");
        assert_eq!(shown.errors.len(), 1, "{:?}", shown.errors);
        assert!(
            shown.errors[0].contains("questions[0]"),
            "{:?}",
            shown.errors
        );
        // A frame showed the first dialog, so the key closes it.
        app.set_dialog_shown(true);
        app.handle(key('x'));
        let shown = app.invalid_shown().expect("the second dialog");
        assert_eq!(shown.id, "garbage");
        assert_eq!(shown.errors.len(), 1, "{:?}", shown.errors);
        assert!(
            shown.errors[0].starts_with("not a session file: "),
            "{:?}",
            shown.errors
        );
    }

    #[test]
    fn every_validation_error_of_an_invalid_session_stays_apart() {
        let (_scratch, location) = queue();
        put(
            &location,
            "broken.json",
            r#"{"asqr": 2, "questions": [
                {"id": "q", "text": "?", "kind": "single", "min": 2,
                 "options": [{"id": "a", "label": "A"}]},
                {"id": "q", "text": "?", "kind": "text"}]}"#,
        );
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        inbox.sync(&mut app).expect("synced");

        let errors = &app.invalid_shown().expect("a dialog").errors;
        assert!(errors.len() >= 2, "{errors:?}");
        assert_eq!(
            result(&location, "broken").error.as_deref(),
            Some(errors.join("; ").as_str()),
            "the result joins them"
        );
    }

    #[test]
    fn every_refused_entry_of_an_invalid_session_stays_apart() {
        // The messages are compared by their start, as the unknown type's
        // message goes on to list the types this asqr supports.
        let (_scratch, location) = queue();
        put(
            &location,
            "broken.json",
            r#"{"asqr": 2, "questions": [
                {"id": "q", "text": "?", "kind": "text",
                 "context": [{"type": "text", "content": "notes"}]},
                {"id": "r", "text": "?", "kind": "text",
                 "context": [{"type": "video", "content": "x"}]}]}"#,
        );
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        inbox.sync(&mut app).expect("synced");

        let errors = &app.invalid_shown().expect("a dialog").errors;
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].starts_with(r#"questions[0].context[0].type: "text" is no entry type"#)
                && errors[1].starts_with(
                    r#"questions[1].context[0].type: "video" is not supported by this asqr"#
                ),
            "{errors:?}"
        );
        assert_eq!(
            result(&location, "broken").error.as_deref(),
            Some(errors.join("; ").as_str()),
            "the result joins them"
        );
    }

    #[test]
    fn a_question_of_an_unknown_kind_gets_an_error_result_naming_it() {
        // An asqr that runs while a newer one is installed meets kinds the
        // newer `asqr ask` accepted (ADR 61).
        let (_scratch, location) = queue();
        put(
            &location,
            "ranked.json",
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "rank"}]}"#,
        );
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        inbox.sync(&mut app).expect("synced");

        let errors = &app.invalid_shown().expect("a dialog").errors;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with(r#"questions[0].kind: "rank" is not supported by this asqr"#),
            "{errors:?}"
        );
        assert_eq!(
            result(&location, "ranked").error.as_deref(),
            Some(errors[0].as_str())
        );
    }

    #[test]
    fn a_session_of_format_version_1_still_waiting_gets_an_error_result() {
        // Sessions an earlier asqr would have shown can wait in a queue
        // across an upgrade. Their askers learn from the result what to
        // write instead (spec section 3.6).
        let (_scratch, location) = queue();
        put(
            &location,
            "old.json",
            r#"{"asqr": 1, "questions": [{"id": "q", "text": "?", "kind": "text", "image": "/q.png"}]}"#,
        );
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        inbox.sync(&mut app).expect("synced");

        assert!(app.sessions().is_empty());
        let old = result(&location, "old");
        assert_eq!(old.status, Status::Error);
        assert_eq!(
            old.error.as_deref(),
            Some(
                "asqr: this asqr no longer supports format version 1; `asqr skill` describes the current format (version 2)"
            )
        );
    }

    #[test]
    fn a_session_with_an_unknown_entry_type_gets_an_error_result_that_says_so() {
        // An asqr updated while this one runs accepts types this one does
        // not know, and its asker learns from the result why the session
        // came back.
        let (_scratch, location) = queue();
        put(
            &location,
            "newer.json",
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
                "context": [{"type": "video", "content": "x"}]}]}"#,
        );
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        inbox.sync(&mut app).expect("synced");

        let message = format!(
            r#"questions[0].context[0].type: "video" is not supported by this asqr (version {}), which supports "image", "code", "markdown", "diff" and "ansi"; a newer asqr may support it"#,
            env!("CARGO_PKG_VERSION")
        );
        let newer = result(&location, "newer");
        assert_eq!(newer.status, Status::Error);
        assert_eq!(newer.error.as_deref(), Some(message.as_str()));
        assert_eq!(app.invalid_shown().expect("a dialog").errors, [message]);
    }

    #[test]
    fn a_session_that_becomes_invalid_leaves_the_app() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        fs::write(&path, "{").expect("written");
        inbox.sync(&mut app).expect("synced");

        assert!(app.sessions().is_empty());
        assert_eq!(result(&location, "batch").status, Status::Error);
    }

    #[test]
    fn an_invalid_session_next_to_an_unread_result_is_only_archived() {
        let (_scratch, location) = queue();
        let unread = SessionResult::cancelled(
            "batch",
            "other bytes",
            "2026-09-27T17:05:12+02:00",
            None,
            None,
            Vec::new(),
        );
        fs::write(
            location.outbox().join("batch.json"),
            serde_json::to_vec(&unread).expect("serializes"),
        )
        .expect("written");
        put(&location, "batch.json", "{");
        let mut inbox = Inbox::new(location.clone());
        let mut app = App::default();

        // Recovery archives the colliding file before validation sees it.
        inbox.sync(&mut app).expect("synced");

        assert_eq!(
            result(&location, "batch"),
            unread,
            "the unread result stays"
        );
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
        assert_notice(&app, Tone::Warning, |text| text.contains("unread result"));
    }

    #[test]
    fn a_file_named_after_no_valid_id_is_archived_without_a_result() {
        let (_scratch, location) = queue();
        put(&location, "bad name.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        inbox.sync(&mut app).expect("synced");

        assert_eq!(names(&location.outbox()), Vec::<String>::new());
        let archived = names(&location.archive());
        assert_eq!(archived.len(), 1);
        assert!(parse_archive_name(&archived[0]).is_some(), "{archived:?}");
    }

    #[test]
    fn other_files_in_the_inbox_are_left_alone() {
        let (_scratch, location) = queue();
        put(&location, ".DS_Store", "");
        put(&location, "notes.txt", "");
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        inbox.sync(&mut app).expect("synced");
        inbox.sync(&mut app).expect("synced");

        assert!(app.sessions().is_empty() && app.current_notice().is_none());
        assert_eq!(names(&location.inbox()), [".DS_Store", "notes.txt"]);
        assert_eq!(inbox.ignored.len(), 2, "each is logged once");
    }

    #[test]
    fn an_interrupted_finish_is_completed_and_not_shown() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let done = SessionResult::submitted(
            "batch",
            &session_sha256(SESSION.as_bytes()),
            "2026-09-27T17:05:12+02:00",
            None,
            vec![Answer::skipped("q")],
        );
        fs::write(
            location.outbox().join("batch.json"),
            serde_json::to_vec(&done).expect("serializes"),
        )
        .expect("written");
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        let synced = inbox.sync(&mut app).expect("synced");

        assert!(synced.arrived.is_empty() && app.sessions().is_empty());
        assert_eq!(app.current_notice(), None);
    }

    #[test]
    fn an_unreadable_inbox_is_an_error() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path());
        fs::write(location.inbox(), "not a directory").expect("written");

        assert!(Inbox::new(location).sync(&mut App::default()).is_err());
    }

    #[test]
    fn changes_save_the_draft() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        let effect = app.handle(key('2'));
        assert_eq!(inbox.apply(&mut app, effect), Applied::Continue);

        let draft = load_draft(&location, "batch")
            .expect("readable")
            .expect("saved");
        assert_eq!(draft.answers[0].selected, ["b"]);
    }

    #[test]
    fn a_draft_that_cannot_be_saved_becomes_a_notice() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        fs::remove_dir(location.drafts()).expect("removed");
        fs::write(location.drafts(), "in the way").expect("written");

        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);

        assert_notice(&app, Tone::Error, |text| {
            text.starts_with("cannot save the draft")
        });
    }

    /// Picks the second option, which moves on to the review and its
    /// Submit row, then presses enter on Submit or on Reject, which lies
    /// below the session note.
    fn answer_and(app: &mut App, inbox: &mut Inbox, reject: bool) -> Applied {
        let effect = app.handle(key('2'));
        inbox.apply(app, effect);
        if reject {
            app.handle(key('j'));
            app.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        let effect = app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        inbox.apply(app, effect)
    }

    #[test]
    fn a_submit_finishes_the_session_with_its_hash() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        put(&location, "next.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        assert_eq!(answer_and(&mut app, &mut inbox, false), Applied::Continue);

        let result = result(&location, "batch");
        assert_eq!(result.status, Status::Submitted);
        assert_eq!(
            result.session_sha256,
            Some(session_sha256(SESSION.as_bytes()))
        );
        assert_eq!(result.answers[0].selected, ["b"]);
        assert_eq!(names(&location.inbox()), ["next.json"]);
        assert_eq!(names(&location.drafts()), Vec::<String>::new());
        assert_eq!(ids(&app), ["next"], "the next session takes over");
        assert_eq!(inbox.sync(&mut app).expect("synced"), Synced::default());
    }

    #[test]
    fn a_submit_with_a_session_note_writes_it_into_the_result() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);

        app.handle(key('j'));
        for c in "ship it".chars() {
            let effect = app.handle(key(c));
            inbox.apply(&mut app, effect);
        }
        assert_eq!(
            load_draft(&location, "batch")
                .expect("readable")
                .and_then(|draft| draft.note)
                .as_deref(),
            Some("ship it"),
            "the draft keeps the note while it is typed"
        );
        let effect = app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        inbox.apply(&mut app, effect);

        let result = result(&location, "batch");
        assert_eq!(
            (result.status, result.note.as_deref()),
            (Status::Submitted, Some("ship it"))
        );
    }

    #[test]
    fn a_reject_finishes_the_session_as_cancelled() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        answer_and(&mut app, &mut inbox, true);

        assert_eq!(result(&location, "batch").status, Status::Cancelled);
        assert!(app.sessions().is_empty());
    }

    /// A titled session with two questions, so the confirmation of a
    /// submit can name the title and count an answer and a skip.
    const RELEASE: &str = r#"{"asqr": 2, "title": "Release 0.2.0", "questions": [
        {"id": "q", "text": "?", "kind": "single",
         "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]},
        {"id": "r", "text": "?", "kind": "single",
         "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]}]}"#;

    #[test]
    fn a_submit_confirms_the_session_with_its_title_and_counts() {
        let (_scratch, location) = queue();
        put(&location, "release.json", RELEASE);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        // `2` answers the first question and moves on to the second,
        // which `→` leaves unanswered for the review and its Submit row.
        for key in [
            key('2'),
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            let effect = app.handle(key);
            inbox.apply(&mut app, effect);
        }

        assert_eq!(result(&location, "release").status, Status::Submitted);
        assert_eq!(
            app.current_notice(),
            Some(&Notice::naming(
                Tone::Success,
                "✔ sent \"",
                "Release 0.2.0",
                "\" · 1 answered, 1 skipped"
            ))
        );
    }

    /// The status line of `app` drawn `width` columns wide.
    fn status_row(app: &App, width: u16) -> String {
        use crate::graphics::Images;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        use crate::tui::render::{PaneLayout, View, draw};

        let mut terminal = Terminal::new(TestBackend::new(width, 20)).expect("test terminal");
        let mut view = View::for_tests("default", PaneLayout::Auto, Images::half_blocks());
        terminal
            .draw(|frame| {
                draw(frame, app, &mut view);
            })
            .expect("draws");
        let buf = terminal.backend().buffer();
        (0..width)
            .map(|x| buf[(x, 18)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn a_long_title_is_cut_in_the_confirmation_and_the_counts_stay() {
        let (_scratch, location) = queue();
        let title = "Release 0.2.0: onboarding, billing and the new API";
        assert_eq!(title.chars().count(), 50);
        put(
            &location,
            "release.json",
            &RELEASE.replace("Release 0.2.0", title),
        );
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        for key in [
            key('2'),
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            let effect = app.handle(key);
            inbox.apply(&mut app, effect);
        }

        // `✔ sent "` and `" · 1 answered, 1 skipped` leave the title 27 of
        // the 60 columns.
        assert_eq!(
            status_row(&app, 60),
            "✔ sent \"Release 0.2.0: onboarding,…\" · 1 answered, 1 skipped"
        );
    }

    #[test]
    fn a_long_title_is_cut_in_the_rejection_before_its_closing_quote() {
        let (_scratch, location) = queue();
        let titled = SESSION.replace(
            r#""questions""#,
            r#""title": "Release 0.2.0: onboarding, billing and the new API", "questions""#,
        );
        put(&location, "release.json", &titled);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        answer_and(&mut app, &mut inbox, true);

        // `✗ rejected "` and `"` leave the title 47 of the 60 columns.
        assert_eq!(
            status_row(&app, 60),
            "✗ rejected \"Release 0.2.0: onboarding, billing and the new…\""
        );
    }

    #[test]
    fn a_reject_confirms_the_session_by_its_id_without_a_title() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        answer_and(&mut app, &mut inbox, true);

        assert_eq!(result(&location, "batch").status, Status::Cancelled);
        assert_eq!(
            app.current_notice(),
            Some(&Notice::naming(Tone::Info, "✗ rejected \"", "batch", "\""))
        );
    }

    #[test]
    fn a_reject_keeps_the_notes_and_the_answers_in_the_result() {
        let (_scratch, location) = queue();
        put(&location, "release.json", RELEASE);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        // A note on `q`, then `2` picks b and moves on to `r`, which `→`
        // leaves unanswered for the review. There `j` goes into the session
        // note and `↓` on to the reject reason.
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let mut keys = vec![key('n')];
        keys.extend("see the logs".chars().map(key));
        keys.extend([
            enter,
            key('2'),
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            key('j'),
        ]);
        keys.extend("ship after the fix".chars().map(key));
        keys.push(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        keys.extend("see the note on q".chars().map(key));
        keys.push(enter);
        for key in keys {
            let effect = app.handle(key);
            inbox.apply(&mut app, effect);
        }

        let result = result(&location, "release");
        assert_eq!(
            (
                result.status,
                result.reason.as_deref(),
                result.note.as_deref(),
                result.answers
            ),
            (
                Status::Cancelled,
                Some("see the note on q"),
                Some("ship after the fix"),
                vec![Answer {
                    selected: vec!["b".into()],
                    note: Some("see the logs".into()),
                    ..Answer::new("q")
                }]
            )
        );
        assert_eq!(names(&location.drafts()), Vec::<String>::new());
    }

    #[test]
    fn the_confirmation_stays_over_the_next_session_until_a_key() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        put(&location, "next.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        answer_and(&mut app, &mut inbox, false);
        // The watcher syncs again after the finish moved the file.
        inbox.sync(&mut app).expect("synced");

        assert_eq!(ids(&app), ["next"]);
        assert_notice(&app, Tone::Success, |text| {
            text.starts_with("✔ sent \"batch\"")
        });
        app.handle(key('j'));
        assert_eq!(app.current_notice(), None);
    }

    /// The session waiting behind the one answered was never on screen, so
    /// its reload says nothing to the person and comes without a message.
    #[test]
    fn a_reload_of_a_session_not_yet_seen_brings_no_message() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let next = put(&location, "next.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        let titled = SESSION.replacen(r#"{"asqr": 2,"#, r#"{"asqr": 2, "title": "Next","#, 1);
        fs::write(&next, titled).expect("written");
        inbox.sync(&mut app).expect("synced");

        answer_and(&mut app, &mut inbox, false);

        let state = app.active().expect("the next session takes over");
        assert_eq!((state.id(), state.name()), ("next", "Next"));
        assert_eq!(state.message(), None);
        assert_notice(&app, Tone::Success, |text| {
            text.starts_with("✔ sent \"batch\"")
        });
    }

    /// One sync can take the session on screen out and reload the one that
    /// takes over before any frame showed it.
    #[test]
    fn a_session_that_takes_over_and_is_reloaded_in_one_sync_brings_no_message() {
        let (_scratch, location) = queue();
        let batch = put(&location, "batch.json", SESSION);
        let next = put(&location, "next.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        fs::remove_file(&batch).expect("removed");
        let titled = SESSION.replacen(r#"{"asqr": 2,"#, r#"{"asqr": 2, "title": "Next","#, 1);
        fs::write(&next, titled).expect("written");

        inbox.sync(&mut app).expect("synced");

        let state = app.active().expect("the next session takes over");
        assert_eq!((state.id(), state.name()), ("next", "Next"));
        assert_eq!(state.message(), None);
    }

    #[test]
    fn a_session_changed_on_disk_is_not_sent_but_reloaded() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        fs::write(&path, SESSION.replace(r#""?""#, r#""Which one?""#)).expect("written");

        answer_and(&mut app, &mut inbox, false);

        assert_eq!(names(&location.outbox()), Vec::<String>::new());
        let state = app.active().expect("still waiting");
        assert_eq!(state.session().questions[0].text, "Which one?");
        assert_notice(&app, Tone::Warning, |text| text.contains("changed on disk"));
    }

    #[test]
    fn a_finish_that_fails_keeps_the_session() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        // An unread result that arrived after the sync, as `ask --force`
        // cannot produce but a hand-written file can.
        fs::write(location.outbox().join("batch.json"), "{}").expect("written");

        answer_and(&mut app, &mut inbox, false);

        assert_eq!(ids(&app), ["batch"]);
        assert_notice(&app, Tone::Error, |text| {
            text.starts_with("cannot finish batch")
        });
    }

    #[test]
    fn a_file_that_cannot_be_archived_becomes_a_notice() {
        let (_scratch, location) = queue();
        put(&location, "bad name.json", SESSION);
        fs::remove_dir(location.archive()).expect("removed");
        fs::write(location.archive(), "in the way").expect("written");
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        assert_notice(&app, Tone::Error, |text| text.starts_with("cannot archive"));
    }

    #[cfg(unix)]
    #[test]
    fn a_session_that_cannot_be_read_becomes_a_notice() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, location) = queue();
        let path = put(&location, "locked.json", SESSION);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("chmod");
        put(&location, "open.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        let synced = inbox.sync(&mut app).expect("synced");

        assert_eq!(synced.arrived, ["open"]);
        assert_notice(&app, Tone::Error, |text| text.starts_with("cannot read"));
    }

    #[test]
    fn a_session_gone_since_the_scan_is_skipped() {
        let (_scratch, location) = queue();
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        let gone = Waiting {
            id: "gone".into(),
            path: location.inbox().join("gone.json"),
            modified: std::time::SystemTime::UNIX_EPOCH,
        };
        let mut synced = Synced::default();

        inbox.load(&mut app, &gone, &mut synced).expect("no error");

        assert!(synced.arrived.is_empty() && app.sessions().is_empty());
    }

    // The two rejections below cannot be reached through a sync, whose
    // recovery archives a colliding file first; they cover a file that
    // changes in between.
    #[test]
    fn a_rejection_next_to_an_unread_result_says_so() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", "{");
        fs::write(location.outbox().join("batch.json"), "{}").expect("written");
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        let waiting = Waiting {
            id: "batch".into(),
            path,
            modified: std::time::SystemTime::UNIX_EPOCH,
        };

        inbox.reject(&mut app, &waiting, b"{", &["broken".to_owned()]);

        assert_notice(&app, Tone::Warning, |text| {
            text.contains("without a result") && text.ends_with(": broken")
        });
        assert_eq!(app.invalid_shown(), None, "no dialog");
        assert_eq!(names(&location.inbox()), Vec::<String>::new());
    }

    #[test]
    fn a_rejection_that_fails_becomes_a_notice() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", "{");
        fs::remove_dir(location.outbox()).expect("removed");
        fs::write(location.outbox(), "in the way").expect("written");
        let (mut inbox, mut app) = (Inbox::new(location), App::default());
        let waiting = Waiting {
            id: "batch".into(),
            path,
            modified: std::time::SystemTime::UNIX_EPOCH,
        };

        inbox.reject(&mut app, &waiting, b"{", &["broken".to_owned()]);

        assert_notice(&app, Tone::Error, |text| text.starts_with("cannot reject"));
        assert_eq!(app.invalid_shown(), None, "no dialog");
    }

    #[test]
    fn a_rejection_of_a_file_that_changed_since_it_was_read_does_nothing() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        let waiting = Waiting {
            id: "batch".into(),
            path,
            modified: std::time::SystemTime::UNIX_EPOCH,
        };

        // Read as "{" and found invalid, then replaced by a valid file.
        inbox.reject(&mut app, &waiting, b"{", &["broken".to_owned()]);

        assert_eq!(app.current_notice(), None);
        assert_eq!(app.invalid_shown(), None, "no dialog");
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
        assert_eq!(names(&location.inbox()), ["batch.json"]);
    }

    #[test]
    fn a_finish_with_the_inbox_gone_reports_both() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        fs::remove_file(location.inbox().join("batch.json")).expect("removed");
        fs::remove_dir(location.inbox()).expect("removed");
        fs::write(location.inbox(), "in the way").expect("written");

        answer_and(&mut app, &mut inbox, false);

        assert_notice(&app, Tone::Error, |text| {
            text.starts_with("cannot read the inbox")
        });
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
    }

    // -----------------------------------------------------------------
    // The drafts lock
    // -----------------------------------------------------------------

    #[test]
    fn a_key_after_the_session_left_saves_no_draft() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        // Taken out before the watcher's sync told the app.
        fs::remove_file(path).expect("removed");

        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);

        assert_eq!(names(&location.drafts()), Vec::<String>::new());
        assert_eq!(app.current_notice(), None);
    }

    #[test]
    fn a_draft_of_a_session_the_inbox_does_not_know_is_not_saved() {
        let (_scratch, location) = queue();
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        let draft = SessionResult::draft("unknown", "q", vec![Answer::new("q")]);
        inbox.apply(&mut app, AppEffect::SaveDraft(draft));

        assert_eq!(names(&location.drafts()), Vec::<String>::new());
    }

    #[test]
    fn a_busy_drafts_lock_keeps_the_answers_and_says_so() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        inbox.lock_attempts = 1;
        let _held = crate::queue::lock_drafts(&location, 1).expect("locks");

        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);

        assert_notice(&app, Tone::Error, |text| {
            text == "cannot save the draft of batch: the queue is busy"
        });
        assert_eq!(names(&location.drafts()), Vec::<String>::new());
        assert_eq!(app.active().expect("active").answers()[0].selected, ["b"]);
    }

    #[test]
    fn a_submit_while_the_drafts_lock_is_busy_keeps_the_session() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        inbox.lock_attempts = 1;
        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);
        let _held = crate::queue::lock_drafts(&location, 1).expect("locks");

        let effect = app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        inbox.apply(&mut app, effect);

        assert_notice(&app, Tone::Error, |text| {
            text == "cannot finish batch: the queue is busy"
        });
        assert_eq!(ids(&app), ["batch"]);
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
    }

    #[test]
    fn a_sync_loads_the_inbox_while_the_drafts_lock_is_busy() {
        let (_scratch, location) = queue();
        put(&location, "waiting.json", SESSION);
        // A finish interrupted after its result was written, which recovery
        // completes once the lock is free again.
        put(&location, "done.json", SESSION);
        let done = SessionResult::submitted(
            "done",
            &session_sha256(SESSION.as_bytes()),
            "2026-09-27T17:05:12+02:00",
            None,
            vec![Answer::skipped("q")],
        );
        fs::write(
            location.outbox().join("done.json"),
            serde_json::to_vec(&done).expect("serializes"),
        )
        .expect("written");
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.lock_attempts = 1;
        let held = crate::queue::lock_drafts(&location, 1).expect("locks");

        inbox.sync(&mut app).expect("synced");

        assert!(ids(&app).contains(&"waiting"), "{:?}", ids(&app));
        assert_eq!(names(&location.inbox()), ["done.json", "waiting.json"]);
        assert_eq!(app.current_notice(), None);

        drop(held);
        inbox.sync(&mut app).expect("synced");
        assert_eq!(ids(&app), ["waiting"], "recovered once the lock is free");
    }

    // -----------------------------------------------------------------
    // A session the asker withdrew
    // -----------------------------------------------------------------

    use crate::queue::withdraw_session;

    /// The notice for session `name` withdrawn, `after` following the name.
    fn withdrawn(tone: Tone, name: &str, after: &str) -> Notice {
        Notice::naming(tone, "↩ withdrawn \"", name, after)
    }

    #[test]
    fn a_withdrawn_session_leaves_with_a_notice() {
        let (_scratch, location) = queue();
        put(&location, "release.json", RELEASE);
        put(&location, "next.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        withdraw_session(&location, "release", false).expect("withdrawn");
        inbox.sync(&mut app).expect("synced");

        assert_eq!(ids(&app), ["next"]);
        assert_eq!(
            app.current_notice(),
            Some(&withdrawn(Tone::Info, "Release 0.2.0", "\" by the asker"))
        );
    }

    #[test]
    fn a_session_withdrawn_with_its_draft_leaves_with_a_warning() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);

        withdraw_session(&location, "batch", true).expect("withdrawn");
        inbox.sync(&mut app).expect("synced");

        assert_eq!(
            app.current_notice(),
            Some(&withdrawn(
                Tone::Warning,
                "batch",
                "\" by the asker, with your answers so far"
            ))
        );
    }

    #[test]
    fn a_key_during_a_withdrawal_saves_nothing_and_warns_after_the_sync() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        // The withdrawal saw no answer; the key comes before the watcher's
        // sync, so its draft finds the session gone.
        withdraw_session(&location, "batch", false).expect("withdrawn");
        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);
        inbox.sync(&mut app).expect("synced");

        assert_eq!(names(&location.drafts()), Vec::<String>::new());
        assert_eq!(
            app.current_notice(),
            Some(&withdrawn(
                Tone::Warning,
                "batch",
                "\" by the asker; your answers to it are dropped"
            ))
        );
    }

    #[test]
    fn an_untouched_default_counts_as_no_answer_in_the_notice() {
        let (_scratch, location) = queue();
        put(
            &location,
            "batch.json",
            &SESSION.replace(r#""label": "A""#, r#""label": "A", "default": true"#),
        );
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        withdraw_session(&location, "batch", false).expect("withdrawn");
        inbox.sync(&mut app).expect("synced");

        assert_eq!(
            app.current_notice(),
            Some(&withdrawn(Tone::Info, "batch", "\" by the asker"))
        );
    }

    /// A frame of `app`, handed back to it as the run loop does, so the
    /// session it shows counts as seen.
    fn shown(app: &mut App) {
        use crate::graphics::Images;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        use crate::tui::render::{Drawn, PaneLayout, View, draw};

        let mut terminal = Terminal::new(TestBackend::new(60, 20)).expect("test terminal");
        let mut view = View::for_tests("default", PaneLayout::Auto, Images::half_blocks());
        let mut drawn = Drawn::default();
        terminal
            .draw(|frame| drawn = draw(frame, app, &mut view))
            .expect("draws");
        app.set_session_shown(drawn.session_shown);
    }

    /// A session whose option `a` is the asker's default.
    fn with_default() -> String {
        SESSION.replace(r#""label": "A""#, r#""label": "A", "default": true"#)
    }

    /// Shows the session of [`with_default`] and moves on to the review,
    /// which saves its draft with the default untouched.
    fn browsed(location: &QueueLocation) -> (Inbox, App) {
        put(location, "batch.json", &with_default());
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        shown(&mut app);
        let effect = app.handle(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        inbox.apply(&mut app, effect);
        assert!(
            load_draft(location, "batch").expect("readable").is_some(),
            "the move saved a draft"
        );
        (inbox, app)
    }

    #[test]
    fn a_draft_saved_by_a_tab_move_holds_no_answer() {
        let (_scratch, location) = queue();
        browsed(&location);

        assert!(!crate::queue::has_answered_draft(&location, "batch"));
    }

    #[test]
    fn a_session_only_browsed_is_withdrawn_without_force() {
        let (_scratch, location) = queue();
        browsed(&location);

        let withdrawn = withdraw_session(&location, "batch", false);

        assert!(matches!(withdrawn, Ok(Withdrawn::Now(_))), "{withdrawn:?}");
    }

    #[test]
    fn a_session_only_browsed_leaves_with_the_plain_notice() {
        let (_scratch, location) = queue();
        let (mut inbox, mut app) = browsed(&location);

        withdraw_session(&location, "batch", true).expect("withdrawn");
        inbox.sync(&mut app).expect("synced");

        assert_eq!(
            app.current_notice(),
            Some(&withdrawn(Tone::Info, "batch", "\" by the asker"))
        );
    }

    #[test]
    fn a_re_picked_default_still_counts() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", &with_default());
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        let effect = app.handle(key('1'));
        inbox.apply(&mut app, effect);

        assert!(crate::queue::has_answered_draft(&location, "batch"));
    }

    #[test]
    fn a_typed_note_counts_as_an_answer_in_the_notice() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        withdraw_session(&location, "batch", false).expect("withdrawn");
        // A note to the whole session on the review tab, never saved.
        app.handle(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle(key('j'));
        app.handle(key('x'));
        inbox.sync(&mut app).expect("synced");

        assert_notice(&app, Tone::Warning, |text| text.ends_with("are dropped"));
    }

    /// Writes the result a withdrawal writes first, as if it stopped
    /// before it archived the session.
    fn interrupted_withdrawal(location: &QueueLocation, id: &str) {
        let result = SessionResult::withdrawn(
            id,
            &session_sha256(SESSION.as_bytes()),
            "2026-10-28T10:00:00+01:00",
            None,
        );
        fs::write(
            location.outbox().join(format!("{id}.json")),
            serde_json::to_vec(&result).expect("serializes"),
        )
        .expect("written");
    }

    #[test]
    fn an_interrupted_withdrawal_completed_by_recovery_shows_the_notice() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        interrupted_withdrawal(&location, "batch");
        inbox.sync(&mut app).expect("synced");

        assert!(app.sessions().is_empty());
        assert_eq!(
            app.current_notice(),
            Some(&withdrawn(Tone::Info, "batch", "\" by the asker"))
        );
    }

    #[test]
    fn an_interrupted_withdrawal_completed_at_start_shows_no_notice() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        interrupted_withdrawal(&location, "batch");
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        inbox.sync(&mut app).expect("synced");

        assert!(app.sessions().is_empty());
        assert_eq!(app.current_notice(), None);
    }

    #[test]
    fn a_session_taken_out_by_hand_leaves_without_a_notice() {
        let (_scratch, location) = queue();
        let path = put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        fs::remove_file(path).expect("removed");
        inbox.sync(&mut app).expect("synced");

        assert!(app.sessions().is_empty());
        assert_eq!(app.current_notice(), None);
    }

    #[test]
    fn a_submit_after_a_withdrawal_writes_nothing_and_names_the_withdrawal() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);
        let Withdrawn::Now(withdrawal) =
            withdraw_session(&location, "batch", true).expect("withdrawn")
        else {
            panic!("withdrawn now");
        };

        let effect = app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        inbox.apply(&mut app, effect);

        assert_eq!(
            result(&location, "batch"),
            withdrawal,
            "only the withdrawal"
        );
        assert!(app.sessions().is_empty());
        assert_eq!(
            app.current_notice(),
            Some(&withdrawn(
                Tone::Warning,
                "batch",
                "\" by the asker, with your answers so far"
            ))
        );
    }

    #[test]
    fn the_withdrawn_notice_shows_in_the_status_line() {
        let (_scratch, location) = queue();
        put(&location, "release.json", RELEASE);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        withdraw_session(&location, "release", false).expect("withdrawn");
        inbox.sync(&mut app).expect("synced");

        assert_eq!(
            status_row(&app, 60),
            "↩ withdrawn \"Release 0.2.0\" by the asker"
        );
    }

    #[test]
    fn the_withdrawn_warning_shows_in_the_status_line_cut_before_its_quote() {
        let (_scratch, location) = queue();
        put(&location, "release.json", RELEASE);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        let effect = app.handle(key('2'));
        inbox.apply(&mut app, effect);

        withdraw_session(&location, "release", true).expect("withdrawn");
        inbox.sync(&mut app).expect("synced");

        assert_eq!(
            status_row(&app, 60),
            "! ↩ withdrawn \"Rele…\" by the asker, with your answers so far"
        );
    }

    #[test]
    fn quitting_and_opening_an_image_go_back_to_the_running_app() {
        let (_scratch, location) = queue();
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        assert_eq!(inbox.apply(&mut app, AppEffect::None), Applied::Continue);
        assert_eq!(inbox.apply(&mut app, AppEffect::Quit), Applied::Quit);
        assert_eq!(
            inbox.apply(&mut app, AppEffect::OpenImage("/i.png".into())),
            Applied::OpenImage("/i.png".into())
        );
        let unknown = AppEffect::Finish {
            id: "unknown".into(),
            outcome: Outcome::Rejected {
                reason: None,
                note: None,
                answers: Vec::new(),
            },
        };
        assert_eq!(inbox.apply(&mut app, unknown), Applied::Continue);
    }

    // -----------------------------------------------------------------
    // The session a follow-up continues
    // -----------------------------------------------------------------

    /// A session with `fields` before its question, such as
    /// `"title": "Round one",`.
    fn with(fields: &str) -> String {
        SESSION.replacen('{', &format!("{{{fields}"), 1)
    }

    /// Archives `text` as the finished session `id`, archived at `millis`.
    fn archived(location: &QueueLocation, id: &str, millis: u64, text: &str) {
        let name = archive_name(id, ulid::Ulid::from_parts(millis, 0), Archived::Session);
        fs::write(location.archive().join(name), text).expect("written");
    }

    /// The `follows` title the app keeps for session `id`.
    fn follows_title<'a>(app: &'a App, id: &str) -> Option<&'a str> {
        app.sessions()
            .iter()
            .find(|state| state.id() == id)
            .and_then(SessionState::follows_title)
    }

    #[test]
    fn a_follow_up_names_the_waiting_session_it_follows() {
        let (_scratch, location) = queue();
        put(&location, "round-1.json", &with(r#""title": "Round one","#));
        let follow_up = put(&location, "round-2.json", &with(r#""follows": "ROUND-1","#));
        // Older than its predecessor, the follow-up loads first.
        fs::File::options()
            .write(true)
            .open(&follow_up)
            .expect("opened")
            .set_modified(std::time::SystemTime::UNIX_EPOCH)
            .expect("touched");
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        assert_eq!(ids(&app), ["round-2", "round-1"]);
        assert_eq!(follows_title(&app, "round-2"), Some("Round one"));
    }

    #[test]
    fn a_waiting_session_wins_over_its_archived_round() {
        let (_scratch, location) = queue();
        archived(
            &location,
            "round-1",
            1_000,
            &with(r#""title": "Round one, first try","#),
        );
        put(&location, "round-1.json", &with(r#""title": "Round one","#));
        put(&location, "round-2.json", &with(r#""follows": "round-1","#));
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        assert_eq!(follows_title(&app, "round-2"), Some("Round one"));
    }

    #[test]
    fn a_follow_up_names_an_archived_session() {
        let (_scratch, location) = queue();
        archived(
            &location,
            "round-1",
            1_000,
            &with(r#""title": "Round one","#),
        );
        put(&location, "round-2.json", &with(r#""follows": "round-1","#));
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        assert_eq!(follows_title(&app, "round-2"), Some("Round one"));
    }

    #[test]
    fn a_follow_up_shows_the_id_where_no_title_is_found() {
        let (_scratch, location) = queue();
        // A waiting predecessor without a title is found, so its archived
        // round does not count.
        archived(
            &location,
            "untitled",
            1_000,
            &with(r#""title": "Archived","#),
        );
        put(&location, "untitled.json", SESSION);
        put(&location, "a.json", &with(r#""follows": "untitled","#));
        put(&location, "b.json", &with(r#""follows": "unknown","#));
        put(
            &location,
            "c.json",
            &with(r#""follows": "round 1 of the review","#),
        );
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        for id in ["a", "b", "c"] {
            assert_eq!(follows_title(&app, id), None, "{id}");
        }
    }

    #[test]
    fn a_session_that_follows_its_own_id_names_the_archived_round() {
        let (_scratch, location) = queue();
        archived(
            &location,
            "review",
            1_000,
            &with(r#""title": "Review, round 1","#),
        );
        put(
            &location,
            "review.json",
            &with(r#""title": "Review, round 2", "follows": "review","#),
        );
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        assert_eq!(follows_title(&app, "review"), Some("Review, round 1"));
    }

    #[test]
    fn a_reloaded_follow_up_looks_its_session_up_again() {
        let (_scratch, location) = queue();
        let follow_up = put(&location, "round-2.json", &with(r#""follows": "round-1","#));
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");
        assert_eq!(follows_title(&app, "round-2"), None);

        archived(
            &location,
            "round-1",
            1_000,
            &with(r#""title": "Round one","#),
        );
        fs::write(
            &follow_up,
            with(r#""follows": "round-1", "title": "Round two","#),
        )
        .expect("written");
        inbox.sync(&mut app).expect("synced");

        assert_eq!(follows_title(&app, "round-2"), Some("Round one"));
    }

    #[cfg(unix)]
    #[test]
    fn a_follow_up_shows_the_id_when_the_archive_cannot_be_read() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, location) = queue();
        archived(
            &location,
            "round-1",
            1_000,
            &with(r#""title": "Round one","#),
        );
        put(&location, "round-2.json", &with(r#""follows": "round-1","#));
        fs::set_permissions(location.archive(), fs::Permissions::from_mode(0o000)).expect("chmod");
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());

        let synced = inbox.sync(&mut app);

        fs::set_permissions(location.archive(), fs::Permissions::from_mode(0o755))
            .expect("chmod back, so the temp dir can go");
        assert_eq!(synced.expect("synced").arrived, ["round-2"]);
        assert_eq!(follows_title(&app, "round-2"), None);
        assert!(app.current_notice().is_none(), "{:?}", app.current_notice());
    }
}
