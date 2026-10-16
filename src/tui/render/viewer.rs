// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Viewers show the content of one context entry in the body of the
//! context pane (spec section 7.7), and [`Viewers`] keeps one per entry
//! shown, so work that depends only on the entry, or on the entry and the
//! body's size, is done once.

use std::collections::HashMap;

use super::code::CodeViewer;
use super::diff::DiffViewer;
use super::markdown_viewer::MarkdownViewer;
use super::source_file::{Unreadable, regular_file};
use super::theme;
use super::wrap::wrap;
use crate::format::{ContextEntry, ImageEntry};
use crate::graphics::{Graphics, Image, Images, KittyBook, PlacedImage};
use crate::tui::{EntryKey, Offset};
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
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
/// with an area of the size last passed to `lay_out`.
pub trait Viewer {
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
}

/// Which entry of which session a viewer belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ViewerKey {
    session: String,
    entry: EntryKey,
}

/// A viewer with the entry it was built from, and the body size it was
/// last laid out for with the size its content took there.
struct Kept {
    entry: ContextEntry,
    viewer: Box<dyn Viewer>,
    laid_out: Option<((u16, u16), ContentSize)>,
}

/// Builds the viewer for an entry, given what makes its images.
type Build = Box<dyn Fn(&ContextEntry, &Images) -> Box<dyn Viewer>>;

/// One viewer per entry shown, built when the entry is first shown and
/// kept while the entry stays the same and its session waits. A
/// viewer is laid out again only when the body's size changes, as its
/// content does not.
pub struct Viewers {
    images: Images,
    build: Build,
    viewers: HashMap<ViewerKey, Kept>,
    /// How many viewers were built, so tests see that one is reused.
    #[cfg(test)]
    built: usize,
}

impl Viewers {
    /// Viewers whose images `images` makes.
    pub fn new(images: Images) -> Self {
        Viewers {
            images,
            build: Box::new(viewer_for),
            viewers: HashMap::new(),
            #[cfg(test)]
            built: 0,
        }
    }

    /// The viewer of `entry`, which session `session` shows as `key`, laid
    /// out for a body of `width`×`height` cells, and the size its content
    /// takes there. A viewer is built when its key shows an entry for the
    /// first time or another entry than before, as a replaced session file
    /// can.
    pub fn laid_out(
        &mut self,
        session: &str,
        key: EntryKey,
        entry: &ContextEntry,
        width: u16,
        height: u16,
    ) -> (&mut dyn Viewer, ContentSize) {
        let key = ViewerKey {
            session: session.to_owned(),
            entry: key,
        };
        let current = self
            .viewers
            .get(&key)
            .is_some_and(|kept| kept.entry == *entry);
        if !current {
            #[cfg(test)]
            {
                self.built += 1;
            }
            let kept = Kept {
                entry: entry.clone(),
                viewer: (self.build)(entry, &self.images),
                laid_out: None,
            };
            self.viewers.insert(key.clone(), kept);
        }
        let kept = self
            .viewers
            .get_mut(&key)
            .expect("the viewer was just built when it was missing");
        let size = match kept.laid_out {
            Some((laid_out_for, size)) if laid_out_for == (width, height) => size,
            _ => {
                let size = kept.viewer.lay_out(width, height);
                kept.laid_out = Some(((width, height), size));
                size
            }
        };
        (kept.viewer.as_mut(), size)
    }

    /// The graphics the images are made for.
    pub fn graphics(&self) -> Graphics {
        self.images.graphics()
    }

    /// Drops every image viewer, so each image is read and sent to the
    /// terminal again when it is next shown. The Kitty protocol sends an
    /// image's data once, on its first draw, and afterwards only refers to
    /// it; tmux with `allow-passthrough on` drops that transfer while the
    /// pane is not visible, and the image would never appear (ADR 24).
    /// Other viewers keep what they derived, as nothing of theirs went to
    /// the terminal.
    pub fn forget_images(&mut self) {
        self.viewers
            .retain(|_, kept| !matches!(kept.entry, ContextEntry::Image(_)));
    }

    /// Drops the viewers of every session `waiting` says no longer waits.
    pub fn retain_sessions(&mut self, waiting: impl Fn(&str) -> bool) {
        self.viewers.retain(|key, _| waiting(&key.session));
    }

    /// The book of the Kitty ids the images of the viewers take.
    pub fn kitty_book(&self) -> &KittyBook {
        self.images.kitty_book()
    }

    /// The Kitty ids of the images whose viewers went since the last call,
    /// for the terminal to delete.
    #[cfg(test)]
    pub(super) fn dropped_images(&self) -> Vec<u32> {
        self.images.dropped()
    }

