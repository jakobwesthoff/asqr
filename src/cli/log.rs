// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The log file (ADR 15). The TUI owns the terminal, so everything worth
//! keeping goes into `asqr.log` in the platform cache directory. Logging
//! is a convenience: when the file cannot be opened, the command runs
//! without it. `ASQR_LOG` sets which events reach it (ADR 83).
//!
//! A process that finds `asqr.log` at or above [`LIMIT`] moves it aside as
//! `asqr-<UTC timestamp>.log` and compresses the moved logs no process
//! writes into any more (ADR 91).

use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use jiff::Timestamp;

use tracing::Dispatch;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::util::SubscriberInitExt;

use super::paths::log_file;
use crate::queue::{acquired, still_at};

/// The size at which a process moves the log aside.
const LIMIT: u64 = 1 << 20;

const LOG_NAME: &str = "asqr.log";

/// Locked by the process that rotates, so two of them never move a log
/// one after the other from the same size check.
const ROTATION_LOCK: &str = "asqr.rotate.lock";

/// How long a moved log has to stay unwritten before a rotation compresses
/// it.
const QUIET_BEFORE_COMPRESSION: Duration = Duration::from_secs(24 * 60 * 60);

/// How often a process opens the log while it keeps meeting a rotation or
/// a compressor.
const OPEN_ATTEMPTS: u32 = 3;

/// Sends `tracing` events of this process to the log file, appending.
/// `variable` is the value of `ASQR_LOG`.
pub(super) fn init(variable: Option<&str>) {
    if let Some(path) = log_file() {
        let _ = try_init(&path, variable);
    }
}

fn try_init(path: &Path, variable: Option<&str>) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        rotate(dir, LIMIT, Timestamp::now());
    }
    let file = open_log(path)?;
    // Only fails when a subscriber is set already, which then keeps
    // logging.
    let _ = dispatch(Mutex::new(file), variable).try_init();
    Ok(())
}

// =========================================================
// Opening the log
// =========================================================

fn open_log(path: &Path) -> io::Result<File> {
    open_log_with(path, || {})
}

/// Opens the log for appending and holds a shared lock on it for as long
/// as the process lives, which is as long as it can write into the file. A
/// compressor only takes a moved log it can lock exclusively, so no line
/// of a process that holds the lock is lost to it.
///
/// A rotation may move the log between the open and the lock, and a
/// compressor may hold the exclusive lock of a moved log that was opened
/// just before the move. Either way the path is opened again, up to
/// [`OPEN_ATTEMPTS`] times. If every try met one of them, the file at the
/// path is used as it is, unlocked, as logging into it is better than not
/// logging. So is a file that cannot be locked at all, as on a filesystem
/// without `flock`.
///
/// `opened` runs after the open of each try, before the lock is tried.
fn open_log_with(path: &Path, mut opened: impl FnMut()) -> io::Result<File> {
    for _ in 0..OPEN_ATTEMPTS {
        let file = open_for_appending(path)?;
        opened();
        match file.try_lock_shared() {
            Ok(()) => {
                if still_at(&file, path).unwrap_or(true) {
                    return Ok(file);
                }
            }
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(_)) => return Ok(file),
        }
    }
    open_for_appending(path)
}

fn open_for_appending(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

// =========================================================
// Rotation
// =========================================================

/// Where `rotate_with` is, for a test that acts between its steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The log is at the limit; the rotation lock is not tried yet.
    SizeChecked,
    /// The rotation lock is held.
    Locked,
}

/// Moves `asqr.log` in `dir` aside when it is at or above `limit`, named
/// after `now`, and compresses the moved logs. Failing at any point leaves
/// the files as they are; the next start tries again.
fn rotate(dir: &Path, limit: u64, now: Timestamp) {
    rotate_with(dir, limit, now, &mut |_| {});
}

fn rotate_with(dir: &Path, limit: u64, now: Timestamp, gate: &mut dyn FnMut(Step)) {
    let _ = try_rotate(dir, limit, now, gate);
}

