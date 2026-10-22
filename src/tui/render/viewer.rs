// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Viewers show the content of one context entry in the body of the
//! context pane (spec section 8), and [`Viewers`] keeps one per entry
//! shown, so work that depends only on the entry, or on the entry and the
//! body's size, is done once. Viewers are built and laid out apart from
//! drawing (ADR 46); the pane shows a dim `loading…` until its viewer is
//! ready.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use super::ansi::ansi_content;
use super::code::code_content;
use super::diff::diff_content;
use super::loading::{self, Ahead, Finished, Job, Loaded, Tools, Wake};
use super::markdown_viewer::markdown_content;
use super::patine_adapter::colours_on;
use super::problem::Card;
use super::source_file::{Unreadable, read_image};
use super::theme;
use super::wrap::wrap;
use crate::format::{ContextEntry, ImageEntry};
use crate::graphics::{Graphics, Image, Images, KittyBook, PlacedImage};
use crate::tui::{EntryKey, Offset};
use patine::QuoteStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect, Size};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

/// The size of a viewer's content in cells: the number of lines and the
/// width of the widest line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContentSize {
    pub lines: usize,
    pub columns: usize,
}

/// Shows the content of one context entry inside the context pane.
///
/// A viewer owns its content, which does not change for the viewer's life,
/// and anything derived from it. The pane owns everything around it: the
/// header, the scroll position and the keys.
///
/// Building a viewer prepares everything that does not depend on the
/// body's size: reading files, decoding images, highlighting, computing a
/// diff. A failure is not returned; the viewer keeps it and shows it as
/// its content. The pane then calls `lay_out` before the first `draw` and
/// again whenever the body's size changes, and calls `draw` on every frame
/// with an area of the size last passed to `lay_out`. Building and laying
/// out run on a thread of their own, drawing on the drawing thread, so a
/// viewer moves between them.
pub trait Viewer: Send {
    /// Lays the content out for a body of `width`×`height` cells and returns
    /// its size: the number of lines and the width of the widest line. The
    /// pane scrolls vertically when `lines` exceeds `height` and sideways
    /// when `columns` exceeds `width`. Content that wraps therefore reports
    /// at most `width` columns, and content scaled to the body reports the
    /// body's size, so it never scrolls.
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize;

    /// Draws the content as the last `lay_out` arranged it into `area` of
    /// `buf`, starting at line `offset.top` and column `offset.left`. The
    /// offset never exceeds the size `lay_out` returned.
    ///
    /// Returns the Kitty image it drew, for the screen to mend once
    /// everything drawn over it is drawn too (ADR 48).
    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage>;

    /// Whether the viewer shows a problem in place of its content, such as
    /// a file that cannot be read, which the pane's header marks.
    fn failed(&self) -> bool {
        false
    }

    /// Whether the viewer stands in for an entry still loading, which
    /// shows nothing of the entry and so says nothing of how far it
    /// scrolls.
    fn loading(&self) -> bool {
        false
    }
}

/// Which entry of which session a viewer belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ViewerKey {
    pub(super) session: String,
    pub(super) entry: EntryKey,
}

/// A viewer with the entry and the quote style it was built from, and the
/// body size it was last laid out for with the size its content took
/// there.
struct Kept {
    entry: ContextEntry,
    quote: QuoteStyle,
    viewer: Box<dyn Viewer>,
    laid_out: ((u16, u16), ContentSize),
    /// Whether a frame showed the viewer.
    shown: bool,
}

/// A job asked for and not yet taken back: which one, and what for.
struct Request {
    ticket: u64,
    entry: ContextEntry,
    quote: QuoteStyle,
    /// The job lays out a viewer a frame showed, rather than building one.
    lays_out: bool,
}

/// How long a draw waits for the entry it shows before it shows the
/// placeholder instead, so an entry that loads quickly never flashes it.
const LOAD_WAIT: Duration = Duration::from_millis(100);

/// How long a draw waits in tests: long enough that every test that does
/// not ask for the placeholder draws what loaded, short enough that a
/// load that never ends fails the test.
#[cfg(test)]
const TEST_WAIT: Duration = Duration::from_secs(60);

/// One viewer per entry of the window of the question shown: the
/// previous, the current and the next question (ADR 46). A viewer is
/// built when its entry is first shown or loaded ahead, kept while the
/// entry stays the same and inside the window, and laid out again only
/// when the body's size changes, as its content does not.
pub struct Viewers {
    tools: Tools,
    finished: Receiver<Finished>,
    viewers: HashMap<ViewerKey, Kept>,
    /// The keys with a job in flight or queued, at most one each. A job
    /// in flight stays noted when its key leaves the window, so a frame
    /// that wants the entry again takes it over.
    loading: HashMap<ViewerKey, Request>,
    next_ticket: u64,
    /// The keys of the last window, `None` before the first: until then
    /// nothing is outside it.
    window: Option<Vec<ViewerKey>>,
    /// The key the pane last asked for.
    shown: Option<ViewerKey>,
    /// The job the pane waits for, and until when.
    waiting: Option<(ViewerKey, u64, Instant)>,
    wait: Duration,
    placeholder: LoadingNote,
    ahead: Ahead,
    /// How many jobs run on threads of their own.
    running: Arc<AtomicUsize>,
    /// How many jobs may run on threads of their own at once.
    cap: usize,
    /// Whether a key, a paste, a resize or a quit waits to be handled
    /// after this draw, which the next draw replaces at once.
    input_pending: bool,
    /// Whether a draw with input pending shows the placeholder at once
    /// rather than wait for the entry.
    skips_wait_for_input: bool,
    /// Whether the entries of the window load before they are shown.
    loads_ahead: bool,
    /// How many build jobs were started, so tests see that a viewer is
    /// reused.
    #[cfg(test)]
    built: usize,
    /// How long each wait for a job was to take, so tests see when a draw
    /// waits.
    #[cfg(test)]
    waits: Vec<Duration>,
    /// How many results were taken in, so tests see that one arrived.
    #[cfg(test)]
    collected: usize,
}

impl Viewers {
    /// Viewers whose images `images` makes.
    pub fn new(images: Images) -> Self {
        let (results, finished) = mpsc::channel();
        Viewers {
            tools: Tools {
                build: Arc::new(viewer_for),
                images,
                home: None,
                results,
                wake: Arc::new(|| {}),
            },
            finished,
            viewers: HashMap::new(),
            loading: HashMap::new(),
            next_ticket: 0,
            window: None,
            shown: None,
            waiting: None,
            wait: LOAD_WAIT,
            placeholder: LoadingNote,
            ahead: Ahead::new(),
            running: Arc::default(),
            cap: loading::own_thread_cap(),
            input_pending: false,
            skips_wait_for_input: true,
            loads_ahead: true,
            #[cfg(test)]
            built: 0,
            #[cfg(test)]
            waits: Vec::new(),
            #[cfg(test)]
            collected: 0,
        }
    }

    /// Viewers as tests need them to see the same on every machine and at
    /// every speed. A draw waits for its entry up to [`TEST_WAIT`] and also
    /// while input waits, as tests feed every event before the run loop
    /// starts and look at frames that show what loaded. Any number of jobs
    /// run on threads of their own, as machines differ in their cores.
    /// Tests that need the terminal's way ask for it by name.
    #[cfg(test)]
    pub(super) fn for_tests(images: Images) -> Self {
        Viewers {
            wait: TEST_WAIT,
            skips_wait_for_input: false,
            cap: usize::MAX,
            ..Viewers::new(images)
        }
    }

    /// Takes the home folder, for the viewers built from now on.
    pub fn set_home(&mut self, home: Option<PathBuf>) {
        self.tools.home = home;
    }

    /// Takes whether input waits to be handled after the next draw.
    pub fn set_input_pending(&mut self, pending: bool) {
        self.input_pending = pending;
    }

    /// Takes what wakes the run loop once a load finished.
    pub fn wake_with(&mut self, wake: Wake) {
        self.tools.wake = wake;
    }

    /// The viewer of `entry`, which session `session` shows as `key`, with
    /// Markdown quotes in `quote`, laid out for a body of `width`×`height`
    /// cells, and the size its content takes there. A viewer is built when
    /// its key shows an entry for the first time, another entry than
    /// before, as a replaced session file can, or the entry with quotes in
    /// another style.
    ///
    /// Building and laying out run on a thread of their own. The call
    /// waits for that job up to [`LOAD_WAIT`], once per job; a job that
    /// takes longer leaves the placeholder in the viewer's place until a
    /// later draw finds it done.
    pub fn laid_out(
        &mut self,
        session: &str,
        key: EntryKey,
        entry: &ContextEntry,
        width: u16,
        height: u16,
        quote: QuoteStyle,
    ) -> (&mut dyn Viewer, ContentSize) {
        let key = ViewerKey {
            session: session.to_owned(),
            entry: key,
        };
        let size = (width, height);
        self.shown = Some(key.clone());
        self.collect();
        if !self.ready(&key, entry, quote, size) {
            self.request(&key, entry, quote, size);
            self.wait_for(&key, entry, quote, size);
        }
        if self.ready(&key, entry, quote, size) {
            self.waiting = None;
            let kept = self.viewers.get_mut(&key).expect("a ready viewer is kept");
            kept.shown = true;
            return (kept.viewer.as_mut(), kept.laid_out.1);
        }
        let content = self.placeholder.lay_out(width, height);
        (&mut self.placeholder, content)
    }

    /// Whether the viewer kept for `key` shows `entry` with quotes in
    /// `quote`, laid out for `size`.
    fn ready(
        &self,
        key: &ViewerKey,
        entry: &ContextEntry,
        quote: QuoteStyle,
        size: (u16, u16),
    ) -> bool {
        self.viewers.get(key).is_some_and(|kept| {
            kept.entry == *entry && kept.quote == quote && kept.laid_out.0 == size
        })
    }

    /// Starts the job that makes the viewer of `key` ready, unless the
    /// right one is in flight. A viewer kept for another entry or quote
    /// goes, as no frame shows it again, and a new one is built; a job in
    /// flight for that other entry is outrun, and its result is not taken.
    /// A viewer kept for another size is laid out again; while that job
    /// holds it, the pane waits for it. A job queued to load ahead leaves
    /// the queue and starts at once at the size the pane asks for, as the
    /// entry shown goes first, or goes first in the queue while the cap's
    /// jobs run on threads of their own. This runs on every draw while
    /// the entry is not ready, so a job freed of the cap starts then.
    fn request(
        &mut self,
        key: &ViewerKey,
        entry: &ContextEntry,
        quote: QuoteStyle,
        size: (u16, u16),
    ) {
        let current = self
            .viewers
            .get(key)
            .is_some_and(|kept| kept.entry == *entry && kept.quote == quote);
        let in_flight = self
            .loading
            .get(key)
            .is_some_and(|request| request.entry == *entry && request.quote == quote);
        if in_flight {
            if !self.below_cap() {
                self.ahead.to_front(key);
            } else if let Some(mut queued) = self.ahead.take(key) {
                queued.size = size;
                #[cfg(test)]
                {
                    self.built += 1;
                }
                loading::start(queued, &self.tools, &self.running);
            }
            return;
        }
        if current {
            let kept = self.viewers.remove(key).expect("a current viewer is kept");
            self.start(key, entry, quote, size, Some(kept));
        } else {
            self.viewers.remove(key);
            self.start(key, entry, quote, size, None);
        }
    }

    /// Starts a job for `key` under a new ticket: one that lays `kept` out
    /// again, or without it one that builds the viewer. A job still queued
    /// for `key` was asked for another entry or quote, as `request` takes
    /// the right one out of the queue, and goes: a queued job is always
    /// the one its key's note names.
    fn start(
        &mut self,
        key: &ViewerKey,
        entry: &ContextEntry,
        quote: QuoteStyle,
        size: (u16, u16),
        kept: Option<Kept>,
    ) {
        #[cfg(test)]
        if kept.is_none() {
            self.built += 1;
        }
        drop(self.ahead.take(key));
        let job = self.job(key, entry, quote, size, kept);
        self.run_first(job);
    }

    /// Whether a job may start on a thread of its own.
    fn below_cap(&self) -> bool {
        self.running.load(Ordering::SeqCst) < self.cap
    }

    /// Runs `job` on a thread of its own, or past the cap first on the
    /// thread that loads ahead, so moving fast over many slow entries
    /// does not start a thread for each. Where that thread cannot start,
    /// the job runs on one of its own all the same.
    fn run_first(&mut self, job: Job) {
        if self.below_cap() {
            loading::start(job, &self.tools, &self.running);
            return;
        }
        if let Some(job) = self.ahead.push_first(job, self.tools.clone()) {
            loading::start(job, &self.tools, &self.running);
        }
    }

