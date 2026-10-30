// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The committed fonts cover every glyph asqr draws.
//!
//! What asqr draws of its own is in its sources: the characters of the
//! string and character literals of the code that is built, and of the
//! files that code includes. The scan follows asqr's module tree from
//! `src/lib.rs` and `src/main.rs`, so modules that only tests build, also
//! those declared in a file of their own, stay out. Characters asqr
//! computes instead of writing them, and those ratatui draws for asqr's
//! borders, are added by hand below. Text a session brings is not asqr's
//! to choose; a scene that shows a character no font has fails to draw.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::TokenTree;
use ratatui::buffer::CellWidth;
use screens::fonts::{Source, source_for};
use syn::visit::{self, Visit};
use syn::{Attribute, ImplItem, Item, Lit, Meta};

/// asqr's sources, two levels above this crate.
fn asqr_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src")
        .canonicalize()
        .expect("asqr's sources are two levels up")
}

/// The file that holds the Kitty image placeholder and its diacritics.
/// Under the Kitty protocol the terminal draws the image where they stand,
/// so they are never drawn from a font, and the pictures draw images as
/// half blocks: the scan leaves those two items out.
const PLACEHOLDERS_FILE: &str = "graphics/placeholders.rs";
const PLACEHOLDER_ITEMS: [&str; 2] = ["PLACEHOLDER", "DIACRITICS"];

// =====================================================================
// The scan
// =====================================================================

#[derive(Default)]
struct Scan {
    /// Each character found, with the first file it was found in.
    chars: BTreeMap<char, String>,
    /// The files read, relative to `src/`.
    read: BTreeSet<String>,
    /// The module files left out because only tests build them.
    test_only: BTreeSet<String>,
    /// The placeholder items left out, by name.
    left_out: BTreeSet<String>,
}

impl Scan {
    fn add(&mut self, text: &str, from: &str) {
        for ch in text.chars() {
            self.chars.entry(ch).or_insert_with(|| from.to_owned());
        }
    }

    /// Reads the module in `file`, whose child modules live in `children`.
    fn module(&mut self, src: &Path, file: &Path, children: &Path) {
        let relative = file
            .strip_prefix(src)
            .expect("modules lie in src")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(file).expect("a module file is readable");
        let parsed = syn::parse_file(&text).expect("asqr's sources parse");
        self.read.insert(relative.clone());
        let mut walker = Walker {
            scan: self,
            src,
            file,
            relative: &relative,
            out_of_line: Vec::new(),
        };
        walker.visit_file(&parsed);
        let out_of_line = std::mem::take(&mut walker.out_of_line);
        for (name, test_only) in out_of_line {
            let flat = children.join(format!("{name}.rs"));
            let (child, grandchildren) = if flat.is_file() {
                (flat, children.join(&name))
            } else {
                (children.join(&name).join("mod.rs"), children.join(&name))
            };
            if test_only {
                let relative = child
                    .strip_prefix(src)
                    .expect("modules lie in src")
                    .to_string_lossy()
                    .into_owned();
                self.test_only.insert(relative);
            } else {
                self.module(src, &child, &grandchildren);
            }
        }
    }
}

/// Whether `attrs` mark an item only tests build: `#[test]`,
/// `#[cfg(test)]` or `#[cfg(all(test, ...))]`. `cfg(any(test, ...))`
/// builds outside tests too.
fn test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("test") {
            return true;
        }
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let Ok(condition) = attr.parse_args::<Meta>() else {
            return false;
        };
        match condition {
            Meta::Path(path) => path.is_ident("test"),
            Meta::List(list) if list.path.is_ident("all") => list
                .parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                )
                .is_ok_and(|parts| parts.iter().any(|part| part.path().is_ident("test"))),
            _ => false,
        }
    })
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn item_name(item: &Item) -> Option<String> {
    match item {
        Item::Const(item) => Some(item.ident.to_string()),
        Item::Static(item) => Some(item.ident.to_string()),
        _ => None,
    }
}

struct Walker<'a> {
    scan: &'a mut Scan,
    src: &'a Path,
    file: &'a Path,
    relative: &'a str,
    /// The modules this file declares without a body, and whether only
    /// tests build them.
    out_of_line: Vec<(String, bool)>,
}

