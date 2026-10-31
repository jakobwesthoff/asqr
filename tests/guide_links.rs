// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The guide in `docs/guide` holds together: its table of contents lists
//! every page, its start page leads to every section, every link and
//! picture points at a file inside the vault, and every scene has the
//! picture the pages show.
//!
//! The vault is exported as a website with `ntropy site --strict`, which
//! fails on a file outside the vault. CI has no ntropy, so this test holds
//! the same rule for the links.

use std::fs;
use std::path::{Path, PathBuf};

fn guide() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/guide")
}

/// The Markdown files of the notes, `all-notes/*.md`, sorted.
fn notes(guide: &Path) -> Vec<PathBuf> {
    let mut notes: Vec<PathBuf> = fs::read_dir(guide.join("all-notes"))
        .expect("the guide has notes")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    notes.sort();
    notes
}

/// The targets of the Markdown links and images in `text`, as written,
/// without those that leave the vault or point into the page.
fn link_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else { break };
        let target = &after[..end];
        let target = target.split('#').next().unwrap_or_default();
        let outside = target.contains("://") || target.starts_with("mailto:");
        if !target.is_empty() && !outside {
            targets.push(target.to_owned());
        }
        rest = &after[end..];
    }
    targets
}

/// What is wrong with the links of the file at `path`, one line for each:
/// a target that is no file, or a file outside `root`.
fn broken_links(path: &Path, root: &Path) -> Vec<String> {
    let text = fs::read_to_string(path).expect("a note is readable");
    let folder = path.parent().expect("a note has a folder");
    let root = root.canonicalize().expect("the vault exists");
    link_targets(&text)
        .into_iter()
        .filter_map(|target| match folder.join(&target).canonicalize() {
            Err(_) => Some(format!("{}: {target} is no file", path.display())),
            Ok(file) if !file.starts_with(&root) => Some(format!(
                "{}: {target} lies outside the vault",
                path.display()
            )),
            Ok(_) => None,
        })
        .collect()
}

/// The front matter of a note, between its two `---` lines.
fn front_matter(text: &str) -> Vec<&str> {
    text.lines()
        .skip(1)
        .take_while(|line| *line != "---")
        .collect()
}

/// Whether the front matter marks the note as the landing note of its
/// section, with `index: true` in its `site` table.
fn is_section_landing(text: &str) -> bool {
    front_matter(text)
        .iter()
        .any(|line| line.trim() == "index: true")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .expect("a note has a file name")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn the_table_of_contents_lists_every_page() {
    let guide = guide();
    let readme = fs::read_to_string(guide.join("README.md")).expect("the guide has a README");

    let missing: Vec<String> = notes(&guide)
        .iter()
        .map(|note| file_name(note))
        .filter(|name| !readme.contains(&format!("all-notes/{name}")))
        .collect();

    assert!(
        missing.is_empty(),
        "README.md does not list these pages: {missing:?}"
    );
}

#[test]
fn the_start_page_leads_to_the_landing_note_of_every_section() {
    let guide = guide();
    let all = notes(&guide);
    let start = all
        .iter()
        .find(|note| {
            fs::read_to_string(note)
                .expect("a note is readable")
                .contains("title: \"asqr\"")
        })
        .expect("the start page is titled asqr");
    let start_text = fs::read_to_string(start).expect("the start page is readable");

    let unlinked: Vec<String> = all
        .iter()
        .filter(|note| *note != start)
        .filter(|note| is_section_landing(&fs::read_to_string(note).expect("readable")))
        .map(|note| file_name(note))
        .filter(|name| !start_text.contains(name))
        .collect();

    assert!(
        unlinked.is_empty(),
        "the start page does not link the landing notes {unlinked:?}"
    );
}

#[test]
fn every_link_and_picture_points_at_a_file_inside_the_vault() {
    let guide = guide();
    let mut files = notes(&guide);
    files.push(guide.join("README.md"));

    let broken: Vec<String> = files
        .iter()
        .flat_map(|file| broken_links(file, &guide))
        .collect();

    assert!(broken.is_empty(), "{}", broken.join("\n"));
}

#[test]
fn every_scene_has_its_picture_and_every_picture_its_scene() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stems = |folder: PathBuf, extension: &str| -> Vec<String> {
        let mut stems: Vec<String> = fs::read_dir(&folder)
            .unwrap_or_else(|error| panic!("reading {}: {error}", folder.display()))
            .map(|entry| entry.expect("a directory entry").path())
            .filter(|path| path.extension().is_some_and(|found| found == extension))
            .filter_map(|path| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .collect();
        stems.sort();
        stems
    };

    let scenes = stems(root.join("tools/screens/scenes"), "toml");
    let pictures = stems(guide().join("pictures"), "png");

    assert_eq!(
        scenes, pictures,
        "`just guide-pictures` draws a picture for every scene"
    );
}

/// The pictures `text` shows from the guide's `pictures/` folder, as pairs
/// of the picture's name without `.png` and its alt text.
fn pictures_shown(text: &str) -> Vec<(String, String)> {
    let mut shown = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("![") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("](") else { break };
        let alt = &after[..end];
        let target = &after[end + 2..];
        let Some(close) = target.find(')') else { break };
        if let Some(name) = target[..close]
            .strip_prefix("../pictures/")
            .and_then(|file| file.strip_suffix(".png"))
        {
            shown.push((name.to_owned(), alt.to_owned()));
        }
        rest = &target[close..];
    }
    shown
}

