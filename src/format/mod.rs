// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The asqr file format, version 1 (spec sections 4 and 5): the session
//! file an asker writes, and the result and draft files asqr writes back.

mod session;

pub use session::*;
