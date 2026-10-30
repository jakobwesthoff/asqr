// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The fonts the pictures are drawn with, which face draws which
//! character, and the size of a cell.

use std::sync::LazyLock;

use ab_glyph::{Font, FontRef, PxScale};

// =====================================================================
// The cell
// =====================================================================
//
// The cells have the proportions Ghostty gives "VictorMono Nerd Font
// Mono" (the Nerd Fonts build of Victor Mono 1.560) at 14 pt on a display
// of content scale 2. The numbers follow Ghostty's rules at commit
// fbed180a5f18:
//
// - points to pixels: `points * ydpi / 72`, with `ydpi = content_scale *
//   72` on macOS (`src/font/face.zig`, `src/Surface.zig`): 28 px per em;
// - with USE_TYPO_METRICS set, the OS/2 typo ascender, descender and line
//   gap (`src/font/face/coretext.zig`, `getMetrics`), for that build 1200,
//   -350 and 0 of 1100 units per em; the cell width is the widest advance
//   of ASCII, 600 units;
// - `src/font/Metrics.zig`, `calc`: the width and the line height
//   rounded, the baseline centred in the rounded height with half the line
//   gap above it, the underline and the strikethrough from the post and
//   OS/2 tables (-75/50 and 370/50 units), and box lines as thick as the
//   underline.
//
// The upstream file committed here gives the same numbers: its typo
// metrics (1100, -250, 200) make the same line height and put the
// baseline as far from the bottom. A test checks that it still does.

/// Pixels per em of every face.
pub const EM_PX: f32 = 28.0;
pub const CELL_WIDTH: u32 = 15;
pub const CELL_HEIGHT: u32 = 39;
/// The baseline's row, from the cell's top.
pub const BASELINE: u32 = 30;
/// The first row of an underline, from the cell's top.
pub const UNDERLINE_TOP: u32 = 32;
pub const UNDERLINE_THICKNESS: u32 = 2;
/// The first row of a strikethrough, from the cell's top.
pub const STRIKE_TOP: u32 = 21;
pub const STRIKE_THICKNESS: u32 = 2;
/// The width of a light box-drawing line; a heavy one is twice as wide.
pub const BOX_THICKNESS: u32 = 2;

// =====================================================================
// The faces
// =====================================================================

/// A font face the pictures draw with, by the file it comes from.
pub struct Face {
    pub file: &'static str,
    pub font: FontRef<'static>,
    /// Whether the face is the text face, whose glyphs fill the cells as
    /// a monospace font does, rather than a fallback.
    pub text: bool,
}

impl Face {
    fn text(file: &'static str, bytes: &'static [u8]) -> Face {
        Face {
            file,
            font: FontRef::try_from_slice(bytes).expect("the committed font files parse"),
            text: true,
        }
    }

    fn fallback(file: &'static str, bytes: &'static [u8]) -> Face {
        Face {
            text: false,
            ..Face::text(file, bytes)
        }
    }

    /// The scale that draws this face at [`EM_PX`] pixels per em. ab_glyph
    /// scales by the height from ascent to descent, not by the em.
    pub fn scale(&self) -> PxScale {
        let units_per_em = self
            .font
            .units_per_em()
            .expect("the committed fonts have units per em");
        PxScale::from(EM_PX * self.font.height_unscaled() / units_per_em)
    }

    /// Whether this face has a glyph for `ch`.
    pub fn has(&self, ch: char) -> bool {
        self.font.glyph_id(ch).0 != 0
    }
}

pub(crate) const VICTOR_MONO_SEMIBOLD: &[u8] =
    include_bytes!("../fonts/victor-mono/VictorMono-SemiBold.ttf");

/// Victor Mono in its four styles, indexed by [`victor_index`].
static VICTOR_MONO: LazyLock<[Face; 4]> = LazyLock::new(|| {
    [
        Face::text("VictorMono-SemiBold.ttf", VICTOR_MONO_SEMIBOLD),
        Face::text(
            "VictorMono-Bold.ttf",
            include_bytes!("../fonts/victor-mono/VictorMono-Bold.ttf"),
        ),
        Face::text(
            "VictorMono-SemiBoldItalic.ttf",
            include_bytes!("../fonts/victor-mono/VictorMono-SemiBoldItalic.ttf"),
        ),
        Face::text(
            "VictorMono-BoldItalic.ttf",
            include_bytes!("../fonts/victor-mono/VictorMono-BoldItalic.ttf"),
        ),
    ]
});

fn victor_index(bold: bool, italic: bool) -> usize {
    usize::from(bold) + 2 * usize::from(italic)
}

/// What Victor Mono lacks comes from these, in this order: the box marks,
/// checks, pointers and the return sign from DejaVu Sans Mono in the
/// cell's weight, `ℹ` from DejaVu Sans, the Control Pictures from Noto
/// Sans Symbols 2.
struct Fallbacks {
    dejavu_sans_mono: Face,
    dejavu_sans_mono_bold: Face,
    dejavu_sans: Face,
    noto_sans_symbols_2: Face,
}

