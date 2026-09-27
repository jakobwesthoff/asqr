// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Watching the inbox (spec section 3.4). The watcher only says that
//! something changed; [`Inbox::sync`](super::Inbox::sync) then works out
//! what, so events carry no paths.

use std::time::Duration;

use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::queue::QueueLocation;

/// Watches the inbox for as long as it lives.
pub struct InboxWatcher {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

/// Starts watching the inbox of `location` and calls `changed` after each
/// burst of changes. Register it before the first scan (spec section 3.4).
pub fn watch_inbox(
    location: &QueueLocation,
    mut changed: impl FnMut() + Send + 'static,
) -> notify_debouncer_full::notify::Result<InboxWatcher> {
    // An `ask` writes a temp file and renames it, several events for one
    // drop; the debouncer turns them into a single sync.
    let mut debouncer = new_debouncer(DEBOUNCE, None, move |events: DebounceEventResult| {
        // A watcher error may mean missed events; a sync catches up on
        // whatever happened, so it is logged and treated as a change.
        if let Err(errors) = events {
            tracing::warn!(?errors, "the inbox watcher reported errors");
        }
        changed();
    })?;
    debouncer.watch(location.inbox(), RecursiveMode::NonRecursive)?;
    Ok(InboxWatcher {
        _debouncer: debouncer,
    })
}

/// How long the watcher waits for a burst of changes to end.
const DEBOUNCE: Duration = Duration::from_millis(150);

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn a_file_dropped_into_the_inbox_is_reported() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("queue"));
        location.create_layout().expect("layout");
        let (sender, receiver) = mpsc::channel();

        let _watcher = watch_inbox(&location, move || {
            let _ = sender.send(());
        })
        .expect("watching");
        std::fs::write(location.inbox().join("batch.json"), "{}").expect("written");

        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the change is reported");
    }

    #[test]
    fn a_missing_inbox_cannot_be_watched() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let location = QueueLocation::at(scratch.path().join("never-created"));

        assert!(watch_inbox(&location, || {}).is_err());
    }
}
