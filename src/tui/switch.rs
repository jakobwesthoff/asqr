// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Switching the watched queue (ADR 86): what the switcher lists, and the
//! switch itself. A switch takes the new queue's lock, watches its inbox
//! and reads it before it lets go of the old queue, and pins the new queue
//! last; a step that fails leaves asqr on the old queue.

use std::path::Path;

use super::render::{View, shown_path};
use super::{App, Host, Inbox, InboxWatcher, Notice, Place, QueueChoice, QueueRow, TakenQueue};
use super::{Tone, start, watch_inbox};
use crate::pairing::{self, Tmux, TmuxFailure, TmuxPane};
use crate::queue::{LockError, QueueLocation, QueueLock, list_queues, lock_queue, summarize};

/// The queues for the switcher, looked at now: the named queues under
/// `root`, and `dir`, a queue this run was given by `--dir`, first. This
/// asqr watches `watched`. `pane` is asqr's tmux pane where tmux answers,
/// whose session says what a switch does to the agents; `home` shows as
/// `~` in a path.
pub fn queue_choice(
    root: Option<&Path>,
    watched: &QueueLocation,
    dir: Option<&QueueLocation>,
    pane: Option<&TmuxPane>,
    tmux: &dyn Tmux,
    home: Option<&Path>,
) -> QueueChoice {
    let named = root
        .map(|root| list_queues(root).unwrap_or_default())
        .unwrap_or_default();
    let summaries: Vec<_> = dir
        .map(|dir| summarize(dir.clone()))
        .into_iter()
        .chain(named)
        .collect();
    let sessions = pairing::sessions_of_watchers(
        summaries
            .iter()
            .filter_map(|summary| summary.holder.as_ref()),
        tmux,
    );
    let mut rows = Vec::new();
    let mut taken = Vec::new();
    for summary in &summaries {
        let label = match summary.location.name() {
            Some(name) => name.to_owned(),
            None => shown_path(&summary.location.dir().display().to_string(), home),
        };
        let mine = summary.location.same_queue(watched);
        match &summary.holder {
            Some(holder) if !mine => taken.push(TakenQueue {
                label,
                session: sessions.of(holder).map(str::to_owned),
            }),
            _ => rows.push(QueueRow {
                location: summary.location.clone(),
                label,
                waiting: summary.waiting,
                watched: mine,
            }),
        }
    }
    let place = pane
        .and_then(|pane| pairing::ask_session(tmux, pane).ok())
        .map_or(Place::Outside, |session| Place::Tmux(session.name));
    QueueChoice {
        rows,
        taken,
        root: root.map(Path::to_path_buf),
        place,
    }
}

/// A queue ready to be watched: this process holds its lock and watches
/// its inbox, until the value is dropped.
pub struct Held {
    pub location: QueueLocation,
    pub lock: QueueLock,
    pub watcher: InboxWatcher,
}

/// Why a queue cannot be switched to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Another asqr watches it.
    Taken,
    /// Taking its lock, making its directories or watching its inbox
    /// failed, for the reason given.
    Failed(String),
}

/// Takes the lock of `target`, makes its directories, as a queue named on
/// the new-queue row has none yet, and watches its inbox, calling
/// `changed` after each change. The lock names `pane`, asqr's tmux pane.
/// A step that fails gives back what the steps before it took.
pub fn prepare_switch(
    target: &QueueLocation,
    pane: Option<&TmuxPane>,
    changed: impl FnMut() + Send + 'static,
) -> Result<Held, Refusal> {
    let lock = match lock_queue(target, pane) {
        Ok(lock) => lock,
        Err(LockError::Held(_)) => return Err(Refusal::Taken),
        Err(LockError::Io(error)) => return Err(Refusal::Failed(error.to_string())),
    };
    target
        .create_layout()
        .map_err(|error| Refusal::Failed(error.to_string()))?;
    let watcher =
        watch_inbox(target, changed).map_err(|error| Refusal::Failed(error.to_string()))?;
    Ok(Held {
        location: target.clone(),
        lock,
        watcher,
    })
}

/// How a queue is named in the status line and the title: by its name, or
/// by its directory for a queue given by `--dir`.
pub fn queue_label(location: &QueueLocation) -> String {
    match location.name() {
        Some(name) => name.to_owned(),
        None => location.dir().display().to_string(),
    }
}