/// The caption of every scene, by the scene's name: the `caption = "..."`
/// line of its file, which `just guide-pictures` draws the picture of the
/// same name from.
fn scene_captions() -> Vec<(String, String)> {
    let scenes = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/screens/scenes");
    let mut captions: Vec<(String, String)> = fs::read_dir(&scenes)
        .expect("the scenes are readable")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|found| found == "toml"))
        .map(|path| {
            let text = fs::read_to_string(&path).expect("a scene is readable");
            let caption = text
                .lines()
                .find_map(|line| line.strip_prefix("caption = \""))
                .and_then(|rest| rest.strip_suffix('"'))
                .unwrap_or_else(|| panic!("{} has a caption line", path.display()));
            assert!(
                !caption.contains('\\'),
                "{}: a caption with an escape is not read here",
                path.display()
            );
            let name = path
                .file_stem()
                .expect("a scene has a name")
                .to_string_lossy()
                .into_owned();
            (name, caption.to_owned())
        })
        .collect();
    captions.sort();
    captions
}

#[test]
fn every_picture_a_page_shows_has_its_scene_s_caption_as_alt_text() {
    let captions = scene_captions();
    let mut drifted = Vec::new();
    for note in notes(&guide()) {
        let text = fs::read_to_string(&note).expect("a note is readable");
        for (picture, alt) in pictures_shown(&text) {
            let caption = captions
                .iter()
                .find(|(name, _)| *name == picture)
                .map(|(_, caption)| caption.as_str());
            if caption != Some(alt.as_str()) {
                drifted.push(format!(
                    "{}: {picture}.png has alt text {alt:?}, its scene's caption is {caption:?}",
                    file_name(&note)
                ));
            }
        }
    }

    assert!(drifted.is_empty(), "{}", drifted.join("\n"));
}

#[test]
fn every_picture_is_shown_by_a_page() {
    let shown: Vec<String> = notes(&guide())
        .iter()
        .flat_map(|note| pictures_shown(&fs::read_to_string(note).expect("a note is readable")))
        .map(|(name, _)| name)
        .collect();

    let unshown: Vec<String> = scene_captions()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !shown.contains(name))
        .collect();

    assert!(
        unshown.is_empty(),
        "no page shows these pictures: {unshown:?}"
    );
}

/// The value of `key` in the front matter of a note, such as `order` in
/// its `site` table or `tags`, as written.
fn front_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    front_matter(text)
        .into_iter()
        .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix(':'))
        .map(str::trim)
}

/// The sections of the README's page list, in order: each `###` heading
/// with the note files listed under it.
fn readme_sections(readme: &str) -> Vec<(String, Vec<String>)> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for line in readme.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            sections.push((heading.trim().to_owned(), Vec::new()));
        } else if let Some((_, files)) = sections.last_mut()
            && let Some(start) = line.find("](all-notes/")
        {
            let rest = &line[start + "](all-notes/".len()..];
            let end = rest.find(')').unwrap_or(rest.len());
            files.push(rest[..end].to_owned());
        }
    }
    sections
}

/// A note as the site orders it: its file name, tag, `order` and, for the
/// landing note of a section, its `label`.
struct Placed {
    file: String,
    tag: String,
    order: u32,
    label: Option<String>,
}

