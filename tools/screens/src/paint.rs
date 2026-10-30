// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The characters painted cell by cell rather than drawn from a font: box
//! drawing, block elements and the sextants.
//!
//! The geometry follows the sprites Ghostty draws for these ranges
//! (`src/font/sprite/draw/box.zig`, `block.zig`, `common.zig` and
//! `symbols_for_legacy_computing.zig` in ghostty-org/ghostty at commit
//! fbed180a5f18, MIT, `LICENSE-ghostty` next to this crate): the same
//! rounding of fractions of a cell, the same line widths and the same
//! curves, so lines and Talqi's cells meet as they do there.

use crate::fonts::{BOX_THICKNESS, CELL_HEIGHT, CELL_WIDTH};

/// How much of each pixel of one cell a painted character covers, 0 to
/// 255, row by row.
pub struct Mask {
    alpha: Vec<u8>,
}

impl Mask {
    fn empty() -> Mask {
        Mask {
            alpha: vec![0; (CELL_WIDTH * CELL_HEIGHT) as usize],
        }
    }

    pub fn at(&self, x: u32, y: u32) -> u8 {
        self.alpha[(y * CELL_WIDTH + x) as usize]
    }

    /// Covers the pixels `x0..x1` by `y0..y1` with `alpha`, as far as they
    /// lie in the cell. A pixel covered twice keeps the stronger cover.
    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, alpha: u8) {
        let clamp = |value: i32, size: u32| value.clamp(0, size as i32) as u32;
        for y in clamp(y0, CELL_HEIGHT)..clamp(y1, CELL_HEIGHT) {
            for x in clamp(x0, CELL_WIDTH)..clamp(x1, CELL_WIDTH) {
                let pixel = &mut self.alpha[(y * CELL_WIDTH + x) as usize];
                *pixel = (*pixel).max(alpha);
            }
        }
    }

    /// Strokes the open path through `points` with a line `width` pixels
    /// wide, anti-aliased by each pixel centre's distance to the path.
    fn stroke(&mut self, points: &[(f64, f64)], width: f64) {
        for y in 0..CELL_HEIGHT {
            for x in 0..CELL_WIDTH {
                let centre = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let distance = points
                    .windows(2)
                    .map(|segment| distance_to_segment(centre, segment[0], segment[1]))
                    .fold(f64::INFINITY, f64::min);
                let cover = (width / 2.0 + 0.5 - distance).clamp(0.0, 1.0);
                let pixel = &mut self.alpha[(y * CELL_WIDTH + x) as usize];
                *pixel = (*pixel).max((cover * 255.0).round() as u8);
            }
        }
    }
}

fn distance_to_segment(point: (f64, f64), start: (f64, f64), end: (f64, f64)) -> f64 {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let length = dx * dx + dy * dy;
    let along = if length == 0.0 {
        0.0
    } else {
        (((point.0 - start.0) * dx + (point.1 - start.1) * dy) / length).clamp(0.0, 1.0)
    };
    let nearest = (start.0 + along * dx, start.1 + along * dy);
    ((point.0 - nearest.0).powi(2) + (point.1 - nearest.1).powi(2)).sqrt()
}

// =====================================================================
// Fractions of a cell
// =====================================================================

/// Where a fraction `f` of `size` lies as the start of a filled span:
/// counted from the far edge, so that `0..max(f)` and `min(f)..size` meet
/// without a gap whatever the rounding (`common.zig`, `Fraction.min`).
fn min(f: f64, size: u32) -> i32 {
    let size = f64::from(size);
    (size - ((1.0 - f) * size).round()) as i32
}

/// Where a fraction `f` of `size` lies as the end of a filled span
/// (`common.zig`, `Fraction.max`).
fn max(f: f64, size: u32) -> i32 {
    (f * f64::from(size)).round() as i32
}

/// Fills the part of the cell between the fractions `x0..x1` of its width
/// and `y0..y1` of its height (`common.zig`, `fill`).
fn fill_fractions(mask: &mut Mask, x0: f64, x1: f64, y0: f64, y1: f64) {
    mask.fill(
        min(x0, CELL_WIDTH),
        min(y0, CELL_HEIGHT),
        max(x1, CELL_WIDTH),
        max(y1, CELL_HEIGHT),
        255,
    );
}

