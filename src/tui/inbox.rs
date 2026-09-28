// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Keeps the app in step with the queue's files (spec sections 3.4 to 3.8):
//! sessions that arrive, change or disappear in the inbox, invalid files,
//! and the files the app's effects write, drafts and finished sessions.
//!
//! Every sync scans the whole inbox instead of following single watcher
//! events. An inbox holds a handful of files, and a full scan cannot miss
//! a rename or a burst of events the way event bookkeeping can.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use super::{App, AppEffect, Outcome, SessionState};
use crate::format::{Session, SessionResult, now_rfc3339, same_session_id, validate};
use crate::queue::{
    QueueLocation, Recovery, Rejected, Waiting, archive_invalid_stem, finish, load_draft, recover,
    reject_invalid, save_draft, scan_inbox, session_sha256,
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
}

#[derive(Debug)]
pub struct Inbox {
    location: QueueLocation,
    known: Vec<Known>,
    ignored: HashSet<PathBuf>,
}

impl Inbox {
    pub fn new(location: QueueLocation) -> Self {
        Inbox {
            location,
            known: Vec::new(),
            ignored: HashSet::new(),
        }
    }

    fn known(&self, id: &str) -> Option<usize> {
        self.known
            .iter()
            .position(|known| same_session_id(&known.id, id))
    }

    fn forget(&mut self, app: &mut App, id: &str) {
        if let Some(index) = self.known(id) {
            self.known.remove(index);
        }
        app.remove(id);
    }

