// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Warnings about a session file (spec section 3.4): things that do not make
//! it invalid but are likely mistakes. Parsing ignores unknown fields so
//! later versions can add some; a typo such as `requred` would then be
//! silently lost, so `asqr validate` and `asqr ask` report them. The files
//! that context entries name are checked here too, since the TUI only
//! shows a placeholder for a file it cannot read.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ContextEntry, FieldPath, Session, session_schema};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub path: FieldPath,
    pub kind: WarningKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    UnknownField,
    /// The `image` field of format version 1 on a question or an option.
    /// An agent working from an older skill writes it, and the generic
    /// unknown-field warning would not tell it where the image goes.
    ImageField,
    RelativePath,
    FileNotFound(PathBuf),
    /// The path exists but names a directory or another file that is not
    /// a regular file, such as a FIFO.
    NotAFile(PathBuf),
    /// A side of a diff holds a unified diff instead of a version of the
    /// text. asqr would diff it like any other text and show a diff of the
    /// diff.
    UnifiedDiff,
    /// A code, Markdown or diff entry above 1 MiB. Carries its size in
    /// bytes.
    LargeEntry(u64),
    /// The file exists but asqr has no decoder for it, or its header is
    /// broken. Carries the reason.
    ImageNotDecodable(String),
}

impl fmt::Display for Warning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", self.path)?;
        match &self.kind {
            WarningKind::UnknownField => formatter.write_str("unknown field"),
            WarningKind::ImageField => formatter.write_str(
                r#"ignored; since format version 2 an image is a context entry: "context": [{"type": "image", "path": ...}]"#,
            ),
            WarningKind::RelativePath => formatter.write_str(
                "relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path",
            ),
            WarningKind::FileNotFound(path) => write!(formatter, "file not found: {}", path.display()),
            WarningKind::NotAFile(path) => write!(formatter, "not a file: {}", path.display()),
            WarningKind::UnifiedDiff => formatter.write_str(
                "looks like a unified diff; give the text before and after the change as `old` and `new`, asqr computes the diff",
            ),
            WarningKind::LargeEntry(size) => {
                // Rounded up, so a size just above the limit does not read
                // as the limit itself.
                let tenths = size.saturating_mul(10).div_ceil(LARGE_ENTRY_BYTES);
                write!(
                    formatter,
                    "{}.{} MiB, more than the 1 MiB an entry should stay under; asqr still shows it",
                    tenths / 10,
                    tenths % 10
                )
            }
            WarningKind::ImageNotDecodable(reason) => write!(formatter, "cannot be shown: {reason}"),
        }
    }
}

