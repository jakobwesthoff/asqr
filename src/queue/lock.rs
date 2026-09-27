// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One asqr instance per queue (spec section 3.6, ADR 14). The TUI holds
//! an advisory lock on `<queue>/lock` for as long as it runs. The kernel
//! releases the lock when the process ends, however it ends, so a lock can
//! never go stale. The pid and host name written into the file only serve
//! the "held by" message and may be outdated once the holder is gone.

use std::fs::{File, TryLockError};
use std::io::{self, Read, Seek, Write};

use thiserror::Error;

use super::QueueLocation;

/// The held lock; dropping it releases the queue.
#[derive(Debug)]
pub struct QueueLock {
    _file: File,
}

#[derive(Debug, Error)]
pub enum LockError {
    #[error("another asqr is watching this queue ({0})")]
    Held(String),

    #[error(transparent)]
    Io(#[from] io::Error),
}

fn open_lock_file(location: &QueueLocation) -> io::Result<File> {
    std::fs::create_dir_all(location.dir())?;
    File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(location.lock_file())
}

fn read_holder(file: &mut File) -> io::Result<String> {
    let mut holder = String::new();
    file.read_to_string(&mut holder)?;
    let holder = holder.trim();
    Ok(if holder.is_empty() {
        "unknown process".to_owned()
    } else {
        holder.to_owned()
    })
}

/// Whether a non-blocking lock attempt got the lock. Only `WouldBlock`
/// means someone else holds it; any other failure is a plain error.
fn acquired(attempt: Result<(), TryLockError>) -> io::Result<bool> {
    match attempt {
        Ok(()) => Ok(true),
        Err(TryLockError::WouldBlock) => Ok(false),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Takes the queue lock for this process.
pub fn lock_queue(location: &QueueLocation) -> Result<QueueLock, LockError> {
    let mut file = open_lock_file(location)?;
    if !acquired(file.try_lock())? {
        return Err(LockError::Held(read_holder(&mut file)?));
    }
    let host = gethostname::gethostname();
    file.set_len(0)?;
    file.rewind()?;
    writeln!(
        file,
        "pid {} on {}",
        std::process::id(),
        host.to_string_lossy()
    )?;
    Ok(QueueLock { _file: file })
}

/// Who holds the queue lock, if anyone, without taking it. Commands that
/// only ask or read use it, for the "no asqr is watching" warning and for
/// `asqr status`.
pub fn lock_holder(location: &QueueLocation) -> io::Result<Option<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn queue() -> (tempfile::TempDir, QueueLocation) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        (scratch, location)
    }

    #[test]
    fn a_second_instance_is_refused_and_told_who_holds_the_queue() {
        let (_scratch, location) = queue();
        let _held = lock_queue(&location).expect("first instance locks");

        let refused = lock_queue(&location).expect_err("second instance is refused");

        let LockError::Held(holder) = refused else {
            panic!("expected Held, got {refused:?}");
        };
        assert!(
            holder.contains(&format!("pid {}", std::process::id())),
            "{holder}"
        );
    }

    #[test]
    fn the_lock_ends_with_its_holder() {
        let (_scratch, location) = queue();
        let held = lock_queue(&location).expect("locks");
        drop(held);

        lock_queue(&location).expect("the queue is free again");
        // The file stays behind; its text is only advisory.
        assert!(location.lock_file().is_file());
    }

    #[test]
    fn reports_the_holder_without_taking_the_lock() {
        let (_scratch, location) = queue();
        assert_eq!(lock_holder(&location).expect("no lock file yet"), None);

        let held = lock_queue(&location).expect("locks");
        let holder = lock_holder(&location)
            .expect("readable")
            .expect("someone holds it");
        assert!(holder.starts_with("pid "), "{holder}");

        drop(held);
        assert_eq!(lock_holder(&location).expect("readable"), None);
    }

    #[test]
    fn a_lock_file_that_cannot_be_opened_is_an_error() {
        let (_scratch, location) = queue();
        std::fs::create_dir_all(location.lock_file())
            .expect("a directory where the lock file goes");

        assert!(matches!(lock_queue(&location), Err(LockError::Io(_))));
    }

    #[test]
    fn a_lock_file_that_cannot_be_read_is_an_error() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, location) = queue();
        drop(lock_queue(&location).expect("creates the lock file"));
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
            lock_holder(&location).expect("readable").as_deref(),
            Some("unknown process")
        );
    }

    #[test]
    fn describes_the_refusal() {
        assert_eq!(
            LockError::Held("pid 7 on box".into()).to_string(),
            "another asqr is watching this queue (pid 7 on box)"
        );
    }
}