/// Switches `app`, `inbox` and `view` from the queue they show to `target`
/// (spec section 7.6). Every failure is a notice, and asqr stays on the
/// queue it watched: one another asqr took meanwhile, one whose inbox
/// cannot be watched or read, and a draft of the old queue that cannot be
/// saved, as the old app holds the only copy of its answers.
pub fn switch_queue(
    target: &QueueLocation,
    app: &mut App,
    inbox: &mut Inbox,
    view: &mut View,
    host: &mut impl Host,
) {
    let (new, old) = (queue_label(target), queue_label(inbox.location()));
    let held = match host.prepare(target) {
        Ok(held) => held,
        Err(Refusal::Taken) => {
            app.show(Notice::naming(
                Tone::Error,
                "another asqr is watching queue ",
                &new,
                "",
            ));
            return;
        }
        Err(Refusal::Failed(reason)) => {
            app.show(cannot_watch(&new, &reason));
            return;
        }
    };
    // The new queue is read before the old one is let go of; `held` goes
    // with a failure, and with it the new lock and watcher.
    let (mut new_app, new_inbox) = match start(target.clone()) {
        Ok(started) => started,
        Err(error) => {
            app.show(cannot_watch(&new, &error.root_cause().to_string()));
            return;
        }
    };
    if let Err((id, error)) = inbox.save_drafts(app) {
        tracing::warn!(%id, %error, "could not save a draft before a switch");
        app.show(Notice::naming(
            Tone::Error,
            &format!("cannot save the draft of {id}; still watching "),
            &old,
            "",
        ));
        return;
    }
    let left = app.sessions().len();
    let pinned = host.switch(held);
    new_app.set_elsewhere(host.elsewhere());
    new_app.show(switched_notice(&new, &old, left, pinned));
    *app = new_app;
    *inbox = new_inbox;
    view.set_queue(&new, !target.is_default());
}

/// The notice for a switch to `new` that failed after its lock, for
/// `reason`.
fn cannot_watch(new: &str, reason: &str) -> Notice {
    Notice::naming(
        Tone::Error,
        "cannot watch queue ",
        new,
        &format!(": {reason}"),
    )
}

