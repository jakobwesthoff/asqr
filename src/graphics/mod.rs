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
use std::time::{Duration, Instant};

use ::image::ImageError;
use ratatui_image::picker::{Capability, Picker, ProtocolType};

use crate::image_format::reader_with_format;

#[cfg(unix)]
pub use detect::discard_late_answers;
pub use detect::{
    LATE_ANSWER_CAP, LATE_ANSWER_QUIET, PanePassthrough, QUERY_WAIT, answers_may_follow_the_query,
    focus_reports_off, may_query_protocol, passthrough_all_for_images, settle_passthrough,
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
    /// `all`: tmux passes sequences from a hidden pane too, to every client
    /// whose session holds the pane's window. With no such client attached,
    /// as while the client shows another session or after a detach, it
    /// passes nothing (ADR 110).
    Always,
}

/// When a focus gain sends the Kitty images again and repeats the deletes,
/// as the passthrough may have dropped them while asqr was out of view
/// (ADRs 24, 42 and 110).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusRule {
    /// Nothing is lost: no tmux in between, or images that are drawn into
    /// the cells with every frame or written where the cursor is.
    Never,
    /// tmux with `on` drops whatever a hidden pane writes, so every focus
    /// gain sends again.
    EveryGain,
    /// tmux with `all` drops only what goes out while no client shows the
    /// pane's session, so a focus gain sends again only what a
    /// [`FocusWatch`] counts as possibly lost.
    AfterHiddenWrites,
}

/// How long after a frame that wrote Kitty commands a focus change still
/// counts that frame as possibly lost. tmux decides whether to pass a
/// command when it reads it from the pane, which can be after asqr's write
/// returned; tmux 3.8 passed 4 MB in 4096-byte chunks 25 ms after the
/// write returned (ADR 110). The rest is room for a loop that handles the
/// focus event late.
pub const FOCUS_GRACE: Duration = Duration::from_secs(1);

/// Which kinds of Kitty command went out: image transmissions, deletes or
/// both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Writes {
    pub sent: bool,
    pub deleted: bool,
}

impl Writes {
    fn any(self) -> bool {
        self.sent || self.deleted
    }

    fn add(&mut self, other: Writes) {
        self.sent |= other.sent;
        self.deleted |= other.deleted;
    }
}

/// The last frame that wrote Kitty commands.
#[derive(Debug, Clone, Copy)]
struct KittyFrame {
    /// When its writes, the deletes after it included, were done.
    at: Instant,
    wrote: Writes,
    /// Whether asqr had no focus while it was written.
    unfocused: bool,
}

/// Follows the focus reports and the frames that wrote Kitty commands,
/// under the [`FocusRule::AfterHiddenWrites`] of tmux's `all`, and tells
/// on a focus gain which of those writes may have been lost (ADR 110).
///
/// tmux reports a change of the pane's focus only, and nothing when asqr
/// turns the reports on, so the watch starts as having the focus.
#[derive(Debug)]
pub struct FocusWatch {
    focused: bool,
    last_frame: Option<KittyFrame>,
    /// What went out since the last focus gain, for a gain that follows no
    /// loss: asqr missed that loss and cannot tell when it was.
    since_gain: Writes,
    /// What went out since the last focus gain and may have been lost.
    hidden: Writes,
}

impl Default for FocusWatch {
    fn default() -> Self {
        FocusWatch {
            focused: true,
            last_frame: None,
            since_gain: Writes::default(),
            hidden: Writes::default(),
        }
    }
}

impl FocusWatch {
    /// A frame wrote `wrote`, done at `at`. While asqr has no focus, it
    /// asks `shown` whether a tmux client shows the pane's session, unless
    /// what the frame wrote counts as possibly lost already: what went out
    /// while no client showed it never reached the terminal. A focus loss
    /// alone does not mean that, as the terminal may only have lost the
    /// focus to another app, or the client may show another window of the
    /// same session, and tmux passed the frame then.
    pub fn frame_written(&mut self, wrote: Writes, at: Instant, shown: impl FnOnce() -> bool) {
        if !wrote.any() {
            return;
        }
        self.since_gain.add(wrote);
        self.last_frame = Some(KittyFrame {
            at,
            wrote,
            unfocused: !self.focused,
        });
        let unmarked = (wrote.sent && !self.hidden.sent) || (wrote.deleted && !self.hidden.deleted);
        if !self.focused && unmarked && !shown() {
            self.hidden.add(wrote);
        }
    }