fn try_rotate(
    dir: &Path,
    limit: u64,
    now: Timestamp,
    gate: &mut dyn FnMut(Step),
) -> io::Result<()> {
    let log = dir.join(LOG_NAME);
    if !is_due(&log, limit)? {
        return Ok(());
    }
    gate(Step::SizeChecked);

    // Without the lock, two processes that saw the same size could both
    // rename: the second would move the fresh log the first one's
    // successor already writes into.
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(ROTATION_LOCK))?;
    if !acquired(lock.try_lock())? {
        return Ok(());
    }
    gate(Step::Locked);

    // A process that rotated just before this one left a small log.
    if !is_due(&log, limit)? {
        return Ok(());
    }
    let moved = dir.join(format!("asqr-{}.log", now.strftime("%Y%m%dT%H%M%SZ")));
    // A rename replaces what is at its target, and a compressed log is
    // never given up.
    if moved.exists() || compressed_name(&moved).exists() {
        return Ok(());
    }
    std::fs::rename(&log, &moved)?;
    let cutoff = SystemTime::from(now) - QUIET_BEFORE_COMPRESSION;
    compress_old(dir, File::try_lock, &moved, cutoff)
}

/// Whether the log at `path` is at or above `limit`. A log that does not
/// exist yet is not.
fn is_due(path: &Path, limit: u64) -> io::Result<bool> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len() >= limit),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Compresses the moved logs in `dir`, but `newest`, the one the running
/// rotation moved, and the ones written to after `cutoff`. A process of a
/// version that takes no lock on the log writes into its moved log
/// without the lock showing it, and such a log is recent. One that is not
/// compressed, or fails on the way, stays plain for the next rotation,
/// and the others go on.
fn compress_old(dir: &Path, lock: LockFn, newest: &Path, cutoff: SystemTime) -> io::Result<()> {
    let mut moved: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path != newest
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_moved_log)
                && std::fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .is_ok_and(|written| written <= cutoff)
        })
        .collect();
    moved.sort();
    for path in moved {
        let _ = compress_file(&path, lock);
    }
    Ok(())
}

/// How a compressor tries the exclusive lock of a moved log.
type LockFn = fn(&File) -> Result<(), TryLockError>;

/// Replaces the moved log at `path` with its compressed form, once the
/// compressed form decodes to the same bytes.
///
/// The exclusive lock is the proof that no process writes into the file.
/// Only a lock that is granted counts: a failed attempt of any kind leaves
/// the file as it is, as the process that holds it may be one whose own
/// lock failed. The lock stays until the plain file is gone, so no line
/// is added between the check and the removal.
fn compress_file(path: &Path, lock: LockFn) -> io::Result<()> {
    let mut file = File::open(path)?;
    if !acquired(lock(&file))? {
        return Ok(());
    }
    let mut original = Vec::new();
    file.read_to_end(&mut original)?;

    let target = compressed_name(path);
    if target.exists() {
        // Whoever wrote it, it is not replaced: the plain file goes only
        // when it holds nothing more.
        let existing = std::fs::read(&target)?;
        if verify(&existing, &original).is_ok() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }

    let compressed = gzip(&original)?;
    verify(&compressed, &original)?;

    // The temporary file has one name per log, so one that a crash left
    // behind is overwritten by the next try.
    let mut temporary_name = OsString::from(".");
    temporary_name.push(target.file_name().expect("a moved log has a name"));
    temporary_name.push(".tmp");
    let temporary = target.with_file_name(temporary_name);
    let mut out = File::create(&temporary)?;
    out.write_all(&compressed)?;
    out.sync_all()?;
    std::fs::rename(&temporary, &target)?;
    // The plain file goes only once the rename of the compressed one is on
    // disk, or a crash could leave neither.
    File::open(path.parent().expect("a moved log has a directory"))?.sync_all()?;
    std::fs::remove_file(path)
}

