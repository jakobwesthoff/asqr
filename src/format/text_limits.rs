// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! How much of a text asqr shows (spec section 8.3). The viewers cut a
//! text by these limits, and the warnings about a session file count by
//! them, so both sides read a text the same way.

/// How much of a file a viewer reads. The read happens while the person
/// waits for the screen, and a viewer keeps all of it, so a log that grew
/// to gigabytes must not stall asqr or fill its memory.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
