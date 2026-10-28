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
use std::collections::VecDeque;
use std::io;
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

use patine::QuoteStyle;
use tracing::Dispatch;

use super::viewer::{ContentSize, Room, Viewer, ViewerKey};
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
    pub(super) size: Room,
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
    pub(super) size: Room,
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
        let Room {
            width,
            height,
            alone,
        } = self.size;
        let content = if alone {
            viewer.lay_out_alone(width, height)
        } else {
            viewer.lay_out(width, height)
        };
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

/// Runs `job` on a thread of its own and hands its result back, counted
/// in `running` while it runs there. Should the system refuse a thread,
/// the job runs on the calling thread, which waits for it, and is not
/// counted.
pub(super) fn start(job: Job, tools: &Tools, running: &Arc<AtomicUsize>) {
    // `spawn` drops its closure when it fails, so the job waits outside
    // it, where it can be taken back; the count goes with the closure.
    let slot = Arc::new(Mutex::new(Some(job)));
    let theirs = Arc::clone(&slot);
    let their_tools = tools.clone();
    let counted = Running::new(running);
    let spawned = thread::Builder::new()
        .name("asqr-load".to_owned())
        .stack_size(STACK)
        .spawn(move || {
            if let Some(job) = take(&theirs) {
                finish(job, &their_tools, Some(counted));
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start a thread to load a context entry");
        if let Some(job) = take(&slot) {
            finish(job, tools, None);
        }
    }
}

/// One job counted as running on a thread of its own, until it drops.
pub(super) struct Running(Arc<AtomicUsize>);

impl Running {
    fn new(running: &Arc<AtomicUsize>) -> Self {
        running.fetch_add(1, Ordering::SeqCst);
        Running(Arc::clone(running))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How many jobs may run on threads of their own at once: one fewer
/// than the cores, so the thread that loads ahead and the drawing thread
/// keep one, and at least one. Without a count of the cores, one.
pub(super) fn own_thread_cap() -> usize {
    cap_for(thread::available_parallelism().ok().map(usize::from))
}

fn cap_for(cores: Option<usize>) -> usize {
    cores.map_or(1, |cores| cores.saturating_sub(1).max(1))
}

fn take(slot: &Mutex<Option<Job>>) -> Option<Job> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Runs `job` and hands its result on, a panic included, then wakes the
/// run loop. The job's count of `running`, where it has one, ends before
/// the result goes, so the draw the wake starts sees its thread free.
pub(super) fn finish(job: Job, tools: &Tools, running: Option<Running>) {
    let dispatch = job.dispatch.clone();
    LOADING.set(true);
    let finished = tracing::dispatcher::with_default(&dispatch, || {
        match catch_unwind(AssertUnwindSafe(|| job.run(tools))) {
            Ok(loaded) => Finished::Loaded(Box::new(loaded)),
            Err(payload) => Finished::Panicked(panic_message(payload.as_ref())),
        }
    });
    LOADING.set(false);
    drop(running);
    // The viewers went away with the view as asqr quits; nobody waits for
    // the result then.
    let _ = tools.results.send(finished);
    (tools.wake)();
}

// ---------------------------------------------------------------------
// Loading ahead
// ---------------------------------------------------------------------

/// The jobs that load entries ahead of the cursor, run one after another
/// on one thread in the order they were queued (ADR 46). A job still in
/// the queue has not started: the entry shown takes its job out to run
/// it on a thread of its own, and a new window replaces what did not
/// start.
pub(super) struct Ahead {
    queue: Shared,
    /// Whether the thread runs; it starts with the first job.
    started: bool,
    /// Whether a refused thread was logged since the last one started.
    warned: bool,
    /// Starts the thread; tests refuse it there.
    spawn: fn(Shared) -> io::Result<()>,
}

/// The queue as the thread that loads ahead and the viewers share it.
type Shared = Arc<(Mutex<Queue>, Condvar)>;

/// Starts the thread that runs the jobs of `queue`.
fn spawn_worker(queue: Shared) -> io::Result<()> {
    thread::Builder::new()
        .name("asqr-ahead".to_owned())
        .stack_size(STACK)
        .spawn(move || work(&queue))
        .map(drop)
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(Job, Tools)>,
    /// The viewers went; the thread ends after the job it runs.
    closed: bool,
}

impl Ahead {
    pub(super) fn new() -> Self {
        Ahead {
            queue: Arc::default(),
            started: false,
            warned: false,
            spawn: spawn_worker,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Queue> {
        lock_queue(&self.queue.0)
    }

    /// The keys of the jobs not started yet, in their order.
    #[cfg(test)]
    pub(super) fn queued(&self) -> Vec<ViewerKey> {
        self.lock()
            .jobs
            .iter()
            .map(|(job, _)| job.key.clone())
            .collect()
    }

    /// This queue, whose thread the system refuses.
    #[cfg(test)]
    pub(super) fn refusing_threads(&mut self) {
        self.spawn = |_| Err(io::Error::other("no thread for the test"));
    }

    /// The key, ticket and size of each job not started yet, in their
    /// order.
    #[cfg(test)]
    pub(super) fn queued_jobs(&self) -> Vec<(ViewerKey, u64, Room)> {
        self.lock()
            .jobs
            .iter()
            .map(|(job, _)| (job.key.clone(), job.ticket, job.size))
            .collect()
    }

    /// Hands `rearrange` the jobs not started yet, to reorder, change or
    /// replace. The queue stays locked meanwhile, so no job starts
    /// between what `rearrange` sees and the queue it leaves.
    pub(super) fn requeue(&mut self, rearrange: impl FnOnce(&mut VecDeque<(Job, Tools)>)) {
        let waiting = {
            let mut queue = self.lock();
            rearrange(&mut queue.jobs);
            !queue.jobs.is_empty()
        };
        // Also a queue that did not change asks for its thread again,
        // where the system refused one before.
        if waiting {
            self.queue.1.notify_all();
            self.ensure_worker();
        }
    }

    /// Starts the thread unless it runs. Without it the jobs wait in the
    /// queue, where the entry shown still takes its own out to run it,
    /// and the next call asks again. Every frame with a pane asks, so a
    /// refusal is logged once until a thread starts.
    fn ensure_worker(&mut self) {
        if self.started {
            return;
        }
        match (self.spawn)(Arc::clone(&self.queue)) {
            Ok(()) => {
                self.started = true;
                self.warned = false;
            }
            Err(error) => {
                if !self.warned {
                    tracing::warn!(
                        %error,
                        "could not start the thread that loads ahead; the next requeue tries again"
                    );
                    self.warned = true;
                }
            }
        }
    }

    /// Puts `job` first in the queue, in place of a queued job of its key,
    /// so it runs next on the thread that loads ahead. Where that thread
    /// cannot start, the job comes back, to run elsewhere rather than
    /// wait for ever.
    pub(super) fn push_first(&mut self, job: Job, tools: Tools) -> Option<Job> {
        {
            let mut queue = self.lock();
            queue.jobs.retain(|(queued, _)| queued.key != job.key);
            queue.jobs.push_front((job, tools));
        }
        self.queue.1.notify_all();
        self.ensure_worker();
        if self.started {
            return None;
        }
        let mut queue = self.lock();
        let (job, _) = queue
            .jobs
            .pop_front()
            .expect("no thread runs to take the job just put first");
        Some(job)
    }

    /// Moves the queued job of `key` to the front, if it is queued.
    pub(super) fn to_front(&self, key: &ViewerKey) {
        let mut queue = self.lock();
        if let Some(at) = queue.jobs.iter().position(|(job, _)| job.key == *key)
            && at > 0
        {
            let job = queue.jobs.remove(at).expect("the position is in the queue");
            queue.jobs.push_front(job);
        }
    }

    /// Takes the job of `key` out, if it has not started.
    pub(super) fn take(&self, key: &ViewerKey) -> Option<Job> {
        let mut queue = self.lock();
        let position = queue.jobs.iter().position(|(job, _)| job.key == *key)?;
        queue.jobs.remove(position).map(|(job, _)| job)
    }

    /// Drops the jobs not started yet that `keep` refuses, and says which
    /// they were, by key and ticket.
    pub(super) fn retain(&self, keep: impl Fn(&Job) -> bool) -> Vec<(ViewerKey, u64)> {
        let mut dropped = Vec::new();
        self.lock().jobs.retain(|(job, _)| {
            let kept = keep(job);
            if !kept {
                dropped.push((job.key.clone(), job.ticket));
            }
            kept
        });
        dropped
    }
}

impl Drop for Ahead {
    fn drop(&mut self) {
        let mut queue = self.lock();
        queue.closed = true;
        queue.jobs.clear();
        drop(queue);
        self.queue.1.notify_all();
    }
}

fn lock_queue(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    queue.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs the queued jobs one after another until the queue closes.
fn work(queue: &(Mutex<Queue>, Condvar)) {
    loop {
        let next = {
            let mut waiting = lock_queue(&queue.0);
            loop {
                if waiting.closed {
                    return;
                }
                if let Some(next) = waiting.jobs.pop_front() {
                    break next;
                }
                waiting = queue
                    .1
                    .wait(waiting)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let (job, tools) = next;
        finish(job, &tools, None);
    }
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
            size: Room::pane(40, 10),
            viewer: None,
            shown: false,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        }
    }

    /// Without asqr's hook, as in tests, the panic's own message goes on.
    #[test]
    fn a_job_that_panics_hands_its_message_on() {
        let (tools, finished) = panicking();

        finish(job(), &tools, None);

        let Ok(Finished::Panicked(message)) = finished.try_recv() else {
            panic!("the panic is handed on");
        };
        assert_eq!(message, "boom");
        assert!(!LOADING.get(), "the thread runs no job any more");
    }

    /// A spawn that fails, as at the system's limit of threads.
    fn refuse(_: Shared) -> io::Result<()> {
        Err(io::Error::other("no thread for the test"))
    }

    /// Tools whose builds make a loading note, and where results go.
    fn noting() -> (Tools, mpsc::Receiver<Finished>) {
        let (results, finished) = mpsc::channel();
        let tools = Tools {
            build: Arc::new(|_, _, _, _| Box::new(super::super::viewer::LoadingNote)),
            images: Images::half_blocks(),
            home: None,
            results,
            wake: Arc::new(|| {}),
        };
        (tools, finished)
    }

    /// A thread the system refused is asked for again by every requeue
    /// until one starts, and only the first refusal of a run is logged.
    #[test]
    fn a_refused_thread_is_tried_again_by_the_next_requeue() {
        let (tools, finished) = noting();
        let mut ahead = Ahead::new();
        ahead.spawn = refuse;

        ahead.requeue(|queue| queue.push_back((job(), tools.clone())));
        assert!(!ahead.started);
        assert!(ahead.warned);
        let short = std::time::Duration::from_millis(100);
        assert!(finished.recv_timeout(short).is_err(), "no thread ran it");

        ahead.requeue(|queue| queue.push_back((job(), tools.clone())));
        assert!(!ahead.started, "refused again");
        assert!(ahead.warned);

        // A requeue that leaves the queue as it is asks again too.
        ahead.spawn = spawn_worker;
        ahead.requeue(|_| {});
        assert!(ahead.started);
        assert!(!ahead.warned, "the next refusal warns again");
        assert!(
            finished
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok(),
            "the thread runs the job"
        );
    }

    #[test]
    fn the_cap_leaves_a_core_and_runs_at_least_one() {
        let cases = [
            (None, 1),
            (Some(1), 1),
            (Some(2), 1),
            (Some(3), 2),
            (Some(8), 7),
        ];
        for (cores, cap) in cases {
            assert_eq!(cap_for(cores), cap, "{cores:?}");
        }
        assert!(own_thread_cap() >= 1);
    }

    /// A job put first replaces a queued job of its key, so the queue
    /// keeps one job per key.
    #[test]
    fn a_job_put_first_replaces_the_queued_one_of_its_key() {
        let (tools, _finished) = noting();
        let mut ahead = Ahead::new();
        let other = || Job {
            key: ViewerKey {
                session: "s".to_owned(),
                entry: EntryKey {
                    question: 1,
                    option: None,
                },
            },
            ..job()
        };
        // Counted as started without a thread, so the jobs stay queued.
        ahead.started = true;
        ahead.requeue(|queue| {
            queue.push_back((other(), tools.clone()));
            queue.push_back((job(), tools.clone()));
        });

        let mut first = job();
        first.ticket = 2;
        assert!(ahead.push_first(first, tools).is_none());

        let queued: Vec<(usize, u64)> = ahead
            .queued_jobs()
            .into_iter()
            .map(|(key, ticket, _)| (key.entry.question, ticket))
            .collect();
        assert_eq!(queued, [(0, 2), (1, 1)]);

        // The entry shown, queued behind another, moves to the front.
        let shown = other().key;
        ahead.to_front(&shown);
        let keys: Vec<usize> = ahead
            .queued()
            .into_iter()
            .map(|key| key.entry.question)
            .collect();
        assert_eq!(keys, [1, 0]);
    }

    /// The viewers going close the queue, and its thread ends after its
    /// job.
    #[test]
    fn closing_the_queue_ends_its_thread() {
        let (results, finished) = mpsc::channel();
        let tools = Tools {
            build: Arc::new(|_, _, _, _| Box::new(super::super::viewer::LoadingNote)),
            images: Images::half_blocks(),
            home: None,
            results,
            wake: Arc::new(|| {}),
        };
        let mut ahead = Ahead::new();
        ahead.requeue(|queue| queue.push_back((job(), tools)));
        assert!(
            finished
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok(),
            "the job ran"
        );
        let queue = Arc::downgrade(&ahead.queue);

        drop(ahead);

        let start = std::time::Instant::now();
        while queue.upgrade().is_some() {
            assert!(
                start.elapsed() < std::time::Duration::from_secs(5),
                "the thread never ended"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn a_panic_names_its_message_as_it_gave_it() {
        assert_eq!(panic_message(&String::from("as given")), "as given");
        assert_eq!(panic_message(&3_u8), "a panic without a message");
    }
}
