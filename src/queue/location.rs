// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Which queue a command works on (spec section 5.2).

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::format::is_valid_session_id;

/// The queue every command uses unless told otherwise.
pub const DEFAULT_QUEUE: &str = "default";

/// A queue chosen one way: by name under the platform root, or as a
/// directory. Command-line flags and environment variables each give one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub queue: Option<String>,
    pub dir: Option<PathBuf>,
}

impl Selection {
    /// Whether this source chose no queue.
    pub fn is_empty(&self) -> bool {
        self.queue.is_none() && self.dir.is_none()
    }
}

/// What the tmux session of this process says about its queue (ADR 85),
/// the source after the flags and the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Paired {
    /// tmux was not asked: asqr runs outside tmux, or a flag or a
    /// variable chose the queue, or the command works on no queue.
    #[default]
    NotAsked,
    /// The queue the session's asqr pinned (`ASQR_SESSION_QUEUE`).
    Pinned(String),
    /// The session's name, made a valid queue name.
    Session(String),
    /// tmux did not answer, for the reason given; asqr goes on as outside
    /// tmux.
    Unreachable(String),
}

/// Where the resolved queue came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Flag,
    Environment,
    Pin,
    SessionName,
    /// Nothing chose a queue, so it is the default one.
    Default,
    /// The default queue, taken because tmux did not answer.
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LocationError {
    #[error("--queue and --dir (or ASQR_QUEUE and ASQR_DIR) cannot be used together")]
    QueueAndDir,

    #[error("invalid queue name {0:?} (letters, digits, '-', '_' and '.', not starting with '.')")]
    InvalidQueueName(String),

    #[error(
        "invalid queue name {0:?} in the tmux session variable ASQR_SESSION_QUEUE (letters, digits, '-', '_' and '.', not starting with '.')"
    )]
    InvalidPinnedQueue(String),

    #[error("no platform data directory found; use --dir to name a queue directory")]
    NoDataDirectory,
}

/// A resolved queue: its directory, and for a named queue the root it
/// lives under and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueLocation {
    dir: PathBuf,
    root: Option<PathBuf>,
    name: Option<String>,
}

impl QueueLocation {
    /// Resolves the queue. Flags win over the environment, and the
    /// environment wins over the default queue under `root`, the platform
    /// data directory. Within one source, a name and a directory together
    /// are ambiguous, so they are refused.
    pub fn resolve(
        flags: Selection,
        environment: Selection,
        root: Option<PathBuf>,
    ) -> Result<Self, LocationError> {
        Self::resolve_paired(flags, environment, Paired::NotAsked, root)
            .map(|(location, _)| location)
    }

    /// Resolves the queue as [`QueueLocation::resolve`] does, with what
    /// tmux said as the source after the environment (ADR 85): the pinned
    /// queue, then the session's name. Says where the queue came from.
    pub fn resolve_paired(
        flags: Selection,
        environment: Selection,
        paired: Paired,
        root: Option<PathBuf>,
    ) -> Result<(Self, Origin), LocationError> {
        let named = |name: String| Selection {
            queue: Some(name),
            dir: None,
        };
        let (chosen, origin) = if !flags.is_empty() {
            (flags, Origin::Flag)
        } else if !environment.is_empty() {
            (environment, Origin::Environment)
        } else {
            match paired {
                // The pin is written by asqr with a valid name, so one that
                // is not was set by hand; it is refused as a bad
                // `ASQR_QUEUE` is, naming where it came from.
                Paired::Pinned(name) if !is_valid_session_id(&name) => {
                    return Err(LocationError::InvalidPinnedQueue(name));
                }
                Paired::Pinned(name) => (named(name), Origin::Pin),
                Paired::Session(name) => (named(name), Origin::SessionName),
                Paired::Unreachable(_) => (Selection::default(), Origin::Unreachable),
                Paired::NotAsked => (Selection::default(), Origin::Default),
            }
        };
        let location = match chosen {
            Selection {
                queue: Some(_),
                dir: Some(_),
            } => return Err(LocationError::QueueAndDir),
            Selection { dir: Some(dir), .. } => QueueLocation {
                dir,
                root: None,
                name: None,
            },
            Selection { queue, .. } => {
                let name = queue.unwrap_or_else(|| DEFAULT_QUEUE.to_owned());
                // The name becomes a directory name, so it follows the id
                // syntax; that also keeps `..` and slashes out.
                if !is_valid_session_id(&name) {
                    return Err(LocationError::InvalidQueueName(name));
                }
                let root = root.ok_or(LocationError::NoDataDirectory)?;
                QueueLocation {
                    dir: root.join("queues").join(&name),
                    root: Some(root),
                    name: Some(name),
                }
            }
        };
        Ok((location, origin))
    }

