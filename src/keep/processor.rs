// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The step between a shown file and what is kept of it. A processor is
//! chosen by the kind of the entry that showed the file, so another
//! encoder, another format or processing for another kind replaces one
//! type here and touches nothing of the archiving around it.

use std::ffi::c_int;
use std::io::Cursor;
use std::path::Path;

use image::codecs::png::PngDecoder;
use image::{ImageFormat, ImageReader, Limits, Rgba, RgbaImage};

use crate::syntaxes::SYNTAXES;

use super::{KeepFiles, ShownKind};

/// The bytes to keep and the extension to name them with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Processed {
    pub bytes: Vec<u8>,
    pub extension: String,
}

pub trait Processor {
    /// `original` is the file as read, `name` the name or path it was
    /// shown under. The bytes come by value, so a processor that keeps a
    /// file as it is hands the same allocation back.
    fn process(&self, original: Vec<u8>, name: &str) -> Processed;
}

/// Keeps the bytes as they are.
pub struct AsIs;

impl Processor for AsIs {
    fn process(&self, original: Vec<u8>, name: &str) -> Processed {
        Processed {
            bytes: original,
            extension: kept_extension(name),
        }
    }
}

/// Extensions of text files that no syntax lists.
const TEXT_EXTENSIONS: [&str; 6] = ["txt", "md", "log", "diff", "patch", "ansi"];

/// The extension a kept file is named with: the lowercased extension of
/// `name` when an image format, a syntax or [`TEXT_EXTENSIONS`] knows it,
/// else `dat`. A name taken from the session is never put into the
/// archive as it is, as the extension of a file is what a viewer or the
/// system picks an application by. The language of a file without a
/// listed extension, `Makefile` for one, comes from the original name the
/// archived session records.
fn kept_extension(name: &str) -> String {
    let Some(extension) = Path::new(name).extension().and_then(|part| part.to_str()) else {
        return "dat".to_owned();
    };
    let extension = extension.to_ascii_lowercase();
    let well_formed = (1..=8).contains(&extension.len())
        && extension
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    let known = well_formed
        && (TEXT_EXTENSIONS.contains(&extension.as_str())
            || ImageFormat::all()
                .any(|format| format.extensions_str().contains(&extension.as_str()))
            || SYNTAXES.find_syntax_by_extension(&extension).is_some());
    if known { extension } else { "dat".to_owned() }
}

/// The libwebp quality of every re-encoded image.
const WEBP_QUALITY: f32 = 90.0;

/// Encodes an image as lossy WebP.
pub trait ImageEncoder {
    fn encode_webp(&self, rgba: &RgbaImage, quality: f32) -> Result<Vec<u8>, String>;
}

/// libwebp, called through its simple encoding API.
pub struct Libwebp;

impl ImageEncoder for Libwebp {
    fn encode_webp(&self, rgba: &RgbaImage, quality: f32) -> Result<Vec<u8>, String> {
        let too_large = || {
            format!(
                "{}x{} is too large for libwebp",
                rgba.width(),
                rgba.height()
            )
        };
        let width = c_int::try_from(rgba.width()).map_err(|_| too_large())?;
        let height = c_int::try_from(rgba.height()).map_err(|_| too_large())?;
        let stride = width.checked_mul(4).ok_or_else(too_large)?;
        let mut output: *mut u8 = std::ptr::null_mut();
        // SAFETY: `rgba` holds `height` rows of `stride` bytes, the layout
        // the call reads, and outlives it. `output` is a valid place for
        // the pointer the call sets.
        let written = unsafe {
            libwebp_sys::WebPEncodeRGBA(
                rgba.as_raw().as_ptr(),
                width,
                height,
                stride,
                quality,
                &mut output,
            )
        };
        if written == 0 || output.is_null() {
            return Err(format!(
                "libwebp could not encode a {}x{} picture",
                rgba.width(),
                rgba.height()
            ));
        }
        // SAFETY: on success the call returned in `output` a buffer of
        // `written` bytes that libwebp allocated and nothing else owns.
        let encoded = unsafe { std::slice::from_raw_parts(output, written) }.to_vec();
        // SAFETY: `output` came from the encoder above, which documents
        // `WebPFree` as the way to release it, and is not used again.
        unsafe { libwebp_sys::WebPFree(output.cast()) };
        Ok(encoded)
    }
}

/// Re-encodes an image as WebP and keeps the result only when it decodes
/// back to the same picture and is smaller.
pub struct WebpImages<E: ImageEncoder> {
    encoder: E,
}

