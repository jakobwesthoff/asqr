// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The colours of the pictures: Gruvbox Material Hard dark, by sainnhe
//! (<https://github.com/sainnhe/gruvbox-material>, MIT), as a terminal's
//! background, foreground and 16-colour palette.

use ratatui::style::Color;

/// An sRGB colour.
pub type Rgb = [u8; 3];

pub const BACKGROUND: Rgb = [0x1d, 0x20, 0x21];
pub const FOREGROUND: Rgb = [0xd4, 0xbe, 0x98];

/// The 16 colours of the palette. Entries 9 to 15 repeat 1 to 7.
pub const PALETTE: [Rgb; 16] = [
    [0x1d, 0x20, 0x21],
    [0xea, 0x69, 0x62],
    [0xa9, 0xb6, 0x65],
    [0xd8, 0xa6, 0x57],
    [0x7d, 0xae, 0xa3],
    [0xd3, 0x86, 0x9b],
    [0x89, 0xb4, 0x82],
    [0xd4, 0xbe, 0x98],
    [0xed, 0xde, 0xb5],
    [0xea, 0x69, 0x62],
    [0xa9, 0xb6, 0x65],
    [0xd8, 0xa6, 0x57],
    [0x7d, 0xae, 0xa3],
    [0xd3, 0x86, 0x9b],
    [0x89, 0xb4, 0x82],
    [0xd4, 0xbe, 0x98],
];

/// The sRGB value of `color`, with `default` for the terminal's own
/// colour (`Color::Reset`).
///
/// ratatui's named colours are the 16 ANSI colours: the eight of SGR
/// 30-37 and 40-47 take entries 0 to 7, the bright ones of 90-97 and
/// 100-107 entries 8 to 15. Indexed colours above 15 are xterm's 256-colour
/// cube and grey ramp.
pub fn rgb(color: Color, default: Rgb) -> Rgb {
    match color {
        Color::Reset => default,
        Color::Black => PALETTE[0],
        Color::Red => PALETTE[1],
        Color::Green => PALETTE[2],
        Color::Yellow => PALETTE[3],
        Color::Blue => PALETTE[4],
        Color::Magenta => PALETTE[5],
        Color::Cyan => PALETTE[6],
        Color::Gray => PALETTE[7],
        Color::DarkGray => PALETTE[8],
        Color::LightRed => PALETTE[9],
        Color::LightGreen => PALETTE[10],
        Color::LightYellow => PALETTE[11],
        Color::LightBlue => PALETTE[12],
        Color::LightMagenta => PALETTE[13],
        Color::LightCyan => PALETTE[14],
        Color::White => PALETTE[15],
        Color::Indexed(index @ 0..=15) => PALETTE[usize::from(index)],
        Color::Indexed(index @ 16..=231) => {
            const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let cube = usize::from(index - 16);
            [STEPS[cube / 36], STEPS[cube / 6 % 6], STEPS[cube % 6]]
        }
        Color::Indexed(index) => {
            let grey = 8 + 10 * (index - 232);
            [grey; 3]
        }
        Color::Rgb(red, green, blue) => [red, green, blue],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rows(rows: &[(Color, Rgb)]) {
        let failures: Vec<String> = rows
            .iter()
            .filter(|(color, expected)| rgb(*color, [1, 2, 3]) != *expected)
            .map(|(color, expected)| {
                format!(
                    "{color:?}: {:02x?}, not {expected:02x?}",
                    rgb(*color, [1, 2, 3])
                )
            })
            .collect();
        assert!(
            failures.is_empty(),
            "failing rows:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn reset_takes_the_default_colour() {
        assert_eq!(rgb(Color::Reset, FOREGROUND), FOREGROUND);
        assert_eq!(rgb(Color::Reset, BACKGROUND), BACKGROUND);
    }

    /// ANSI 30-37 and 40-47.
    #[test]
    fn named_colours_take_palette_0_to_7() {
        assert_rows(&[
            (Color::Black, [0x1d, 0x20, 0x21]),
            (Color::Red, [0xea, 0x69, 0x62]),
            (Color::Green, [0xa9, 0xb6, 0x65]),
            (Color::Yellow, [0xd8, 0xa6, 0x57]),
            (Color::Blue, [0x7d, 0xae, 0xa3]),
            (Color::Magenta, [0xd3, 0x86, 0x9b]),
            (Color::Cyan, [0x89, 0xb4, 0x82]),
            (Color::Gray, [0xd4, 0xbe, 0x98]),
        ]);
    }

    /// ANSI 90-97 and 100-107.
    #[test]
    fn bright_colours_take_palette_8_to_15() {
        assert_rows(&[
            (Color::DarkGray, [0xed, 0xde, 0xb5]),
            (Color::LightRed, [0xea, 0x69, 0x62]),
            (Color::LightGreen, [0xa9, 0xb6, 0x65]),
            (Color::LightYellow, [0xd8, 0xa6, 0x57]),
            (Color::LightBlue, [0x7d, 0xae, 0xa3]),
            (Color::LightMagenta, [0xd3, 0x86, 0x9b]),
            (Color::LightCyan, [0x89, 0xb4, 0x82]),
            (Color::White, [0xd4, 0xbe, 0x98]),
        ]);
    }

    #[test]
    fn indexed_0_to_15_take_the_palette() {
        assert_rows(&[
            (Color::Indexed(0), [0x1d, 0x20, 0x21]),
            (Color::Indexed(3), [0xd8, 0xa6, 0x57]),
            (Color::Indexed(8), [0xed, 0xde, 0xb5]),
            (Color::Indexed(15), [0xd4, 0xbe, 0x98]),
        ]);
    }

    #[test]
    fn indexed_16_to_231_follow_the_xterm_cube() {
        assert_rows(&[
            (Color::Indexed(16), [0, 0, 0]),
            (Color::Indexed(17), [0, 0, 95]),
            (Color::Indexed(196), [255, 0, 0]),
            (Color::Indexed(231), [255, 255, 255]),
        ]);
    }

    #[test]
    fn indexed_232_to_255_follow_the_grey_ramp() {
        assert_rows(&[
            (Color::Indexed(232), [8, 8, 8]),
            (Color::Indexed(255), [238, 238, 238]),
        ]);
    }

    #[test]
    fn rgb_colours_stay() {
        assert_eq!(rgb(Color::Rgb(1, 2, 250), FOREGROUND), [1, 2, 250]);
    }
}
