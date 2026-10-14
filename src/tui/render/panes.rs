// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! How a question tab shares its area between the question and its context
//! (spec section 7.1, ADR 30). The question never gets wider than a reading
//! width. When the question or one of its options has context, the context
//! area goes beside the question on a landscape area and below it on a
//! portrait one, or shrinks to a one-line hint when neither fits.
//!
//! The decision and the geometry are plain functions of sizes, so every
//! terminal size can be tested without drawing.

use ratatui::layout::Rect;

/// The widest the question gets, in columns. Wider lines are hard to read,
/// and on a terminal of 230 columns question texts and option descriptions
/// ran across all of it (user, 2026-10-14). The plan names 100 to 110;
/// 100 is where trying it starts.
pub const READING_WIDTH: u16 = 100;

/// The narrowest question beside a context area: the smallest terminal
/// asqr supports is 60 columns wide, so the question never gets narrower
/// than it is there.
pub const MIN_QUESTION_WIDTH: u16 = 60;

/// The narrowest context area beside the question.
pub const MIN_CONTEXT_WIDTH: u16 = 40;

/// The fewest rows of a context area below the question.
pub const MIN_CONTEXT_ROWS: u16 = 8;

/// The blank column between the question and a context area beside it.
const SEPARATOR: u16 = 1;

/// Where the context goes, as asked for with `--layout` or `ASQR_LAYOUT`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum PaneLayout {
    /// Beside on a landscape area, below on a portrait one.
    #[default]
    Auto,
    /// Beside whenever it fits.
    Beside,
    /// Below whenever it fits.
    Below,
}

/// Where the context of the current question is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    /// Right of the question, after a blank column.
    Beside,
    /// Under the question, over the whole width.
    Below,
    /// Neither fits: one line under the question names the context.
    Hint,
}

/// Picks the arrangement for a tab area of `width`×`height` cells.
///
/// When both arrangements fit, `layout` decides; `auto` goes by the shape
/// the person sees. Cells are about twice as tall as wide, so the area
/// looks landscape when `width ÷ (2 × height)` is at least 1. `previous`,
/// the arrangement shown before, adds hysteresis: beside stays until the
/// shape drops under 0.9, below stays until it reaches 1.1, so a resize
/// around 1 does not flip the screen back and forth. When only one fits,
/// that one is taken whatever `layout` asks for.
pub fn choose(
    width: u16,
    height: u16,
    layout: PaneLayout,
    previous: Option<Arrangement>,
) -> Arrangement {
    let beside_fits = width >= MIN_QUESTION_WIDTH + SEPARATOR + MIN_CONTEXT_WIDTH;
    // Measured with the question at its largest, so a question that grows
    // while the person types never flips the arrangement.
    let below_fits = height - height / 2 >= MIN_CONTEXT_ROWS;
    match (beside_fits, below_fits) {
        (true, true) => {}
        (true, false) => return Arrangement::Beside,
        (false, true) => return Arrangement::Below,
        (false, false) => return Arrangement::Hint,
    }
    match layout {
        PaneLayout::Beside => return Arrangement::Beside,
        PaneLayout::Below => return Arrangement::Below,
        PaneLayout::Auto => {}
    }

    // The shape `width ÷ (2 × height)` against tenths, in integers.
    let tenfold_width = u32::from(width) * 10;
    let doubled_height = u32::from(height) * 2;
    let landscape = match previous {
        Some(Arrangement::Beside) => tenfold_width >= doubled_height * 9,
        Some(Arrangement::Below) => tenfold_width >= doubled_height * 11,
        Some(Arrangement::Hint) | None => tenfold_width >= doubled_height * 10,
    };
    if landscape {
        Arrangement::Beside
    } else {
        Arrangement::Below
    }
}