    /// asqr lost the focus; the report arrived at `arrived`. A frame
    /// finished less than [`FOCUS_GRACE`] before, or after it arrived, may
    /// still have been in the pane's output when tmux hid it.
    pub fn focus_lost(&mut self, arrived: Instant) {
        self.focused = false;
        if let Some(frame) = self.last_frame
            && frame.at + FOCUS_GRACE > arrived
        {
            self.hidden.add(frame.wrote);
        }
    }

    /// asqr gained the focus; the report arrived at `arrived`. Gives what
    /// may have been lost since the last gain, and starts over.
    ///
    /// After a loss, that is what was marked, and a frame written without
    /// the focus less than [`FOCUS_GRACE`] before: the client may have come
    /// back between its write and the question whether a client shows the
    /// session. A gain that follows no loss means asqr missed the loss, as
    /// from a client killed, or while the person's editor had the terminal
    /// and its focus reports were off; everything since the last gain
    /// counts then.
    pub fn focus_gained(&mut self, arrived: Instant) -> Writes {
        let mut lost = if self.focused {
            self.since_gain
        } else {
            self.hidden
        };
        if !self.focused
            && let Some(frame) = self.last_frame
            && frame.unfocused
            && frame.at + FOCUS_GRACE > arrived
        {
            lost.add(frame.wrote);
        }
        self.focused = true;
        self.since_gain = Writes::default();
        self.hidden = Writes::default();
        lost
    }
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

