// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One image of a context entry. It is decoded when it is made, encoded
//! for the terminal's graphics protocol whenever it is laid out at another
//! size, and drawn as encoded. A picture that cannot be decoded never
//! becomes an [`Image`]; one that cannot be encoded draws nothing.

use ::image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::widgets::Widget;
use ratatui_image::errors::Errors;
use ratatui_image::protocol::Protocol as Encoded;
use ratatui_image::protocol::halfblocks::Halfblocks;
use ratatui_image::protocol::iterm2::Iterm2;
use ratatui_image::protocol::kitty::Kitty;
use ratatui_image::protocol::sixel::Sixel;
use ratatui_image::{FilterType, FontSize, Resize};

use super::kitty::HeldId;
use super::placeholders::PlacedImage;
use super::{Images, Protocol};

/// One image file, decoded, and encoded for the size it was last laid out
/// at.
pub struct Image {
    images: Images,
    state: State,
}

enum State {
    /// The decoded picture, and its encoding once it was laid out.
    Ready {
        picture: DynamicImage,
        encoding: Option<Encoding>,
    },
    /// An encoding failed. The image draws nothing from then on, where
    /// encoding again at every size would log the same error each time.
    Failed,
}

/// The picture encoded for one size, and under the Kitty protocol the id
/// the terminal keeps it under. The id's drop hands it on for deleting,
/// so the image's holder decides when the terminal lets go of it, and a
/// drawn image names it to the screen that mends it.
struct Encoding {
    encoded: Encoded,
    kitty_id: Option<HeldId>,
}

/// The filter that scales a picture to its cells: each pixel of the
/// result mixes the pixels it covers, where the nearest pixel alone would
/// drop thin lines and text.
const SCALING: FilterType = FilterType::Triangle;

impl Image {
    /// `picture`, to be encoded by `images` once it is laid out.
    pub(super) fn new(images: &Images, picture: DynamicImage) -> Image {
        Image {
            images: images.clone(),
            state: State::Ready {
                picture,
                encoding: None,
            },
        }
    }

    /// Encodes the picture for an area of `available` cells, unless its
    /// encoding has the size it takes there already. With `enlarge`, the
    /// picture is scaled to fill the area as far as its proportions
    /// allow; without, it keeps its natural size and only shrinks when it
    /// does not fit.
    ///
    /// Under the Kitty protocol each encoding goes out under a new id.
    /// Ghostty deletes an image when a new transmission under its id
    /// begins, and shows it blank until that ends; under a new id the old
    /// copy stays untouched while the new one comes in, and the old id
    /// goes for deleting after the frame (ADR 42).
    pub fn lay_out(&mut self, available: Size, enlarge: bool) {
        let State::Ready { picture, encoding } = &mut self.state else {
            return;
        };
        if available.width == 0 || available.height == 0 {
            return;
        }
        let resize = if enlarge {
            Resize::Scale(Some(SCALING))
        } else {
            Resize::Fit(Some(SCALING))
        };
        let graphics = self.images.graphics;
        let font = FontSize::new(graphics.cell.width, graphics.cell.height);
        let size = resize.size_for(picture, font, available);
        if encoding
            .as_ref()
            .is_some_and(|encoding| encoding.encoded.size() == size)
        {
            return;
        }
        let resized = resize.resize(picture, font, size, None);
        let encoded = encode(&self.images, resized, size);
        self.settle(encoded);
    }

    /// Keeps a new encoding, or on a failure logs it and lets the picture
    /// go. Either way the encoding before goes, and with it its Kitty id.
    fn settle(&mut self, encoded: Result<Encoding, Errors>) {
        match (encoded, &mut self.state) {
            (Ok(new), State::Ready { encoding, .. }) => *encoding = Some(new),
            (Err(error), _) => {
                tracing::warn!(%error, "could not encode an image; it stays empty");
                self.state = State::Failed;
            }
            (Ok(_), State::Failed) => {}
        }
    }

