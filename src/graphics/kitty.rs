// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Kitty image ids of a run and the commands that delete their images
//! (ADR 42). The terminal keeps an image sent under the Kitty protocol
//! until it is deleted, so asqr hands out every id itself and notes when
//! the image that holds it goes.

use std::collections::{BTreeSet, HashSet};
use std::hash::{BuildHasher, RandomState};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::Graphics;
use crate::terminal_env::passthrough;

/// Where Kitty ids come from. It is `Send` as the book is shared with the
/// panic hook.
pub(super) type IdSource = Box<dyn FnMut() -> u32 + Send>;

/// Where Kitty ids come from: 32 random bits per call. `RandomState` is
/// seeded from the operating system once per thread, so two asqr
/// processes, which may share one terminal through tmux, draw different
/// ids.
pub(super) fn random_ids() -> IdSource {
    let random = RandomState::new();
    let mut drawn: u64 = 0;
    Box::new(move || {
        drawn += 1;
        let hash = random.hash_one(drawn);
        u32::try_from(hash >> 32).expect("32 bits are left of 64 shifted by 32")
    })
}

/// What the book of a run holds.
struct Ids {
    source: IdSource,
    /// Every id handed out in this run. An id is never handed out twice:
    /// a delete for an id written late, or written again, would remove
    /// the image that took it over.
    issued: HashSet<u32>,
    /// The ids of the images that exist. A panic deletes them too, as its
    /// hook runs before the unwinding lets the images go.
    live: BTreeSet<u32>,
    /// The ids of the images that went and are not deleted yet.
    dropped: Vec<u32>,
    /// What the repeat after regaining the focus needs, where the images
    /// are sent again then (ADR 42).
    repeat: Option<FocusRepeat>,
    /// Whether the deletes go out wrapped for tmux's passthrough, as the
    /// transmissions of the images do.
    tmux: bool,
}

/// Inside tmux with `allow-passthrough on`, a delete written while asqr's
/// pane is hidden is lost, as a transfer is (ADR 24). The deletes since
/// the last focus gain are written again after the frame that follows the
/// next one, and only then forgotten.
#[derive(Default)]
struct FocusRepeat {
    since_focus: Vec<u32>,
    regained: bool,
}

impl Ids {
    /// A new id, drawn again while it is 0, which names no image in the
    /// Kitty protocol, or one handed out before.
    fn issue(&mut self) -> u32 {
        loop {
            let id = (self.source)();
            if id != 0 && self.issued.insert(id) {
                self.live.insert(id);
                return id;
            }
        }
    }

    /// Notes that the image with `id` went, for the terminal to delete.
    /// Every way an image goes ends here, through the drop of its
    /// [`HeldId`]. An id drained already is deleted already.
    fn release(&mut self, id: u32) {
        if self.live.remove(&id) {
            self.dropped.push(id);
        }
    }

    /// Every id the terminal may still keep, each once: those of the
    /// images that went, then those of the images that exist.
    fn drain(&mut self) -> KittyDeletes {
        let mut ids = std::mem::take(&mut self.dropped);
        ids.extend(std::mem::take(&mut self.live));
        KittyDeletes {
            ids,
            tmux: self.tmux,
        }
    }
}

/// The book of the Kitty image ids of one run. Its clones share it: the
/// images take their ids from it, and the run loop and the exit take the
/// deletes out of it.
#[derive(Clone)]
pub struct KittyBook(Arc<Mutex<Ids>>);

impl KittyBook {
    pub(super) fn new(graphics: Graphics, source: IdSource) -> Self {
        KittyBook(Arc::new(Mutex::new(Ids {
            source,
            issued: HashSet::new(),
            live: BTreeSet::new(),
            dropped: Vec::new(),
            repeat: graphics.resends_on_focus().then(FocusRepeat::default),
            tmux: graphics.tmux_wrapped,
        })))
    }

    /// The book, also after a panic while it was locked: the ids in it
    /// still name the images the terminal keeps.
    fn lock(&self) -> MutexGuard<'_, Ids> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Notes that asqr regained the focus and sends its images again, so
    /// the deletes after the next frame repeat those since the last time.
    pub fn focus_gained(&self) {
        if let Some(repeat) = &mut self.lock().repeat {
            repeat.regained = true;
        }
    }