/// The line characters, each with the style of its arms in the order up,
/// right, down, left: `n` none, `l` light, `h` heavy, `d` double. The
/// styles follow the characters' Unicode names, and Ghostty's table
/// (`box.zig`, `draw2500_257F`) agrees with every row.
pub(crate) const LINES: [(char, &str); 109] = [
    ('\u{2500}', "nlnl"), // Box Drawings Light Horizontal
    ('\u{2501}', "nhnh"), // Box Drawings Heavy Horizontal
    ('\u{2502}', "lnln"), // Box Drawings Light Vertical
    ('\u{2503}', "hnhn"), // Box Drawings Heavy Vertical
    ('\u{250C}', "nlln"), // Box Drawings Light Down And Right
    ('\u{250D}', "nhln"), // Box Drawings Down Light And Right Heavy
    ('\u{250E}', "nlhn"), // Box Drawings Down Heavy And Right Light
    ('\u{250F}', "nhhn"), // Box Drawings Heavy Down And Right
    ('\u{2510}', "nnll"), // Box Drawings Light Down And Left
    ('\u{2511}', "nnlh"), // Box Drawings Down Light And Left Heavy
    ('\u{2512}', "nnhl"), // Box Drawings Down Heavy And Left Light
    ('\u{2513}', "nnhh"), // Box Drawings Heavy Down And Left
    ('\u{2514}', "llnn"), // Box Drawings Light Up And Right
    ('\u{2515}', "lhnn"), // Box Drawings Up Light And Right Heavy
    ('\u{2516}', "hlnn"), // Box Drawings Up Heavy And Right Light
    ('\u{2517}', "hhnn"), // Box Drawings Heavy Up And Right
    ('\u{2518}', "lnnl"), // Box Drawings Light Up And Left
    ('\u{2519}', "lnnh"), // Box Drawings Up Light And Left Heavy
    ('\u{251A}', "hnnl"), // Box Drawings Up Heavy And Left Light
    ('\u{251B}', "hnnh"), // Box Drawings Heavy Up And Left
    ('\u{251C}', "llln"), // Box Drawings Light Vertical And Right
    ('\u{251D}', "lhln"), // Box Drawings Vertical Light And Right Heavy
    ('\u{251E}', "hlln"), // Box Drawings Up Heavy And Right Down Light
    ('\u{251F}', "llhn"), // Box Drawings Down Heavy And Right Up Light
    ('\u{2520}', "hlhn"), // Box Drawings Vertical Heavy And Right Light
    ('\u{2521}', "hhln"), // Box Drawings Down Light And Right Up Heavy
    ('\u{2522}', "lhhn"), // Box Drawings Up Light And Right Down Heavy
    ('\u{2523}', "hhhn"), // Box Drawings Heavy Vertical And Right
    ('\u{2524}', "lnll"), // Box Drawings Light Vertical And Left
    ('\u{2525}', "lnlh"), // Box Drawings Vertical Light And Left Heavy
    ('\u{2526}', "hnll"), // Box Drawings Up Heavy And Left Down Light
    ('\u{2527}', "lnhl"), // Box Drawings Down Heavy And Left Up Light
    ('\u{2528}', "hnhl"), // Box Drawings Vertical Heavy And Left Light
    ('\u{2529}', "hnlh"), // Box Drawings Down Light And Left Up Heavy
    ('\u{252A}', "lnhh"), // Box Drawings Up Light And Left Down Heavy
    ('\u{252B}', "hnhh"), // Box Drawings Heavy Vertical And Left
    ('\u{252C}', "nlll"), // Box Drawings Light Down And Horizontal
    ('\u{252D}', "nllh"), // Box Drawings Left Heavy And Right Down Light
    ('\u{252E}', "nhll"), // Box Drawings Right Heavy And Left Down Light
    ('\u{252F}', "nhlh"), // Box Drawings Down Light And Horizontal Heavy
    ('\u{2530}', "nlhl"), // Box Drawings Down Heavy And Horizontal Light
    ('\u{2531}', "nlhh"), // Box Drawings Right Light And Left Down Heavy
    ('\u{2532}', "nhhl"), // Box Drawings Left Light And Right Down Heavy
    ('\u{2533}', "nhhh"), // Box Drawings Heavy Down And Horizontal
    ('\u{2534}', "llnl"), // Box Drawings Light Up And Horizontal
    ('\u{2535}', "llnh"), // Box Drawings Left Heavy And Right Up Light
    ('\u{2536}', "lhnl"), // Box Drawings Right Heavy And Left Up Light
    ('\u{2537}', "lhnh"), // Box Drawings Up Light And Horizontal Heavy
    ('\u{2538}', "hlnl"), // Box Drawings Up Heavy And Horizontal Light
    ('\u{2539}', "hlnh"), // Box Drawings Right Light And Left Up Heavy
    ('\u{253A}', "hhnl"), // Box Drawings Left Light And Right Up Heavy
    ('\u{253B}', "hhnh"), // Box Drawings Heavy Up And Horizontal
    ('\u{253C}', "llll"), // Box Drawings Light Vertical And Horizontal
    ('\u{253D}', "lllh"), // Box Drawings Left Heavy And Right Vertical Light
    ('\u{253E}', "lhll"), // Box Drawings Right Heavy And Left Vertical Light
    ('\u{253F}', "lhlh"), // Box Drawings Vertical Light And Horizontal Heavy
    ('\u{2540}', "hlll"), // Box Drawings Up Heavy And Down Horizontal Light
    ('\u{2541}', "llhl"), // Box Drawings Down Heavy And Up Horizontal Light
    ('\u{2542}', "hlhl"), // Box Drawings Vertical Heavy And Horizontal Light
    ('\u{2543}', "hllh"), // Box Drawings Left Up Heavy And Right Down Light
    ('\u{2544}', "hhll"), // Box Drawings Right Up Heavy And Left Down Light
    ('\u{2545}', "llhh"), // Box Drawings Left Down Heavy And Right Up Light
    ('\u{2546}', "lhhl"), // Box Drawings Right Down Heavy And Left Up Light
    ('\u{2547}', "hhlh"), // Box Drawings Down Light And Up Horizontal Heavy
    ('\u{2548}', "lhhh"), // Box Drawings Up Light And Down Horizontal Heavy
    ('\u{2549}', "hlhh"), // Box Drawings Right Light And Left Vertical Heavy
    ('\u{254A}', "hhhl"), // Box Drawings Left Light And Right Vertical Heavy
    ('\u{254B}', "hhhh"), // Box Drawings Heavy Vertical And Horizontal
    ('\u{2550}', "ndnd"), // Box Drawings Double Horizontal
    ('\u{2551}', "dndn"), // Box Drawings Double Vertical
    ('\u{2552}', "ndln"), // Box Drawings Down Single And Right Double
    ('\u{2553}', "nldn"), // Box Drawings Down Double And Right Single
    ('\u{2554}', "nddn"), // Box Drawings Double Down And Right
    ('\u{2555}', "nnld"), // Box Drawings Down Single And Left Double
    ('\u{2556}', "nndl"), // Box Drawings Down Double And Left Single
    ('\u{2557}', "nndd"), // Box Drawings Double Down And Left
    ('\u{2558}', "ldnn"), // Box Drawings Up Single And Right Double
    ('\u{2559}', "dlnn"), // Box Drawings Up Double And Right Single
    ('\u{255A}', "ddnn"), // Box Drawings Double Up And Right
    ('\u{255B}', "lnnd"), // Box Drawings Up Single And Left Double
    ('\u{255C}', "dnnl"), // Box Drawings Up Double And Left Single
    ('\u{255D}', "dnnd"), // Box Drawings Double Up And Left
    ('\u{255E}', "ldln"), // Box Drawings Vertical Single And Right Double
    ('\u{255F}', "dldn"), // Box Drawings Vertical Double And Right Single
    ('\u{2560}', "dddn"), // Box Drawings Double Vertical And Right
    ('\u{2561}', "lnld"), // Box Drawings Vertical Single And Left Double
    ('\u{2562}', "dndl"), // Box Drawings Vertical Double And Left Single
    ('\u{2563}', "dndd"), // Box Drawings Double Vertical And Left
    ('\u{2564}', "ndld"), // Box Drawings Down Single And Horizontal Double
    ('\u{2565}', "nldl"), // Box Drawings Down Double And Horizontal Single
    ('\u{2566}', "nddd"), // Box Drawings Double Down And Horizontal
    ('\u{2567}', "ldnd"), // Box Drawings Up Single And Horizontal Double
    ('\u{2568}', "dlnl"), // Box Drawings Up Double And Horizontal Single
    ('\u{2569}', "ddnd"), // Box Drawings Double Up And Horizontal
    ('\u{256A}', "ldld"), // Box Drawings Vertical Single And Horizontal Double
    ('\u{256B}', "dldl"), // Box Drawings Vertical Double And Horizontal Single
    ('\u{256C}', "dddd"), // Box Drawings Double Vertical And Horizontal
    ('\u{2574}', "nnnl"), // Box Drawings Light Left
    ('\u{2575}', "lnnn"), // Box Drawings Light Up
    ('\u{2576}', "nlnn"), // Box Drawings Light Right
    ('\u{2577}', "nnln"), // Box Drawings Light Down
    ('\u{2578}', "nnnh"), // Box Drawings Heavy Left
    ('\u{2579}', "hnnn"), // Box Drawings Heavy Up
    ('\u{257A}', "nhnn"), // Box Drawings Heavy Right
    ('\u{257B}', "nnhn"), // Box Drawings Heavy Down
    ('\u{257C}', "nhnl"), // Box Drawings Light Left And Heavy Right
    ('\u{257D}', "lnhn"), // Box Drawings Light Up And Heavy Down
    ('\u{257E}', "nlnh"), // Box Drawings Heavy Left And Light Right
    ('\u{257F}', "hnln"), // Box Drawings Heavy Up And Light Down
];