    /// The queue named `name` under `root`, as `--queue` would name it.
    /// `name` is a valid queue name: one read back from the root's
    /// listing or checked by [`QueueLocation::resolve`].
    pub fn named(root: &Path, name: &str) -> Self {
        QueueLocation {
            dir: root.join("queues").join(name),
            root: Some(root.to_path_buf()),
            name: Some(name.to_owned()),
        }
    }

    /// Whether `self` and `other` are one queue, that is one directory. On
    /// a filesystem that ignores case, as macOS's does by default, `work`
    /// and `Work` are one directory, elsewhere two; comparing the
    /// directories gets both right. Where one of them does not exist yet,
    /// the paths are compared.
    pub fn same_queue(&self, other: &QueueLocation) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let (Ok(mine), Ok(theirs)) =
                (std::fs::metadata(&self.dir), std::fs::metadata(&other.dir))
            {
                return (mine.dev(), mine.ino()) == (theirs.dev(), theirs.ino());
            }
        }
        self.dir == other.dir
    }

    /// A queue in `dir`, as `--dir` would name it.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        QueueLocation {
            dir: dir.into(),
            root: None,
            name: None,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The root a named queue lives under; `None` for a queue given as a
    /// directory.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Whether this is the queue named `default`, which asqr watches when
    /// no queue is chosen. A queue given by `--dir` never is.
    ///
    /// The name is compared exactly: on a case-sensitive filesystem
    /// `--queue DEFAULT` is a queue of its own, in a directory of its own.
    pub fn is_default(&self) -> bool {
        self.name.as_deref() == Some(DEFAULT_QUEUE)
    }

    pub fn inbox(&self) -> PathBuf {
        self.dir.join("inbox")
    }

    pub fn outbox(&self) -> PathBuf {
        self.dir.join("outbox")
    }

    pub fn drafts(&self) -> PathBuf {
        self.dir.join("drafts")
    }

    pub fn archive(&self) -> PathBuf {
        self.dir.join("archive")
    }

    /// Where the files kept for archived sessions lie, loose (spec section
    /// 5.1).
    pub fn kept_files(&self) -> PathBuf {
        self.archive().join("files")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.dir.join("lock")
    }

    /// The lock that keeps a draft save apart from a command changing the
    /// same waiting session (spec section 5.6).
    pub fn drafts_lock_file(&self) -> PathBuf {
        self.dir.join("drafts.lock")
    }

    /// Creates the four queue directories. Every command that touches a
    /// queue calls it, so a queue exists from its first use on.
    pub fn create_layout(&self) -> std::io::Result<()> {
        for dir in [self.inbox(), self.outbox(), self.drafts(), self.archive()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "asqr")
}

/// The platform data directory, the default root of named queues:
/// `~/Library/Application Support/asqr` on macOS, `$XDG_DATA_HOME/asqr` on
/// Linux.
pub fn platform_data_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.data_dir().to_path_buf())
}

/// The platform config directory, where the settings file goes (ADR 92):
/// `~/Library/Application Support/asqr` on macOS, `$XDG_CONFIG_HOME/asqr`
/// on Linux.
pub fn platform_config_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().to_path_buf())
}