    /// The deletes to write after a frame is on the screen: the images
    /// that went before it, which it no longer shows, and after a focus
    /// gain those since the one before (ADR 42).
    pub fn after_frame(&self) -> KittyDeletes {
        let mut book = self.lock();
        let book = &mut *book;
        let mut ids = std::mem::take(&mut book.dropped);
        if let Some(repeat) = &mut book.repeat {
            if std::mem::take(&mut repeat.regained) {
                ids.splice(0..0, repeat.since_focus.drain(..));
            } else {
                repeat.since_focus.extend(&ids);
            }
        }
        KittyDeletes {
            ids,
            tmux: book.tmux,
        }
    }

    /// Takes back `deletes` that could not be written. They are written
    /// after the next frame, or on quitting.
    pub fn put_back(&self, deletes: KittyDeletes) {
        self.lock().dropped.splice(0..0, deletes.ids);
    }

    /// The deletes of every image the terminal may still keep, each once:
    /// those that went, then those that exist. A second call finds none.
    pub fn drain(&self) -> KittyDeletes {
        self.lock().drain()
    }

    /// The ids of the images that exist.
    #[cfg(test)]
    pub(super) fn live(&self) -> Vec<u32> {
        self.lock().live.iter().copied().collect()
    }
}

/// The Kitty id of one image, handed back to its book when the image
/// goes.
pub(super) struct HeldId {
    pub(super) id: u32,
    /// `None` once the id is forgotten.
    book: Option<KittyBook>,
}

impl HeldId {
    pub(super) fn issued(book: &KittyBook) -> Self {
        HeldId {
            id: book.lock().issue(),
            book: Some(book.clone()),
        }
    }

    /// Gives the id back without a delete, for an image that never went
    /// out to the terminal.
    pub(super) fn forget(mut self) {
        if let Some(book) = self.book.take() {
            book.lock().live.remove(&self.id);
        }
    }
}

impl Drop for HeldId {
    fn drop(&mut self) {
        if let Some(book) = &self.book {
            book.lock().release(self.id);
        }
    }
}

/// The Kitty commands that delete the images [`KittyDeletes::ids`] names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KittyDeletes {
    ids: Vec<u32>,
    tmux: bool,
}

