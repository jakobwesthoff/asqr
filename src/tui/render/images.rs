// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Question images (spec section 7.6). The graphics protocol is detected
//! once, in the binary, by querying the terminal; this module receives the
//! resulting [`Picker`], so tests draw with the halfblock protocol and never
//! talk to a terminal. Images load on first display and are cached by
//! path. A path that is relative, missing or unreadable shows a placeholder
//! and never becomes an error.

use std::collections::HashMap;
use std::path::Path;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

enum Slot {
    Ready(Box<StatefulProtocol>),
    Unavailable(String),
}

pub struct Images {
    picker: Picker,
    cache: HashMap<String, Slot>,
}

impl Images {
    pub fn new(picker: Picker) -> Self {
        Images {
            picker,
            cache: HashMap::new(),
        }
    }

    fn slot(&mut self, path: &str) -> &mut Slot {
        let picker = &self.picker;
        self.cache.entry(path.to_owned()).or_insert_with(|| {
            // A relative path depends on where asqr was started, so it is
            // never resolved (spec section 7.6).
            if Path::new(path).is_relative() {
                return Slot::Unavailable("relative path".to_owned());
            }
            match image::ImageReader::open(path).and_then(|reader| reader.with_guessed_format()) {
                Err(error) => Slot::Unavailable(error.to_string()),
                Ok(reader) => match reader.decode() {
                    Ok(decoded) => Slot::Ready(Box::new(picker.new_resize_protocol(decoded))),
                    Err(error) => Slot::Unavailable(error.to_string()),
                },
            }
        })
    }

    /// Draws the image at `path` into `area`, never larger than it is, or
    /// the placeholder.
    pub fn draw(&mut self, frame: &mut Frame, path: &str, area: Rect) {
        self.draw_resized(frame, path, area, Resize::Fit(None));
    }

    /// Draws the image at `path` filling `area` as far as its proportions
    /// allow, enlarging small images: the `z` view.
    pub fn draw_full_screen(&mut self, frame: &mut Frame, path: &str, area: Rect) {
        self.draw_resized(frame, path, area, Resize::Scale(None));
    }

    fn draw_resized(&mut self, frame: &mut Frame, path: &str, area: Rect, resize: Resize) {
        match self.slot(path) {
            Slot::Ready(protocol) => frame.render_stateful_widget(
                StatefulImage::default().resize(resize),
                area,
                protocol.as_mut(),
            ),
            Slot::Unavailable(reason) => {
                let text = vec![
                    Line::from("image not shown").dim(),
                    Line::from(path.to_owned()),
                    Line::from(reason.clone()).dim(),
                ];
                frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
            }
        }
    }
}