static FALLBACKS: LazyLock<Fallbacks> = LazyLock::new(|| Fallbacks {
    dejavu_sans_mono: Face::fallback(
        "DejaVuSansMono.ttf",
        include_bytes!("../fonts/dejavu/DejaVuSansMono.ttf"),
    ),
    dejavu_sans_mono_bold: Face::fallback(
        "DejaVuSansMono-Bold.ttf",
        include_bytes!("../fonts/dejavu/DejaVuSansMono-Bold.ttf"),
    ),
    dejavu_sans: Face::fallback(
        "DejaVuSans.ttf",
        include_bytes!("../fonts/dejavu/DejaVuSans.ttf"),
    ),
    noto_sans_symbols_2: Face::fallback(
        "NotoSansSymbols2-Regular.ttf",
        include_bytes!("../fonts/noto-sans-symbols-2/NotoSansSymbols2-Regular.ttf"),
    ),
});

/// Every face, for the tests that check them all.
#[cfg(test)]
fn every_face() -> Vec<&'static Face> {
    let fallbacks = &*FALLBACKS;
    VICTOR_MONO
        .iter()
        .chain([
            &fallbacks.dejavu_sans_mono,
            &fallbacks.dejavu_sans_mono_bold,
            &fallbacks.dejavu_sans,
            &fallbacks.noto_sans_symbols_2,
        ])
        .collect()
}

// =====================================================================
// Which face draws a character
// =====================================================================

/// Where the pixels of a character come from.
pub enum Source {
    /// Painted cell by cell, never taken from a font.
    Painted,
    Font(&'static Face),
    /// No committed font has the character.
    Missing,
}

/// Whether `ch` is painted rather than drawn from a font: the ranges
/// Ghostty draws as sprites of its own (`src/font/sprite/draw/`), Box
/// Drawing, Block Elements and the sextants, so lines and Talqi's cells
/// meet without seams.
pub fn is_painted(ch: char) -> bool {
    matches!(u32::from(ch), 0x2500..=0x259F | 0x1FB00..=0x1FB3B)
}

/// The source of `ch` in a cell drawn `bold` and `italic`.
pub fn source_for(ch: char, bold: bool, italic: bool) -> Source {
    if is_painted(ch) {
        return Source::Painted;
    }
    let fallbacks = &*FALLBACKS;
    let dejavu_sans_mono = if bold {
        &fallbacks.dejavu_sans_mono_bold
    } else {
        &fallbacks.dejavu_sans_mono
    };
    [
        &VICTOR_MONO[victor_index(bold, italic)],
        dejavu_sans_mono,
        &fallbacks.dejavu_sans,
        &fallbacks.noto_sans_symbols_2,
    ]
    .into_iter()
    .find(|face| face.has(ch))
    .map_or(Source::Missing, Source::Font)
}

#[cfg(test)]
mod tests {
    use ab_glyph::ScaleFont;

    use super::*;

    /// The file of the face `source` names, or what else it is.
    fn file_of(source: Source) -> String {
        match source {
            Source::Painted => "painted".to_owned(),
            Source::Font(face) => face.file.to_owned(),
            Source::Missing => "missing".to_owned(),
        }
    }