/// The mask of the painted character `ch`; empty for any other.
pub fn paint(ch: char) -> Mask {
    let mut mask = Mask::empty();
    let code = u32::from(ch);
    match code {
        0x2500..=0x257F => box_drawing(&mut mask, ch),
        0x2580..=0x259F => block_element(&mut mask, code),
        0x1FB00..=0x1FB3B => sextant(&mut mask, code),
        _ => {}
    }
    mask
}

// =====================================================================
// Block elements (`block.zig`)
// =====================================================================

/// The edge a partial block grows from.
#[derive(Clone, Copy)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// A block of `width` by `height` of the cell, as fractions, against
/// `edge`: `round(size * fraction)` pixels.
fn block(mask: &mut Mask, edge: Edge, width: f64, height: f64, alpha: u8) {
    let w = (f64::from(CELL_WIDTH) * width).round() as i32;
    let h = (f64::from(CELL_HEIGHT) * height).round() as i32;
    let (cell_w, cell_h) = (CELL_WIDTH as i32, CELL_HEIGHT as i32);
    let (x, y) = match edge {
        Edge::Top | Edge::Left => (0, 0),
        Edge::Bottom => (0, cell_h - h),
        Edge::Right => (cell_w - w, 0),
    };
    mask.fill(x, y, x + w, y + h, alpha);
}

fn block_element(mask: &mut Mask, code: u32) {
    let eighths = |n: u32| f64::from(n) / 8.0;
    match code {
        0x2580 => block(mask, Edge::Top, 1.0, 0.5, 255),
        // ▁ to ▇: one to seven eighths from the bottom.
        0x2581..=0x2587 => block(mask, Edge::Bottom, 1.0, eighths(code - 0x2580), 255),
        0x2588 => block(mask, Edge::Top, 1.0, 1.0, 255),
        // ▉ to ▏: seven eighths to one eighth from the left.
        0x2589..=0x258F => block(mask, Edge::Left, eighths(0x2590 - code), 1.0, 255),
        0x2590 => block(mask, Edge::Right, 0.5, 1.0, 255),
        // The shades cover the whole cell at a quarter, a half and three
        // quarters (`common.zig`, `Shade`).
        0x2591 => block(mask, Edge::Top, 1.0, 1.0, 0x40),
        0x2592 => block(mask, Edge::Top, 1.0, 1.0, 0x80),
        0x2593 => block(mask, Edge::Top, 1.0, 1.0, 0xC0),
        0x2594 => block(mask, Edge::Top, 1.0, eighths(1), 255),
        0x2595 => block(mask, Edge::Right, eighths(1), 1.0, 255),
        _ => {
            // The quadrants ▖ to ▟, as upper left, upper right, lower
            // left, lower right.
            let [upper_left, upper_right, lower_left, lower_right] = match code {
                0x2596 => [false, false, true, false],
                0x2597 => [false, false, false, true],
                0x2598 => [true, false, false, false],
                0x2599 => [true, false, true, true],
                0x259A => [true, false, false, true],
                0x259B => [true, true, true, false],
                0x259C => [true, true, false, true],
                0x259D => [false, true, false, false],
                0x259E => [false, true, true, false],
                _ => [false, true, true, true],
            };
            for (on, x0, x1, y0, y1) in [
                (upper_left, 0.0, 0.5, 0.0, 0.5),
                (upper_right, 0.5, 1.0, 0.0, 0.5),
                (lower_left, 0.0, 0.5, 0.5, 1.0),
                (lower_right, 0.5, 1.0, 0.5, 1.0),
            ] {
                if on {
                    fill_fractions(mask, x0, x1, y0, y1);
                }
            }
        }
    }
}

