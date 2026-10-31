// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::io::Cursor;

use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageFormat, Rgba};

use super::*;

// =========================================================
// Fixtures
// =========================================================

/// A deterministic stream of bytes, so a test picture is the same on
/// every run.
fn noise(seed: u32) -> impl FnMut() -> u8 {
    let mut state = seed;
    move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state >> 8) as u8
    }
}

/// A smooth gradient with a little noise, the kind of picture lossy WebP
/// shrinks well and keeps close to the original.
fn photo_like(width: u32, height: u32) -> RgbaImage {
    let mut next = noise(7);
    RgbaImage::from_fn(width, height, |x, y| {
        let mut jitter =
            |value: u32| (value + u32::from(next() % 7)).saturating_sub(3).min(255) as u8;
        Rgba([
            jitter(x * 255 / width),
            jitter(y * 255 / height),
            jitter(128),
            255,
        ])
    })
}

fn encoded(picture: &RgbaImage, format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    picture.write_to(&mut bytes, format).expect("encodes");
    bytes.into_inner()
}

fn png(picture: &RgbaImage) -> Vec<u8> {
    encoded(picture, ImageFormat::Png)
}

fn decoded(bytes: &[u8]) -> RgbaImage {
    image::load_from_memory(bytes).expect("decodes").to_rgba8()
}

fn lossless_webp(picture: &RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    WebPEncoder::new_lossless(&mut bytes)
        .encode(
            picture.as_raw(),
            picture.width(),
            picture.height(),
            ExtendedColorType::Rgba8,
        )
        .expect("encodes");
    bytes
}

fn compact(original: &[u8], name: &str) -> Processed {
    processor_for(ShownKind::Image, KeepFiles::Compact).process(original.to_vec(), name)
}

/// What the processor keeps when it refuses to change a file.
fn unchanged(original: &[u8], extension: &str) -> Processed {
    Processed {
        bytes: original.to_vec(),
        extension: extension.to_owned(),
    }
}

/// An encoder that returns a fixed answer whatever it is given, to put a
/// bad encoding in front of the gate.
struct Fixed(Result<Vec<u8>, String>);

impl ImageEncoder for Fixed {
    fn encode_webp(&self, _rgba: &RgbaImage, _quality: f32) -> Result<Vec<u8>, String> {
        self.0.clone()
    }
}

/// An encoder that must not be reached.
struct Forbidden;

impl ImageEncoder for Forbidden {
    fn encode_webp(&self, _rgba: &RgbaImage, _quality: f32) -> Result<Vec<u8>, String> {
        panic!("the picture was handed to the encoder");
    }
}

/// An encoder whose output is a faithful, lossless WebP padded with a
/// chunk of `padding` bytes, so it decodes to the same picture and is as
/// large as the test needs.
struct Padded {
    padding: usize,
}

impl ImageEncoder for Padded {
    fn encode_webp(&self, rgba: &RgbaImage, _quality: f32) -> Result<Vec<u8>, String> {
        let mut bytes = lossless_webp(rgba);
        let chunk = u32::try_from(self.padding).expect("small padding");
        bytes.extend_from_slice(b"JUNK");
        bytes.extend_from_slice(&chunk.to_le_bytes());
        bytes.resize(bytes.len() + self.padding + self.padding % 2, 0);
        let riff = u32::try_from(bytes.len() - 8).expect("small file");
        bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        Ok(bytes)
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&u32::try_from(data.len()).expect("small").to_be_bytes());
    let mut checked = kind.to_vec();
    checked.extend_from_slice(data);
    chunk.extend_from_slice(&checked);
    chunk.extend_from_slice(&crc32(&checked).to_be_bytes());
    chunk
}

