// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Context entries (spec section 4): material that helps to answer a
//! question, attached to the question or to one of its options. The TUI
//! shows it next to the question (spec section 7.7).
//!
//! As with the rest of the session, the types only describe the shape.
//! Rules that span fields, such as a text entry naming either a file or
//! inline text, live in validation, which can name the offending field.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One entry of a `context` list. The `type` field picks the kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContextEntry {
    Image(ImageEntry),
    Text(TextEntry),
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

/// Longer text: source code, plain text or Markdown, from a file or
/// inline. Exactly one of `path` and `text` is given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextEntry {
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
    pub text: Option<String>,

    /// The language for highlighting, such as `rust` or `markdown`.
    /// Without it, the extension of `path`, then of `title`, decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// `false` shows Markdown as its source instead of rendering it.
    #[serde(default = "render_by_default")]
    pub render: bool,
}

fn render_by_default() -> bool {
    true
}

/// A change between two versions of a text, which asqr diffs itself. A
/// missing `old` is a new file, a missing `new` a deleted one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiffEntry {
    /// A name for the entry, such as a file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<DiffSide>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<DiffSide>,

    /// The file name the diff is about, shown in the header. Its
    /// extension picks the language when `language` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// One side of a diff: a file or inline text. Exactly one of the two is
/// given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiffSide {
    /// An absolute path; `asqr ask` makes a relative one absolute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ContextEntry {
    /// The path of an image entry; `o` and `z` act on it.
    pub fn image_path(&self) -> Option<&str> {
        match self {
            ContextEntry::Image(image) => Some(&image.path),
            ContextEntry::Text(_) | ContextEntry::Diff(_) => None,
        }
    }

    pub fn title(&self) -> Option<&str> {
        match self {
            ContextEntry::Image(image) => image.title.as_deref(),
            ContextEntry::Text(text) => text.title.as_deref(),
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
            parse(json!({"type": "text", "title": "Notes", "text": "# Plan", "render": false})),
            ContextEntry::Text(TextEntry {
                title: Some("Notes".into()),
                path: None,
                text: Some("# Plan".into()),
                language: None,
                render: false,
            })
        );
        assert_eq!(
            parse(
                json!({"type": "diff", "name": "main.rs", "language": "rust",
                         "old": {"path": "/src/main.rs"}, "new": {"text": "fn main() {}"}})
            ),
            ContextEntry::Diff(DiffEntry {
                title: None,
                old: Some(DiffSide {
                    path: Some("/src/main.rs".into()),
                    text: None
                }),
                new: Some(DiffSide {
                    path: None,
                    text: Some("fn main() {}".into())
                }),
                name: Some("main.rs".into()),
                language: Some("rust".into()),
            })
        );
    }

    #[test]
    fn markdown_renders_unless_the_entry_says_otherwise() {
        let ContextEntry::Text(text) = parse(json!({"type": "text", "text": "**bold**"})) else {
            panic!("a text entry");
        };

        assert!(text.render);
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
        let parsed: Result<ContextEntry, _> =
            serde_json::from_value(json!({"type": "video", "path": "/clip.mp4"}));

        assert!(parsed.is_err());
    }

    #[test]
    fn writes_the_type_back_with_the_fields_in_use() {
        let entry = parse(json!({"type": "diff", "new": {"text": "added"}}));

        assert_eq!(
            serde_json::to_value(&entry).expect("entry serializes"),
            json!({"type": "diff", "new": {"text": "added"}})
        );
    }

    #[test]
    fn only_image_entries_have_an_image_path() {
        let image = parse(json!({"type": "image", "title": "Login", "path": "/a.png"}));
        let text = parse(json!({"type": "text", "title": "Log", "text": "ok"}));
        let diff = parse(json!({"type": "diff", "title": "Change", "new": {"text": "x"}}));

        assert_eq!(image.image_path(), Some("/a.png"));
        assert_eq!(text.image_path(), None);
        assert_eq!(diff.image_path(), None);
        assert_eq!(
            [image.title(), text.title(), diff.title()],
            [Some("Login"), Some("Log"), Some("Change")]
        );
    }
}
