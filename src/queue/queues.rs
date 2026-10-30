// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every named queue under a root and what it holds: for `asqr queues`,
//! and for what asqr says about the queues besides the one in use.

use std::io;
use std::path::Path;

use super::names::entries_if_present;
use super::{DEFAULT_QUEUE, Holder, InboxName, QueueLocation, classify_inbox_name, lock_holder};
use crate::format::is_valid_session_id;

/// A named queue and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSummary {
    pub location: QueueLocation,
    /// The sessions waiting in its inbox.
    pub waiting: usize,
    /// The results in its outbox.
    pub answered: usize,
    /// The asqr watching it, if one does.
    pub holder: Option<Holder>,
}

impl QueueSummary {
    pub fn name(&self) -> &str {
        self.location
            .name()
            .expect("a summary is made of a named queue")
    }
}

/// Every named queue under `root`, sorted by name. The default queue is
/// always among them, also before its first use, since every command
/// without a choice uses it. A directory whose name is no queue name is
/// left out. A queue whose files cannot be read counts as empty and
/// unwatched: one queue nobody can read must not hide the others.
pub fn list_queues(root: &Path) -> io::Result<Vec<QueueSummary>> {
    let mut names: Vec<String> = entries_if_present(&root.join("queues"))?
        .into_iter()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_valid_session_id(name))
        .collect();
    if !names.iter().any(|name| name == DEFAULT_QUEUE) {
        names.push(DEFAULT_QUEUE.to_owned());
    }
    names.sort();
    Ok(names
        .into_iter()
        .map(|name| summarize(QueueLocation::named(root, &name)))
        .collect())
}

fn summarize(location: QueueLocation) -> QueueSummary {
    let sessions = |dir: &Path| {
        entries_if_present(dir)
            .unwrap_or_default()
            .iter()
            .filter(|entry| {
                matches!(
                    entry.file_name().to_str().map(classify_inbox_name),
                    Some(InboxName::Session(_))
                )
            })
            .count()
    };
    QueueSummary {
        waiting: sessions(&location.inbox()),
        answered: sessions(&location.outbox()),
        holder: lock_holder(&location).ok().flatten(),
        location,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::queue::lock_queue;

    fn root() -> (tempfile::TempDir, PathBuf) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path().to_path_buf();
        (scratch, root)
    }

    fn queue(root: &Path, name: &str) -> QueueLocation {
        let location = QueueLocation::named(root, name);
        location.create_layout().expect("layout");
        location
    }

    fn counts(summaries: &[QueueSummary]) -> Vec<(&str, usize, usize, bool)> {
        summaries
            .iter()
            .map(|summary| {
                (
                    summary.name(),
                    summary.waiting,
                    summary.answered,
                    summary.holder.is_some(),
                )
            })
            .collect()
    }

    #[test]
    fn lists_every_queue_sorted_with_what_it_holds() {
        let (_scratch, root) = root();
        let work = queue(&root, "work");
        std::fs::write(work.inbox().join("a.json"), "{}").expect("written");
        std::fs::write(work.inbox().join("b.json"), "{}").expect("written");
        std::fs::write(work.inbox().join("c.json.tmp"), "{}").expect("written");
        std::fs::write(work.outbox().join("old.json"), "{}").expect("written");
        let asqr = queue(&root, "asqr-todos");
        let _watching = lock_queue(&asqr, None).expect("locked");

        let summaries = list_queues(&root).expect("lists");

        assert_eq!(
            counts(&summaries),
            [
                ("asqr-todos", 0, 0, true),
                ("default", 0, 0, false),
                ("work", 2, 1, false)
            ]
        );
    }

    #[test]
    fn the_default_queue_is_listed_before_its_first_use() {
        let (_scratch, root) = root();

        let summaries = list_queues(&root).expect("lists");

        assert_eq!(counts(&summaries), [("default", 0, 0, false)]);
        assert!(!root.join("queues").exists(), "listing creates nothing");
    }

    #[test]
    fn a_directory_with_no_queue_name_is_left_out() {
        let (_scratch, root) = root();
        std::fs::create_dir_all(root.join("queues/.hidden")).expect("dir");
        std::fs::create_dir_all(root.join("queues/my work")).expect("dir");
        std::fs::write(root.join("queues/stray-file"), "").expect("file");

        let summaries = list_queues(&root).expect("lists");

        assert_eq!(counts(&summaries), [("default", 0, 0, false)]);
    }

    #[test]
    fn a_queue_that_cannot_be_read_counts_as_empty() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, root) = root();
        let work = queue(&root, "work");
        std::fs::write(work.inbox().join("a.json"), "{}").expect("written");
        std::fs::set_permissions(work.inbox(), std::fs::Permissions::from_mode(0o000))
            .expect("permissions change");

        let summaries = list_queues(&root);
        std::fs::set_permissions(work.inbox(), std::fs::Permissions::from_mode(0o755))
            .expect("permissions back");

        assert_eq!(
            counts(&summaries.expect("lists")),
            [("default", 0, 0, false), ("work", 0, 0, false)]
        );
    }

    #[test]
    fn a_root_whose_queues_cannot_be_read_is_an_error() {
        use std::os::unix::fs::PermissionsExt;

        let (_scratch, root) = root();
        std::fs::create_dir_all(root.join("queues")).expect("dir");
        std::fs::set_permissions(root.join("queues"), std::fs::Permissions::from_mode(0o000))
            .expect("permissions change");

        let listed = list_queues(&root);
        std::fs::set_permissions(root.join("queues"), std::fs::Permissions::from_mode(0o755))
            .expect("permissions back");

        assert!(listed.is_err());
    }
}
