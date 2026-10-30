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
use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::{Path, PathBuf};

use image::error::{ImageFormatHint, UnsupportedError, UnsupportedErrorKind};
use image::{ImageError, ImageFormat, ImageReader};
use serde_json::Value;

use super::{
    ContextEntry, FieldPath, LineBreaks, MAX_BYTES, MAX_LINES, Session, line_cut, session_schema,
};
use crate::regular_file::{self, OpenError};

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
    /// `"note": false` on a question or an info item: every one takes a
    /// note, so the field is ignored (ADR 68).
    NoteIgnored,
    RelativePath,
    FileNotFound(PathBuf),
    /// The path exists but names a directory or another file that is not
    /// a regular file, such as a FIFO.
    NotAFile(PathBuf),
    /// A side of a diff holds a unified diff instead of a version of the
    /// text. asqr would diff it like any other text and show a diff of the
    /// diff.
    UnifiedDiff,
    /// A code, Markdown, ANSI or diff entry above 1 MiB. Carries its size
    /// in bytes, an ANSI entry's escape sequences included.
    LargeEntry(u64),
    /// A code, Markdown or ANSI entry, a side of a diff or an image's
    /// caption of more than [`MAX_LINES`] lines, which its viewer cuts.
    ManyLines,
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
            WarningKind::NoteIgnored => {
                formatter.write_str("false is ignored; every question takes a note")
            }
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
            WarningKind::ManyLines => formatter
                .write_str("more than 500,000 lines; asqr shows only the first 500,000"),
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
    note_fields(raw, &mut found);
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

/// Reports `"note": false` on a question, which no longer turns the note
/// off (ADR 68). `true` changes nothing either way and gets no warning.
fn note_fields(raw: &Value, found: &mut Vec<Warning>) {
    let questions = raw["questions"].as_array().into_iter().flatten();
    for (index, question) in questions.enumerate() {
        if question.get("note") == Some(&Value::Bool(false)) {
            found.push(Warning {
                path: FieldPath::root("questions").index(index).field("note"),
                kind: WarningKind::NoteIgnored,
            });
        }
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
                // The image file gets no size warning, its caption does: it
                // is text the pane renders like a Markdown entry.
                if let Some(caption) = &image.caption {
                    let caption_path = path.field("caption");
                    check_size(caption.len() as u64, &caption_path, found);
                    let text = Some(caption.as_str());
                    check_lines(text, None, LineBreaks::Any, &caption_path, found);
                }
            }
            ContextEntry::Code(code) => {
                let (size, file) = source_size(&code.path, &code.content, &path, base_dir, found);
                check_size(size, &path, found);
                let text = code.content.as_deref();
                check_lines(text, handle(&file), LineBreaks::Any, &path, found);
            }
            ContextEntry::Markdown(markdown) => {
                let (size, file) =
                    source_size(&markdown.path, &markdown.content, &path, base_dir, found);
                check_size(size, &path, found);
                let text = markdown.content.as_deref();
                check_lines(text, handle(&file), LineBreaks::Any, &path, found);
            }
            ContextEntry::Ansi(ansi) => {
                let (size, file) = source_size(&ansi.path, &ansi.content, &path, base_dir, found);
                check_size(size, &path, found);
                let text = ansi.content.as_deref();
                check_lines(text, handle(&file), LineBreaks::Newline, &path, found);
            }
            ContextEntry::Diff(diff) => {
                // The viewer works on both texts, so they count together.
                let mut size = 0;
                // Each side is cut by lines on its own, so its warning
                // names the side.
                let mut texts = Vec::new();
                let sides = [("old", &diff.old), ("new", &diff.new)];
                for (field, side) in sides {
                    let Some(side) = side else { continue };
                    let side_path = path.field(field);
                    let (side_size, file) =
                        source_size(&side.path, &side.content, &side_path, base_dir, found);
                    size += side_size;
                    let content = side.content.as_deref();
                    check_unified_diff(content, handle(&file), side_path.clone(), found);
                    texts.push((side_path, content, file));
                }
                check_size(size, &path, found);
                for (side_path, content, file) in texts {
                    check_lines(content, handle(&file), LineBreaks::Any, &side_path, found);
                }
            }
        }
    }
}

