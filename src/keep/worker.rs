// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The thread that keeps files after a session is finished (ADR 100), so
//! that encoding images never delays a result or freezes the screen. One
//! worker serves the whole process, whichever queue it shows: a job carries
//! its queue and needs only that queue's drafts lock.

use std::cell::Cell;
use std::ffi::OsStr;
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use super::apply::{KeepJob, Kept, keep};
use super::snapshot::MemoryBudget;

/// How often a drain that can be stopped asks whether it should.
pub const STOP_POLL: Duration = Duration::from_millis(50);

/// A handle to the thread; clones share it.
#[derive(Debug, Clone)]
pub struct KeepWorker {
    sender: Sender<Message>,
    /// The jobs handed over that have not finished.
    pending: Arc<AtomicUsize>,
    /// The file content the snapshots of the queued jobs may hold together.
    budget: MemoryBudget,
}

#[derive(Debug)]
enum Message {
    Job(KeepJob),
    Idle(Sender<()>),
}

impl KeepWorker {
    /// A worker that keeps the files of every job it is given. The run of
    /// the TUI makes one and hands clones to whatever finishes sessions.
    pub fn new() -> Self {
        Self::new_with(run_job)
    }

    /// A worker that runs `runner` on each job, one after another. A panic
    /// of the runner is logged and the worker goes on with the next job.
    pub fn new_with(runner: impl Fn(KeepJob) + Send + 'static) -> Self {
        Self::new_with_budget(MemoryBudget::new(MemoryBudget::RUN), runner)
    }

    /// [`KeepWorker::new_with`] whose snapshots draw on `budget`.
    pub fn new_with_budget(
        budget: MemoryBudget,
        runner: impl Fn(KeepJob) + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<Message>();
        let pending = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&pending);
        thread::Builder::new()
            .name("asqr-keep".to_owned())
            .spawn(move || {
                for message in receiver {
                    match message {
                        Message::Job(job) => {
                            IN_JOB.set(true);
                            let outcome = catch_unwind(AssertUnwindSafe(|| runner(job)));
                            IN_JOB.set(false);
                            counted.fetch_sub(1, Ordering::SeqCst);
                            if outcome.is_err() {
                                tracing::warn!("a job that keeps files panicked");
                            }
                        }
                        Message::Idle(reply) => {
                            let _ = reply.send(());
                        }
                    }
                }
            })
            .expect("the system can start a thread");
        KeepWorker {
            sender,
            pending,
            budget,
        }
    }

    /// The budget every snapshot taken for this worker draws on. A snapshot
    /// that does not fit what the queued ones left leaves its files to the
    /// worker to read, so submitting a job never waits.
    pub fn budget(&self) -> &MemoryBudget {
        &self.budget
    }

    /// Hands `job` over and returns at once. A worker that is gone drops
    /// the job: the plain archived session stays, as after a crash.
    pub fn submit(&self, job: KeepJob) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        if self.sender.send(Message::Job(job)).is_err() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Waits until every job handed over before the call has run.
    pub fn idle(&self) {
        let (reply, answered) = mpsc::channel();
        if self.sender.send(Message::Idle(reply)).is_ok() {
            let _ = answered.recv();
        }
    }

    /// Waits for the queue to run empty for at most `limit` and says whether
    /// it did. What is left then is let go of, as a crash would.
    pub fn drain(&self, limit: Duration) -> bool {
        self.drain_unless(limit, || false)
    }

    /// How many jobs were handed over and have not finished.
    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }

    /// [`KeepWorker::drain`] that also stops, saying `false`, as soon as
    /// `stop` says so. It is asked every [`STOP_POLL`], so a person who
    /// presses ctrl-c does not wait for the limit.
    pub fn drain_unless(&self, limit: Duration, mut stop: impl FnMut() -> bool) -> bool {
        let (reply, answered) = mpsc::channel();
        if self.sender.send(Message::Idle(reply)).is_err() {
            return false;
        }
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match answered.recv_timeout(left.min(STOP_POLL)) {
                Ok(()) => return true,
                Err(mpsc::RecvTimeoutError::Disconnected) => return false,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if left <= STOP_POLL || stop() {
                        return false;
                    }
                }
            }
        }
    }
}

/// The line quitting prints on stderr while `pending` finished sessions are
/// still being kept; none where nothing is.
pub fn quit_notice(pending: usize) -> Option<String> {
    match pending {
        0 => None,
        1 => Some("keeping the files of 1 finished session, ctrl-c lets go of it...".to_owned()),
        many => Some(format!(
            "keeping the files of {many} finished sessions, ctrl-c lets go of them..."
        )),
    }
}

thread_local! {
    /// Whether this thread is inside a job.
    static IN_JOB: Cell<bool> = const { Cell::new(false) };
}

/// A debug build holds each job until the file this variable names exists,
/// so a test can quit while a job is running without a timing bound.
pub const GATE_VARIABLE: &str = "ASQR_TEST_KEEP_GATE";

/// The longest a held job waits for its file.
const GATE_PATIENCE: Duration = Duration::from_secs(60);

/// The file a job waits for given the value of [`GATE_VARIABLE`]: only in a
/// debug build and only for a value that is not empty.
fn gate_from(variable: Option<&OsStr>, debug_build: bool) -> Option<PathBuf> {
    if !debug_build {
        return None;
    }
    variable
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Waits until `gate` exists, at most `patience`, and says whether it does.
fn wait_for_gate(gate: &Path, patience: Duration) -> bool {
    wait_for_gate_with(gate, patience, &|| thread::sleep(Duration::from_millis(10)))
}

/// [`wait_for_gate`] with what happens between two looks at the file
/// given, so a test can step through the waiting.
fn wait_for_gate_with(gate: &Path, patience: Duration, between: &dyn Fn()) -> bool {
    let deadline = Instant::now() + patience;
    while !gate.exists() {
        if Instant::now() >= deadline {
            return false;
        }
        between();
    }
    true
}

/// Keeps the files of `job`, logging how it went.
fn run_job(job: KeepJob) {
    if let Some(gate) = gate_from(
        std::env::var_os(GATE_VARIABLE).as_deref(),
        cfg!(debug_assertions),
    ) {
        wait_for_gate(&gate, GATE_PATIENCE);
    }
    let id = job.archived.path.display().to_string();
    match keep(job, &|| {}) {
        Ok(Kept::Recorded) => tracing::info!(session = %id, "kept the files of a session"),
        Ok(Kept::NothingToRecord) => {}
        Ok(Kept::Skipped(reason)) => {
            tracing::warn!(session = %id, reason, "left a session without kept files");
        }
        Err(error) => {
            tracing::warn!(session = %id, %error, "could not keep the files of a session");
        }
    }
}

impl Default for KeepWorker {
    fn default() -> Self {
        Self::new()
    }
}

/// Says whether the panic in `info` is one of a job, for the binary's panic
/// hook: a job's panic is logged and the worker goes on, so the hook must
/// not give the terminal back.
pub fn job_panicked(info: &PanicHookInfo) -> bool {
    if !IN_JOB.get() {
        return false;
    }
    tracing::warn!(%info, "a job that keeps files panicked");
    true
}

#[cfg(test)]
mod tests;
