// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr replay <ref>` (spec section 6.5): opens one finished session in
//! the terminal UI, read-only.

use std::path::{Path, PathBuf};

use jiff::tz::TimeZone;

use super::Exit;
use crate::queue::{
    BundleError, EntryPlace, EntryRef, LoadedReplay, QueueLocation, Resolution, bundle_path,
    describe_entry, load_replay, resolve_reference,
};
use crate::settings::{Locks, SettingsStore};
use crate::tui::{OpenedFiles, ReplayInfo, SessionState, replay_time_text};

/// A finished session loaded and ready to be shown: the terminal UI takes
/// it from here.
pub struct Replay {
    location: QueueLocation,
    loaded: LoadedReplay,
    opened: Option<OpenedFiles>,
    settings: SettingsStore,
    no_color: bool,
    home: Option<PathBuf>,
}

impl Replay {
    /// The queue the session was found in.
    pub fn location(&self) -> &QueueLocation {
        &self.location
    }

    /// The reference of the entry.
    pub fn reference(&self) -> String {
        self.loaded.entry.entry.reference()
    }

    /// The state of the session to show, with its answers filled in.
    pub fn state(&self) -> SessionState {
        let loaded = &self.loaded;
        let entry = &loaded.entry;
        let zone = TimeZone::system();
        let bundle = match &entry.entry.place {
            EntryPlace::Bundled(month, _) => bundle_path(&entry.entry.queue, *month)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            EntryPlace::Loose(_) => None,
        };
        let info = ReplayInfo {
            reference: entry.entry.reference(),
            outcome: entry.outcome,
            finished: replay_time_text(entry.finished, &zone),
            files: loaded.files.clone(),
            bundle,
            zone,
        };
        SessionState::replay(
            entry.entry.id.clone(),
            loaded.session.clone(),
            loaded.draft.as_ref(),
        )
        .with_replay_info(info)
    }

    pub fn settings(&self) -> &SettingsStore {
        &self.settings
    }

    pub fn no_color(&self) -> bool {
        self.no_color
    }

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// The files the replay wrote for the session to show, which `o` and
    /// the run loop go on using.
    pub fn take_opened(&mut self) -> OpenedFiles {
        self.opened.take().expect("the files are taken once")
    }
}

pub(super) struct ReplayRequest<'a> {
    pub reference: &'a str,
    pub root: Option<&'a Path>,
    pub config: Option<PathBuf>,
    pub no_color: bool,
    pub home: Option<PathBuf>,
}

pub(super) fn run(
    location: &QueueLocation,
    request: ReplayRequest<'_>,
    ui: impl FnOnce(Replay) -> Exit,
) -> Exit {
    let ReplayRequest {
        reference,
        root,
        config,
        no_color,
        home,
    } = request;
    let (entry, unreadable) = match find_entry(location, root, reference) {
        Ok(found) => found,
        Err(exit) => return exit,
    };
    // The readable bundles name exactly one session, so it is shown; the
    // warning tells that the search left some bundles out.
    for error in &unreadable {
        eprintln!("warning: {error}; its sessions could not be searched for this REF");
    }

    // The copies of the files go into the directory of this run, which `o`
    // and the viewers read afterwards.
    let mut opened = OpenedFiles::in_temp_dir();
    let loaded = match load_replay(&entry, &mut |name, bytes| opened.extract(name, bytes)) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("error: {error}");
            return Exit::Failure;
        }
    };
    let loaded_settings = SettingsStore::load(config, Locks::default());
    for warning in &loaded_settings.warnings {
        tracing::warn!("{warning}");
    }
    ui(Replay {
        location: entry.queue.clone(),
        loaded,
        opened: Some(opened),
        settings: loaded_settings.store,
        no_color,
        home,
    })
}

/// The finished session `reference` names in the queues under `root` and
/// in `location` where that is a directory given by `--dir`, with the
/// bundles that could not be searched, or the exit to end with after the
/// reason has been printed.
pub(super) fn find_entry(
    location: &QueueLocation,
    root: Option<&Path>,
    reference: &str,
) -> Result<(EntryRef, Vec<BundleError>), Exit> {
    // A queue given as a directory outside the data directory is searched
    // only when it is named.
    let extra = location.root().is_none().then_some(location);
    if root.is_none() && extra.is_none() {
        eprintln!("error: no platform data directory found, so there are no queues to search");
        return Err(Exit::Failure);
    }
    let resolved = match resolve_reference(root, extra, reference) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("error: cannot read the queues: {error}");
            return Err(Exit::Failure);
        }
    };
    let entry = match resolved.resolution {
        Resolution::Found(entry) => entry,
        Resolution::TooShort => {
            eprintln!("asqr: \"{reference}\" is too short; give at least 6 characters of the REF");
            return Err(Exit::Usage);
        }
        Resolution::TooLong => {
            eprintln!("asqr: \"{reference}\" is too long; a REF has at most 8 characters");
            return Err(Exit::Usage);
        }
        Resolution::NotAReference(letter) => {
            eprintln!("asqr: \"{reference}\" is no REF: no REF holds the letter {letter}");
            return Err(Exit::Usage);
        }
        Resolution::NoMatch => {
            // The bundle that cannot be read may hold the entry.
            if let Some(error) = resolved.unreadable.first() {
                eprintln!("error: {error}");
                return Err(Exit::Failure);
            }
            eprintln!(
                "asqr: no finished session ends in \"{reference}\" in any queue under the data directory; a queue given with --dir is only searched when named"
            );
            return Err(Exit::Failure);
        }
        Resolution::Ambiguous(candidates) => {
            eprintln!(
                "asqr: \"{reference}\" ends {} finished sessions; give more of the REF:",
                candidates.len()
            );
            print_candidates(&candidates);
            return Err(Exit::Failure);
        }
    };
    Ok((entry, resolved.unreadable))
}

/// One line for each of `candidates`, the newest first: the reference, the
/// queue, when the session was archived and its title.
fn print_candidates(candidates: &[EntryRef]) {
    let zone = TimeZone::system();
    let names: Vec<String> = candidates
        .iter()
        .map(|entry| {
            entry
                .queue
                .name()
                .map_or_else(|| entry.queue.dir().display().to_string(), str::to_owned)
        })
        .collect();
    let width = names
        .iter()
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(0);
    for (entry, name) in candidates.iter().zip(&names) {
        let title = describe_entry(entry)
            .ok()
            .and_then(|described| described.title)
            .unwrap_or_else(|| entry.id.clone());
        let when = jiff::Timestamp::try_from(entry.ulid.datetime())
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_zoned(zone.clone())
            .strftime("%d %b %H:%M");
        eprintln!("  {}  {name:<width$}  {when}  {title}", entry.reference());
    }
}