    /// Brings `app` in line with the inbox. A failure with a single file
    /// becomes a notice; only an unreadable inbox is an error.
    pub fn sync(&mut self, app: &mut App) -> io::Result<Synced> {
        // Recovery runs on every sync, not only at the start: it is also
        // what archives a session dropped by hand next to an unread result
        // under its id (spec section 3.7).
        for recovery in recover(&self.location)? {
            match recovery {
                Recovery::Completed(id) => {
                    tracing::info!(%id, "completed an interrupted finish");
                    self.forget(app, &id);
                }
                Recovery::Conflict(id) => {
                    tracing::warn!(%id, "archived a session that collided with an unread result");
                    self.forget(app, &id);
                    app.notice(format!(
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
                app.notice(format!("cannot archive {}: {error}", path.display()));
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
            self.forget(app, &id);
        }

        let mut synced = Synced::default();
        for waiting in scan.sessions {
            if let Err(error) = self.load(app, &waiting, &mut synced) {
                app.notice(format!("cannot read {}: {error}", waiting.path.display()));
            }
        }
        Ok(synced)
    }

    /// Loads one waiting file into the app, unless the app shows exactly
    /// these bytes already.
    fn load(&mut self, app: &mut App, waiting: &Waiting, synced: &mut Synced) -> io::Result<()> {
        let bytes = match std::fs::read(&waiting.path) {
            // Finished or taken back between the scan and now; the next
            // sync sees it gone.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            other => other?,
        };
        let sha256 = session_sha256(&bytes);
        let index = self.known(&waiting.id);
        if index.is_some_and(|index| self.known[index].sha256 == sha256) {
            return Ok(());
        }

        let session = serde_json::from_slice::<Session>(&bytes)
            .map_err(|error| format!("not a session file: {error}"))
            .and_then(|session| match validate(&session, Some(&waiting.id)) {
                Ok(()) => Ok(session),
                Err(errors) => Err(errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")),
            });
        let session = match session {
            Ok(session) => session,
            Err(message) => {
                self.forget(app, &waiting.id);
                self.reject(app, waiting, &bytes, &message);
                return Ok(());
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
        Ok(())
    }

    fn reject(&mut self, app: &mut App, waiting: &Waiting, bytes: &[u8], message: &str) {
        let id = &waiting.id;
        tracing::warn!(%id, %message, "rejected an invalid session");
        match reject_invalid(
            &self.location,
            &waiting.path,
            bytes,
            message,
            &now_rfc3339(),
        ) {
            Ok(Rejected::ErrorResult) => {
                app.notice(format!(
                    "{id} is invalid and was answered with an error: {message}"
                ));
            }
            Ok(Rejected::Conflict) => app.notice(format!(
                "{id} is invalid and was archived without a result, since an unread result with \
                 that id is in the outbox: {message}"
            )),
            Err(error) => app.notice(format!("cannot reject the invalid {id}: {error}")),
        }
    }

    /// Carries out what a key asked for. Failures become notices, so a
    /// full disk or a vanished file never ends the app with answers lost.
    pub fn apply(&mut self, app: &mut App, effect: AppEffect) -> Applied {
        match effect {
            AppEffect::None => {}
            AppEffect::SaveDraft(draft) => {
                if let Err(error) = save_draft(&self.location, &draft) {
                    app.notice(format!("cannot save the draft of {}: {error}", draft.id));
                }
            }
            AppEffect::Finish { id, outcome } => self.finish(app, &id, outcome),
            AppEffect::Quit => return Applied::Quit,
            AppEffect::OpenImage(path) => return Applied::OpenImage(path),
        }
        Applied::Continue
    }

    fn finish(&mut self, app: &mut App, id: &str, outcome: Outcome) {
        let Some(index) = self.known(id) else {
            return;
        };
        let known = &self.known[index];

        // The result has to answer the bytes the person saw. A file that
        // changed since the last sync is loaded again instead, so the
        // person sees the new version before answering it.
        let current = std::fs::read(&known.path).map(|bytes| session_sha256(&bytes));
        if current.as_ref().ok() != Some(&known.sha256) {
            app.notice(format!(
                "{id} changed on disk and was not sent; check the new version"
            ));
            if let Err(error) = self.sync(app) {
                app.notice(format!("cannot read the inbox: {error}"));
            }
            return;
        }

        let at = now_rfc3339();
        let result = match outcome {
            Outcome::Submitted(answers) => {
                SessionResult::submitted(&known.id, &known.sha256, &at, answers)
            }
            Outcome::Rejected(reason) => {
                SessionResult::cancelled(&known.id, &known.sha256, &at, reason)
            }
        };
        match finish(&self.location, &known.path, &result) {
            Ok(()) => {
                tracing::info!(%id, status = ?result.status, "finished a session");
                self.forget(app, id);
            }
            Err(error) => app.notice(format!("cannot finish {id}: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::format::{Answer, Status};
    use crate::queue::{drop_session, parse_archive_name};

    const SESSION: &str = r#"{"asqr": 1, "questions": [{"id": "q", "text": "?", "kind": "single",
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
    fn an_invalid_session_gets_an_error_result_and_a_notice() {
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
        assert!(
            app.current_notice()
                .is_some_and(|notice| notice.contains("answered with an error")),
            "{:?}",
            app.current_notice()
        );
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
        let unread =
            SessionResult::cancelled("batch", "other bytes", "2026-09-27T17:05:12+02:00", None);
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
        assert!(
            app.current_notice()
                .is_some_and(|notice| notice.contains("unread result")),
            "{:?}",
            app.current_notice()
        );
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

        assert!(
            app.current_notice()
                .is_some_and(|notice| notice.starts_with("cannot save the draft")),
            "{:?}",
            app.current_notice()
        );
    }

    /// Picks the second option, which moves on to the review and its
    /// Submit row, then presses enter on Submit or on Reject below it.
    fn answer_and(app: &mut App, inbox: &mut Inbox, reject: bool) -> Applied {
        let effect = app.handle(key('2'));
        inbox.apply(app, effect);
        if reject {
            app.handle(key('j'));
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
    fn a_reject_finishes_the_session_as_cancelled() {
        let (_scratch, location) = queue();
        put(&location, "batch.json", SESSION);
        let (mut inbox, mut app) = (Inbox::new(location.clone()), App::default());
        inbox.sync(&mut app).expect("synced");

        answer_and(&mut app, &mut inbox, true);

        assert_eq!(result(&location, "batch").status, Status::Cancelled);
        assert!(app.sessions().is_empty());
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
        assert!(
            app.current_notice()
                .is_some_and(|notice| notice.contains("changed on disk")),
            "{:?}",
            app.current_notice()
        );
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
        assert!(
            app.current_notice()
                .is_some_and(|notice| notice.starts_with("cannot finish batch")),
            "{:?}",
            app.current_notice()
        );
    }

    fn notice(app: &App) -> &str {
        app.current_notice().expect("a notice is shown")
    }

    #[test]
    fn a_file_that_cannot_be_archived_becomes_a_notice() {
        let (_scratch, location) = queue();
        put(&location, "bad name.json", SESSION);
        fs::remove_dir(location.archive()).expect("removed");
        fs::write(location.archive(), "in the way").expect("written");
        let (mut inbox, mut app) = (Inbox::new(location), App::default());

        inbox.sync(&mut app).expect("synced");

        assert!(
            notice(&app).starts_with("cannot archive"),
            "{}",
            notice(&app)
        );
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
        assert!(notice(&app).starts_with("cannot read"), "{}", notice(&app));
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

        inbox.reject(&mut app, &waiting, b"{", "broken");

        assert!(
            notice(&app).contains("without a result"),
            "{}",
            notice(&app)
        );
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

        inbox.reject(&mut app, &waiting, b"{", "broken");

        assert!(
            notice(&app).starts_with("cannot reject"),
            "{}",
            notice(&app)
        );
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

        assert!(
            notice(&app).starts_with("cannot read the inbox"),
            "{}",
            notice(&app)
        );
        assert_eq!(names(&location.outbox()), Vec::<String>::new());
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
            outcome: Outcome::Rejected(None),
        };
        assert_eq!(inbox.apply(&mut app, unknown), Applied::Continue);
    }
}
