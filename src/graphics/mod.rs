// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Images in the terminal (spec section 7.7). The binary asks the terminal
//! once at start which graphics protocol it speaks and keeps what it
//! learnt as a [`Graphics`]; [`Images`] makes the [`Image`]s of context
//! entries for it and keeps the book of the run's Kitty image ids. Only
//! this module and the binary's start see ratatui-image, so the screen and
//! its tests work with asqr's own values and never talk to a terminal or
//! to tmux.

mod detect;
mod image;
mod kitty;
#[cfg(test)]
pub(crate) mod testing;

use std::cell::RefCell;
use std::rc::Rc;

use ::image::{DynamicImage, ImageError};
use ratatui_image::picker::{Capability, Picker, ProtocolType};

pub use detect::{
    PanePassthrough, answers_may_follow_the_query, may_query_protocol, passthrough_all_for_images,
    settle_passthrough,
};
#[cfg(unix)]
pub use detect::{discard_late_answers, passthrough_answers};
pub use image::Image;
pub use kitty::kitty_deletes;
use kitty::{KittyIds, random_ids};

/// The graphics protocol images are sent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Unicode half blocks in two colours, which every terminal shows.
    HalfBlocks,
    Sixel,
    Kitty,
    Iterm2,
}

/// What tmux passes on to the terminal outside from asqr's pane, as its
/// `allow-passthrough` says (spec section 7.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passthrough {
    /// No tmux stands between asqr and the terminal, or it passes nothing.
    None,
    /// `on`: tmux passes sequences only while a client shows the pane, and
    /// drops what is sent while it is hidden.
    WhileVisible,
    /// `all`: tmux passes sequences from a hidden pane too.
    Always,
}

/// The size of a terminal cell in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSize {
    pub width: u16,
    pub height: u16,
}

/// What asqr learnt at start about showing images in its terminal. It
/// holds for the whole run: images are sized with this cell size also
/// after the font changes, so a report of a cropped or shrunken image
/// starts by comparing it with the cell size the terminal reports on a
/// resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Graphics {
    pub protocol: Protocol,
    pub cell: CellSize,
    /// Whether every command goes out wrapped for tmux's passthrough, the
    /// Kitty deletes as the transmissions. ratatui-image wraps where the
    /// terminal names tmux.
    pub tmux_wrapped: bool,
    /// What tmux passes from asqr's pane for the run, settled once the
    /// pane's setting is (see [`settle_passthrough`]).
    pub passthrough: Passthrough,
    /// Whether the terminal said it inflates Kitty transmissions sent with
    /// `o=z`. A terminal that cannot refuses a compressed transmission, so
    /// asqr compresses only where it said so.
    pub kitty_compression: bool,
}

impl Graphics {
    /// Half blocks, for a terminal that was not asked or did not answer.
    /// The cell size only sets the proportions of a half block, so it is
    /// ratatui-image's default for them.
    pub const HALF_BLOCKS: Graphics = Graphics {
        protocol: Protocol::HalfBlocks,
        cell: CellSize {
            width: 10,
            height: 20,
        },
        tmux_wrapped: false,
        passthrough: Passthrough::None,
        kitty_compression: false,
    };

    /// What `picker` found out from the terminal's answer to its query,
    /// with no passthrough yet: it is settled after the query.
    pub fn from_picker(picker: &Picker) -> Graphics {
        let font = picker.font_size();
        Graphics {
            protocol: match picker.protocol_type() {
                ProtocolType::Halfblocks => Protocol::HalfBlocks,
                ProtocolType::Sixel => Protocol::Sixel,
                ProtocolType::Kitty => Protocol::Kitty,
                ProtocolType::Iterm2 => Protocol::Iterm2,
            },
            cell: CellSize {
                width: font.width,
                height: font.height,
            },
            tmux_wrapped: picker.tmux_detected(),
            passthrough: Passthrough::None,
            kitty_compression: picker
                .capabilities()
                .contains(&Capability::KittyCompression),
        }
    }
}

/// Makes the [`Image`]s of context entries for the terminal's graphics.
/// Its clones share one book of the Kitty image ids handed out in this
/// run, so it is made once, at start.
#[derive(Clone)]
pub struct Images {
    graphics: Graphics,
    book: Rc<RefCell<KittyIds>>,
}

impl Images {
    /// Images for `graphics`, under Kitty ids drawn at random.
    pub fn new(graphics: Graphics) -> Self {
        Self::with_source(graphics, random_ids())
    }

    fn with_source(graphics: Graphics, source: Box<dyn FnMut() -> u32>) -> Self {
        Images {
            graphics,
            book: Rc::new(RefCell::new(KittyIds::new(source))),
        }
    }

    /// The graphics these images are made for.
    pub fn graphics(&self) -> Graphics {
        self.graphics
    }

    /// Reads and decodes the image at `path`. The caller checks first
    /// that the path may be opened at all.
    pub fn load(&self, path: &str) -> Result<Image, ImageError> {
        let decoded = ::image::ImageReader::open(path)?
            .with_guessed_format()?
            .decode()?;
        Ok(self.encode(decoded))
    }

    /// Makes an image of `picture`, which is decoded already.
    pub fn encode(&self, picture: DynamicImage) -> Image {
        Image::new(self, picture)
    }

    /// The Kitty ids of the images that went since the last call, for the
    /// terminal to delete: a context image drops its id when its viewer
    /// goes, on regaining the focus, when its session leaves or when its
    /// entry is replaced (ADR 42).
    pub fn take_dropped(&self) -> Vec<u32> {
        self.book.borrow_mut().take_dropped()
    }

    /// The Kitty ids of every image the terminal may still keep, each
    /// once: those that went since [`Images::take_dropped`] last took
    /// them, then those that exist. asqr deletes them all before it leaves
    /// the alternate screen, as the images stay there after it (ADR 42).
    pub fn take_all(&self) -> Vec<u32> {
        self.book.borrow_mut().take_all()
    }
}
