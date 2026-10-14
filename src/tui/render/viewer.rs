// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Viewers show one context entry in the body of the context pane (spec
//! section 7.7). The pane draws the header above the body and decides
//! where the body starts scrolled to; a viewer only knows its content.
//!
//! A viewer lives as long as its entry is open, in [`Viewers`], so work
//! that only depends on the entry and the pane's width, such as reading a
//! file or highlighting it, is done once and not on every frame.

use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::Images;
use super::screen::wrap;
use crate::format::ContextEntry;
use crate::tui::{EntryKey, Offset};

/// The size of a viewer's content in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentSize {
    pub lines: usize,
    pub columns: usize,
}

/// Shared render resources a viewer borrows for one draw.
pub struct DrawContext<'a> {
    pub images: &'a mut Images,
    /// The entry fills the tab's area (`z`) rather than the pane.
    pub full_screen: bool,
}

/// Shows one kind of context entry in the pane's body. The text and diff
/// viewers implement it as well, and a strip of several entries above the
/// body leaves it as it is.
pub trait Viewer {
    /// Lays the content out for a body of `width`×`height` cells and
    /// returns its size. Work that depends on the width (wrapping,
    /// rendering Markdown) happens here and is kept until the width
    /// changes. The pane calls `lay_out` before every `draw`, with the size
    /// of the area it then passes to `draw`. Content that wraps reports at
    /// most `width` columns; content scaled to fit reports the body itself.
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize;

    /// Whether lines wider than the body are cut and scroll sideways
    /// (source code, diffs) instead of wrapping.
    fn scrolls_sideways(&self) -> bool;

    /// Draws the content as last laid out into `area`, with `offset` at
    /// its top-left corner.
    fn draw(&mut self, frame: &mut Frame, area: Rect, offset: Offset, cx: &mut DrawContext<'_>);
}

/// Which entry of which session a viewer belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ViewerKey {
    session: String,
    entry: EntryKey,
}

/// One viewer per shown entry, created on first show and kept for as long
/// as its session waits.
#[derive(Default)]
pub struct Viewers {
    /// Each viewer with the entry it was built from. A replaced session
    /// file can put another entry under the same key, which then gets a
    /// viewer of its own.
    viewers: HashMap<ViewerKey, (ContextEntry, Box<dyn Viewer>)>,
    /// How many viewers were built, so tests see that one is reused.
    #[cfg(test)]
    built: usize,
}

impl Viewers {
    /// The viewer of `entry`, which session `session` shows as `key`.
    pub fn viewer(
        &mut self,
        session: &str,
        key: EntryKey,
        entry: &ContextEntry,
    ) -> &mut dyn Viewer {
        let key = ViewerKey {
            session: session.to_owned(),
            entry: key,
        };
        let current = self
            .viewers
            .get(&key)
            .is_some_and(|(built_from, _)| built_from == entry);
        if !current {
            #[cfg(test)]
            {
                self.built += 1;
            }
            self.viewers
                .insert(key.clone(), (entry.clone(), viewer_for(entry)));
        }
        let (_, viewer) = self
            .viewers
            .get_mut(&key)
            .expect("the viewer was just made when it was missing");
        viewer.as_mut()
    }

    /// Drops the viewers of every session `waiting` says no longer waits.
    pub fn retain_sessions(&mut self, waiting: impl Fn(&str) -> bool) {
        self.viewers.retain(|key, _| waiting(&key.session));
    }
}

