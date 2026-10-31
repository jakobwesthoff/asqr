// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The terminal UI (spec section 7). The interaction is a state machine
//! driven by key events, kept apart from drawing and from the terminal so
//! tests can run every key without one.

mod alert;
mod app;
mod editor;
mod inbox;
mod key_reader;
mod open;
mod opened;
mod picker;
mod progress;
pub mod render;
mod run;
mod scroll;
mod session;
mod settings;
mod switch;
mod switcher;
mod text_field;
mod watch;

pub use alert::*;
pub use app::*;
pub use editor::*;
pub use inbox::*;
pub use key_reader::*;
pub use open::*;
pub use opened::*;
pub use picker::*;
pub use progress::*;
pub use run::*;
pub use scroll::{Extent, Offset, Page};
pub use session::*;
pub use settings::*;
pub use switch::*;
pub use switcher::*;
pub use text_field::{FieldKey, FieldShape, Leave, TextField};
pub use watch::*;
