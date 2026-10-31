// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One asqr instance per queue (spec section 5.6, ADR 14). The TUI holds
//! an advisory lock on `<queue>/lock` for as long as it runs. The kernel
//! releases the lock when the process ends, however it ends, so a lock can
//! never go stale. The pid and host name written into the file only serve
//! the "held by" message and may be outdated once the holder is gone.

use std::fs::{File, TryLockError};
use std::io::{self, Read, Seek, Write};
use std::path::Path;

use thiserror::Error;

use super::QueueLocation;
use crate::pairing::TmuxPane;

/// The held lock; dropping it releases the queue.
#[derive(Debug)]
pub struct QueueLock {
    _file: File,
}

/// Who holds a queue lock, as the lock file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// `pid <pid> on <host>`, the file's first line, or `unknown process`
    /// for a file that names nobody.
    pub text: String,
    /// The tmux pane the holder runs in, where it runs in one (ADR 85).
    pub tmux: Option<HolderPane>,
}

/// The tmux pane of a lock's holder: the server's socket and the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolderPane {
    pub socket: String,
    pub pane: String,
}

#[derive(Debug, Error)]
pub enum LockError {
    #[error("another asqr is watching this queue ({})", .0.text)]
    Held(Holder),

    #[error(transparent)]
    Io(#[from] io::Error),
}

fn open_lock_file(location: &QueueLocation) -> io::Result<File> {
    open_in_queue(location, &location.lock_file())
}

/// Opens the lock file at `path` in the queue at `location`, creating
/// both where they are missing, as after `asqr prune` removed the queue.
fn open_in_queue(location: &QueueLocation, path: &Path) -> io::Result<File> {
    std::fs::create_dir_all(location.dir())?;
    File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// The outcome of [`take`].
enum Taken {
    Locked(File),
    /// Someone else held the lock through every attempt; the file names
    /// the holder.
    Held(File),
}

/// Takes the exclusive lock on the lock file at `path`, which `open`
/// opens, trying `attempts` times, [`LOCK_RETRY`] apart.
///
/// `flock` locks the open file, not its path. `asqr prune` removes the
/// lock files of a queue it removes while it holds their locks, so a
/// process that opened one before and waited gets the lock of a file no
/// path names, which guards nothing: the next process opens a new file at
/// the path and locks that one too. So a lock taken is kept only on the
/// file the path still names; otherwise the path is opened again with
/// `open` and tried at once.
fn take(path: &Path, open: impl FnMut() -> io::Result<File>, attempts: u32) -> io::Result<Taken> {
    take_waiting(path, open, attempts, &mut || {
        std::thread::sleep(LOCK_RETRY);
    })
}

/// [`take`] that calls `wait` between two attempts, in place of sleeping:
/// tests release a held lock there.
fn take_waiting(
    path: &Path,
    mut open: impl FnMut() -> io::Result<File>,
    attempts: u32,
    wait: &mut dyn FnMut(),
) -> io::Result<Taken> {
    let mut file = open()?;
    let mut tried = 0;
    loop {
        if acquired(file.try_lock())? {
            if still_at(&file, path)? {
                return Ok(Taken::Locked(file));
            }
            file = open()?;
            continue;
        }
        tried += 1;
        if tried >= attempts {
            return Ok(Taken::Held(file));
        }
        wait();
    }
}

/// Whether `file` is the file `path` names now: the same device and inode.
/// A path that names nothing any more names another file.
#[cfg(unix)]
pub(crate) fn still_at(file: &File, path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let held = file.metadata()?;
    match std::fs::metadata(path) {
        Ok(named) => Ok((held.dev(), held.ino()) == (named.dev(), named.ino())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// asqr is released for macOS and Linux only; elsewhere the check is not
/// made.
#[cfg(not(unix))]
pub(crate) fn still_at(_file: &File, _path: &Path) -> io::Result<bool> {
    Ok(true)
}

fn read_holder(file: &mut File) -> io::Result<Holder> {
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let mut lines = content.lines().map(str::trim);
    let text = lines
        .next()
        .filter(|text| !text.is_empty())
        .unwrap_or("unknown process")
        .to_owned();
    // `tmux <socket> <pane>`: a pane id has no blank, a socket's path may,
    // so the pane is the last word.
    let tmux = lines
        .next()
        .and_then(|line| line.strip_prefix("tmux "))
        .and_then(|place| place.rsplit_once(' '))
        .map(|(socket, pane)| HolderPane {
            socket: socket.to_owned(),
            pane: pane.to_owned(),
        });
    Ok(Holder { text, tmux })
}

/// Whether a non-blocking lock attempt got the lock. Only `WouldBlock`
/// means someone else holds it; any other failure is a plain error.
pub(crate) fn acquired(attempt: Result<(), TryLockError>) -> io::Result<bool> {
    match attempt {
        Ok(()) => Ok(true),
        Err(TryLockError::WouldBlock) => Ok(false),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// How often, and how far apart, a held lock is tried before the queue
/// counts as watched: 200 ms in all.
pub const LOCK_ATTEMPTS: u32 = 10;
const LOCK_RETRY: std::time::Duration = std::time::Duration::from_millis(20);

/// Takes the queue lock for this process, which runs in the tmux pane
/// `tmux` where given.
pub fn lock_queue(
    location: &QueueLocation,
    tmux: Option<&TmuxPane>,
) -> Result<QueueLock, LockError> {
    let path = location.lock_file();
    // `lock_holder` takes a shared lock for a moment, so a refusal is only
    // final once it lasts longer than such a check.
    let mut file = match take(&path, || open_lock_file(location), LOCK_ATTEMPTS)? {
        Taken::Locked(file) => file,
        Taken::Held(mut file) => return Err(LockError::Held(read_holder(&mut file)?)),
    };
    let host = gethostname::gethostname();
    file.set_len(0)?;
    file.rewind()?;
    writeln!(
        file,
        "pid {} on {}",
        std::process::id(),
        host.to_string_lossy()
    )?;
    // The pane, not its session: the session is looked up when the line
    // is shown, so a renamed session or a moved window still reads right.
    if let Some(TmuxPane {
        socket,
        pane: Some(pane),
    }) = tmux
    {
        writeln!(file, "tmux {socket} {pane}")?;
    }
    Ok(QueueLock { _file: file })
}

/// Takes the queue lock once, without waiting and without writing this
/// process into the file, or gives `None` while someone holds it: for
/// `asqr prune`, which removes a queue only while nobody watches it.
pub fn try_lock_queue(location: &QueueLocation) -> io::Result<Option<QueueLock>> {
    let path = location.lock_file();
    Ok(match take(&path, || open_lock_file(location), 1)? {
        Taken::Locked(file) => Some(QueueLock { _file: file }),
        Taken::Held(_) => None,
    })
}

/// [`try_lock_queue`] that tries `attempts` times, 20 ms apart;
/// [`LOCK_ATTEMPTS`] is the number a start uses.
/// `lock_holder` takes a shared lock for a moment, as every TUI and
/// command that looks at a queue does, so a single attempt can meet it
/// and report a queue as watched that nobody watches. Like
/// [`try_lock_queue`], it does not write this process into the file.
pub fn try_lock_queue_for(
    location: &QueueLocation,
    attempts: u32,
) -> io::Result<Option<QueueLock>> {
    try_lock_queue_waiting(location, attempts, &mut || {
        std::thread::sleep(LOCK_RETRY);
    })
}

fn try_lock_queue_waiting(
    location: &QueueLocation,
    attempts: u32,
    wait: &mut dyn FnMut(),
) -> io::Result<Option<QueueLock>> {
    let path = location.lock_file();
    Ok(
        match take_waiting(&path, || open_lock_file(location), attempts, wait)? {
            Taken::Locked(file) => Some(QueueLock { _file: file }),
            Taken::Held(_) => None,
        },
    )
}

/// Who holds the queue lock, if anyone, without taking it. Commands that
/// only ask or read use it, for the "no asqr is watching" warning and for
/// `asqr status`.
pub fn lock_holder(location: &QueueLocation) -> io::Result<Option<Holder>> {
    let mut file = match File::open(location.lock_file()) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // A shared lock succeeds when nobody holds the exclusive one; it is
    // released again with `file`.
    if acquired(file.try_lock_shared())? {
        Ok(None)
    } else {
        read_holder(&mut file).map(Some)
    }
}

// =========================================================================
// The drafts lock
// =========================================================================
//
// The TUI and the commands that change a waiting session are separate
// processes. Each of them checks the queue's files and then writes, and a
// check-then-write between two processes races: a draft saved between
// `ask`'s look at the draft and its replacement of the session is never
// seen. Every such check and the writes that follow it therefore run under
// an exclusive lock on `<queue>/drafts.lock` (spec section 5.6, ADR 63).
// It is held for a few file operations only, apart from the instance lock
// that the TUI holds for its whole run.

/// How often, and how far apart, the drafts lock is tried before an
/// operation gives up: about 2 s, so a holder that stopped can hold up the
/// TUI or an asker's command only that long.
pub const DRAFTS_LOCK_ATTEMPTS: u32 = 100;

/// The held drafts lock; dropping it lets the next operation in.
#[derive(Debug)]
pub struct DraftsLock {
    _file: File,
}

/// Takes the drafts lock, trying `attempts` times. A lock still held after
/// the last attempt is an error of kind [`io::ErrorKind::WouldBlock`]
/// saying that the queue is busy.
pub fn lock_drafts(location: &QueueLocation, attempts: u32) -> io::Result<DraftsLock> {
    let path = location.drafts_lock_file();
    match take(&path, || open_in_queue(location, &path), attempts)? {
        Taken::Locked(file) => Ok(DraftsLock { _file: file }),
        Taken::Held(_) => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "the queue is busy",
        )),
    }
}

/// A step of an operation under the drafts lock, after which the
/// operation's [`Gate`] is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The operation looked at the files it decides on.
    Checked,
    /// A draft was written.
    DraftWritten,
    /// A result was written into the outbox.
    ResultWritten,
    /// A session or a result was moved into the archive.
    Archived,
    /// A draft was deleted.
    DraftDeleted,
    /// A session file was written into the inbox.
    SessionWritten,
}

/// How an operation takes the drafts lock, and what it calls after each
/// [`Step`] while it holds the lock. Tests use the call to look at the lock
/// from outside at every step, which needs no timing; the operations
/// themselves pass the [`Default`], which tries [`DRAFTS_LOCK_ATTEMPTS`]
/// times and calls nothing.
pub struct Gate<'a> {
    pub attempts: u32,
    pub step: Box<dyn FnMut(Step) + 'a>,
}

impl Default for Gate<'_> {
    fn default() -> Self {
        Gate {
            attempts: DRAFTS_LOCK_ATTEMPTS,
            step: Box::new(|_| {}),
        }
    }
}

/// Test support: whether someone holds the drafts lock of `location`, seen
/// from another open file, as another process would see it.
#[cfg(test)]
pub(crate) fn drafts_lock_is_held(location: &QueueLocation) -> bool {
    let file = File::open(location.drafts_lock_file()).expect("the drafts lock file exists");
    !acquired(file.try_lock()).expect("the drafts lock can be tried")
}

/// Test support: a [`Gate`] that tries once and records, after each step,
/// whether the drafts lock was held.
#[cfg(test)]
pub(crate) fn recording_gate<'a>(
    location: &'a QueueLocation,
    seen: &'a std::cell::RefCell<Vec<(Step, bool)>>,
) -> Gate<'a> {
    Gate {
        attempts: 1,
        step: Box::new(move |step| {
            seen.borrow_mut()
                .push((step, drafts_lock_is_held(location)));
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        (scratch, location)
    }

    #[test]
    fn a_holder_check_at_the_same_moment_does_not_refuse_the_start() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");
        std::fs::write(location.lock_file(), "").expect("lock file");
        // What `lock_holder` does: a shared lock, held for a moment.
        let probe = std::fs::File::open(location.lock_file()).expect("opens");
        probe.try_lock_shared().expect("shared lock");
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            drop(probe);
        });

