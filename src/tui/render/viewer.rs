// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Viewers show the content of one context entry in the body of the
//! context pane (spec section 7.7), and [`Viewers`] keeps one per entry
//! shown, so work that depends only on the entry, or on the entry and the
//! body's size, is done once.

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};
use ratatui_image::picker::Picker;

use super::images::Image;
use super::screen::wrap;
use crate::format::ContextEntry;
use crate::tui::{EntryKey, Offset};

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
/// header, the scroll position and the keys. The pane calls `load` once,
/// then `lay_out` before the first `draw` and again whenever the body's size
/// changes, and calls `draw` on every frame with an area of the size last
/// passed to `lay_out`.
pub trait Viewer {
    /// Prepares everything that does not depend on the body's size: reading
    /// files, decoding images, highlighting, computing a diff. Called once,
    /// before the first `lay_out`. A failure is not returned; the viewer
    /// keeps it and shows it as its content.
    fn load(&mut self);

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
    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset);
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

/// Builds the viewer for an entry, given the terminal's graphics protocol.
type Build = Box<dyn Fn(&ContextEntry, &Picker) -> Box<dyn Viewer>>;

/// One viewer per entry shown, built and loaded when the entry is first
/// shown and kept while the entry stays the same and its session waits. A
/// viewer is laid out again only when the body's size changes, as its
/// content does not.
pub struct Viewers {
    picker: Picker,
    build: Build,
    viewers: HashMap<ViewerKey, Kept>,
    /// How many viewers were built, so tests see that one is reused.
    #[cfg(test)]
    built: usize,
}

impl Viewers {
    /// Viewers for a terminal whose graphics protocol `picker` speaks.
    pub fn new(picker: Picker) -> Self {
        Viewers {
            picker,
            build: Box::new(viewer_for),
            viewers: HashMap::new(),
            #[cfg(test)]
            built: 0,
        }
    }

    /// The viewer of `entry`, which session `session` shows as `key`, laid
    /// out for a body of `width`×`height` cells, and the size its content
    /// takes there. A viewer is built and loaded when its key shows an
    /// entry for the first time or another entry than before, as a
    /// replaced session file can.
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
            let mut viewer = (self.build)(entry, &self.picker);
            viewer.load();
            let kept = Kept {
                entry: entry.clone(),
                viewer,
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
}

fn viewer_for(entry: &ContextEntry, picker: &Picker) -> Box<dyn Viewer> {
    match entry {
        ContextEntry::Image(image) => Box::new(ImageViewer {
            path: image.path.clone(),
            enlarge: image.enlarge,
            picker: picker.clone(),
            image: None,
        }),
        // TODO: replace with the text and diff viewers, see the todos
        // `01M4XHQSD1EM17QD87CFB3VWMG-show-text-entries-in-the-context-pane-with-syntax-highlighting.md`
        // and `01M4XHQSDRCG7M60ZQ5M2D530J-show-diff-entries-in-the-context-pane-as-a-unified-diff.md`.
        ContextEntry::Text(text) => Box::new(Placeholder::new(
            "text not shown",
            text.title.as_ref().or(text.path.as_ref()),
            "this asqr cannot show text entries yet",
        )),
        ContextEntry::Diff(diff) => Box::new(Placeholder::new(
            "diff not shown",
            diff.title.as_ref().or(diff.name.as_ref()),
            "this asqr cannot show diffs yet",
        )),
    }
}

// ---------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------

/// An image file, scaled to fill the body as far as its proportions allow,
/// or, without `enlarge`, at its natural size where that fits. A file that
/// cannot be shown shows why instead.
struct ImageViewer {
    path: String,
    enlarge: bool,
    picker: Picker,
    image: Option<Image>,
}

impl Viewer for ImageViewer {
    fn load(&mut self) {
        self.image = Some(Image::load(&self.picker, &self.path));
    }

    /// The image never gets larger than the body, so the body is all the
    /// room it needs.
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        ContentSize {
            lines: usize::from(height),
            columns: usize::from(width),
        }
    }

    /// Nothing scrolls, so the offset is always zero.
    fn draw(&mut self, area: Rect, buf: &mut Buffer, _offset: Offset) {
        self.image
            .as_mut()
            .expect("the pane loads a viewer before drawing it")
            .draw(&self.path, area, buf, self.enlarge);
    }
}

// ---------------------------------------------------------------------
// Entries without a viewer of their own
// ---------------------------------------------------------------------

/// Three lines that name an entry this asqr cannot show, the way an image
/// placeholder names its file, wrapped to the body.
struct Placeholder {
    lines: Vec<Line<'static>>,
    wrapped: Vec<Line<'static>>,
}

impl Placeholder {
    fn new(heading: &'static str, name: Option<&String>, reason: &'static str) -> Self {
        let mut lines = vec![Line::from(heading).dim()];
        lines.extend(name.map(|name| Line::from(name.clone())));
        lines.push(Line::from(reason).dim());
        Placeholder {
            lines,
            wrapped: Vec::new(),
        }
    }
}