/// `bytes` as one gzip stream at the best level.
fn gzip(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes)?;
    encoder.finish()
}

/// Reads `compressed` back and fails unless it decodes to exactly
/// `original`, whose bytes it compares chunk by chunk to not hold a third
/// copy.
fn verify(compressed: &[u8], original: &[u8]) -> io::Result<()> {
    let differs = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the compressed log does not decode to the original",
        )
    };
    let mut decoder = GzDecoder::new(compressed);
    let mut chunk = vec![0; 64 * 1024];
    let mut offset = 0;
    loop {
        let read = decoder.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if original.get(offset..offset + read) != Some(&chunk[..read]) {
            return Err(differs());
        }
        offset += read;
    }
    if offset == original.len() {
        Ok(())
    } else {
        Err(differs())
    }
}

/// The compressed form of the moved log at `plain`.
fn compressed_name(plain: &Path) -> PathBuf {
    let mut name = OsString::from(plain.as_os_str());
    name.push(".gz");
    PathBuf::from(name)
}

/// Whether `name` is `asqr-<UTC timestamp>.log`, the timestamp as
/// `YYYYMMDDTHHMMSSZ`.
fn is_moved_log(name: &str) -> bool {
    let Some(stamp) = name
        .strip_prefix("asqr-")
        .and_then(|rest| rest.strip_suffix(".log"))
    else {
        return false;
    };
    let stamp = stamp.as_bytes();
    stamp.len() == 16
        && stamp[..8].iter().all(u8::is_ascii_digit)
        && stamp[8] == b'T'
        && stamp[9..15].iter().all(u8::is_ascii_digit)
        && stamp[15] == b'Z'
}

