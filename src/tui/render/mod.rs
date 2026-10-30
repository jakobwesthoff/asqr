// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drawing the TUI (spec section 7) from the state in [`super::App`].

// The submodules are private, so a plain `pub` in them reaches nobody, and
// rustc's `dead_code` cannot tell a `pub` item that fell out of use from one
// that is part of the API. Items are `pub(super)` or `pub(crate)` unless
// they are re-exported below for use outside this module.
#![warn(unreachable_pub)]

mod ansi;
mod code;
mod content;
mod context;
mod diff;
mod dim;
mod elsewhere;
mod empty;
mod header;
mod keys;
mod loading;
mod markdown_viewer;
mod mascot;
mod overlay;
mod panes;
mod parts;
mod patine_adapter;
mod problem;
mod screen;
mod source_file;
mod source_line;
mod switcher;
mod tabs;
#[cfg(test)]
pub(crate) mod test_support;
mod text;
pub(crate) mod theme;
mod viewer;
mod wrap;

pub(crate) use diff::unified_text;
pub use loading::loader_panicked;
pub use mascot::Glyphs;
pub use panes::{PaneLayout, ReadingWidth};
pub(crate) use problem::shown_path;
pub(crate) use screen::shorten_from_left;
pub use screen::{Drawn, MIN_HEIGHT, MIN_WIDTH, View, draw};
pub(crate) use source_line::{TAB_STOP, columns, picture};
pub use switcher::draw_picker;
pub(crate) use wrap::{Glyph, styled_line, wrap_rows};

/// Why the file at `path` may not be opened, by the rule every viewer
/// follows (spec section 8.3), in the words of the card a viewer shows for
/// it; `None` for a file that may.
pub(crate) fn unopenable(path: &str) -> Option<String> {
    source_file::regular_file(path)
        .err()
        .map(|unreadable| unreadable.reason)
}
