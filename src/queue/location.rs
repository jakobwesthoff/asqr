// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Which queue a command works on (spec section 3.2).

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
    fn is_empty(&self) -> bool {
        self.queue.is_none() && self.dir.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LocationError {
    #[error("--queue and --dir (or ASQR_QUEUE and ASQR_DIR) cannot be used together")]
    QueueAndDir,

    #[error("invalid queue name {0:?} (letters, digits, '-', '_' and '.', not starting with '.')")]
    InvalidQueueName(String),

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
        let chosen = if flags.is_empty() { environment } else { flags };
        match chosen {
            Selection {
                queue: Some(_),
                dir: Some(_),
            } => Err(LocationError::QueueAndDir),
            Selection { dir: Some(dir), .. } => Ok(QueueLocation {
                dir,
                root: None,
                name: None,
            }),
            Selection { queue, .. } => {
                let name = queue.unwrap_or_else(|| DEFAULT_QUEUE.to_owned());
                // The name becomes a directory name, so it follows the id
                // syntax; that also keeps `..` and slashes out.
                if !is_valid_session_id(&name) {
                    return Err(LocationError::InvalidQueueName(name));
                }
                let root = root.ok_or(LocationError::NoDataDirectory)?;
                Ok(QueueLocation {
                    dir: root.join("queues").join(&name),
                    root: Some(root),
                    name: Some(name),
                })
            }
        }
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

    pub fn lock_file(&self) -> PathBuf {
        self.dir.join("lock")
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
}