impl<E: ImageEncoder> WebpImages<E> {
    pub fn new(encoder: E) -> Self {
        Self { encoder }
    }
}

impl<E: ImageEncoder> Processor for WebpImages<E> {
    fn process(&self, original: Vec<u8>, name: &str) -> Processed {
        let Some(rgba) = decode_for_reencoding(&original) else {
            return AsIs.process(original, name);
        };
        let Ok(encoded) = self.encoder.encode_webp(&rgba, WEBP_QUALITY) else {
            return AsIs.process(original, name);
        };
        let Ok(round_trip) = image::load_from_memory_with_format(&encoded, ImageFormat::WebP)
        else {
            return AsIs.process(original, name);
        };
        let round_trip = round_trip.to_rgba8();
        if !same_picture(&rgba, &round_trip) || encoded.len() >= original.len() {
            return AsIs.process(original, name);
        }
        Processed {
            bytes: encoded,
            extension: "webp".to_owned(),
        }
    }
}

/// The longest side libwebp encodes (the format's limit).
const WEBP_MAX_SIDE: u32 = 16383;

/// The most memory a decode may allocate, so a small file that claims an
/// enormous picture is kept as it is instead of exhausting memory.
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;

/// The pixels of `original`, or `None` when the file must stay as it is.
/// The format is read from the content, never the name. WebP is already
/// what re-encoding makes, and GIF and animated PNG hold frames that one
/// still picture would lose. The size is read from the header first, so a
/// picture libwebp cannot encode is not decoded.
fn decode_for_reencoding(original: &[u8]) -> Option<RgbaImage> {
    match image::guess_format(original).ok()? {
        ImageFormat::WebP | ImageFormat::Gif => return None,
        ImageFormat::Png => {
            let animated = PngDecoder::new(Cursor::new(original))
                .ok()?
                .is_apng()
                .ok()?;
            if animated {
                return None;
            }
        }
        _ => {}
    }
    let reader = || {
        ImageReader::new(Cursor::new(original))
            .with_guessed_format()
            .ok()
    };
    let (width, height) = reader()?.into_dimensions().ok()?;
    if !(1..=WEBP_MAX_SIDE).contains(&width) || !(1..=WEBP_MAX_SIDE).contains(&height) {
        return None;
    }
    let mut reader = reader()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    Some(reader.decode().ok()?.to_rgba8())
}

/// The lowest PSNR, in dB, at which a re-encoded image still counts as
/// the same picture.
const MIN_PSNR_DB: f64 = 30.0;

/// Whether `kept` shows the picture `original` does. Both are laid over
/// mid-grey before they are compared, as a viewer shows transparency, so
/// the colour libwebp is free to change under a fully transparent pixel
/// does not count. The alpha channel is compared on its own, since over
/// grey a faint pixel hides a wrong alpha.
fn same_picture(original: &RgbaImage, kept: &RgbaImage) -> bool {
    if original.dimensions() != kept.dimensions() {
        return false;
    }
    let over_grey = |pixel: &Rgba<u8>| {
        let alpha = f64::from(pixel[3]);
        [0, 1, 2]
            .map(|channel| (f64::from(pixel[channel]) * alpha + MID_GREY * (255.0 - alpha)) / 255.0)
    };
    let colour_error = original
        .pixels()
        .zip(kept.pixels())
        .flat_map(|(a, b)| {
            let (a, b) = (over_grey(a), over_grey(b));
            [0, 1, 2].map(|channel| (a[channel] - b[channel]).powi(2))
        })
        .sum::<f64>();
    let alpha_error = original
        .pixels()
        .zip(kept.pixels())
        .map(|(a, b)| (f64::from(a[3]) - f64::from(b[3])).powi(2))
        .sum::<f64>();
    let pixels = f64::from(original.width()) * f64::from(original.height());
    psnr(colour_error / (pixels * 3.0)) > MIN_PSNR_DB && psnr(alpha_error / pixels) > MIN_PSNR_DB
}

/// The grey a transparent picture is laid over for the comparison.
const MID_GREY: f64 = 128.0;

fn psnr(mean_squared_error: f64) -> f64 {
    if mean_squared_error == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0f64.powi(2) / mean_squared_error).log10()
}

/// The processor for files of `kind` under `mode`.
pub fn processor_for(kind: ShownKind, mode: KeepFiles) -> Box<dyn Processor> {
    match (kind, mode) {
        (ShownKind::Image, KeepFiles::Compact) => Box::new(WebpImages::new(Libwebp)),
        _ => Box::new(AsIs),
    }
}

#[cfg(test)]
mod tests;