    /// When images are sent again as asqr regains the focus: the Kitty
    /// protocol sends an image's data once and afterwards only refers to
    /// it, and tmux drops that transfer from a pane no client shows, with
    /// `on` while the pane is hidden (ADR 24), with `all` while no client
    /// shows its session (ADR 110).
    pub fn focus_rule(self) -> FocusRule {
        match (self.protocol, self.passthrough) {
            (Protocol::Kitty, Passthrough::WhileVisible) => FocusRule::EveryGain,
            (Protocol::Kitty, Passthrough::Always) => FocusRule::AfterHiddenWrites,
            _ => FocusRule::Never,
        }
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
    /// [`reader_with_format`] decides the format from the bytes and `path`.
    pub fn decode(&self, bytes: Vec<u8>, path: &str) -> Result<Image, ImageError> {
        let reader = reader_with_format(Cursor::new(&bytes), Path::new(path))?;
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
            .write_to(&mut Cursor::new(&mut bytes), ::image::ImageFormat::Png)
            .expect("PNG takes 8-bit RGBA, written into memory");
        let read = Read {
            bytes,
            format: ::image::ImageFormat::Png,
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

    /// Only Kitty transfers can be lost to tmux: with `on` on every focus
    /// gain, with `all` after writes no client may have seen.
    #[test]
    fn only_kitty_images_behind_tmux_are_sent_again_on_focus() {
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
                let expected = match (protocol, passthrough) {
                    (Protocol::Kitty, Passthrough::WhileVisible) => FocusRule::EveryGain,
                    (Protocol::Kitty, Passthrough::Always) => FocusRule::AfterHiddenWrites,
                    _ => FocusRule::Never,
                };
                assert_eq!(
                    graphics.focus_rule(),
                    expected,
                    "{protocol:?} {passthrough:?}"
                );
            }
        }
    }

    // -----------------------------------------------------------------
    // The focus watch (ADR 110)
    // -----------------------------------------------------------------

    const SENT: Writes = Writes {
        sent: true,
        deleted: false,
    };
    const DELETED: Writes = Writes {
        sent: false,
        deleted: true,
    };
    const NOTHING: Writes = Writes {
        sent: false,
        deleted: false,
    };

    /// A time and the time `millis` milliseconds after it.
    fn later(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    /// An answer to whether a client shows the session that must not be
    /// asked for.
    fn never_asked() -> bool {
        panic!("asked whether a client shows the session")
    }

    #[test]
    fn a_frame_written_with_the_focus_asks_nothing_and_loses_nothing() {
        let start = Instant::now();
        let mut watch = FocusWatch::default();

        watch.frame_written(SENT, start, never_asked);
        watch.focus_lost(later(start, 2000));

        assert_eq!(watch.focus_gained(later(start, 3000)), NOTHING);
    }

    /// A frame that wrote no Kitty command has nothing to lose, so tmux is
    /// not asked about it, focus or not.
    #[test]
    fn a_frame_without_kitty_commands_asks_nothing() {
        let start = Instant::now();
        let mut watch = FocusWatch::default();
        watch.focus_lost(start);

        watch.frame_written(NOTHING, later(start, 2000), never_asked);

        assert_eq!(watch.focus_gained(later(start, 4000)), NOTHING);
    }

    /// Written without the focus while no client showed the session: the
    /// gain gives it once.
    #[test]
    fn a_frame_written_while_no_client_shows_the_session_is_lost() {
        let start = Instant::now();
        let mut watch = FocusWatch::default();
        watch.focus_lost(start);

        watch.frame_written(SENT, later(start, 2000), || false);

        assert_eq!(watch.focus_gained(later(start, 4000)), SENT);
        assert_eq!(watch.focus_gained(later(start, 5000)), NOTHING, "once");
    }

    /// Without the focus but with a client on the session, as in another
    /// app or another window of the session, tmux passed the frame.
    #[test]
    fn a_frame_written_while_a_client_shows_the_session_is_kept() {
        let start = Instant::now();
        let mut watch = FocusWatch::default();
        watch.focus_lost(start);

        watch.frame_written(DELETED, later(start, 2000), || true);

        assert_eq!(watch.focus_gained(later(start, 4000)), NOTHING);
    }

    /// Once a kind of write counts as lost, later frames of that kind
    /// need no answer from tmux; another kind still does.
    #[test]
    fn tmux_is_asked_once_per_kind_until_the_gain() {
        let start = Instant::now();
        let mut watch = FocusWatch::default();
        watch.focus_lost(start);
        let mut asked = 0;

        for at in [2000, 3000] {
            watch.frame_written(SENT, later(start, at), || {
                asked += 1;
                false
            });
        }
        watch.frame_written(DELETED, later(start, 4000), || {
            asked += 1;
            true
        });

        assert_eq!(asked, 2);
        assert_eq!(watch.focus_gained(later(start, 6000)), SENT);
    }

    /// A loss that arrives within the grace after a frame, or before a
    /// frame finished, counts that frame: tmux may have hidden the pane
    /// before it read the frame's last bytes.
    #[test]
    fn a_loss_close_after_a_frame_counts_the_frame() {
        let start = Instant::now();
        for (frame, loss, lost) in [(0, 500, SENT), (0, 2000, NOTHING), (1000, 500, SENT)] {
            let mut watch = FocusWatch::default();
            watch.frame_written(SENT, later(start, frame), never_asked);
            watch.focus_lost(later(start, loss));

            assert_eq!(
                watch.focus_gained(later(start, 5000)),
                lost,
                "frame at {frame}, loss at {loss}"
            );
        }
    }

    /// A gain close after a frame written without the focus counts it,
    /// whatever tmux said: the client may have come back between the
    /// write and the question.
    #[test]
    fn a_gain_close_after_a_frame_written_without_the_focus_counts_it() {
        let start = Instant::now();
        for (gain, lost) in [(2500, DELETED), (4000, NOTHING)] {
            let mut watch = FocusWatch::default();
            watch.focus_lost(start);
            watch.frame_written(DELETED, later(start, 2000), || true);

            assert_eq!(
                watch.focus_gained(later(start, gain)),
                lost,
                "gain at {gain}"
            );
        }
    }

    /// A gain with no loss before it means a loss went unreported, at any
    /// time since the last gain; everything written since counts. That
    /// holds for the first gain after the start too.
    #[test]
    fn a_gain_without_a_loss_counts_everything_since_the_last_gain() {
        let start = Instant::now();
        let mut watch = FocusWatch::default();
        watch.frame_written(SENT, start, never_asked);
        watch.frame_written(DELETED, later(start, 100), never_asked);

        assert_eq!(
            watch.focus_gained(later(start, 5000)),
            Writes {
                sent: true,
                deleted: true
            }
        );
        assert_eq!(watch.focus_gained(later(start, 6000)), NOTHING, "once");
    }
}
