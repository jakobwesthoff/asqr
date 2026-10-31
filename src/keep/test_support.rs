// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixtures the tests of the modules that keep files share.

use std::io::Cursor;

use image::{ImageFormat, Rgba, RgbaImage};

/// A PNG of a smooth gradient, which WebP at quality 90 shrinks and
/// reproduces closely.
pub(crate) fn gradient_png() -> Vec<u8> {
    let picture = RgbaImage::from_fn(128, 128, |x, y| {
        Rgba([(x * 2) as u8, (y * 2) as u8, 90, 255])
    });
    let mut bytes = Cursor::new(Vec::new());
    picture
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("encodes");
    bytes.into_inner()
}