/// The question's area and the context's in `area`, for `arrangement`,
/// or for a question without context when it is `None`. `question_rows`
/// is how many rows the question asks for above a context area below it:
/// the most any question with context in the session takes at the width it
/// gets there, `min(area.width, READING_WIDTH)`, so moving between
/// questions keeps the split. Only the below arrangement uses it.
pub fn areas(
    area: Rect,
    arrangement: Option<Arrangement>,
    question_rows: usize,
) -> (Rect, Option<Rect>) {
    let capped = Rect {
        width: area.width.min(READING_WIDTH),
        ..area
    };
    match arrangement {
        None => (capped, None),
        Some(Arrangement::Beside) => {
            // Half each, the odd column to the context, but never less
            // than the narrowest question nor more than the reading width.
            let question_width = ((area.width - SEPARATOR) / 2)
                .clamp(MIN_QUESTION_WIDTH, READING_WIDTH)
                .min(area.width);
            let context_x = area.x + question_width + SEPARATOR;
            (
                Rect {
                    width: question_width,
                    ..area
                },
                Some(Rect {
                    x: context_x,
                    width: area.right().saturating_sub(context_x),
                    ..area
                }),
            )
        }
        Some(Arrangement::Below) => {
            let rows = u16::try_from(question_rows).unwrap_or(u16::MAX);
            let question_height = rows.min(area.height / 2);
            (
                Rect {
                    height: question_height,
                    ..capped
                },
                Some(Rect {
                    y: area.y + question_height,
                    height: area.height - question_height,
                    ..area
                }),
            )
        }
        Some(Arrangement::Hint) => {
            let question_height = area.height.saturating_sub(1);
            (
                Rect {
                    height: question_height,
                    ..capped
                },
                Some(Rect {
                    y: area.y + question_height,
                    height: area.height - question_height,
                    ..area
                }),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Arrangement::{Below, Beside, Hint};

    // -----------------------------------------------------------------
    // Which arrangement
    // -----------------------------------------------------------------

    #[test]
    fn auto_goes_by_the_visible_shape_when_both_fit() {
        // width, height, expected; the shape is width ÷ (2 × height).
        let cases = [
            (118, 33, Beside),  // 120×40 with a one-line header: 1.79
            (228, 53, Beside),  // 230×60: 2.15
            (108, 63, Below),   // 110×70: 0.86
            (101, 51, Below),   // 0.99: under 1 when nothing was shown
            (102, 51, Beside),  // 1.0
            (101, 50, Beside),  // 1.01
            (120, 61, Below),   // 0.98
            (200, 100, Beside), // 1.0
            (199, 100, Below),  // 0.995
            (101, 15, Beside),  // the lowest area below fits in
            (300, 15, Beside),
            (101, 200, Below),
        ];
        for (width, height, expected) in cases {
            assert_eq!(
                choose(width, height, PaneLayout::Auto, None),
                expected,
                "{width}×{height}"
            );
        }
    }

    #[test]
    fn a_resize_around_square_keeps_the_arrangement_shown() {
        // width, height, previous, expected
        let cases = [
            // From beside, only a shape under 0.9 switches.
            (180, 100, Beside, Beside), // 0.9
            (179, 100, Beside, Below),  // 0.895
            (190, 100, Beside, Beside), // 0.95
            // From below, only a shape of 1.1 or more switches.
            (219, 100, Below, Below),  // 1.095
            (220, 100, Below, Beside), // 1.1
            (205, 100, Below, Below),  // 1.025
            (110, 50, Below, Beside),  // 1.1
            // A hint before counts as nothing shown yet.
            (205, 100, Hint, Beside),
            (190, 100, Hint, Below),
        ];
        for (width, height, previous, expected) in cases {
            assert_eq!(
                choose(width, height, PaneLayout::Auto, Some(previous)),
                expected,
                "{width}×{height} after {previous:?}"
            );
        }
    }

    #[test]
    fn a_redraw_at_the_same_size_keeps_what_auto_chose() {
        for (width, height) in [(118, 33), (108, 63), (190, 100), (205, 100), (101, 51)] {
            let first = choose(width, height, PaneLayout::Auto, None);
            let again = choose(width, height, PaneLayout::Auto, Some(first));
            assert_eq!(again, first, "{width}×{height}");
        }
    }

    #[test]
    fn beside_needs_101_columns_and_below_8_rows_for_the_context() {
        // Below fits when the context gets 8 rows with the question at
        // its largest, half the height: from 15 rows on.
        let cases = [
            (100, 40, PaneLayout::Auto, Below),
            (100, 40, PaneLayout::Beside, Below),
            (101, 14, PaneLayout::Auto, Beside),
            (101, 14, PaneLayout::Below, Beside),
            (58, 15, PaneLayout::Auto, Below),
            (58, 15, PaneLayout::Beside, Below),
            (58, 14, PaneLayout::Auto, Hint),
            (78, 9, PaneLayout::Below, Hint),
            (100, 14, PaneLayout::Beside, Hint),
            (300, 4, PaneLayout::Auto, Beside),
        ];
        for (width, height, layout, expected) in cases {
            assert_eq!(
                choose(width, height, layout, None),
                expected,
                "{width}×{height} with {layout:?}"
            );
        }
    }

    #[test]
    fn beside_or_below_overrides_the_shape_when_both_fit() {
        for (width, height) in [(118, 33), (108, 63), (228, 15), (101, 200)] {
            for previous in [None, Some(Beside), Some(Below), Some(Hint)] {
                assert_eq!(choose(width, height, PaneLayout::Beside, previous), Beside);
                assert_eq!(choose(width, height, PaneLayout::Below, previous), Below);
            }
        }
    }

    // -----------------------------------------------------------------
    // The areas
    // -----------------------------------------------------------------

    fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect::new(x, y, width, height)
    }

    #[test]
    fn a_question_without_context_takes_up_to_the_reading_width() {
        let cases = [(58, 58), (100, 100), (118, 100), (228, 100)];
        for (width, question) in cases {
            assert_eq!(
                areas(rect(1, 5, width, 20), None, 7),
                (rect(1, 5, question, 20), None),
                "{width} columns"
            );
        }
    }

    #[test]
    fn beside_splits_half_and_half_until_the_question_reaches_the_cap() {
        // width, question width, context width
        let cases = [
            (101, 60, 40),
            (110, 60, 49),
            (118, 60, 57),
            (121, 60, 60),
            (122, 60, 61),
            (123, 61, 61),
            (160, 79, 80),
            (198, 98, 99),
            (201, 100, 100),
            (228, 100, 127),
        ];
        for (width, question, context) in cases {
            let (question_area, context_area) = areas(rect(1, 5, width, 30), Some(Beside), 99);
            assert_eq!(question_area, rect(1, 5, question, 30), "{width} columns");
            assert_eq!(
                context_area,
                Some(rect(1 + question + 1, 5, context, 30)),
                "{width} columns"
            );
        }
    }

    #[test]
    fn below_gives_the_question_its_rows_up_to_half_the_height() {
        // width, height, question rows, question area height
        let cases = [
            (58, 15, 4, 4),
            (58, 15, 7, 7),
            (58, 15, 8, 7),
            (58, 15, 40, 7),
            (108, 63, 12, 12),
            (108, 63, 31, 31),
            (108, 63, 60, 31),
            (228, 100, 30, 30),
        ];
        for (width, height, rows, question_height) in cases {
            let (question, context) = areas(rect(1, 5, width, height), Some(Below), rows);
            assert_eq!(
                question,
                rect(1, 5, width.min(READING_WIDTH), question_height),
                "{width}×{height}, {rows} rows"
            );
            assert_eq!(
                context,
                Some(rect(
                    1,
                    5 + question_height,
                    width,
                    height - question_height
                )),
                "the context takes the full width and the rest"
            );
        }
    }

    #[test]
    fn a_hint_takes_the_last_line_of_the_area() {
        for (width, height, question) in [(58, 9, 58), (78, 9, 78), (100, 14, 100)] {
            assert_eq!(
                areas(rect(1, 5, width, height), Some(Hint), 30),
                (
                    rect(1, 5, question, height - 1),
                    Some(rect(1, 5 + height - 1, width, 1))
                ),
                "{width}×{height}"
            );
        }
    }
}