/// The notice after a switch to `new` from `old`, in which `left` sessions
/// still wait.
fn switched_notice(new: &str, old: &str, left: usize, pinned: Result<(), TmuxFailure>) -> Notice {
    let watching = format!("watching {new} · ");
    if let Err(TmuxFailure(reason)) = pinned {
        tracing::info!(reason, "tmux did not take the pin after a switch");
        return Notice::naming(
            Tone::Warning,
            &format!("{watching}tmux kept the old pin, agents stay on "),
            old,
            "",
        );
    }
    match left {
        0 => Notice::new(Tone::Info, format!("watching {new}")),
        1 => Notice::naming(
            Tone::Info,
            &format!("{watching}1 session waits in "),
            old,
            "",
        ),
        _ => Notice::naming(
            Tone::Info,
            &format!("{watching}{left} sessions wait in "),
            old,
            "",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::queue::lock_holder;

    /// A tmux that names the session of asqr's pane `asqr` and has pane
    /// `%7` in session `ntropy`, and keeps the calls.
    #[derive(Default)]
    struct Sessions {
        calls: RefCell<Vec<String>>,
    }

    impl Tmux for Sessions {
        fn run(&self, _socket: Option<&str>, args: &[&str]) -> Result<String, TmuxFailure> {
            self.calls.borrow_mut().push(args[0].to_owned());
            Ok(match args[0] {
                "display-message" => "asqr\n".to_owned(),
                _ => "%0 asqr\n%7 ntropy\n".to_owned(),
            })
        }
    }

    fn pane(id: &str) -> TmuxPane {
        TmuxPane {
            socket: "/tmp/tmux-501/default".into(),
            pane: Some(id.into()),
        }
    }

    fn queue(root: &Path, name: &str, waiting: usize) -> QueueLocation {
        let location = QueueLocation::named(root, name);
        location.create_layout().expect("layout");
        for n in 0..waiting {
            std::fs::write(location.inbox().join(format!("s{n}.json")), "{}").expect("written");
        }
        location
    }

    #[test]
    fn the_choice_sets_the_taken_queues_apart_with_their_tmux_session() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path();
        let watched = queue(root, "asqr-todos", 2);
        let _mine = lock_queue(&watched, None).expect("locked");
        queue(root, "default", 1);
        let ntropy = queue(root, "ntropy", 0);
        let _theirs = lock_queue(&ntropy, Some(&pane("%7"))).expect("locked");
        let tmux = Sessions::default();

        let choice = queue_choice(Some(root), &watched, None, Some(&pane("%0")), &tmux, None);

        let rows: Vec<_> = choice
            .rows
            .iter()
            .map(|row| (row.label.as_str(), row.waiting, row.watched))
            .collect();
        assert_eq!(rows, [("asqr-todos", 2, true), ("default", 1, false)]);
        assert_eq!(
            choice.taken,
            [TakenQueue {
                label: "ntropy".into(),
                session: Some("ntropy".into())
            }]
        );
        assert_eq!(choice.place, Place::Tmux("asqr".into()));
        assert_eq!(choice.root.as_deref(), Some(root));
    }

    #[test]
    fn outside_tmux_tmux_is_not_asked() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let watched = queue(scratch.path(), "default", 0);

        let choice = queue_choice(
            Some(scratch.path()),
            &watched,
            None,
            None,
            &pairing::testing::Unasked,
            None,
        );

        assert_eq!(choice.place, Place::Outside);
    }

    #[test]
    fn a_tmux_that_does_not_answer_counts_as_outside_tmux() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let watched = queue(scratch.path(), "default", 0);
        let tmux = pairing::testing::StandIn::failing("no server running");

        let choice = queue_choice(
            Some(scratch.path()),
            &watched,
            None,
            Some(&pane("%0")),
            &tmux,
            None,
        );

        assert_eq!(choice.place, Place::Outside);
    }

    #[test]
    fn a_dir_queue_of_this_run_stands_first_by_its_path() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path().join("data");
        let home = scratch.path().join("home");
        let dir = QueueLocation::at(home.join("work/queue"));
        dir.create_layout().expect("layout");
        let watched = queue(&root, "default", 0);

        let choice = queue_choice(
            Some(&root),
            &watched,
            Some(&dir),
            None,
            &pairing::testing::Unasked,
            Some(&home),
        );

        let labels: Vec<_> = choice.rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["~/work/queue", "default"]);
        assert_eq!(choice.rows[0].location, dir);
    }

    #[test]
    fn a_switch_into_a_new_queue_creates_it_and_holds_its_lock() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = QueueLocation::named(scratch.path(), "weekly-review");

        let held = prepare_switch(&target, Some(&pane("%0")), || {}).expect("prepared");

        assert!(target.inbox().is_dir() && target.drafts().is_dir());
        let holder = lock_holder(&target).expect("readable").expect("held");
        assert_eq!(holder.tmux.map(|place| place.pane), Some("%0".into()));
        drop(held);
        assert_eq!(lock_holder(&target).expect("readable"), None);
    }

    #[test]
    fn a_queue_another_asqr_watches_is_refused() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = queue(scratch.path(), "ntropy", 0);
        let _theirs = lock_queue(&target, None).expect("locked");

        assert!(matches!(
            prepare_switch(&target, None, || {}),
            Err(Refusal::Taken)
        ));
    }

    #[test]
    fn a_lock_that_cannot_be_taken_is_a_failure() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = QueueLocation::named(scratch.path(), "blocked");
        std::fs::create_dir_all(scratch.path().join("queues")).expect("dir");
        // A file where the queue's directory belongs.
        std::fs::write(target.dir(), "").expect("file");

        let refused = prepare_switch(&target, None, || {});

        assert!(
            matches!(refused, Err(Refusal::Failed(_))),
            "{:?}",
            refused.err()
        );
    }

    #[test]
    fn a_failed_watch_releases_the_new_lock() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = QueueLocation::named(scratch.path(), "broken");
        std::fs::create_dir_all(target.dir()).expect("dir");
        // A file where the inbox belongs: no layout, no watcher.
        std::fs::write(target.inbox(), "").expect("file");

        let refused = prepare_switch(&target, None, || {});

        assert!(
            matches!(refused, Err(Refusal::Failed(_))),
            "{:?}",
            refused.err()
        );
        assert_eq!(lock_holder(&target).expect("readable"), None);
    }

    #[test]
    fn after_a_switch_the_notice_names_the_sessions_left_behind() {
        let notice = |left| switched_notice("default", "asqr-todos", left, Ok(()));

        assert_eq!(
            notice(2),
            Notice::naming(
                Tone::Info,
                "watching default · 2 sessions wait in ",
                "asqr-todos",
                ""
            )
        );
        assert_eq!(
            notice(1).text,
            "watching default · 1 session waits in asqr-todos"
        );
        assert_eq!(notice(0), Notice::new(Tone::Info, "watching default"));
    }

    #[test]
    fn a_pin_tmux_refused_after_a_switch_is_a_warning() {
        let notice = switched_notice(
            "default",
            "asqr-todos",
            2,
            Err(TmuxFailure("no server running".into())),
        );

        assert_eq!(
            notice,
            Notice::naming(
                Tone::Warning,
                "watching default · tmux kept the old pin, agents stay on ",
                "asqr-todos",
                ""
            )
        );
    }

    #[test]
    fn a_queue_is_named_by_its_name_or_its_directory() {
        assert_eq!(
            queue_label(&QueueLocation::named(Path::new("/data"), "work")),
            "work"
        );
        assert_eq!(queue_label(&QueueLocation::at("/tmp/queue")), "/tmp/queue");
    }
}
