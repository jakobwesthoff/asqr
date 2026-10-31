// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr ask` (spec sections 5.5 and 6.3): validate a session file, make it
//! ready for the inbox and drop it there, then optionally wait for the
//! result.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::Exit;
use super::format::{print_errors, print_warnings, read_session_file, session_dir};
use super::hint::{self, Caller, Invocation, Refusing};
use super::skill_age;
use super::wait::{Elsewhere, elsewhere, wait_for_result};
use crate::format::{
    SessionResult, WarningKind, array_mut, is_valid_session_id, make_context_paths_absolute,
    same_session_id, validate, warnings,
};
use crate::keep::apply::record_after_withdrawal;
use crate::pairing::{self, Tmux, TmuxPane, TmuxSession};
use crate::queue::{
    Carry, DropError, Dropped, Gate, Origin, QueueLocation, ReplaceError, Replaced, WithdrawError,
    Withdrawn, drop_replacing_carrying, drop_session, list_queues, lock_holder,
};
use crate::tui::SessionState;

pub(super) struct AskOptions {
    pub wait: bool,
    pub timeout: Option<Duration>,
    pub force: bool,
    /// The waiting session the new one takes the place of.
    pub replaces: Option<String>,
}

/// What `ask` knows about the queues around its own, for its advice on
/// who watches (ADR 85).
pub(super) struct Surroundings<'a> {
    /// The root of the named queues, to name the watched ones.
    pub root: Option<&'a Path>,
    pub tmux: &'a dyn Tmux,
    pub origin: Origin,
    /// The asker's tmux pane and what tmux said about its session, where
    /// tmux was asked and answered.
    pub tmux_session: Option<(&'a TmuxPane, TmuxSession)>,
    /// The variable that chose the queue: `ASQR_QUEUE` or `ASQR_DIR`.
    pub variable: Option<&'static str>,
}

pub(super) fn run(
    location: &QueueLocation,
    invocation: &Invocation,
    file: &Path,
    options: AskOptions,
    around: &Surroundings<'_>,
) -> Exit {
    skill_age::hint_if_older();
    let mut session_file = match read_session_file(file) {
        Ok(session_file) => session_file,
        Err(error) => return error.report(file),
    };

    // `ask` turns relative paths into absolute ones itself, so only the
    // other warnings concern the asker.
    let found = warnings(
        &session_file.raw,
        &session_file.session,
        session_dir(file).as_deref(),
    );
    let shown: Vec<_> = found
        .iter()
        .filter(|warning| warning.kind != WarningKind::RelativePath)
        .collect();
    print_warnings(shown.iter().copied());
    if let Err(errors) = validate(&session_file.session, None) {
        print_errors(&errors);
        return Exit::ErrorResult;
    }

    let id_in_file = session_file.session.id.is_some();
    let id = session_file
        .session
        .id
        .clone()
        .unwrap_or_else(|| ulid::Ulid::generate().to_string());
    let raw = &mut session_file.raw;
    raw["id"] = Value::from(id.as_str());
    make_context_paths_absolute(raw, file);
    drop_note(raw);
    drop_reserved_names(raw);
    let mut bytes = serde_json::to_vec_pretty(raw).expect("a serde_json Value always serializes");
    bytes.push(b'\n');

    let dropped = match options.replaces.as_deref() {
        None => drop_session(location, &id, &bytes, options.force)
            .map(|dropped| (None, dropped, false))
            .map_err(|error| {
                tracing::warn!(id, %error, "ask refused");
                match error {
                    DropError::UnreadResult(id) => {
                        hint::unread_result(invocation, &id, None).print();
                    }
                    error => eprintln!("error: {error}"),
                }
                Exit::Failure
            }),
        Some(old) => {
            // The person's draft of `old` becomes the draft of the new
            // session, kept to what the new file has a place for.
            let new_session = &session_file.session;
            let carry =
                |draft: &SessionResult| SessionState::carried_draft(&id, new_session, draft);
            let replacement = Replacement {
                old,
                id: &id,
                bytes: &bytes,
                force: options.force,
                carry: &carry,
            };
            replace(location, invocation, around.root, &replacement)
                .map(|(withdrawn, dropped, carried)| (Some(withdrawn), dropped, carried))
        }
    };
    let (withdrawn, carried) = match dropped {
        Ok((withdrawn, dropped, carried)) => {
            tracing::info!(id, ?dropped, carried, "dropped session");
            if let Dropped::Replaced(_) = dropped {
                hint::replaced_waiting(&id).print();
            }
            (withdrawn, carried)
        }
        Err(exit) => return exit,
    };
    advise_on_watchers(location, invocation, around);
    // Keyed on the `draft`, not on `--force`: an earlier forced withdrawal
    // of the old session comes back as `Already` with its draft.
    if let (Some(old), Some(Withdrawn::Now(result) | Withdrawn::Already(result))) =
        (options.replaces.as_deref(), &withdrawn)
        && let Some(line) = hint::started_on(invocation, old, &id, carried, result.draft.is_some())
    {
        line.print();
    }

    if options.wait {
        eprintln!("{id}");
        let exit = wait_for_result(location, invocation, &id, options.timeout);
        if let (Exit::Timeout, Some(timeout)) = (exit, options.timeout) {
            if !shown.is_empty() {
                hint::warnings_above(invocation, shown.len(), file, &id, id_in_file, true).print();
            }
            hint::no_result_yet(invocation, &id, Caller::Wait(timeout.as_secs())).print();
        }
        exit
    } else {
        println!("{id}");
        if shown.is_empty() {
            hint::wait_for_it(invocation, &id).print();
        } else {
            hint::warnings_above(invocation, shown.len(), file, &id, id_in_file, false).print();
        }
        Exit::Success
    }
}