/// A context file that exists and is a regular file, opened once. Every
/// check reads from this handle and never opens the path again, as a path
/// swapped for a FIFO between two opens would hold the check for ever.
struct ContextFile {
    path: PathBuf,
    /// The open can fail on a regular file the system does not let asqr
    /// read. Such a file still has a size to warn about.
    opened: io::Result<File>,
}

impl ContextFile {
    /// The size in bytes, from the opened file where there is one. The path
    /// is only asked for its metadata, which never waits.
    fn size(&self) -> u64 {
        let metadata = match &self.opened {
            Ok(file) => file.metadata(),
            Err(_) => std::fs::metadata(&self.path),
        };
        metadata.map_or(0, |metadata| metadata.len())
    }
}

/// The opened file of `file`, if it has one.
fn handle(file: &Option<ContextFile>) -> Option<&File> {
    file.as_ref()?.opened.as_ref().ok()
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
) -> (u64, Option<ContextFile>) {
    let resolved = file
        .as_ref()
        .and_then(|file| check_file(file, &path.field("path"), base_dir, found));
    let file_size = resolved.as_ref().map_or(0, ContextFile::size);
    let content_size = content.as_ref().map_or(0, |content| content.len() as u64);
    (file_size + content_size, resolved)
}

/// Warns when a side of a diff holds a unified diff, which an agent with
/// a patch at hand writes there instead of the two versions. A file is
/// only read up to the size an entry should stay under: a larger one has
/// its own warning, and reading it would slow `ask` down.
fn check_unified_diff(
    content: Option<&str>,
    file: Option<&File>,
    path: FieldPath,
    found: &mut Vec<Warning>,
) {
    let from_file = || {
        let mut file = file?;
        let size = file.metadata().ok()?.len();
        if size > LARGE_ENTRY_BYTES {
            return None;
        }
        // Another check may have read the file before.
        file.rewind().ok()?;
        let mut bytes = Vec::new();
        file.take(LARGE_ENTRY_BYTES).read_to_end(&mut bytes).ok()?;
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

/// Above this size a code, Markdown, ANSI or diff entry gets a warning.
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

/// Warns when a viewer would cut a text at the line limit: `content`, or
/// else the file `file`, whose lines end at `breaks`.
fn check_lines(
    content: Option<&str>,
    file: Option<&File>,
    breaks: LineBreaks,
    path: &FieldPath,
    found: &mut Vec<Warning>,
) {
    let cut = match content {
        Some(content) => line_cut(content.as_bytes(), MAX_LINES, breaks).is_some(),
        None => file.is_some_and(|file| file_cut_by_lines(file, breaks)),
    };
    if cut {
        found.push(Warning {
            path: path.clone(),
            kind: WarningKind::ManyLines,
        });
    }
}

/// Whether a viewer would cut the file `file`, whose lines end at
/// `breaks`, at the line limit. It counts what a viewer reads, the first
/// [`MAX_BYTES`], and a binary file, which a viewer does not show, counts
/// as not cut. A file of no more bytes than [`MAX_LINES`] cannot hold more
/// lines, so it is not read; a larger one takes a read of at most 8 MiB.
fn file_cut_by_lines(file: &File, breaks: LineBreaks) -> bool {
    let size = file.metadata().map_or(0, |metadata| metadata.len());
    if size <= MAX_LINES as u64 {
        return false;
    }
    // Another check may have read the file before.
    let mut reader = file;
    let mut bytes = Vec::new();
    let read = reader
        .rewind()
        .and_then(|()| file.take(MAX_BYTES).read_to_end(&mut bytes));
    read.is_ok() && !bytes.contains(&0) && line_cut(&bytes, MAX_LINES, breaks).is_some()
}

/// Warns about a relative or missing file, or a path that names no regular
/// file, and returns the existing file it resolves to, opened.
fn check_file(
    file: &str,
    path: &FieldPath,
    base_dir: Option<&Path>,
    found: &mut Vec<Warning>,
) -> Option<ContextFile> {
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
    if let Some(problem) = type_problem(&resolved) {
        found.push(Warning {
            path: path.clone(),
            kind: problem,
        });
        return None;
    }
    open_listed(resolved, path, found)
}

/// What is wrong with the type of the file at `resolved`, if anything.
/// `metadata` follows symbolic links, so a link to a file is a file and a
/// dangling link is not found.
fn type_problem(resolved: &Path) -> Option<WarningKind> {
    match std::fs::metadata(resolved) {
        Ok(metadata) if metadata.is_file() => None,
        Ok(_) => Some(WarningKind::NotAFile(resolved.to_path_buf())),
        Err(_) => Some(WarningKind::FileNotFound(resolved.to_path_buf())),
    }
}

/// Opens the file at `resolved`, which looked like a regular file a moment
/// ago. It may have turned into something else since, such as a FIFO that
/// would hold the open for ever, so the open is the non-blocking one that
/// checks the type again on the file it opened.
fn open_listed(
    resolved: PathBuf,
    path: &FieldPath,
    found: &mut Vec<Warning>,
) -> Option<ContextFile> {
    let opened = match regular_file::open(&resolved) {
        Ok(file) => Ok(file),
        Err(OpenError::NotRegular { .. }) => {
            found.push(Warning {
                path: path.clone(),
                kind: WarningKind::NotAFile(resolved),
            });
            return None;
        }
        Err(OpenError::Io(error)) => {
            // A socket, or a device that refuses to be opened, fails here
            // before its type can be asked for. Only a regular file that
            // asqr may not read keeps its place among the checks.
            if let Some(problem) = type_problem(&resolved) {
                found.push(Warning {
                    path: path.clone(),
                    kind: problem,
                });
                return None;
            }
            Err(error)
        }
    };
    Some(ContextFile {
        path: resolved,
        opened,
    })
}

fn check_decodable(file: &ContextFile, path: FieldPath, found: &mut Vec<Warning>) {
    // The terminal UI would show a placeholder for this file, and a broken
    // image never becomes an error result (spec section 8.3), so the asker
    // only learns about it here. Only the header is read: that is enough
    // to find a missing decoder, as for SVG or AVIF, without decoding every
    // pixel of a large image.
    let reason = match &file.opened {
        // The system's own words, as the viewer's card has them.
        Err(error) => error.to_string(),
        Ok(opened) => match image_header(opened, &file.path) {
            Ok(_) => return,
            // The crate's own wording names only what failed. An asker that
            // picked the wrong format needs to know which ones work.
            Err(ImageError::Unsupported(_)) => {
                format!("unsupported image format, use one of {DECODABLE_FORMATS}")
            }
            Err(error) => error.to_string(),
        },
    };
    found.push(Warning {
        path,
        kind: WarningKind::ImageNotDecodable(reason),
    });
}

/// The dimensions of the image in `file`, which `path` names. The format
/// comes from the file's first bytes, else from the extension of `path`,
/// as the viewer decides it: TGA, for one, has no signature to recognise.
fn image_header(file: &File, path: &Path) -> Result<(u32, u32), ImageError> {
    let mut reader = ImageReader::new(BufReader::new(file)).with_guessed_format()?;
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
    reader.into_dimensions()
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

    /// The warnings about `entry` that concern a `caption`.
    fn caption_warnings(entry: &serde_json::Value) -> Vec<String> {
        warn(&with_context(entry), None)
            .into_iter()
            .filter(|warning| warning.contains("caption"))
            .collect()
    }

    #[test]
    fn a_caption_on_an_image_entry_is_a_known_field() {
        assert_eq!(
            caption_warnings(&serde_json::json!(
                {"type": "image", "path": "/a.png", "caption": "Draft 3"})),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_caption_on_a_code_entry_is_an_unknown_field() {
        // Only image entries take a caption.
        assert_eq!(
            caption_warnings(&serde_json::json!(
                {"type": "code", "content": "x", "caption": "Draft 3"})),
            ["questions[0].context[0].caption: unknown field"]
        );
    }

    #[test]
    fn a_caption_above_1_mib_gets_the_large_entry_warning() {
        assert_eq!(
            caption_warnings(&serde_json::json!(
                {"type": "image", "path": "/a.png", "caption": "x".repeat(MIB + 1)})),
            [
                "questions[0].context[0].caption: 1.1 MiB, more than the 1 MiB an entry should stay under; asqr still shows it"
            ]
        );
    }

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

    /// Text of exactly `bytes` bytes, mostly escape sequences: a red `x`
    /// after another, padded with plain `x`.
    fn coloured_text(bytes: usize) -> String {
        const RED_X: &str = "\u{1b}[31mx\u{1b}[0m";
        let mut text = RED_X.repeat(bytes / RED_X.len());
        text.push_str(&"x".repeat(bytes - text.len()));
        text
    }

    #[test]
    fn warns_about_an_ansi_entry_above_1_mib_counting_its_escape_sequences() {
        assert_eq!(
            warn(
                &with_context(
                    &serde_json::json!({"type": "ansi", "content": coloured_text(MIB + 1)})
                ),
                None
            ),
            [
                "questions[0].context[0]: 1.1 MiB, more than the 1 MiB an entry should stay under; asqr still shows it"
            ]
        );
        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "ansi", "content": coloured_text(MIB)})),
                None
            ),
            Vec::<String>::new(),
            "exactly 1 MiB is fine"
        );
    }

    #[test]
    fn an_ansi_entry_takes_no_language_and_its_file_is_checked() {
        let json = with_context(
            &serde_json::json!({"type": "ansi", "path": "/nonexistent/out.ansi", "language": "rust"}),
        );

        assert_eq!(
            warn(&json, None),
            [
                "questions[0].context[0].language: unknown field",
                "questions[0].context[0].path: file not found: /nonexistent/out.ansi",
            ]
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

    // -----------------------------------------------------------------
    // The line limit
    // -----------------------------------------------------------------

    const MANY_LINES: &str = "more than 500,000 lines; asqr shows only the first 500,000";

    /// `count` lines of `x`, each ended by `\n`: under 1 MiB up to the
    /// line limit and one more, so only the line warning applies.
    fn lines_of_x(count: usize) -> String {
        "x\n".repeat(count)
    }

    #[test]
    fn warns_about_a_code_entry_above_the_line_limit() {
        assert_eq!(
            warn(
                &with_context(
                    &serde_json::json!({"type": "code", "content": lines_of_x(MAX_LINES + 1)})
                ),
                None
            ),
            [format!("questions[0].context[0]: {MANY_LINES}")]
        );
    }

    #[test]
    fn a_code_entry_at_the_line_limit_gets_no_line_warning() {
        assert_eq!(
            warn(
                &with_context(
                    &serde_json::json!({"type": "code", "content": lines_of_x(MAX_LINES)})
                ),
                None
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn warns_about_a_markdown_file_above_the_line_limit() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let notes = directory.path().join("notes.md");
        std::fs::write(&notes, lines_of_x(MAX_LINES + 1)).expect("file is writable");

        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "markdown", "path": notes})),
                None
            ),
            [format!("questions[0].context[0]: {MANY_LINES}")]
        );
    }

    #[test]
    fn warns_about_an_ansi_entry_above_the_line_limit() {
        assert_eq!(
            warn(
                &with_context(
                    &serde_json::json!({"type": "ansi", "content": lines_of_x(MAX_LINES + 1)})
                ),
                None
            ),
            [format!("questions[0].context[0]: {MANY_LINES}")]
        );
    }

    /// An ANSI entry keeps a lone `\r` in its line, as progress output
    /// writes it.
    #[test]
    fn an_ansi_entry_counts_only_newlines_for_the_line_limit() {
        assert_eq!(
            warn(
                &with_context(
                    &serde_json::json!({"type": "ansi", "content": "x\r".repeat(MAX_LINES + 1)})
                ),
                None
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_diff_side_above_the_line_limit_gets_the_warning() {
        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "diff",
                    "old": {"content": "x\n"}, "new": {"content": lines_of_x(MAX_LINES + 1)}})),
                None
            ),
            [format!("questions[0].context[0].new: {MANY_LINES}")]
        );
    }

    #[test]
    fn a_caption_above_the_line_limit_gets_the_warning() {
        assert_eq!(
            caption_warnings(&serde_json::json!(
                {"type": "image", "path": "/a.png", "caption": lines_of_x(MAX_LINES + 1)})),
            [format!("questions[0].context[0].caption: {MANY_LINES}")]
        );
    }

    /// A viewer reads the first 8 MiB of a file, so lines after them are
    /// never shown and do not count.
    #[test]
    fn lines_past_the_first_8_mib_of_a_file_do_not_count() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let log = directory.path().join("build.log");
        let limit = usize::try_from(MAX_BYTES).expect("8 MiB fit");
        let mut text = "x".repeat(limit);
        text.push_str(&"\n".repeat(600_000));
        std::fs::write(&log, text).expect("log is writable");

        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "code", "path": log})),
                None
            ),
            [
                "questions[0].context[0]: 8.6 MiB, more than the 1 MiB an entry should stay under; asqr still shows it"
            ]
        );
    }

    /// A file with a NUL byte shows as "binary file", not as lines.
    #[test]
    fn a_binary_file_gets_no_line_warning() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let dump = directory.path().join("dump.bin");
        let mut bytes = vec![0];
        bytes.extend(lines_of_x(MAX_LINES + 1).bytes());
        std::fs::write(&dump, bytes).expect("file is writable");

        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "code", "path": dump})),
                None
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_entry_above_both_limits_gets_both_warnings() {
        let text = "xx\n".repeat(600_000);

        assert_eq!(
            warn(
                &with_context(&serde_json::json!({"type": "code", "content": text})),
                None
            ),
            [
                "questions[0].context[0]: 1.8 MiB, more than the 1 MiB an entry should stay under; asqr still shows it".to_owned(),
                format!("questions[0].context[0]: {MANY_LINES}"),
            ]
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
    fn note_false_is_ignored_with_a_warning() {
        let json = r#"{"asqr": 2, "questions": [
            {"id": "q", "text": "?", "kind": "text", "note": false},
            {"id": "i", "text": "Done.", "kind": "info", "note": false}]}"#;

        let ignored = "false is ignored; every question takes a note";
        assert_eq!(
            warn(json, None),
            [
                format!("questions[0].note: {ignored}"),
                format!("questions[1].note: {ignored}"),
            ]
        );
    }

    #[test]
    fn note_true_changes_nothing_and_gets_no_warning() {
        let json =
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text", "note": true}]}"#;

        assert_eq!(warn(json, None), Vec::<String>::new());
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
        // generic warning would only repeat the problem. A real unknown
        // field next to them is still reported.
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "diff", "path": "/change.diff", "content": "x", "lang": "rust",
                         "new": {"content": "b"}}]}]}"#;

        assert_eq!(
            warn(json, None),
            ["questions[0].context[0].lang: unknown field"]
        );
    }

    #[test]
    fn a_diff_takes_a_language() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
            "context": [{"type": "diff", "name": "fetch.ts", "language": "tsx",
                         "new": {"content": "b"}}]}]}"#;

        assert_eq!(warn(json, None), Vec::<String>::new());
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

    /// `DECODABLE_FORMATS`, `skills/asqr/references/context.md`, the
    /// `image` field's description and spec section 8.8 name these formats. The set follows the `image`
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

    // -----------------------------------------------------------------
    // Files that change or cannot be read while they are checked
    // -----------------------------------------------------------------

    /// What `open_listed` makes of `resolved`, and the warnings it gave.
    fn opened(resolved: &Path) -> (Option<ContextFile>, Vec<WarningKind>) {
        let mut found = Vec::new();
        let file = open_listed(
            resolved.to_path_buf(),
            &FieldPath::root("questions"),
            &mut found,
        );
        (
            file,
            found.into_iter().map(|warning| warning.kind).collect(),
        )
    }

    /// A file that was opened and then lost its name: only the handle
    /// still leads to its content.
    #[cfg(unix)]
    fn handle_without_path(content: &[u8]) -> File {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("side.txt");
        std::fs::write(&path, content).expect("file is writable");
        let opened = regular_file::open(&path).expect("a regular file");
        std::fs::remove_file(&path).expect("file is removable");
        opened
    }

    /// A path swapped for a FIFO after its type was checked: the open must
    /// not wait for a writer. The check runs apart, so a hang fails the
    /// test instead of stalling the run.
    #[cfg(unix)]
    #[test]
    fn a_path_swapped_for_a_fifo_is_not_a_file_and_does_not_block() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let fifo = directory.path().join("pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());

        let (sender, receiver) = std::sync::mpsc::channel();
        let swapped = fifo.clone();
        std::thread::spawn(move || {
            let (file, found) = opened(&swapped);
            let _ = sender.send((file.is_some(), found));
        });
        let (has_file, found) = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("opening a FIFO must not block");

        assert!(!has_file);
        assert_eq!(found, [WarningKind::NotAFile(fifo)]);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_swapped_for_a_socket_is_not_a_file() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let socket = directory.path().join("s");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("socket binds");

        let (file, found) = opened(&socket);

        assert!(file.is_none());
        assert_eq!(found, [WarningKind::NotAFile(socket)]);
    }

    #[test]
    fn a_path_swapped_for_a_directory_is_not_a_file() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");

        let (file, found) = opened(directory.path());

        assert!(file.is_none());
        assert_eq!(found, [WarningKind::NotAFile(directory.path().to_owned())]);
    }

    #[test]
    fn a_path_removed_after_its_check_is_not_found() {
        let missing = Path::new("/nowhere/fetch.rs");

        let (file, found) = opened(missing);

        assert!(file.is_none());
        assert_eq!(found, [WarningKind::FileNotFound(missing.to_owned())]);
    }

    /// A regular file asqr may not read stays among the checks, without a
    /// warning of its own.
    #[cfg(unix)]
    #[test]
    fn a_file_that_may_not_be_read_is_kept_without_a_warning() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let secret = directory.path().join("secret.txt");
        std::fs::write(&secret, "x").expect("file is writable");
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o000))
            .expect("permissions are settable");

        let (file, found) = opened(&secret);

        let file = file.expect("the file is kept");
        assert!(found.is_empty(), "{found:?}");
        assert_eq!(
            file.opened.expect_err("not readable").kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_image_still_gets_its_warning() {
        use std::os::unix::fs::PermissionsExt;

        let directory = directory_with_png();
        let picture = directory.path().join("picture.png");
        std::fs::set_permissions(&picture, std::fs::Permissions::from_mode(0o000))
            .expect("permissions are settable");

        let warnings = warn(&text_question_with_image(&picture), None);

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("questions[0].context[0].path: cannot be shown: ")
                && warnings[0].to_lowercase().contains("permission denied"),
            "{warnings:?}"
        );
    }

    /// The size of a file that may not be read still counts, alone and in
    /// the sum of a diff's sides.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_above_1_mib_still_gets_the_large_entry_warning() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let large = directory.path().join("large.log");
        std::fs::write(&large, "x".repeat(MIB + 1)).expect("file is writable");
        std::fs::set_permissions(&large, std::fs::Permissions::from_mode(0o000))
            .expect("permissions are settable");

        let alone = warn(
            &with_context(&serde_json::json!({"type": "code", "path": large})),
            None,
        );
        let in_a_diff = warn(
            &with_context(&serde_json::json!({"type": "diff",
                "old": {"content": "a"}, "new": {"path": large}})),
            None,
        );

        for warnings in [alone, in_a_diff] {
            assert_eq!(warnings.len(), 1, "{warnings:?}");
            assert!(warnings[0].contains("MiB"), "{warnings:?}");
        }
    }

    // The three checks that read a file take the opened file and not its
    // path. These tests hand each one a file whose name is gone.

    #[cfg(unix)]
    #[test]
    fn the_unified_diff_check_reads_the_opened_file() {
        let file = handle_without_path(GIT_DIFF.as_bytes());
        let mut found = Vec::new();

        check_unified_diff(None, Some(&file), FieldPath::root("new"), &mut found);

        assert_eq!(found.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn the_line_check_reads_the_opened_file() {
        let file = handle_without_path(lines_of_x(MAX_LINES + 1).as_bytes());

        assert!(file_cut_by_lines(&file, LineBreaks::Any));
    }

    #[cfg(unix)]
    #[test]
    fn the_decoder_check_reads_the_opened_file() {
        let directory = directory_with_png();
        let picture = directory.path().join("picture.png");
        let opened = regular_file::open(&picture).expect("a regular file");
        std::fs::remove_file(&picture).expect("file is removable");
        let mut found = Vec::new();

        let file = ContextFile {
            path: picture,
            opened: Ok(opened),
        };
        check_decodable(&file, FieldPath::root("path"), &mut found);

        assert_eq!(found, []);
    }

    /// Both checks of a diff side read its file, one after the other.
    #[test]
    fn the_unified_diff_and_the_line_check_both_read_one_file() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let patch = directory.path().join("big.patch");
        let text = format!("{GIT_DIFF}{}", lines_of_x(MAX_LINES + 1));
        assert!(text.len() < MIB, "small enough for the unified diff check");
        std::fs::write(&patch, text).expect("written");

        let warnings = warn(
            &with_context(&serde_json::json!({"type": "diff", "new": {"path": patch}})),
            None,
        );

        assert_eq!(
            warnings,
            [
                format!("questions[0].context[0].new: {UNIFIED}"),
                format!("questions[0].context[0].new: {MANY_LINES}"),
            ]
        );
    }

    /// TGA has no signature, so only its extension names the format.
    #[test]
    fn a_tga_file_is_checked_by_its_extension() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let tga = directory.path().join("dot.tga");
        image::RgbImage::new(2, 3)
            .save(&tga)
            .expect("tga is writable");

        assert_eq!(
            warn(&text_question_with_image(&tga), None),
            [] as [String; 0]
        );
        let file = File::open(&tga).expect("tga is readable");
        let size = image_header(&file, &tga).expect("a TGA has a header");
        assert_eq!(size, (2, 3));
    }

    /// The warning says only that the format is unsupported, so the error
    /// under it is checked here: it names the extension.
    #[test]
    fn an_unknown_extension_without_a_signature_names_itself() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("dot.xyzzy");
        std::fs::write(&path, b"no picture in here").expect("file is writable");
        let file = File::open(&path).expect("file is readable");

        let error = image_header(&file, &path).expect_err("no format is known");

        assert!(matches!(error, ImageError::Unsupported(_)), "{error:?}");
        assert!(error.to_string().contains("xyzzy"), "{error}");
    }

    /// A signature decides before the extension is asked, also where the
    /// extension names another format that does exist.
    #[test]
    fn a_png_named_as_another_format_is_still_read_as_a_png() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("picture.jpg");
        image::RgbImage::new(4, 5)
            .save_with_format(&path, image::ImageFormat::Png)
            .expect("png is writable");
        let file = File::open(&path).expect("file is readable");

        assert_eq!(image_header(&file, &path).expect("a PNG"), (4, 5));
        assert_eq!(
            warn(&text_question_with_image(&path), None),
            [] as [String; 0]
        );
    }

    /// An image is decoded by what its bytes are, and only an image that
    /// no bytes identify falls back to the name of its file.
    #[test]
    fn the_format_of_an_image_comes_from_its_bytes_before_its_name() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let named = |name: &str, bytes: &[u8]| {
            let file = directory.path().join(name);
            std::fs::write(&file, bytes).expect("file is writable");
            warn(&text_question_with_image(&file), None)
        };
        let mut png = Vec::new();
        image::RgbImage::new(1, 1)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("png is encodable");

        assert_eq!(named("picture.svg", &png), [] as [String; 0]);
        assert_eq!(named("picture.foo", &png), [] as [String; 0]);
        assert_eq!(named("picture", &png), [] as [String; 0]);
        for name in ["notes.svg", "notes.foo", "notes"] {
            let warnings = named(name, b"just some text");
            assert_eq!(warnings.len(), 1, "{name}: {warnings:?}");
            assert!(
                warnings[0].contains("unsupported image format, use one of"),
                "{name}: {warnings:?}"
            );
        }
    }
}