// =====================================================================
// Sextants (`symbols_for_legacy_computing.zig`, `draw1FB00_1FB3B`)
// =====================================================================

/// The sextants number their six cells as bits, top left first, row by
/// row; the code points leave out the patterns of the empty cell, the
/// left and right halves and the full cell, which have characters of their
/// own.
fn sextant(mask: &mut Mask, code: u32) {
    let index = code - 0x1FB00;
    let bits = index + index / 0x14 + 1;
    let third = 1.0 / 3.0;
    for (bit, x0, x1, y0, y1) in [
        (0, 0.0, 0.5, 0.0, third),
        (1, 0.5, 1.0, 0.0, third),
        (2, 0.0, 0.5, third, 2.0 * third),
        (3, 0.5, 1.0, third, 2.0 * third),
        (4, 0.0, 0.5, 2.0 * third, 1.0),
        (5, 0.5, 1.0, 2.0 * third, 1.0),
    ] {
        if bits & (1 << bit) != 0 {
            fill_fractions(mask, x0, x1, y0, y1);
        }
    }
}

// =====================================================================
// Box drawing (`box.zig`, `draw2500_257F`)
// =====================================================================

const LIGHT: u32 = BOX_THICKNESS;
const HEAVY: u32 = 2 * BOX_THICKNESS;

fn box_drawing(mask: &mut Mask, ch: char) {
    if let Some((_, arms)) = LINES.iter().find(|(line, _)| *line == ch) {
        let arms = arms.as_bytes();
        lines(mask, [arms[0], arms[1], arms[2], arms[3]]);
        return;
    }
    let gap = LIGHT.max(4);
    match u32::from(ch) {
        0x2504 => dashes(mask, Direction::Across, 3, LIGHT, gap),
        0x2505 => dashes(mask, Direction::Across, 3, HEAVY, gap),
        0x2506 => dashes(mask, Direction::Down, 3, LIGHT, gap),
        0x2507 => dashes(mask, Direction::Down, 3, HEAVY, gap),
        0x2508 => dashes(mask, Direction::Across, 4, LIGHT, gap),
        0x2509 => dashes(mask, Direction::Across, 4, HEAVY, gap),
        0x250A => dashes(mask, Direction::Down, 4, LIGHT, gap),
        0x250B => dashes(mask, Direction::Down, 4, HEAVY, gap),
        0x254C => dashes(mask, Direction::Across, 2, LIGHT, LIGHT),
        0x254D => dashes(mask, Direction::Across, 2, HEAVY, HEAVY),
        0x254E => dashes(mask, Direction::Down, 2, LIGHT, HEAVY),
        0x254F => dashes(mask, Direction::Down, 2, HEAVY, HEAVY),
        0x256D => arc(mask, Corner::BottomRight),
        0x256E => arc(mask, Corner::BottomLeft),
        0x256F => arc(mask, Corner::TopLeft),
        0x2570 => arc(mask, Corner::TopRight),
        0x2571 => diagonal(mask, true),
        0x2572 => diagonal(mask, false),
        _ => {
            diagonal(mask, true);
            diagonal(mask, false);
        }
    }
}

