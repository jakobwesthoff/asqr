// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The thread that keeps files after a session is finished (ADR 100), so
//! that encoding images never delays a result or freezes the screen. One
//! worker serves the whole process, whichever queue it shows: a job carries
//! its queue and needs only that queue's drafts lock.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use super::apply::{KeepJob, Kept, keep};

/// A handle to the thread; clones share it.
#[derive(Debug, Clone)]
pub struct KeepWorker {
    sender: Sender<Message>,
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
        let (sender, receiver) = mpsc::channel::<Message>();
        thread::Builder::new()
            .name("asqr-keep".to_owned())
            .spawn(move || {
                for message in receiver {
                    match message {
                        Message::Job(job) => {
                            IN_JOB.set(true);
                            let outcome = catch_unwind(AssertUnwindSafe(|| runner(job)));
                            IN_JOB.set(false);
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
        KeepWorker { sender }
    }

    /// Hands `job` over and returns at once. A worker that is gone drops
    /// the job: the plain archived session stays, as after a crash.
    pub fn submit(&self, job: KeepJob) {
        let _ = self.sender.send(Message::Job(job));
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
        let (reply, answered) = mpsc::channel();
        self.sender.send(Message::Idle(reply)).is_ok() && answered.recv_timeout(limit).is_ok()
    }
}

thread_local! {
    /// Whether this thread is inside a job.
    static IN_JOB: Cell<bool> = const { Cell::new(false) };
}

/// Keeps the files of `job`, logging how it went.
fn run_job(job: KeepJob) {
    let id = job.archived.path.display().to_string();
    match keep(&job, &|| {}) {
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
