// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Images in the terminal (spec section 8.8). The binary asks the terminal
//! once at start which graphics protocol it speaks and keeps what it
//! learnt as a [`Graphics`]; [`Images`] makes the [`Image`]s of context
//! entries for it and keeps the book of the run's Kitty image ids. Only
//! this module and the binary's start see ratatui-image, so the screen and
//! its tests work with asqr's own values and never talk to a terminal or
//! to tmux.

mod detect;
mod image;
mod kitty;
mod placeholders;
#[cfg(test)]
pub(crate) mod testing;

use std::io::Cursor;
use std::path::Path;

use ::image::error::{ImageFormatHint, UnsupportedError, UnsupportedErrorKind};
use ::image::{ImageError, ImageFormat};
use ratatui_image::picker::{Capability, Picker, ProtocolType};

#[cfg(unix)]
pub use detect::discard_late_answers;
pub use detect::{
    LATE_ANSWER_CAP, LATE_ANSWER_QUIET, PanePassthrough, QUERY_WAIT, answers_may_follow_the_query,
    may_query_protocol, passthrough_all_for_images, settle_passthrough,
};
pub use image::Image;
use image::Read;
use kitty::{IdSource, random_ids};
pub use kitty::{KittyBook, KittyDeletes, KittyExit};
pub use placeholders::{PlacedImage, disarm_placeholders};

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
/// `allow-passthrough` says (spec section 8.9).
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
    /// Kitty deletes as the transmissions. The wrapping follows what
    /// ratatui-image sees: it wraps where the terminal names tmux, also
    /// where asqr runs in no tmux pane it could ask, as over ssh.
    pub tmux_wrapped: bool,
    /// What tmux passes from asqr's pane for the run, settled once the
    /// pane's setting is (see [`settle_passthrough`]). It follows what
    /// tmux does with the pane, whatever the wrapping.
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

    /// Whether the protocol marks the cells an image covers as skipped, so
    /// ratatui never writes them again, also not where an overlay's text
    /// stood over the image. The Sixel and iTerm2 protocols of
    /// ratatui-image write the picture where the cursor is and do; half
    /// blocks and Kitty placeholders are written into every cell (ADR 31).
    pub fn cells_are_skipped(self) -> bool {
        matches!(self.protocol, Protocol::Sixel | Protocol::Iterm2)
    }

    /// Whether images must be sent again when asqr regains the focus: the
    /// Kitty protocol sends an image's data once and afterwards only
    /// refers to it, and tmux with `allow-passthrough on` drops that
    /// transfer while asqr's pane is hidden (ADR 24).
    pub fn resends_on_focus(self) -> bool {
        self.protocol == Protocol::Kitty && self.passthrough == Passthrough::WhileVisible
    }
}

/// Makes the [`Image`]s of context entries for the terminal's graphics.
/// Its clones share one book of the Kitty image ids handed out in this
/// run, so it is made once, at start.
#[derive(Clone)]
pub struct Images {
    graphics: Graphics,
    book: KittyBook,
}

impl Images {
    /// Images for `graphics`, under Kitty ids drawn at random.
    pub fn new(graphics: Graphics) -> Self {
        Self::with_source(graphics, random_ids())
    }

    fn with_source(graphics: Graphics, source: IdSource) -> Self {
        Images {
            graphics,
            book: KittyBook::new(graphics, source),
        }
    }

    /// The graphics these images are made for.
    pub fn graphics(&self) -> Graphics {
        self.graphics
    }

    /// Decodes `bytes`, the image file at `path`. The caller reads them,
    /// by the rule that decides which paths may be opened at all and how
    /// large an image file may be.
    ///
    /// The format comes from the file's first bytes, else from the
    /// extension of `path`, as `ImageReader::open` finds it: TGA, for
    /// one, has no signature to recognise. An extension no format knows
    /// fails with the error that names it.
    pub fn decode(&self, bytes: Vec<u8>, path: &str) -> Result<Image, ImageError> {
        let mut reader = ::image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        if reader.format().is_none()
            && let Some(extension) = Path::new(path).extension().filter(|ext| !ext.is_empty())
        {
            match ImageFormat::from_extension(extension) {
                Some(format) => reader.set_format(format),
                None => {
                    let hint = ImageFormatHint::PathExtension(extension.into());
                    return Err(ImageError::Unsupported(
                        UnsupportedError::from_format_and_kind(
                            hint.clone(),
                            UnsupportedErrorKind::Format(hint),
                        ),
                    ));
                }
            }
        }
        // Decoding needs a format, so a picture that decodes had one.
        let format = reader.format();
        let picture = reader.decode()?;
        let format = format.expect("a decoded picture had a format");
        let read = Read {
            bytes,
            format,
            pixels: (picture.width(), picture.height()),
        };
        Ok(Image::new(self, read, picture))
    }

    /// Makes an image of `picture`, which a test drew, as if it were read
    /// from a file. It is encoded when it is laid out, and kept as PNG in
    /// 8-bit RGBA for the encodings after the first.
    #[cfg(test)]
    pub(crate) fn image(&self, picture: ::image::DynamicImage) -> Image {
        let mut bytes = Vec::new();
        ::image::DynamicImage::ImageRgba8(picture.to_rgba8())
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("PNG takes 8-bit RGBA, written into memory");
        let read = Read {
            bytes,
            format: ImageFormat::Png,
            pixels: (picture.width(), picture.height()),
        };
        Image::new(self, read, picture)
    }

    /// The book of the Kitty ids these images take. A context image hands
    /// its id on for deleting when its viewer goes: on regaining the
    /// focus, when its session leaves or when its entry is replaced, and
    /// when it is laid out at another size (ADR 42).
    pub fn kitty_book(&self) -> &KittyBook {
        &self.book
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROTOCOLS: [Protocol; 4] = [
        Protocol::HalfBlocks,
        Protocol::Sixel,
        Protocol::Kitty,
        Protocol::Iterm2,
    ];

    #[test]
    fn only_sixel_and_iterm2_leave_skipped_cells() {
        for protocol in PROTOCOLS {
            let graphics = Graphics {
                protocol,
                ..Graphics::HALF_BLOCKS
            };
            assert_eq!(
                graphics.cells_are_skipped(),
                matches!(protocol, Protocol::Sixel | Protocol::Iterm2),
                "{protocol:?}"
            );
        }
    }

    #[test]
    fn only_kitty_images_behind_passthrough_on_are_sent_again_on_focus() {
        for protocol in PROTOCOLS {
            for passthrough in [
                Passthrough::None,
                Passthrough::WhileVisible,
                Passthrough::Always,
            ] {
                let graphics = Graphics {
                    protocol,
                    passthrough,
                    ..Graphics::HALF_BLOCKS
                };
                assert_eq!(
                    graphics.resends_on_focus(),
                    protocol == Protocol::Kitty && passthrough == Passthrough::WhileVisible,
                    "{protocol:?} {passthrough:?}"
                );
            }
        }
    }
}
