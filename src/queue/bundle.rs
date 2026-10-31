// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Month bundles of the archive (spec section 5.1): one zip per queue and
//! UTC calendar month, `archive/<YYYY-MM>.zip`, holding the finished
//! sessions and the results of that month.
//!
//! Entry names are `sessions/<id>.<ulid>.json`, `results/<id>.<ulid>.json`
//! and `replaced/<id>.<ulid>.json`. Other prefixes, such as `files/`, are
//! listed and left alone, so a later kind of entry only adds names.
//!
//! This module is the one place that reads and writes bundles. `status`,
//! `result`, `wait` and `withdraw` read through [`archived_standing`],
//! [`bundled_results`] and [`newest_session`]; the compaction writes
//! through [`prepare_bundle`].
//!
//! # Rules for readers
//!
//! A writer renames the new bundle into place first and removes the loose
//! file after. A reader therefore lists the loose sources (outbox, then
//! `archive/`) before it opens any bundle: an entry that is moving is then
//! seen twice at worst and never zero times. Callers that count entries
//! deduplicate by entry name.
//!
//! # Rules for writers
//!
//! A bundle is never appended to in place. [`prepare_bundle`] writes a new
//! file beside the old one without any lock, [`PreparedBundle::still_current`]
//! and [`PreparedBundle::commit`] run under the drafts lock, so a `prune`
//! that deleted the bundle in between is noticed and the bundle does not
//! come back. A temp file `.asqr-*.tmp` older than 24 hours is stale; only
//! the compaction deletes those, as the temp file of a writer in progress
//! is seconds old.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use jiff::tz::TimeZone;
use sha2::{Digest, Sha256};
use thiserror::Error;
use ulid::Ulid;
use zip::write::FullFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::QueueLocation;
use super::atomic::temp_file_in;
use super::names::{Archived, entries_if_present, parse_archive_name};
use crate::format::{is_valid_session_id, same_session_id};

/// The id of the extra field that holds the time an entry was bundled as
/// four bytes of Unix seconds in UTC, little endian. The zip format's own
/// modification time has two-second resolution and no zone. The id is not
/// one of the standard ones, as the crate drops the standard timestamp
/// field when it copies an entry over unchanged.
const BUNDLED_AT_FIELD: u16 = 0xA5C1;

/// Deflate level of every entry written.
const DEFLATE_LEVEL: i64 = 9;

/// A UTC calendar month, the unit of a bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month {
    year: i32,
    month: u8,
}

impl Month {
    /// The month `month` (1 to 12) of `year`, or `None` for another month
    /// number.
    pub fn new(year: i32, month: u8) -> Option<Month> {
        (1..=12).contains(&month).then_some(Month { year, month })
    }

    /// The UTC month a ULID's time falls in.
    pub fn of(ulid: Ulid) -> Month {
        Month::of_time(ulid.datetime())
    }

    pub fn of_time(time: SystemTime) -> Month {
        let zoned = zoned_utc(time);
        Month {
            year: i32::from(zoned.year()),
            month: u8::try_from(zoned.month()).expect("a month number is 1 to 12"),
        }
    }

    /// The first instant of the month.
    pub fn start(self) -> SystemTime {
        let first = jiff::civil::date(
            i16::try_from(self.year).expect("years of ULIDs fit i16"),
            i8::try_from(self.month).expect("a month number is 1 to 12"),
            1,
        );
        SystemTime::from(
            first
                .to_zoned(TimeZone::UTC)
                .expect("the first of a month exists in UTC")
                .timestamp(),
        )
    }

    /// The first instant of the next month: everything of this month lies
    /// before it.
    pub fn end(self) -> SystemTime {
        let next = if self.month == 12 {
            Month {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Month {
                year: self.year,
                month: self.month + 1,
            }
        };
        next.start()
    }
}

impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

impl FromStr for Month {
    type Err = ();