/// A character of lines from the centre to its edges, each arm `n` none,
/// `l` light, `h` heavy or `d` double, in the order up, right, down, left
/// (`box.zig`, `linesChar`). Where arms of different styles meet, each
/// stops where the strokes across it end, so the joint has no notch.
fn lines(mask: &mut Mask, [up, right, down, left]: [u8; 4]) {
    let (w, h) = (CELL_WIDTH, CELL_HEIGHT);
    let h_light_top = h.saturating_sub(LIGHT) / 2;
    let h_light_bottom = h_light_top + LIGHT;
    let h_heavy_top = h.saturating_sub(HEAVY) / 2;
    let h_heavy_bottom = h_heavy_top + HEAVY;
    let h_double_top = h_light_top.saturating_sub(LIGHT);
    let h_double_bottom = h_light_bottom + LIGHT;
    let v_light_left = w.saturating_sub(LIGHT) / 2;
    let v_light_right = v_light_left + LIGHT;
    let v_heavy_left = w.saturating_sub(HEAVY) / 2;
    let v_heavy_right = v_heavy_left + HEAVY;
    let v_double_left = v_light_left.saturating_sub(LIGHT);
    let v_double_right = v_light_right + LIGHT;
    let (n, heavy, double) = (b'n', b'h', b'd');

    let up_bottom = if left == heavy || right == heavy {
        h_heavy_bottom
    } else if left != right || down == up {
        if left == double || right == double {
            h_double_bottom
        } else {
            h_light_bottom
        }
    } else if left == n && right == n {
        h_light_bottom
    } else {
        h_light_top
    };
    let down_top = if left == heavy || right == heavy {
        h_heavy_top
    } else if left != right || up == down {
        if left == double || right == double {
            h_double_top
        } else {
            h_light_top
        }
    } else if left == n && right == n {
        h_light_top
    } else {
        h_light_bottom
    };
    let left_right = if up == heavy || down == heavy {
        v_heavy_right
    } else if up != down || left == right {
        if up == double || down == double {
            v_double_right
        } else {
            v_light_right
        }
    } else if up == n && down == n {
        v_light_right
    } else {
        v_light_left
    };
    let right_left = if up == heavy || down == heavy {
        v_heavy_left
    } else if up != down || right == left {
        if up == double || down == double {
            v_double_left
        } else {
            v_light_left
        }
    } else if up == n && down == n {
        v_light_left
    } else {
        v_light_right
    };

    let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32| {
        mask.fill(x0 as i32, y0 as i32, x1 as i32, y1 as i32, 255);
    };
    match up {
        b'l' => fill(v_light_left, 0, v_light_right, up_bottom),
        b'h' => fill(v_heavy_left, 0, v_heavy_right, up_bottom),
        b'd' => {
            let left_bottom = if left == double {
                h_light_top
            } else {
                up_bottom
            };
            let right_bottom = if right == double {
                h_light_top
            } else {
                up_bottom
            };
            fill(v_double_left, 0, v_light_left, left_bottom);
            fill(v_light_right, 0, v_double_right, right_bottom);
        }
        _ => {}
    }
    match right {
        b'l' => fill(right_left, h_light_top, w, h_light_bottom),
        b'h' => fill(right_left, h_heavy_top, w, h_heavy_bottom),
        b'd' => {
            let top_left = if up == double {
                v_light_right
            } else {
                right_left
            };
            let bottom_left = if down == double {
                v_light_right
            } else {
                right_left
            };
            fill(top_left, h_double_top, w, h_light_top);
            fill(bottom_left, h_light_bottom, w, h_double_bottom);
        }
        _ => {}
    }
    match down {
        b'l' => fill(v_light_left, down_top, v_light_right, h),
        b'h' => fill(v_heavy_left, down_top, v_heavy_right, h),
        b'd' => {
            let left_top = if left == double {
                h_light_bottom
            } else {
                down_top
            };
            let right_top = if right == double {
                h_light_bottom
            } else {
                down_top
            };
            fill(v_double_left, left_top, v_light_left, h);
            fill(v_light_right, right_top, v_double_right, h);
        }
        _ => {}
    }
    match left {
        b'l' => fill(0, h_light_top, left_right, h_light_bottom),
        b'h' => fill(0, h_heavy_top, left_right, h_heavy_bottom),
        b'd' => {
            let top_right = if up == double {
                v_light_left
            } else {
                left_right
            };
            let bottom_right = if down == double {
                v_light_left
            } else {
                left_right
            };
            fill(0, h_double_top, top_right, h_light_top);
            fill(0, h_light_bottom, bottom_right, h_double_bottom);
        }
        _ => {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Across,
    Down,
}

/// `count` dashes `thickness` pixels thick across or down the middle of
/// the cell, with gaps of about `gap` pixels (`box.zig`, `dashHorizontal`
/// and `dashVertical`). A dashed line across has half a gap at either end,
/// one down a whole gap at the bottom, so that dashes tile evenly from cell
/// to cell. Pixels left over widen the first dashes.
fn dashes(mask: &mut Mask, direction: Direction, count: u32, thickness: u32, gap: u32) {
    let (length, breadth) = match direction {
        Direction::Across => (CELL_WIDTH, CELL_HEIGHT),
        Direction::Down => (CELL_HEIGHT, CELL_WIDTH),
    };
    let gap = gap.min(length / (2 * count));
    let dash_total = length - count * gap;
    let (dash, mut extra) = (dash_total / count, dash_total % count);
    let across = breadth.saturating_sub(thickness) / 2;
    let mut start = match direction {
        Direction::Across => gap / 2,
        Direction::Down => 0,
    };
    for _ in 0..count {
        let mut end = start + dash;
        if extra > 0 {
            extra -= 1;
            end += 1;
        }
        let (x0, y0, x1, y1) = match direction {
            Direction::Across => (start, across, end, across + thickness),
            Direction::Down => (across, start, across + thickness, end),
        };
        mask.fill(x0 as i32, y0 as i32, x1 as i32, y1 as i32, 255);
        start = end + gap;
    }
}

/// The corner of the cell an arc's curve bends around.
#[derive(Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// A light arc from the middle of one edge to the middle of another: a
/// straight run from the edge, a curve of radius half the cell's smaller
/// side with its control points a quarter of the radius from the centre,
/// and a straight run to the other edge (`box.zig`, `arc`).
fn arc(mask: &mut Mask, corner: Corner) {
    let (w, h) = (f64::from(CELL_WIDTH), f64::from(CELL_HEIGHT));
    let thick = f64::from(LIGHT);
    let centre_x = f64::from(CELL_WIDTH.saturating_sub(LIGHT) / 2) + thick / 2.0;
    let centre_y = f64::from(CELL_HEIGHT.saturating_sub(LIGHT) / 2) + thick / 2.0;
    let radius = w.min(h) / 2.0;
    let control = 0.25;
    // Which way the curve runs from the centre: up or down, left or right.
    let (vertical, horizontal) = match corner {
        Corner::TopLeft => (-1.0, -1.0),
        Corner::TopRight => (-1.0, 1.0),
        Corner::BottomLeft => (1.0, -1.0),
        Corner::BottomRight => (1.0, 1.0),
    };
    let edge_y = if vertical < 0.0 { 0.0 } else { h };
    let edge_x = if horizontal < 0.0 { 0.0 } else { w };
    let start = (centre_x, centre_y + vertical * radius);
    let first = (centre_x, centre_y + vertical * control * radius);
    let second = (centre_x + horizontal * control * radius, centre_y);
    let end = (centre_x + horizontal * radius, centre_y);

    let mut points = vec![(centre_x, edge_y), start];
    const STEPS: u32 = 64;
    for step in 1..=STEPS {
        let t = f64::from(step) / f64::from(STEPS);
        let u = 1.0 - t;
        let along = |a: f64, b: f64, c: f64, d: f64| {
            u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
        };
        points.push((
            along(start.0, first.0, second.0, end.0),
            along(start.1, first.1, second.1, end.1),
        ));
    }
    points.push((edge_x, centre_y));
    mask.stroke(&points, thick);
}

/// A light diagonal from corner to corner, reaching a little past both so
/// that diagonals meet across the corners of cells (`box.zig`,
/// `lightDiagonalUpperRightToLowerLeft` and its mirror).
fn diagonal(mask: &mut Mask, rising: bool) {
    let (w, h) = (f64::from(CELL_WIDTH), f64::from(CELL_HEIGHT));
    let slope_x = (w / h).min(1.0);
    let slope_y = (h / w).min(1.0);
    let points = if rising {
        [
            (w + 0.5 * slope_x, -0.5 * slope_y),
            (-0.5 * slope_x, h + 0.5 * slope_y),
        ]
    } else {
        [
            (-0.5 * slope_x, -0.5 * slope_y),
            (w + 0.5 * slope_x, h + 0.5 * slope_y),
        ]
    };
    mask.stroke(&points, f64::from(LIGHT));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::is_painted;

    const W: u32 = CELL_WIDTH;
    const H: u32 = CELL_HEIGHT;

    /// A rectangle of pixels, `x0..x1` by `y0..y1`.
    type Rect = (u32, u32, u32, u32);

    /// The pixels where `mask` differs from full cover inside `rects` and
    /// none outside them, as a count.
    fn mismatches(mask: &Mask, rects: &[Rect]) -> usize {
        let inside = |x: u32, y: u32| {
            rects
                .iter()
                .any(|&(x0, x1, y0, y1)| (x0..x1).contains(&x) && (y0..y1).contains(&y))
        };
        (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .filter(|&(x, y)| (mask.at(x, y) == 255) != inside(x, y))
            .count()
    }

    fn assert_rows(failures: Vec<String>) {
        assert!(
            failures.is_empty(),
            "failing rows:\n{}",
            failures.join("\n")
        );
    }

    fn check_rects(rows: &[(char, &[Rect])]) {
        let failures = rows
            .iter()
            .filter_map(|(ch, rects)| {
                let wrong = mismatches(&paint(*ch), rects);
                (wrong > 0).then(|| format!("U+{:04X} {ch}: {wrong} pixels wrong", u32::from(*ch)))
            })
            .collect();
        assert_rows(failures);
    }

    // =================================================================
    // Block elements
    // =================================================================
    //
    // A cell is 15 by 39 pixels. A block of a fraction f of the cell is
    // round(size * f) pixels, from the edge the character names.

    #[test]
    fn lower_eighths_and_the_upper_half_fill_their_rows() {
        check_rects(&[
            ('▀', &[(0, W, 0, 20)]),
            ('▁', &[(0, W, 34, H)]),
            ('▂', &[(0, W, 29, H)]),
            ('▃', &[(0, W, 24, H)]),
            ('▄', &[(0, W, 19, H)]),
            ('▅', &[(0, W, 15, H)]),
            ('▆', &[(0, W, 10, H)]),
            ('▇', &[(0, W, 5, H)]),
            ('█', &[(0, W, 0, H)]),
        ]);
    }

    #[test]
    fn left_eighths_and_the_right_half_fill_their_columns() {
        check_rects(&[
            ('▉', &[(0, 13, 0, H)]),
            ('▊', &[(0, 11, 0, H)]),
            ('▋', &[(0, 9, 0, H)]),
            ('▌', &[(0, 8, 0, H)]),
            ('▍', &[(0, 6, 0, H)]),
            ('▎', &[(0, 4, 0, H)]),
            ('▏', &[(0, 2, 0, H)]),
            ('▐', &[(7, W, 0, H)]),
        ]);
    }

    #[test]
    fn the_upper_and_the_right_eighth_fill_their_edge() {
        check_rects(&[('▔', &[(0, W, 0, 5)]), ('▕', &[(13, W, 0, H)])]);
    }

    #[test]
    fn shades_cover_the_cell_at_a_quarter_a_half_and_three_quarters() {
        let failures = [('░', 0x40), ('▒', 0x80), ('▓', 0xC0)]
            .into_iter()
            .filter_map(|(ch, alpha)| {
                let mask = paint(ch);
                let wrong = (0..H)
                    .flat_map(|y| (0..W).map(move |x| (x, y)))
                    .filter(|&(x, y)| mask.at(x, y) != alpha)
                    .count();
                (wrong > 0).then(|| format!("{ch}: {wrong} pixels not at {alpha:#x}"))
            })
            .collect();
        assert_rows(failures);
    }

    /// The quadrants by name. On an odd width the halves overlap by a
    /// pixel: the left half ends at round(7.5) = 8, the right half starts
    /// at 15 - 8 = 7, so they meet without a gap.
    #[test]
    fn quadrants_fill_the_quarters_they_name() {
        let upper_left = (0, 8, 0, 20);
        let upper_right = (7, W, 0, 20);
        let lower_left = (0, 8, 19, H);
        let lower_right = (7, W, 19, H);
        check_rects(&[
            ('▖', &[lower_left]),
            ('▗', &[lower_right]),
            ('▘', &[upper_left]),
            ('▙', &[upper_left, lower_left, lower_right]),
            ('▚', &[upper_left, lower_right]),
            ('▛', &[upper_left, upper_right, lower_left]),
            ('▜', &[upper_left, upper_right, lower_right]),
            ('▝', &[upper_right]),
            ('▞', &[upper_right, lower_left]),
            ('▟', &[upper_right, lower_left, lower_right]),
        ]);
    }

    /// The sextants by the cells their names list: 1 and 2 in the top
    /// row, 3 and 4 in the middle, 5 and 6 at the bottom, odd ones left.
    /// The rows are thirds of 39 pixels, 0..13, 13..26 and 26..39.
    #[test]
    fn sextants_fill_the_cells_their_names_list() {
        let names = [
            ('\u{1FB00}', "1"),
            ('\u{1FB01}', "2"),
            ('\u{1FB02}', "12"),
            ('\u{1FB03}', "3"),
            ('\u{1FB04}', "13"),
            ('\u{1FB05}', "23"),
            ('\u{1FB06}', "123"),
            ('\u{1FB07}', "4"),
            ('\u{1FB08}', "14"),
            ('\u{1FB09}', "24"),
            ('\u{1FB0A}', "124"),
            ('\u{1FB0B}', "34"),
            ('\u{1FB0C}', "134"),
            ('\u{1FB0D}', "234"),
            ('\u{1FB0E}', "1234"),
            ('\u{1FB0F}', "5"),
            ('\u{1FB10}', "15"),
            ('\u{1FB11}', "25"),
            ('\u{1FB12}', "125"),
            ('\u{1FB13}', "35"),
            ('\u{1FB14}', "235"),
            ('\u{1FB15}', "1235"),
            ('\u{1FB16}', "45"),
            ('\u{1FB17}', "145"),
            ('\u{1FB18}', "245"),
            ('\u{1FB19}', "1245"),
            ('\u{1FB1A}', "345"),
            ('\u{1FB1B}', "1345"),
            ('\u{1FB1C}', "2345"),
            ('\u{1FB1D}', "12345"),
            ('\u{1FB1E}', "6"),
            ('\u{1FB1F}', "16"),
            ('\u{1FB20}', "26"),
            ('\u{1FB21}', "126"),
            ('\u{1FB22}', "36"),
            ('\u{1FB23}', "136"),
            ('\u{1FB24}', "236"),
            ('\u{1FB25}', "1236"),
            ('\u{1FB26}', "46"),
            ('\u{1FB27}', "146"),
            ('\u{1FB28}', "1246"),
            ('\u{1FB29}', "346"),
            ('\u{1FB2A}', "1346"),
            ('\u{1FB2B}', "2346"),
            ('\u{1FB2C}', "12346"),
            ('\u{1FB2D}', "56"),
            ('\u{1FB2E}', "156"),
            ('\u{1FB2F}', "256"),
            ('\u{1FB30}', "1256"),
            ('\u{1FB31}', "356"),
            ('\u{1FB32}', "1356"),
            ('\u{1FB33}', "2356"),
            ('\u{1FB34}', "12356"),
            ('\u{1FB35}', "456"),
            ('\u{1FB36}', "1456"),
            ('\u{1FB37}', "2456"),
            ('\u{1FB38}', "12456"),
            ('\u{1FB39}', "3456"),
            ('\u{1FB3A}', "13456"),
            ('\u{1FB3B}', "23456"),
        ];
        let cell = |n: char| -> Rect {
            let n = n.to_digit(10).expect("a digit") - 1;
            let (x0, x1) = if n.is_multiple_of(2) { (0, 8) } else { (7, W) };
            let (y0, y1) = [(0, 13), (13, 26), (26, H)][(n / 2) as usize];
            (x0, x1, y0, y1)
        };
        let rows: Vec<(char, Vec<Rect>)> = names
            .iter()
            .map(|&(ch, cells)| (ch, cells.chars().map(cell).collect()))
            .collect();
        let rows: Vec<(char, &[Rect])> = rows.iter().map(|(ch, rects)| (*ch, &rects[..])).collect();
        check_rects(&rows);
    }

    // =================================================================
    // Box drawing
    // =================================================================
    //
    // A light line is 2 pixels wide, a heavy one 4, a double one two light
    // lines with a light line's width between them, each centred at
    // (size - width) / 2. Far from the centre, each arm of a character
    // shows its style alone.

    /// The pixels lit across an arm, far from the centre: the columns at
    /// row 2 (up) or 36 (down), the rows at column 1 (left) or 13 (right).
    fn across(mask: &Mask, arm: usize) -> Vec<u32> {
        match arm {
            0 => (0..W).filter(|&x| mask.at(x, 2) == 255).collect(),
            1 => (0..H).filter(|&y| mask.at(13, y) == 255).collect(),
            2 => (0..W).filter(|&x| mask.at(x, 36) == 255).collect(),
            _ => (0..H).filter(|&y| mask.at(1, y) == 255).collect(),
        }
    }

    /// The pixels an arm of `style` lights across a vertical (`vertical`)
    /// or a horizontal arm.
    fn expected_across(style: u8, vertical: bool) -> Vec<u32> {
        // A vertical arm lies across the cell's width, a horizontal one
        // across its height.
        let light: Vec<u32> = if vertical { vec![6, 7] } else { vec![18, 19] };
        let heavy: Vec<u32> = if vertical {
            vec![5, 6, 7, 8]
        } else {
            vec![17, 18, 19, 20]
        };
        let double: Vec<u32> = if vertical {
            vec![4, 5, 8, 9]
        } else {
            vec![16, 17, 20, 21]
        };
        match style {
            b'n' => vec![],
            b'l' => light,
            b'h' => heavy,
            _ => double,
        }
    }

    /// Checks the arms of every line character `select` takes.
    fn check_lines(select: impl Fn(&str) -> bool) {
        let rows: Vec<_> = LINES.iter().filter(|(_, arms)| select(arms)).collect();
        assert!(!rows.is_empty());
        let failures = rows
            .into_iter()
            .flat_map(|&(ch, arms)| {
                let mask = paint(ch);
                arms.bytes()
                    .enumerate()
                    .filter_map(move |(arm, style)| {
                        let lit = across(&mask, arm);
                        let expected = expected_across(style, arm % 2 == 0);
                        (lit != expected).then(|| {
                            format!(
                                "U+{:04X} {ch} arm {arm}: {lit:?}, not {expected:?}",
                                u32::from(ch)
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_rows(failures);
    }

    fn arm_count(arms: &str) -> usize {
        arms.bytes().filter(|&style| style != b'n').count()
    }

    /// One arm, or two that make a straight line.
    #[test]
    fn straight_lines_and_half_lines_light_their_arms() {
        check_lines(|arms| {
            let lit: Vec<bool> = arms.bytes().map(|style| style != b'n').collect();
            arm_count(arms) == 1 || (arm_count(arms) == 2 && lit[0] == lit[2])
        });
    }

    #[test]
    fn corners_light_their_two_arms() {
        check_lines(|arms| {
            let lit: Vec<bool> = arms.bytes().map(|style| style != b'n').collect();
            arm_count(arms) == 2 && lit[0] != lit[2]
        });
    }

    #[test]
    fn tees_light_their_three_arms() {
        check_lines(|arms| arm_count(arms) == 3);
    }

    #[test]
    fn crosses_light_their_four_arms() {
        check_lines(|arms| arm_count(arms) == 4);
    }

    /// The runs of lit pixels along the middle row (horizontal) or the
    /// middle column.
    fn runs(mask: &Mask, horizontal: bool) -> usize {
        let lit: Vec<bool> = if horizontal {
            (0..W).map(|x| mask.at(x, 19) == 255).collect()
        } else {
            (0..H).map(|y| mask.at(7, y) == 255).collect()
        };
        lit.windows(2).filter(|pair| !pair[0] && pair[1]).count() + usize::from(lit[0])
    }

    #[test]
    fn dashed_lines_have_their_number_of_dashes() {
        let rows = [
            ('┄', true, 3),
            ('┅', true, 3),
            ('┆', false, 3),
            ('┇', false, 3),
            ('┈', true, 4),
            ('┉', true, 4),
            ('┊', false, 4),
            ('┋', false, 4),
            ('╌', true, 2),
            ('╍', true, 2),
            ('╎', false, 2),
            ('╏', false, 2),
        ];
        let failures = rows
            .into_iter()
            .filter_map(|(ch, horizontal, dashes)| {
                let found = runs(&paint(ch), horizontal);
                (found != dashes).then(|| format!("{ch}: {found} dashes, not {dashes}"))
            })
            .collect();
        assert_rows(failures);
    }

    /// An arc leaves its cell at the middle of the two edges it joins and
    /// keeps out of the quarter opposite its curve.
    #[test]
    fn arcs_join_the_middles_of_two_edges() {
        let bottom = (7, 38);
        let top = (7, 0);
        let left = (0, 19);
        let right = (14, 19);
        let rows = [
            ('╭', [bottom, right], (0, 0)),
            ('╮', [bottom, left], (14, 0)),
            ('╯', [top, left], (14, 38)),
            ('╰', [top, right], (0, 38)),
        ];
        let failures = rows
            .into_iter()
            .flat_map(|(ch, ends, empty)| {
                let mask = paint(ch);
                let mut failures: Vec<String> = ends
                    .iter()
                    .filter(|&&(x, y)| mask.at(x, y) == 0)
                    .map(|(x, y)| format!("{ch}: nothing at ({x}, {y})"))
                    .collect();
                if mask.at(empty.0, empty.1) != 0 {
                    failures.push(format!("{ch}: ink in the far corner {empty:?}"));
                }
                failures
            })
            .collect();
        assert_rows(failures);
    }

    #[test]
    fn diagonals_run_between_their_corners() {
        let corners =
            |mask: &Mask| [(1, 1), (13, 1), (1, 37), (13, 37)].map(|(x, y)| mask.at(x, y) > 0);
        let rows = [
            ('╱', [false, true, true, false]),
            ('╲', [true, false, false, true]),
            ('╳', [true, true, true, true]),
        ];
        let failures = rows
            .into_iter()
            .filter_map(|(ch, expected)| {
                let found = corners(&paint(ch));
                (found != expected).then(|| format!("{ch}: corners {found:?}, not {expected:?}"))
            })
            .collect();
        assert_rows(failures);
    }

    /// Every painted code point puts ink into its cell; the mask is the
    /// cell, so nothing reaches past it.
    #[test]
    fn every_painted_code_point_paints_inside_its_cell() {
        let painted: Vec<char> = (0x2500..=0x259F)
            .chain(0x1FB00..=0x1FB3B)
            .filter_map(char::from_u32)
            .collect();
        assert_eq!(painted.len(), 220);
        let failures = painted
            .into_iter()
            .filter(|&ch| {
                let mask = paint(ch);
                !is_painted(ch)
                    || mask.alpha.len() != (W * H) as usize
                    || mask.alpha.iter().all(|&alpha| alpha == 0)
            })
            .map(|ch| format!("U+{:04X} {ch}", u32::from(ch)))
            .collect();
        assert_rows(failures);
    }

    /// Guard: a character outside the three ranges paints nothing.
    #[test]
    fn a_character_outside_the_painted_ranges_paints_nothing() {
        assert!(paint('a').alpha.iter().all(|&alpha| alpha == 0));
    }

    /// Guard: a path that stays on a point measures to that point.
    #[test]
    fn a_segment_of_no_length_measures_to_its_point() {
        assert_eq!(distance_to_segment((3.0, 4.0), (0.0, 0.0), (0.0, 0.0)), 5.0);
    }
}
