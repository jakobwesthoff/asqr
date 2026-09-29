// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Warnings about a session file (spec section 4): things that do not make
//! it invalid but are likely mistakes. Parsing ignores unknown fields so
//! later versions can add some; a typo such as `requred` would then be
//! silently lost, so `asqr validate` and `asqr ask` report them.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{FieldPath, Session, session_schema};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub path: FieldPath,
    pub kind: WarningKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    UnknownField,
    RelativeImagePath,
    ImageNotFound(PathBuf),
    /// The file exists but asqr has no decoder for it, or its header is
    /// broken. Carries the reason.
    ImageNotDecodable(String),
}

impl fmt::Display for Warning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", self.path)?;
        match &self.kind {
            WarningKind::UnknownField => formatter.write_str("unknown field"),
            WarningKind::RelativeImagePath => formatter.write_str(
                "relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path",
            ),
            WarningKind::ImageNotFound(path) => write!(formatter, "file not found: {}", path.display()),
            WarningKind::ImageNotDecodable(reason) => write!(formatter, "cannot be shown: {reason}"),
        }
    }
}

/// The warnings for a session. `raw` is the file as parsed JSON, which
/// still holds the unknown fields the typed `session` dropped. `base_dir` is
/// the directory of the file, against which relative image paths are
/// checked; without it only absolute paths are.
pub fn warnings(raw: &Value, session: &Session, base_dir: Option<&Path>) -> Vec<Warning> {
    let mut found = Vec::new();
    unknown_fields(raw, &mut found);
    images(session, base_dir, &mut found);
    found
}

/// The known field names of each object in the format, read from the
/// derived schema so they cannot fall out of step with the types.
struct KnownFields {
    session: BTreeSet<String>,
    question: BTreeSet<String>,
    choice: BTreeSet<String>,
    custom: BTreeSet<String>,
    length: BTreeSet<String>,
}

impl KnownFields {
    fn from_schema() -> Self {
        let schema = session_schema();
        let names = |object: &Value| -> BTreeSet<String> {
            object["properties"]
                .as_object()
                .map(|properties| properties.keys().cloned().collect())
                .unwrap_or_default()
        };
        let definition = |name: &str| names(&schema["$defs"][name]);
        KnownFields {
            session: names(&schema),
            question: definition("Question"),
            choice: definition("Choice"),
            custom: definition("CustomConfig"),
            length: definition("Length"),
        }
    }
}

fn unknown_fields(raw: &Value, found: &mut Vec<Warning>) {
    let known = KnownFields::from_schema();
    let mut report = |object: &Value, fields: &BTreeSet<String>, path: Option<&FieldPath>| {
        for key in object
            .as_object()
            .into_iter()
            .flat_map(|object| object.keys())
        {
            if !fields.contains(key) {
                let path = match path {
                    Some(parent) => parent.field(key),
                    None => FieldPath::root(key),
                };
                found.push(Warning {
                    path,
                    kind: WarningKind::UnknownField,
                });
            }
        }
    };

    report(raw, &known.session, None);
    let questions = raw["questions"].as_array().into_iter().flatten();
    for (index, question) in questions.enumerate() {
        let path = FieldPath::root("questions").index(index);
        report(question, &known.question, Some(&path));

        let options = question["options"].as_array().into_iter().flatten();
        for (index, option) in options.enumerate() {
            report(
                option,
                &known.choice,
                Some(&path.field("options").index(index)),
            );
        }
        // `custom: true` is a plain switch; only the object form has fields.
        if question["custom"].is_object() {
            let custom = path.field("custom");
            report(&question["custom"], &known.custom, Some(&custom));
            report(
                &question["custom"]["length"],
                &known.length,
                Some(&custom.field("length")),
            );
        }
        report(
            &question["length"],
            &known.length,
            Some(&path.field("length")),
        );
    }
}

fn images(session: &Session, base_dir: Option<&Path>, found: &mut Vec<Warning>) {
    for (index, question) in session.questions.iter().enumerate() {
        let question_path = FieldPath::root("questions").index(index);
        if let Some(image) = &question.image {
            check_image(image, question_path.field("image"), base_dir, found);
        }
        for (option_index, option) in question.options.iter().flatten().enumerate() {
            if let Some(image) = &option.image {
                let path = question_path
                    .field("options")
                    .index(option_index)
                    .field("image");
                check_image(image, path, base_dir, found);
            }
        }
    }
}

