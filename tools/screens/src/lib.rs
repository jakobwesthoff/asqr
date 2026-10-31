// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Draws asqr's screens with asqr's own code into PNG pictures for the
//! documentation (ADR 89). The README next to this crate describes how to
//! run it and the scene files it reads.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use image::RgbImage;
use image::codecs::png::CompressionType;

use crate::play::play;
use crate::raster::{encode_png, rasterize};
use crate::scene::{Scene, image_file, read_scene};

pub mod fonts;
pub mod paint;
pub mod palette;
pub mod picture;
pub mod play;
pub mod raster;
pub mod scene;

/// How long a scene waits for a context entry to load before it fails.
pub const LOAD_CAP: Duration = Duration::from_secs(60);

/// Draws asqr's screens from scene files into PNG pictures.
#[derive(Debug, Parser)]
pub struct Cli {
    /// Where the pictures go; `target/screens` of the workspace without
    /// one.
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// The scene files to draw, or directories of them; every one in
    /// `tools/screens/scenes` without any.
    pub scenes: Vec<PathBuf>,
}

/// `target/screens` of the workspace this crate belongs to, two levels
/// above it.
pub fn default_out() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate lies in tools/ of the workspace")
        .join("target/screens")
}

/// The scenes this crate keeps.
pub fn scenes_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenes")
}

/// Draws the scenes `cli` names and returns the files it wrote.
pub fn run(cli: Cli) -> Result<Vec<PathBuf>> {
    let scenes = scene_files(cli.scenes)?;
    let out = cli.out.unwrap_or_else(default_out);
    draw_scenes(&scenes, &out, CompressionType::Best)
}

/// The scene files `paths` name: files as they are, and every `.toml`
/// file of a directory, by name. Without any, the scenes this crate keeps.
pub fn scene_files(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let paths = if paths.is_empty() {
        vec![scenes_dir()]
    } else {
        paths
    };
    let mut scenes = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(&path)
                .with_context(|| format!("reading the scenes in {}", path.display()))?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|file| {
                    file.extension()
                        .is_some_and(|extension| extension == "toml")
                })
                .collect();
            if found.is_empty() {
                bail!("no scene files in {}", path.display());
            }
            found.sort();
            scenes.extend(found);
        } else {
            scenes.push(path);
        }
    }
    Ok(scenes)
}

/// The picture of `scene`, read from `scene_file`, waiting at most `cap`
/// for each context entry to load. A scene with an `image` gets that
/// picture painted where the screen shows the image ([`picture`]).
pub fn draw(scene: &Scene, scene_file: &Path, cap: Duration) -> Result<RgbImage> {
    let image = image_file(scene, scene_file)?
        .map(|file| {
            image::open(&file).with_context(|| format!("reading the image {}", file.display()))
        })
        .transpose()?;
    let mut buffer = play(scene, scene_file, cap)?;
    let Some(image) = image else {
        return Ok(rasterize(&buffer)?);
    };
    let cells = picture::image_cells(&buffer)?;
    picture::clear_cells(&mut buffer, cells);
    let mut drawn = rasterize(&buffer)?;
    picture::paint(&mut drawn, cells, &image);
    Ok(drawn)
}