    /// The job for `key` under a new ticket, noted as in flight.
    fn job(
        &mut self,
        key: &ViewerKey,
        entry: &ContextEntry,
        quote: QuoteStyle,
        size: (u16, u16),
        kept: Option<Kept>,
    ) -> Job {
        noted_job(
            &mut self.loading,
            &mut self.next_ticket,
            key,
            entry,
            quote,
            size,
            kept,
        )
    }

    /// Keeps the viewers of `window`, the entries the question shown in
    /// session `session` keeps loaded, in the order they load (ADR 46),
    /// and lets every other viewer go, also one a frame showed and those
    /// of other sessions. A job in flight for an entry outside the window
    /// runs to its end and stays noted, so coming back to the entry takes
    /// it over; what it made goes when it arrives. A job queued and not
    /// started goes. With the body's `size`, which a pane shown as a
    /// one-line hint has none of, the entries of the window that are not
    /// loaded yet are queued to load ahead in that order, with quotes in
    /// `quote`.
    ///
    /// A viewer loaded for another size is left as it is: it is laid out
    /// again when it is shown.
    pub fn load_window(
        &mut self,
        session: &str,
        window: &[(EntryKey, &ContextEntry)],
        size: Option<(u16, u16)>,
        quote: QuoteStyle,
    ) {
        let keys: Vec<ViewerKey> = window
            .iter()
            .map(|(entry, _)| ViewerKey {
                session: session.to_owned(),
                entry: *entry,
            })
            .collect();
        self.viewers.retain(|key, _| keys.contains(key));
        let dropped = self.ahead.retain(|job| keys.contains(&job.key));
        forget_dropped(&mut self.loading, &dropped);
        self.window = Some(keys.clone());
        let Some(size) = size.filter(|_| self.loads_ahead) else {
            return;
        };

        // The queue stays locked while it is rebuilt, so its thread cannot
        // take a job between the look at what waits and the new queue: a
        // job it took is in flight and is not queued again.
        let Viewers {
            ahead,
            viewers,
            loading,
            next_ticket,
            tools,
            ..
        } = self;
        ahead.requeue(|queue| {
            let mut waiting: Vec<(Job, Tools)> = queue.drain(..).collect();
            for (key, (_, entry)) in keys.iter().zip(window) {
                let entry = *entry;
                let current = viewers
                    .get(key)
                    .is_some_and(|kept| kept.entry == *entry && kept.quote == quote);
                if current {
                    continue;
                }
                let note = loading.get(key);
                let noted = |job: &Job| note.is_some_and(|request| request.ticket == job.ticket);
                // A queued job of the entry keeps its ticket and its note,
                // takes the window's place and is laid out for its size.
                if let Some(at) = waiting
                    .iter()
                    .position(|(job, _)| job.key == *key && noted(job))
                {
                    let (mut job, job_tools) = waiting.remove(at);
                    if job.entry == *entry && job.quote == quote {
                        job.size = size;
                        queue.push_back((job, job_tools));
                        continue;
                    }
                } else if note
                    .is_some_and(|request| request.entry == *entry && request.quote == quote)
                {
                    // Noted and not queued: it runs already.
                    continue;
                }
                viewers.remove(key);
                let job = noted_job(loading, next_ticket, key, entry, quote, size, None);
                queue.push_back((job, tools.clone()));
            }
            // Every queued job left is of an entry replaced in the window,
            // and a new job holds its key's note. The jobs outside the
            // window went with `retain` above.
            drop(waiting);
        });
    }

    /// Waits for the job in flight for `key` until it makes the viewer
    /// ready or the wait for it ends. The wait belongs to the job: a later
    /// draw of the same job does not wait again, a new job does. A result
    /// that is not what the pane needs, such as one laid out for a size
    /// the body no longer has, starts the next job within the same wait.
    ///
    /// With input waiting behind the draw, there is no wait: the next
    /// draw replaces this one at once. The job keeps its wait for the
    /// first draw without input.
    fn wait_for(
        &mut self,
        key: &ViewerKey,
        entry: &ContextEntry,
        quote: QuoteStyle,
        size: (u16, u16),
    ) {
        if self.input_pending && self.skips_wait_for_input {
            return;
        }
        let ticket = self
            .loading
            .get(key)
            .map(|request| request.ticket)
            .expect("`request` leaves a job in flight for a key not ready");
        let deadline = match &self.waiting {
            Some((waited, waited_for, deadline)) if waited == key && *waited_for == ticket => {
                *deadline
            }
            _ => {
                #[cfg(test)]
                self.waits.push(self.wait);
                Instant::now() + self.wait
            }
        };
        loop {
            if let Some(request) = self.loading.get(key) {
                self.waiting = Some((key.clone(), request.ticket, deadline));
            }
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            let Ok(finished) = self.finished.recv_timeout(deadline - now) else {
                return;
            };
            self.take(finished);
            if self.ready(key, entry, quote, size) {
                return;
            }
            self.request(key, entry, quote, size);
        }
    }

    /// Takes in every result that arrived. A panic of a job becomes a
    /// panic here, on the drawing thread, so asqr ends as on a panic while
    /// drawing; the screen calls this on every frame, whatever it shows.
    pub(super) fn collect(&mut self) {
        while let Ok(finished) = self.finished.try_recv() {
            self.take(finished);
        }
    }

    /// Keeps what a job made, if it answers the latest request of its key
    /// and its entry is inside the window or shown. Any other result
    /// belongs to a session that left, an entry replaced since or left
    /// behind, or an image forgotten, and goes. The entry shown counts:
    /// the pane asks for the entry of a new question before the window of
    /// that question is sent.
    fn take(&mut self, finished: Finished) {
        #[cfg(test)]
        {
            self.collected += 1;
        }
        let loaded = match finished {
            Finished::Loaded(loaded) => loaded,
            Finished::Panicked(message) => {
                panic!("loading a context entry panicked: {message}")
            }
        };
        let Loaded {
            key,
            ticket,
            entry,
            quote,
            size,
            viewer,
            content,
            shown,
        } = *loaded;
        if self
            .loading
            .get(&key)
            .is_none_or(|request| request.ticket != ticket)
        {
            return;
        }
        self.loading.remove(&key);
        let in_window = self
            .window
            .as_ref()
            .is_none_or(|window| window.contains(&key));
        if !in_window && self.shown.as_ref() != Some(&key) {
            return;
        }
        self.viewers.insert(
            key,
            Kept {
                entry,
                quote,
                viewer,
                laid_out: (size, content),
                shown,
            },
        );
    }

    /// The graphics the images are made for.
    pub fn graphics(&self) -> Graphics {
        self.tools.images.graphics()
    }

    /// Drops every image viewer a frame showed, so each of those images is
    /// read and sent to the terminal again when it is next shown, behind
    /// the placeholder. The Kitty protocol sends an image's data once, on
    /// its first draw, and afterwards only refers to it; tmux with
    /// `allow-passthrough on` drops that transfer while the pane is not
    /// visible, and the image would never appear (ADR 24). A job that lays
    /// out an image a frame showed is not taken back either, as it may keep
    /// the encoding sent before. An image loaded ahead and never shown, or
    /// a job that builds one, has sent nothing. Other viewers keep what
    /// they derived, as nothing of theirs went to the terminal.
    pub fn forget_images(&mut self) {
        let image = |entry: &ContextEntry| matches!(entry, ContextEntry::Image(_));
        self.viewers
            .retain(|_, kept| !(kept.shown && image(&kept.entry)));
        self.loading
            .retain(|_, request| !(request.lays_out && image(&request.entry)));
        self.ahead
            .retain(|job| !(job.viewer.is_some() && image(&job.entry)));
    }

    /// Drops the viewers of every session `waiting` says no longer waits,
    /// and forgets the jobs in flight or queued for them.
    pub fn retain_sessions(&mut self, waiting: impl Fn(&str) -> bool) {
        self.viewers.retain(|key, _| waiting(&key.session));
        self.loading.retain(|key, _| waiting(&key.session));
        self.ahead.retain(|job| waiting(&job.key.session));
        if let Some(window) = &mut self.window {
            window.retain(|key| waiting(&key.session));
        }
        if self
            .shown
            .as_ref()
            .is_some_and(|key| !waiting(&key.session))
        {
            self.shown = None;
        }
        if self
            .waiting
            .as_ref()
            .is_some_and(|(key, ..)| !waiting(&key.session))
        {
            self.waiting = None;
        }
    }

    /// The book of the Kitty ids the images of the viewers take.
    pub fn kitty_book(&self) -> &KittyBook {
        self.tools.images.kitty_book()
    }

    /// The Kitty ids of the images whose viewers went since the last call,
    /// for the terminal to delete.
    #[cfg(test)]
    pub(super) fn dropped_images(&self) -> Vec<u32> {
        self.tools.images.dropped()
    }

    /// The Kitty ids of the images the viewers hold.
    #[cfg(test)]
    pub(super) fn held_images(&self) -> Vec<u32> {
        self.tools.images.held()
    }

    /// These viewers, waiting `wait` for a job before they show the
    /// placeholder.
    #[cfg(test)]
    pub(super) fn waiting(&mut self, wait: Duration) {
        self.wait = wait;
    }

    /// These viewers, showing the placeholder at once while input waits.
    #[cfg(test)]
    pub(super) fn skipping_waits_for_input(&mut self) {
        self.skips_wait_for_input = true;
    }

    /// How many waits for a job the draws took.
    #[cfg(test)]
    pub(super) fn wait_count(&self) -> usize {
        self.waits.len()
    }

    /// These viewers, building through `build`: a test's own viewers, or
    /// in a debug build one that panics on purpose (see [`panicking_on`]).
    #[cfg(any(test, debug_assertions))]
    pub(super) fn building(&mut self, build: loading::Build) {
        self.tools.build = build;
    }

    /// These viewers, running at most `cap` jobs on threads of their own.
    #[cfg(test)]
    pub(super) fn capping(&mut self, cap: usize) {
        self.cap = cap;
    }

    /// These viewers, whose thread to load ahead the system refuses.
    #[cfg(test)]
    pub(super) fn refusing_the_ahead_thread(&mut self) {
        self.ahead.refusing_threads();
    }

    /// The entries queued to load ahead and not started yet.
    #[cfg(test)]
    pub(super) fn queued(&self) -> Vec<EntryKey> {
        self.ahead
            .queued()
            .into_iter()
            .map(|key| key.entry)
            .collect()
    }

    /// The entry, ticket and size of each job queued to load ahead and not
    /// started yet.
    #[cfg(test)]
    pub(super) fn queued_jobs(&self) -> Vec<(EntryKey, u64, (u16, u16))> {
        self.ahead
            .queued_jobs()
            .into_iter()
            .map(|(key, ticket, size)| (key.entry, ticket, size))
            .collect()
    }

    /// The entries whose job is in flight or queued.
    #[cfg(test)]
    pub(super) fn pending(&self) -> Vec<EntryKey> {
        self.loading.keys().map(|key| key.entry).collect()
    }
}

/// Forgets the notes of the queued jobs `dropped` names by key and ticket.
/// A note with another ticket belongs to a later job, which runs.
fn forget_dropped(loading: &mut HashMap<ViewerKey, Request>, dropped: &[(ViewerKey, u64)]) {
    for (key, ticket) in dropped {
        if loading
            .get(key)
            .is_some_and(|request| request.ticket == *ticket)
        {
            loading.remove(key);
        }
    }
}

/// The job for `key` under the next ticket of `next_ticket`, noted in
/// `loading` as in flight: one that lays `kept` out again, or without it
/// one that builds the viewer of `entry`.
fn noted_job(
    loading: &mut HashMap<ViewerKey, Request>,
    next_ticket: &mut u64,
    key: &ViewerKey,
    entry: &ContextEntry,
    quote: QuoteStyle,
    size: (u16, u16),
    kept: Option<Kept>,
) -> Job {
    *next_ticket += 1;
    let ticket = *next_ticket;
    loading.insert(
        key.clone(),
        Request {
            ticket,
            entry: entry.clone(),
            quote,
            lays_out: kept.is_some(),
        },
    );
    let (viewer, shown) = match kept {
        Some(kept) => (Some(kept.viewer), kept.shown),
        None => (None, false),
    };
    Job {
        key: key.clone(),
        ticket,
        entry: entry.clone(),
        quote,
        size,
        viewer,
        shown,
        dispatch: tracing::dispatcher::get_default(Clone::clone),
    }
}

fn viewer_for(
    entry: &ContextEntry,
    images: &Images,
    quote: QuoteStyle,
    home: Option<&Path>,
) -> Box<dyn Viewer> {
    match entry {
        ContextEntry::Image(image) => Box::new(image_content(image, images, home)),
        ContextEntry::Code(code) => Box::new(code_content(code, home)),
        ContextEntry::Markdown(markdown) => Box::new(markdown_content(markdown, quote, home)),
        ContextEntry::Diff(diff) => Box::new(diff_content(diff, home)),
        ContextEntry::Ansi(ansi) => Box::new(ansi_content(ansi, colours_on(quote), home)),
    }
}