fn placed_notes() -> Vec<Placed> {
    notes(&guide())
        .iter()
        .map(|note| {
            let text = fs::read_to_string(note).expect("a note is readable");
            let file = file_name(note);
            let order = front_value(&text, "order")
                .and_then(|order| order.parse().ok())
                .unwrap_or_else(|| panic!("{file} has a site order"));
            let tag = front_value(&text, "tags")
                .unwrap_or_else(|| panic!("{file} has tags"))
                .to_owned();
            let label = is_section_landing(&text)
                .then(|| front_value(&text, "label").map(str::to_owned))
                .flatten();
            Placed {
                file,
                tag,
                order,
                label,
            }
        })
        .collect()
}

/// The site lists the sections by their landing notes' `order` and the
/// notes of a section by theirs (ntropy's site export); the README lists
/// them in the same order, so the guide reads the same on GitHub.
#[test]
fn the_notes_are_ordered_as_the_readme_lists_them() {
    let readme = fs::read_to_string(guide().join("README.md")).expect("the guide has a README");
    let sections = readme_sections(&readme);
    let notes = placed_notes();

    let mut landings: Vec<&Placed> = notes.iter().filter(|note| note.label.is_some()).collect();
    landings.sort_by_key(|note| note.order);
    let orders: Vec<u32> = landings.iter().map(|note| note.order).collect();
    assert!(
        orders.windows(2).all(|pair| pair[0] < pair[1]),
        "two sections share an order: {orders:?}"
    );
    let labels: Vec<&str> = landings
        .iter()
        .filter_map(|note| note.label.as_deref())
        .collect();
    let headings: Vec<&str> = sections
        .iter()
        .map(|(heading, _)| heading.as_str())
        .collect();
    assert_eq!(labels, headings, "the sections by order, and the README's");

    // A landing note's `order` places its section; the note itself is the
    // section's page, which the README lists first.
    for (landing, (heading, listed)) in landings.iter().zip(&sections) {
        let mut members: Vec<&Placed> = notes
            .iter()
            .filter(|note| note.tag == landing.tag && note.label.is_none())
            .collect();
        members.sort_by_key(|note| note.order);
        let ordered: Vec<&str> = std::iter::once(landing.file.as_str())
            .chain(members.iter().map(|note| note.file.as_str()))
            .collect();
        assert_eq!(
            ordered, *listed,
            "the notes of {heading} by order, and the README's list"
        );
    }
}

#[test]
fn the_readme_s_sections_are_read_with_their_notes() {
    let readme = "# x\n\n### Start\n\n- [a](all-notes/a.md)\n- [b](all-notes/b.md)\n\n### Answering\n\n- [c](all-notes/c.md), details of [a](all-notes/a.md)\n\n## As a website\n";

    assert_eq!(
        readme_sections(readme),
        [
            (
                "Start".to_owned(),
                vec!["a.md".to_owned(), "b.md".to_owned()]
            ),
            ("Answering".to_owned(), vec!["c.md".to_owned()])
        ]
    );
}

#[test]
fn pictures_are_read_with_their_alt_text() {
    let text = "![A b](../pictures/one.png) ![c](../pictures/talqi.webp) [d](x.md) ![](../pictures/two.png)";

    assert_eq!(
        pictures_shown(text),
        [
            ("one".to_owned(), "A b".to_owned()),
            ("two".to_owned(), String::new())
        ]
    );
}

#[test]
fn links_are_read_without_the_ones_that_leave_the_vault() {
    let text = "[a](x.md) ![b](../p/y.png) [c](https://example.org/z) [d](#here) [e](f.md#top)";

    assert_eq!(link_targets(text), ["x.md", "../p/y.png", "f.md"]);
}

#[test]
fn a_link_to_a_file_outside_the_vault_is_reported() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let vault = dir.path().join("vault");
    fs::create_dir_all(vault.join("notes")).expect("made");
    fs::write(dir.path().join("outside.md"), "").expect("written");
    fs::write(vault.join("inside.md"), "").expect("written");
    let note = vault.join("notes/a.md");
    fs::write(
        &note,
        "[1](../inside.md) [2](../../outside.md) [3](../missing.md)",
    )
    .expect("written");

    let broken = broken_links(&note, &vault);

    assert_eq!(broken.len(), 2, "{broken:?}");
    assert!(broken[0].contains("lies outside the vault"), "{broken:?}");
    assert!(broken[1].contains("is no file"), "{broken:?}");
}

#[test]
fn a_landing_note_is_found_by_its_index_line() {
    assert!(is_section_landing(
        "---\ntitle: \"x\"\nsite:\n  index: true\n---\nbody\n"
    ));
    assert!(!is_section_landing("---\ntitle: \"x\"\n---\nindex: true\n"));
}
