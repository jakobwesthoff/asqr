// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drawing the TUI (spec section 7) from the state in [`super::App`].

mod code;
mod context;
mod diff;
mod images;
mod markdown_viewer;
mod panes;
mod parts;
mod patine_adapter;
mod screen;
mod source_file;
mod source_line;
mod text;
mod viewer;

pub use images::may_query_protocol;
pub use panes::PaneLayout;
pub use screen::{MIN_HEIGHT, MIN_WIDTH, View, draw};