fn check_image(image: &str, path: FieldPath, base_dir: Option<&Path>, found: &mut Vec<Warning>) {
    let image = Path::new(image);
    let resolved = if image.is_absolute() {
        Some(image.to_path_buf())
    } else {
        found.push(Warning {
            path: path.clone(),
            kind: WarningKind::RelativeImagePath,
        });
        base_dir.map(|base| base.join(image))
    };
    let Some(resolved) = resolved else {
        return;
    };
    if !resolved.is_file() {
        found.push(Warning {
            path,
            kind: WarningKind::ImageNotFound(resolved),
        });
        return;
    }

    // The terminal UI would show a placeholder for this file, and a broken
    // image never becomes an error result (spec section 7.7), so the asker
    // only learns about it here. The file is
    // opened the way `tui/render/images.rs` opens it, but only its header
    // is read: that is enough to find a missing decoder, as for SVG or
    // AVIF, without decoding every pixel of a large image.
    let header = image::ImageReader::open(&resolved)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(image::ImageError::from)
        .and_then(|reader| reader.into_dimensions());
    let reason = match header {
        Ok(_) => return,
        // The crate's own wording names only what failed. An asker that
        // picked the wrong format needs to know which ones work.
        Err(image::ImageError::Unsupported(_)) => {
            format!("unsupported image format, use one of {DECODABLE_FORMATS}")
        }
        Err(error) => error.to_string(),
    };
    found.push(Warning {
        path,
        kind: WarningKind::ImageNotDecodable(reason),
    });
}

