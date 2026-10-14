// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drawing the TUI (spec section 7) from the state in [`super::App`].

mod context;
mod images;
pub mod markdown;
mod panes;
mod parts;
mod screen;
mod viewer;

pub use images::may_query_protocol;
pub use panes::PaneLayout;
pub use screen::{MIN_HEIGHT, MIN_WIDTH, View, draw};