impl Walker<'_> {
    fn literal(&mut self, lit: &Lit) {
        match lit {
            Lit::Str(text) => self.scan.add(&text.value(), self.relative),
            Lit::Char(ch) => self.scan.add(&ch.value().to_string(), self.relative),
            _ => {}
        }
    }

    /// The literals inside a macro's tokens, which syn leaves unparsed.
    fn tokens(&mut self, tokens: proc_macro2::TokenStream) {
        for token in tokens {
            match token {
                TokenTree::Literal(literal) => self.literal(&Lit::new(literal)),
                TokenTree::Group(group) => self.tokens(group.stream()),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Walker<'_> {
    // Doc comments and other attributes are not drawn.
    fn visit_attribute(&mut self, _: &'ast Attribute) {}

    fn visit_item(&mut self, item: &'ast Item) {
        if test_only(item_attrs(item)) {
            if let Item::Mod(module) = item
                && module.content.is_none()
            {
                self.out_of_line.push((module.ident.to_string(), true));
            }
            return;
        }
        if self.relative == PLACEHOLDERS_FILE
            && let Some(name) = item_name(item)
            && PLACEHOLDER_ITEMS.contains(&name.as_str())
        {
            self.scan.left_out.insert(name);
            return;
        }
        if let Item::Mod(module) = item
            && module.content.is_none()
        {
            self.out_of_line.push((module.ident.to_string(), false));
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        let attrs = match item {
            ImplItem::Const(item) => &item.attrs,
            ImplItem::Fn(item) => &item.attrs,
            ImplItem::Type(item) => &item.attrs,
            ImplItem::Macro(item) => &item.attrs,
            _ => return visit::visit_impl_item(self, item),
        };
        if !test_only(attrs) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_lit(&mut self, lit: &'ast Lit) {
        self.literal(lit);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // A file `include_str!` pulls in from inside `src/` is drawn as it
        // is, as Talqi's templates are.
        if mac.path.is_ident("include_str")
            && let Ok(name) = mac.parse_body::<syn::LitStr>()
        {
            let included = self
                .file
                .parent()
                .expect("a module file has a directory")
                .join(name.value());
            if let Ok(included) = included.canonicalize()
                && included.starts_with(self.src)
            {
                let text =
                    std::fs::read_to_string(&included).expect("an included file is readable");
                let from = included
                    .strip_prefix(self.src)
                    .expect("checked above")
                    .to_string_lossy()
                    .into_owned();
                self.scan.add(&text, &from);
            }
            return;
        }
        self.tokens(mac.tokens.clone());
    }
}

/// What asqr draws: the scan, and what it computes or ratatui draws for it.
fn drawn() -> Scan {
    let src = asqr_src();
    let mut scan = Scan::default();
    scan.module(&src, &src.join("lib.rs"), &src);
    scan.module(&src, &src.join("main.rs"), &src);

    let by_hand = "added by hand";
    // Any printable ASCII can come from a session.
    let ascii: String = (0x20u8..0x7F).map(char::from).collect();
    scan.add(&ascii, by_hand);
    // A code or diff entry shows a control byte as its Control Picture and
    // a C1 control or invalid UTF-8 as U+FFFD
    // (`src/tui/render/source_line.rs`, `picture`;
    // `src/tui/render/source_file.rs`, `String::from_utf8_lossy`).
    let pictures: String = (0x2400..=0x241F)
        .chain([0x2421, 0xFFFD])
        .map(|code| char::from_u32(code).expect("valid code points"))
        .collect();
    scan.add(&pictures, by_hand);
    // Overlays and the invalid-session dialog are rounded ratatui blocks.
    let rounded = ratatui::symbols::border::ROUNDED;
    for symbol in [
        rounded.top_left,
        rounded.top_right,
        rounded.bottom_left,
        rounded.bottom_right,
        rounded.vertical_left,
        rounded.vertical_right,
        rounded.horizontal_top,
        rounded.horizontal_bottom,
    ] {
        scan.add(symbol, "ratatui's rounded border");
    }
    scan
}

/// Whether `ch` takes a cell of its own: controls and characters of width
/// 0 (variation selectors, joiners, combining marks) never do.
fn takes_a_cell(ch: char) -> bool {
    !ch.is_control() && ch.to_string().as_str().cell_width() > 0
}

#[test]
fn the_fonts_cover_every_glyph_asqr_draws() {
    let scan = drawn();

    let missing: Vec<String> = scan
        .chars
        .iter()
        .filter(|(ch, _)| takes_a_cell(**ch))
        .filter(|(ch, _)| {
            [false, true].into_iter().any(|bold| {
                [false, true]
                    .into_iter()
                    .any(|italic| matches!(source_for(**ch, bold, italic), Source::Missing))
            })
        })
        .map(|(ch, from)| format!("U+{:04X} {ch:?} from {from}", u32::from(*ch)))
        .collect();
    assert!(
        missing.is_empty(),
        "no committed font has:\n{}",
        missing.join("\n")
    );
}

/// Guard: the scan reads what it must and leaves out what it must, so a
/// scan that finds nothing cannot pass the test above.
#[test]
fn the_scan_follows_the_module_tree() {
    let scan = drawn();

    assert!(scan.read.contains("tui/render/parts.rs"), "{:?}", scan.read);
    assert!(
        scan.test_only.contains("tui/render/test_support.rs"),
        "{:?}",
        scan.test_only
    );
    assert!(!scan.read.contains("tui/render/test_support.rs"));
    for ch in ['❯', 'ℹ', '☐'] {
        assert!(scan.chars.contains_key(&ch), "the scan misses {ch}");
    }
    assert_eq!(
        scan.left_out,
        PLACEHOLDER_ITEMS.map(str::to_owned).into_iter().collect(),
        "the placeholder items are still where the scan leaves them out"
    );
    assert!(!scan.chars.contains_key(&'\u{10EEEE}'));
}