    /// The Kitty ids of the images the viewers hold.
    #[cfg(test)]
    pub(super) fn held_images(&self) -> Vec<u32> {
        self.images.held()
    }
}

fn viewer_for(entry: &ContextEntry, images: &Images) -> Box<dyn Viewer> {
    match entry {
        ContextEntry::Image(image) => Box::new(ImageViewer::new(image, images)),
        ContextEntry::Code(code) => Box::new(CodeViewer::new(code)),
        ContextEntry::Markdown(markdown) => Box::new(MarkdownViewer::new(markdown)),
        ContextEntry::Diff(diff) => Box::new(DiffViewer::new(diff)),
    }
}

// ---------------------------------------------------------------------
// What a viewer shows
// ---------------------------------------------------------------------

/// What a viewer made of its entry when it was built: the content it
/// shows, or a placeholder in its place, such as for a file that cannot be
/// read. The viewer keeps it and hands laying out and drawing on to it.
pub(super) enum Content<T> {
    Body(T),
    Note(Placeholder),
}

impl<T: Viewer> Viewer for Content<T> {
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        match self {
            Content::Body(body) => body.lay_out(width, height),
            Content::Note(placeholder) => placeholder.lay_out(width, height),
        }
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        match self {
            Content::Body(body) => body.draw(area, buf, offset),
            Content::Note(placeholder) => placeholder.draw(area, buf, offset),
        }
    }
}

// ---------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------

/// An image file, scaled to fill the body as far as its proportions allow,
/// or, without `enlarge`, at its natural size where that fits. A file that
/// cannot be shown shows why instead, in the placeholder every viewer of a
/// file shows, which scrolls when its lines do not fit.
struct ImageViewer {
    content: Content<Picture>,
}

impl ImageViewer {
    /// Opens the path by the rule every viewer of a file follows, then
    /// decodes the image. It is encoded when it is laid out, as that
    /// depends on the body's size.
    fn new(entry: &ImageEntry, images: &Images) -> Self {
        let decoded = regular_file(&entry.path).and_then(|()| {
            images.load(&entry.path).map_err(|error| Unreadable {
                path: entry.path.clone(),
                reason: error.to_string(),
            })
        });
        let content = match decoded {
            Ok(image) => Content::Body(Picture {
                image,
                enlarge: entry.enlarge,
            }),
            Err(unreadable) => Content::Note(unreadable.placeholder("image not shown")),
        };
        ImageViewer { content }
    }
}

impl Viewer for ImageViewer {
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        self.content.lay_out(width, height)
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        self.content.draw(area, buf, offset)
    }
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

/// Up to three lines that name a file a viewer cannot read and say why,
/// or one dim note in place of content, wrapped to the body.
pub(super) struct Placeholder {
    lines: Vec<Line<'static>>,
    wrapped: Vec<Line<'static>>,
}

impl Placeholder {
    pub(super) fn new(
        heading: &'static str,
        name: Option<&String>,
        reason: impl Into<String>,
    ) -> Self {
        let mut lines = vec![Line::styled(heading, theme::secondary())];
        lines.extend(name.map(|name| Line::from(name.clone())));
        lines.push(Line::styled(reason.into(), theme::secondary()));
        Placeholder {
            lines,
            wrapped: Vec::new(),
        }
    }

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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::super::test_support::{self, row};
    use super::*;
    use crate::graphics::Protocol;

    fn entry(json: &str) -> ContextEntry {
        serde_json::from_str(json).expect("test entry parses")
    }

    fn viewers() -> Viewers {
        Viewers::new(Images::half_blocks())
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
        viewers.laid_out(session, key, entry, 40, 10);
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
        let (viewer, _) = viewers.laid_out("s", QUESTION, entry, width, height);
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

    /// What `Viewers` asked of the viewers a test built.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct Calls {
        builds: usize,
        lay_outs: Vec<(u16, u16)>,
    }

    /// A viewer that only records the calls it gets.
    struct Recording(Rc<RefCell<Calls>>);

    impl Viewer for Recording {
        fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
            self.0.borrow_mut().lay_outs.push((width, height));
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
    fn recording() -> (Viewers, Rc<RefCell<Calls>>) {
        let calls = Rc::new(RefCell::new(Calls::default()));
        let mut viewers = viewers();
        let shared = Rc::clone(&calls);
        viewers.build = Box::new(move |_, _| {
            shared.borrow_mut().builds += 1;
            Box::new(Recording(Rc::clone(&shared)))
        });
        (viewers, calls)
    }

