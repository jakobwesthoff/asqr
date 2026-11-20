// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Watching the inbox (spec section 5.4). The watcher only says that
//! something changed; [`Inbox::sync`](super::Inbox::sync) then works out
//! what, so events carry no paths.
//!
//! On macOS notify watches through kqueue (ADR 113). A kqueue watch on the
//! inbox directory reports files that come, go or are renamed, but not a
//! file rewritten in place, so every file in the inbox gets a watch of its
//! own as well: one file descriptor per waiting session.

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use notify_debouncer_full::notify::{self, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::queue::QueueLocation;

/// Watches the inbox for as long as it lives.
pub struct InboxWatcher {
    signals: mpsc::Sender<Signal>,
    thread: Option<JoinHandle<()>>,
}

/// What the thread that owns the watches is told.
enum Signal {
    /// The debouncer reported a burst of changes.
    Changed,
    /// The [`InboxWatcher`] is dropped.
    Stop,
}

/// Starts watching the inbox of `location` and calls `changed` after each
/// burst of changes. Register it before the first scan (spec section 5.4).
pub fn watch_inbox(
    location: &QueueLocation,
    mut changed: impl FnMut() + Send + 'static,
) -> notify::Result<InboxWatcher> {
    let inbox = location.inbox();
    let (signals, received) = mpsc::channel();
    let from_debouncer = signals.clone();
    // An `ask` writes a temp file and renames it, several events for one
    // drop; the debouncer turns them into a single sync.
    let mut debouncer = new_debouncer(DEBOUNCE, None, move |events: DebounceEventResult| {
        // A watcher error may mean missed events; a sync catches up on
        // whatever happened, so it is logged and treated as a change.
        if let Err(errors) = events {
            tracing::warn!(?errors, "the inbox watcher reported errors");
        }
        let _ = from_debouncer.send(Signal::Changed);
    })?;
    debouncer.watch(&inbox, RecursiveMode::NonRecursive)?;
    let mut files = FileWatches::default();
    files.sync(&mut debouncer, &inbox);

    // The file watches change with the inbox, and only the owner of the
    // debouncer can change them, so a thread owns it. Each burst first
    // watches the files that arrived and then reports: a file rewritten
    // after its watch is reported again, one rewritten before is read by
    // the sync this report starts.
    let thread = std::thread::Builder::new()
        .name("asqr inbox watcher".to_owned())
        .spawn(move || {
            while let Ok(Signal::Changed) = received.recv() {
                files.sync(&mut debouncer, &inbox);
                changed();
            }
        })
        .map_err(notify::Error::io)?;
    Ok(InboxWatcher {
        signals,
        thread: Some(thread),
    })
}

impl Drop for InboxWatcher {
    /// Waits for the thread, so the watches and their file descriptors are
    /// gone once the watcher is.
    fn drop(&mut self) {
        let _ = self.signals.send(Signal::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How long the watcher waits for a burst of changes to end.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Adds and removes the watch of a single file.
trait FileWatcher {
    fn watch_file(&mut self, path: &Path) -> notify::Result<()>;
    fn unwatch_file(&mut self, path: &Path) -> notify::Result<()>;
}

impl FileWatcher for Debouncer<RecommendedWatcher, RecommendedCache> {
    fn watch_file(&mut self, path: &Path) -> notify::Result<()> {
        self.watch(path, RecursiveMode::NonRecursive)
    }

    fn unwatch_file(&mut self, path: &Path) -> notify::Result<()> {
        self.unwatch(path)
    }
}

/// The files of the inbox that have a watch of their own.
#[derive(Default)]
struct FileWatches {
    /// Each watched file with the device and inode it had when it was
    /// listed. A file that a rename replaced has the same name and another
    /// inode, and the old watch stays on the old inode.
    watched: HashMap<PathBuf, (u64, u64)>,
}

impl FileWatches {
    /// Watches every file in `inbox` that has no watch on its current
    /// inode, and gives up the watches of files that left. A file that
    /// cannot be watched, mostly one that left between listing and
    /// watching, is tried again at the next sync.
    fn sync(&mut self, watcher: &mut impl FileWatcher, inbox: &Path) {
        let entries = match std::fs::read_dir(inbox) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, "the inbox could not be listed to watch its files");
                return;
            }
        };
        // Only regular files: opening anything else for a watch could block,
        // a FIFO until a writer comes.
        let present: HashMap<PathBuf, (u64, u64)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                metadata
                    .is_file()
                    .then(|| (entry.path(), (metadata.dev(), metadata.ino())))
            })
            .collect();

        self.watched.retain(|path, inode| {
            if present.get(path) == Some(inode) {
                return true;
            }
            // notify's kqueue backend drops the watch of a file that was
            // removed or renamed itself, so a failure here is expected.
            if let Err(error) = watcher.unwatch_file(path) {
                tracing::debug!(%error, path = %path.display(), "no watch to give up");
            }
            false
        });
        for (path, inode) in present {
            if self.watched.contains_key(&path) {
                continue;
            }
            match watcher.watch_file(&path) {
                Ok(()) => {
                    self.watched.insert(path, inode);
                }
                Err(error) => {
                    tracing::debug!(%error, path = %path.display(), "an inbox file is not watched");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How long a change may take to be reported.
    const REPORT_WAIT: Duration = Duration::from_secs(10);

    /// Waits for the report of `change`, then until the watcher has been
    /// quiet for a few debounce periods, so that a late report of the same
    /// burst is not taken for the report of the next change.
    fn reported(receiver: &mpsc::Receiver<()>, change: &str) {
        receiver
            .recv_timeout(REPORT_WAIT)
            .unwrap_or_else(|_| panic!("{change} is not reported"));
        while receiver.recv_timeout(DEBOUNCE * 4).is_ok() {}
    }

    /// A watched inbox that held the files `present` when the watch began.
    struct Watched {
        _scratch: tempfile::TempDir,
        inbox: PathBuf,
        receiver: mpsc::Receiver<()>,
        _watcher: InboxWatcher,
    }

    fn watched_with(present: &[&str]) -> Watched {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        location.create_layout().expect("layout");
        let inbox = location.inbox();
        for name in present {
            std::fs::write(inbox.join(name), "{}").expect("written");
        }
        let (sender, receiver) = mpsc::channel();
        let watcher = watch_inbox(&location, move || {
            let _ = sender.send(());
        })
        .expect("watching");
        Watched {
            _scratch: scratch,
            inbox,
            receiver,
            _watcher: watcher,
        }
    }

    /// Writes `content` into the file at `path` through the file itself,
    /// so the inbox directory keeps the same entry for it.
    fn rewrite_in_place(path: &Path, content: &str) {
        use std::io::Write;

        let inode = std::fs::metadata(path).expect("metadata").ino();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .expect("opened");
        file.write_all(content.as_bytes()).expect("written");
        drop(file);
        assert_eq!(std::fs::metadata(path).expect("metadata").ino(), inode);
    }

    /// FSEvents goes through a system daemon that loses events when it
    /// falls behind; kqueue reports from the kernel (ADR 113).
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_watches_through_kqueue() {
        use notify_debouncer_full::notify::{Watcher, WatcherKind};

        assert_eq!(RecommendedWatcher::kind(), WatcherKind::Kqueue);
    }

    /// Every change the TUI acts on is reported, each on its own: a drop,
    /// `ask`'s temp file renamed into place, a removal and drops in a row.
    /// A watcher that loses events under load fails here, where a test of
    /// a single drop could still pass on the one event that got through.
    #[test]
    fn every_change_of_the_inbox_is_reported() {
        let Watched {
            inbox, receiver, ..
        } = &watched_with(&[]);

        std::fs::write(inbox.join("dropped.json"), "{}").expect("written");
        reported(receiver, "a drop");

        std::fs::write(inbox.join("asked.json.tmp"), "{}").expect("written");
        std::fs::rename(inbox.join("asked.json.tmp"), inbox.join("asked.json")).expect("renamed");
        reported(receiver, "a rename into place");

        std::fs::remove_file(inbox.join("dropped.json")).expect("removed");
        reported(receiver, "a removal");

        for n in 1..=5 {
            std::fs::write(inbox.join(format!("next-{n}.json")), "{}").expect("written");
            reported(receiver, &format!("drop {n} in a row"));
        }
    }

    /// A changed inbox file is reloaded (spec section 5.4), also one that
    /// waited in the inbox before asqr started.
    #[test]
    fn a_file_there_at_the_start_rewritten_in_place_is_reported() {
        let Watched {
            inbox, receiver, ..
        } = &watched_with(&["waiting.json"]);

        rewrite_in_place(&inbox.join("waiting.json"), r#"{"changed": 1}"#);
        reported(receiver, "the first rewrite");
        rewrite_in_place(&inbox.join("waiting.json"), r#"{"changed": 2}"#);
        reported(receiver, "the second rewrite");
    }

    /// Every report starts a sync, which reads the inbox files. Were a
    /// read reported, the TUI would sync over and over.
    #[test]
    fn reading_a_watched_file_is_not_reported() {
        let Watched {
            inbox, receiver, ..
        } = &watched_with(&["waiting.json"]);

        for _ in 0..3 {
            std::fs::read(inbox.join("waiting.json")).expect("read");
        }

        assert_eq!(
            receiver.recv_timeout(DEBOUNCE * 4),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
    }

    /// Two files arriving in one burst are each watched, not only the one
    /// the burst's report happens to name.
    #[test]
    fn files_that_arrived_later_rewritten_in_place_are_reported() {
        let Watched {
            inbox, receiver, ..
        } = &watched_with(&[]);
        std::fs::write(inbox.join("first.json"), "{}").expect("written");
        std::fs::write(inbox.join("second.json"), "{}").expect("written");
        reported(receiver, "the two drops");

        rewrite_in_place(&inbox.join("first.json"), r#"{"changed": 1}"#);
        reported(receiver, "the rewrite of the first");
        rewrite_in_place(&inbox.join("second.json"), r#"{"changed": 1}"#);
        reported(receiver, "the rewrite of the second");
    }

    /// `ask` dropping a session again renames a new file over the old one.
    /// The new file is watched in turn.
    #[test]
    fn a_file_replaced_by_a_rename_rewritten_in_place_is_reported() {
        let Watched {
            inbox, receiver, ..
        } = &watched_with(&["asked.json"]);
        std::fs::write(inbox.join("asked.json.tmp"), "{}").expect("written");
        std::fs::rename(inbox.join("asked.json.tmp"), inbox.join("asked.json")).expect("renamed");
        reported(receiver, "the replacement");

        rewrite_in_place(&inbox.join("asked.json"), r#"{"changed": 1}"#);
        reported(receiver, "the rewrite of the new file");
    }

    /// The drop waits for the thread that owns the watches: once it
    /// returns, the callback is gone with it.
    #[test]
    fn a_dropped_watcher_has_ended_its_thread() {
        let Watched {
            _scratch,
            receiver,
            _watcher: watcher,
            ..
        } = watched_with(&["waiting.json"]);

        drop(watcher);

        assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[test]
    fn a_missing_inbox_cannot_be_watched() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("never-created"));

        assert!(watch_inbox(&location, || {}).is_err());
    }

    /// A [`FileWatcher`] that notes each call, and fails the watches of
    /// the paths in `failing` and every unwatch when `unwatch_fails`.
    #[derive(Default)]
    struct Recorder {
        calls: Vec<String>,
        failing: Vec<PathBuf>,
        unwatch_fails: bool,
    }

    impl Recorder {
        fn take_calls(&mut self) -> Vec<String> {
            let mut calls = std::mem::take(&mut self.calls);
            calls.sort();
            calls
        }
    }

    fn name_of(path: &Path) -> String {
        path.file_name()
            .expect("a file name")
            .to_string_lossy()
            .into_owned()
    }

    impl FileWatcher for Recorder {
        fn watch_file(&mut self, path: &Path) -> notify::Result<()> {
            self.calls.push(format!("watch {}", name_of(path)));
            if self.failing.iter().any(|failing| failing == path) {
                return Err(notify::Error::generic("cannot watch"));
            }
            Ok(())
        }

        fn unwatch_file(&mut self, path: &Path) -> notify::Result<()> {
            self.calls.push(format!("unwatch {}", name_of(path)));
            if self.unwatch_fails {
                return Err(notify::Error::watch_not_found());
            }
            Ok(())
        }
    }

    #[test]
    fn each_regular_file_is_watched_once() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let inbox = scratch.path();
        std::fs::write(inbox.join("a.json"), "{}").expect("written");
        std::fs::write(inbox.join("b.json.tmp"), "{}").expect("written");
        std::fs::create_dir(inbox.join("folder")).expect("folder");
        std::os::unix::fs::symlink(inbox.join("a.json"), inbox.join("link.json")).expect("symlink");
        let mut recorder = Recorder::default();
        let mut files = FileWatches::default();

        files.sync(&mut recorder, inbox);
        assert_eq!(recorder.take_calls(), ["watch a.json", "watch b.json.tmp"]);

        files.sync(&mut recorder, inbox);
        assert_eq!(recorder.take_calls(), Vec::<String>::new());
    }

    #[test]
    fn a_file_replaced_by_a_rename_is_watched_again() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let inbox = scratch.path();
        std::fs::write(inbox.join("a.json"), "{}").expect("written");
        let mut recorder = Recorder::default();
        let mut files = FileWatches::default();
        files.sync(&mut recorder, inbox);
        recorder.take_calls();

        std::fs::write(inbox.join("a.json.tmp"), "{}").expect("written");
        std::fs::rename(inbox.join("a.json.tmp"), inbox.join("a.json")).expect("renamed");
        files.sync(&mut recorder, inbox);

        assert_eq!(recorder.take_calls(), ["unwatch a.json", "watch a.json"]);
    }

    #[test]
    fn a_file_that_left_is_unwatched_once_even_when_that_fails() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let inbox = scratch.path();
        std::fs::write(inbox.join("a.json"), "{}").expect("written");
        std::fs::write(inbox.join("b.json"), "{}").expect("written");
        let mut recorder = Recorder {
            unwatch_fails: true,
            ..Recorder::default()
        };
        let mut files = FileWatches::default();
        files.sync(&mut recorder, inbox);
        recorder.take_calls();

        std::fs::remove_file(inbox.join("a.json")).expect("removed");
        files.sync(&mut recorder, inbox);
        assert_eq!(recorder.take_calls(), ["unwatch a.json"]);

        files.sync(&mut recorder, inbox);
        assert_eq!(recorder.take_calls(), Vec::<String>::new());
    }

    #[test]
    fn a_file_that_could_not_be_watched_is_tried_again() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let inbox = scratch.path();
        std::fs::write(inbox.join("a.json"), "{}").expect("written");
        let mut recorder = Recorder {
            failing: vec![inbox.join("a.json")],
            ..Recorder::default()
        };
        let mut files = FileWatches::default();

        files.sync(&mut recorder, inbox);
        assert_eq!(recorder.take_calls(), ["watch a.json"]);

        recorder.failing.clear();
        files.sync(&mut recorder, inbox);
        assert_eq!(recorder.take_calls(), ["watch a.json"]);
        files.sync(&mut recorder, inbox);
        assert_eq!(recorder.take_calls(), Vec::<String>::new());
    }

    /// An inbox that cannot be listed changes nothing: the watches stay
    /// until a listing shows the files gone.
    #[test]
    fn an_inbox_that_cannot_be_listed_keeps_the_watches() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let inbox = scratch.path().join("inbox");
        std::fs::create_dir(&inbox).expect("inbox");
        std::fs::write(inbox.join("a.json"), "{}").expect("written");
        let mut recorder = Recorder::default();
        let mut files = FileWatches::default();
        files.sync(&mut recorder, &inbox);
        recorder.take_calls();

        std::fs::rename(&inbox, scratch.path().join("moved")).expect("moved");
        files.sync(&mut recorder, &inbox);

        assert_eq!(recorder.take_calls(), Vec::<String>::new());
        assert_eq!(files.watched.len(), 1);
    }
}
