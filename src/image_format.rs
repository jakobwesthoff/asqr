// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The rule that names the format of an image file, shared by what shows an
//! image (the viewer's decoding) and what warns about one (the check of
//! `asqr validate` and `asqr ask`). Both must accept the same files, or
//! `asqr ask` would let through an image the viewer cannot show, or warn
//! about one it would show.

use std::io::{BufRead, Seek};
use std::path::Path;

use image::error::{ImageFormatHint, UnsupportedError, UnsupportedErrorKind};
use image::{ImageError, ImageFormat, ImageReader};

/// An [`ImageReader`] over `source`, the file at `path`, with its format
/// set. The format comes from the file's first bytes, else from the
/// extension of `path`: TGA, for one, has no signature to recognise. A
/// file that no bytes identify and that has no extension comes back
/// without a format, for the reader to fail on in its own words. An
/// extension no format knows fails with an error that names it.
pub fn reader_with_format<R: BufRead + Seek>(
    source: R,
    path: &Path,
) -> Result<ImageReader<R>, ImageError> {
    let mut reader = ImageReader::new(source).with_guessed_format()?;
    if reader.format().is_none()
        && let Some(extension) = path.extension().filter(|extension| !extension.is_empty())
    {
        match ImageFormat::from_extension(extension) {
            Some(format) => reader.set_format(format),
            None => {
                let hint = ImageFormatHint::PathExtension(extension.into());
                return Err(ImageError::Unsupported(
                    UnsupportedError::from_format_and_kind(
                        hint.clone(),
                        UnsupportedErrorKind::Format(hint),
                    ),
                ));
            }
        }
    }
    Ok(reader)
}