    /// Draws the image as it was last laid out into `area` of `buf`, which
    /// has the size it was laid out for. Under the Kitty protocol it says
    /// where it was drawn, for the screen to mend once everything over it
    /// is drawn too (ADR 48).
    pub fn draw(&self, area: Rect, buf: &mut Buffer) -> Option<PlacedImage> {
        let State::Ready {
            encoding: Some(encoding),
            ..
        } = &self.state
        else {
            return None;
        };
        ratatui_image::Image::new(&encoding.encoded).render(area, buf);
        let kitty_id = encoding.kitty_id.as_ref()?;
        Some(PlacedImage::read(
            buf,
            area,
            encoding.encoded.size(),
            kitty_id.id,
        ))
    }
}

/// `resized`, which fills `size` cells, encoded for the protocol `images`
/// makes images for. One function builds every protocol, and under the
/// Kitty protocol under a new id from the run's book.
///
/// It passes what `Picker::new_protocol` passes (ratatui-image
/// 12.0.0-rc.2 `picker.rs` l.263-298) but for the Kitty id and shared
/// memory: the tmux wrapping, and compression only where the terminal
/// said it inflates `o=z`. Shared memory stays off. The picker only
/// reports it when the query asked for it, it works only with a terminal
/// on the same machine, and every transmission leaves an object behind
/// for the terminal to unlink. Without a background colour, a picture is
/// padded with transparent pixels.
fn encode(images: &Images, resized: DynamicImage, size: Size) -> Result<Encoding, Errors> {
    let graphics = images.graphics;
    let tmux = graphics.tmux_wrapped;
    let encoded = match graphics.protocol {
        Protocol::HalfBlocks => Encoded::Halfblocks(Halfblocks::new(resized, size)?),
        Protocol::Sixel => Encoded::Sixel(Sixel::new(resized, size, tmux)?),
        Protocol::Iterm2 => Encoded::ITerm2(Iterm2::new(resized, size, tmux)?),
        Protocol::Kitty => {
            let kitty_id = HeldId::issued(&images.book);
            let kitty = Kitty::new(
                resized,
                size,
                kitty_id.id,
                tmux,
                graphics.kitty_compression,
                false,
            );
            return with_kitty_id(kitty, kitty_id);
        }
    };
    Ok(Encoding {
        encoded,
        kitty_id: None,
    })
}

