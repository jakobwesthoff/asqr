// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Context entries (spec section 4): material that helps to answer a
//! question, attached to the question or to one of its options. The TUI
//! shows it next to the question (spec section 7.7).
//!
//! As with the rest of the session, the types only describe the shape.
//! Rules that span fields, such as a code entry naming either a file or
//! inline content, live in validation, which can name the offending field.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One entry of a `context` list. The `type` field picks the kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContextEntry {
    Image(ImageEntry),
    Code(CodeEntry),
    Markdown(MarkdownEntry),
    Diff(DiffEntry),
}

/// An image file. PNG, JPEG, GIF (first frame only), WebP, BMP, TIFF,
/// ICO, TGA, PNM, QOI, DDS, OpenEXR, HDR and farbfeld are shown. SVG and
/// AVIF are not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImageEntry {
    /// A name for the entry, such as a file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// An absolute path; `asqr ask` makes a relative one absolute.
    pub path: String,

    /// `false` keeps the image at its natural size where it fits, instead
    /// of scaling it up to fill the space it gets. A larger image still
    /// shrinks to fit.
    // Written back only when it differs from the default, so `asqr ask`
    // adds nothing to an image entry that leaves it out. The skip would
    // also hide the default from the schema, which names it here.
    #[serde(default = "enlarge_by_default", skip_serializing_if = "is_true")]
    #[schemars(extend("default" = true))]
    pub enlarge: bool,
}

fn enlarge_by_default() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// Text shown exactly as written and highlighted, like a fenced code
/// block: source code, a log, terminal output, a configuration file, or
/// the source of a Markdown file. Exactly one of `path` and `content` is
/// given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CodeEntry {
    /// A name for the entry, such as a file name. Its extension picks the
    /// language when neither `language` nor `path` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// An absolute path to the file to show; `asqr ask` makes a relative
    /// one absolute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// The text itself, in place of a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// The language for highlighting, such as `rust` or `json`. Without
    /// it, the extension of `path`, then of `title`, decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// Markdown, rendered and wrapped to the space it gets. Exactly one of
/// `path` and `content` is given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MarkdownEntry {
    /// A name for the entry, such as a file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// An absolute path to the file to show; `asqr ask` makes a relative
    /// one absolute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// The Markdown itself, in place of a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// A change between two versions of a text, which asqr diffs itself. A
/// missing `old` is a new file, a missing `new` a deleted one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiffEntry {
    /// A name for the entry, such as a file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<DiffSide>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<DiffSide>,

    /// The file name the diff is about, shown in the header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    // A `path` or `content` on the entry itself, where an agent with a
    // unified diff at hand puts it. Neither is part of the format, so the
    // schema leaves them out and nothing writes them back; they are read
    // only so validation can say where the texts go, together with every
    // other error of the file.
    #[serde(default, rename = "path", skip_serializing)]
    #[schemars(skip)]
    pub(crate) misplaced_path: Option<serde_json::Value>,

    #[serde(default, rename = "content", skip_serializing)]
    #[schemars(skip)]
    pub(crate) misplaced_content: Option<serde_json::Value>,
}

impl DiffEntry {
    /// Whether the entry carries `path` or `content` itself instead of on
    /// `old` or `new`.
    pub fn misplaced_source(&self) -> bool {
        self.misplaced_path.is_some() || self.misplaced_content.is_some()
    }
}

