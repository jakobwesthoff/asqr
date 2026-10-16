// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loading context entries apart from drawing the screen (ADR 46). A job
//! builds the viewer of one entry and lays it out for the body's size, or
//! lays out again a viewer it was handed, on a thread of its own; reading
//! a large file, highlighting it or scaling an image takes seconds, and
//! keys work meanwhile. The result comes back on a channel, and a wake-up
//! tells the run loop to draw again.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use patine::QuoteStyle;
use tracing::Dispatch;

use super::viewer::{ContentSize, Viewer, ViewerKey};
use crate::format::ContextEntry;
use crate::graphics::Images;

/// Builds the viewer for an entry, given what makes its images, how
/// Markdown quotes look and the home folder, which paths show as `~`.
pub(super) type Build =
    Arc<dyn Fn(&ContextEntry, &Images, QuoteStyle, Option<&Path>) -> Box<dyn Viewer> + Send + Sync>;

/// Tells the run loop that a load finished, so it draws again.
pub(super) type Wake = Arc<dyn Fn() + Send + Sync>;

/// The stack of a loading thread: as large as the main thread's on macOS
/// and Linux, as the parsers that run here need what they would need
/// there. An overflow aborts asqr with the terminal still in raw mode.
const STACK: usize = 8 * 1024 * 1024;

/// What a job is to make: the viewer of `entry`, which its session shows
/// as `key`, laid out for a body of `size`.
pub(super) struct Job {
    pub(super) key: ViewerKey,
    /// Which request of the key this job answers: only the result of the
    /// latest one is taken.
    pub(super) ticket: u64,
    pub(super) entry: ContextEntry,
    pub(super) quote: QuoteStyle,
    pub(super) size: (u16, u16),
    /// A viewer built before, to be laid out again for another size, or
    /// `None` to build one.
    pub(super) viewer: Option<Box<dyn Viewer>>,
    /// Whether a frame showed the viewer handed in.
    pub(super) shown: bool,
    /// Where the job logs: the subscriber of the thread that asked for it,
    /// so a test that watches the log sees what the job writes.
    pub(super) dispatch: Dispatch,
}

/// What a job made.
pub(super) struct Loaded {
    pub(super) key: ViewerKey,
    pub(super) ticket: u64,
    pub(super) entry: ContextEntry,
    pub(super) quote: QuoteStyle,
    pub(super) size: (u16, u16),
    pub(super) viewer: Box<dyn Viewer>,
    pub(super) content: ContentSize,
    pub(super) shown: bool,
}

/// How a job ended.
pub(super) enum Finished {
    Loaded(Box<Loaded>),
    /// The job panicked with this message.
    Panicked(String),
}

/// What every job needs besides itself.
#[derive(Clone)]
pub(super) struct Tools {
    pub(super) build: Build,
    pub(super) images: Images,
    pub(super) home: Option<PathBuf>,
    pub(super) results: Sender<Finished>,
    pub(super) wake: Wake,
}

impl Job {
    /// Builds the viewer unless it has one, and lays it out. Every kind of
    /// entry goes this one way.
    fn run(self, tools: &Tools) -> Loaded {
        let mut viewer = self.viewer.unwrap_or_else(|| {
            (tools.build)(
                &self.entry,
                &tools.images,
                self.quote,
                tools.home.as_deref(),
            )
        });
        let content = viewer.lay_out(self.size.0, self.size.1);
        Loaded {
            key: self.key,
            ticket: self.ticket,
            entry: self.entry,
            quote: self.quote,
            size: self.size,
            viewer,
            content,
            shown: self.shown,
        }
    }
}

/// Runs `job` on a thread of its own and hands its result back. Should
/// the system refuse a thread, the job runs on the calling thread, which
/// waits for it.
pub(super) fn start(job: Job, tools: &Tools) {
    // `spawn` drops its closure when it fails, so the job waits outside
    // it, where it can be taken back.
    let slot = Arc::new(Mutex::new(Some(job)));
    let theirs = Arc::clone(&slot);
    let their_tools = tools.clone();
    let spawned = thread::Builder::new()
        .name("asqr-load".to_owned())
        .stack_size(STACK)
        .spawn(move || {
            if let Some(job) = take(&theirs) {
                finish(job, &their_tools);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start a thread to load a context entry");
        if let Some(job) = take(&slot) {
            finish(job, tools);
        }
    }
}

fn take(slot: &Mutex<Option<Job>>) -> Option<Job> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Runs `job` and hands its result on, a panic included, then wakes the
/// run loop.
pub(super) fn finish(job: Job, tools: &Tools) {
    let dispatch = job.dispatch.clone();
    LOADING.set(true);
    let finished = tracing::dispatcher::with_default(&dispatch, || {
        match catch_unwind(AssertUnwindSafe(|| job.run(tools))) {
            Ok(loaded) => Finished::Loaded(Box::new(loaded)),
            Err(payload) => Finished::Panicked(panic_message(payload.as_ref())),
        }
    });
    LOADING.set(false);
    // The viewers went away with the view as asqr quits; nobody waits for
    // the result then.
    let _ = tools.results.send(finished);
    (tools.wake)();
}

// ---------------------------------------------------------------------
// Panics
// ---------------------------------------------------------------------

thread_local! {
    /// Whether this thread runs a job.
    static LOADING: Cell<bool> = const { Cell::new(false) };
    /// What the panic hook noted of a job's panic on this thread.
    static NOTED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Notes a panic of a job, for the panic hook of the binary, and says
/// whether it was one. The hook gives the terminal back, which must not
/// happen while the drawing thread goes on drawing; for a job's panic the
/// hook only lets this note it, and the drawing thread panics with the
/// message once the result reaches it, so the hook runs there.
pub fn loader_panicked(info: &PanicHookInfo) -> bool {
    if !LOADING.get() {
        return false;
    }
    NOTED.set(Some(info.to_string()));
    true
}

/// The message of a job's panic: as the hook noted it, with its place,
/// or else as the panic gave it.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(noted) = NOTED.take() {
        return noted;
    }
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::tui::EntryKey;

    /// Tools whose builds panic with "boom", and where their results go.
    fn panicking() -> (Tools, mpsc::Receiver<Finished>) {
        let (results, finished) = mpsc::channel();
        let tools = Tools {
            build: Arc::new(|_, _, _, _| panic!("boom")),
            images: Images::half_blocks(),
            home: None,
            results,
            wake: Arc::new(|| {}),
        };
        (tools, finished)
    }

    fn job() -> Job {
        Job {
            key: ViewerKey {
                session: "s".to_owned(),
                entry: EntryKey {
                    question: 0,
                    option: None,
                },
            },
            ticket: 1,
            entry: serde_json::from_str(r#"{"type": "code", "content": "x"}"#)
                .expect("test entry parses"),
            quote: QuoteStyle::DimBar,
            size: (40, 10),
            viewer: None,
            shown: false,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        }
    }

    /// Without asqr's hook, as in tests, the panic's own message goes on.
    #[test]
    fn a_job_that_panics_hands_its_message_on() {
        let (tools, finished) = panicking();

        finish(job(), &tools);

        let Ok(Finished::Panicked(message)) = finished.try_recv() else {
            panic!("the panic is handed on");
        };
        assert_eq!(message, "boom");
        assert!(!LOADING.get(), "the thread runs no job any more");
    }

    #[test]
    fn a_panic_names_its_message_as_it_gave_it() {
        assert_eq!(panic_message(&String::from("as given")), "as given");
        assert_eq!(panic_message(&3_u8), "a panic without a message");
    }
}