    /// `YYYY-MM` with exactly four digits for the year.
    fn from_str(text: &str) -> Result<Month, ()> {
        let (year, month) = text.split_once('-').ok_or(())?;
        let digits = |part: &str, len: usize| {
            part.len() == len && part.bytes().all(|byte| byte.is_ascii_digit())
        };
        if !digits(year, 4) || !digits(month, 2) {
            return Err(());
        }
        Month::new(
            year.parse().map_err(|_| ())?,
            month.parse().map_err(|_| ())?,
        )
        .ok_or(())
    }
}

fn zoned_utc(time: SystemTime) -> jiff::Zoned {
    jiff::Timestamp::try_from(time)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(TimeZone::UTC)
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// =========================================================================
// Entry names
// =========================================================================

/// What a bundle entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A finished session: `sessions/`.
    Session,
    /// A result that left the outbox: `results/`.
    Result,
    /// A result `asqr ask --force` moved aside, which answers no session
    /// any more: `replaced/`.
    Replaced,
}

impl EntryKind {
    fn prefix(self) -> &'static str {
        match self {
            EntryKind::Session => "sessions",
            EntryKind::Result => "results",
            EntryKind::Replaced => "replaced",
        }
    }
}

/// The parsed name of an entry in a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryName {
    pub kind: EntryKind,
    pub id: String,
    /// Unique per entry. Its time decides the month of the bundle.
    pub ulid: Ulid,
}

impl EntryName {
    /// The name the entry has inside the zip.
    pub fn zip_name(&self) -> String {
        format!("{}/{}.{}.json", self.kind.prefix(), self.id, self.ulid)
    }

    /// Parses `<prefix>/<id>.<ulid>.json`; anything else, `files/...`
    /// included, is not an entry this module reads.
    pub fn parse(zip_name: &str) -> Option<EntryName> {
        let (prefix, rest) = zip_name.split_once('/')?;
        let kind = [EntryKind::Session, EntryKind::Result, EntryKind::Replaced]
            .into_iter()
            .find(|kind| kind.prefix() == prefix)?;
        // Ids may contain `.`, so the ULID is taken from the end.
        let (id, ulid) = rest.strip_suffix(".json")?.rsplit_once('.')?;
        is_valid_session_id(id).then_some(())?;
        Some(EntryName {
            kind,
            id: id.to_owned(),
            ulid: Ulid::from_string(ulid).ok()?,
        })
    }
}

/// The entry name of the result `bytes` of session `id`, which was
/// finished at `finished`. Built from the content, so the same file always
/// gets the same name and a compaction that was interrupted finds its
/// entry again instead of adding a second one. The time part makes the
/// result lie in the month it was finished in.
pub fn result_entry_name(id: &str, bytes: &[u8], finished: SystemTime) -> EntryName {
    let digest = Sha256::digest(bytes);
    let random = digest[..10]
        .iter()
        .fold(0u128, |acc, byte| (acc << 8) | u128::from(*byte));
    let millis = u64::try_from(
        finished
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(0);
    EntryName {
        kind: EntryKind::Result,
        id: id.to_owned(),
        ulid: Ulid::from_parts(millis, random),
    }
}

// =========================================================================
// Errors
// =========================================================================

/// A bundle that cannot be read or written, with the file named.
#[derive(Debug, Error)]
pub enum BundleError {
    #[error("cannot read {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },

    #[error("cannot read {}: {source}", path.display())]
    Zip {
        path: PathBuf,
        source: zip::result::ZipError,
    },
}

impl BundleError {
    fn io(path: &Path, source: io::Error) -> BundleError {
        BundleError::Io {
            path: path.to_owned(),
            source,
        }
    }

    fn zip(path: &Path, source: zip::result::ZipError) -> BundleError {
        BundleError::Zip {
            path: path.to_owned(),
            source,
        }
    }
}

// =========================================================================
// Reading
// =========================================================================

/// The bundle of `month`.
pub fn bundle_path(location: &QueueLocation, month: Month) -> PathBuf {
    location.archive().join(format!("{month}.zip"))
}

/// The months that have a bundle, oldest first. Files in `archive/` that
/// are no bundle by name are not months.
pub fn list_months(location: &QueueLocation) -> io::Result<Vec<Month>> {
    let mut months: Vec<Month> = entries_if_present(&location.archive())?
        .iter()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()?
                .strip_suffix(".zip")?
                .parse()
                .ok()
        })
        .collect();
    months.sort();
    Ok(months)
}

