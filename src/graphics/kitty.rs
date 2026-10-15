// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Kitty image ids of a run and the commands that delete their images
//! (ADR 42). The terminal keeps an image sent under the Kitty protocol
//! until it is deleted, so asqr hands out every id itself and notes when
//! the image that holds it goes.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::hash::{BuildHasher, RandomState};
use std::rc::Rc;

use crate::terminal_env::passthrough;

/// Where Kitty ids come from: 32 random bits per call. `RandomState` is
/// seeded from the operating system once per thread, so two asqr
/// processes, which may share one terminal through tmux, draw different
/// ids.
pub(super) fn random_ids() -> Box<dyn FnMut() -> u32> {
    let random = RandomState::new();
    let mut drawn: u64 = 0;
    Box::new(move || {
        drawn += 1;
        let hash = random.hash_one(drawn);
        u32::try_from(hash >> 32).expect("32 bits are left of 64 shifted by 32")
    })
}

/// The book of the Kitty image ids of one run.
pub(super) struct KittyIds {
    source: Box<dyn FnMut() -> u32>,
    /// Every id handed out in this run. An id is never handed out twice:
    /// a delete for an id written late, or written again, would remove
    /// the image that took it over.
    issued: HashSet<u32>,
    /// The ids of the images that exist.
    held: BTreeSet<u32>,
    /// The ids of the images that went since [`KittyIds::take_dropped`]
    /// last took them.
    dropped: Vec<u32>,
}

impl KittyIds {
    pub(super) fn new(source: Box<dyn FnMut() -> u32>) -> Self {
        KittyIds {
            source,
            issued: HashSet::new(),
            held: BTreeSet::new(),
            dropped: Vec::new(),
        }
    }

    /// A new id, drawn again while it is 0, which names no image in the
    /// Kitty protocol, or one handed out before.
    fn issue(&mut self) -> u32 {
        loop {
            let id = (self.source)();
            if id != 0 && self.issued.insert(id) {
                self.held.insert(id);
                return id;
            }
        }
    }

    /// Notes that the image with `id` went, for the terminal to delete.
    /// Every way an image goes ends here, through the drop of its
    /// [`HeldId`].
    fn release(&mut self, id: u32) {
        if self.held.remove(&id) {
            self.dropped.push(id);
        }
    }

    /// The ids of the images that went since the last call.
    pub(super) fn take_dropped(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.dropped)
    }

    /// The ids of every image the terminal may still keep, each once:
    /// those that went since [`KittyIds::take_dropped`] last took them,
    /// then those that exist.
    pub(super) fn take_all(&mut self) -> Vec<u32> {
        let mut all = std::mem::take(&mut self.dropped);
        all.extend(std::mem::take(&mut self.held));
        all
    }

    /// The ids of the images that exist.
    #[cfg(test)]
    pub(super) fn held(&self) -> Vec<u32> {
        self.held.iter().copied().collect()
    }
}

/// The Kitty id of one image, handed back to its book when the image
/// goes.
pub(super) struct HeldId {
    pub(super) id: u32,
    book: Rc<RefCell<KittyIds>>,
}

impl HeldId {
    pub(super) fn issued(book: &Rc<RefCell<KittyIds>>) -> Self {
        HeldId {
            id: book.borrow_mut().issue(),
            book: Rc::clone(book),
        }
    }

    /// Gives the id back without a delete, for an image that never went
    /// out to the terminal. The drop that follows finds nothing to release.
    pub(super) fn forget(self) {
        self.book.borrow_mut().held.remove(&self.id);
    }
}

impl Drop for HeldId {
    fn drop(&mut self) {
        self.book.borrow_mut().release(self.id);
    }
}

/// The Kitty commands that delete the images `ids` names, each wrapped
/// for tmux's passthrough when `tmux` says the transmissions are, as
/// ratatui-image wraps them (ADR 42).
///
/// `d=I` with an id and no placement id removes every placement of the
/// image, the virtual one its placeholders refer to included, and frees
/// its data. `q=2` keeps the terminal from answering into asqr's input. A
/// delete of an id the terminal does not know does nothing.
pub fn kitty_deletes(ids: &[u32], tmux: bool) -> Vec<u8> {
    ids.iter()
        .flat_map(|id| {
            let delete = format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\").into_bytes();
            if tmux { passthrough(&delete) } else { delete }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kitty_id_is_never_0_and_never_handed_out_twice() {
        let mut drawn = vec![0, 7, 7, 9, 0, 7, 11].into_iter();
        let mut ids = KittyIds::new(Box::new(move || drawn.next().expect("enough ids")));

        assert_eq!([ids.issue(), ids.issue(), ids.issue()], [7, 9, 11]);
    }

    #[test]
    fn random_kitty_ids_differ() {
        let mut random = random_ids();
        let drawn: HashSet<u32> = (0..1000).map(|_| random()).collect();

        assert!(drawn.len() > 990, "{} of 1000 differ", drawn.len());
    }

    #[test]
    fn a_delete_names_its_image_and_asks_for_no_answer() {
        assert_eq!(
            kitty_deletes(&[7, 4_000_000_000], false),
            b"\x1b_Ga=d,d=I,i=7,q=2\x1b\\\x1b_Ga=d,d=I,i=4000000000,q=2\x1b\\"
        );
        assert_eq!(kitty_deletes(&[], false), b"");
    }

    /// Inside tmux, each delete is wrapped for passthrough as the
    /// transmissions are: the escapes inside doubled.
    #[test]
    fn a_delete_inside_tmux_goes_through_the_passthrough() {
        assert_eq!(
            kitty_deletes(&[7], true),
            b"\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=7,q=2\x1b\x1b\\\x1b\\"
        );
    }
}