fn viewer_for(entry: &ContextEntry) -> Box<dyn Viewer> {
    match entry {
        ContextEntry::Image(image) => Box::new(ImageViewer {
            path: image.path.clone(),
        }),
        // TODO: the text and diff viewers (context pane plan, steps 4 and
        // 5) replace these placeholders.
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

/// An image, scaled to fit the body. The image itself is loaded and kept
/// by [`Images`], which forgets it when the terminal regains the focus.
struct ImageViewer {
    path: String,
}

impl Viewer for ImageViewer {
    /// An image is scaled to fit the body, so its content is exactly the
    /// body and nothing scrolls.
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        ContentSize {
            lines: usize::from(height),
            columns: usize::from(width),
        }
    }

    fn scrolls_sideways(&self) -> bool {
        false
    }

    /// The offset is always zero, as nothing scrolls. Full screen, a small
    /// image grows to fill the area.
    fn draw(&mut self, frame: &mut Frame, area: Rect, _offset: Offset, cx: &mut DrawContext<'_>) {
        if cx.full_screen {
            cx.images.draw_full_screen(frame, &self.path, area);
        } else {
            cx.images.draw(frame, &self.path, area);
        }
    }
}

// ---------------------------------------------------------------------
// Entries without a viewer yet
// ---------------------------------------------------------------------

/// What stands in for an entry this asqr has no viewer for. It names the
/// entry the way an image placeholder names its file.
struct Placeholder {
    lines: Vec<Line<'static>>,
    /// The lines wrapped at the width they were last laid out for.
    wrapped: Option<(u16, Vec<Line<'static>>)>,
}

impl Placeholder {
    fn new(heading: &'static str, name: Option<&String>, reason: &'static str) -> Self {
        let mut lines = vec![Line::from(heading).dim()];
        lines.extend(name.map(|name| Line::from(name.clone())));
        lines.push(Line::from(reason).dim());
        Placeholder {
            lines,
            wrapped: None,
        }
    }

    fn wrapped(&mut self, width: u16) -> &[Line<'static>] {
        if self.wrapped.as_ref().is_none_or(|(at, _)| *at != width) {
            let lines = self
                .lines
                .iter()
                .flat_map(|line| wrap(line.clone(), usize::from(width), 0, 0))
                .collect();
            self.wrapped = Some((width, lines));
        }
        &self.wrapped.as_ref().expect("wrapped just now").1
    }
}

impl Viewer for Placeholder {
    fn lay_out(&mut self, width: u16, _height: u16) -> ContentSize {
        ContentSize {
            lines: self.wrapped(width).len(),
            columns: usize::from(width),
        }
    }

    fn scrolls_sideways(&self) -> bool {
        false
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, offset: Offset, _cx: &mut DrawContext<'_>) {
        let lines: Vec<Line<'static>> = self
            .wrapped(area.width)
            .iter()
            .skip(offset.top)
            .cloned()
            .collect();
        frame.render_widget(Paragraph::new(lines), area);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui_image::picker::Picker;

    use super::*;

    fn entry(json: &str) -> ContextEntry {
        serde_json::from_str(json).expect("test entry parses")
    }

    const QUESTION: EntryKey = EntryKey {
        question: 0,
        option: None,
    };

    const OPTION: EntryKey = EntryKey {
        question: 0,
        option: Some(1),
    };

    /// The text the viewer for `entry` draws into `width`×`height` cells
    /// from line `top` on, one string per row.
    fn drawn(entry: &ContextEntry, width: u16, height: u16, top: usize) -> Vec<String> {
        let mut viewers = Viewers::default();
        let mut images = Images::new(Picker::halfblocks());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| {
                let mut cx = DrawContext {
                    images: &mut images,
                    full_screen: false,
                };
                let offset = Offset { top, left: 0 };
                viewers
                    .viewer("s", QUESTION, entry)
                    .draw(frame, frame.area(), offset, &mut cx);
            })
            .expect("draws");
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn a_viewer_is_built_once_per_entry_and_reused() {
        let mut viewers = Viewers::default();
        let image = entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#);
        let text = entry(r#"{"type": "text", "text": "notes"}"#);

        viewers.viewer("s", QUESTION, &image);
        viewers.viewer("s", QUESTION, &image);
        assert_eq!(viewers.built, 1, "the second draw reuses the viewer");

        viewers.viewer("s", OPTION, &text);
        viewers.viewer("other", QUESTION, &image);
        viewers.viewer("s", QUESTION, &image);
        viewers.viewer("s", OPTION, &text);
        assert_eq!(viewers.built, 3, "one per entry of each session");
    }

    #[test]
    fn a_replaced_entry_gets_a_new_viewer() {
        let mut viewers = Viewers::default();
        viewers.viewer("s", QUESTION, &entry(r#"{"type": "text", "text": "one"}"#));

        viewers.viewer("s", QUESTION, &entry(r#"{"type": "text", "text": "two"}"#));

        assert_eq!(viewers.built, 2);
        assert_eq!(viewers.viewers.len(), 1, "the old one is gone");
    }

    #[test]
    fn the_viewers_of_a_finished_session_go() {
        let mut viewers = Viewers::default();
        let image = entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#);
        viewers.viewer("finished", QUESTION, &image);
        viewers.viewer("waiting", QUESTION, &image);

        viewers.retain_sessions(|session| session == "waiting");
        viewers.viewer("waiting", QUESTION, &image);

        assert_eq!(viewers.viewers.len(), 1);
        assert_eq!(viewers.built, 2, "the waiting session kept its viewer");
    }

    #[test]
    fn an_image_fills_the_body_and_does_not_scroll() {
        let mut viewers = Viewers::default();
        let viewer = viewers.viewer(
            "s",
            QUESTION,
            &entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#),
        );

        assert_eq!(
            viewer.lay_out(40, 12),
            ContentSize {
                lines: 12,
                columns: 40
            }
        );
        assert!(!viewer.scrolls_sideways());
    }

    #[test]
    fn a_broken_image_shows_why_in_the_body() {
        let rows = drawn(
            &entry(r#"{"type": "image", "path": "/nowhere/cat.png"}"#),
            40,
            4,
            0,
        );
        assert_eq!(rows[0], "image not shown");
        assert_eq!(rows[1], "/nowhere/cat.png");
    }

    #[test]
    fn a_placeholder_wraps_to_the_body_and_counts_its_lines() {
        let diff = entry(r#"{"type": "diff", "name": "src/fetch.rs", "new": {"text": "x"}}"#);
        let mut viewers = Viewers::default();
        let viewer = viewers.viewer("s", QUESTION, &diff);

        assert_eq!(
            viewer.lay_out(40, 10),
            ContentSize {
                lines: 3,
                columns: 40
            }
        );
        assert_eq!(
            viewer.lay_out(20, 10).lines,
            4,
            "the reason takes two lines at 20 columns"
        );
        assert!(!viewer.scrolls_sideways());

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
