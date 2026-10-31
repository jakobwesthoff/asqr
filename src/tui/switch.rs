// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Switching the watched queue (ADR 86): what the switcher lists, and the
//! switch itself. A switch takes the new queue's lock, watches its inbox
//! and reads it before it lets go of the old queue, and pins the new queue
//! last; a step that fails leaves asqr on the old queue.

use std::path::{Path, PathBuf};

use super::render::{View, shown_path};
use super::{App, Host, Inbox, InboxWatcher, Notice, Place, QueueChoice, QueueRow, TakenQueue};
use super::{Tone, start, watch_inbox};
use crate::cli::shell_word;
use crate::pairing::{self, Tmux, TmuxFailure, TmuxPane};
use crate::queue::{
    LockError, QueueLocation, QueueLock, compact_quietly, list_queues, lock_queue, summarize,
};
use crate::settings::SettingsStore;

/// The queues for the switcher, looked at now: the named queues under
/// `root`, and `dir`, a queue this run was given by `--dir`, first. This
/// asqr watches `watched`, and none while the start picker shows. `pane` is asqr's tmux pane where tmux answers,
/// whose session says what a switch does to the agents; `home` shows as
/// `~` in a path.
pub fn queue_choice(
    root: Option<&Path>,
    watched: Option<&QueueLocation>,
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
        let (label, agent_dir) = match summary.location.name() {
            Some(name) => (name.to_owned(), None),
            None => {
                let dir = summary.location.dir();
                (
                    absolute_with_home_as_tilde(dir, home),
                    Some(agent_dir_word(dir, home)),
                )
            }
        };
        let mine = watched.is_some_and(|watched| summary.location.same_queue(watched));
        match &summary.holder {
            Some(holder) if !mine => taken.push(TakenQueue {
                label,
                session: sessions.of(holder).map(str::to_owned),
            }),
            _ => rows.push(QueueRow {
                location: summary.location.clone(),
                label,
                agent_dir,
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
    // The asqr that shows a queue bundles its old entries (spec section
    // 5.9); a failure is logged and never stops the switch.
    compact_quietly(target, &lock);
    let watcher =
        watch_inbox(target, changed).map_err(|error| Refusal::Failed(watch_failure(error)))?;
    Ok(Held {
        location: target.clone(),
        lock,
        watcher,
    })
}

/// A queue picked at the start, ready to be watched: held, and read for
/// the first time.
pub struct Taken {
    pub held: Held,
    pub app: App,
    pub inbox: Inbox,
}

/// Takes `target`, picked in the start picker, as a switch takes its
/// queue (see [`prepare_switch`]), and reads it. What fails is the notice
/// the picker shows; nothing is held then.
pub fn take_picked(
    target: &QueueLocation,
    settings: &SettingsStore,
    changed: impl FnMut() + Send + 'static,
) -> Result<Taken, Notice> {
    // The picker lists named queues, whose label does not use the home
    // folder.
    let name = queue_label(target, None);
    let held = prepare_switch(target, None, changed).map_err(|refusal| refused(&name, refusal))?;
    // A failed read drops `held`, and with it the lock and the watcher.
    let (app, inbox) = start(target.clone(), settings.clone())
        .map_err(|error| cannot_watch(&name, &error.root_cause().to_string()))?;
    Ok(Taken { held, app, inbox })
}

/// The notice for a queue named `name` that `refusal` keeps asqr from.
fn refused(name: &str, refusal: Refusal) -> Notice {
    match refusal {
        Refusal::Taken => {
            Notice::naming_queue(Tone::Error, "another asqr is watching queue ", name, "")
        }
        Refusal::Failed(reason) => cannot_watch(name, &reason),
    }
}

/// Why the inbox cannot be watched, as the status line says it: the cause
/// alone, without the paths notify lists after it.
fn watch_failure(error: notify_debouncer_full::notify::Error) -> String {
    // notify writes the paths it has after the cause, and without any the
    // cause alone, whatever its kind.
    error.set_paths(Vec::new()).to_string()
}

/// How a queue is named in the status line and the title: by its name, or
/// by its absolute directory, with `~` for `home`, for a queue given by
/// `--dir`.
pub fn queue_label(location: &QueueLocation, home: Option<&Path>) -> String {
    match location.name() {
        Some(name) => name.to_owned(),
        None => absolute_with_home_as_tilde(location.dir(), home),
    }
}

/// `dir` as an absolute path, for showing to the person: relative to the
/// current directory where it is relative, without a trailing slash, and
/// not resolved through symlinks.
///
/// `std::path::absolute` fails only for an empty path, which clap refuses,
/// or a current directory that cannot be read. The directory as given is
/// the best name left then.
fn absolute_dir(dir: &Path) -> PathBuf {
    let absolute = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    // `absolute` keeps a trailing slash, which would make `--dir ./` from
    // the home folder show as `~/` and `--dir .` as the full path.
    absolute.components().collect()
}

/// `dir` made absolute, with the home folder as `~`, as a queue given by
/// `--dir` is named wherever the person sees it.
pub(crate) fn absolute_with_home_as_tilde(dir: &Path, home: Option<&Path>) -> String {
    let absolute = absolute_dir(dir).display().to_string();
    match home_as_tilde(&absolute, home) {
        // `shown_path` leaves the home folder itself alone, as it has no
        // `/` behind it.
        HomeForm::Itself => "~".to_owned(),
        HomeForm::Inside(shown) | HomeForm::Outside(shown) => shown,
    }
}

/// Where an absolute path stands to the home folder.
enum HomeForm {
    /// The home folder itself.
    Itself,
    /// Inside it, as `~/rest`.
    Inside(String),
    /// Not inside it, the path unchanged.
    Outside(String),
}

fn home_as_tilde(absolute: &str, home: Option<&Path>) -> HomeForm {
    let trimmed = home
        .and_then(Path::to_str)
        .map(|home| home.trim_end_matches('/'));
    if trimmed.is_some_and(|home| !home.is_empty() && home == absolute) {
        return HomeForm::Itself;
    }
    let shown = shown_path(absolute, home);
    match shown.starts_with("~/") {
        true => HomeForm::Inside(shown),
        false => HomeForm::Outside(shown),
    }
}

/// The word after `--dir` that tells an agent the queue in `dir`, to be
/// pasted into a shell: the path as `cli::hint` writes one, and with `~/`
/// in front of the quoted rest where the path is inside the home folder.
/// The shell expands `~/` only unquoted, so a quote around the whole path
/// would keep the tilde from expanding.
pub(crate) fn agent_dir_word(dir: &Path, home: Option<&Path>) -> String {
    const UNWRITABLE: &str = "<dir>";
    let absolute = absolute_dir(dir);
    let Some(text) = absolute.to_str() else {
        return UNWRITABLE.to_owned();
    };
    match home_as_tilde(text, home) {
        HomeForm::Itself => "~".to_owned(),
        HomeForm::Inside(shown) => match shell_word(Path::new(&shown["~/".len()..]), UNWRITABLE) {
            word if word == UNWRITABLE => word,
            word => format!("~/{word}"),
        },
        HomeForm::Outside(_) => shell_word(&absolute, UNWRITABLE),
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
    let home = host.home().map(Path::to_path_buf);
    let (new, old) = (
        queue_label(target, home.as_deref()),
        queue_label(inbox.location(), home.as_deref()),
    );
    let held = match host.prepare(target) {
        Ok(held) => held,
        Err(refusal) => {
            app.show(refused(&new, refusal));
            return;
        }
    };
    // The new queue is read before the old one is let go of; `held` goes
    // with a failure, and with it the new lock and watcher.
    let (mut new_app, new_inbox) = match start(target.clone(), app.settings().clone()) {
        Ok(started) => started,
        Err(error) => {
            app.show(cannot_watch(&new, &error.root_cause().to_string()));
            return;
        }
    };
    if let Err((id, error)) = inbox.save_drafts(app) {
        tracing::warn!(%id, %error, "could not save a draft before a switch");
        app.show(Notice::naming_queue_after(
            Tone::Error,
            "cannot save the draft of ",
            &id,
            "; still watching ",
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
    Notice::naming_queue(
        Tone::Error,
        "cannot watch queue ",
        new,
        &format!(": {reason}"),
    )
}

/// The notice after a switch to `new` from `old`, in which `left` sessions
/// still wait.
fn switched_notice(new: &str, old: &str, left: usize, pinned: Result<(), TmuxFailure>) -> Notice {
    // The queue switched to and the one left both have names of up to 200
    // bytes; where the line is too narrow, the first gives way.
    let watching =
        |tone, before: &str| Notice::naming_queue_after(tone, "watching ", new, before, old, "");
    if let Err(TmuxFailure(reason)) = pinned {
        tracing::info!(reason, "tmux did not take the pin after a switch");
        return watching(Tone::Warning, " · tmux kept the old pin, agents stay on ");
    }
    match left {
        0 => Notice::naming_queue(Tone::Info, "watching ", new, ""),
        1 => watching(Tone::Info, " · 1 session waits in "),
        _ => watching(Tone::Info, &format!(" · {left} sessions wait in ")),
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

        let choice = queue_choice(
            Some(root),
            Some(&watched),
            None,
            Some(&pane("%0")),
            &tmux,
            None,
        );

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
            Some(&watched),
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
            Some(&watched),
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
            Some(&watched),
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
            Notice::naming_queue_after(
                Tone::Info,
                "watching ",
                "default",
                " · 2 sessions wait in ",
                "asqr-todos",
                ""
            )
        );
        assert_eq!(
            notice(1),
            Notice::naming_queue_after(
                Tone::Info,
                "watching ",
                "default",
                " · 1 session waits in ",
                "asqr-todos",
                ""
            )
        );
    }

    /// With nobody left behind, the queue switched to is the one name, and
    /// a long one ends in `…` as in the other notices of a switch.
    #[test]
    fn a_switch_that_leaves_nobody_behind_names_the_new_queue() {
        let notice = switched_notice("default", "asqr-todos", 0, Ok(()));

        assert_eq!(
            notice,
            Notice::naming_queue(Tone::Info, "watching ", "default", "")
        );
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
            Notice::naming_queue_after(
                Tone::Warning,
                "watching ",
                "default",
                " · tmux kept the old pin, agents stay on ",
                "asqr-todos",
                ""
            )
        );
    }

    /// An archived session of 2020 in `target`, which is long due.
    fn old_loose_session(target: &QueueLocation) -> std::path::PathBuf {
        target.create_layout().expect("layout");
        let name = crate::queue::archive_name(
            "old",
            ulid::Ulid::from_parts(1_600_000_000_000, 1),
            crate::queue::Archived::Session,
        );
        let path = target.archive().join(name);
        std::fs::write(&path, "{}").expect("written");
        path
    }

    #[test]
    fn a_switch_compacts_the_queue_it_takes() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = QueueLocation::named(scratch.path(), "old-work");
        let old = old_loose_session(&target);

        let held = prepare_switch(&target, None, || {}).expect("prepared");

        assert!(!old.exists(), "bundled");
        assert_eq!(crate::queue::list_months(&target).expect("listed").len(), 1);
        drop(held);
    }

    #[test]
    fn a_picked_queue_is_compacted() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = QueueLocation::named(scratch.path(), "old-work");
        let old = old_loose_session(&target);

        let taken =
            take_picked(&target, &SettingsStore::default(), || {}).map_err(|notice| notice.text);

        assert!(taken.is_ok());
        assert!(!old.exists(), "bundled");
    }

    #[test]
    fn a_picked_queue_is_held_and_read() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = QueueLocation::named(scratch.path(), "work");

        let taken =
            take_picked(&target, &SettingsStore::default(), || {}).map_err(|notice| notice.text);

        let taken = taken.expect("taken");
        assert_eq!(taken.held.location, target);
        assert_eq!(taken.inbox.location(), &target);
        assert!(lock_holder(&target).expect("readable").is_some());
    }

    #[test]
    fn a_picked_queue_is_watched_with_the_settings_of_the_run() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = QueueLocation::named(scratch.path(), "work");
        let config = scratch.path().join("settings.toml");
        std::fs::write(&config, "context = \"below\"\n").expect("written");
        let settings = SettingsStore::load(Some(config), None).store;

        let taken = take_picked(&target, &settings, || {}).expect("taken");

        assert_eq!(taken.app.settings(), &settings);
    }

    #[test]
    fn a_picked_queue_another_asqr_took_is_a_notice() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = queue(scratch.path(), "work", 0);
        let _theirs = lock_queue(&target, None).expect("locked");

        let refused = take_picked(&target, &SettingsStore::default(), || {}).err();

        assert_eq!(
            refused,
            Some(Notice::naming_queue(
                Tone::Error,
                "another asqr is watching queue ",
                "work",
                ""
            ))
        );
    }

    #[test]
    fn a_picked_queue_that_cannot_be_watched_is_a_notice_and_holds_nothing() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = QueueLocation::named(scratch.path(), "broken");
        std::fs::create_dir_all(target.dir()).expect("dir");
        std::fs::write(target.inbox(), "").expect("a file where the inbox belongs");

        let refused = take_picked(&target, &SettingsStore::default(), || {})
            .err()
            .map(|notice| notice.text);

        assert!(
            refused
                .as_deref()
                .is_some_and(|text| text.starts_with("cannot watch queue broken: ")),
            "{refused:?}"
        );
        assert_eq!(lock_holder(&target).expect("readable"), None);
    }

    #[test]
    fn a_picked_queue_that_cannot_be_read_is_a_notice_and_holds_nothing() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = tempfile::tempdir().expect("temp dir");
        let target = queue(scratch.path(), "work", 0);
        std::fs::set_permissions(target.inbox(), std::fs::Permissions::from_mode(0o000))
            .expect("permissions change");

        let refused = take_picked(&target, &SettingsStore::default(), || {})
            .err()
            .map(|notice| notice.text);
        std::fs::set_permissions(target.inbox(), std::fs::Permissions::from_mode(0o755))
            .expect("permissions back");

        assert!(
            refused
                .as_deref()
                .is_some_and(|text| text.starts_with("cannot watch queue work: ")),
            "{refused:?}"
        );
        assert_eq!(lock_holder(&target).expect("readable"), None);
    }

    #[test]
    fn a_watch_failure_names_its_cause_without_the_paths() {
        use notify_debouncer_full::notify;

        let refused = notify::Error::io(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            .add_path(std::path::PathBuf::from("/tmp/queue/inbox"));

        assert_eq!(
            watch_failure(refused),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied).to_string()
        );
    }

    #[test]
    fn a_queue_is_named_by_its_name_or_its_directory() {
        assert_eq!(
            queue_label(&QueueLocation::named(Path::new("/data"), "work"), None),
            "work"
        );
        assert_eq!(
            queue_label(&QueueLocation::at("/tmp/queue"), None),
            "/tmp/queue"
        );
    }

    // Home folders in these tests are made-up paths: nothing reads the
    // real one, and none of the paths has to exist.
    fn shown(dir: &str, home: Option<&str>) -> String {
        absolute_with_home_as_tilde(Path::new(dir), home.map(Path::new))
    }

    #[test]
    fn a_relative_dir_is_named_by_its_absolute_path() {
        let here = std::env::current_dir().expect("a current directory");

        assert_eq!(
            queue_label(&QueueLocation::at(".asqr"), None),
            here.join(".asqr").display().to_string()
        );
    }

    #[test]
    fn a_dir_inside_home_is_named_with_a_tilde() {
        assert_eq!(
            queue_label(
                &QueueLocation::at("/h/me/work/.asqr"),
                Some(Path::new("/h/me"))
            ),
            "~/work/.asqr"
        );
    }

    /// `/h/me2` starts with the letters of `/h/me` and is another folder.
    #[test]
    fn a_folder_that_only_starts_like_home_keeps_its_path() {
        assert_eq!(shown("/h/me2/q", Some("/h/me")), "/h/me2/q");
    }

    #[test]
    fn a_dir_equal_to_home_is_a_tilde() {
        assert_eq!(shown("/h/me", Some("/h/me")), "~");
    }

    #[test]
    fn without_a_home_the_absolute_path_shows() {
        assert_eq!(shown("/h/me/q", None), "/h/me/q");
    }

    #[test]
    fn a_trailing_slash_is_dropped() {
        assert_eq!(shown("/h/me/q/", Some("/h/me")), "~/q");
    }

    #[cfg(unix)]
    #[test]
    fn a_dir_that_is_no_utf_8_shows_with_a_replacement_character() {
        use std::os::unix::ffi::OsStrExt;

        let latin1 = Path::new(std::ffi::OsStr::from_bytes(b"/h/me/j\xe4"));

        assert_eq!(
            absolute_with_home_as_tilde(latin1, Some(Path::new("/h/me"))),
            "~/j\u{fffd}"
        );
    }

    fn told(dir: &str, home: Option<&str>) -> String {
        agent_dir_word(Path::new(dir), home.map(Path::new))
    }

    #[test]
    fn a_plain_path_under_home_is_told_with_a_tilde() {
        assert_eq!(told("/h/me/work/q", Some("/h/me")), "~/work/q");
    }

    /// The shell expands `~/` only unquoted, so the quotes cover the rest.
    #[test]
    fn a_path_with_a_space_under_home_is_told_with_a_tilde() {
        assert_eq!(told("/h/me/my q", Some("/h/me")), "~/'my q'");
    }

    #[test]
    fn a_path_with_a_quote_is_told_quoted() {
        assert_eq!(told("/h/me/it's", Some("/h/me")), r"~/'it'\''s'");
    }

    #[test]
    fn a_path_outside_home_is_told_quoted_and_absolute() {
        assert_eq!(told("/srv/my q", Some("/h/me")), "'/srv/my q'");
        assert_eq!(told("/srv/q", None), "/srv/q");
    }

    #[test]
    fn home_itself_is_told_as_a_tilde() {
        assert_eq!(told("/h/me", Some("/h/me")), "~");
    }

    /// A newline cannot be written into a command a person pastes.
    #[test]
    fn a_path_with_a_newline_under_home_is_told_as_a_placeholder() {
        assert_eq!(told("/h/me/a\nb", Some("/h/me")), "<dir>");
    }

    /// `std::path::absolute` refuses an empty path, which clap never lets
    /// through.
    #[test]
    fn an_empty_dir_keeps_the_name_it_has() {
        assert_eq!(shown("", Some("/h/me")), "");
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_no_utf_8_is_told_as_a_placeholder() {
        use std::os::unix::ffi::OsStrExt;

        let latin1 = Path::new(std::ffi::OsStr::from_bytes(b"/h/me/j\xe4"));

        assert_eq!(agent_dir_word(latin1, Some(Path::new("/h/me"))), "<dir>");
    }

    #[test]
    fn a_relative_dir_row_is_labelled_absolute() {
        let name = "no-such-queue-01M660F3";
        let here = std::env::current_dir().expect("a current directory");
        let dir = QueueLocation::at(name);

        let choice = queue_choice(
            None,
            None,
            Some(&dir),
            None,
            &pairing::testing::Unasked,
            None,
        );

        let row = &choice.rows[0];
        let expected = here.join(name).display().to_string();
        assert_eq!(row.label, expected);
        assert_eq!(row.agent_dir.as_deref(), Some(expected.as_str()));
    }
}