/// Draws each scene file of `scenes` into `out`, as `<name>.png`, and
/// writes `index.md` there, which shows every picture under its caption.
/// Returns the files it wrote.
pub fn draw_scenes(
    scenes: &[PathBuf],
    out: &Path,
    compression: CompressionType,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out)
        .with_context(|| format!("creating the output directory {}", out.display()))?;
    let mut written = Vec::new();
    let mut index = String::new();
    for file in scenes {
        let scene = read_scene(file)?;
        let picture = draw(&scene, file, LOAD_CAP)
            .with_context(|| format!("drawing the scene {}", file.display()))?;
        let name = file
            .file_stem()
            .map(|stem| format!("{}.png", stem.to_string_lossy()))
            .with_context(|| format!("naming the picture of {}", file.display()))?;
        let png = out.join(&name);
        std::fs::write(&png, encode_png(&picture, compression))
            .with_context(|| format!("writing {}", png.display()))?;
        written.push(png);
        index.push_str(&format!("![{}]({name})\n\n", scene.caption));
    }
    let index_file = out.join("index.md");
    std::fs::write(&index_file, index)
        .with_context(|| format!("writing {}", index_file.display()))?;
    written.push(index_file);
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_of(result: Result<Vec<PathBuf>>) -> String {
        match result {
            Ok(_) => "no error".to_owned(),
            Err(error) => format!("{error:#}"),
        }
    }

    fn cli(out: &Path, scenes: &[&Path]) -> Cli {
        Cli {
            out: Some(out.to_path_buf()),
            scenes: scenes.iter().map(|path| path.to_path_buf()).collect(),
        }
    }

    #[test]
    fn the_pictures_go_to_target_screens_of_the_workspace() {
        let out = default_out();

        assert!(out.ends_with("target/screens"), "{}", out.display());
        let root = out
            .parent()
            .and_then(Path::parent)
            .expect("the output directory lies two levels down");
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
        assert!(manifest.contains("[workspace]"), "{}", root.display());
    }

    #[test]
    fn a_missing_scene_file_is_an_error() {
        let out = tempfile::tempdir().expect("a temporary directory");
        let missing = out.path().join("missing.toml");

        let error = error_of(run(cli(out.path(), &[&missing])));

        assert!(error.contains("missing.toml"), "{error}");
    }

    #[test]
    fn a_scene_that_does_not_parse_is_an_error() {
        let out = tempfile::tempdir().expect("a temporary directory");
        let broken = out.path().join("broken.toml");
        std::fs::write(&broken, "caption = ").expect("written");

        let error = error_of(run(cli(out.path(), &[&broken])));

        assert!(
            error.contains("reading the scene file") && error.contains("broken.toml"),
            "{error}"
        );
    }

    #[test]
    fn an_output_directory_that_cannot_be_made_is_an_error() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let file = dir.path().join("file");
        std::fs::write(&file, "").expect("written");

        let error = error_of(run(cli(
            &file.join("pictures"),
            &[&scenes_dir().join("help.toml")],
        )));

        assert!(error.contains("creating the output directory"), "{error}");
    }

    #[test]
    fn without_scenes_named_every_scene_of_the_crate_is_drawn() {
        let files = scene_files(Vec::new()).expect("the scenes are listed");

        let names: Vec<String> = files
            .iter()
            .filter_map(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        assert!(
            names.windows(2).all(|pair| pair[0] < pair[1]),
            "the scenes come sorted by name: {names:?}"
        );
        for scene in ["empty-queue.toml", "help.toml", "queue-switcher.toml"] {
            assert!(names.iter().any(|name| name == scene), "{names:?}");
        }
        assert!(
            names.iter().all(|name| name.ends_with(".toml")),
            "only scene files: {names:?}"
        );
    }

    #[test]
    fn a_directory_stands_for_its_scene_files_and_a_file_for_itself() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for name in ["b.toml", "a.toml", "notes.md"] {
            std::fs::write(dir.path().join(name), "").expect("written");
        }
        let single = PathBuf::from("single.toml");

        let files = scene_files(vec![dir.path().to_path_buf(), single.clone()])
            .expect("the scenes are listed");

        assert_eq!(
            files,
            [dir.path().join("a.toml"), dir.path().join("b.toml"), single]
        );
    }

    #[test]
    fn a_directory_without_scenes_is_an_error() {
        let dir = tempfile::tempdir().expect("a temporary directory");

        let error = error_of(run(cli(dir.path(), &[dir.path()])));

        assert!(error.contains("no scene files in"), "{error}");
    }

    #[test]
    fn the_index_shows_each_picture_under_its_caption() {
        let out = tempfile::tempdir().expect("a temporary directory");

        let written = draw_scenes(
            &[scenes_dir().join("help.toml")],
            out.path(),
            CompressionType::Fast,
        )
        .expect("draws");

        assert_eq!(
            written,
            [out.path().join("help.png"), out.path().join("index.md")]
        );
        let index = std::fs::read_to_string(out.path().join("index.md")).unwrap_or_default();
        assert!(
            index
                .contains("![The help over a session, with the screen behind it dimmed](help.png)"),
            "{index}"
        );
    }

    // =================================================================
    // A picture painted where the image shows
    // =================================================================

    const RED: [u8; 3] = [220, 30, 30];

    /// A scene in `dir` whose one question shows `picture.png` at its
    /// natural size, with `image` naming that picture, and the scene file.
    /// The picture is 37 by 39 pixels: red in its first 20 columns and
    /// transparent in the rest, so its edge falls inside its second cell.
    fn picture_scene(dir: &Path) -> PathBuf {
        let picture = image::RgbaImage::from_fn(37, 39, |x, _| {
            if x < 20 {
                image::Rgba([RED[0], RED[1], RED[2], 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        picture
            .save(dir.join("picture.png"))
            .expect("the picture is written");
        std::fs::write(
            dir.join("session.json"),
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
                "context": [{"type": "image", "path": "picture.png", "enlarge": false}]}]}"#,
        )
        .expect("the session is written");
        let scene_file = dir.join("scene.toml");
        std::fs::write(
            &scene_file,
            "caption = \"c\"\ncolumns = 120\nrows = 30\nimage = \"picture.png\"\n\
             [[sessions]]\nfile = \"session.json\"\n",
        )
        .expect("the scene is written");
        scene_file
    }

    /// The top left cell of the cells half blocks drew the image into: those
    /// with an RGB background, which asqr's own drawing never has.
    fn image_origin(buffer: &ratatui::buffer::Buffer) -> (u32, u32) {
        let area = buffer.area;
        (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| (x, y)))
            .find(|&(x, y)| matches!(buffer[(x, y)].bg, ratatui::style::Color::Rgb(..)))
            .map(|(x, y)| {
                (
                    raster::PADDING + u32::from(x) * fonts::CELL_WIDTH,
                    raster::PADDING + u32::from(y) * fonts::CELL_HEIGHT,
                )
            })
            .expect("the image shows in half blocks")
    }

    #[test]
    fn the_scene_s_picture_is_painted_into_the_image_s_cells_over_the_background() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let scene_file = picture_scene(dir.path());
        let scene = read_scene(&scene_file).expect("the scene parses");
        let (left, top) = image_origin(&play(&scene, &scene_file, LOAD_CAP).expect("plays"));

        let picture = draw(&scene, &scene_file, LOAD_CAP).expect("draws");

        let at = |x: u32| picture.get_pixel(left + x, top + 10).0;
        // Pixels 16 and 25 lie in the image's second cell, which a half
        // block can only fill in one colour from side to side.
        assert_eq!(at(16), RED, "an opaque pixel of the picture");
        assert_eq!(at(25), palette::BACKGROUND, "a transparent pixel");
        assert_eq!(at(40), palette::BACKGROUND, "the padding after the picture");
    }
}
