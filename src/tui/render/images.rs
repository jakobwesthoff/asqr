// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Question images (spec section 7.7). The graphics protocol is detected
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
            // never resolved (spec section 7.7).
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

    /// Forgets every loaded image, so each is read and sent to the terminal
    /// again when it is next drawn.
    ///
    /// The Kitty protocol sends an image's data once, on its first draw,
    /// and afterwards only refers to it. tmux with `allow-passthrough on`
    /// drops that transfer while the pane is not visible, and the image
    /// would never appear (spec section 7.7).
    pub fn forget(&mut self) {
        self.cache.clear();
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

/// Whether the terminal may be asked for its graphics protocol. `tmux` is
/// the `TMUX` variable, and `tmux_state` asks tmux for
/// `#{allow-passthrough} #{session_attached}`: the pane's passthrough
/// option and the number of clients attached to the session.
///
/// Inside tmux the query only reaches a terminal through passthrough, and
/// only while a client is attached. Without an answer the query's reader
/// goes on taking the keys the person types, so the query is skipped and
/// images fall back to half blocks (spec section 7.7).
pub fn may_query_protocol(tmux: Option<&str>, tmux_state: impl FnOnce() -> Option<String>) -> bool {
    if tmux.is_none_or(str::is_empty) {
        return true;
    }
    let Some(state) = tmux_state() else {
        return false;
    };
    let mut words = state.split_whitespace();
    // `on` lets the pane pass sequences through, `all` any pane.
    let passthrough = matches!(words.next(), Some("on" | "all"));
    let attached = words
        .next()
        .and_then(|clients| clients.parse::<u32>().ok())
        .is_some_and(|clients| clients > 0);
    passthrough && attached
}

#[cfg(test)]
mod tests {
    use super::*;

    const TMUX: Option<&str> = Some("/tmp/tmux-501/default,1234,0");

    #[test]
    fn outside_tmux_the_terminal_is_asked() {
        assert!(may_query_protocol(None, || unreachable!(
            "not asked outside tmux"
        )));
        assert!(may_query_protocol(Some(""), || unreachable!(
            "an empty TMUX is unset"
        )));
    }

    #[test]
    fn inside_tmux_only_with_passthrough_and_a_client() {
        assert!(may_query_protocol(TMUX, || Some("on 1\n".into())));
        assert!(may_query_protocol(TMUX, || Some("all 2".into())));
        assert!(!may_query_protocol(TMUX, || Some("off 1\n".into())));
        assert!(
            !may_query_protocol(TMUX, || Some("on 0\n".into())),
            "detached"
        );
    }

    #[test]
    fn inside_tmux_anything_unexpected_counts_as_no() {
        assert!(!may_query_protocol(TMUX, || None));
        assert!(!may_query_protocol(TMUX, || Some(String::new())));
        assert!(!may_query_protocol(TMUX, || Some("on".into())));
        assert!(!may_query_protocol(TMUX, || Some("on many".into())));
    }
}