/// One entry of a bundle, as its directory lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleEntry {
    /// The name inside the zip.
    pub name: String,
    /// The name as an entry of sessions, results or replaced results;
    /// `None` for other entries, such as kept files.
    pub parsed: Option<EntryName>,
    /// Size of the content, uncompressed.
    pub size: u64,
    pub crc32: u32,
    /// Unix seconds, UTC, at which the entry was bundled; `None` for an
    /// entry written without the time.
    pub bundled_at: Option<u64>,
}

fn open(path: &Path) -> Result<ZipArchive<File>, BundleError> {
    let file = File::open(path).map_err(|error| BundleError::io(path, error))?;
    ZipArchive::new(file).map_err(|error| BundleError::zip(path, error))
}

fn bundled_at_of<R: Read>(file: &zip::read::ZipFile<'_, R>) -> Option<u64> {
    file.extra_data_fields().find_map(|field| {
        let zip::extra_fields::ExtraField::Custom(custom) = field else {
            return None;
        };
        let seconds: [u8; 4] = (custom.header_id == BUNDLED_AT_FIELD)
            .then(|| custom.data.as_ref().try_into().ok())??;
        Some(u64::from(u32::from_le_bytes(seconds)))
    })
}

/// The entries of the bundle of `month`, in the order the zip lists them.
pub fn list_entries(
    location: &QueueLocation,
    month: Month,
) -> Result<Vec<BundleEntry>, BundleError> {
    let path = bundle_path(location, month);
    let mut archive = open(&path)?;
    entries_of(&mut archive, &path)
}

fn entries_of(
    archive: &mut ZipArchive<File>,
    path: &Path,
) -> Result<Vec<BundleEntry>, BundleError> {
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|error| BundleError::zip(path, error))?;
        let name = file
            .name()
            .map_err(|error| BundleError::zip(path, error))?
            .into_owned();
        entries.push(BundleEntry {
            parsed: EntryName::parse(&name),
            name,
            size: file.size(),
            crc32: file.crc32(),
            bundled_at: bundled_at_of(&file),
        });
    }
    Ok(entries)
}

/// The content of the entry `name` of the bundle of `month`. The zip
/// checks the entry's checksum while it is read.
pub fn read_entry(
    location: &QueueLocation,
    month: Month,
    name: &str,
) -> Result<Vec<u8>, BundleError> {
    let path = bundle_path(location, month);
    read_entry_of(&mut open(&path)?, &path, name)
}

fn read_entry_of(
    archive: &mut ZipArchive<File>,
    path: &Path,
    name: &str,
) -> Result<Vec<u8>, BundleError> {
    let mut file = archive
        .by_name(name)
        .map_err(|error| BundleError::zip(path, error))?;
    let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
    file.read_to_end(&mut bytes)
        .map_err(|error| BundleError::io(path, error))?;
    Ok(bytes)
}

/// What the archive says about the result of a session whose file is in
/// neither the outbox nor the inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchivedStanding {
    /// The result, as the file holds it.
    Result(Vec<u8>),
    /// The archive has the session or a result moved aside, but no result
    /// that answers it: the result was removed.
    Gone,
    /// The archive has nothing under this id.
    Unknown,
}