/// Builds as [`viewer_for`] does, but panics on the image entry whose path
/// is `path`, so the PTY tests see what asqr gives back on a panic while
/// loading an entry. Only a debug build has it.
#[cfg(debug_assertions)]
pub(super) fn panicking_on(path: std::ffi::OsString) -> loading::Build {
    Arc::new(move |entry, images, quote, home| {
        if entry.image_path().is_some_and(|image| path == image) {
            panic!("ASQR_TEST_PANIC_ON_LOAD names {}", path.to_string_lossy());
        }
        viewer_for(entry, images, quote, home)
    })
}

// ---------------------------------------------------------------------
// What a viewer shows
// ---------------------------------------------------------------------

/// What a viewer made of its entry when it was built: the content it
/// shows, a dim note in its place where there is nothing to show, or the
/// card of a problem, such as a file that cannot be read. The viewer
/// keeps it and hands laying out and drawing on to it.
pub(super) enum Content<T> {
    Body(T),
    Note(Placeholder),
    Problem(Card),
}

impl<T: Viewer> Viewer for Content<T> {
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        match self {
            Content::Body(body) => body.lay_out(width, height),
            Content::Note(placeholder) => placeholder.lay_out(width, height),
            Content::Problem(card) => card.lay_out(width, height),
        }
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        match self {
            Content::Body(body) => body.draw(area, buf, offset),
            Content::Note(placeholder) => placeholder.draw(area, buf, offset),
            Content::Problem(card) => card.draw(area, buf, offset),
        }
    }

    fn failed(&self) -> bool {
        matches!(self, Content::Problem(_))
    }
}

// ---------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------

/// The viewer of the image file of `entry`, scaled to fill the body as far
/// as its proportions allow, or, without `enlarge`, at its natural size
/// where that fits. A file that cannot be shown shows why instead, in the
/// card every viewer of a file shows, which scrolls when it does not fit.
/// Decodes the image, which is encoded when it is laid out, as that
/// depends on the body's size. The home folder `home` shows as `~` in the
/// path of an image that cannot be shown.
fn image_content(entry: &ImageEntry, images: &Images, home: Option<&Path>) -> Content<Picture> {
    match load(entry, images) {
        Ok(image) => Content::Body(Picture {
            image,
            enlarge: entry.enlarge,
        }),
        Err(unreadable) => Content::Problem(unreadable.card(home)),
    }
}

/// The image of `entry`, read by the rule for image files and decoded.
fn load(entry: &ImageEntry, images: &Images) -> Result<Image, Unreadable> {
    let bytes = read_image(&entry.path)?;
    images
        .decode(bytes, &entry.path)
        .map_err(|error| Unreadable::image(&entry.path, &error))
}

/// A decoded image and whether it is enlarged to fill the body.
struct Picture {
    image: Image,
    enlarge: bool,
}

impl Viewer for Picture {
    /// Encodes the image for the body. It never gets larger than the
    /// body, so the body is all the room it needs, and it never scrolls.
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        self.image.lay_out(Size::new(width, height), self.enlarge);
        ContentSize {
            lines: usize::from(height),
            columns: usize::from(width),
        }
    }

    /// The offset of an image is always zero, as it never scrolls.
    fn draw(&mut self, area: Rect, buf: &mut Buffer, _offset: Offset) -> Option<PlacedImage> {
        self.image.draw(area, buf)
    }
}

// ---------------------------------------------------------------------
// Placeholders
// ---------------------------------------------------------------------

/// One dim note in place of content, such as "empty", wrapped to the body.
pub(super) struct Placeholder {
    lines: Vec<Line<'static>>,
    wrapped: Vec<Line<'static>>,
}

impl Placeholder {
    /// A dim note, such as "empty", where there is no content to show.
    pub(super) fn note(note: &'static str) -> Self {
        Placeholder {
            lines: vec![Line::styled(note, theme::secondary())],
            wrapped: Vec::new(),
        }
    }
}

impl Viewer for Placeholder {
    fn lay_out(&mut self, width: u16, _height: u16) -> ContentSize {
        self.wrapped = self
            .lines
            .iter()
            .flat_map(|line| wrap(line.clone(), usize::from(width), 0, 0))
            .collect();
        // Wrapping lets blanks hang past the edge, where they show nothing,
        // so they must not make the content look wider than the body.
        let widest = self.wrapped.iter().map(Line::width).max().unwrap_or(0);
        ContentSize {
            lines: self.wrapped.len(),
            columns: widest.min(usize::from(width)),
        }
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        let lines: Vec<Line<'static>> = self.wrapped.iter().skip(offset.top).cloned().collect();
        Paragraph::new(lines).render(area, buf);
        None
    }
}

/// What the pane shows while its entry loads.
const LOADING: &str = "loading…";

/// A dim `loading…` in the middle of the body, where the viewer goes once
/// it is ready. It takes one line, so the pane neither scrolls nor names
/// a position or `tab` in its bottom rule meanwhile.
pub(super) struct LoadingNote;

impl Viewer for LoadingNote {
    fn lay_out(&mut self, width: u16, _height: u16) -> ContentSize {
        ContentSize {
            lines: 1,
            columns: Line::from(LOADING).width().min(usize::from(width)),
        }
    }

    /// Ignores the offset: the note stays in the middle.
    fn draw(&mut self, area: Rect, buf: &mut Buffer, _offset: Offset) -> Option<PlacedImage> {
        let middle = Rect {
            y: area.y + area.height / 2,
            height: area.height.min(1),
            ..area
        };
        Paragraph::new(Line::styled(LOADING, theme::secondary()))
            .alignment(Alignment::Center)
            .render(middle, buf);
        None
    }

    fn loading(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};
    use std::thread::{self, ThreadId};

    use patine::AnsiColor;

    use super::super::patine_adapter::quote_style;
    use super::super::source_file::regular_file;
    use super::super::test_support::{self, Gate, row};
    use super::*;
    use crate::graphics::Protocol;

    /// Quotes as the screen draws them with colours.
    const COLOURED: QuoteStyle = quote_style(false);

    fn entry(json: &str) -> ContextEntry {
        serde_json::from_str(json).expect("test entry parses")
    }

    fn viewers() -> Viewers {
        Viewers::for_tests(Images::half_blocks())
    }

    const QUESTION: EntryKey = EntryKey {
        question: 0,
        option: None,
    };

    const OPTION: EntryKey = EntryKey {
        question: 0,
        option: Some(1),
    };

    fn built(viewers: &mut Viewers, session: &str, key: EntryKey, entry: &ContextEntry) {
        viewers.laid_out(session, key, entry, 40, 10, COLOURED);
    }

    /// The text the viewer for `entry` draws into `width`×`height` cells
    /// from line `top` on, one string per row without the blanks after it.
    /// The viewer draws as `Viewers` laid it out, with no lay out of the
    /// test's own, as the pane draws it.
    fn drawn(entry: &ContextEntry, width: u16, height: u16, top: usize) -> Vec<String> {
        let buf = drawn_by(&mut viewers(), entry, width, height, top);
        (0..height).map(|y| row(&buf, y)).collect()
    }

