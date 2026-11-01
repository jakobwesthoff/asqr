// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr queues` (spec section 6.5): the named queues in use, or those
//! taken out of the lists, what each holds and which asqr watches it, with
//! the tmux session that asqr runs in. `asqr queues remove` takes a queue
//! out of the lists (spec section 5.11).

use std::path::Path;

use serde_json::json;

use super::Exit;
use crate::format::is_valid_session_id;
use crate::keep::apply::record_without_copies;
use crate::pairing::{self, Tmux};
use crate::queue::{
    Ended, LocationError, QueueLocation, QueueSummary, Rejection, RemoveError, Removed,
    bundled_results, list_queues, remove_queue,
};

/// The results of a queue: the ones in its outbox and the ones in its
/// month bundles. `QueueSummary` counts the outbox alone, as the TUI asks
/// for it every few seconds and must not open a zip each time; this
/// command asks once. A bundle that cannot be read counts as empty.
fn answered_total(summary: &QueueSummary) -> usize {
    let bundled = bundled_results(&summary.location).map_or(0, |(found, _)| found.len());
    summary.answered + bundled
}

/// Lists the queues in use, or with `removed` the named queues that are
/// not.
pub(super) fn run(root: Option<&Path>, tmux: &dyn Tmux, json: bool, removed: bool) -> Exit {
    let Some(root) = root else {
        eprintln!("{NO_ROOT}");
        return Exit::Failure;
    };
    let summaries: Vec<QueueSummary> = match list_queues(root) {
        Ok(summaries) => summaries
            .into_iter()
            .filter(|summary| summary.in_use() != removed)
            .collect(),
        Err(error) => {
            eprintln!("error: cannot read the queues: {error}");
            return Exit::Failure;
        }
    };
    let sessions = pairing::sessions_of_watchers(
        summaries
            .iter()
            .filter_map(|summary| summary.holder.as_ref()),
        tmux,
    );
    let session = |summary: &QueueSummary| -> Option<String> {
        sessions.of(summary.holder.as_ref()?).map(str::to_owned)
    };

    if json {
        let queues: Vec<_> = summaries
            .iter()
            .map(|summary| {
                json!({
                    "queue": summary.name(),
                    "waiting_total": summary.waiting,
                    "answered_total": answered_total(summary),
                    "watched_by": summary.holder.as_ref().map(|holder| &holder.text),
                    "tmux_session": session(summary),
                })
            })
            .collect();
        println!("{}", serde_json::Value::from(queues));
        return Exit::Success;
    }

    let width = summaries
        .iter()
        .map(|summary| summary.name().len())
        .chain(["QUEUE".len()])
        .max()
        .unwrap_or_default();
    println!("{:<width$}  WAITING  ANSWERED  WATCHED BY", "QUEUE");
    for summary in &summaries {
        let watched_by = match (&summary.holder, session(summary)) {
            (None, _) => "nobody".to_owned(),
            (Some(holder), None) => holder.text.clone(),
            (Some(holder), Some(session)) => format!("{}, tmux session {session}", holder.text),
        };
        println!(
            "{:<width$}  {:>7}  {:>8}  {watched_by}",
            summary.name(),
            summary.waiting,
            answered_total(summary)
        );
    }
    Exit::Success
}

const NO_ROOT: &str = "error: no platform data directory found, so there are no named queues";

