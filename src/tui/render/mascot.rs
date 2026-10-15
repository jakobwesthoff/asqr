// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Talqi, asqr's mascot, as the empty queue shows it (spec section 7.1,
//! ADR 43): the terminal emblem as a fixed block of text cells, in colour
//! or as an outline under `NO_COLOR`, in glyphs chosen for the terminal.

mod glyphs;
mod template;

pub use template::Glyphs;
pub(super) use template::{EMBLEM_COLUMNS, EMBLEM_ROWS};