    /// The buffer the viewer `viewers` keep for `entry` draws into
    /// `width`×`height` cells from line `top` on, as the pane draws it.
    fn drawn_by(
        viewers: &mut Viewers,
        entry: &ContextEntry,
        width: u16,
        height: u16,
        top: usize,
    ) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let (viewer, _) = viewers.laid_out("s", QUESTION, entry, width, height, COLOURED);
        viewer.draw(area, &mut buf, Offset { top, left: 0 });
        buf
    }

    /// The text `viewer`, laid out and drawn into `width`×`height` cells
    /// from line `top` on, shows, one string per row without the blanks
    /// after it.
    fn drawn_viewer(viewer: &mut dyn Viewer, width: u16, height: u16, top: usize) -> Vec<String> {
        let buf = test_support::drawn(viewer, width, height, Offset { top, left: 0 });
        (0..height).map(|y| row(&buf, y)).collect()
    }

    // -----------------------------------------------------------------
    // The life of a viewer
    // -----------------------------------------------------------------

    /// What `Viewers` asked of the viewers a test built, and on which
    /// threads.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct Calls {
        builds: usize,
        lay_outs: Vec<(u16, u16)>,
        threads: Vec<ThreadId>,
    }

    type Shared = Arc<Mutex<Calls>>;

    fn calls(shared: &Shared) -> MutexGuard<'_, Calls> {
        shared.lock().expect("no test thread panicked")
    }

    /// A viewer that only records the calls it gets. Its lay-outs after
    /// the first wait for `gate`, where there is one.
    struct Recording {
        calls: Shared,
        gate: Option<Gate>,
    }

    impl Viewer for Recording {
        fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
            let again = !calls(&self.calls).lay_outs.is_empty();
            if again && let Some(gate) = &self.gate {
                gate.wait();
            }
            let mut calls = calls(&self.calls);
            calls.lay_outs.push((width, height));
            calls.threads.push(thread::current().id());
            ContentSize {
                lines: 3,
                columns: usize::from(width),
            }
        }

        fn draw(&mut self, _area: Rect, _buf: &mut Buffer, _offset: Offset) -> Option<PlacedImage> {
            None
        }
    }

    /// Viewers whose every viewer records into the returned calls, its
    /// building included.
    fn recording() -> (Viewers, Shared) {
        recording_behind(None)
    }

    /// Like [`recording`], with every lay-out after a viewer's first
    /// waiting for `gate`.
    fn recording_behind(gate: Option<Gate>) -> (Viewers, Shared) {
        let shared = Shared::default();
        let mut viewers = viewers();
        let theirs = Arc::clone(&shared);
        viewers.building(Arc::new(move |_, _, _, _| {
            let mut recorded = calls(&theirs);
            recorded.builds += 1;
            recorded.threads.push(thread::current().id());
            Box::new(Recording {
                calls: Arc::clone(&theirs),
                gate: gate.clone(),
            })
        }));
        (viewers, shared)
    }

    /// Every kind of entry builds through `viewer_for` into its viewer,
    /// which shows the entry or, where it cannot, the card.
    #[test]
    fn every_kind_of_entry_draws_through_viewer_for() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let png = directory.path().join("dot.png");
        image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]))
            .save(&png)
            .expect("image written");
        let cases = [
            (
                serde_json::json!({"type": "code", "content": "fn main() {}"}),
                "1 fn main() {}",
                false,
            ),
            (
                serde_json::json!({"type": "markdown", "content": "Some *notes*"}),
                "Some notes",
                false,
            ),
            (
                serde_json::json!({"type": "diff", "old": {"content": "a\n"}, "new": {"content": "b\n"}}),
                "1   - a",
                false,
            ),
            (serde_json::json!({"type": "image", "path": png}), "", false),
            (
                serde_json::json!({"type": "code", "path": "/nowhere/a.rs"}),
                "╭",
                true,
            ),
            (
                serde_json::json!({"type": "markdown", "path": "/nowhere/a.md"}),
                "╭",
                true,
            ),
            (
                serde_json::json!({"type": "diff", "old": {"path": "/nowhere/a"}, "new": {"content": "b"}}),
                "╭",
                true,
            ),
            (
                serde_json::json!({"type": "image", "path": "/nowhere/a.png"}),
                "╭",
                true,
            ),
        ];
        for (json, first, failed) in cases {
            let entry = entry(&json.to_string());
            let mut viewer = viewer_for(&entry, &Images::half_blocks(), COLOURED, None);
            let buf = test_support::drawn(viewer.as_mut(), 40, 10, Offset::default());
            let shown: Vec<String> = (0..10).map(|y| row(&buf, y)).collect();

            assert!(
                shown
                    .iter()
                    .any(|line| line.trim_start().starts_with(first)),
                "{json}: {shown:#?}"
            );
            assert_eq!(viewer.failed(), failed, "{json}");
        }
    }

    #[test]
    fn an_ansi_entry_draws_through_viewer_for() {
        let inline = entry(r#"{"type": "ansi", "content": "\u001b[31mfailed\u001b[0m 3 tests"}"#);
        let mut viewer = viewer_for(&inline, &Images::half_blocks(), COLOURED, None);
        let buf = test_support::drawn(viewer.as_mut(), 40, 3, Offset::default());

        assert_eq!(row(&buf, 0), "failed 3 tests");
        assert_eq!(
            buf[(0, 0)].fg,
            ratatui::style::Color::Indexed(1),
            "red, as the terminal's palette has it"
        );
        assert!(!viewer.failed());

        let missing = entry(r#"{"type": "ansi", "path": "/nowhere/out.ansi"}"#);
        let mut viewer = viewer_for(&missing, &Images::half_blocks(), COLOURED, None);
        let buf = test_support::drawn(viewer.as_mut(), 40, 10, Offset::default());
        assert!(
            (0..10).any(|y| row(&buf, y).trim_start().starts_with('╭')),
            "the card"
        );
        assert!(viewer.failed());
    }

    /// The screen builds every viewer with the quote style of its colour
    /// setting, so an `ansi` entry built for `NO_COLOR` draws no colour at
    /// all, RGB included, which the pass after drawing would keep.
    #[test]
    fn an_ansi_entry_built_for_no_color_has_no_colour() {
        let coloured = entry(
            r#"{"type": "ansi", "content": "\u001b[1;38;2;255;0;0;48;5;4mbold\u001b[0m \u001b[32mgreen"}"#,
        );
        let mut viewer = viewer_for(&coloured, &Images::half_blocks(), quote_style(true), None);

        let buf = test_support::drawn(viewer.as_mut(), 20, 1, Offset::default());

        assert_eq!(row(&buf, 0), "bold green");
        for cell in &buf.content {
            assert_eq!(
                (cell.fg, cell.bg, cell.underline_color),
                (
                    ratatui::style::Color::Reset,
                    ratatui::style::Color::Reset,
                    ratatui::style::Color::Reset
                ),
                "at {:?}",
                cell.symbol()
            );
        }
        assert!(
            buf[(0, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
    }

    /// The PTY tests make a debug build panic while it loads the image
    /// they name; every other entry builds as always.
    #[cfg(debug_assertions)]
    #[test]
    fn the_panicking_build_panics_only_on_its_image() {
        let build = panicking_on("/pictures/doomed.png".into());
        let built = |json: &str| {
            let entry = entry(json);
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                drop(build(&entry, &Images::half_blocks(), COLOURED, None));
            }))
        };

        let panicked = built(r#"{"type": "image", "path": "/pictures/doomed.png"}"#)
            .expect_err("the named image panics");
        assert_eq!(
            panicked.downcast_ref::<String>().map(String::as_str),
            Some("ASQR_TEST_PANIC_ON_LOAD names /pictures/doomed.png")
        );
        assert!(built(r#"{"type": "image", "path": "/pictures/cat.png"}"#).is_ok());
        assert!(built(r#"{"type": "code", "content": "x"}"#).is_ok());
    }

    /// The viewers asqr runs with wait 100 ms, skip that wait while input
    /// waits, cap their threads and load ahead; only the test constructor
    /// sets them otherwise.
    #[test]
    fn new_viewers_are_set_up_for_the_terminal() {
        let viewers = Viewers::new(Images::half_blocks());

        assert_eq!(viewers.wait, LOAD_WAIT);
        assert!(viewers.skips_wait_for_input);
        assert_eq!(viewers.cap, loading::own_thread_cap());
        assert!(viewers.loads_ahead);
    }

    #[test]
    fn a_viewer_is_built_when_its_entry_first_shows_and_laid_out_only_when_the_size_changes() {
        let (mut viewers, shared) = recording();
        let text = entry(r#"{"type": "code", "content": "notes"}"#);
        assert_eq!(calls(&shared).builds, 0, "nothing shows yet");

        for _ in 0..3 {
            let (_, size) = viewers.laid_out("s", QUESTION, &text, 40, 10, COLOURED);
            assert_eq!(
                size,
                ContentSize {
                    lines: 3,
                    columns: 40
                }
            );
        }
        let (_, size) = viewers.laid_out("s", QUESTION, &text, 30, 10, COLOURED);
        assert_eq!(size.columns, 30, "a resize lays it out again");
        viewers.laid_out("s", QUESTION, &text, 30, 10, COLOURED);
        viewers.laid_out("s", QUESTION, &text, 30, 12, COLOURED);

        let recorded = calls(&shared);
        assert_eq!(recorded.builds, 1);
        assert_eq!(recorded.lay_outs, [(40, 10), (30, 10), (30, 12)]);
    }

    // -----------------------------------------------------------------
    // Loading apart from drawing
    // -----------------------------------------------------------------

    /// Counts the wake-ups of `viewers`, one per finished load.
    fn counted(viewers: &mut Viewers) -> Arc<AtomicUsize> {
        let woken = Arc::new(AtomicUsize::new(0));
        let theirs = Arc::clone(&woken);
        viewers.wake_with(Arc::new(move || {
            theirs.fetch_add(1, Ordering::SeqCst);
        }));
        woken
    }

    /// Viewers that build what `viewer_for` builds once `gate` is open
    /// for entries `held` says so of, wait `wait` for a load, and count
    /// their wake-ups.
    fn held_back(
        gate: &Gate,
        held: fn(&ContextEntry) -> bool,
        wait: Duration,
    ) -> (Viewers, Arc<AtomicUsize>) {
        let mut viewers = viewers();
        let theirs = gate.clone();
        viewers.building(Arc::new(move |entry, images, quote, home| {
            if held(entry) {
                theirs.wait();
            }
            viewer_for(entry, images, quote, home)
        }));
        viewers.waiting(wait);
        let woken = counted(&mut viewers);
        (viewers, woken)
    }

    /// Viewers whose every build waits for `gate`.
    fn gated(gate: &Gate, wait: Duration) -> (Viewers, Arc<AtomicUsize>) {
        held_back(gate, |_| true, wait)
    }

    const FETCH: &str = r#"{"type": "code", "content": "fn main() {}\n"}"#;

    /// The bug the loading fixes: a slow entry held up the draw, and with
    /// it every key, until it was built.
    #[test]
    fn the_pane_shows_loading_while_its_entry_builds() {
        let gate = Gate::default();
        let (mut viewers, woken) = gated(&gate, Duration::ZERO);
        let code = entry(FETCH);

        let buf = drawn_by(&mut viewers, &code, 30, 9, 0);

        assert_eq!(
            woken.load(Ordering::SeqCst),
            0,
            "the draw returned while the entry still builds"
        );
        let rows: Vec<String> = (0..9).map(|y| row(&buf, y)).collect();
        let mut expected = vec![String::new(); 9];
        expected[4] = format!("{}loading…", " ".repeat(11));
        assert_eq!(rows, expected, "in the middle");
        assert!(
            buf[(11, 4)]
                .modifier
                .contains(ratatui::style::Modifier::DIM)
        );

        gate.open();
        test_support::wait_until(&woken, 1);
        let buf = drawn_by(&mut viewers, &code, 30, 9, 0);
        assert_eq!(row(&buf, 0), "1 fn main() {}");
    }

    #[test]
    fn an_entry_that_loads_within_the_wait_never_shows_loading() {
        let mut viewers = viewers();
        viewers.waiting(Duration::from_secs(10));

        let buf = drawn_by(&mut viewers, &entry(FETCH), 30, 9, 0);

        assert_eq!(row(&buf, 0), "1 fn main() {}");
    }

    #[test]
    fn the_wait_is_spent_once_per_job() {
        let gate = Gate::default();
        let wait = Duration::from_millis(50);
        let (mut viewers, _) = gated(&gate, wait);
        let code = entry(FETCH);

        let start = Instant::now();
        built(&mut viewers, "s", QUESTION, &code);
        assert!(start.elapsed() >= wait, "{:?}", start.elapsed());
        assert_eq!(viewers.waits, [wait]);

        built(&mut viewers, "s", QUESTION, &code);
        assert_eq!(
            viewers.waits,
            [wait],
            "the same job is not waited for again"
        );

        built(&mut viewers, "s", OPTION, &code);
        assert_eq!(viewers.waits, [wait, wait], "another entry's job is");
        gate.open();
    }

    /// Input waiting behind a draw replaces it at once, so the draw shows
    /// the placeholder without waiting for the entry.
    #[test]
    fn a_draw_with_input_pending_shows_loading_at_once() {
        let gate = Gate::default();
        let (mut viewers, woken) = gated(&gate, Duration::from_secs(2));
        viewers.skipping_waits_for_input();
        viewers.set_input_pending(true);
        let code = entry(FETCH);

        let buf = drawn_by(&mut viewers, &code, 30, 9, 0);

        assert_eq!(row(&buf, 4).trim_start(), "loading…");
        assert!(viewers.waits.is_empty());
        gate.open();
        test_support::wait_until(&woken, 1);
        assert_eq!(
            row(&drawn_by(&mut viewers, &code, 30, 9, 0), 0),
            "1 fn main() {}"
        );
    }

    #[test]
    fn a_draw_without_input_pending_still_waits() {
        let gate = Gate::default();
        let wait = Duration::from_millis(50);
        let (mut viewers, _) = gated(&gate, wait);
        viewers.skipping_waits_for_input();
        viewers.set_input_pending(false);

        built(&mut viewers, "s", QUESTION, &entry(FETCH));

        assert_eq!(viewers.waits, [wait]);
        gate.open();
    }

    /// The deadline belongs to a job: an entry that once loaded slowly
    /// waits again for its next lay-out, so a quick one does not flash
    /// the placeholder.
    #[test]
    fn a_later_lay_out_waits_again() {
        let gate = Gate::default();
        let wait = Duration::from_millis(50);
        let (mut viewers, woken) = gated(&gate, wait);
        let code = entry(FETCH);
        built(&mut viewers, "s", QUESTION, &code);
        gate.open();
        test_support::wait_until(&woken, 1);
        assert_eq!(
            row(&drawn_by(&mut viewers, &code, 40, 10, 0), 0),
            "1 fn main() {}"
        );

        // A wait long enough for any machine shows that the lay-out is
        // waited for at all: with the first job's deadline, past by now,
        // the draw would return the placeholder at once.
        let long = Duration::from_secs(60);
        viewers.waiting(long);
        let buf = drawn_by(&mut viewers, &code, 30, 8, 0);

        assert_eq!(row(&buf, 0), "1 fn main() {}", "laid out within the wait");
        assert_eq!(viewers.waits, [wait, long]);
    }

    #[test]
    fn a_finished_load_wakes_the_loop_and_shows_without_building_again() {
        let gate = Gate::default();
        let (mut viewers, woken) = gated(&gate, Duration::ZERO);
        let code = entry(FETCH);
        built(&mut viewers, "s", QUESTION, &code);
        assert_eq!(woken.load(Ordering::SeqCst), 0);

        gate.open();
        test_support::wait_until(&woken, 1);

        assert_eq!(
            row(&drawn_by(&mut viewers, &code, 40, 10, 0), 0),
            "1 fn main() {}"
        );
        assert_eq!(viewers.built, 1);
    }

    #[test]
    fn a_load_no_longer_shown_is_kept() {
        let gate = Gate::default();
        let (mut viewers, woken) = gated(&gate, Duration::ZERO);
        let code = entry(FETCH);
        let notes = entry(r#"{"type": "markdown", "content": "notes"}"#);
        built(&mut viewers, "s", QUESTION, &code);
        built(&mut viewers, "s", OPTION, &notes);

        gate.open();
        test_support::wait_until(&woken, 2);
        let buf = drawn_by(&mut viewers, &code, 40, 10, 0);

        assert_eq!(row(&buf, 0), "1 fn main() {}");
        assert_eq!(viewers.built, 2, "one build each");
    }

    /// A job runs to its end, as patine cannot be stopped. Leaving its
    /// entry's window and coming back takes the running job over instead
    /// of starting a second one for the same entry.
    #[test]
    fn a_job_that_left_the_window_is_adopted_when_its_entry_comes_back() {
        let gate = Gate::default();
        let (mut viewers, woken) = gated(&gate, Duration::ZERO);
        let (code, other) = (entry(FETCH), entry(r#"{"type": "code", "content": "x"}"#));
        built(&mut viewers, "s", QUESTION, &code);

        viewers.load_window("s", &[(OPTION, &other)], None, COLOURED);
        assert_eq!(viewers.pending(), [QUESTION], "still noted while it runs");
        viewers.load_window("s", &[(QUESTION, &code)], None, COLOURED);
        built(&mut viewers, "s", QUESTION, &code);

        assert_eq!(viewers.built, 1);
        gate.open();
        test_support::wait_until(&woken, 1);
        assert_eq!(
            row(&drawn_by(&mut viewers, &code, 40, 10, 0), 0),
            "1 fn main() {}"
        );
    }

    /// What a job made for an entry outside the window, and not shown,
    /// goes when it arrives.
    #[test]
    fn a_result_outside_the_window_is_dropped() {
        let gate = Gate::default();
        let (mut viewers, woken) = gated(&gate, Duration::ZERO);
        let (code, other) = (entry(FETCH), entry(r#"{"type": "code", "content": "x"}"#));
        built(&mut viewers, "s", QUESTION, &code);
        viewers.load_window("s", &[(OPTION, &other)], None, COLOURED);
        built(&mut viewers, "s", OPTION, &other);

        gate.open();
        test_support::wait_until(&woken, 2);
        viewers.collect();

        let kept: Vec<EntryKey> = viewers.viewers.keys().map(|key| key.entry).collect();
        assert_eq!(kept, [OPTION]);
        assert_eq!(viewers.pending(), []);
    }

    /// A job for an entry the session file has since replaced is not taken
    /// over: the new entry gets a job of its own, and only its result is
    /// taken.
    #[test]
    fn a_replaced_entry_is_not_adopted() {
        let gate = Gate::default();
        let (mut viewers, woken) = gated(&gate, Duration::ZERO);
        let (old, new) = (entry(FETCH), entry(r#"{"type": "code", "content": "new"}"#));
        let other = entry(r#"{"type": "code", "content": "x"}"#);
        built(&mut viewers, "s", QUESTION, &old);
        viewers.load_window("s", &[(OPTION, &other)], None, COLOURED);
        viewers.load_window("s", &[(QUESTION, &new)], None, COLOURED);

        built(&mut viewers, "s", QUESTION, &new);

        assert_eq!(viewers.built, 2);
        gate.open();
        test_support::wait_until(&woken, 2);
        assert_eq!(row(&drawn_by(&mut viewers, &new, 40, 10, 0), 0), "1 new");
    }

    /// The pane asks for the entry of a question before the window of that
    /// question is sent, so the entry shown counts as inside the window.
    #[test]
    fn a_result_for_the_entry_shown_is_kept_before_its_window_arrives() {
        let gate = Gate::default();
        let (mut viewers, _) = gated(&gate, Duration::from_secs(60));
        let (code, other) = (entry(FETCH), entry(r#"{"type": "code", "content": "x"}"#));
        viewers.load_window("s", &[(QUESTION, &code)], None, COLOURED);
        let opener = gate.clone();
        let opening = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            opener.open();
        });

        let (viewer, _) = viewers.laid_out("s", OPTION, &other, 40, 10, COLOURED);
        let buf = test_support::drawn(viewer, 40, 10, Offset::default());

        opening.join().expect("the opening thread ends");
        assert_eq!(row(&buf, 0), "1 x");
        assert_eq!(viewers.built, 1);
    }

    #[test]
    fn building_and_laying_out_run_off_the_drawing_thread() {
        let (mut viewers, shared) = recording();
        let text = entry(r#"{"type": "code", "content": "notes"}"#);

        viewers.laid_out("s", QUESTION, &text, 40, 10, COLOURED);
        viewers.laid_out("s", QUESTION, &text, 30, 10, COLOURED);

        let recorded = calls(&shared);
        assert_eq!(recorded.threads.len(), 3, "a build and two lay-outs");
        assert!(
            recorded
                .threads
                .iter()
                .all(|thread| *thread != thread::current().id())
        );
    }

    #[test]
    fn every_kind_of_entry_builds_on_a_worker() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let threads = Arc::new(Mutex::new(Vec::new()));
        let mut viewers = viewers();
        let theirs = Arc::clone(&threads);
        viewers.building(Arc::new(move |entry, images, quote, home| {
            theirs
                .lock()
                .expect("no test thread panicked")
                .push(thread::current().id());
            viewer_for(entry, images, quote, home)
        }));

        for (key, kind) in [
            entry(FETCH),
            entry(r#"{"type": "markdown", "content": "*notes*"}"#),
            entry(r#"{"type": "diff", "new": {"content": "x"}}"#),
            picture(&directory, "a.png"),
        ]
        .iter()
        .enumerate()
        {
            let key = EntryKey {
                question: key,
                option: None,
            };
            viewers.laid_out("s", key, kind, 40, 10, COLOURED);
        }

        let threads = threads.lock().expect("no test thread panicked");
        assert_eq!(threads.len(), 4);
        assert!(
            threads
                .iter()
                .all(|thread| *thread != thread::current().id())
        );
    }

    /// A panic while loading reaches the drawing thread, which panics with
    /// its message, so asqr ends as it would on a panic while drawing.
    #[test]
    #[should_panic(expected = "boom")]
    fn a_panic_while_loading_ends_on_the_drawing_thread() {
        let mut viewers = viewers();
        viewers.building(Arc::new(|_, _, _, _| panic!("boom")));

        built(&mut viewers, "s", QUESTION, &entry(FETCH));
    }

    #[test]
    fn the_results_of_a_session_that_left_are_dropped() {
        let gate = Gate::default();
        let (mut viewers, woken) = gated(&gate, Duration::ZERO);
        built(&mut viewers, "gone", QUESTION, &entry(FETCH));

        viewers.retain_sessions(|session| session != "gone");
        gate.open();
        test_support::wait_until(&woken, 1);
        viewers.collect();

        assert_eq!(viewers.collected, 1, "the result arrived");
        assert!(viewers.viewers.is_empty(), "and went");
    }

    /// A session can leave and come back under its id, as its next round:
    /// only the result of the latest job for an entry is taken.
    #[test]
    fn a_session_back_under_its_id_takes_only_its_own_results() {
        let gate = Gate::default();
        let (mut viewers, woken) = held_back(
            &gate,
            |entry| matches!(entry, ContextEntry::Code(code) if code.content.as_deref() == Some("one")),
            Duration::ZERO,
        );
        let one = entry(r#"{"type": "code", "content": "one"}"#);
        let two = entry(r#"{"type": "code", "content": "two"}"#);
        built(&mut viewers, "s", QUESTION, &one);
        viewers.retain_sessions(|_| false);
        built(&mut viewers, "s", QUESTION, &two);

        gate.open();
        test_support::wait_until(&woken, 2);
        viewers.collect();

        assert_eq!(viewers.collected, 2);
        assert_eq!(viewers.viewers.len(), 1);
        assert!(viewers.loading.is_empty());
        let buf = drawn_by(&mut viewers, &two, 40, 10, 0);
        assert_eq!(row(&buf, 0), "1 two");
    }

    /// A lay-out job holds the viewer, so the pane waits for it before it
    /// lays the viewer out for the size the body has by then.
    #[test]
    fn a_resize_while_laying_out_waits_for_that_job() {
        let gate = Gate::default();
        let (mut viewers, shared) = recording_behind(Some(gate.clone()));
        viewers.waiting(Duration::ZERO);
        let woken = counted(&mut viewers);
        let text = entry(r#"{"type": "code", "content": "notes"}"#);
        viewers.laid_out("s", QUESTION, &text, 40, 10, COLOURED);
        test_support::wait_until(&woken, 1);

        viewers.laid_out("s", QUESTION, &text, 30, 10, COLOURED);
        viewers.laid_out("s", QUESTION, &text, 20, 10, COLOURED);
        gate.open();
        test_support::wait_until(&woken, 2);
        viewers.laid_out("s", QUESTION, &text, 20, 10, COLOURED);
        test_support::wait_until(&woken, 3);
        let (_, size) = viewers.laid_out("s", QUESTION, &text, 20, 10, COLOURED);

        assert_eq!(size.columns, 20);
        assert_eq!(calls(&shared).lay_outs, [(40, 10), (30, 10), (20, 10)]);
    }

    /// The result of another entry's job that arrives while the pane waits
    /// is kept, and the wait goes on for the entry shown. The order is
    /// forced: the other entry's build waits until the shown one's begins,
    /// which is after the draw took in what had arrived, and the shown one
    /// waits until the other's result is sent, which its wake-up tells.
    #[test]
    fn the_wait_takes_in_other_results_and_goes_on() {
        let other = Gate::default();
        let shown = Gate::default();
        let mut viewers = viewers();
        let (held_other, released_other, held_shown) =
            (other.clone(), other.clone(), shown.clone());
        viewers.building(Arc::new(move |entry, images, quote, home| {
            match entry {
                ContextEntry::Markdown(_) => held_other.wait(),
                _ => {
                    released_other.open();
                    held_shown.wait();
                }
            }
            viewer_for(entry, images, quote, home)
        }));
        let woken = Arc::new(AtomicUsize::new(0));
        let theirs = Arc::clone(&woken);
        viewers.wake_with(Arc::new(move || {
            // The first wake-up is the other entry's, sent with its result.
            if theirs.fetch_add(1, Ordering::SeqCst) == 0 {
                shown.open();
            }
        }));
        let notes = entry(r#"{"type": "markdown", "content": "notes"}"#);
        let code = entry(FETCH);
        viewers.waiting(Duration::ZERO);
        built(&mut viewers, "s", OPTION, &notes);

        viewers.waiting(Duration::from_secs(60));
        let collected = viewers.collected;
        let buf = drawn_by(&mut viewers, &code, 40, 10, 0);

        assert_eq!(row(&buf, 0), "1 fn main() {}");
        assert_eq!(viewers.collected, collected + 2, "both taken in the wait");
        assert_eq!(viewers.viewers.len(), 2, "the other entry's viewer is kept");
    }

    /// A viewer that lays its viewer out after the first time only once
    /// `gate` is open.
    struct HeldLayOut {
        inner: Box<dyn Viewer>,
        gate: Gate,
        laid_out: bool,
    }

    impl Viewer for HeldLayOut {
        fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
            if std::mem::replace(&mut self.laid_out, true) {
                self.gate.wait();
            }
            self.inner.lay_out(width, height)
        }

        fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
            self.inner.draw(area, buf, offset)
        }
    }

    /// An image a frame showed and a resize handed to a job may come back
    /// with the encoding sent before; on regaining the focus that job's
    /// result goes, and the image is built and sent again.
    #[test]
    fn regaining_the_focus_drops_an_image_being_laid_out_again() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let gate = Gate::default();
        let mut viewers = kitty_viewers();
        let theirs = gate.clone();
        viewers.building(Arc::new(move |entry, images, quote, home| {
            Box::new(HeldLayOut {
                inner: viewer_for(entry, images, quote, home),
                gate: theirs.clone(),
                laid_out: false,
            })
        }));
        let woken = counted(&mut viewers);
        let dot = picture(&directory, "a.png");
        built(&mut viewers, "s", QUESTION, &dot);
        assert_eq!(viewers.held_images(), [1]);
        viewers.waiting(Duration::ZERO);
        viewers.laid_out("s", QUESTION, &dot, 30, 8, COLOURED);

        viewers.forget_images();
        gate.open();
        test_support::wait_until(&woken, 2);
        viewers.collect();
        assert!(viewers.viewers.is_empty(), "the result went");

        viewers.waiting(Duration::from_secs(60));
        viewers.laid_out("s", QUESTION, &dot, 30, 8, COLOURED);
        assert_eq!(viewers.built, 2, "built again");
        assert_eq!(viewers.held_images(), [3], "sent under a new id");
    }

    // -----------------------------------------------------------------
    // The window and loading ahead
    // -----------------------------------------------------------------

    fn at(question: usize, option: Option<usize>) -> EntryKey {
        EntryKey { question, option }
    }

    /// Code entries whose text names them.
    fn code(text: &str) -> ContextEntry {
        serde_json::from_value(serde_json::json!({"type": "code", "content": text}))
            .expect("test entry parses")
    }

    /// Viewers that load ahead, and what their builds of code entries tell:
    /// the text of each one built, with the thread it was built on, how
    /// many builds began, and how many loads woke the run loop.
    struct Noting {
        viewers: Viewers,
        built: Arc<Mutex<Vec<(String, ThreadId)>>>,
        began: Arc<AtomicUsize>,
        woken: Arc<AtomicUsize>,
    }

    impl Noting {
        fn texts(&self) -> Vec<String> {
            let built = self.built.lock().expect("no test thread panicked");
            built.iter().map(|(text, _)| text.clone()).collect()
        }
    }

    /// Viewers that load ahead and note every code entry they build, each
    /// build waiting for `gate` while `held` says so of its text.
    fn noting(gate: &Gate, held: fn(&str) -> bool) -> Noting {
        let built = Arc::new(Mutex::new(Vec::new()));
        let began = Arc::new(AtomicUsize::new(0));
        let mut viewers = viewers();
        let (theirs, their_count, held_back) =
            (Arc::clone(&built), Arc::clone(&began), gate.clone());
        viewers.building(Arc::new(move |entry, images, quote, home| {
            if let ContextEntry::Code(code) = entry {
                their_count.fetch_add(1, Ordering::SeqCst);
                let text = code.content.clone().unwrap_or_default();
                if held(&text) {
                    held_back.wait();
                }
                theirs
                    .lock()
                    .expect("no test thread panicked")
                    .push((text, thread::current().id()));
            }
            viewer_for(entry, images, quote, home)
        }));
        let woken = counted(&mut viewers);
        Noting {
            viewers,
            built,
            began,
            woken,
        }
    }

    #[test]
    fn the_window_loads_ahead_one_after_another_on_one_thread() {
        let mut noting = noting(&Gate::default(), |_| false);
        let (a, b, c) = (code("a"), code("b"), code("c"));

        noting.viewers.load_window(
            "s",
            &[
                (at(0, Some(0)), &a),
                (at(0, Some(1)), &b),
                (at(1, None), &c),
            ],
            Some((40, 10)),
            COLOURED,
        );
        test_support::wait_until(&noting.woken, 3);

        assert_eq!(noting.texts(), ["a", "b", "c"]);
        let built = noting.built.lock().expect("no test thread panicked");
        assert!(
            built.iter().all(|(_, thread)| *thread == built[0].1),
            "one thread"
        );
        assert_ne!(built[0].1, thread::current().id());
        drop(built);
        noting.viewers.collect();
        assert_eq!(noting.viewers.viewers.len(), 3, "kept");
    }

    /// The entry shown goes first: its job leaves the queue and runs on a
    /// thread of its own, whatever the queue's thread is busy with.
    #[test]
    fn the_entry_shown_does_not_wait_behind_the_queue() {
        let gate = Gate::default();
        let mut noting = noting(&gate, |text| text == "a");
        let (a, b, c) = (code("a"), code("b"), code("c"));
        noting.viewers.load_window(
            "s",
            &[
                (at(0, Some(0)), &a),
                (at(0, Some(1)), &b),
                (at(0, Some(2)), &c),
            ],
            Some((40, 1)),
            COLOURED,
        );
        test_support::wait_until(&noting.began, 1);
        assert_eq!(
            noting.viewers.queued(),
            [at(0, Some(1)), at(0, Some(2))],
            "a runs and waits"
        );
        noting.viewers.waiting(Duration::from_secs(10));

        let (viewer, _) = noting
            .viewers
            .laid_out("s", at(0, Some(2)), &c, 40, 1, COLOURED);
        let buf = test_support::drawn(viewer, 40, 1, Offset::default());

        assert_eq!(row(&buf, 0), "1 c");
        assert_eq!(noting.texts(), ["c"], "while a still waits");
        assert_eq!(noting.viewers.queued(), [at(0, Some(1))]);
        gate.open();
        test_support::wait_until(&noting.woken, 3);
    }

    /// A new window replaces the jobs that did not start: one that left the
    /// window never runs, one still in the window runs once.
    #[test]
    fn a_new_window_replaces_the_jobs_not_yet_started() {
        let gate = Gate::default();
        let mut noting = noting(&gate, |text| text == "x");
        let (x, a, b, c) = (code("x"), code("a"), code("b"), code("c"));
        noting.viewers.load_window(
            "s",
            &[
                (at(0, None), &x),
                (at(0, Some(0)), &a),
                (at(0, Some(1)), &b),
            ],
            Some((40, 10)),
            COLOURED,
        );
        test_support::wait_until(&noting.began, 1);

        noting.viewers.load_window(
            "s",
            &[
                (at(0, None), &x),
                (at(0, Some(0)), &a),
                (at(0, Some(2)), &c),
            ],
            Some((40, 10)),
            COLOURED,
        );

        assert_eq!(noting.viewers.queued(), [at(0, Some(0)), at(0, Some(2))]);
        gate.open();
        test_support::wait_until(&noting.woken, 3);
        noting.viewers.collect();
        assert_eq!(noting.texts(), ["x", "a", "c"], "b never, each once");
    }

    /// A job the queue's thread took runs to its end and is not queued
    /// again by the next window.
    #[test]
    fn a_job_already_started_is_not_queued_again() {
        let gate = Gate::default();
        let mut noting = noting(&gate, |text| text == "a");
        let (a, b, c) = (code("a"), code("b"), code("c"));
        noting.viewers.load_window(
            "s",
            &[(at(0, Some(0)), &a), (at(0, Some(1)), &b)],
            Some((40, 10)),
            COLOURED,
        );
        test_support::wait_until(&noting.began, 1);

        noting.viewers.load_window(
            "s",
            &[(at(0, Some(0)), &a), (at(0, Some(2)), &c)],
            Some((40, 10)),
            COLOURED,
        );

        assert_eq!(noting.viewers.queued(), [at(0, Some(2))]);
        gate.open();
        test_support::wait_until(&noting.woken, 2);
        noting.viewers.collect();
        assert_eq!(noting.texts(), ["a", "c"], "a once, b never");
        assert_eq!(noting.viewers.viewers.len(), 2, "both kept");
    }

    /// Viewers that load ahead, whose every code entry builds a
    /// [`Recording`] noting into the calls kept for its text, each build
    /// waiting for `gate` while `held` says so of its text, and how many
    /// builds began and loads woke the run loop.
    struct PerEntry {
        viewers: Viewers,
        by_text: Arc<Mutex<HashMap<String, Shared>>>,
        began: Arc<AtomicUsize>,
        woken: Arc<AtomicUsize>,
    }

    impl PerEntry {
        /// The builds and lay-outs of the entries with `text`.
        fn of(&self, text: &str) -> (usize, Vec<(u16, u16)>) {
            let by_text = self.by_text.lock().expect("no test thread panicked");
            by_text.get(text).map_or((0, Vec::new()), |shared| {
                let recorded = calls(shared);
                (recorded.builds, recorded.lay_outs.clone())
            })
        }
    }

    fn per_entry(gate: &Gate, held: fn(&str) -> bool) -> PerEntry {
        let by_text: Arc<Mutex<HashMap<String, Shared>>> = Arc::default();
        let began = Arc::new(AtomicUsize::new(0));
        let mut viewers = viewers();
        let (theirs, their_count, held_back) =
            (Arc::clone(&by_text), Arc::clone(&began), gate.clone());
        viewers.building(Arc::new(move |entry, _, _, _| {
            let ContextEntry::Code(code) = entry else {
                unreachable!("the tests build code entries only")
            };
            let text = code.content.clone().unwrap_or_default();
            their_count.fetch_add(1, Ordering::SeqCst);
            if held(&text) {
                held_back.wait();
            }
            let shared = Arc::clone(
                theirs
                    .lock()
                    .expect("no test thread panicked")
                    .entry(text)
                    .or_default(),
            );
            calls(&shared).builds += 1;
            Box::new(Recording {
                calls: shared,
                gate: None,
            })
        }));
        let woken = counted(&mut viewers);
        PerEntry {
            viewers,
            by_text,
            began,
            woken,
        }
    }

    /// An entry queued to load ahead at one size and shown at another
    /// starts at the size shown, and is laid out once.
    #[test]
    fn a_queued_entry_shown_at_another_size_lays_out_once() {
        let gate = Gate::default();
        let mut entries = per_entry(&gate, |text| text == "a");
        let (a, b) = (code("a"), code("b"));
        entries.viewers.load_window(
            "s",
            &[(at(0, Some(0)), &a), (at(0, Some(1)), &b)],
            Some((40, 10)),
            COLOURED,
        );
        test_support::wait_until(&entries.began, 1);
        entries.viewers.waiting(Duration::from_secs(10));

        entries
            .viewers
            .laid_out("s", at(0, Some(1)), &b, 30, 10, COLOURED);

        gate.open();
        test_support::wait_until(&entries.woken, 2);
        assert_eq!(entries.of("b"), (1, vec![(30, 10)]));
    }

    /// A window sent again at a new size gives its queued jobs that size,
    /// and keeps them: the same ticket, no new job.
    #[test]
    fn a_window_sent_at_a_new_size_rewrites_the_queued_sizes() {
        let gate = Gate::default();
        let mut entries = per_entry(&gate, |text| text == "a");
        let (a, b) = (code("a"), code("b"));
        let window = [(at(0, Some(0)), &a), (at(0, Some(1)), &b)];
        entries
            .viewers
            .load_window("s", &window, Some((40, 10)), COLOURED);
        test_support::wait_until(&entries.began, 1);
        let [(_, ticket, _)] = entries.viewers.queued_jobs()[..] else {
            panic!("b waits behind a");
        };

        entries
            .viewers
            .load_window("s", &window, Some((30, 10)), COLOURED);

        assert_eq!(
            entries.viewers.queued_jobs(),
            [(at(0, Some(1)), ticket, (30, 10))]
        );
        gate.open();
        test_support::wait_until(&entries.woken, 2);
    }

    /// A replaced session file can show an entry under a key whose old
    /// entry still waits in the queue. The new entry's job is the one
    /// noted, and the next window neither queues it again nor runs the
    /// old one.
    #[test]
    fn a_replaced_entry_queued_ahead_is_built_once() {
        let gate = Gate::default();
        let mut entries = per_entry(&gate, |text| text.starts_with('a'));
        let (a, old, new, l) = (code("a"), code("k1"), code("a2"), code("l"));
        let (shown, replaced, last) = (at(0, Some(0)), at(0, Some(1)), at(0, Some(2)));
        entries.viewers.load_window(
            "s",
            &[(shown, &a), (replaced, &old)],
            Some((40, 10)),
            COLOURED,
        );
        test_support::wait_until(&entries.began, 1);
        entries.viewers.waiting(Duration::ZERO);

        entries
            .viewers
            .laid_out("s", replaced, &new, 40, 10, COLOURED);
        entries.viewers.load_window(
            "s",
            &[(shown, &a), (replaced, &new), (last, &l)],
            Some((40, 10)),
            COLOURED,
        );

        gate.open();
        test_support::wait_until(&entries.woken, 3);
        assert_eq!(entries.of("a2").0, 1);
        assert_eq!(entries.of("k1").0, 0, "the old entry never ran");
    }

    /// A window whose entry was replaced while its job waited queues a job
    /// for the new entry in the old one's place.
    #[test]
    fn a_window_with_a_replaced_entry_queues_the_new_one() {
        let gate = Gate::default();
        let mut entries = per_entry(&gate, |text| text == "a");
        let (a, old, new) = (code("a"), code("k1"), code("k2"));
        let (first, replaced) = (at(0, Some(0)), at(0, Some(1)));
        entries.viewers.load_window(
            "s",
            &[(first, &a), (replaced, &old)],
            Some((40, 10)),
            COLOURED,
        );
        test_support::wait_until(&entries.began, 1);
        let [(_, old_ticket, _)] = entries.viewers.queued_jobs()[..] else {
            panic!("k1 waits behind a");
        };

        entries.viewers.load_window(
            "s",
            &[(first, &a), (replaced, &new)],
            Some((40, 10)),
            COLOURED,
        );

        let [(key, ticket, _)] = entries.viewers.queued_jobs()[..] else {
            panic!("k2 waits behind a");
        };
        assert_eq!(key, replaced);
        assert_ne!(ticket, old_ticket);
        gate.open();
        test_support::wait_until(&entries.woken, 2);
        assert_eq!((entries.of("k1").0, entries.of("k2").0), (0, 1));
    }

    // -----------------------------------------------------------------
    // The cap on threads of their own
    // -----------------------------------------------------------------

    /// Moving fast over slow entries runs the cap's jobs on threads of
    /// their own and one on the thread that loads ahead; the others wait
    /// in its queue, the latest entry shown first.
    #[test]
    fn entries_shown_past_the_cap_wait_first_in_line() {
        let gate = Gate::default();
        let mut entries = per_entry(&gate, |_| true);
        entries.viewers.capping(2);
        entries.viewers.waiting(Duration::ZERO);
        let texts = ["1", "2", "3", "4", "5"].map(code);
        let key = |index: usize| at(0, Some(index));
        let show = |entries: &mut PerEntry, index: usize| {
            entries
                .viewers
                .laid_out("s", key(index), &texts[index], 40, 10, COLOURED);
        };

        show(&mut entries, 0);
        show(&mut entries, 1);
        show(&mut entries, 2);
        test_support::wait_until(&entries.began, 3);
        show(&mut entries, 3);
        show(&mut entries, 4);
        thread::sleep(Duration::from_millis(50));

        assert_eq!(entries.viewers.queued(), [key(4), key(3)]);
        assert_eq!(entries.began.load(Ordering::SeqCst), 3, "the cap and one");
        show(&mut entries, 4);
        assert_eq!(entries.viewers.queued(), [key(4), key(3)], "first already");
        assert_eq!(entries.viewers.built, 5, "one build each");
        gate.open();
        test_support::wait_until(&entries.woken, 5);
    }

    /// A job that ends frees its thread before it wakes the run loop, so
    /// the draw it wakes starts the entry shown on a thread of its own
    /// instead of leaving it behind the queue's job.
    #[test]
    fn a_freed_thread_takes_the_queued_entry_shown() {
        let (one, busy) = (Gate::default(), Gate::default());
        let began = Arc::new(AtomicUsize::new(0));
        let mut viewers = viewers();
        let (their_one, their_busy, their_count) = (one.clone(), busy.clone(), Arc::clone(&began));
        viewers.building(Arc::new(move |entry, images, quote, home| {
            if let ContextEntry::Code(code) = entry {
                their_count.fetch_add(1, Ordering::SeqCst);
                match code.content.as_deref() {
                    Some("one") => their_one.wait(),
                    Some("busy") => their_busy.wait(),
                    _ => {}
                }
            }
            viewer_for(entry, images, quote, home)
        }));
        let woken = counted(&mut viewers);
        viewers.capping(1);
        viewers.waiting(Duration::ZERO);
        let (first, ahead, shown) = (code("one"), code("busy"), code("two"));
        viewers.laid_out("s", at(0, Some(0)), &first, 40, 10, COLOURED);
        viewers.laid_out("s", at(0, Some(1)), &ahead, 40, 10, COLOURED);
        test_support::wait_until(&began, 2);
        viewers.laid_out("s", at(0, Some(2)), &shown, 40, 10, COLOURED);
        assert_eq!(viewers.queued(), [at(0, Some(2))]);

        one.open();
        test_support::wait_until(&woken, 1);
        // The draw the wake starts; its job's wait was spent above.
        viewers.laid_out("s", at(0, Some(2)), &shown, 40, 10, COLOURED);
        test_support::wait_until(&woken, 2);
        let (viewer, _) = viewers.laid_out("s", at(0, Some(2)), &shown, 40, 10, COLOURED);
        let buf = test_support::drawn(viewer, 40, 10, Offset::default());

        assert_eq!(row(&buf, 0), "1 two", "while busy still waits");
        busy.open();
        test_support::wait_until(&woken, 3);
    }

    /// Where the system refuses the thread that loads ahead, a job past
    /// the cap runs on a thread of its own rather than wait for ever.
    #[test]
    fn a_job_past_the_cap_runs_on_its_own_thread_without_the_ahead_thread() {
        let gate = Gate::default();
        let mut entries = per_entry(&gate, |_| true);
        entries.viewers.capping(1);
        entries.viewers.refusing_the_ahead_thread();
        entries.viewers.waiting(Duration::ZERO);
        let (first, second) = (code("1"), code("2"));

        entries
            .viewers
            .laid_out("s", at(0, Some(0)), &first, 40, 10, COLOURED);
        entries
            .viewers
            .laid_out("s", at(0, Some(1)), &second, 40, 10, COLOURED);

        test_support::wait_until(&entries.began, 2);
        assert_eq!(entries.viewers.queued(), []);
        gate.open();
        test_support::wait_until(&entries.woken, 2);
    }

    /// Only an image a frame showed can have had its transfer dropped by
    /// tmux; one loaded ahead has sent nothing and stays.
    #[test]
    fn regaining_the_focus_reloads_only_images_a_frame_showed() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let mut viewers = kitty_viewers();
        let woken = counted(&mut viewers);
        let (shown, ahead) = (picture(&directory, "a.png"), picture(&directory, "b.png"));
        built(&mut viewers, "s", QUESTION, &shown);
        viewers.load_window(
            "s",
            &[(QUESTION, &shown), (OPTION, &ahead)],
            Some((40, 10)),
            COLOURED,
        );
        test_support::wait_until(&woken, 2);
        viewers.collect();
        assert_eq!(viewers.held_images(), [1, 2]);
        viewers.laid_out("s", QUESTION, &shown, 30, 8, COLOURED);
        assert_eq!(
            viewers.dropped_images(),
            [1],
            "laid out again under a new id"
        );

        viewers.forget_images();

        assert_eq!(viewers.dropped_images(), [3], "the one shown");
        assert_eq!(viewers.held_images(), [2], "the one loaded ahead");
    }

    /// The window is the previous, the current and the next question:
    /// what lies outside goes, shown or not, of any session, and loads
    /// again on coming back.
    #[test]
    fn the_viewers_outside_the_window_go() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let mut viewers = kitty_viewers();
        let first = picture(&directory, "a.png");
        let text = code("x");
        built(&mut viewers, "s", at(0, None), &first);
        built(&mut viewers, "other", at(0, None), &text);
        let second = code("second");

        viewers.load_window(
            "s",
            &[(at(1, None), &second), (at(0, None), &first)],
            None,
            COLOURED,
        );
        assert!(
            viewers.held_images().contains(&1),
            "the question before stays"
        );

        viewers.load_window("s", &[(at(3, None), &second)], None, COLOURED);
        assert_eq!(viewers.dropped_images(), [1], "shown, and gone");
        assert!(viewers.viewers.is_empty(), "also the other session's");

        built(&mut viewers, "s", at(0, None), &first);
        assert_eq!(viewers.built, 3, "built again");
    }

    #[test]
    fn a_viewer_is_built_once_per_entry_and_reused() {
        let mut viewers = viewers();
        let image = entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#);
        let text = entry(r#"{"type": "code", "content": "notes"}"#);

        built(&mut viewers, "s", QUESTION, &image);
        built(&mut viewers, "s", QUESTION, &image);
        assert_eq!(viewers.built, 1, "the second draw reuses the viewer");

        built(&mut viewers, "s", OPTION, &text);
        built(&mut viewers, "other", QUESTION, &image);
        built(&mut viewers, "s", QUESTION, &image);
        built(&mut viewers, "s", OPTION, &text);
        assert_eq!(viewers.built, 3, "one per entry of each session");
    }

    #[test]
    fn a_replaced_entry_gets_a_new_viewer_built_and_laid_out() {
        let (mut viewers, shared) = recording();
        built(
            &mut viewers,
            "s",
            QUESTION,
            &entry(r#"{"type": "code", "content": "one"}"#),
        );

        built(
            &mut viewers,
            "s",
            QUESTION,
            &entry(r#"{"type": "code", "content": "two"}"#),
        );

        assert_eq!(viewers.built, 2);
        assert_eq!(viewers.viewers.len(), 1, "the old one is gone");
        assert_eq!(calls(&shared).builds, 2);
        assert_eq!(calls(&shared).lay_outs.len(), 2, "at the same size");
    }

    #[test]
    fn a_viewer_is_built_again_for_another_quote_style() {
        let mut viewers = viewers();
        let notes = entry(r#"{"type": "markdown", "content": "> quoted"}"#);

        viewers.laid_out(
            "s",
            QUESTION,
            &notes,
            40,
            10,
            QuoteStyle::Bar(AnsiColor::Blue),
        );
        viewers.laid_out("s", QUESTION, &notes, 40, 10, QuoteStyle::DimBar);

        assert_eq!(viewers.built, 2, "its quotes show in the new style");
        assert_eq!(viewers.viewers.len(), 1, "the old one is gone");
    }

    #[test]
    fn the_viewers_of_a_finished_session_go() {
        let mut viewers = viewers();
        let image = entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#);
        built(&mut viewers, "finished", QUESTION, &image);
        built(&mut viewers, "waiting", QUESTION, &image);

        viewers.retain_sessions(|session| session == "waiting");
        built(&mut viewers, "waiting", QUESTION, &image);

        assert_eq!(viewers.viewers.len(), 1);
        assert_eq!(viewers.built, 2, "the waiting session kept its viewer");
    }

    #[test]
    fn forgetting_images_rebuilds_only_the_image_viewers() {
        let mut viewers = viewers();
        let image = entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#);
        let text = entry(r#"{"type": "code", "content": "notes"}"#);
        built(&mut viewers, "s", QUESTION, &image);
        built(&mut viewers, "s", OPTION, &text);

        viewers.forget_images();
        built(&mut viewers, "s", QUESTION, &image);
        built(&mut viewers, "s", OPTION, &text);

        assert_eq!(viewers.built, 3, "the image again, the text kept");
    }

    /// The entry's file is read when the entry first shows, and a viewer
    /// laid out for another size shows what was read then.
    #[test]
    fn a_file_is_read_when_its_entry_first_shows_and_not_again() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("notes.txt");
        let code: ContextEntry =
            serde_json::from_value(serde_json::json!({"type": "code", "path": path}))
                .expect("test entry parses");
        let mut viewers = viewers();
        std::fs::write(&path, "first\n").expect("file is writable");

        let buf = drawn_by(&mut viewers, &code, 40, 1, 0);
        assert_eq!(row(&buf, 0), "1 first");

        std::fs::write(&path, "second\n").expect("file is writable");
        let buf = drawn_by(&mut viewers, &code, 30, 2, 0);
        assert_eq!(row(&buf, 0), "1 first", "a resize reads nothing");
    }

    /// An image is decoded when its entry first shows. Laying it out for
    /// another size encodes the picture decoded then, without reading the
    /// file again.
    #[test]
    fn an_image_is_decoded_when_its_entry_first_shows_and_not_again() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let dot = picture(&directory, "a.png");
        let mut viewers = viewers();
        let shows = |buf: &Buffer, colour: ratatui::style::Color| {
            buf.content
                .iter()
                .any(|cell| cell.fg == colour || cell.bg == colour)
        };
        let red = ratatui::style::Color::Rgb(255, 0, 0);
        let green = ratatui::style::Color::Rgb(0, 255, 0);

        assert!(shows(&drawn_by(&mut viewers, &dot, 40, 10, 0), red));

        image::RgbImage::from_pixel(2, 2, image::Rgb([0, 255, 0]))
            .save(directory.path().join("a.png"))
            .expect("image written");
        let buf = drawn_by(&mut viewers, &dot, 30, 8, 0);
        assert!(shows(&buf, red), "the picture read first: {buf:?}");
        assert!(!shows(&buf, green), "not the file as it is now");
    }

    // -----------------------------------------------------------------
    // Deleting Kitty images
    // -----------------------------------------------------------------

    /// Viewers whose terminal speaks the Kitty protocol and whose images
    /// get the ids 1, 2, 3 and so on.
    fn kitty_viewers() -> Viewers {
        Viewers::for_tests(Images::kitty())
    }

    /// An image entry for a red picture written into `directory` as
    /// `name`.
    fn picture(directory: &tempfile::TempDir, name: &str) -> ContextEntry {
        let path = directory.path().join(name);
        image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]))
            .save(&path)
            .expect("image written");
        serde_json::from_value(serde_json::json!({"type": "image", "path": path}))
            .expect("test entry parses")
    }

    #[test]
    fn forgetting_the_images_drops_their_kitty_ids() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let mut viewers = kitty_viewers();
        built(&mut viewers, "s", QUESTION, &picture(&directory, "a.png"));
        built(
            &mut viewers,
            "s",
            OPTION,
            &entry(r#"{"type": "code", "content": "x"}"#),
        );
        assert_eq!(viewers.held_images(), [1]);

        viewers.forget_images();
        built(&mut viewers, "s", QUESTION, &picture(&directory, "a.png"));

        assert_eq!(viewers.dropped_images(), [1]);
        assert_eq!(viewers.held_images(), [2], "sent again under a new id");
        assert_eq!(
            viewers.dropped_images(),
            Vec::<u32>::new(),
            "handed out once"
        );
    }

    #[test]
    fn a_session_that_leaves_drops_the_kitty_ids_of_its_images() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let mut viewers = kitty_viewers();
        built(
            &mut viewers,
            "finished",
            QUESTION,
            &picture(&directory, "a.png"),
        );
        built(
            &mut viewers,
            "waiting",
            QUESTION,
            &picture(&directory, "b.png"),
        );

        viewers.retain_sessions(|session| session == "waiting");

        assert_eq!(viewers.dropped_images(), [1]);
        assert_eq!(viewers.held_images(), [2]);
    }

    /// A replaced entry drops the old viewer's image whatever replaces it:
    /// another image, an image that cannot be read, or another kind.
    #[test]
    fn a_replaced_image_entry_drops_its_kitty_id() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let mut viewers = kitty_viewers();
        built(&mut viewers, "s", QUESTION, &picture(&directory, "a.png"));
        built(&mut viewers, "s", QUESTION, &picture(&directory, "a.png"));
        assert_eq!(
            viewers.dropped_images(),
            Vec::<u32>::new(),
            "the same entry keeps it"
        );

        built(&mut viewers, "s", QUESTION, &picture(&directory, "b.png"));
        assert_eq!(viewers.dropped_images(), [1], "another image");
        assert_eq!(viewers.held_images(), [2]);

        let missing = entry(r#"{"type": "image", "path": "/nowhere/cat.png"}"#);
        built(&mut viewers, "s", QUESTION, &missing);
        assert_eq!(viewers.dropped_images(), [2], "an image not shown");
        assert_eq!(viewers.held_images(), Vec::<u32>::new());

        built(&mut viewers, "s", QUESTION, &picture(&directory, "c.png"));
        built(
            &mut viewers,
            "s",
            QUESTION,
            &entry(r#"{"type": "code", "content": "x"}"#),
        );
        assert_eq!(viewers.dropped_images(), [3], "another kind");
    }

    /// Half blocks and the other protocols carry their picture in the
    /// cells, so nothing of theirs stays in the terminal to delete.
    #[test]
    fn images_without_the_kitty_protocol_drop_no_id() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        for protocol in [Protocol::HalfBlocks, Protocol::Iterm2] {
            let mut viewers = Viewers::for_tests(Images::new(Graphics {
                protocol,
                ..Graphics::HALF_BLOCKS
            }));
            built(&mut viewers, "s", QUESTION, &picture(&directory, "a.png"));
            built(&mut viewers, "s", QUESTION, &picture(&directory, "b.png"));
            built(&mut viewers, "t", QUESTION, &picture(&directory, "c.png"));
            viewers.forget_images();
            viewers.retain_sessions(|_| false);

            assert_eq!(viewers.dropped_images(), Vec::<u32>::new(), "{protocol:?}");
        }
    }

    // -----------------------------------------------------------------
    // Images
    // -----------------------------------------------------------------

    #[test]
    fn an_image_takes_the_body() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("dot.png");
        image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]))
            .save(&path)
            .expect("image written");
        let image: ContextEntry =
            serde_json::from_value(serde_json::json!({"type": "image", "path": path}))
                .expect("test entry parses");
        let mut viewers = viewers();

        let (viewer, size) = viewers.laid_out("s", QUESTION, &image, 40, 12, COLOURED);

        assert_eq!(
            size,
            ContentSize {
                lines: 12,
                columns: 40
            }
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 12));
        viewer.draw(buf.area, &mut buf, Offset::default());
        let red = ratatui::style::Color::Rgb(255, 0, 0);
        assert!(
            buf.content
                .iter()
                .any(|cell| cell.fg == red || cell.bg == red),
            "the image draws: {buf:?}"
        );
    }

    #[test]
    fn an_image_that_cannot_be_read_shows_why_as_its_content() {
        let reason = std::fs::metadata("/nowhere/cat.png")
            .expect_err("the file is missing")
            .to_string();

        let rows = drawn(
            &entry(r#"{"type": "image", "path": "/nowhere/cat.png"}"#),
            50,
            8,
            0,
        );

        assert!(rows[1].trim_start().starts_with('╭'), "{rows:#?}");
        assert!(
            rows[2].contains(&format!("│ ✗ {reason}")),
            "the reason first: {rows:#?}"
        );
        assert!(rows[4].contains("│ cat.png "), "{rows:#?}");
        assert!(rows[5].contains("│ /nowhere/ "), "{rows:#?}");
        assert!(rows[6].trim_start().starts_with('╰'), "{rows:#?}");
    }

    /// Why the image entry at `path` cannot be shown.
    fn refused_image(path: &std::path::Path) -> Unreadable {
        let image: ContextEntry =
            serde_json::from_value(serde_json::json!({"type": "image", "path": path}))
                .expect("test entry parses");
        let ContextEntry::Image(image) = image else {
            unreachable!("the entry is an image")
        };
        load(&image, &Images::half_blocks())
            .err()
            .expect("the image cannot be shown")
    }

    #[test]
    fn an_image_file_larger_than_64_mib_shows_a_card_unread() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("huge.png");
        std::fs::File::create(&path)
            .and_then(|huge| huge.set_len(64 * 1024 * 1024 + 1))
            .expect("sparse file is creatable");

        let refused = refused_image(&path);
        assert_eq!(refused.cause, "TooLarge");

        let json = serde_json::json!({"type": "image", "path": path}).to_string();
        let rows = drawn(&entry(&json), 50, 8, 0);
        assert!(
            rows[2].contains("│ ✗ image file larger than 64 MiB"),
            "{rows:#?}"
        );
    }

    #[test]
    fn an_svg_is_a_format_asqr_cannot_decode() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("emblem.svg");
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#)
            .expect("file is writable");

        let refused = refused_image(&path);

        assert_eq!(refused.cause, "Unsupported");
        assert!(
            refused
                .reason
                .contains("was not recognized as an image format"),
            "{}",
            refused.reason
        );
    }

    /// TGA has no signature in its first bytes, so only the extension
    /// names its format.
    #[test]
    fn a_tga_file_loads_by_its_extension() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("dot.tga");
        image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]))
            .save(&path)
            .expect("image written");
        let image: ContextEntry =
            serde_json::from_value(serde_json::json!({"type": "image", "path": path}))
                .expect("test entry parses");

        let buf = drawn_by(&mut viewers(), &image, 20, 10, 0);

        let red = ratatui::style::Color::Rgb(255, 0, 0);
        assert!(
            buf.content
                .iter()
                .any(|cell| cell.fg == red || cell.bg == red),
            "{buf:?}"
        );
    }

    #[test]
    fn a_file_without_extension_that_is_no_image_cannot_be_decoded() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("notes");
        std::fs::write(&path, b"no picture in here").expect("file is writable");

        let refused = refused_image(&path);

        assert_eq!(refused.cause, "Unsupported");
        assert!(
            refused.reason.contains("could not be determined"),
            "{}",
            refused.reason
        );
    }

    #[test]
    fn a_cut_png_is_damaged() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let whole = directory.path().join("whole.png");
        image::RgbImage::from_pixel(64, 64, image::Rgb([255, 0, 0]))
            .save(&whole)
            .expect("image written");
        let bytes = std::fs::read(&whole).expect("image read");
        let cut = directory.path().join("cut.png");
        std::fs::write(&cut, &bytes[..bytes.len() / 2]).expect("file is writable");

        assert_eq!(refused_image(&cut).cause, "IoError(UnexpectedEof)");
    }

    #[test]
    fn an_image_is_refused_by_the_path_rule() {
        let (_directory, paths) = test_support::refused_paths();

        for path in paths {
            let image = entry(&format!(r#"{{"type": "image", "path": {path:?}}}"#));
            let mut refused = regular_file(&path)
                .expect_err("the rule refuses it")
                .card(None);

            assert_eq!(
                drawn(&image, 60, 8, 0),
                drawn_viewer(&mut refused, 60, 8, 0),
                "{path}"
            );
        }
    }

    #[test]
    fn a_file_that_does_not_decode_shows_why() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("notes.png");
        std::fs::write(&path, b"no picture in here").expect("file is writable");
        let image: ContextEntry =
            serde_json::from_value(serde_json::json!({"type": "image", "path": path}))
                .expect("test entry parses");

        let refused = refused_image(&path);
        let rows = drawn(&image, 200, 6, 0);

        assert_eq!(refused.cause, "Decoding");
        assert!(
            rows[1].contains(&format!("✗ {}", refused.reason)),
            "{rows:#?}"
        );
        assert!(rows[3].contains("notes.png"), "{rows:#?}");
    }

    #[test]
    fn an_image_that_cannot_be_read_scrolls_like_any_card() {
        let mut card = regular_file("/nowhere/cat.png")
            .expect_err("the file is missing")
            .card(None);
        let image = entry(r#"{"type": "image", "path": "/nowhere/cat.png"}"#);

        let (_, size) = viewers().laid_out("s", QUESTION, &image, 20, 2, COLOURED);

        assert_eq!(size, card.lay_out(20, 2));
        assert!(size.lines > 2, "taller than the body: {size:?}");
        assert_eq!(
            drawn(&image, 20, 2, 2),
            drawn_viewer(&mut card, 20, 2, 2),
            "it starts at the offset"
        );
    }

    #[test]
    fn a_viewer_that_cannot_show_its_entry_says_so() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let failed = |entry: &ContextEntry| {
            let mut viewers = viewers();
            let (viewer, _) = viewers.laid_out("s", QUESTION, entry, 40, 10, COLOURED);
            viewer.failed()
        };

        for json in [
            r#"{"type": "code", "path": "/nowhere/fetch.rs"}"#,
            r#"{"type": "markdown", "path": "/nowhere/plan.md"}"#,
            r#"{"type": "diff", "new": {"path": "/nowhere/fetch.rs"}}"#,
            r#"{"type": "image", "path": "/nowhere/cat.png"}"#,
        ] {
            assert!(failed(&entry(json)), "{json}");
        }
        for json in [
            r#"{"type": "code", "content": "fn main() {}"}"#,
            r#"{"type": "code", "content": ""}"#,
            r#"{"type": "markdown", "content": "*notes*"}"#,
            r#"{"type": "diff", "new": {"content": "x"}}"#,
        ] {
            assert!(!failed(&entry(json)), "{json}");
        }
        assert!(!failed(&picture(&directory, "a.png")), "a decoded image");

        let (mut recording, _) = recording();
        let (viewer, _) = recording.laid_out(
            "s",
            QUESTION,
            &entry(r#"{"type": "code", "content": "x"}"#),
            40,
            10,
            COLOURED,
        );
        assert!(!viewer.failed(), "a viewer that shows no problem says so");
    }

    /// What `run` logged, as asqr's log file has it.
    fn logged(run: impl FnOnce()) -> String {
        #[derive(Clone, Default)]
        struct Written(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

        impl std::io::Write for Written {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0
                    .lock()
                    .expect("no test thread panicked while writing")
                    .extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let written = Written::default();
        let log = written.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || log.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, run);
        let bytes = written
            .0
            .lock()
            .expect("no test thread panicked while writing")
            .clone();
        String::from_utf8(bytes).expect("the log is UTF-8")
    }

    #[test]
    fn a_card_logs_the_original_message_once() {
        let reason = std::fs::metadata("/nowhere/fetch.rs")
            .expect_err("the file is missing")
            .to_string();
        let code = entry(r#"{"type": "code", "path": "/nowhere/fetch.rs"}"#);

        let log = logged(|| {
            let mut viewers = viewers();
            for (width, height) in [(40, 10), (40, 10), (30, 8)] {
                viewers.laid_out("s", QUESTION, &code, width, height, COLOURED);
            }
        });

        assert_eq!(log.matches("/nowhere/fetch.rs").count(), 1, "{log}");
        assert_eq!(log.matches(&reason).count(), 1, "{log}");
        assert!(log.contains("cause=NotFound"), "{log}");
    }

    /// The log names what failed in the words of the system or the image
    /// decoder, where the card shows only the reason.
    #[test]
    fn a_card_logs_the_cause_of_a_cut_image() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let whole = directory.path().join("whole.png");
        image::RgbImage::from_pixel(64, 64, image::Rgb([255, 0, 0]))
            .save(&whole)
            .expect("image written");
        let bytes = std::fs::read(&whole).expect("image read");
        let cut = directory.path().join("cut.png");
        std::fs::write(&cut, &bytes[..bytes.len() / 2]).expect("file is writable");
        let image = entry(&serde_json::json!({"type": "image", "path": cut}).to_string());

        let log = logged(|| {
            viewers().laid_out("s", QUESTION, &image, 40, 10, COLOURED);
        });

        assert!(log.contains("cause=IoError(UnexpectedEof)"), "{log}");
    }

    // -----------------------------------------------------------------
    // Placeholders
    // -----------------------------------------------------------------

    #[test]
    fn a_note_is_one_dim_line() {
        let mut note = Placeholder::note("empty");

        assert_eq!(
            note.lay_out(30, 4),
            ContentSize {
                lines: 1,
                columns: 5
            }
        );
        assert_eq!(drawn_viewer(&mut note, 30, 1, 0), ["empty"]);
    }

    // -----------------------------------------------------------------
    // Diffs
    // -----------------------------------------------------------------

    #[test]
    fn a_diff_entry_shows_its_diff() {
        let diff = entry(
            r#"{"type": "diff", "name": "src/fetch.rs", "old": {"content": "get(url)\n"}, "new": {"content": "get(uri)\n"}}"#,
        );

        assert_eq!(drawn(&diff, 40, 2, 0), ["1   - get(url)", "  1 + get(uri)"]);
    }

    // -----------------------------------------------------------------
    // Code and Markdown
    // -----------------------------------------------------------------

    #[test]
    fn a_code_entry_shows_its_numbered_lines() {
        let code =
            entry(r#"{"type": "code", "language": "rust", "content": "fn a() {}\nfn b() {}\n"}"#);

        assert_eq!(drawn(&code, 20, 2, 0), ["1 fn a() {}", "2 fn b() {}"]);
    }

    #[test]
    fn a_markdown_entry_shows_rendered() {
        let markdown = entry(r#"{"type": "markdown", "content": "**Plan**\n\n- one\n"}"#);

        assert_eq!(drawn(&markdown, 20, 3, 0), ["Plan", "", "• one"]);
    }

    #[test]
    fn code_and_markdown_viewers_keep_what_they_derived_when_the_images_go() {
        let mut viewers = viewers();
        let code = entry(r#"{"type": "code", "content": "x"}"#);
        let markdown = entry(r#"{"type": "markdown", "content": "x"}"#);
        built(&mut viewers, "s", QUESTION, &code);
        built(&mut viewers, "s", OPTION, &markdown);

        viewers.forget_images();
        built(&mut viewers, "s", QUESTION, &code);
        built(&mut viewers, "s", OPTION, &markdown);

        assert_eq!(viewers.built, 2);
    }
}