/// The warnings for a session. `raw` is the file as parsed JSON, which
/// still holds the unknown fields the typed `session` dropped. `base_dir` is
/// the directory of the file, against which relative paths are checked;
/// without it only absolute paths are.
pub fn warnings(raw: &Value, session: &Session, base_dir: Option<&Path>) -> Vec<Warning> {
    let mut found = Vec::new();
    unknown_fields(raw, &mut found);
    image_fields(raw, &mut found);
    context_files(session, base_dir, &mut found);
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
    /// The fields of each kind of context entry, by its `type`, including
    /// `type` itself.
    context: BTreeMap<String, BTreeSet<String>>,
    diff_side: BTreeSet<String>,
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
        // Each variant of the tagged entry refers to the definition of its
        // fields and pins `type` to its name.
        let variants = schema["$defs"]["ContextEntry"]["oneOf"].as_array();
        let context = variants
            .into_iter()
            .flatten()
            .filter_map(|variant| {
                let kind = variant["properties"]["type"]["const"].as_str()?;
                let reference = variant["$ref"].as_str()?;
                let name = reference.rsplit('/').next()?;
                let mut fields = definition(name);
                fields.insert("type".to_owned());
                // A `path` or `content` on a diff itself is a validation
                // error that says where the texts go (`DiffEntry`), so it
                // does not count as unknown as well.
                if kind == "diff" {
                    fields.extend(["path".to_owned(), "content".to_owned()]);
                }
                Some((kind.to_owned(), fields))
            })
            .collect();
        // `image` gets a warning of its own (`image_fields`), so it does not
        // count as unknown as well.
        let with_image = |mut fields: BTreeSet<String>| {
            fields.insert("image".to_owned());
            fields
        };
        KnownFields {
            session: names(&schema),
            question: with_image(definition("Question")),
            choice: with_image(definition("Choice")),
            custom: definition("CustomConfig"),
            length: definition("Length"),
            context,
            diff_side: definition("DiffSide"),
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
        report_context(question, &known, &path, &mut report);

        let options = question["options"].as_array().into_iter().flatten();
        for (index, option) in options.enumerate() {
            let path = path.field("options").index(index);
            report(option, &known.choice, Some(&path));
            report_context(option, &known, &path, &mut report);
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

/// Reports the `image` field of format version 1 on questions and options.
fn image_fields(raw: &Value, found: &mut Vec<Warning>) {
    let mut report = |holder: &Value, path: FieldPath| {
        if holder.get("image").is_some() {
            found.push(Warning {
                path: path.field("image"),
                kind: WarningKind::ImageField,
            });
        }
    };
    let questions = raw["questions"].as_array().into_iter().flatten();
    for (index, question) in questions.enumerate() {
        let path = FieldPath::root("questions").index(index);
        report(question, path.clone());
        let options = question["options"].as_array().into_iter().flatten();
        for (index, option) in options.enumerate() {
            report(option, path.field("options").index(index));
        }
    }
}

/// Reports the unknown fields of the context entries of a question or an
/// option. An entry of an unknown `type` fails parsing anyway, so only the
/// known kinds are looked at.
fn report_context(
    holder: &Value,
    known: &KnownFields,
    path: &FieldPath,
    report: &mut impl FnMut(&Value, &BTreeSet<String>, Option<&FieldPath>),
) {
    let entries = holder["context"].as_array().into_iter().flatten();
    for (index, entry) in entries.enumerate() {
        let path = path.field("context").index(index);
        let Some(fields) = entry["type"]
            .as_str()
            .and_then(|kind| known.context.get(kind))
        else {
            continue;
        };
        report(entry, fields, Some(&path));
        if entry["type"] == "diff" {
            for side in ["old", "new"] {
                report(&entry[side], &known.diff_side, Some(&path.field(side)));
            }
        }
    }
}

/// Checks every file a context entry names: that its path is absolute, that
/// it exists, and for an image, that asqr can decode it.
fn context_files(session: &Session, base_dir: Option<&Path>, found: &mut Vec<Warning>) {
    for (index, question) in session.questions.iter().enumerate() {
        let question_path = FieldPath::root("questions").index(index);
        check_context(&question.context, &question_path, base_dir, found);
        for (option_index, option) in question.options.iter().flatten().enumerate() {
            let option_path = question_path.field("options").index(option_index);
            check_context(&option.context, &option_path, base_dir, found);
        }
    }
}

fn check_context(
    context: &[ContextEntry],
    holder: &FieldPath,
    base_dir: Option<&Path>,
    found: &mut Vec<Warning>,
) {
    for (index, entry) in context.iter().enumerate() {
        let path = holder.field("context").index(index);
        match entry {
            ContextEntry::Image(image) => {
                if let Some(file) = check_file(&image.path, &path.field("path"), base_dir, found) {
                    check_decodable(&file, path.field("path"), found);
                }
            }
            ContextEntry::Code(code) => {
                let (size, _) = source_size(&code.path, &code.content, &path, base_dir, found);
                check_size(size, &path, found);
            }
            ContextEntry::Markdown(markdown) => {
                let (size, _) =
                    source_size(&markdown.path, &markdown.content, &path, base_dir, found);
                check_size(size, &path, found);
            }
            ContextEntry::Diff(diff) => {
                // The viewer works on both texts, so they count together.
                let mut size = 0;
                let sides = [("old", &diff.old), ("new", &diff.new)];
                for (field, side) in sides {
                    let Some(side) = side else { continue };
                    let side_path = path.field(field);
                    let (side_size, file) =
                        source_size(&side.path, &side.content, &side_path, base_dir, found);
                    size += side_size;
                    check_unified_diff(side.content.as_deref(), file.as_deref(), side_path, found);
                }
                check_size(size, &path, found);
            }
        }
    }
}

/// The size in bytes of text given as a file or inline, and the file it
/// resolves to. A file is checked first; one that cannot be found counts
/// as empty, as it has its own warning.
fn source_size(
    file: &Option<String>,
    content: &Option<String>,
    path: &FieldPath,
    base_dir: Option<&Path>,
    found: &mut Vec<Warning>,
) -> (u64, Option<PathBuf>) {
    let resolved = file
        .as_ref()
        .and_then(|file| check_file(file, &path.field("path"), base_dir, found));
    let file_size = resolved
        .as_ref()
        .and_then(|resolved| std::fs::metadata(resolved).ok())
        .map_or(0, |metadata| metadata.len());
    let content_size = content.as_ref().map_or(0, |content| content.len() as u64);
    (file_size + content_size, resolved)
}

/// Warns when a side of a diff holds a unified diff, which an agent with
/// a patch at hand writes there instead of the two versions. A file is
/// only read up to the size an entry should stay under: a larger one has
/// its own warning, and reading it would slow `ask` down.
fn check_unified_diff(
    content: Option<&str>,
    file: Option<&Path>,
    path: FieldPath,
    found: &mut Vec<Warning>,
) {
    let from_file = || {
        let file = file?;
        let size = std::fs::metadata(file).ok()?.len();
        if size > LARGE_ENTRY_BYTES {
            return None;
        }
        let bytes = std::fs::read(file).ok()?;
        Some(Cow::Owned(String::from_utf8_lossy(&bytes).into_owned()))
    };
    let text = match content {
        Some(content) => Some(Cow::Borrowed(content)),
        None => from_file(),
    };
    if text.is_some_and(|text| looks_like_unified_diff(&text)) {
        found.push(Warning {
            path,
            kind: WarningKind::UnifiedDiff,
        });
    }
}

/// A text counts as a unified diff when a line is a full hunk header, or
/// starts with `@@ -` after a file header (`--- `, `+++ `) or the
/// `diff --git` line that `git diff` starts with. The full header alone
/// catches a hunk copied out of `git diff` without its file headers. A
/// text that only mentions a hunk header inside a line, such as
/// documentation about diffs, does not count.
fn looks_like_unified_diff(text: &str) -> bool {
    let mut after_header = false;
    for line in text.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("diff --git") {
            after_header = true;
        } else if is_hunk_header(line) || (after_header && line.starts_with("@@ -")) {
            return true;
        }
    }
    false
}

/// Whether `line` is a hunk header, `@@ -N[,N] +N[,N] @@`, optionally
/// followed by a blank and the section text that `git diff` adds, such as
/// the enclosing function.
fn is_hunk_header(line: &str) -> bool {
    fn strip_number(text: &str) -> Option<&str> {
        let rest = text.trim_start_matches(|c: char| c.is_ascii_digit());
        (rest.len() < text.len()).then_some(rest)
    }
    // A range is a line number with an optional count: `12` or `12,3`.
    fn strip_range(text: &str) -> Option<&str> {
        let rest = strip_number(text)?;
        match rest.strip_prefix(',') {
            Some(count) => strip_number(count),
            None => Some(rest),
        }
    }
    let rest = line
        .strip_prefix("@@ -")
        .and_then(strip_range)
        .and_then(|rest| rest.strip_prefix(" +"))
        .and_then(strip_range)
        .and_then(|rest| rest.strip_prefix(" @@"));
    rest.is_some_and(|section| section.is_empty() || section.starts_with(' '))
}

/// Above this size a code, Markdown or diff entry gets a warning.
/// asqr still shows it. Images are exempt: screenshots above it are common,
/// and the image viewer handles them.
const LARGE_ENTRY_BYTES: u64 = 1024 * 1024;

fn check_size(size: u64, path: &FieldPath, found: &mut Vec<Warning>) {
    if size > LARGE_ENTRY_BYTES {
        found.push(Warning {
            path: path.clone(),
            kind: WarningKind::LargeEntry(size),
        });
    }
}

/// Warns about a relative or missing file, or a path that names no regular
/// file, and returns the existing file it resolves to.
fn check_file(
    file: &str,
    path: &FieldPath,
    base_dir: Option<&Path>,
    found: &mut Vec<Warning>,
) -> Option<PathBuf> {
    let file = Path::new(file);
    let resolved = if file.is_absolute() {
        Some(file.to_path_buf())
    } else {
        found.push(Warning {
            path: path.clone(),
            kind: WarningKind::RelativePath,
        });
        base_dir.map(|base| base.join(file))
    }?;
    // `metadata` follows symbolic links, so a link to a file is a file and
    // a dangling link is not found.
    let problem = match std::fs::metadata(&resolved) {
        Ok(metadata) if metadata.is_file() => return Some(resolved),
        Ok(_) => WarningKind::NotAFile(resolved),
        Err(_) => WarningKind::FileNotFound(resolved),
    };
    found.push(Warning {
        path: path.clone(),
        kind: problem,
    });
    None
}

fn check_decodable(resolved: &Path, path: FieldPath, found: &mut Vec<Warning>) {
    // The terminal UI would show a placeholder for this file, and a broken
    // image never becomes an error result (spec section 8.3), so the asker
    // only learns about it here. The file is
    // opened the way `graphics/mod.rs` opens it, but only its header
    // is read: that is enough to find a missing decoder, as for SVG or
    // AVIF, without decoding every pixel of a large image.
    let header = image::ImageReader::open(resolved)
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
                "questions[0].context[0].path: file not found: /tmp/asqr-example/301-holly-crown-green-robe-feast-ghost.png"
            ]
        );
    }

    #[test]
    fn reports_unknown_fields_at_every_level() {
        let json = r#"{"asqr": 2, "future": 1, "questions": [
            {"id": "s", "text": "?", "kind": "single", "requred": true,
             "options": [{"id": "a", "label": "A", "colour": "red"}],
             "custom": {"label": "Own", "x": 1, "length": {"warn": 9, "z": 2}}},
            {"id": "t", "text": "?", "kind": "text", "length": {"warn": 9, "y": 3}},
            {"id": "m", "text": "?", "kind": "multi", "custom": true,
             "context": [{"type": "code", "content": "x", "lang": "rust"}],
             "options": [{"id": "a", "label": "A",
                          "context": [{"type": "diff", "nmae": "a.rs", "old": {"content": "a", "p": 1},
                                       "new": {"content": "b"}}]},
                         {"id": "b", "label": "B",
                          "context": [{"type": "image", "path": "/b.png", "alt": "B", "old": {"x": 1}}]}]}]}"#;

        assert_eq!(
            warn(json, None),
            [
                "future: unknown field",
                "questions[0].requred: unknown field",
                "questions[0].options[0].colour: unknown field",
                "questions[0].custom.x: unknown field",
                "questions[0].custom.length.z: unknown field",
                "questions[1].length.y: unknown field",
                "questions[2].context[0].lang: unknown field",
                "questions[2].options[0].context[0].nmae: unknown field",
                "questions[2].options[0].context[0].old.p: unknown field",
                "questions[2].options[1].context[0].alt: unknown field",
                "questions[2].options[1].context[0].old: unknown field",
                "questions[2].options[1].context[0].path: file not found: /b.png",
            ]
        );
    }

    #[test]
    fn the_dropped_max_multiline_and_required_are_unknown_fields() {
        let json = r#"{"asqr": 2, "questions": [
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
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "text",
                "context": [{{"type": "image", "path": {image:?}}}]}}]}}"#,
            image = image.display().to_string()
        )
    }

    const RELATIVE: &str = "relative path; `asqr ask` makes it absolute, a file dropped into an inbox by hand must use an absolute path";

    #[test]
    fn warns_about_relative_paths() {
        let directory = directory_with_png();
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "image", "path": "picture.png"}]}]}"#;

        // Relative to the file's directory the image exists, but a relative
        // path means nothing once the file sits in an inbox.
        assert_eq!(
            warn(json, Some(directory.path())),
            [format!("questions[0].context[0].path: {RELATIVE}")]
        );
    }

    #[test]
    fn checks_that_relative_files_exist_where_the_session_file_is() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "image", "path": "missing.png"}]}]}"#;

        assert_eq!(
            warn(json, Some(repo())),
            [
                format!("questions[0].context[0].path: {RELATIVE}"),
                format!(
                    "questions[0].context[0].path: file not found: {}",
                    repo().join("missing.png").display()
                ),
            ]
        );
    }

    #[test]
    fn checks_every_file_of_code_markdown_and_diff_entries_on_questions_and_options() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "context": [{"type": "markdown", "path": "missing.md"}],
            "options": [
                {"id": "a", "label": "A", "context": [{"type": "diff",
                    "old": {"path": "Cargo.toml"}, "new": {"path": "/nonexistent/Cargo.toml"}}]},
                {"id": "b", "label": "B", "context": [{"type": "diff", "new": {"content": "inline"}}]},
                {"id": "c", "label": "C", "context": [{"type": "code", "content": "inline"}]},
                {"id": "d", "label": "D", "context": [{"type": "code", "path": "/nonexistent/fetch.rs"}]}]}]}"#;

        assert_eq!(
            warn(json, Some(repo())),
            [
                format!("questions[0].context[0].path: {RELATIVE}"),
                format!(
                    "questions[0].context[0].path: file not found: {}",
                    repo().join("missing.md").display()
                ),
                format!("questions[0].options[0].context[0].old.path: {RELATIVE}"),
                "questions[0].options[0].context[0].new.path: file not found: /nonexistent/Cargo.toml"
                    .to_owned(),
                "questions[0].options[3].context[0].path: file not found: /nonexistent/fetch.rs"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn only_image_entries_are_checked_for_a_decoder() {
        // An existing text file is no image, but a code entry shows text.
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "text",
                "context": [{{"type": "code", "path": {path:?}}}]}}]}}"#,
            path = repo().join("Cargo.toml").display().to_string()
        );

        assert_eq!(warn(&json, None), Vec::<String>::new());
    }

    /// A session with one question carrying `entry` as its context.
    fn with_context(entry: &serde_json::Value) -> String {
        format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "text", "context": [{entry}]}}]}}"#
        )
    }

    const MIB: usize = 1024 * 1024;

    #[test]
    fn warns_about_a_code_or_markdown_entry_above_1_mib_inline_or_in_a_file() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let log = directory.path().join("build.log");
        std::fs::write(&log, "x".repeat(3 * MIB / 2)).expect("log is writable");

        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "code", "content": "x".repeat(MIB + 1)})),
                None
            ),
            [
                "questions[0].context[0]: 1.1 MiB, more than the 1 MiB an entry should stay under; asqr still shows it"
            ]
        );
        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "markdown", "path": log})),
                None
            ),
            [
                "questions[0].context[0]: 1.5 MiB, more than the 1 MiB an entry should stay under; asqr still shows it"
            ]
        );
        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "markdown", "content": "x".repeat(MIB)})),
                None
            ),
            Vec::<String>::new(),
            "exactly 1 MiB is fine"
        );
    }

    #[test]
    fn a_diff_counts_both_sides_together() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let old = directory.path().join("old.rs");
        std::fs::write(&old, "x".repeat(MIB / 2)).expect("file is writable");
        let half_and_more = "y".repeat(MIB / 2 + 1);

        assert_eq!(
            warn(
                &with_context(&serde_json::json!(
                    {"type": "diff", "old": {"path": old}, "new": {"content": half_and_more}})),
                None
            ),
            [
                "questions[0].context[0]: 1.1 MiB, more than the 1 MiB an entry should stay under; asqr still shows it"
            ]
        );
        assert_eq!(
            warn(
                &with_context(
                    &serde_json::json!({"type": "diff", "new": {"content": half_and_more}})
                ),
                None
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_large_image_gets_no_size_warning() {
        // Screenshots above 1 MiB are common, and the image viewer handles
        // them.
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let large = directory.path().join("large.png");
        // Noise does not compress, so the file stays near its 3 MiB of
        // pixels.
        let mut state: u32 = 0x9e37_79b9;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        };
        image::RgbImage::from_fn(1024, 1024, |_, _| image::Rgb([noise(), noise(), noise()]))
            .save(&large)
            .expect("png is writable");
        assert!(std::fs::metadata(&large).expect("written").len() > MIB as u64);

        assert_eq!(
            warn(&text_question_with_image(&large), None),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_image_field_of_format_version_1_says_where_images_go() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "image": "/q.png", "options": [{"id": "a", "label": "A", "image": "/a.png"}]}]}"#;

        let moved = r#"ignored; since format version 2 an image is a context entry: "context": [{"type": "image", "path": ...}]"#;
        assert_eq!(
            warn(json, None),
            [
                format!("questions[0].image: {moved}"),
                format!("questions[0].options[0].image: {moved}"),
            ]
        );
    }

    #[test]
    fn render_and_an_inline_text_key_are_unknown_fields() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "single",
            "context": [{"type": "code", "text": "fn main() {}", "content": "fn main() {}"}],
            "options": [
                {"id": "a", "label": "A", "context": [{"type": "markdown", "content": "*a*", "render": false}]},
                {"id": "b", "label": "B", "context": [{"type": "diff",
                    "old": {"text": "a", "content": "a"}, "new": {"content": "b"}}]}]}]}"#;

        assert_eq!(
            warn(json, None),
            [
                "questions[0].context[0].text: unknown field",
                "questions[0].options[0].context[0].render: unknown field",
                "questions[0].options[1].context[0].old.text: unknown field",
            ]
        );
    }

    #[test]
    fn a_directory_given_as_a_file_is_not_a_file() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let json = with_context(&serde_json::json!({"type": "code", "path": directory.path()}));

        assert_eq!(
            warn(&json, None),
            [format!(
                "questions[0].context[0].path: not a file: {}",
                directory.path().display()
            )]
        );
    }

    const UNIFIED: &str = "looks like a unified diff; give the text before and after the change as `old` and `new`, asqr computes the diff";

    /// A unified diff as `git diff` writes it.
    const GIT_DIFF: &str = "diff --git a/fetch.rs b/fetch.rs\nindex 1..2 100644\n--- a/fetch.rs\n+++ b/fetch.rs\n@@ -1,2 +1,2 @@\n fn fetch() {\n-    get(url)\n+    get(uri)\n";

    #[test]
    fn warns_about_a_diff_side_that_holds_a_unified_diff_inline_or_in_a_file() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let patch = directory.path().join("change.patch");
        // `diff -u` output starts with the file headers, without the git
        // line.
        std::fs::write(&patch, "--- a.rs\n+++ b.rs\n@@ -1 +1 @@\n-a\n+b\n").expect("written");

        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "diff",
                    "old": {"content": "fn fetch() {}"}, "new": {"content": GIT_DIFF}})),
                None
            ),
            [format!("questions[0].context[0].new: {UNIFIED}")]
        );
        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "diff", "old": {"path": patch}})),
                None
            ),
            [format!("questions[0].context[0].old: {UNIFIED}")]
        );
    }

    #[test]
    fn a_bare_hunk_is_a_unified_diff() {
        // A hunk copied out of `git diff` has no file headers. A full hunk
        // header at a line start is rare enough in other text to count on
        // its own.
        for text in [
            "@@ -1,2 +1,2 @@\n-a\n+b\n",
            "@@ -12 +12,3 @@ fn fetch() {\n-    get(url)\n+    get(uri)\n",
            "Before the change:\n@@ -0,0 +1 @@\n+a\n",
            "@@ -1 +1 @@\n--- a.rs\n+++ b.rs\n",
        ] {
            assert_eq!(
                warn(
                    &with_context(&serde_json::json!({"type": "diff", "new": {"content": text}})),
                    None
                ),
                [format!("questions[0].context[0].new: {UNIFIED}")],
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_text_that_only_mentions_a_hunk_header_is_no_unified_diff() {
        for text in [
            // File headers, but the hunk header is not at a line start.
            "--- a.rs\n+++ b.rs\nsee @@ -1 +1 @@ in the patch\n",
            "Markdown uses `--- ` for a rule; `@@ -1 +1 @@` is a hunk header.",
            "`@@ -1 +1 @@` starts a hunk.\n",
            // Lines that start like a hunk header but are none.
            "@@ -foo\n",
            "@@ -1 +1\n",
            "@@ -1,x +1 @@\n",
            "@@ -1 +1 @@section\n",
            "@@ - +1 @@\n",
        ] {
            assert_eq!(
                warn(
                    &with_context(&serde_json::json!({"type": "diff", "new": {"content": text}})),
                    None
                ),
                Vec::<String>::new(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_unified_diff_in_code_or_markdown_entries_is_fine() {
        // Showing a patch as code is a choice, not a mistake.
        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "code", "content": GIT_DIFF})),
                None
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_side_file_above_the_size_limit_is_not_read_for_the_check() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let large = directory.path().join("large.patch");
        let mut text = GIT_DIFF.to_owned();
        text.push_str(&"+x\n".repeat(MIB / 3 + 1));
        std::fs::write(&large, text).expect("written");

        let warnings = warn(
            &with_context(&serde_json::json!({"type": "diff", "new": {"path": large}})),
            None,
        );

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("MiB"), "{warnings:?}");
    }

    #[test]
    fn path_and_content_on_a_diff_are_an_error_not_unknown_fields() {
        // Validation reports them with a hint where the texts go, so the
        // generic warning would only repeat the problem. A diff has no
        // `language`, as it shows without highlighting.
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "diff", "path": "/change.diff", "content": "x", "language": "rust",
                         "new": {"content": "b"}}]}]}"#;

        assert_eq!(
            warn(json, None),
            ["questions[0].context[0].language: unknown field"]
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
                "questions[0].context[0].path: cannot be shown: unsupported image format, use one of {DECODABLE_FORMATS}"
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
                "questions[0].context[0].path: cannot be shown: unsupported image format, use one of {DECODABLE_FORMATS}"
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
            warnings[0].starts_with("questions[0].context[0].path: cannot be shown: "),
            "{warnings:?}"
        );
    }

    /// `DECODABLE_FORMATS`, SKILL.md, the `image` field's description and
    /// spec section 8.8 name these formats. The set follows the `image`
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