/// `asqr queues remove <name> [--force]`: takes the queue `name` out of the
/// lists for new work and says what it did (spec section 5.11).
pub(super) fn run_remove(root: Option<&Path>, tmux: &dyn Tmux, name: &str, force: bool) -> Exit {
    if !is_valid_session_id(name) {
        eprintln!(
            "error: {}",
            LocationError::InvalidQueueName(name.to_owned())
        );
        return Exit::Usage;
    }
    let Some(root) = root else {
        eprintln!("{NO_ROOT}");
        return Exit::Failure;
    };
    let location = QueueLocation::named(root, &spelled(root, name));
    let shown = location.name().expect("a named queue").to_owned();
    let outcome = remove_queue(&location, force);
    // The locks are let go of by now, so the files a rejected session showed
    // can be read for its record without holding up the queue.
    let (rejected, stopped) = match &outcome {
        Ok(Removed::Now { rejected }) => (rejected.as_slice(), false),
        Err(RemoveError::Stopped { rejected, .. }) => (rejected.as_slice(), true),
        _ => (&[][..], false),
    };
    for rejection in rejected {
        record_without_copies(&location, rejection.archived.clone());
        if let Some(line) = how_it_ended(rejection, stopped) {
            eprintln!("{line}");
        }
    }
    match outcome {
        Ok(Removed::Already) => {
            println!("queue {shown} is removed already");
            Exit::Success
        }
        Ok(Removed::Now { rejected }) => {
            let cancelled = rejected
                .iter()
                .filter(|rejection| rejection.how == Ended::Cancelled)
                .count();
            let and_rejected = match cancelled {
                0 => String::new(),
                1 => " and rejected its 1 waiting session".to_owned(),
                n => format!(" and rejected its {n} waiting sessions"),
            };
            println!(
                "removed queue {shown} from the lists{and_rejected}; its finished sessions stay in asqr history"
            );
            Exit::Success
        }
        Err(error) => {
            eprintln!("error: {}", refusal(&error, &shown, root, tmux));
            Exit::Failure
        }
    }
}

/// The queue's name as its directory spells it: on a filesystem that
/// ignores case, `Work` names the directory `work`, and the output says
/// `work`. Only names equal to `name` ignoring ASCII case count, since a
/// symbolic link under another name leads to the same directory and must
/// not stand for it.
fn spelled(root: &Path, name: &str) -> String {
    let given = QueueLocation::named(root, name);
    list_queues(root)
        .unwrap_or_default()
        .into_iter()
        .map(|summary| summary.name().to_owned())
        .find(|listed| {
            listed.eq_ignore_ascii_case(name)
                && given.same_queue(&QueueLocation::named(root, listed))
        })
        .unwrap_or_else(|| name.to_owned())
}

/// The stderr line on what became of a waiting session where the output
/// line does not say it: one ended as recovery ends it, and, when the
/// removal `stopped` at a later session, one it rejected.
fn how_it_ended(rejection: &Rejection, stopped: bool) -> Option<String> {
    let id = &rejection.id;
    match rejection.how {
        Ended::Cancelled if stopped => Some(format!("asqr: rejected {id} before stopping")),
        Ended::Cancelled => None,
        Ended::Completed => Some(format!(
            "asqr: {id} was answered already; its result stays in the outbox"
        )),
        Ended::ArchivedUnanswered => Some(format!(
            "asqr: {id} waited next to an unread result of an earlier session; archived without a result"
        )),
    }
}