/// The log as asqr writes it into `writer`: without colours, and with the
/// events that `variable`, the value of `ASQR_LOG`, lets through, `info`
/// and above without one. A value that does not parse lets `info` and
/// above through, and the log says so.
pub(crate) fn dispatch<W>(writer: W, variable: Option<&str>) -> Dispatch
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let value = variable.unwrap_or_default();
    // tracing's parser takes a space around a directive as part of it, so
    // ` debug` would name a target and ` asqr=debug` would not parse.
    let directives = value
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(",");
    let builder = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into());
    let (filter, rejected) = match builder.parse(&directives) {
        Ok(filter) => (filter, None),
        Err(error) => (builder.parse_lossy(""), Some(error)),
    };
    let dispatch = Dispatch::new(
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(writer)
            .with_ansi(false)
            .finish(),
    );
    // The TUI owns the terminal, so the warning goes into the log.
    if let Some(error) = rejected {
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::warn!(
                value,
                %error,
                "ASQR_LOG does not parse; the log takes info and above"
            );
        });
    }
    dispatch
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::EnvFilter;

    use crate::tui::render::test_support::logged_with;

    /// What the log holds with `variable` as `ASQR_LOG` after asqr logged
    /// a line at `debug` and one at `info`, and another library one line
    /// at each of them.
    fn log_with(variable: Option<&str>) -> String {
        logged_with(variable, || {
            tracing::debug!("asqr debug");
            tracing::info!("asqr info");
            tracing::debug!(target: "elsewhere", "library debug");
            tracing::info!(target: "elsewhere", "library info");
        })
    }

    #[test]
    fn without_asqr_log_the_log_takes_info_and_above() {
        let log = log_with(None);

        assert!(log.contains("asqr info"), "{log}");
        assert!(log.contains("library info"), "{log}");
        assert!(!log.contains("asqr debug"), "{log}");
        assert!(!log.contains("library debug"), "{log}");
    }

    #[test]
    fn a_level_in_asqr_log_applies_to_every_target() {
        let log = log_with(Some("debug"));

        assert!(log.contains("asqr debug"), "{log}");
        assert!(log.contains("library debug"), "{log}");
    }

    #[test]
    fn a_value_with_directives_replaces_the_info_default() {
        let log = log_with(Some("asqr::cli::log=debug"));

        assert!(log.contains("asqr debug"), "{log}");
        assert!(!log.contains("library info"), "{log}");
    }

    #[test]
    fn a_level_in_front_keeps_the_other_targets_at_it() {
        let log = log_with(Some("info,asqr=debug"));

        assert!(log.contains("asqr debug"), "{log}");
        assert!(log.contains("library info"), "{log}");
        assert!(!log.contains("library debug"), "{log}");
    }

    #[test]
    fn an_asqr_log_that_does_not_parse_takes_info_and_warns_about_it() {
        let error = EnvFilter::builder()
            .parse("asqr=loud")
            .expect_err("loud is no level");

        let log = log_with(Some("asqr=loud"));

        assert!(log.contains("asqr info"), "{log}");
        assert!(!log.contains("asqr debug"), "{log}");
        let warnings: Vec<&str> = log.lines().filter(|line| line.contains("WARN")).collect();
        assert_eq!(warnings.len(), 1, "{log}");
        assert!(warnings[0].contains("ASQR_LOG"), "{log}");
        assert!(warnings[0].contains(r#"value="asqr=loud""#), "{log}");
        assert!(warnings[0].contains(&error.to_string()), "{log}");
    }

    /// A bare word that is no level names a target, so a misspelled level
    /// lets nothing through, and no warning either.
    #[test]
    fn a_bare_word_that_is_no_level_is_a_target() {
        assert_eq!(log_with(Some("degub")), "");
    }

    #[test]
    fn spaces_around_directives_do_not_count() {
        let log = log_with(Some(" info , asqr=debug "));

        assert!(log.contains("asqr debug"), "{log}");
        assert!(log.contains("library info"), "{log}");
        assert!(!log.contains("WARN"), "{log}");
    }
}

#[cfg(test)]
mod rotation {
    use std::cell::Cell;

    use super::*;

    const SMALL_LIMIT: u64 = 100;
    const FIRST: &str = "2026-10-30T12:03:48Z";
    const FIRST_NAME: &str = "asqr-20261030T120348Z.log";
    const SECOND: &str = "2026-12-24T18:00:00Z";
    const SECOND_NAME: &str = "asqr-20261224T180000Z.log";

    fn at(stamp: &str) -> Timestamp {
        stamp.parse().expect("a timestamp")
    }

    /// A log of `tag` bytes, large enough for [`SMALL_LIMIT`] and as
    /// compressible as the real one.
    fn full(tag: u8) -> Vec<u8> {
        vec![tag; 200]
    }

    fn dir_with_log(bytes: &[u8]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join(LOG_NAME), bytes).expect("written");
        dir
    }

    /// The files of the directory that are neither `asqr.log` nor the
    /// rotation lock, sorted.
    fn kept(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("listed")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name != LOG_NAME && name != ROTATION_LOCK)
            .collect();
        names.sort();
        names
    }

    fn decoded(path: &Path) -> Vec<u8> {
        let mut bytes = Vec::new();
        GzDecoder::new(File::open(path).expect("opened"))
            .read_to_end(&mut bytes)
            .expect("gunzipped");
        bytes
    }

    /// Sets the last write to `path` to `hours` before the time `stamp`.
    fn written_hours_before(path: &Path, stamp: &str, hours: u64) {
        let time = SystemTime::from(at(stamp)) - Duration::from_secs(hours * 3600);
        OpenOptions::new()
            .write(true)
            .open(path)
            .expect("opened")
            .set_modified(time)
            .expect("time set");
    }

    /// A cutoff that every file written so far is before.
    fn in_an_hour() -> SystemTime {
        SystemTime::now() + Duration::from_secs(3600)
    }

    /// A writer of the log, such as a running TUI, as the shared lock on
    /// the file it opened.
    fn writer_of(path: &Path) -> File {
        let file = File::open(path).expect("opened");
        file.try_lock_shared().expect("locked shared");
        file
    }

    #[test]
    fn a_log_at_the_limit_moves_aside_under_a_timestamped_name() {
        let dir = dir_with_log(&full(b'a'));
        let _writer = writer_of(&dir.path().join(LOG_NAME));

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert!(!dir.path().join(LOG_NAME).exists());
        assert_eq!(kept(dir.path()), [FIRST_NAME]);
        assert_eq!(
            std::fs::read(dir.path().join(FIRST_NAME)).expect("read"),
            full(b'a')
        );
    }

    /// Green without any rotation as well: it pins what a log that is not
    /// due keeps.
    #[test]
    fn a_log_below_the_limit_stays() {
        let dir = dir_with_log(&full(b'a')[..99]);

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert_eq!(kept(dir.path()), Vec::<String>::new());
        assert_eq!(
            std::fs::read(dir.path().join(LOG_NAME))
                .expect("read")
                .len(),
            99
        );
    }

    /// Green without any rotation as well.
    #[test]
    fn a_missing_log_is_no_rotation() {
        let dir = tempfile::tempdir().expect("temp dir");

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert_eq!(kept(dir.path()), Vec::<String>::new());
    }

    /// Green without any rotation as well. Skipped for a user a read-only
    /// directory does not stop, such as root.
    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_written_is_no_rotation() {
        use std::os::unix::fs::PermissionsExt;

        let dir = dir_with_log(&full(b'a'));
        let read_only = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(dir.path(), read_only).expect("made read-only");
        let writable = std::fs::write(dir.path().join("probe"), b"").is_ok();

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        let log_stayed = dir.path().join(LOG_NAME).exists();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("made writable again");
        assert!(writable || log_stayed);
    }

    /// Green without any rotation as well. Skipped for a user a closed
    /// directory does not stop, such as root.
    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_searched_is_no_rotation() {
        use std::os::unix::fs::PermissionsExt;

        let dir = dir_with_log(&full(b'a'));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o000))
            .expect("closed");
        let searchable = std::fs::metadata(dir.path().join(LOG_NAME)).is_ok();

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("opened again");
        assert!(searchable || dir.path().join(LOG_NAME).exists());
    }

    /// The log has not been written to for two days, so only the rule
    /// that the rotation which moves a log does not compress it keeps it
    /// plain.
    #[test]
    fn the_rotation_that_moved_a_log_leaves_it_plain() {
        let dir = dir_with_log(&full(b'a'));
        written_hours_before(&dir.path().join(LOG_NAME), FIRST, 48);

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert_eq!(kept(dir.path()), [FIRST_NAME]);
        assert_eq!(
            std::fs::read(dir.path().join(FIRST_NAME)).expect("read"),
            full(b'a')
        );
    }

    /// Nobody holds the earlier log, so only its last write, 23 hours
    /// before the rotation, keeps it plain.
    #[test]
    fn a_plain_log_written_to_within_a_day_stays_plain() {
        let dir = dir_with_log(&full(b'b'));
        let earlier = dir.path().join("asqr-20260101T000000Z.log");
        std::fs::write(&earlier, full(b'a')).expect("written");
        written_hours_before(&earlier, FIRST, 23);

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert_eq!(
            kept(dir.path()),
            [
                "asqr-20260101T000000Z.log".to_owned(),
                FIRST_NAME.to_owned()
            ]
        );
    }

    #[test]
    fn a_later_rotation_compresses_a_plain_log_nobody_holds_that_is_a_day_old() {
        let dir = dir_with_log(&full(b'b'));
        let earlier = dir.path().join("asqr-20260101T000000Z.log");
        std::fs::write(&earlier, full(b'a')).expect("written");
        written_hours_before(&earlier, FIRST, 25);

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert_eq!(
            kept(dir.path()),
            [
                "asqr-20260101T000000Z.log.gz".to_owned(),
                FIRST_NAME.to_owned()
            ]
        );
        assert_eq!(
            decoded(&dir.path().join("asqr-20260101T000000Z.log.gz")),
            full(b'a')
        );
    }

    #[test]
    fn a_compressed_log_is_a_gzip_file_named_log_gz() {
        let dir = dir_with_log(&full(b'a'));
        let earlier = dir.path().join("asqr-20260101T000000Z.log");
        std::fs::write(&earlier, full(b'a')).expect("written");
        written_hours_before(&earlier, FIRST, 48);

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        let compressed = std::fs::read(dir.path().join("asqr-20260101T000000Z.log.gz"))
            .expect("a .log.gz beside the plain logs");
        assert_eq!(compressed[..2], [0x1f, 0x8b]);
        let mut original = Vec::new();
        flate2::read::GzDecoder::new(&compressed[..])
            .read_to_end(&mut original)
            .expect("gunzipped");
        assert_eq!(original, full(b'a'));
        assert!(!earlier.exists());
    }

    #[test]
    fn a_plain_log_a_process_still_holds_stays_plain() {
        let dir = dir_with_log(&full(b'b'));
        let earlier = dir.path().join("asqr-20260101T000000Z.log");
        std::fs::write(&earlier, full(b'a')).expect("written");
        written_hours_before(&earlier, FIRST, 48);
        let _writer = writer_of(&earlier);

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert_eq!(
            kept(dir.path()),
            [
                "asqr-20260101T000000Z.log".to_owned(),
                FIRST_NAME.to_owned()
            ]
        );
        assert_eq!(std::fs::read(&earlier).expect("read"), full(b'a'));
    }

    #[test]
    fn a_plain_log_is_compressed_once_the_holder_lets_go() {
        let dir = dir_with_log(&full(b'b'));
        let earlier = dir.path().join("asqr-20260101T000000Z.log");
        std::fs::write(&earlier, full(b'a')).expect("written");
        written_hours_before(&earlier, FIRST, 48);
        written_hours_before(&dir.path().join(LOG_NAME), FIRST, 0);
        let writer = writer_of(&earlier);
        rotate(dir.path(), SMALL_LIMIT, at(FIRST));
        drop(writer);
        std::fs::write(dir.path().join(LOG_NAME), full(b'c')).expect("written");

        rotate(dir.path(), SMALL_LIMIT, at(SECOND));

        assert_eq!(
            kept(dir.path()),
            [
                "asqr-20260101T000000Z.log.gz".to_owned(),
                format!("{FIRST_NAME}.gz"),
                SECOND_NAME.to_owned(),
            ]
        );
        assert_eq!(
            decoded(&dir.path().join("asqr-20260101T000000Z.log.gz")),
            full(b'a')
        );
    }

    #[test]
    fn a_lock_that_fails_leaves_the_plain_log() {
        let dir = tempfile::tempdir().expect("temp dir");
        let plain = dir.path().join(FIRST_NAME);
        std::fs::write(&plain, full(b'a')).expect("written");

        let result = compress_file(&plain, |_| {
            Err(TryLockError::Error(io::Error::other("no locks here")))
        });

        assert!(result.is_err());
        assert_eq!(kept(dir.path()), [FIRST_NAME]);
    }

    #[test]
    fn a_rotation_between_the_size_check_and_the_lock_leaves_one_moved_log() {
        let dir = dir_with_log(&full(b'a'));
        let ran = Cell::new(false);

        rotate_with(dir.path(), SMALL_LIMIT, at(FIRST), &mut |step| {
            if step == Step::SizeChecked {
                ran.set(true);
                rotate(dir.path(), SMALL_LIMIT, at(SECOND));
                std::fs::write(dir.path().join(LOG_NAME), b"fresh").expect("written");
            }
        });

        assert!(ran.get());
        assert_eq!(kept(dir.path()), [SECOND_NAME]);
        assert_eq!(
            std::fs::read(dir.path().join(LOG_NAME)).expect("read"),
            b"fresh"
        );
    }

    #[test]
    fn a_rotation_that_finds_the_lock_taken_does_not_wait_or_move_the_log() {
        let dir = dir_with_log(&full(b'a'));
        let ran = Cell::new(false);

        rotate_with(dir.path(), SMALL_LIMIT, at(FIRST), &mut |step| {
            if step == Step::Locked {
                ran.set(true);
                rotate(dir.path(), SMALL_LIMIT, at(SECOND));
                assert!(dir.path().join(LOG_NAME).exists());
                assert_eq!(kept(dir.path()), Vec::<String>::new());
            }
        });

        assert!(ran.get());
        assert_eq!(kept(dir.path()), [FIRST_NAME]);
    }

    #[test]
    fn two_rotations_in_one_second_rotate_once() {
        let dir = dir_with_log(&full(b'a'));
        rotate(dir.path(), SMALL_LIMIT, at(FIRST));
        assert_eq!(kept(dir.path()), [FIRST_NAME]);
        std::fs::write(dir.path().join(LOG_NAME), full(b'b')).expect("written");

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert_eq!(
            std::fs::read(dir.path().join(LOG_NAME)).expect("read"),
            full(b'b')
        );
        assert_eq!(
            std::fs::read(dir.path().join(FIRST_NAME)).expect("read"),
            full(b'a')
        );
    }

    /// Green without any rotation as well.
    #[test]
    fn a_compressed_log_of_the_same_second_is_never_replaced() {
        let dir = dir_with_log(&full(b'b'));
        let gz = dir.path().join(format!("{FIRST_NAME}.gz"));
        std::fs::write(&gz, b"kept").expect("written");

        rotate(dir.path(), SMALL_LIMIT, at(FIRST));

        assert_eq!(std::fs::read(&gz).expect("read"), b"kept");
        assert_eq!(
            std::fs::read(dir.path().join(LOG_NAME)).expect("read"),
            full(b'b')
        );
    }

    #[test]
    fn a_compression_that_does_not_decode_to_the_original_fails_verification() {
        let compressed = gzip(&full(b'a')).expect("compressed");

        assert!(verify(&compressed, &full(b'a')).is_ok());
        assert!(verify(&compressed, &full(b'b')).is_err());
        assert!(verify(&compressed, &full(b'a')[..199]).is_err());
        assert!(verify(&compressed[..compressed.len() / 2], &full(b'a')).is_err());
        let shorter = gzip(&full(b'a')[..100]).expect("compressed");
        assert!(verify(&shorter, &full(b'a')).is_err());
    }

    #[test]
    fn a_plain_log_with_a_compressed_twin_of_other_bytes_keeps_both() {
        let dir = tempfile::tempdir().expect("temp dir");
        let plain = dir.path().join(FIRST_NAME);
        std::fs::write(&plain, full(b'a')).expect("written");
        let twin = dir.path().join(format!("{FIRST_NAME}.gz"));
        let other = gzip(&full(b'b')).expect("compressed");
        std::fs::write(&twin, &other).expect("written");

        compress_file(&plain, File::try_lock).expect("left alone");

        assert_eq!(std::fs::read(&plain).expect("read"), full(b'a'));
        assert_eq!(std::fs::read(&twin).expect("read"), other);
    }

    #[test]
    fn a_plain_log_with_a_compressed_twin_of_the_same_bytes_goes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let plain = dir.path().join(FIRST_NAME);
        std::fs::write(&plain, full(b'a')).expect("written");
        let twin = dir.path().join(format!("{FIRST_NAME}.gz"));
        let same = gzip(&full(b'a')).expect("compressed");
        std::fs::write(&twin, &same).expect("written");

        compress_file(&plain, File::try_lock).expect("settled");

        assert_eq!(kept(dir.path()), [format!("{FIRST_NAME}.gz")]);
        assert_eq!(std::fs::read(&twin).expect("read"), same);
    }

    #[test]
    fn only_files_named_like_moved_logs_are_compressed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let foreign = [
            "asqr-notes.log",
            "asqr-notes-1234567890.log",
            "asqr-20250101T000000Z.log.gz",
            "asqr.log.gz",
            ".asqr-stray.tmp",
        ];
        for name in foreign {
            std::fs::write(dir.path().join(name), b"not mine").expect("written");
        }
        std::fs::write(dir.path().join(FIRST_NAME), full(b'a')).expect("written");

        compress_old(dir.path(), File::try_lock, Path::new("none"), in_an_hour()).expect("passed");

        let mut expected: Vec<String> = foreign.iter().map(|name| (*name).to_owned()).collect();
        expected.push(format!("{FIRST_NAME}.gz"));
        expected.sort();
        assert_eq!(kept(dir.path()), expected);
        for name in foreign {
            assert_eq!(
                std::fs::read(dir.path().join(name)).expect("read"),
                b"not mine"
            );
        }
    }

    #[test]
    fn a_moved_log_that_cannot_be_compressed_does_not_stop_the_others() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("asqr-20260101T000000Z.log");
        std::fs::write(&first, full(b'a')).expect("written");
        std::fs::write(dir.path().join(FIRST_NAME), full(b'b')).expect("written");
        // A directory where the temporary file belongs makes the first
        // log fail.
        std::fs::create_dir(dir.path().join(".asqr-20260101T000000Z.log.gz.tmp"))
            .expect("directory in the way");

        compress_old(dir.path(), File::try_lock, Path::new("none"), in_an_hour()).expect("passed");

        assert!(first.exists());
        assert_eq!(
            decoded(&dir.path().join(format!("{FIRST_NAME}.gz"))),
            full(b'b')
        );
    }

    #[test]
    fn a_log_moved_after_it_was_opened_is_opened_again() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(LOG_NAME);
        std::fs::write(&path, b"old\n").expect("written");
        let moved = dir.path().join(FIRST_NAME);
        let opens = Cell::new(0);

        let file = open_log_with(&path, || {
            opens.set(opens.get() + 1);
            if opens.get() == 1 {
                std::fs::rename(&path, &moved).expect("moved");
            }
        })
        .expect("opened");
        (&file).write_all(b"new\n").expect("written");

        assert_eq!(opens.get(), 2);
        assert_eq!(std::fs::read(&path).expect("read"), b"new\n");
        assert_eq!(std::fs::read(&moved).expect("read"), b"old\n");
    }

    /// Every file opened is moved and taken by a compressor before the lock
    /// is tried, as three rotations in a row would do.
    #[test]
    fn a_log_that_is_moved_and_compressed_every_time_is_opened_at_the_path_in_the_end() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(LOG_NAME);
        std::fs::write(&path, b"old\n").expect("written");
        let compressors = std::cell::RefCell::new(Vec::new());

        let file = open_log_with(&path, || {
            let moved = dir
                .path()
                .join(format!("moved-{}", compressors.borrow().len()));
            std::fs::rename(&path, &moved).expect("moved");
            let compressor = File::open(&moved).expect("opened");
            compressor.lock().expect("locked exclusively");
            compressors.borrow_mut().push(compressor);
        })
        .expect("opened");
        (&file).write_all(b"new\n").expect("written");

        assert_eq!(compressors.borrow().len(), OPEN_ATTEMPTS as usize);
        assert_eq!(std::fs::read(&path).expect("read"), b"new\n");
    }

    #[test]
    fn a_log_that_nobody_moves_is_opened_once_and_locked_shared() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(LOG_NAME);
        let opens = Cell::new(0);

        let _file = open_log_with(&path, || opens.set(opens.get() + 1)).expect("opened");

        assert_eq!(opens.get(), 1);
        let compressor = File::open(&path).expect("opened");
        assert!(matches!(
            compressor.try_lock(),
            Err(TryLockError::WouldBlock)
        ));
    }
}