    #[test]
    fn a_viewer_is_built_when_its_entry_first_shows_and_laid_out_only_when_the_size_changes() {
        let (mut viewers, calls) = recording();
        let text = entry(r#"{"type": "code", "content": "notes"}"#);
        assert_eq!(calls.borrow().builds, 0, "nothing shows yet");

        for _ in 0..3 {
            let (_, size) = viewers.laid_out("s", QUESTION, &text, 40, 10);
            assert_eq!(
                size,
                ContentSize {
                    lines: 3,
                    columns: 40
                }
            );
        }
        let (_, size) = viewers.laid_out("s", QUESTION, &text, 30, 10);
        assert_eq!(size.columns, 30, "a resize lays it out again");
        viewers.laid_out("s", QUESTION, &text, 30, 10);
        viewers.laid_out("s", QUESTION, &text, 30, 12);

        assert_eq!(
            *calls.borrow(),
            Calls {
                builds: 1,
                lay_outs: vec![(40, 10), (30, 10), (30, 12)]
            }
        );
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
        let (mut viewers, calls) = recording();
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
        assert_eq!(calls.borrow().builds, 2);
        assert_eq!(calls.borrow().lay_outs.len(), 2, "at the same size");
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
        Viewers::new(Images::kitty())
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
            let mut viewers = Viewers::new(Images::new(Graphics {
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

        let (viewer, size) = viewers.laid_out("s", QUESTION, &image, 40, 12);

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
        let rows = drawn(
            &entry(r#"{"type": "image", "path": "/nowhere/cat.png"}"#),
            40,
            4,
            0,
        );
        assert_eq!(rows[0], "image not shown");
        assert_eq!(rows[1], "/nowhere/cat.png");
        assert!(!rows[2].is_empty(), "the reason: {rows:?}");
    }

    #[test]
    fn an_image_is_refused_by_the_path_rule() {
        let (_directory, paths) = test_support::refused_paths();

        for path in paths {
            let image = entry(&format!(r#"{{"type": "image", "path": {path:?}}}"#));
            let mut refused = regular_file(&path)
                .expect_err("the rule refuses it")
                .placeholder("image not shown");

            assert_eq!(
                drawn(&image, 60, 3, 0),
                drawn_viewer(&mut refused, 60, 3, 0),
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

        let rows = drawn(&image, 200, 3, 0);

        assert_eq!(rows[0], "image not shown");
        assert_eq!(rows[1], path.to_str().expect("a UTF-8 path"));
        assert!(!rows[2].is_empty(), "the reason: {rows:?}");
    }

    #[test]
    fn an_image_that_cannot_be_read_scrolls_like_any_placeholder() {
        let path = "/nowhere/cat.png".to_owned();
        let reason = std::fs::metadata(&path)
            .expect_err("the file is missing")
            .to_string();
        let mut placeholder = Placeholder::new("image not shown", Some(&path), reason);
        let image = entry(r#"{"type": "image", "path": "/nowhere/cat.png"}"#);

        let (_, size) = viewers().laid_out("s", QUESTION, &image, 20, 2);

        assert_eq!(size, placeholder.lay_out(20, 2));
        assert!(size.lines > 2, "taller than the body: {size:?}");
        assert_eq!(
            drawn(&image, 20, 2, 2),
            drawn_viewer(&mut placeholder, 20, 2, 2),
            "it starts at the offset"
        );
    }

    // -----------------------------------------------------------------
    // Placeholders
    // -----------------------------------------------------------------

    #[test]
    fn a_placeholder_wraps_to_the_body_and_counts_its_lines() {
        let path = "/src/fetch.rs".to_owned();
        let mut placeholder = Placeholder::new(
            "code not shown",
            Some(&path),
            "this file moved after the session came",
        );

        assert_eq!(
            placeholder.lay_out(40, 10),
            ContentSize {
                lines: 3,
                columns: 38
            },
            "as wide as its widest line"
        );
        assert_eq!(
            placeholder.lay_out(20, 10),
            ContentSize {
                lines: 5,
                columns: 18
            },
            "the reason takes three lines at 20 columns"
        );
        assert_eq!(
            placeholder.lay_out(9, 10).columns,
            9,
            "the blank after `file`, hanging past the edge, does not count"
        );

        assert_eq!(
            drawn_viewer(&mut placeholder, 40, 3, 0),
            [
                "code not shown",
                "/src/fetch.rs",
                "this file moved after the session came"
            ]
        );
        assert_eq!(
            drawn_viewer(&mut placeholder, 40, 3, 1),
            [
                "/src/fetch.rs",
                "this file moved after the session came",
                ""
            ],
            "it starts at the offset"
        );
    }

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