/// The error line for `error`, a refused removal of queue `name`.
fn refusal(error: &RemoveError, name: &str, root: &Path, tmux: &dyn Tmux) -> String {
    match error {
        RemoveError::Default => "the default queue cannot be removed".to_owned(),
        RemoveError::NotThere => format!("no queue {name} under {}", root.display()),
        RemoveError::Link => {
            format!("queue {name} is a symbolic link; asqr removes only queue directories")
        }
        // A command names itself in the lock file and lets go of the queue
        // on its own, so there is no asqr to switch or quit.
        RemoveError::Held(holder) if holder.text.contains(", running asqr ") => format!(
            "cannot remove queue {name}: it is busy ({}); try again in a moment",
            holder.text
        ),
        RemoveError::Held(holder) => {
            let sessions = pairing::sessions_of_watchers([holder], tmux);
            let watcher = match sessions.of(holder) {
                Some(session) => format!("{}, tmux session {session}", holder.text),
                None => holder.text.clone(),
            };
            format!(
                "queue {name} is watched by {watcher}; switch that asqr to another queue or quit it first"
            )
        }
        RemoveError::Busy => format!("cannot remove queue {name}: the queue is busy"),
        RemoveError::Waiting(waiting) => {
            let sessions = match waiting {
                1 => "1 session waits".to_owned(),
                n => format!("{n} sessions wait"),
            };
            format!(
                "{sessions} in queue {name}; `asqr queues remove --force {name}` removes it anyway and rejects them"
            )
        }
        RemoveError::Stopped { id, error, .. } => format!(
            "cannot read session {id} of queue {name}: {error}; queue {name} stays in the lists"
        ),
        RemoveError::Io(error) => format!("cannot remove queue {name}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;
    use crate::pairing::testing::Unasked;
    use crate::queue::drop_session;

    const SESSION: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
        "options": [{"id": "a", "label": "A"}]}]}"#;

    #[test]
    fn without_a_data_directory_there_is_no_queue_to_remove() {
        assert_eq!(run_remove(None, &Unasked, "work", false), Exit::Failure);
    }

    #[test]
    fn force_rejects_every_waiting_session() {
        let root = tempfile::tempdir().expect("temp dir");
        let work = QueueLocation::named(root.path(), "work");
        for id in ["a", "b"] {
            drop_session(&work, id, SESSION.as_bytes(), false).expect("dropped");
        }

        assert_eq!(
            run_remove(Some(root.path()), &Unasked, "work", true),
            Exit::Success
        );
        assert!(!work.active_marker().exists());
    }

    #[test]
    fn a_stopped_force_records_and_names_what_it_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir");
        let work = QueueLocation::named(root.path(), "work");
        drop_session(&work, "a", SESSION.as_bytes(), false).expect("dropped");
        let locked = work.inbox().join("b.json");
        std::fs::write(&locked, SESSION).expect("dropped by hand");
        let time = std::fs::metadata(work.inbox().join("a.json"))
            .and_then(|metadata| metadata.modified())
            .expect("a time");
        std::fs::File::options()
            .write(true)
            .open(&locked)
            .and_then(|file| file.set_modified(time))
            .expect("the same drop time, so `a` comes first");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("unreadable");

        let exit = run_remove(Some(root.path()), &Unasked, "work", true);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600))
            .expect("readable again");

        assert_eq!(exit, Exit::Failure);
        assert!(work.outbox().join("a.json").is_file(), "a was rejected");
        assert!(work.active_marker().is_file());
    }

    fn rejection(how: Ended) -> Rejection {
        Rejection {
            id: "first".into(),
            how,
            archived: None,
        }
    }

    #[test]
    fn what_became_of_a_session_is_said_where_the_output_does_not() {
        assert_eq!(how_it_ended(&rejection(Ended::Cancelled), false), None);
        assert_eq!(
            how_it_ended(&rejection(Ended::Cancelled), true).as_deref(),
            Some("asqr: rejected first before stopping")
        );
        assert_eq!(
            how_it_ended(&rejection(Ended::Completed), false).as_deref(),
            Some("asqr: first was answered already; its result stays in the outbox")
        );
        assert_eq!(
            how_it_ended(&rejection(Ended::ArchivedUnanswered), false).as_deref(),
            Some(
                "asqr: first waited next to an unread result of an earlier session; archived without a result"
            )
        );
    }

    #[test]
    fn each_refusal_says_what_to_do() {
        let root = std::path::Path::new("/data/asqr");
        let refused = |error: RemoveError| refusal(&error, "work", root, &Unasked);

        assert_eq!(
            refused(RemoveError::Link),
            "queue work is a symbolic link; asqr removes only queue directories"
        );
        assert_eq!(
            refused(RemoveError::Held(crate::queue::Holder {
                text: "pid 7 on imperius.local".into(),
                tmux: None,
            })),
            "queue work is watched by pid 7 on imperius.local; switch that asqr to another queue or quit it first"
        );
        assert_eq!(
            refused(RemoveError::Busy),
            "cannot remove queue work: the queue is busy"
        );
        assert_eq!(
            refused(RemoveError::Waiting(1)),
            "1 session waits in queue work; `asqr queues remove --force work` removes it anyway and rejects them"
        );
        assert_eq!(
            refused(RemoveError::Stopped {
                id: "b".into(),
                error: io::Error::from(io::ErrorKind::PermissionDenied),
                rejected: vec![],
            }),
            "cannot read session b of queue work: permission denied; queue work stays in the lists"
        );
        assert_eq!(
            refused(RemoveError::Io(io::Error::from(
                io::ErrorKind::PermissionDenied
            ))),
            "cannot remove queue work: permission denied"
        );
    }
}