    /// Checks every row and names each one that fails, so one wrong row
    /// does not hide another.
    fn assert_rows(failures: Vec<String>) {
        assert!(
            failures.is_empty(),
            "failing rows:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn victor_mono_draws_ascii_in_the_cell_s_style() {
        let rows = [
            (false, false, "VictorMono-SemiBold.ttf"),
            (true, false, "VictorMono-Bold.ttf"),
            (false, true, "VictorMono-SemiBoldItalic.ttf"),
            (true, true, "VictorMono-BoldItalic.ttf"),
        ];
        let failures = rows
            .into_iter()
            .filter_map(|(bold, italic, expected)| {
                let file = file_of(source_for('a', bold, italic));
                (file != expected)
                    .then(|| format!("bold {bold}, italic {italic}: {file}, not {expected}"))
            })
            .collect();
        assert_rows(failures);
    }

    #[test]
    fn each_fallback_takes_what_the_ones_before_lack() {
        let rows = [
            ('x', false, "VictorMono-SemiBold.ttf"),
            ('☐', false, "DejaVuSansMono.ttf"),
            ('☐', true, "DejaVuSansMono-Bold.ttf"),
            ('ℹ', false, "DejaVuSans.ttf"),
            ('␛', false, "NotoSansSymbols2-Regular.ttf"),
        ];
        let failures = rows
            .into_iter()
            .filter_map(|(ch, bold, expected)| {
                let file = file_of(source_for(ch, bold, false));
                (file != expected).then(|| format!("{ch:?} bold {bold}: {file}, not {expected}"))
            })
            .collect();
        assert_rows(failures);
    }

    #[test]
    fn the_painted_ranges_are_painted() {
        let rows = [
            '\u{2500}',
            '\u{257F}',
            '\u{2580}',
            '\u{259F}',
            '\u{1FB00}',
            '\u{1FB3B}',
            '▀',
            '🬞',
        ];
        let failures = rows
            .into_iter()
            .filter_map(|ch| {
                let file = file_of(source_for(ch, false, false));
                (file != "painted").then(|| format!("U+{:04X}: {file}", u32::from(ch)))
            })
            .collect();
        assert_rows(failures);
    }

    /// Guard: the stub returns `Missing` for every character too.
    #[test]
    fn a_character_no_font_has_is_missing() {
        assert_eq!(file_of(source_for('\u{10FFFD}', false, false)), "missing");
    }

    #[test]
    fn every_face_draws_at_28_pixels_per_em() {
        let failures = every_face()
            .into_iter()
            .filter_map(|face| {
                let font = &face.font;
                let glyph = font.glyph_id('a');
                let upm = font.units_per_em().expect("every face has units per em");
                let expected = font.h_advance_unscaled(glyph) * 28.0 / upm;
                let drawn = font.as_scaled(face.scale()).h_advance(glyph);
                ((drawn - expected).abs() > 0.01)
                    .then(|| format!("{}: advance of a {drawn}, not {expected}", face.file))
            })
            .collect();
        assert_rows(failures);
    }

    /// Ghostty's cell metrics for a face at 14 pt on a display of content
    /// scale 2 (`src/font/face.zig`, `src/font/face/coretext.zig`,
    /// `src/font/Metrics.zig` at fbed180a5f18), applied to the committed
    /// upstream file, which has the line height and the depth of the
    /// baseline of the Nerd Fonts build the constants come from.
    #[test]
    fn the_upstream_font_yields_ghostty_s_cell_constants() {
        let face = ttf_parser::Face::parse(VICTOR_MONO_SEMIBOLD, 0).expect("the font parses");
        let px_per_unit = 14.0 * 144.0 / 72.0 / f64::from(face.units_per_em());
        let os2 = face.tables().os2.expect("the font has an OS/2 table");
        assert!(
            os2.use_typographic_metrics(),
            "Ghostty reads the typo metrics"
        );
        let ascent = f64::from(os2.typographic_ascender()) * px_per_unit;
        let descent = f64::from(os2.typographic_descender()) * px_per_unit;
        let line_gap = f64::from(os2.typographic_line_gap()) * px_per_unit;
        let widest_ascii = (32..127u8)
            .filter_map(|byte| face.glyph_index(char::from(byte)))
            .filter_map(|glyph| face.glyph_hor_advance(glyph))
            .max()
            .expect("the font has ASCII");
        let face_width = f64::from(widest_ascii) * px_per_unit;
        let face_height = ascent - descent + line_gap;
        let cell_width = face_width.round();
        let cell_height = face_height.round();
        let cell_baseline = (line_gap / 2.0 - descent - (cell_height - face_height) / 2.0).round();
        let top_to_baseline = cell_height - cell_baseline;
        let underline = face
            .underline_metrics()
            .expect("the font has underline metrics");
        let strikeout = face
            .strikeout_metrics()
            .expect("the font has strikeout metrics");
        let thickness = |units: i16| (f64::from(units) * px_per_unit).ceil().max(1.0);
        let top = |position: i16| (top_to_baseline - f64::from(position) * px_per_unit).round();

        let rows = [
            ("EM_PX", f64::from(EM_PX), 28.0),
            ("CELL_WIDTH", f64::from(CELL_WIDTH), cell_width),
            ("CELL_HEIGHT", f64::from(CELL_HEIGHT), cell_height),
            ("BASELINE", f64::from(BASELINE), top_to_baseline),
            (
                "UNDERLINE_TOP",
                f64::from(UNDERLINE_TOP),
                top(underline.position),
            ),
            (
                "UNDERLINE_THICKNESS",
                f64::from(UNDERLINE_THICKNESS),
                thickness(underline.thickness),
            ),
            ("STRIKE_TOP", f64::from(STRIKE_TOP), top(strikeout.position)),
            (
                "STRIKE_THICKNESS",
                f64::from(STRIKE_THICKNESS),
                thickness(strikeout.thickness),
            ),
            (
                "BOX_THICKNESS",
                f64::from(BOX_THICKNESS),
                thickness(underline.thickness),
            ),
        ];
        let failures = rows
            .into_iter()
            .filter(|(_, constant, computed)| constant != computed)
            .map(|(name, constant, computed)| {
                format!("{name} is {constant}, Ghostty gives {computed}")
            })
            .collect();
        assert_rows(failures);
    }
}