/// A one-frame animated PNG: the PNG with an animation control chunk and
/// the frame control chunk of its default image put in front of the data.
fn apng(picture: &RgbaImage) -> Vec<u8> {
    let plain = png(picture);
    let idat = plain
        .windows(4)
        .position(|window| window == b"IDAT")
        .expect("has image data")
        - 4;
    let mut animation = Vec::new();
    animation.extend_from_slice(&1u32.to_be_bytes());
    animation.extend_from_slice(&0u32.to_be_bytes());
    let mut frame = Vec::new();
    frame.extend_from_slice(&0u32.to_be_bytes());
    frame.extend_from_slice(&picture.width().to_be_bytes());
    frame.extend_from_slice(&picture.height().to_be_bytes());
    frame.extend_from_slice(&[0; 8]);
    frame.extend_from_slice(&1u16.to_be_bytes());
    frame.extend_from_slice(&10u16.to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    let mut bytes = plain[..idat].to_vec();
    bytes.extend(png_chunk(b"acTL", &animation));
    bytes.extend(png_chunk(b"fcTL", &frame));
    bytes.extend_from_slice(&plain[idat..]);
    bytes
}

// =========================================================
// The modes
// =========================================================

#[test]
fn keep_files_parses_its_three_words_and_nothing_else() {
    assert_eq!("compact".parse(), Ok(KeepFiles::Compact));
    assert_eq!("exact".parse(), Ok(KeepFiles::Exact));
    assert_eq!("off".parse(), Ok(KeepFiles::Off));
    for refused in ["", "Compact", "on", "none", " off"] {
        assert!(refused.parse::<KeepFiles>().is_err(), "{refused:?}");
    }
}

#[test]
fn keep_files_prints_the_word_it_parses() {
    for mode in [KeepFiles::Compact, KeepFiles::Exact, KeepFiles::Off] {
        assert_eq!(mode.to_string().parse(), Ok(mode));
    }
    assert_eq!(KeepFiles::default(), KeepFiles::Compact);
}

// =========================================================
// Re-encoding as WebP
// =========================================================

#[test]
fn compact_turns_a_png_into_a_smaller_webp() {
    let picture = photo_like(256, 256);
    let original = png(&picture);

    let processed = compact(&original, "/tmp/x/hero.png");

    assert_eq!(processed.extension, "webp");
    assert!(processed.bytes.len() < original.len());
    let kept = decoded(&processed.bytes);
    assert_eq!(kept.dimensions(), picture.dimensions());
}

#[test]
fn an_encoding_that_does_not_decode_back_keeps_the_original() {
    let original = png(&photo_like(32, 32));
    let processor = WebpImages::new(Fixed(Ok(b"not a webp".to_vec())));

    assert_eq!(
        processor.process(original.clone(), "hero.png"),
        unchanged(&original, "png")
    );
}

#[test]
fn libwebp_reports_a_picture_it_cannot_encode_as_an_error() {
    let too_tall = RgbaImage::from_pixel(1, 16384, Rgba([0, 0, 0, 255]));

    assert!(Libwebp.encode_webp(&too_tall, 90.0).is_err());
}

#[test]
fn libwebp_refuses_a_width_its_row_length_cannot_hold() {
    for width in [1 << 29, 1 << 31] {
        let empty = RgbaImage::new(width, 0);

        let refused = Libwebp.encode_webp(&empty, 90.0).expect_err("refused");

        assert!(refused.contains("too large"), "{refused}");
    }
}

#[test]
fn libwebp_encodes_a_picture_that_decodes_back_to_its_size() {
    let picture = photo_like(40, 24);

    let encoded = Libwebp.encode_webp(&picture, 90.0).expect("encodes");

    assert_eq!(decoded(&encoded).dimensions(), (40, 24));
}

#[test]
fn an_encoder_that_fails_keeps_the_original() {
    let original = png(&photo_like(32, 32));
    let processor = WebpImages::new(Fixed(Err("out of memory".to_owned())));

    assert_eq!(
        processor.process(original.clone(), "hero.png"),
        unchanged(&original, "png")
    );
}

#[test]
fn a_jpeg_becomes_a_webp_too() {
    let picture = image::DynamicImage::ImageRgba8(photo_like(256, 256)).to_rgb8();
    let mut original = Cursor::new(Vec::new());
    picture
        .write_to(&mut original, ImageFormat::Jpeg)
        .expect("encodes");
    let original = original.into_inner();

    let processed = compact(&original, "photo.jpg");

    assert_eq!(processed.extension, "webp");
}

#[test]
fn an_encoding_with_other_dimensions_is_refused() {
    let original = png(&photo_like(32, 32));
    let smaller = lossless_webp(&photo_like(1, 1));
    let processor = WebpImages::new(Fixed(Ok(smaller)));

    assert_eq!(
        processor.process(original.clone(), "hero.png"),
        unchanged(&original, "png")
    );
}

#[test]
fn an_encoding_below_30_db_is_refused() {
    let mut next = noise(99);
    let picture = RgbaImage::from_fn(128, 128, |_, _| Rgba([next(), next(), next(), 255]));
    let original = png(&picture);

    assert_eq!(compact(&original, "noise.png"), unchanged(&original, "png"));
}

#[test]
fn an_encoding_larger_than_the_original_is_refused() {
    let original = png(&RgbaImage::from_pixel(16, 16, Rgba([9, 9, 9, 255])));
    let processor = WebpImages::new(Padded {
        padding: original.len() * 4,
    });

    assert_eq!(
        processor.process(original.clone(), "flat.png"),
        unchanged(&original, "png")
    );
}

#[test]
fn an_image_taller_than_webp_allows_keeps_the_original() {
    let original = png(&RgbaImage::from_pixel(1, 16384, Rgba([200, 30, 30, 255])));
    let processor = WebpImages::new(Forbidden);

    assert_eq!(
        processor.process(original.clone(), "tall.png"),
        unchanged(&original, "png")
    );
}

#[test]
fn garbage_colour_under_full_transparency_does_not_fail_the_gate() {
    let mut next = noise(5);
    let mut picture = photo_like(128, 128);
    for (x, _, pixel) in picture.enumerate_pixels_mut() {
        if x < 64 {
            *pixel = Rgba([next(), next(), next(), 0]);
        }
    }
    let original = png(&picture);

    assert_eq!(compact(&original, "cut-out.png").extension, "webp");
}

#[test]
fn an_image_with_transparency_keeps_its_alpha() {
    let mut picture = photo_like(128, 128);
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        pixel.0[3] = ((x + y) * 255 / 254) as u8;
    }
    let original = png(&picture);

    let processed = compact(&original, "faded.png");

    assert_eq!(processed.extension, "webp");
    let alphas = |image: &RgbaImage| image.pixels().map(|pixel| pixel.0[3]).collect::<Vec<_>>();
    assert_eq!(alphas(&decoded(&processed.bytes)), alphas(&picture));
}