/// What the archive, loose entries and bundles together, says about
/// session `id`, compared ignoring case.
///
/// The newest bundled result answers, unless a session of the id was
/// archived after that result was bundled: that session never got a
/// result in the outbox (a result is written before its session is
/// archived), so the bundled one belongs to an earlier session and the
/// answer to this one is gone. A session archived unanswered next to an
/// unread result (spec section 5.7) was archived before the result was
/// bundled, so it still shows that result.
///
/// An unreadable bundle is skipped, and is an error only when nothing
/// else says anything about the id.
pub fn archived_standing(
    location: &QueueLocation,
    id: &str,
) -> Result<ArchivedStanding, BundleError> {
    let mut newest_session: Option<Ulid> = None;
    let mut replaced = false;
    let note_session = |ulid: Ulid, newest: &mut Option<Ulid>| {
        *newest = Some(newest.map_or(ulid, |seen| seen.max(ulid)));
    };

    // Loose entries first, so an entry that moves into a bundle meanwhile
    // is found in the bundle (module docs).
    for entry in entries_if_present(&location.archive())
        .map_err(|error| BundleError::io(&location.archive(), error))?
    {
        let Some(parsed) = entry.file_name().to_str().and_then(parse_archive_name) else {
            continue;
        };
        if !same_session_id(&parsed.id, id) {
            continue;
        }
        match parsed.kind {
            Archived::Session => note_session(parsed.ulid, &mut newest_session),
            Archived::Result => replaced = true,
        }
    }

    let mut newest_result: Option<(Ulid, Month, String, Option<u64>)> = None;
    let mut first_error = None;
    for month in
        list_months(location).map_err(|error| BundleError::io(&location.archive(), error))?
    {
        let entries = match list_entries(location, month) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, "skipped an unreadable bundle");
                first_error.get_or_insert(error);
                continue;
            }
        };
        for entry in entries {
            let Some(parsed) = entry
                .parsed
                .filter(|parsed| same_session_id(&parsed.id, id))
            else {
                continue;
            };
            match parsed.kind {
                EntryKind::Session => note_session(parsed.ulid, &mut newest_session),
                EntryKind::Replaced => replaced = true,
                EntryKind::Result => {
                    if newest_result
                        .as_ref()
                        .is_none_or(|(seen, ..)| parsed.ulid > *seen)
                    {
                        newest_result = Some((parsed.ulid, month, entry.name, entry.bundled_at));
                    }
                }
            }
        }
    }

    if let Some((_, month, name, bundled_at)) = newest_result {
        // An entry without the time is never superseded.
        let superseded = bundled_at
            .zip(newest_session)
            .is_some_and(|(bundled_at, session)| unix_seconds(session.datetime()) > bundled_at + 1);
        return if superseded {
            Ok(ArchivedStanding::Gone)
        } else {
            read_entry(location, month, &name).map(ArchivedStanding::Result)
        };
    }
    if newest_session.is_some() || replaced {
        return Ok(ArchivedStanding::Gone);
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(ArchivedStanding::Unknown),
    }
}

/// The newest finished session of `id` over the loose archive entries and
/// the bundles, as its ULID and file content. `None` when there is none or
/// the newest cannot be read.
pub fn newest_session(location: &QueueLocation, id: &str) -> io::Result<Option<(Ulid, Vec<u8>)>> {
    enum Place {
        Loose(PathBuf),
        Bundled(Month, String),
    }
    let mut newest: Option<(Ulid, Place)> = None;
    let mut consider = |ulid: Ulid, place: Place| {
        if newest.as_ref().is_none_or(|(seen, _)| ulid > *seen) {
            newest = Some((ulid, place));
        }
    };
    for entry in entries_if_present(&location.archive())? {
        if let Some(parsed) = entry.file_name().to_str().and_then(parse_archive_name)
            && parsed.kind == Archived::Session
            && same_session_id(&parsed.id, id)
        {
            consider(parsed.ulid, Place::Loose(entry.path()));
        }
    }
    for month in list_months(location)? {
        let Ok(entries) = list_entries(location, month) else {
            continue;
        };
        for entry in entries {
            if let Some(parsed) = entry.parsed
                && parsed.kind == EntryKind::Session
                && same_session_id(&parsed.id, id)
            {
                consider(parsed.ulid, Place::Bundled(month, entry.name));
            }
        }
    }
    Ok(newest.and_then(|(ulid, place)| {
        let bytes = match place {
            Place::Loose(path) => std::fs::read(path).ok()?,
            Place::Bundled(month, name) => read_entry(location, month, &name).ok()?,
        };
        Some((ulid, bytes))
    }))
}

/// A result in a bundle, known by its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledResult {
    pub month: Month,
    pub name: EntryName,
    /// The name inside the zip.
    pub zip_name: String,
}