impl Viewer for Placeholder {
    /// The lines are known from the entry alone.
    fn load(&mut self) {}

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

    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) {
        let lines: Vec<Line<'static>> = self.wrapped.iter().skip(offset.top).cloned().collect();
        Paragraph::new(lines).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    fn entry(json: &str) -> ContextEntry {
        serde_json::from_str(json).expect("test entry parses")
    }

    fn viewers() -> Viewers {
        Viewers::new(Picker::halfblocks())
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
    /// from line `top` on, one string per row.
    fn drawn(entry: &ContextEntry, width: u16, height: u16, top: usize) -> Vec<String> {
        let mut viewers = viewers();
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let (viewer, _) = viewers.laid_out("s", QUESTION, entry, width, height);
        viewer.draw(area, &mut buf, Offset { top, left: 0 });
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    // -----------------------------------------------------------------
    // The life of a viewer
    // -----------------------------------------------------------------

    /// What `Viewers` asked of the viewers a test built.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct Calls {
        loads: usize,
        lay_outs: Vec<(u16, u16)>,
    }

    /// A viewer that only records the calls it gets.
    struct Recording(Rc<RefCell<Calls>>);

    impl Viewer for Recording {
        fn load(&mut self) {
            self.0.borrow_mut().loads += 1;
        }

        fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
            self.0.borrow_mut().lay_outs.push((width, height));
            ContentSize {
                lines: 3,
                columns: usize::from(width),
            }
        }

        fn draw(&mut self, _area: Rect, _buf: &mut Buffer, _offset: Offset) {}
    }

    /// Viewers whose every viewer records into the returned calls.
    fn recording() -> (Viewers, Rc<RefCell<Calls>>) {
        let calls = Rc::new(RefCell::new(Calls::default()));
        let mut viewers = viewers();
        let shared = Rc::clone(&calls);
        viewers.build = Box::new(move |_, _| Box::new(Recording(Rc::clone(&shared))));
        (viewers, calls)
    }

    #[test]
    fn a_viewer_is_loaded_once_and_laid_out_only_when_the_size_changes() {
        let (mut viewers, calls) = recording();
        let text = entry(r#"{"type": "text", "text": "notes"}"#);

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
                loads: 1,
                lay_outs: vec![(40, 10), (30, 10), (30, 12)]
            }
        );
    }

    #[test]
    fn a_viewer_is_built_once_per_entry_and_reused() {
        let mut viewers = viewers();
        let image = entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#);
        let text = entry(r#"{"type": "text", "text": "notes"}"#);

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
    fn a_replaced_entry_gets_a_new_viewer_loaded_and_laid_out() {
        let (mut viewers, calls) = recording();
        built(
            &mut viewers,
            "s",
            QUESTION,
            &entry(r#"{"type": "text", "text": "one"}"#),
        );

        built(
            &mut viewers,
            "s",
            QUESTION,
            &entry(r#"{"type": "text", "text": "two"}"#),
        );

        assert_eq!(viewers.built, 2);
        assert_eq!(viewers.viewers.len(), 1, "the old one is gone");
        assert_eq!(calls.borrow().loads, 2);
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
        let text = entry(r#"{"type": "text", "text": "notes"}"#);
        built(&mut viewers, "s", QUESTION, &image);
        built(&mut viewers, "s", OPTION, &text);

        viewers.forget_images();
        built(&mut viewers, "s", QUESTION, &image);
        built(&mut viewers, "s", OPTION, &text);

        assert_eq!(viewers.built, 3, "the image again, the text kept");
    }

    // -----------------------------------------------------------------
    // Images
    // -----------------------------------------------------------------

    #[test]
    fn an_image_takes_the_body() {
        let mut viewers = viewers();
        let (_, size) = viewers.laid_out(
            "s",
            QUESTION,
            &entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#),
            40,
            12,
        );

        assert_eq!(
            size,
            ContentSize {
                lines: 12,
                columns: 40
            }
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

    // -----------------------------------------------------------------
    // Placeholders
    // -----------------------------------------------------------------

    #[test]
    fn a_placeholder_wraps_to_the_body_and_counts_its_lines() {
        let diff = entry(r#"{"type": "diff", "name": "src/fetch.rs", "new": {"text": "x"}}"#);
        let mut viewers = viewers();

        let (_, size) = viewers.laid_out("s", QUESTION, &diff, 40, 10);
        assert_eq!(
            size,
            ContentSize {
                lines: 3,
                columns: 31
            },
            "as wide as its widest line"
        );
        let (_, size) = viewers.laid_out("s", QUESTION, &diff, 20, 10);
        assert_eq!(
            size,
            ContentSize {
                lines: 4,
                columns: 17
            },
            "the reason takes two lines at 20 columns"
        );
        let (_, size) = viewers.laid_out("s", QUESTION, &diff, 16, 10);
        assert_eq!(
            size.columns, 16,
            "the blank after `cannot`, hanging past the edge, does not count"
        );

        assert_eq!(
            drawn(&diff, 40, 3, 0),
            [
                "diff not shown",
                "src/fetch.rs",
                "this asqr cannot show diffs yet"
            ]
        );
        assert_eq!(
            drawn(&diff, 40, 3, 1),
            ["src/fetch.rs", "this asqr cannot show diffs yet", ""],
            "it starts at the offset"
        );
    }

    #[test]
    fn a_text_placeholder_names_its_title_or_path() {
        let titled = entry(r#"{"type": "text", "title": "Log", "path": "/var/log/x"}"#);
        assert_eq!(drawn(&titled, 40, 3, 0)[1], "Log");

        let untitled = entry(r#"{"type": "text", "path": "/var/log/x"}"#);
        assert_eq!(drawn(&untitled, 40, 3, 0)[1], "/var/log/x");

        let inline = entry(r#"{"type": "text", "text": "x"}"#);
        assert_eq!(
            drawn(&inline, 40, 3, 0),
            [
                "text not shown",
                "this asqr cannot show text entries yet",
                ""
            ]
        );
    }
}