/// The platform cache directory, where the log file goes (ADR 15).
pub fn platform_cache_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.cache_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn root() -> Option<PathBuf> {
        Some(PathBuf::from("/data/asqr"))
    }

    fn resolve(flags: Selection, environment: Selection) -> Result<QueueLocation, LocationError> {
        QueueLocation::resolve(flags, environment, root())
    }

    fn named(name: &str) -> Selection {
        Selection {
            queue: Some(name.into()),
            dir: None,
        }
    }

    fn dir(path: &str) -> Selection {
        Selection {
            queue: None,
            dir: Some(path.into()),
        }
    }

    #[test]
    fn defaults_to_the_default_queue_under_the_platform_root() {
        let location = resolve(Selection::default(), Selection::default()).expect("resolves");

        assert_eq!(location.dir(), Path::new("/data/asqr/queues/default"));
        assert_eq!(location.root(), Some(Path::new("/data/asqr")));
        assert_eq!(location.name(), Some("default"));
    }

    #[test]
    fn a_named_queue_lives_under_the_root() {
        let location = resolve(named("mascots"), Selection::default()).expect("resolves");

        assert_eq!(location.dir(), Path::new("/data/asqr/queues/mascots"));
        assert_eq!(location.name(), Some("mascots"));
    }

    #[test]
    fn a_directory_is_the_queue_itself_and_has_no_root() {
        let location = resolve(dir("/work/.asqr"), Selection::default()).expect("resolves");

        assert_eq!(location.dir(), Path::new("/work/.asqr"));
        assert_eq!(location.root(), None);
        assert_eq!(location.name(), None);
    }

    #[test]
    fn flags_win_over_the_environment() {
        assert_eq!(
            resolve(named("flag"), dir("/from/env"))
                .expect("resolves")
                .dir(),
            Path::new("/data/asqr/queues/flag")
        );
        assert_eq!(
            resolve(dir("/from/flag"), named("env"))
                .expect("resolves")
                .dir(),
            Path::new("/from/flag")
        );
    }

    #[test]
    fn the_environment_wins_over_the_default() {
        assert_eq!(
            resolve(Selection::default(), named("env"))
                .expect("resolves")
                .dir(),
            Path::new("/data/asqr/queues/env")
        );
    }

    fn paired(
        flags: Selection,
        environment: Selection,
        paired: Paired,
    ) -> Result<(String, Origin), LocationError> {
        QueueLocation::resolve_paired(flags, environment, paired, root())
            .map(|(location, origin)| (location.dir().display().to_string(), origin))
    }

    fn pinned(name: &str) -> Paired {
        Paired::Pinned(name.into())
    }

    #[test]
    fn a_pinned_queue_comes_after_the_environment() {
        assert_eq!(
            paired(Selection::default(), named("env"), pinned("pin")),
            Ok(("/data/asqr/queues/env".into(), Origin::Environment))
        );
        assert_eq!(
            paired(named("flag"), Selection::default(), pinned("pin")),
            Ok(("/data/asqr/queues/flag".into(), Origin::Flag))
        );
        assert_eq!(
            paired(Selection::default(), Selection::default(), pinned("pin")),
            Ok(("/data/asqr/queues/pin".into(), Origin::Pin))
        );
    }

    #[test]
    fn the_session_name_comes_after_the_pin() {
        assert_eq!(
            paired(
                Selection::default(),
                Selection::default(),
                Paired::Session("my-work".into())
            ),
            Ok(("/data/asqr/queues/my-work".into(), Origin::SessionName))
        );
    }

    #[test]
    fn the_default_comes_last_and_after_an_unreachable_tmux() {
        assert_eq!(
            paired(Selection::default(), Selection::default(), Paired::NotAsked),
            Ok(("/data/asqr/queues/default".into(), Origin::Default))
        );
        assert_eq!(
            paired(
                Selection::default(),
                Selection::default(),
                Paired::Unreachable("tmux is not installed".into())
            ),
            Ok(("/data/asqr/queues/default".into(), Origin::Unreachable))
        );
    }

    #[test]
    fn an_invalid_pinned_name_is_a_usage_error() {
        assert_eq!(
            paired(
                Selection::default(),
                Selection::default(),
                pinned("my work")
            ),
            Err(LocationError::InvalidPinnedQueue("my work".into()))
        );
        assert_eq!(
            LocationError::InvalidPinnedQueue("my work".into()).to_string(),
            "invalid queue name \"my work\" in the tmux session variable ASQR_SESSION_QUEUE (letters, digits, '-', '_' and '.', not starting with '.')"
        );
    }

    #[test]
    fn queue_and_dir_together_are_a_usage_error() {
        let both = Selection {
            queue: Some("q".into()),
            dir: Some("/d".into()),
        };

        assert_eq!(
            resolve(both.clone(), Selection::default()),
            Err(LocationError::QueueAndDir)
        );
        assert_eq!(
            resolve(Selection::default(), both),
            Err(LocationError::QueueAndDir)
        );
    }

    #[test]
    fn queue_names_follow_the_id_syntax() {
        assert_eq!(
            resolve(named("../escape"), Selection::default()),
            Err(LocationError::InvalidQueueName("../escape".into()))
        );
    }

    #[test]
    fn a_named_queue_needs_a_platform_root() {
        assert_eq!(
            QueueLocation::resolve(Selection::default(), Selection::default(), None),
            Err(LocationError::NoDataDirectory)
        );
        // A directory needs no root.
        assert!(QueueLocation::resolve(dir("/d"), Selection::default(), None).is_ok());
    }

    #[test]
    fn names_the_queue_directories() {
        let location = resolve(dir("/q"), Selection::default()).expect("resolves");

        assert_eq!(location.inbox(), Path::new("/q/inbox"));
        assert_eq!(location.outbox(), Path::new("/q/outbox"));
        assert_eq!(location.drafts(), Path::new("/q/drafts"));
        assert_eq!(location.archive(), Path::new("/q/archive"));
        assert_eq!(location.lock_file(), Path::new("/q/lock"));
    }

    #[test]
    fn a_queue_can_be_named_by_its_directory_directly() {
        assert_eq!(
            QueueLocation::at("/q"),
            resolve(dir("/q"), Selection::default()).expect("resolves")
        );
    }

    #[test]
    fn creates_the_layout_on_first_use_and_again_without_harm() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));

        location.create_layout().expect("layout is created");
        location
            .create_layout()
            .expect("an existing layout is fine");

        for dir in [
            location.inbox(),
            location.outbox(),
            location.drafts(),
            location.archive(),
        ] {
            assert!(dir.is_dir(), "{}", dir.display());
        }
    }

    #[test]
    fn the_platform_directories_are_named_asqr() {
        let data = platform_data_dir().expect("the test machine has a home directory");
        let cache = platform_cache_dir().expect("the test machine has a home directory");

        assert_eq!(
            data.file_name().and_then(|name| name.to_str()),
            Some("asqr")
        );
        assert_eq!(
            cache.file_name().and_then(|name| name.to_str()),
            Some("asqr")
        );
        assert_ne!(data, cache);
    }

    #[test]
    fn the_config_directory_is_named_asqr_and_differs_from_the_cache() {
        let config = platform_config_dir().expect("the test machine has a home directory");
        let cache = platform_cache_dir().expect("the test machine has a home directory");

        assert_eq!(
            config.file_name().and_then(|name| name.to_str()),
            Some("asqr")
        );
        assert_ne!(config, cache);
    }

    #[test]
    fn the_same_directory_is_the_same_queue() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let work = QueueLocation::named(scratch.path(), "work");
        work.create_layout().expect("layout");
        let other = QueueLocation::named(scratch.path(), "other");
        other.create_layout().expect("layout");

        assert!(work.same_queue(&QueueLocation::named(scratch.path(), "work")));
        assert!(work.same_queue(&QueueLocation::at(work.dir())));
        assert!(!work.same_queue(&other));
    }

    #[test]
    fn names_that_differ_in_case_are_one_queue_where_the_filesystem_ignores_case() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let work = QueueLocation::named(scratch.path(), "work");
        work.create_layout().expect("layout");
        let upper = QueueLocation::named(scratch.path(), "Work");
        // macOS's filesystem ignores case by default, most Linux ones do
        // not; the test asks the one it runs on.
        let ignores_case = upper.dir().exists();

        assert_eq!(work.same_queue(&upper), ignores_case);
    }

    #[test]
    fn queues_not_made_yet_compare_by_their_paths() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let work = QueueLocation::named(scratch.path(), "work");

        assert!(work.same_queue(&QueueLocation::named(scratch.path(), "work")));
        assert!(!work.same_queue(&QueueLocation::named(scratch.path(), "other")));
    }

    #[test]
    fn only_the_queue_named_default_is_the_default() {
        let is_default = |flags| {
            resolve(flags, Selection::default())
                .expect("resolves")
                .is_default()
        };

        assert!(is_default(Selection::default()), "no queue chosen");
        assert!(is_default(named("default")), "chosen by its name");
        assert!(!is_default(named("mascots")));
        assert!(!is_default(named("DEFAULT")), "a queue of its own");
        assert!(
            !is_default(dir("/data/asqr/queues/default")),
            "a directory is never the default"
        );
    }
}