/// What `ask --replaces` puts in place of the session it withdraws.
struct Replacement<'a> {
    /// The waiting session withdrawn.
    old: &'a str,
    /// The id of the new session.
    id: &'a str,
    /// The new session file as dropped into the inbox.
    bytes: &'a [u8],
    force: bool,
    /// Makes the draft of the new session out of that of `old`.
    carry: Carry<'a>,
}

/// `ask --replaces <old>` (spec section 5.5): withdraws `old` and drops the
/// new session `id` in one step. Its refusals exit as those of `ask` and
/// `withdraw` do.
fn replace(
    location: &QueueLocation,
    invocation: &Invocation,
    root: Option<&Path>,
    replacement: &Replacement<'_>,
) -> Result<(Withdrawn, Dropped, bool), Exit> {
    let Replacement {
        old,
        id,
        bytes,
        force,
        carry,
    } = *replacement;
    // `ask` replaces a waiting session of the same id by itself; naming it
    // again would withdraw the session the new file is to replace.
    if same_session_id(old, id) {
        eprintln!(
            "error: --replaces names the session's own id; ask replaces a waiting session of the same id without it"
        );
        return Err(Exit::Usage);
    }
    match drop_replacing_carrying(
        location,
        old,
        id,
        bytes,
        force,
        Some(carry),
        Gate::default(),
    ) {
        Ok(Replaced {
            withdrawn,
            dropped,
            archived,
            carried,
        }) => {
            record_after_withdrawal(location, archived);
            // The result holds what the person typed, so the log gets its
            // status and counts only (ADR 90).
            let (outcome, result) = match &withdrawn {
                Withdrawn::Now(result) => ("now", result),
                Withdrawn::Already(result) => ("already", result),
            };
            tracing::info!(
                id,
                old,
                outcome,
                status = ?result.status,
                answers = result.answers.len(),
                draft_answers = result.draft.as_ref().map_or(0, |draft| draft.answers.len()),
                carried,
                "replaced a session"
            );
            Ok((withdrawn, dropped, carried))
        }
        Err(error) => {
            tracing::warn!(id, old, %error, "ask --replaces refused");
            Err(match error {
                // The old session may wait in a queue the person no longer
                // watches; the new one belongs where they watch now, so it
                // is not dropped there with it.
                ReplaceError::Withdraw(WithdrawError::Unknown(old)) => {
                    match elsewhere(location, root, &old) {
                        Elsewhere::Nowhere => {
                            hint::unknown_session(invocation, &old).print();
                            Exit::UnknownId
                        }
                        Elsewhere::One(found) => {
                            let found = found.name().unwrap_or_default().to_owned();
                            hint::replaces_elsewhere(invocation, &old, &found).print();
                            Exit::UnknownId
                        }
                        Elsewhere::Several(queues) => {
                            hint::in_several_queues(&old, &queues).print();
                            Exit::Failure
                        }
                    }
                }
                ReplaceError::Withdraw(error @ WithdrawError::ResultGone(_)) => {
                    eprintln!("error: {error}");
                    Exit::UnknownId
                }
                ReplaceError::Withdraw(WithdrawError::BeingAnswered(old)) => {
                    hint::being_answered(invocation, &old, Refusing::AskReplaces).print();
                    Exit::Failure
                }
                ReplaceError::Withdraw(WithdrawError::Io(error)) => {
                    eprintln!("error: cannot withdraw {old}: {error}");
                    Exit::Failure
                }
                ReplaceError::Drop(DropError::UnreadResult(id)) => {
                    hint::unread_result(invocation, &id, Some(old)).print();
                    Exit::Failure
                }
                error => {
                    eprintln!("error: {error}");
                    Exit::Failure
                }
            })
        }
    }
}

