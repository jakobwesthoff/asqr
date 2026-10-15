// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One image of a context entry, encoded for the terminal's graphics
//! protocol. A picture that cannot be decoded never becomes an [`Image`];
//! one that cannot be encoded draws nothing.

use ::image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;
use ratatui_image::errors::Errors;
use ratatui_image::protocol::halfblocks::Halfblocks;
use ratatui_image::protocol::iterm2::Iterm2;
use ratatui_image::protocol::kitty::StatefulKitty;
use ratatui_image::protocol::sixel::Sixel;
use ratatui_image::protocol::{StatefulProtocol, StatefulProtocolType};
use ratatui_image::{FilterType, FontSize, Resize, ResizeEncodeRender, StatefulImage};

use super::kitty::HeldId;
use super::{Graphics, Images, Protocol};

/// One image file, decoded and encoded for the terminal's graphics
/// protocol.
pub struct Image {
    protocol: Box<StatefulProtocol>,
    /// What the Kitty protocol needs besides the protocol, `None` under
    /// any other protocol.
    kitty: Option<KittyImage>,
    /// Whether an encoding failed. ratatui-image would try again on every
    /// frame, and log the error each time; the image draws nothing
    /// instead.
    failed: bool,
}

/// The filter that scales a picture to its cells: each pixel of the
/// result mixes the pixels it covers, where the nearest pixel alone would
/// drop thin lines and text.
const SCALING: FilterType = FilterType::Triangle;

/// A Kitty image's id and what encodes it again under a new one.
struct KittyImage {
    /// The id the protocol sends the image under. Its drop hands the id
    /// on for deleting, so the image's holder decides when the image goes,
    /// and with it the id.
    held: HeldId,
    /// The decoded picture, a copy of the one the protocol keeps, which
    /// ratatui-image does not hand out.
    picture: DynamicImage,
    images: Images,
    /// Whether the protocol has encoded the picture for an area yet. Its
    /// first encoding goes out under the id the image got.
    encoded: bool,
}

impl Image {
    /// `picture` for the graphics `images` makes images for, under a Kitty
    /// id from their book where the protocol is Kitty's.
    pub(super) fn new(images: &Images, picture: DynamicImage) -> Image {
        if images.graphics.protocol != Protocol::Kitty {
            return Image {
                protocol: Box::new(stateful_protocol(images.graphics, picture, None)),
                kitty: None,
                failed: false,
            };
        }
        let held = HeldId::issued(&images.book);
        let protocol = stateful_protocol(images.graphics, picture.clone(), Some(&held));
        Image {
            protocol: Box::new(protocol),
            kitty: Some(KittyImage {
                held,
                picture,
                images: images.clone(),
                encoded: false,
            }),
            failed: false,
        }
    }

    /// Draws the image into `area` of `buf`. With `enlarge`, the image is
    /// scaled to fill the area as far as its proportions allow; without, it
    /// keeps its natural size and only shrinks when it does not fit.
    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, enlarge: bool) {
        if self.failed {
            return;
        }
        let resize = if enlarge {
            Resize::Scale(Some(SCALING))
        } else {
            Resize::Fit(Some(SCALING))
        };
        // ratatui-image encodes the picture again when its area changes
        // size, under the same Kitty id, and Ghostty deletes an image when
        // a new transmission under its id begins, blank until it ends.
        // Under a new id the old copy stays untouched while the new one
        // comes in, and the old id goes for deleting after the frame
        // (ADR 42).
        if let Some(kitty) = &mut self.kitty
            && !area.is_empty()
        {
            if kitty.encoded
                && self
                    .protocol
                    .needs_resize(&resize, area.as_size())
                    .is_some()
            {
                let held = HeldId::issued(&kitty.images.book);
                *self.protocol =
                    stateful_protocol(kitty.images.graphics, kitty.picture.clone(), Some(&held));
                kitty.held = held;
            }
            kitty.encoded = true;
        }
        StatefulImage::default()
            .resize(resize)
            .render(area, buf, self.protocol.as_mut());
        let encoded = self.protocol.last_encoding_result();
        self.after_encoding(encoded);
    }

    /// Handles the result of the encoding a draw did, if it did one: a
    /// failure is logged, and the image draws nothing from then on. A
    /// Kitty image whose encoding failed never went out, so its id goes
    /// back without a delete.
    fn after_encoding(&mut self, result: Option<Result<(), Errors>>) {
        let Some(Err(error)) = result else {
            return;
        };
        tracing::warn!(%error, "could not encode an image; it stays empty");
        self.failed = true;
        if let Some(kitty) = self.kitty.take() {
            kitty.held.forget();
        }
    }
}