/// Every `results/` entry of every bundle, oldest month first, without
/// reading any content, and the bundles that could not be read.
pub fn bundled_results(
    location: &QueueLocation,
) -> io::Result<(Vec<BundledResult>, Vec<BundleError>)> {
    let mut found = Vec::new();
    let mut unreadable = Vec::new();
    for month in list_months(location)? {
        match list_entries(location, month) {
            Ok(entries) => found.extend(entries.into_iter().filter_map(|entry| {
                let name = entry.parsed.filter(|name| name.kind == EntryKind::Result)?;
                Some(BundledResult {
                    month,
                    name,
                    zip_name: entry.name,
                })
            })),
            Err(error) => unreadable.push(error),
        }
    }
    Ok((found, unreadable))
}

// =========================================================================
// Writing
// =========================================================================

/// An entry to add to a bundle.
#[derive(Debug, Clone)]
pub struct NewEntry {
    pub name: EntryName,
    pub bytes: Vec<u8>,
}

/// What became of one [`NewEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// The entry is written into the new bundle.
    Added,
    /// The bundle holds an entry of that name with the same content, which
    /// was read back and compared. It keeps the time it was bundled at.
    AlreadyThere,
    /// The bundle holds an entry of that name with other content, or one
    /// that cannot be read. Nothing is written for it.
    Conflict,
}

/// Length and modification time of a file, to tell whether it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp(path: &Path) -> io::Result<Option<Stamp>> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(Stamp {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// A new bundle written beside the old one, not yet in place.
pub struct PreparedBundle {
    target: PathBuf,
    /// Absent when nothing is to be added: the old bundle stays as it is.
    temp: Option<tempfile::NamedTempFile>,
    seen: Option<Stamp>,
    outcomes: Vec<Added>,
}

impl PreparedBundle {
    /// What became of each entry given to [`prepare_bundle`], in order.
    pub fn outcomes(&self) -> &[Added] {
        &self.outcomes
    }

    /// Whether the bundle file is still the one the new bundle was built
    /// from: the same length and modification time, or still absent. The
    /// caller holds the drafts lock, as a `prune` deletes bundles under it.
    pub fn still_current(&self) -> io::Result<bool> {
        Ok(stamp(&self.target)? == self.seen)
    }

    /// Puts the new bundle in place of the old one. The directory is
    /// synced after the rename; a platform that refuses to sync a
    /// directory leaves the rename as durable as it is.
    pub fn commit(self) -> Result<(), BundleError> {
        let Some(temp) = self.temp else {
            return Ok(());
        };
        temp.persist(&self.target)
            .map_err(|error| BundleError::io(&self.target, error.error))?;
        let dir = self.target.parent().expect("a bundle lies in archive/");
        let _ = File::open(dir).and_then(|dir| dir.sync_all());
        Ok(())
    }
}

