// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The syntaxes the code viewer highlights with, through patine, which
//! keeps its own copy private. Everything that names a file by what the
//! highlighting knows asks this one set: the extension of a file `o`
//! writes for an opener, and the extension of a file asqr keeps. It is
//! loaded by the first caller.

use std::sync::LazyLock;

use two_face::re_exports::syntect::parsing::SyntaxSet;

pub static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