/// The formats the terminal UI can show, as the `image` crate's features
/// decide. GIF shows its first frame only.
const DECODABLE_FORMATS: &str =
    "PNG, JPEG, GIF, WebP, BMP, TIFF, ICO, TGA, PNM, QOI, DDS, OpenEXR, HDR and farbfeld";

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const EXAMPLE: &str = include_str!("../../examples/sessions/alt-rework-batch.json");

    fn warn(json: &str, base_dir: Option<&Path>) -> Vec<String> {
        let raw: serde_json::Value = serde_json::from_str(json).expect("test json parses");
        let session: Session = serde_json::from_value(raw.clone()).expect("test session parses");
        warnings(&raw, &session, base_dir)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn repo() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn the_example_only_warns_about_its_placeholder_image() {
        assert_eq!(
            warn(EXAMPLE, None),
            [
                "questions[0].image: file not found: /tmp/asqr-example/301-holly-crown-green-robe-feast-ghost.png"
            ]
        );
    }

    #[test]
    fn reports_unknown_fields_at_every_level() {
        let json = r#"{"asqr": 1, "future": 1, "questions": [
            {"id": "s", "text": "?", "kind": "single", "requred": true,
             "options": [{"id": "a", "label": "A", "colour": "red"}],
             "custom": {"label": "Own", "x": 1, "length": {"warn": 9, "z": 2}}},
            {"id": "t", "text": "?", "kind": "text", "length": {"warn": 9, "y": 3}},
            {"id": "m", "text": "?", "kind": "multi", "custom": true,
             "options": [{"id": "a", "label": "A"}]}]}"#;

        assert_eq!(
            warn(json, None),
            [
                "future: unknown field",
                "questions[0].requred: unknown field",
                "questions[0].options[0].colour: unknown field",
                "questions[0].custom.x: unknown field",
                "questions[0].custom.length.z: unknown field",
                "questions[1].length.y: unknown field",
            ]
        );
    }

    #[test]
    fn the_dropped_max_multiline_and_required_are_unknown_fields() {
        let json = r#"{"asqr": 1, "questions": [
            {"id": "t", "text": "?", "kind": "text", "length": {"max": 9}, "required": true},
            {"id": "s", "text": "?", "kind": "single", "options": [{"id": "a", "label": "A"}],
             "custom": {"multiline": true}}]}"#;

        assert_eq!(
            warn(json, None),
            [
                "questions[0].required: unknown field",
                "questions[0].length.max: unknown field",
                "questions[1].custom.multiline: unknown field",
            ]
        );
    }

    /// A directory holding a one-pixel `picture.png`.
    fn directory_with_png() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        image::RgbImage::new(1, 1)
            .save(directory.path().join("picture.png"))
            .expect("png is writable");
        directory
    }

    fn text_question_with_image(image: &Path) -> String {
        format!(
            r#"{{"asqr": 1, "questions": [{{"id": "q", "text": "?", "kind": "text", "image": {image:?}}}]}}"#,
            image = image.display().to_string()
        )
    }

    #[test]
    fn warns_about_relative_image_paths() {
        let directory = directory_with_png();
        let json = r#"{"asqr": 1, "questions": [
            {"id": "q", "text": "?", "kind": "text", "image": "picture.png"}]}"#;

        // Relative to the file's directory the image exists, but a relative
        // path means nothing once the file sits in an inbox.
        assert_eq!(
            warn(json, Some(directory.path())),
            [
                "questions[0].image: relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path"
            ]
        );
    }

    #[test]
    fn checks_that_relative_images_exist_where_the_file_is() {
        let json = r#"{"asqr": 1, "questions": [
            {"id": "q", "text": "?", "kind": "text", "image": "missing.png"}]}"#;

        assert_eq!(
            warn(json, Some(repo())),
            [
                "questions[0].image: relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path",
                &format!(
                    "questions[0].image: file not found: {}",
                    repo().join("missing.png").display()
                ),
            ]
        );
    }

    #[test]
    fn checks_option_images_like_question_images() {
        let json = r#"{"asqr": 1, "questions": [
            {"id": "q", "text": "?", "kind": "single",
             "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B", "image": "missing.png"}]}]}"#;

        assert_eq!(
            warn(json, Some(repo())),
            [
                "questions[0].options[1].image: relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path",
                &format!(
                    "questions[0].options[1].image: file not found: {}",
                    repo().join("missing.png").display()
                ),
            ]
        );
    }

    #[test]
    fn every_example_session_is_valid_and_uses_only_known_fields() {
        let examples = repo().join("examples/sessions");
        let mut checked = 0;
        for entry in std::fs::read_dir(&examples).expect("examples directory exists") {
            let path = entry.expect("directory entry is readable").path();
            let json = std::fs::read_to_string(&path).expect("example is readable");
            let raw: serde_json::Value = serde_json::from_str(&json).expect("example is JSON");
            let session: Session = serde_json::from_value(raw.clone()).expect("example parses");

            assert_eq!(
                super::super::validate(&session, None),
                Ok(()),
                "{}",
                path.display()
            );
            let unknown: Vec<_> = warnings(&raw, &session, None)
                .into_iter()
                .filter(|warning| warning.kind == WarningKind::UnknownField)
                .collect();
            assert!(unknown.is_empty(), "{}: {unknown:?}", path.display());
            checked += 1;
        }
        assert!(checked >= 3, "only {checked} examples found");
    }

    #[test]
    fn an_existing_absolute_image_is_fine() {
        let directory = directory_with_png();
        let json = text_question_with_image(&directory.path().join("picture.png"));

        assert_eq!(warn(&json, None), Vec::<String>::new());
    }

    #[test]
    fn warns_about_an_image_without_a_decoder() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let svg = directory.path().join("diagram.svg");
        std::fs::write(&svg, r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#)
            .expect("svg is writable");
        let json = text_question_with_image(&svg);

        assert_eq!(
            warn(&json, None),
            [format!(
                "questions[0].image: cannot be shown: unsupported image format, use one of {DECODABLE_FORMATS}"
            )]
        );
    }

    #[test]
    fn warns_about_avif_although_the_format_is_recognised() {
        // The `image` crate recognises AVIF by its `ftyp` box but only
        // decodes it with the `avif-native` feature, which asqr leaves off.
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let avif = directory.path().join("photo.avif");
        std::fs::write(&avif, b"\0\0\0\x20ftypavif\0\0\0\0").expect("avif is writable");
        let json = text_question_with_image(&avif);

        assert_eq!(
            warn(&json, None),
            [format!(
                "questions[0].image: cannot be shown: unsupported image format, use one of {DECODABLE_FORMATS}"
            )]
        );
    }

    #[test]
    fn warns_about_a_file_whose_content_is_no_image() {
        // The extension names a supported format, but the header does not
        // parse, so the decoder refuses the file.
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let fake = directory.path().join("notes.png");
        std::fs::write(&fake, "just some text").expect("file is writable");
        let json = text_question_with_image(&fake);

        let warnings = warn(&json, None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("questions[0].image: cannot be shown: "),
            "{warnings:?}"
        );
    }

    /// `DECODABLE_FORMATS`, SKILL.md, the `image` field's description and
    /// spec section 7.7 name these formats. The set follows the `image`
    /// crate's features, so this test fails when they change until those
    /// texts follow.
    #[test]
    fn the_decodable_formats_are_the_documented_ones() {
        use image::{ImageError, ImageFormat, ImageReader};

        // `ImageFormat::reading_enabled` claims AVIF without its decoder and
        // denies DDS although it decodes, so the decoders are asked
        // directly. Empty input fails every decoder that is built in, but
        // never with `Unsupported`.
        let decodable: Vec<ImageFormat> = ImageFormat::all()
            .filter(|format| {
                let reader = ImageReader::with_format(std::io::Cursor::new(&[][..]), *format);
                !matches!(reader.into_dimensions(), Err(ImageError::Unsupported(_)))
            })
            .collect();

        assert_eq!(
            decodable,
            [
                ImageFormat::Gif,
                ImageFormat::Ico,
                ImageFormat::Jpeg,
                ImageFormat::Png,
                ImageFormat::Bmp,
                ImageFormat::Tiff,
                ImageFormat::Tga,
                ImageFormat::Pnm,
                ImageFormat::Farbfeld,
                ImageFormat::WebP,
                ImageFormat::OpenExr,
                ImageFormat::Qoi,
                ImageFormat::Dds,
                ImageFormat::Hdr,
            ]
        );
    }
}
