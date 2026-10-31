// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The committed scenes draw, and the binary writes their pictures.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use image::codecs::png::CompressionType;
use screens::fonts::{CELL_HEIGHT, CELL_WIDTH};
use screens::raster::{PADDING, encode_png};
use screens::scene::read_scene;
use screens::{LOAD_CAP, scenes_dir};

fn scene_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(scenes_dir())
        .expect("the scenes directory is readable")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    files.sort();
    files
}

/// The picture of the scene in `file`, as PNG bytes.
fn picture(file: &Path) -> Vec<u8> {
    let scene = read_scene(file).expect("the scene parses");
    let image = screens::draw(&scene, file, LOAD_CAP).expect("the scene draws");
    encode_png(&image, CompressionType::Fast)
}

/// Every scene draws, with every glyph in the fonts, into a picture of its
/// size.
#[test]
fn every_scene_draws() {
    let files = scene_files();
    assert!(files.len() >= 5, "{files:?}");
    let failures: Vec<String> = files
        .iter()
        .filter_map(|file| {
            let scene = read_scene(file).ok()?;
            let expected = (
                u32::from(scene.columns) * CELL_WIDTH + 2 * PADDING,
                u32::from(scene.rows) * CELL_HEIGHT + 2 * PADDING,
            );
            let size = image::load_from_memory(&picture(file))
                .map(|image| (image.width(), image.height()))
                .ok();
            (size != Some(expected))
                .then(|| format!("{}: {size:?}, not {expected:?}", file.display()))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_binary_writes_a_picture_per_scene_and_the_index() {
    let out = tempfile::tempdir().expect("a temporary directory");

    let output = Command::cargo_bin("screens")
        .expect("the binary is built")
        .arg("--out")
        .arg(out.path())
        .arg(scenes_dir().join("help.toml"))
        .output()
        .expect("runs");

    assert!(output.status.success(), "{output:?}");
    let picture = out.path().join("help.png");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&picture.display().to_string()), "{stdout}");
    let size = image::open(&picture)
        .map(|image| (image.width(), image.height()))
        .ok();
    assert_eq!(size, Some((94 * 15 + 20, 24 * 39 + 20)));
    let index = std::fs::read_to_string(out.path().join("index.md")).unwrap_or_default();
    assert!(index.contains("(help.png)"), "{index}");
}

#[test]
fn the_binary_names_what_went_wrong() {
    let output = Command::cargo_bin("screens")
        .expect("the binary is built")
        .arg("/nowhere/scene.toml")
        .output()
        .expect("runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error: ") && stderr.contains("/nowhere/scene.toml"),
        "{stderr}"
    );
}

/// Guard against a picture that depends on anything but its scene; its
/// first assertion fails on an empty picture.
#[test]
fn a_scene_drawn_twice_gives_the_same_bytes() {
    let file = scenes_dir().join("session-with-context.toml");
    let first = picture(&file);

    assert!(image::load_from_memory(&first).is_ok());
    assert_eq!(first, picture(&file));
}
