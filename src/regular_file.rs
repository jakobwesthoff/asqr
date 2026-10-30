// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Opening a file that must be a regular file, without waiting. Opening a
//! FIFO for reading blocks until a writer comes, and a device such as
//! `/dev/zero` never ends, so everything that reads a file a session names
//! (the viewers of the terminal UI, the checks of `asqr validate` and
//! `asqr ask`) opens it through [`open`] and reads from the handle it
//! returns, never from the path a second time.

use std::fs::{File, Metadata};
use std::io;
use std::path::Path;

/// Why a file could not be opened as a regular file.
#[derive(Debug)]
pub enum OpenError {
    /// The path names something that is no regular file. `directory` tells
    /// a directory from a FIFO, a device or a socket.
    NotRegular { directory: bool },
    /// The system could not open the file, or ask it for its type.
    Io(io::Error),
}

/// Refuses what `metadata` describes unless it is a regular file.
pub fn check_type(metadata: &Metadata) -> Result<(), OpenError> {
    if metadata.is_file() {
        return Ok(());
    }
    Err(OpenError::NotRegular {
        directory: metadata.is_dir(),
    })
}

/// Opens `path` for reading and refuses what turns out to be no regular
/// file. The type is checked on the file actually opened, so a path that
/// was swapped for a FIFO after someone looked at it is caught here.
///
/// On Unix the open does not wait: opening a FIFO for reading blocks until
/// a writer comes, which would hold the calling thread for ever. The file
/// does not become the controlling terminal of the process if it is a
/// terminal device. Once the file is known to be regular, its reads block
/// as usual again.
#[cfg(unix)]
pub fn open(path: &Path) -> Result<File, OpenError> {
    use rustix::fs::{Mode, OFlags, fcntl_getfl, fcntl_setfl, open};

    let io = |error: rustix::io::Errno| OpenError::Io(error.into());
    let file = File::from(
        open(
            path,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY,
            Mode::empty(),
        )
        .map_err(io)?,
    );
    check_type(&file.metadata().map_err(OpenError::Io)?)?;
    fcntl_setfl(&file, fcntl_getfl(&file).map_err(io)? - OFlags::NONBLOCK).map_err(io)?;
    Ok(file)
}

/// Opens `path` for reading and refuses what turns out to be no regular
/// file.
#[cfg(not(unix))]
pub fn open(path: &Path) -> Result<File, OpenError> {
    let file = File::open(path).map_err(OpenError::Io)?;
    check_type(&file.metadata().map_err(OpenError::Io)?)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    /// A FIFO in a fresh directory, which lives as long as the returned one.
    #[cfg(unix)]
    fn fifo() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let fifo = directory.path().join("pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        (directory, fifo)
    }

    fn not_regular(opened: Result<(), OpenError>) -> bool {
        match opened {
            Err(OpenError::NotRegular { directory }) => !directory,
            _ => false,
        }
    }

    /// A path swapped for a FIFO after someone looked at it: the open
    /// itself must not wait for a writer. It runs apart, so a hang fails
    /// the test instead of stalling the run.
    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_waiting_for_a_writer() {
        let (_directory, fifo) = fifo();

        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(open(&fifo).map(|_| ()));
        });
        let opened = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("opening a FIFO must not block");

        assert!(not_regular(opened));
    }

    #[cfg(unix)]
    #[test]
    fn a_device_is_refused() {
        assert!(not_regular(open(Path::new("/dev/null")).map(|_| ())));
    }

    #[test]
    fn a_directory_is_refused_as_one() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");

        let refused = open(directory.path()).expect_err("a directory");

        assert!(
            matches!(refused, OpenError::NotRegular { directory: true }),
            "{refused:?}"
        );
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let refused = open(Path::new("/nowhere/fetch.rs")).expect_err("missing");

        assert!(
            matches!(&refused, OpenError::Io(error) if error.kind() == io::ErrorKind::NotFound),
            "{refused:?}"
        );
    }

    #[test]
    fn the_type_of_a_regular_file_is_accepted() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("a.txt");
        std::fs::write(&path, b"one\n").expect("file is writable");

        let metadata = std::fs::metadata(&path).expect("file has metadata");

        assert!(check_type(&metadata).is_ok());
        assert!(open(&path).is_ok());
    }

    /// Reads from the opened file block as usual: a file left in
    /// non-blocking mode could fail a read with `WouldBlock`.
    #[cfg(unix)]
    #[test]
    fn an_opened_file_reads_without_o_nonblock() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("a.txt");
        std::fs::write(&path, b"one\n").expect("file is writable");

        let opened = open(&path).expect("a regular file");

        let flags = rustix::fs::fcntl_getfl(&opened).expect("flags are readable");
        assert!(!flags.contains(rustix::fs::OFlags::NONBLOCK), "{flags:?}");
    }
}