/// Builds the bundle of `month` with `entries` added, as a temp file in
/// `archive/`, without touching the bundle that is there. Entries already
/// compressed are copied over as they are. `bundled_at` is the time each
/// new entry records.
///
/// An existing bundle that cannot be read is an error and stays as it is.
pub fn prepare_bundle(
    location: &QueueLocation,
    month: Month,
    entries: &[NewEntry],
    bundled_at: SystemTime,
) -> Result<PreparedBundle, BundleError> {
    let target = bundle_path(location, month);
    let seen = stamp(&target).map_err(|error| BundleError::io(&target, error))?;
    let mut existing = match seen {
        Some(_) => Some(open(&target)?),
        None => None,
    };

    // Reading an entry back checks its checksum, which the directory alone
    // does not: a rotten body must not let the loose file go.
    let mut outcomes = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = entry.name.zip_name();
        let outcome = match existing.as_mut() {
            Some(archive) if archive.index_for_name(&name).is_some() => {
                match read_entry_of(archive, &target, &name) {
                    Ok(bytes) if bytes == entry.bytes => Added::AlreadyThere,
                    _ => Added::Conflict,
                }
            }
            _ => Added::Added,
        };
        outcomes.push(outcome);
    }
    if !outcomes.contains(&Added::Added) {
        return Ok(PreparedBundle {
            target,
            temp: None,
            seen,
            outcomes,
        });
    }

    let archive_dir = location.archive();
    let temp = temp_file_in(&archive_dir).map_err(|error| BundleError::io(&archive_dir, error))?;
    let temp_path = temp.path().to_owned();
    let write_error = |error: zip::result::ZipError| BundleError::zip(&temp_path, error);
    let mut writer = ZipWriter::new(
        temp.as_file()
            .try_clone()
            .map_err(|error| BundleError::io(&temp_path, error))?,
    );
    if let Some(archive) = existing.as_mut() {
        for index in 0..archive.len() {
            let file = archive
                .by_index_raw(index)
                .map_err(|error| BundleError::zip(&target, error))?;
            writer.raw_copy_file(file).map_err(write_error)?;
        }
    }
    let modified = zip_time(bundled_at);
    let seconds = u32::try_from(unix_seconds(bundled_at)).unwrap_or(u32::MAX);
    for (entry, outcome) in entries.iter().zip(&outcomes) {
        if *outcome != Added::Added {
            continue;
        }
        let mut options = FullFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(DEFLATE_LEVEL))
            .last_modified_time(modified);
        options
            .add_extra_field(BUNDLED_AT_FIELD, seconds.to_le_bytes(), false)
            .map_err(write_error)?;
        writer
            .start_file(entry.name.zip_name(), options)
            .map_err(write_error)?;
        writer
            .write_all(&entry.bytes)
            .map_err(|error| BundleError::io(&temp_path, error))?;
    }
    let file = writer.finish().map_err(write_error)?;
    file.sync_all()
        .map_err(|error| BundleError::io(&temp_path, error))?;

    // Everything added is read back from the finished file before it can
    // replace the bundle; the zip checks each entry's checksum as it reads.
    let mut written = open(&temp_path)?;
    for (entry, outcome) in entries.iter().zip(&outcomes) {
        if *outcome == Added::Added {
            read_entry_of(&mut written, &temp_path, &entry.name.zip_name())?;
        }
    }
    Ok(PreparedBundle {
        target,
        temp: Some(temp),
        seen,
        outcomes,
    })
}

fn zip_time(time: SystemTime) -> DateTime {
    let zoned = zoned_utc(time);
    DateTime::from_date_and_time(
        u16::try_from(zoned.year()).unwrap_or(1980),
        u8::try_from(zoned.month()).unwrap_or(1),
        u8::try_from(zoned.day()).unwrap_or(1),
        u8::try_from(zoned.hour()).unwrap_or(0),
        u8::try_from(zoned.minute()).unwrap_or(0),
        u8::try_from(zoned.second()).unwrap_or(0),
    )
    .unwrap_or_default()
}

/// Adds `entries` to the bundle of `month`: [`prepare_bundle`] and
/// [`PreparedBundle::commit`] for callers that need no lock in between.
pub fn add_entries(
    location: &QueueLocation,
    month: Month,
    entries: &[NewEntry],
    bundled_at: SystemTime,
) -> Result<Vec<Added>, BundleError> {
    add_entries_with(location, month, entries, bundled_at, &mut || Ok(()))
}

/// [`add_entries`] that calls `before_commit` after the new bundle is
/// written and before it replaces the old one. An error from it drops the
/// new bundle and is returned. Tests look at the directory in between.
pub fn add_entries_with(
    location: &QueueLocation,
    month: Month,
    entries: &[NewEntry],
    bundled_at: SystemTime,
    before_commit: &mut dyn FnMut() -> io::Result<()>,
) -> Result<Vec<Added>, BundleError> {
    let prepared = prepare_bundle(location, month, entries, bundled_at)?;
    before_commit().map_err(|error| BundleError::io(&prepared.target, error))?;
    let outcomes = prepared.outcomes.clone();
    prepared.commit()?;
    Ok(outcomes)
}

/// The age after which a temp file in `archive/` counts as left behind.
pub const STALE_TEMP_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[cfg(test)]
mod tests;