/// The protocol that encodes `picture` for `graphics`, and for the Kitty
/// protocol under the id `held`.
///
/// It passes what `Picker::new_resize_protocol` passes (ratatui-image
/// 12.0.0-rc.2 `picker.rs` l.301-320) but for the Kitty id and shared
/// memory: the tmux wrapping, and compression only where the terminal
/// said it inflates `o=z`. Shared memory stays off. The picker only
/// reports it when the query asked for it, it works only with a terminal
/// on the same machine, and every transmission leaves an object behind
/// for the terminal to unlink. Without a background colour, pictures are
/// padded with transparent pixels.
fn stateful_protocol(
    graphics: Graphics,
    picture: DynamicImage,
    held: Option<&HeldId>,
) -> StatefulProtocol {
    let tmux = graphics.tmux_wrapped;
    let protocol = match graphics.protocol {
        Protocol::HalfBlocks => StatefulProtocolType::Halfblocks(Halfblocks::default()),
        Protocol::Sixel => StatefulProtocolType::Sixel(Sixel {
            is_tmux: tmux,
            ..Sixel::default()
        }),
        Protocol::Iterm2 => StatefulProtocolType::ITerm2(Iterm2 {
            is_tmux: tmux,
            ..Iterm2::default()
        }),
        Protocol::Kitty => StatefulProtocolType::Kitty(StatefulKitty::new(
            held.expect("a Kitty image holds an id").id,
            tmux,
            graphics.kitty_compression,
            false,
        )),
    };
    let font = FontSize::new(graphics.cell.width, graphics.cell.height);
    StatefulProtocol::new(picture, font, None, protocol)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ratatui_image::picker::{Picker, ProtocolType};

    use super::super::testing::{self, transmission};
    use super::*;
    use crate::terminal_env::TerminalEnv;

    /// Why the file at `path` does not load, or `None` when it does.
    fn refused(path: &str) -> Option<String> {
        Images::half_blocks()
            .load(path)
            .err()
            .map(|error| error.to_string())
    }

    #[cfg(unix)]
    #[test]
    fn a_regular_file_that_may_not_be_read_says_why() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("secret.png");
        fs::write(&path, b"").expect("file is writable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))
            .expect("permissions are settable");
        let path = path.to_str().expect("a UTF-8 path");

        let reason = refused(path).expect("refused");

        assert!(
            reason.to_lowercase().contains("permission denied"),
            "{reason}"
        );
    }

    #[test]
    fn a_file_that_is_no_image_says_why() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("notes.png");
        fs::write(&path, b"no picture in here").expect("file is writable");

        let reason = refused(path.to_str().expect("a UTF-8 path")).expect("refused");

        assert!(!reason.is_empty());
    }

    #[test]
    fn a_regular_image_file_loads() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("dot.png");
        ::image::RgbImage::from_pixel(2, 2, ::image::Rgb([255, 0, 0]))
            .save(&path)
            .expect("image written");

        assert_eq!(refused(path.to_str().expect("a UTF-8 path")), None);
    }

    /// A shrunk image averages its pixels. Columns of one pixel in black
    /// and white, halved, come out grey; picking every other pixel would
    /// leave only black or only white.
    #[test]
    fn a_shrunk_image_mixes_the_pixels_it_shrinks() {
        let stripes = ::image::RgbaImage::from_fn(400, 40, |x, _| {
            let shade = if x % 2 == 0 { 0 } else { 255 };
            ::image::Rgba([shade, shade, shade, 255])
        });
        let mut image = Images::half_blocks().encode(stripes.into());
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));

        image.draw(buf.area, &mut buf, false);

        let greys: Vec<_> = buf
            .content()
            .iter()
            .flat_map(|cell| [cell.fg, cell.bg])
            .filter(|colour| matches!(colour, ratatui::style::Color::Rgb(64..=192, _, _)))
            .collect();
        assert!(!greys.is_empty(), "{:?}", buf.content()[0]);
    }

    /// An encoding that fails is logged once, and the image draws nothing
    /// from then on instead of encoding again on every frame. A Kitty
    /// image that never went out gives its id back without a delete.
    #[test]
    fn an_image_whose_encoding_failed_draws_nothing_and_sends_no_delete() {
        let images = Images::with_ids(Graphics::KITTY, [5]);
        let mut image = images.encode(testing::red());

        image.after_encoding(None);
        image.after_encoding(Some(Ok(())));
        assert_eq!(images.held(), [5], "nothing failed");
        image.after_encoding(Some(Err(Errors::Sixel("no palette".to_owned()))));

        assert_eq!(images.held(), Vec::<u32>::new());
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));
        image.draw(buf.area, &mut buf, false);
        assert_eq!(buf, Buffer::empty(buf.area), "nothing drawn");
        drop(image);
        assert_eq!(images.take_all(), Vec::<u32>::new(), "no delete");
    }

    // -----------------------------------------------------------------
    // Kitty image ids
    // -----------------------------------------------------------------

    /// The id of the image the transmission in `buf` sends.
    fn sent_id(buf: &Buffer) -> u32 {
        let command = transmission(buf);
        let id = &command[command.find(",i=").expect("the command names its image") + 3..];
        id[..id.find(',').expect("more keys follow the id")]
            .parse()
            .expect("a numeric id")
    }

    /// The protocol names an image by its id in the transmission and in
    /// every placeholder, the id's low three bytes as the foreground
    /// colour (ratatui-image 12.0.0-rc.2 `protocol/kitty.rs` l.132-143).
    #[test]
    fn a_kitty_image_is_sent_under_the_id_its_book_issued() {
        let images = Images::with_ids(Graphics::KITTY, [0x00AB_CDEF]);
        let mut image = images.encode(testing::red());
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));

        image.draw(Rect::new(0, 0, 2, 2), &mut buf, false);

        assert!(transmission(&buf).contains(",i=11259375,"));
        assert_eq!(testing::placeholder_cells(&buf).len(), 4, "one per cell");
        assert_eq!(
            testing::kitty_ids_on_screen(&buf),
            [0x00AB_CDEF].into(),
            "every placeholder names it"
        );
    }

    /// ratatui-image encodes an image again when its area changes size.
    /// Under the same id, Ghostty would blank the picture for the length
    /// of the transfer, so the new encoding gets a new id and the old one
    /// goes for deleting. The first encoding uses the id the image got.
    #[test]
    fn a_kitty_image_laid_out_at_another_size_is_sent_under_a_new_id() {
        let images = Images::kitty();
        let mut image = images.encode(testing::red());

        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 8));
        image.draw(Rect::new(0, 0, 2, 2), &mut buf, true);
        assert_eq!(sent_id(&buf), 1);
        assert_eq!(images.take_dropped(), Vec::<u32>::new());

        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 8));
        image.draw(Rect::new(0, 0, 2, 2), &mut buf, true);
        assert_eq!(images.take_dropped(), Vec::<u32>::new(), "same size");

        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 8));
        image.draw(Rect::new(0, 0, 4, 8), &mut buf, true);
        assert_eq!(sent_id(&buf), 2);
        assert_eq!(images.take_dropped(), [1]);
        assert_eq!(images.held(), [2]);

        drop(image);
        assert_eq!(images.take_dropped(), [2]);
    }

    /// Every id the book handed out and has not handed on as deleted, and
    /// those it has, once, for deleting all before asqr quits.
    #[test]
    fn at_the_end_every_image_is_handed_on_once() {
        let images = Images::with_ids(Graphics::KITTY, [3, 1, 2]);
        let kept = [images.encode(testing::red()), images.encode(testing::red())];
        drop(images.encode(testing::red()));

        assert_eq!(images.take_all(), [2, 1, 3]);
        drop(kept);
        assert_eq!(images.take_all(), Vec::<u32>::new(), "handed on once");
        assert_eq!(images.take_dropped(), Vec::<u32>::new());
    }

    /// Every other protocol carries its picture in the cells or in the
    /// command written there, and takes no id.
    #[test]
    fn other_protocols_take_no_kitty_id() {
        for protocol in [Protocol::HalfBlocks, Protocol::Sixel, Protocol::Iterm2] {
            let graphics = Graphics {
                protocol,
                ..Graphics::HALF_BLOCKS
            };
            let images = Images::with_ids(graphics, []);

            let image = images.encode(testing::red());

            assert!(
                matches!(
                    (protocol, image.protocol.protocol_type()),
                    (Protocol::HalfBlocks, StatefulProtocolType::Halfblocks(_))
                        | (Protocol::Sixel, StatefulProtocolType::Sixel(_))
                        | (Protocol::Iterm2, StatefulProtocolType::ITerm2(_))
                ),
                "{protocol:?}"
            );
        }
    }

    /// asqr takes the facts it builds protocols from out of ratatui-image's
    /// picker, and builds the Kitty protocol as the picker does, so the
    /// transmission is the same but for the id. A `Picker` takes the
    /// compression capability only from the terminal's answer to its
    /// query, so this compares the transmission without compression.
    ///
    /// Every `Picker` constructor runs `tmux set -p allow-passthrough on`
    /// in a terminal that names tmux, and would change the pane the tests
    /// run in. There the test checks nothing; outside tmux, as in CI, it
    /// runs.
    #[test]
    fn asqr_s_protocols_are_built_from_what_ratatui_image_s_picker_found() {
        if TerminalEnv::from_process().tmux_like_term() {
            eprintln!("skipped: a Picker built here would set the tmux pane's passthrough");
            return;
        }
        let mut picker = Picker::halfblocks();
        assert_eq!(Graphics::from_picker(&picker), Graphics::HALF_BLOCKS);
        picker.set_protocol_type(ProtocolType::Kitty);
        assert_eq!(Graphics::from_picker(&picker), Graphics::KITTY);
        for (theirs, ours) in [
            (ProtocolType::Sixel, Protocol::Sixel),
            (ProtocolType::Iterm2, Protocol::Iterm2),
        ] {
            let mut picker = Picker::halfblocks();
            picker.set_protocol_type(theirs);
            assert_eq!(Graphics::from_picker(&picker).protocol, ours);
        }

        let area = Rect::new(0, 0, 2, 2);
        let without_id = |protocol: &mut StatefulProtocol| {
            let mut buf = Buffer::empty(area);
            StatefulImage::default()
                .resize(Resize::Fit(None))
                .render(area, &mut buf, protocol);
            let command = transmission(&buf);
            let id = command.find("i=").expect("the command names its image");
            let after = id + command[id..].find(',').expect("more keys follow the id");
            format!("{}{}", &command[..id], &command[after + 1..])
        };
        let mut ours = Images::new(Graphics::from_picker(&picker)).encode(testing::red());
        let mut theirs = picker.new_resize_protocol(testing::red());

        assert_eq!(without_id(&mut ours.protocol), without_id(&mut theirs));
    }
}