/// One side of a diff: a file or inline content. Exactly one of the two
/// is given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiffSide {
    /// An absolute path; `asqr ask` makes a relative one absolute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// The text itself, in place of a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl ContextEntry {
    /// The path of an image entry; `o` and `z` act on it.
    pub fn image_path(&self) -> Option<&str> {
        match self {
            ContextEntry::Image(image) => Some(&image.path),
            ContextEntry::Code(_) | ContextEntry::Markdown(_) | ContextEntry::Diff(_) => None,
        }
    }

    pub fn title(&self) -> Option<&str> {
        match self {
            ContextEntry::Image(image) => image.title.as_deref(),
            ContextEntry::Code(code) => code.title.as_deref(),
            ContextEntry::Markdown(markdown) => markdown.title.as_deref(),
            ContextEntry::Diff(diff) => diff.title.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(value: serde_json::Value) -> ContextEntry {
        serde_json::from_value(value).expect("test entry parses")
    }

    #[test]
    fn the_type_field_picks_the_kind_of_entry() {
        assert_eq!(
            parse(json!({"type": "image", "path": "/shots/login.png"})),
            ContextEntry::Image(ImageEntry {
                title: None,
                path: "/shots/login.png".into(),
                enlarge: true,
            })
        );
        assert_eq!(
            parse(
                json!({"type": "code", "title": "fetch", "content": "fn f() {}", "language": "rust"})
            ),
            ContextEntry::Code(CodeEntry {
                title: Some("fetch".into()),
                path: None,
                content: Some("fn f() {}".into()),
                language: Some("rust".into()),
            })
        );
        assert_eq!(
            parse(json!({"type": "markdown", "title": "Plan", "path": "/notes/plan.md"})),
            ContextEntry::Markdown(MarkdownEntry {
                title: Some("Plan".into()),
                path: Some("/notes/plan.md".into()),
                content: None,
            })
        );
        assert_eq!(
            parse(json!({"type": "diff", "name": "main.rs",
                         "old": {"path": "/src/main.rs"}, "new": {"content": "fn main() {}"}})),
            ContextEntry::Diff(DiffEntry {
                title: None,
                old: Some(DiffSide {
                    path: Some("/src/main.rs".into()),
                    content: None
                }),
                new: Some(DiffSide {
                    path: None,
                    content: Some("fn main() {}".into())
                }),
                name: Some("main.rs".into()),
                ..DiffEntry::default()
            })
        );
    }

    #[test]
    fn a_diff_reads_a_path_or_content_on_the_entry_but_never_writes_it() {
        let parsed = parse(json!({"type": "diff", "path": "/a.diff", "content": "@@ -1 +1 @@"}));

        let ContextEntry::Diff(diff) = &parsed else {
            panic!("a diff entry");
        };
        assert!(diff.misplaced_source());
        assert_eq!(
            serde_json::to_value(&parsed).expect("entry serializes"),
            json!({"type": "diff"})
        );
        let ContextEntry::Diff(diff) = parse(json!({"type": "diff", "new": {"content": "x"}}))
        else {
            panic!("a diff entry");
        };
        assert!(!diff.misplaced_source());
    }

    #[test]
    fn an_image_enlarges_unless_the_entry_says_otherwise() {
        let ContextEntry::Image(image) = parse(json!({"type": "image", "path": "/a.png"})) else {
            panic!("an image entry");
        };
        assert!(image.enlarge);

        let pixel_art = parse(json!({"type": "image", "path": "/a.png", "enlarge": false}));
        let ContextEntry::Image(image) = &pixel_art else {
            panic!("an image entry");
        };
        assert!(!image.enlarge);
        assert_eq!(
            serde_json::to_value(&pixel_art).expect("entry serializes"),
            json!({"type": "image", "path": "/a.png", "enlarge": false}),
            "the default is not written back, anything else is"
        );

        let wrong: Result<ContextEntry, _> =
            serde_json::from_value(json!({"type": "image", "path": "/a.png", "enlarge": "no"}));
        assert!(wrong.is_err(), "enlarge is a boolean");
    }

    #[test]
    fn an_unknown_type_is_no_context_entry() {
        for kind in ["video", "text"] {
            let parsed: Result<ContextEntry, _> =
                serde_json::from_value(json!({"type": kind, "path": "/clip"}));

            assert!(parsed.is_err(), "{kind}");
        }
    }

    #[test]
    fn writes_the_type_back_with_the_fields_in_use() {
        for entry in [
            json!({"type": "diff", "new": {"content": "added"}}),
            json!({"type": "code", "content": "x = 1"}),
            json!({"type": "markdown", "path": "/notes.md"}),
        ] {
            assert_eq!(
                serde_json::to_value(parse(entry.clone())).expect("entry serializes"),
                entry
            );
        }
    }

    #[test]
    fn only_image_entries_have_an_image_path() {
        let image = parse(json!({"type": "image", "title": "Login", "path": "/a.png"}));
        let code = parse(json!({"type": "code", "title": "Log", "content": "ok"}));
        let markdown = parse(json!({"type": "markdown", "title": "Notes", "content": "ok"}));
        let diff = parse(json!({"type": "diff", "title": "Change", "new": {"content": "x"}}));

        assert_eq!(image.image_path(), Some("/a.png"));
        for entry in [&code, &markdown, &diff] {
            assert_eq!(entry.image_path(), None);
        }
        assert_eq!(
            [image.title(), code.title(), markdown.title(), diff.title()],
            [Some("Login"), Some("Log"), Some("Notes"), Some("Change")]
        );
    }
}