        let locked = lock_queue(&location, None);
        release.join().expect("joined");

        assert!(locked.is_ok(), "{locked:?}");
    }

    #[test]
    fn a_holder_check_at_the_same_moment_does_not_make_a_try_give_up() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");
        std::fs::write(location.lock_file(), "").expect("lock file");
        // What `lock_holder` does: a shared lock, held until the try has
        // waited once.
        let probe = std::fs::File::open(location.lock_file()).expect("opens");
        probe.try_lock_shared().expect("shared lock");
        let mut probe = Some(probe);
        let mut waited = 0;

        let locked = try_lock_queue_waiting(&location, LOCK_ATTEMPTS, &mut || {
            waited += 1;
            probe.take();
        });

        assert!(matches!(locked, Ok(Some(_))), "{locked:?}");
        assert_eq!(waited, 1, "the first attempt met the probe");
    }

    #[test]
    fn a_try_gives_up_while_the_queue_stays_held() {
        let (_scratch, location) = queue();
        let _held = lock_queue(&location, None).expect("first instance locks");

        assert!(matches!(try_lock_queue_for(&location, 2), Ok(None)));
    }

    #[test]
    fn a_second_instance_is_refused_and_told_who_holds_the_queue() {
        let (_scratch, location) = queue();
        let _held = lock_queue(&location, None).expect("first instance locks");

        let refused = lock_queue(&location, None).expect_err("second instance is refused");

        let LockError::Held(holder) = refused else {
            panic!("expected Held, got {refused:?}");
        };
        assert!(
            holder.text.contains(&format!("pid {}", std::process::id())),
            "{holder:?}"
        );
    }

    #[test]
    fn the_lock_ends_with_its_holder() {
        let (_scratch, location) = queue();
        let held = lock_queue(&location, None).expect("locks");
        drop(held);

        lock_queue(&location, None).expect("the queue is free again");
        // The file stays behind; its text is only advisory.
        assert!(location.lock_file().is_file());
    }

    #[test]
    fn reports_the_holder_without_taking_the_lock() {
        let (_scratch, location) = queue();
        assert_eq!(lock_holder(&location).expect("no lock file yet"), None);

        let held = lock_queue(&location, None).expect("locks");
        let holder = lock_holder(&location)
            .expect("readable")
            .expect("someone holds it");
        assert!(holder.text.starts_with("pid "), "{holder:?}");
        assert_eq!(holder.tmux, None, "not in a tmux pane");

        drop(held);
        assert_eq!(lock_holder(&location).expect("readable"), None);
    }

    #[test]
    fn a_lock_file_that_cannot_be_opened_is_an_error() {
        let (_scratch, location) = queue();
        std::fs::create_dir_all(location.lock_file())
            .expect("a directory where the lock file goes");

        assert!(matches!(lock_queue(&location, None), Err(LockError::Io(_))));
    }

    #[test]
    fn a_lock_file_that_cannot_be_read_is_an_error() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, location) = queue();
        drop(lock_queue(&location, None).expect("creates the lock file"));
        std::fs::set_permissions(location.lock_file(), std::fs::Permissions::from_mode(0o000))
            .expect("permissions change");

        assert!(lock_holder(&location).is_err());
    }

    #[test]
    fn a_holder_that_wrote_nothing_is_an_unknown_process() {
        let (_scratch, location) = queue();
        let foreign = open_lock_file(&location).expect("lock file opens");
        foreign.lock().expect("locked without writing a holder");

        assert_eq!(
            lock_holder(&location).expect("readable"),
            Some(Holder {
                text: "unknown process".into(),
                tmux: None
            })
        );
    }

    fn pane(socket: &str, pane: &str) -> TmuxPane {
        TmuxPane {
            socket: socket.into(),
            pane: Some(pane.into()),
        }
    }

    #[test]
    fn the_lock_names_its_tmux_pane_on_a_second_line() {
        let (_scratch, location) = queue();
        let _held =
            lock_queue(&location, Some(&pane("/tmp/tmux-501/default", "%74"))).expect("locks");

        let text = std::fs::read_to_string(location.lock_file()).expect("readable");
        let holder = lock_holder(&location).expect("readable").expect("held");

        assert_eq!(text.lines().nth(1), Some("tmux /tmp/tmux-501/default %74"));
        assert_eq!(
            holder.tmux,
            Some(HolderPane {
                socket: "/tmp/tmux-501/default".into(),
                pane: "%74".into()
            })
        );
    }

    #[test]
    fn the_holder_text_is_the_first_line() {
        let (_scratch, location) = queue();
        let _held =
            lock_queue(&location, Some(&pane("/tmp/tmux-501/default", "%74"))).expect("locks");

        let holder = lock_holder(&location).expect("readable").expect("held");

        assert_eq!(
            holder.text,
            format!(
                "pid {} on {}",
                std::process::id(),
                gethostname::gethostname().to_string_lossy()
            )
        );
    }

    #[test]
    fn a_socket_with_a_blank_is_read_back_whole() {
        let (_scratch, location) = queue();
        let _held =
            lock_queue(&location, Some(&pane("/tmp/my tmux/default", "%3"))).expect("locks");

        let holder = lock_holder(&location).expect("readable").expect("held");

        assert_eq!(
            holder.tmux,
            Some(HolderPane {
                socket: "/tmp/my tmux/default".into(),
                pane: "%3".into()
            })
        );
    }

    #[test]
    fn a_pane_without_an_id_writes_no_tmux_line() {
        let (_scratch, location) = queue();
        let without = TmuxPane {
            socket: "/tmp/tmux-501/default".into(),
            pane: None,
        };
        let _held = lock_queue(&location, Some(&without)).expect("locks");

        let text = std::fs::read_to_string(location.lock_file()).expect("readable");

        assert_eq!(text.lines().count(), 1, "{text}");
    }

    #[test]
    fn the_drafts_lock_is_exclusive_and_ends_with_its_holder() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");

        let held = lock_drafts(&location, 1).expect("locks");
        assert!(drafts_lock_is_held(&location), "held while the guard lives");

        drop(held);
        assert!(!drafts_lock_is_held(&location), "free after the guard");
    }

    #[test]
    fn a_busy_drafts_lock_gives_up_after_its_attempts() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");
        let _held = lock_drafts(&location, 1).expect("locks");

        let busy = lock_drafts(&location, 1).expect_err("held by the guard above");

        assert_eq!(
            (busy.kind(), busy.to_string()),
            (io::ErrorKind::WouldBlock, "the queue is busy".to_owned())
        );
    }

    #[test]
    fn a_held_drafts_lock_is_tried_again_before_the_queue_counts_as_busy() {
        let (_scratch, location) = queue();
        location.create_layout().expect("layout");
        let _held = lock_drafts(&location, 1).expect("locks");

        // Held through both tries, so the outcome does not depend on the
        // pause between them.
        let busy = lock_drafts(&location, 2).expect_err("held by the guard above");

        assert_eq!(busy.kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    fn a_drafts_lock_file_that_cannot_be_opened_is_an_error() {
        let (_scratch, location) = queue();
        std::fs::create_dir_all(location.drafts_lock_file())
            .expect("a directory where the drafts lock file goes");

        let failed = lock_drafts(&location, 1).expect_err("a directory is no lock file");

        assert_ne!(failed.kind(), io::ErrorKind::WouldBlock);
    }

    /// Whether `file` is the file at `path`.
    fn is_at(file: &File, path: &std::path::Path) -> bool {
        use std::os::unix::fs::MetadataExt;

        let held = file.metadata().expect("the open file");
        let named = std::fs::metadata(path).expect("the file at the path");
        (held.dev(), held.ino()) == (named.dev(), named.ino())
    }

    /// An `open` for [`take`] that gives `first` and then what `then` opens:
    /// a waiter that opened a lock file before it was removed.
    fn opening(
        first: File,
        then: impl Fn() -> io::Result<File>,
    ) -> impl FnMut() -> io::Result<File> {
        let mut first = Some(first);
        move || first.take().map_or_else(&then, Ok)
    }

    /// A waiter opened the lock file at `path`; then it was removed, as
    /// `asqr prune` removes it, and a new one was made at the path.
    fn stale_lock_file(location: &QueueLocation, path: &std::path::Path) -> File {
        let waiter = open_in_queue(location, path).expect("opens");
        std::fs::remove_file(path).expect("removed");
        std::fs::write(path, "").expect("made anew");
        waiter
    }

    #[test]
    fn the_instance_lock_on_a_removed_file_is_taken_again_on_the_new_one() {
        let (_scratch, location) = queue();
        let path = location.lock_file();
        let waiter = stale_lock_file(&location, &path);

        let taken = take(&path, opening(waiter, || open_lock_file(&location)), 1).expect("takes");

        let Taken::Locked(file) = taken else {
            panic!("nobody else holds it");
        };
        assert!(
            is_at(&file, &path),
            "the lock is on the file the path names"
        );
    }

    #[test]
    fn the_drafts_lock_on_a_removed_file_is_taken_again_on_the_new_one() {
        let (_scratch, location) = queue();
        let path = location.drafts_lock_file();
        let waiter = stale_lock_file(&location, &path);

        let taken = take(
            &path,
            opening(waiter, || open_in_queue(&location, &path)),
            1,
        )
        .expect("takes");

        let Taken::Locked(file) = taken else {
            panic!("nobody else holds it");
        };
        assert!(
            is_at(&file, &path),
            "the lock is on the file the path names"
        );
    }

    #[test]
    fn a_lock_file_whose_path_is_gone_is_made_anew() {
        let (_scratch, location) = queue();
        let path = location.lock_file();
        let waiter = open_lock_file(&location).expect("opens");
        std::fs::remove_dir_all(location.dir()).expect("the queue is removed");

        let taken = take(&path, opening(waiter, || open_lock_file(&location)), 1).expect("takes");

        let Taken::Locked(file) = taken else {
            panic!("nobody else holds it");
        };
        assert!(is_at(&file, &path));
    }

    #[test]
    fn a_lock_file_whose_path_cannot_be_looked_at_is_an_error() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, location) = queue();
        let path = location.lock_file();
        let file = open_lock_file(&location).expect("opens");
        std::fs::set_permissions(location.dir(), std::fs::Permissions::from_mode(0o000))
            .expect("permissions change");

        let taken = take(
            &path,
            opening(file, || Err(io::Error::other("not opened again"))),
            1,
        );
        std::fs::set_permissions(location.dir(), std::fs::Permissions::from_mode(0o755))
            .expect("permissions back");

        assert!(taken.is_err());
    }

    #[test]
    fn the_drafts_lock_creates_a_missing_queue_directory() {
        let (_scratch, location) = queue();

        let held = lock_drafts(&location, 1).expect("locks a queue not made yet");

        assert!(location.drafts_lock_file().is_file());
        drop(held);
    }

    #[test]
    fn describes_the_refusal() {
        assert_eq!(
            LockError::Held(Holder {
                text: "pid 7 on box".into(),
                tmux: None
            })
            .to_string(),
            "another asqr is watching this queue (pid 7 on box)"
        );
    }
}