/// The Kitty encoding `kitty` under `kitty_id`. An encoding that failed
/// never went out, so its id goes back without a delete.
fn with_kitty_id(kitty: Result<Kitty, Errors>, kitty_id: HeldId) -> Result<Encoding, Errors> {
    match kitty {
        Ok(kitty) => Ok(Encoding {
            encoded: Encoded::Kitty(kitty),
            kitty_id: Some(kitty_id),
        }),
        Err(error) => {
            kitty_id.forget();
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use ratatui_image::picker::{Picker, ProtocolType};

    use super::super::Graphics;
    use super::super::testing::{self, placeholder_cells, transmission};
    use super::*;
    use crate::terminal_env::TerminalEnv;

    /// Why the file at `path` does not open or decode, or `None` when it
    /// does.
    fn refused(path: &str) -> Option<String> {
        fs::File::open(path)
            .map_err(::image::ImageError::IoError)
            .and_then(|file| Images::half_blocks().decode(file, path))
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

    /// `image` laid out for `area` and drawn into a buffer of that area.
    fn drawn(image: &mut Image, area: Rect, enlarge: bool) -> Buffer {
        let mut buf = Buffer::empty(area);
        image.lay_out(area.as_size(), enlarge);
        image.draw(area, &mut buf);
        buf
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
        let mut image = Images::half_blocks().image(stripes.into());

        let buf = drawn(&mut image, Rect::new(0, 0, 20, 2), false);

        let greys: Vec<_> = buf
            .content()
            .iter()
            .flat_map(|cell| [cell.fg, cell.bg])
            .filter(|colour| matches!(colour, ratatui::style::Color::Rgb(64..=192, _, _)))
            .collect();
        assert!(!greys.is_empty(), "{:?}", buf.content()[0]);
    }

    #[test]
    fn an_image_not_laid_out_or_laid_out_in_no_room_draws_nothing() {
        let mut image = Images::half_blocks().image(testing::red());
        let area = Rect::new(0, 0, 4, 4);
        let mut buf = Buffer::empty(area);

        image.draw(area, &mut buf);
        image.lay_out(Size::new(0, 4), true);
        image.draw(area, &mut buf);

        assert_eq!(buf, Buffer::empty(area));
    }

    /// An encoding that fails is logged once, and the image draws nothing
    /// from then on instead of encoding again at every size. A Kitty
    /// encoding that failed never went out, so its id goes back without a
    /// delete, while the encoding before it, which did, is deleted.
    #[test]
    fn an_image_whose_encoding_failed_draws_nothing_and_sends_no_delete() {
        let images = Images::kitty();
        let mut image = images.image(testing::red());
        drawn(&mut image, Rect::new(0, 0, 2, 2), true);
        assert_eq!(images.held(), [1]);

        let failed = with_kitty_id(
            Err(Errors::Sixel("no palette".to_owned())),
            HeldId::issued(&images.book),
        );
        image.settle(failed);

        assert_eq!(images.held(), Vec::<u32>::new());
        assert_eq!(images.dropped(), [1], "the one that went out");
        let area = Rect::new(0, 0, 4, 8);
        assert_eq!(drawn(&mut image, area, true), Buffer::empty(area));
        assert_eq!(images.dropped(), Vec::<u32>::new(), "no encoding again");
        image.settle(encode(&images, testing::red(), Size::new(2, 2)));
        assert_eq!(
            drawn(&mut image, area, true),
            Buffer::empty(area),
            "a late encoding is not kept"
        );
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
        let mut image = images.image(testing::red());

        let buf = drawn(&mut image, Rect::new(0, 0, 2, 2), false);

        assert!(transmission(&buf).contains(",i=11259375,"));
        assert_eq!(placeholder_cells(&buf).len(), 4, "one per cell");
        assert_eq!(
            testing::kitty_ids_on_screen(&buf),
            [0x00AB_CDEF].into(),
            "every placeholder names it"
        );
    }

    /// The terminal draws a Kitty image into the cells its placement
    /// names. They must be the cells asqr fills with placeholders: left to
    /// the terminal, it derives them from its own cell size, which differs
    /// from the one asqr resized the image with after a font size change
    /// or in a second tmux client, and crops the image.
    #[test]
    fn a_kitty_image_is_placed_over_exactly_its_placeholder_cells() {
        let wide: DynamicImage =
            ::image::RgbaImage::from_pixel(160, 90, ::image::Rgba([0, 0, 255, 255])).into();
        let mut image = Images::kitty().image(wide);

        let buf = drawn(&mut image, Rect::new(0, 0, 30, 12), true);

        let placeholders = placeholder_cells(&buf);
        let columns: BTreeSet<u16> = placeholders.iter().map(|&(x, _)| x).collect();
        let rows: BTreeSet<u16> = placeholders.iter().map(|&(_, y)| y).collect();
        assert_eq!(
            (columns.len(), rows.len()),
            (30, 9),
            "scaled to the area's width, 300×169 pixels in cells of 10×20"
        );
        let placement = format!("c={},r={}", columns.len(), rows.len());
        assert!(
            transmission(&buf).contains(&placement),
            "the placement names {placement}"
        );
    }

    /// A Kitty image says where it was drawn: its id, the cells it took,
    /// which are fewer than the area where it keeps its size, and on its
    /// first draw the transmission. Other protocols leave nothing to mend.
    #[test]
    fn only_a_kitty_image_says_where_it_was_drawn() {
        let area = Rect::new(1, 1, 6, 4);
        let mut half_blocks = Images::half_blocks().image(testing::red());
        half_blocks.lay_out(area.as_size(), false);
        assert!(
            half_blocks
                .draw(area, &mut Buffer::empty(Rect::new(0, 0, 8, 6)))
                .is_none()
        );

        let images = Images::with_ids(Graphics::KITTY, [0x2A00_0001]);
        let mut image = images.image(testing::red());
        image.lay_out(area.as_size(), false);
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 6));
        let first = image.draw(area, &mut buf).expect("a Kitty image");
        let again = image.draw(area, &mut buf).expect("a Kitty image");

        assert_eq!(first.id(), 0x2A00_0001);
        assert_eq!(
            first.area(),
            Rect::new(1, 1, 2, 2),
            "the red picture's 2×2 cells"
        );
        assert!(
            first
                .transmission()
                .is_some_and(|sent| sent.starts_with("\x1b_G") && sent.contains(",i=704643073,")),
            "{:?}",
            first.transmission()
        );
        assert_eq!(again.transmission(), None, "sent once");
    }

    /// An image laid out at another size is encoded again. Under the same
    /// id, Ghostty would blank the picture for the length of the transfer,
    /// so the new encoding gets a new id and the old one goes for
    /// deleting. A body that changes size without changing the image's
    /// keeps the encoding.
    #[test]
    fn a_kitty_image_laid_out_at_another_size_is_sent_under_a_new_id() {
        let images = Images::kitty();
        let mut image = images.image(testing::red());

        let buf = drawn(&mut image, Rect::new(0, 0, 2, 2), true);
        assert_eq!(sent_id(&buf), 1);
        assert_eq!(images.dropped(), Vec::<u32>::new());

        drawn(&mut image, Rect::new(0, 0, 3, 2), true);
        assert_eq!(images.dropped(), Vec::<u32>::new(), "same image size");

        let buf = drawn(&mut image, Rect::new(0, 0, 4, 8), true);
        assert_eq!(sent_id(&buf), 2);
        assert_eq!(images.dropped(), [1]);
        assert_eq!(images.held(), [2]);

        drop(image);
        assert_eq!(images.dropped(), [2]);
    }

    /// An image takes its id when it is first laid out. Every id the book
    /// handed out and has not handed on as deleted, and those it has, are
    /// handed on once, for deleting all before asqr quits.
    #[test]
    fn at_the_end_every_image_is_handed_on_once() {
        let images = Images::with_ids(Graphics::KITTY, [3, 1, 2]);
        let laid_out = || {
            let mut image = images.image(testing::red());
            image.lay_out(Size::new(2, 2), true);
            image
        };
        let kept = [laid_out(), laid_out()];
        drop(laid_out());
        drop(images.image(testing::red()));

        assert_eq!(images.drained(), [2, 1, 3]);
        drop(kept);
        assert_eq!(images.drained(), Vec::<u32>::new(), "handed on once");
        assert_eq!(images.dropped(), Vec::<u32>::new());
    }

    /// Every other protocol carries its picture in the cells or in the
    /// command written there, and takes no id: the book has none to give.
    #[test]
    fn other_protocols_take_no_kitty_id() {
        for protocol in [Protocol::HalfBlocks, Protocol::Sixel, Protocol::Iterm2] {
            let graphics = Graphics {
                protocol,
                ..Graphics::HALF_BLOCKS
            };
            let images = Images::with_ids(graphics, []);
            let mut image = images.image(testing::red());

            image.lay_out(Size::new(2, 2), true);

            let State::Ready {
                encoding: Some(encoding),
                ..
            } = &image.state
            else {
                panic!("{protocol:?}: encoded");
            };
            assert!(
                matches!(
                    (protocol, &encoding.encoded),
                    (Protocol::HalfBlocks, Encoded::Halfblocks(_))
                        | (Protocol::Sixel, Encoded::Sixel(_))
                        | (Protocol::Iterm2, Encoded::ITerm2(_))
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
            let mut other = Picker::halfblocks();
            other.set_protocol_type(theirs);
            assert_eq!(Graphics::from_picker(&other).protocol, ours);
        }

        let area = Rect::new(0, 0, 2, 2);
        let without_id = |buf: &Buffer| {
            let command = transmission(buf);
            let id = command.find("i=").expect("the command names its image");
            let after = id + command[id..].find(',').expect("more keys follow the id");
            format!("{}{}", &command[..id], &command[after + 1..])
        };
        let mut ours = Images::new(Graphics::from_picker(&picker)).image(testing::red());
        let theirs = picker
            .new_protocol(testing::red(), area.as_size(), Resize::Fit(None))
            .expect("a red picture encodes");
        let mut buf = Buffer::empty(area);
        ratatui_image::Image::new(&theirs).render(area, &mut buf);

        assert_eq!(without_id(&drawn(&mut ours, area, false)), without_id(&buf));
    }
}
