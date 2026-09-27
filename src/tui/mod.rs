// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The terminal UI (spec section 7). The interaction is a state machine
//! driven by key events, kept apart from drawing and from the terminal so
//! tests can run every key without one.

mod alert;
mod app;
mod inbox;
pub mod render;
mod session;
mod watch;

pub use alert::*;
pub use app::*;
pub use inbox::*;
pub use session::*;
pub use watch::*;
