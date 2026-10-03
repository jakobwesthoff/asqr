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
use ratatui::buffer::Buffer;
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
            Slot::Ready(protocol) => {
                frame.render_stateful_widget(
                    StatefulImage::default().resize(resize),
                    area,
                    protocol.as_mut(),
                );
                // Works around images cropped by a changed cell size; see
                // the function for when it can go.
                size_kitty_placement(frame.buffer_mut(), area);
            }
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

/// The character the Kitty protocol reserves for image placeholders.
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

/// The start of the Kitty command that transmits an image and creates its
/// virtual placement, as `ratatui-image` 11.1.0 writes it.
const KITTY_PLACEMENT: &str = "a=T,U=1,";

/// Makes the Kitty placement drawn into `area` span exactly the cells its
/// placeholders cover, by adding `c=` and `r=` to its command. This is a
/// workaround for a gap in `ratatui-image` 11.1.0 and is meant to go away:
/// upstream fixed it in pull request
/// <https://github.com/ratatui/ratatui-image/pull/215> (commit `2d7d7f6`,
/// merged on 2026-10-03), which no release contains yet. The newest one
/// then was 12.0.0-rc.0, which predates the merge.
///
/// # The problem
///
/// With the Kitty protocol, `ratatui-image` sends the image once, with a
/// command that also creates a "virtual placement" (`a=T,U=1`), and then
/// fills the image's area with placeholder characters (`U+10EEEE`) that
/// tell the terminal which part of the image goes into which cell. The
/// image is resized beforehand to the area's cells times the cell size in
/// pixels that asqr asked the terminal for once, at start.
///
/// The placement command names the image's pixel size but not its size in
/// cells. The terminal then works the cells out itself, from the pixels
/// and the cell size it has right now. As long as both cell sizes agree,
/// that gives exactly the placeholder cells. When they differ, it does
/// not:
///
/// - after a font size change while asqr runs, as asqr never asks again;
/// - in tmux with several clients attached whose cell sizes differ, as
///   only one of them can match the cell size asqr got.
///
/// Smaller cells than expected crop the image to its top-left part, larger
/// ones leave it short of its area. Both were seen in Ghostty, with and
/// without tmux, on 2026-10-03.
///
/// # The workaround
///
/// `c=` and `r=` name the placement's size in cells, and the terminal then
/// scales the image into exactly those cells, whatever its own cell size.
/// `ratatui-image` writes the command into the symbol of the first
/// placeholder cell of the frame that sends the image. So after the widget
/// has drawn, this looks for that cell inside `area`, counts the
/// placeholder columns and rows around it, and inserts `c=` and `r=` right
/// after `a=T,U=1,`. Inside tmux the command is wrapped for passthrough,
/// which doubles the escape characters but leaves this plain-text part
/// alone. Frames that send nothing, and every other protocol, have no such
/// cell, and the buffer stays as it is.
///
/// This depends on the exact text `ratatui-image` writes, which is why
/// `Cargo.toml` pins it to `=11.1.0`. A version bump has to check this
/// function, and the test
/// `a_kitty_image_is_placed_over_exactly_its_placeholder_cells` in
/// `screen.rs` fails when the command changes shape.
///
/// # Why not patch `ratatui-image`
///
/// The proper fix belongs in `ratatui-image` itself, which is what pull
/// request 215 does. Until a release carries it, only a git dependency or
/// a `[patch.crates-io]` entry would pull it in, and that works for local
/// builds only: `cargo publish` drops patches, and crates on crates.io
/// cannot depend on git sources. Releases on crates.io would have kept the
/// bug.
///
/// # Removing it
///
/// TODO: Remove this once a `ratatui-image` release contains PR 215 (see
/// the todo `01m4169zbj9p5qfr20761ye0th-drop-the-kitty-placement-workaround.md`):
///
/// 1. Bump `ratatui-image` to that release and drop the `=` pin in
///    `Cargo.toml`. The release is likely a 12.x, which may change the
///    `Picker` API that `src/terminal.rs` and this module use.
/// 2. Delete this function, its two constants and the call in
///    [`Images::draw_resized`].
/// 3. Run the test named above. It must stay green without this
///    function, as the crate then writes `c=` and `r=` itself.
fn size_kitty_placement(buffer: &mut Buffer, area: Rect) {
    let area = area.intersection(buffer.area);
    let mut command = None;
    let (mut columns, mut rows) = (0, 0);
    for y in area.top()..area.bottom() {
        let mut in_row = 0;
        for x in area.left()..area.right() {
            let symbol = buffer[(x, y)].symbol();
            if !symbol.contains(KITTY_PLACEHOLDER) {
                continue;
            }
            in_row += 1;
            if symbol.contains(KITTY_PLACEMENT) {
                command = Some((x, y));
            }
        }
        if in_row > 0 {
            columns = columns.max(in_row);
            rows += 1;
        }
    }
    let Some(position) = command else {
        return;
    };
    let cell = &mut buffer[position];
    let symbol = cell.symbol().replacen(
        KITTY_PLACEMENT,
        &format!("{KITTY_PLACEMENT}c={columns},r={rows},"),
        1,
    );
    cell.set_symbol(&symbol);
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