// =========================================================
// What stays as it is
// =========================================================

#[test]
fn exact_keeps_every_image_byte_for_byte() {
    let original = png(&photo_like(64, 64));

    let processed =
        processor_for(ShownKind::Image, KeepFiles::Exact).process(original.clone(), "a.PNG");

    assert_eq!(processed, unchanged(&original, "png"));
}

#[test]
fn non_image_kinds_are_kept_as_they_are_under_compact() {
    for (kind, name, extension) in [
        (ShownKind::Code, "/src/main.rs", "rs"),
        (ShownKind::Markdown, "notes.md", "md"),
        (ShownKind::Ansi, "build.ansi", "ansi"),
        (ShownKind::DiffSide, "lib.py", "py"),
        (ShownKind::Code, "/repo/Makefile", "dat"),
        (ShownKind::Code, "payload.weirdext", "dat"),
    ] {
        let original = b"fn main() {}\n";
        let processed = processor_for(kind, KeepFiles::Compact).process(original.to_vec(), name);
        assert_eq!(processed, unchanged(original, extension), "{name}");
    }
}

#[test]
fn an_already_webp_stays_as_it_is() {
    let original = lossless_webp(&photo_like(32, 32));

    assert_eq!(compact(&original, "a.webp"), unchanged(&original, "webp"));
}

#[test]
fn a_webp_named_png_stays_as_it_is() {
    let original = lossless_webp(&photo_like(32, 32));

    assert_eq!(compact(&original, "a.png"), unchanged(&original, "png"));
}

#[test]
fn a_gif_stays_as_it_is() {
    let original = encoded(&photo_like(32, 32), ImageFormat::Gif);

    assert_eq!(compact(&original, "a.gif"), unchanged(&original, "gif"));
}

#[test]
fn an_animated_png_stays_as_it_is() {
    let original = apng(&photo_like(64, 64));

    assert_eq!(compact(&original, "a.png"), unchanged(&original, "png"));
}

// =========================================================
// A file kept as it is is not copied
// =========================================================

#[test]
fn a_file_kept_as_it_is_comes_back_in_the_allocation_it_went_in_with() {
    let original = b"fn main() {}\n".to_vec();
    let address = original.as_ptr();

    let processed = AsIs.process(original, "main.rs");

    assert_eq!(processed.bytes.as_ptr(), address);
}

#[test]
fn an_image_that_stays_as_it_is_comes_back_in_the_allocation_it_went_in_with() {
    let original = png(&photo_like(32, 32));
    let address = original.as_ptr();
    let processor = WebpImages::new(Fixed(Err("out of memory".to_owned())));

    let processed = processor.process(original, "hero.png");

    assert_eq!(processed.extension, "png");
    assert_eq!(processed.bytes.as_ptr(), address);
}