impl KittyDeletes {
    pub fn ids(&self) -> &[u32] {
        &self.ids
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The commands, each wrapped for tmux's passthrough where the
    /// transmissions are, as ratatui-image wraps them (ADR 42).
    ///
    /// `d=I` with an id and no placement id removes every placement of the
    /// image, the virtual one its placeholders refer to included, and
    /// frees its data. `q=2` keeps the terminal from answering into asqr's
    /// input. A delete of an id the terminal does not know does nothing.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.ids
            .iter()
            .flat_map(|id| {
                let delete = format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\").into_bytes();
                if self.tmux {
                    passthrough(&delete)
                } else {
                    delete
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::Passthrough;
    use super::*;

    /// A book for `graphics` whose ids are 1, 2, 3 and so on.
    fn counting(graphics: Graphics) -> KittyBook {
        let mut last = 0;
        KittyBook::new(
            graphics,
            Box::new(move || {
                last += 1;
                last
            }),
        )
    }

    /// The Kitty protocol inside tmux with `allow-passthrough on`, where
    /// images are sent again on regaining the focus.
    fn while_visible() -> Graphics {
        Graphics {
            passthrough: Passthrough::WhileVisible,
            ..Graphics::KITTY
        }
    }

    #[test]
    fn a_kitty_id_is_never_0_and_never_handed_out_twice() {
        let mut drawn = vec![0, 7, 7, 9, 0, 7, 11].into_iter();
        let book = KittyBook::new(
            Graphics::KITTY,
            Box::new(move || drawn.next().expect("enough ids")),
        );

        let ids = [(); 3].map(|()| HeldId::issued(&book).id);

        assert_eq!(ids, [7, 9, 11]);
    }

    #[test]
    fn random_kitty_ids_differ() {
        let mut random = random_ids();
        let drawn: HashSet<u32> = (0..1000).map(|_| random()).collect();

        assert!(drawn.len() > 990, "{} of 1000 differ", drawn.len());
    }

    #[test]
    fn an_id_that_goes_is_deleted_after_the_next_frame_once() {
        let book = counting(Graphics::KITTY);
        let first = HeldId::issued(&book);
        let _second = HeldId::issued(&book);
        assert_eq!(book.after_frame().ids(), [0_u32; 0]);

        drop(first);

        assert_eq!(book.after_frame().ids(), [1]);
        assert_eq!(book.after_frame().ids(), [0_u32; 0]);
        assert_eq!(book.live(), [2]);
    }

    /// A forgotten id never went out, so it is neither deleted after a
    /// frame nor on quitting.
    #[test]
    fn a_forgotten_id_is_never_deleted() {
        let book = counting(Graphics::KITTY);

        HeldId::issued(&book).forget();

        assert_eq!(book.after_frame().ids(), [0_u32; 0]);
        assert_eq!(book.drain().ids(), [0_u32; 0]);
    }

    /// Deletes that could not be written come first after the next frame.
    #[test]
    fn deletes_put_back_are_written_after_the_next_frame() {
        let book = counting(Graphics::KITTY);
        let [first, second, third] = [(); 3].map(|()| HeldId::issued(&book));
        drop(first);
        drop(second);
        let failed = book.after_frame();

        drop(third);
        book.put_back(failed);

        assert_eq!(book.after_frame().ids(), [1, 2, 3]);
    }

    /// Draining takes every id once, those of the images that went first,
    /// and an image that goes after the drain hands on nothing more.
    #[test]
    fn draining_takes_every_id_once() {
        let book = counting(Graphics::KITTY);
        let [first, second, third] = [(); 3].map(|()| HeldId::issued(&book));
        drop(second);

        assert_eq!(book.drain().ids(), [2, 1, 3]);
        drop([first, third]);
        assert_eq!(book.drain().ids(), [0_u32; 0]);
        assert_eq!(book.after_frame().ids(), [0_u32; 0]);
    }

    /// After a focus gain, the next frame writes the deletes since the
    /// one before again, ahead of its own; they are then forgotten.
    #[test]
    fn regaining_the_focus_repeats_the_deletes_since_the_last_time_once() {
        let book = counting(while_visible());
        let [first, second, third] = [(); 3].map(|()| HeldId::issued(&book));
        drop(first);
        assert_eq!(book.after_frame().ids(), [1]);
        drop(second);
        assert_eq!(book.after_frame().ids(), [2]);

        book.focus_gained();
        drop(third);

        assert_eq!(book.after_frame().ids(), [1, 2, 3]);
        book.focus_gained();
        assert_eq!(book.after_frame().ids(), [0_u32; 0], "written once");
    }

    /// Only tmux with `allow-passthrough on` loses deletes from a hidden
    /// pane; everywhere else a focus gain repeats nothing.
    #[test]
    fn regaining_the_focus_repeats_nothing_where_no_delete_is_lost() {
        for passthrough in [Passthrough::None, Passthrough::Always] {
            let book = counting(Graphics {
                passthrough,
                ..Graphics::KITTY
            });
            drop(HeldId::issued(&book));
            assert_eq!(book.after_frame().ids(), [1], "{passthrough:?}");

            book.focus_gained();

            assert_eq!(book.after_frame().ids(), [0_u32; 0], "{passthrough:?}");
        }
    }

    /// A panic while the book was locked leaves it poisoned; the ids in
    /// it are still those of the images the terminal keeps.
    #[test]
    fn a_book_poisoned_by_a_panic_still_hands_out_its_deletes() {
        let book = counting(Graphics::KITTY);
        let image = HeldId::issued(&book);

        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _locked = book.0.lock();
            panic!("a panic while the book is locked");
        }));

        assert!(panicked.is_err());
        drop(image);
        assert_eq!(book.after_frame().ids(), [1]);
    }

    #[test]
    fn a_delete_names_its_image_and_asks_for_no_answer() {
        let deletes = |ids: &[u32]| KittyDeletes {
            ids: ids.to_vec(),
            tmux: false,
        };

        assert_eq!(
            deletes(&[7, 4_000_000_000]).to_bytes(),
            b"\x1b_Ga=d,d=I,i=7,q=2\x1b\\\x1b_Ga=d,d=I,i=4000000000,q=2\x1b\\"
        );
        assert_eq!(deletes(&[]).to_bytes(), b"");
        assert!(deletes(&[]).is_empty());
    }

    /// Inside tmux, each delete is wrapped for passthrough as the
    /// transmissions are: the escapes inside doubled. The book takes the
    /// wrapping from the graphics the transmissions use.
    #[test]
    fn a_delete_inside_tmux_goes_through_the_passthrough() {
        let book = counting(Graphics {
            tmux_wrapped: true,
            ..Graphics::KITTY
        });
        drop(HeldId::issued(&book));

        assert_eq!(
            book.after_frame().to_bytes(),
            b"\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=1,q=2\x1b\x1b\\\x1b\\"
        );
    }
}