/// Removes `note`, whatever its value, from every question and info item.
/// Every question takes a note (ADR 68) and this asqr ignores the field, but
/// a released asqr watching the queue reads it as a boolean: it turns the note
/// off for `false` and refuses a session where it is no boolean.
// TODO: goes with the `note` warning once asqr moves to a format version 3
// (see `note_fields` in `format/warnings.rs`).
fn drop_note(raw: &mut Value) {
    for question in array_mut(raw, "questions") {
        if let Some(fields) = question.as_object_mut() {
            fields.remove("note");
        }
    }
}

/// Removes `archived` and every `shown` from the session. asqr writes them
/// into an archived session to record the files it showed (spec section
/// 5.1), and reads them back when the session is replayed. A session an
/// asker brings along must not carry a record the archive never made.
fn drop_reserved_names(raw: &mut Value) {
    if let Some(fields) = raw.as_object_mut() {
        fields.remove("archived");
    }
    for question in array_mut(raw, "questions") {
        drop_shown_in_context(question);
        for option in array_mut(question, "options") {
            drop_shown_in_context(option);
        }
    }
}

/// Removes `shown` from the entries in the `context` of a question or
/// option, and from both sides of a diff.
fn drop_shown_in_context(holder: &mut Value) {
    for entry in array_mut(holder, "context") {
        remove_shown(entry);
        for side in ["old", "new"] {
            if let Some(side) = entry.get_mut(side) {
                remove_shown(side);
            }
        }
    }
}

fn remove_shown(object: &mut Value) {
    if let Some(fields) = object.as_object_mut() {
        fields.remove("shown");
    }
}

/// What the asker should know about who watches its queue (spec section
/// 5.5): that nobody does, and which queues other asqr instances watch;
/// that the queue tmux chose is watched from another tmux session; that
/// its variable chose another queue than its tmux session's pin. The
/// session waits in the queue in every case.
fn advise_on_watchers(
    location: &QueueLocation,
    invocation: &Invocation,
    around: &Surroundings<'_>,
) {
    let from_tmux = matches!(around.origin, Origin::Pin | Origin::SessionName);
    let own = around.tmux_session.as_ref();
    match lock_holder(location) {
        Ok(None) => {
            let watched = around
                .root
                .map(|root| watched_queues(root, location, around.tmux))
                .unwrap_or_default();
            let session = own
                .filter(|_| from_tmux)
                .map(|(_, session)| session.name.as_str());
            hint::nobody_watching(invocation, &watched, session).print();
        }
        // Two tmux sessions in one queue: names that turn into one queue
        // name, or a pin left by an asqr that has quit.
        Ok(Some(holder)) if from_tmux => {
            if let (Some((pane, session)), Some(place)) = (own, &holder.tmux)
                && place.socket == pane.socket
                && let Some(watcher) =
                    pairing::sessions_of_watchers([&holder], around.tmux).of(&holder)
                && watcher != session.name
            {
                hint::watched_from_another_session(invocation, watcher, &session.name).print();
            }
        }
        _ => {}
    }
    if let (Some(variable), Some((_, session)), Some(root)) = (around.variable, own, around.root)
        && let Some(pinned) = session
            .pinned
            .as_deref()
            .filter(|pinned| is_valid_session_id(pinned))
    {
        let pinned_queue = QueueLocation::named(root, pinned);
        if !location.same_queue(&pinned_queue) {
            let watched = matches!(lock_holder(&pinned_queue), Ok(Some(_)));
            hint::environment_beside_pin(variable, invocation, pinned, watched).print();
        }
    }
}

/// The named queues under `root` other asqr instances watch, `location`
/// left out, with the tmux sessions they run in.
fn watched_queues(
    root: &Path,
    location: &QueueLocation,
    tmux: &dyn Tmux,
) -> Vec<hint::WatchedQueue> {
    let watched: Vec<_> = list_queues(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|summary| summary.holder.is_some() && !summary.location.same_queue(location))
        .collect();
    let sessions = pairing::sessions_of_watchers(
        watched.iter().filter_map(|summary| summary.holder.as_ref()),
        tmux,
    );
    watched
        .iter()
        .map(|summary| hint::WatchedQueue {
            name: summary.name().to_owned(),
            session: summary
                .holder
                .as_ref()
                .and_then(|holder| sessions.of(holder))
                .map(str::to_owned),
        })
        .collect()
}
