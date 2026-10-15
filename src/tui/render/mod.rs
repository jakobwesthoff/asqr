// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drawing the TUI (spec section 7) from the state in [`super::App`].

mod code;
mod content;
mod context;
mod diff;
mod empty;
mod header;
mod images;
mod keys;
mod markdown_viewer;
mod mascot;
mod overlay;
mod panes;
mod parts;
mod patine_adapter;
mod screen;
mod source_file;
mod source_line;
mod tabs;
#[cfg(test)]
mod test_support;
mod text;
pub(crate) mod theme;
mod viewer;
mod wrap;

#[cfg(unix)]
pub use images::passthrough_answers;
pub use images::{TerminalNames, kitty_deletes, may_query_protocol};
pub use panes::PaneLayout;
pub use screen::{Drawn, MIN_HEIGHT, MIN_WIDTH, View, draw};
pub(crate) use source_line::{TAB_STOP, columns};
pub(crate) use wrap::{Glyph, styled_line, wrap_rows};
