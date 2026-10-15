// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The main area while no session waits (spec section 7.1, ADR 39): a
//! sentence that says so, with Talqi above it where there is room.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, StatefulWidget, Widget, Wrap};
use ratatui_image::FontSize;

use super::mascot::{Mascot, OUTLINE_COLUMNS, OUTLINE_ROWS, Outline, Which, mascot_rect};

/// What the empty queue says.
const SENTENCE: &str = "Nothing to answer. New sessions show up here as soon as they arrive.";

/// The rows kept for the sentence, which wraps on a narrow screen.
const SENTENCE_ROWS: u16 = 3;

/// The empty queue: Talqi, picture `which`, drawn in cells of `font`'s
/// size in pixels, above the sentence.
pub(super) struct EmptyQueue {
    pub(super) which: Which,
    pub(super) font: FontSize,
    /// Whether the screen is drawn without colours (`NO_COLOR`). Talqi is
    /// then the [`Outline`], the same for `which` awake and asleep, and no
    /// picture is made.
    pub(super) no_color: bool,
}

impl StatefulWidget for EmptyQueue {
    type State = Mascot;

    fn render(self, area: Rect, buf: &mut Buffer, mascot: &mut Mascot) {
        let sentence = if self.no_color {
            let (outline, sentence) = outline_screen_areas(area);
            if let Some(outline) = outline {
                Outline.render(outline, buf);
            }
            sentence
        } else {
            let (picture, sentence) = empty_screen_areas(area, self.font);
            if let Some(picture) = picture {
                mascot.draw(self.which, picture, buf);
            }
            sentence
        };
        Paragraph::new(SENTENCE)
            .centered()
            .wrap(Wrap { trim: true })
            .render(sentence, buf);
    }
}

/// The fewest rows and columns of the main area that show the picture
/// (Jakob, 2026-10-15). At the smallest screen asqr draws, 60×15, the main
/// area has 9 rows, so the smallest terminals show the sentence alone.
const PICTURE_MIN_ROWS: u16 = 12;
const PICTURE_MIN_COLUMNS: u16 = 30;

/// Where the empty queue in `main` draws the picture, if anywhere, and the
/// sentence, for cells of `font`'s size.
///
/// The picture gets what `main` has above the sentence and a blank row,
/// and [`mascot_rect`] sizes it there, however small that turns out.
pub(super) fn empty_screen_areas(main: Rect, font: FontSize) -> (Option<Rect>, Rect) {
    if main.height < PICTURE_MIN_ROWS || main.width < PICTURE_MIN_COLUMNS {
        return (None, sentence_alone(main));
    }
    let above_sentence = Rect {
        height: main.height - SENTENCE_ROWS - 1,
        ..main
    };
    above_the_sentence(main, mascot_rect(above_sentence, font))
}

/// Where the empty queue in `main` draws the [`Outline`] under `NO_COLOR`,
/// if anywhere, and the sentence.
///
/// The outline has a fixed size, so it shows only where `main` holds all
/// of it, a blank row and the sentence (Jakob, 2026-10-15).
pub(super) fn outline_screen_areas(main: Rect) -> (Option<Rect>, Rect) {
    if main.height < OUTLINE_ROWS + 1 + SENTENCE_ROWS || main.width < OUTLINE_COLUMNS {
        return (None, sentence_alone(main));
    }
    let outline = Rect {
        height: OUTLINE_ROWS,
        ..main.centered_horizontally(Constraint::Length(OUTLINE_COLUMNS))
    };
    above_the_sentence(main, outline)
}

/// The sentence's width in `main`: all of it less a margin of two columns
/// on each side.
fn sentence_width(main: Rect) -> Constraint {
    Constraint::Length(main.width.saturating_sub(4))
}

/// The sentence centred in `main`, where no picture shows.
fn sentence_alone(main: Rect) -> Rect {
    main.centered(sentence_width(main), Constraint::Length(SENTENCE_ROWS))
}

/// `picture`, a rect of `main` centred across, moved down so that it
/// stands directly above the sentence, and the sentence. Picture, a blank
/// row and the sentence form one group, centred down `main`.
fn above_the_sentence(main: Rect, picture: Rect) -> (Option<Rect>, Rect) {
    let group = main.centered_vertically(Constraint::Length(picture.height + 1 + SENTENCE_ROWS));
    let [picture_row, _, sentence] = Layout::vertical([
        Constraint::Length(picture.height),
        Constraint::Length(1),
        Constraint::Length(SENTENCE_ROWS),
    ])
    .areas(group);
    let picture = Rect {
        y: picture_row.y,
        ..picture
    };
    (
        Some(picture),
        sentence.centered_horizontally(sentence_width(main)),
    )
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Cell;
    use ratatui_image::picker::Picker;

    use super::super::mascot::tests::{kitty, transmission};
    use super::super::test_support::find;
    use super::*;

    const GHOSTTY: FontSize = FontSize::new(15, 39);
    const HALFBLOCKS: FontSize = FontSize::new(10, 20);
    /// The cell of a half block: one pixel wide, two tall.
    const HALF_BLOCK: FontSize = FontSize::new(1, 2);

    /// The areas in a main area of `width`×`height` that starts below a
    /// title and a tab bar, as on the screen.
    fn areas(width: u16, height: u16, font: FontSize) -> (Option<Rect>, Rect) {
        empty_screen_areas(Rect::new(0, 4, width, height), font)
    }

    /// The sentence centred in `main`, as it stands without a picture.
    fn alone(width: u16, height: u16) -> Rect {
        Rect::new(0, 4, width, height).centered(
            Constraint::Length(width - 4),
            Constraint::Length(SENTENCE_ROWS),
        )
    }

    #[test]
    fn the_picture_needs_12_rows_and_30_columns() {
        for font in [GHOSTTY, HALFBLOCKS, HALF_BLOCK] {
            assert_eq!(areas(80, 11, font), (None, alone(80, 11)));
            assert!(areas(80, 12, font).0.is_some());
            assert_eq!(areas(29, 20, font), (None, alone(29, 20)));
            assert!(areas(30, 20, font).0.is_some());
        }
    }

    #[test]
    fn the_picture_stands_directly_above_the_sentence_and_both_are_centred() {
        for font in [GHOSTTY, HALFBLOCKS, HALF_BLOCK] {
            for (width, height) in [(30, 12), (80, 14), (80, 30), (200, 60)] {
                let main = Rect::new(0, 4, width, height);
                let (picture, sentence) = empty_screen_areas(main, font);
                let picture = picture.expect("room for the picture");
                let at = format!("{font:?} {width}×{height}: {picture:?} {sentence:?}");

                assert_eq!(sentence.y, picture.bottom() + 1, "{at}");
                assert_eq!(sentence.height, SENTENCE_ROWS, "{at}");
                assert_eq!(sentence.width, width - 4, "{at}");
                assert_eq!(sentence.x, 2, "{at}");
                let left = picture.left() - main.left();
                let right = main.right() - picture.right();
                assert!(left.abs_diff(right) <= 1, "centred across: {at}");
                let above = picture.top() - main.top();
                let below = main.bottom() - sentence.bottom();
                assert!(above.abs_diff(below) <= 1, "centred down: {at}");
                assert!(picture.width <= 44 && picture.height <= 22, "{at}");
            }
        }
    }

    /// Draws the empty queue with `mascot` into a 80×30 buffer.
    fn drawn(which: Which, mascot: &mut Mascot) -> (Buffer, (Option<Rect>, Rect)) {
        let area = Rect::new(0, 0, 80, 30);
        let mut buf = Buffer::empty(area);
        let font = mascot.font();
        EmptyQueue {
            which,
            font,
            no_color: false,
        }
        .render(area, &mut buf, mascot);
        (buf, empty_screen_areas(area, font))
    }

    /// The cells with a Kitty placeholder.
    fn placeholders(buf: &Buffer) -> Vec<(u16, u16)> {
        let area = buf.area;
        area.positions()
            .filter(|&position| buf[position].symbol().contains('\u{10EEEE}'))
            .map(|position| (position.x, position.y))
            .collect()
    }

    #[test]
    fn talqi_fills_its_cells_above_the_sentence() {
        let mut mascot = Mascot::new(kitty());

        let (buf, (picture, sentence)) = drawn(Which::Base, &mut mascot);

        let picture = picture.expect("room for the picture");
        let cells = placeholders(&buf);
        assert_eq!(cells.len(), usize::from(picture.width * picture.height));
        assert!(
            cells.iter().all(|&(x, y)| picture.contains((x, y).into())),
            "{picture:?}"
        );
        assert_eq!(find(&buf, "Nothing to answer").1, sentence.y);
        assert_eq!(mascot.shown(), Some(Which::Base));
    }

    /// The twin of `a_kitty_image_is_placed_over_exactly_its_placeholder_cells`
    /// in `screen.rs`: the mascot goes through the same workaround.
    #[test]
    fn a_kitty_mascot_is_placed_over_exactly_its_placeholder_cells() {
        let mut mascot = Mascot::new(kitty());

        let (buf, (picture, _)) = drawn(Which::Sleeping, &mut mascot);

        let picture = picture.expect("room for the picture");
        let placement = format!("c={},r={}", picture.width, picture.height);
        let sent = transmission(&buf).expect("the first draw sends the picture");
        assert!(sent.contains(&placement), "the placement names {placement}");
    }

    #[test]
    fn without_an_image_protocol_talqi_stands_in_half_blocks_above_the_sentence() {
        let mut mascot = Mascot::new(Picker::halfblocks());

        let (buf, (picture, sentence)) = drawn(Which::Sleeping, &mut mascot);

        let picture = picture.expect("room for the picture");
        assert_eq!((picture.width, picture.height), (44, 22));
        let half_blocks: Vec<_> = buf
            .area
            .positions()
            .filter(|&position| matches!(buf[position].symbol(), "▀" | "▄"))
            .collect();
        assert!(!half_blocks.is_empty(), "Talqi is drawn");
        assert!(
            half_blocks
                .iter()
                .all(|&position| picture.contains(position)),
            "{picture:?}"
        );
        assert_eq!(find(&buf, "Nothing to answer").1, sentence.y);
        assert_eq!(mascot.shown(), Some(Which::Sleeping));
    }

    // -----------------------------------------------------------------
    // The outline under NO_COLOR
    // -----------------------------------------------------------------

    /// The outline is 22 rows, and below it come a blank row and the 3
    /// rows of the sentence.
    #[test]
    fn the_outline_needs_26_rows_and_44_columns() {
        let outline = |width, height| outline_screen_areas(Rect::new(0, 4, width, height));

        assert_eq!(outline(80, 25), (None, alone(80, 25)));
        assert!(outline(80, 26).0.is_some());
        assert_eq!(outline(43, 40), (None, alone(43, 40)));
        assert!(outline(44, 40).0.is_some());
    }

    #[test]
    fn the_outline_stands_directly_above_the_sentence_and_both_are_centred() {
        for (width, height) in [(44, 26), (80, 30), (81, 41), (200, 60)] {
            let main = Rect::new(0, 4, width, height);
            let (outline, sentence) = outline_screen_areas(main);
            let outline = outline.expect("room for the outline");
            let at = format!("{width}×{height}: {outline:?} {sentence:?}");

            assert_eq!((outline.width, outline.height), (44, 22), "{at}");
            assert_eq!(sentence.y, outline.bottom() + 1, "{at}");
            assert_eq!(sentence, Rect::new(2, sentence.y, width - 4, 3), "{at}");
            let left = outline.left() - main.left();
            let right = main.right() - outline.right();
            assert!(left.abs_diff(right) <= 1, "centred across: {at}");
            let above = outline.top() - main.top();
            let below = main.bottom() - sentence.bottom();
            assert!(above.abs_diff(below) <= 1, "centred down: {at}");
        }
    }

    /// Draws the empty queue under NO_COLOR with `mascot` into a buffer
    /// of `width`×`height`.
    fn drawn_without_colours(which: Which, mascot: &mut Mascot, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let font = mascot.font();
        EmptyQueue {
            which,
            font,
            no_color: true,
        }
        .render(area, &mut buf, mascot);
        buf
    }

    /// Under NO_COLOR, Talqi awake and asleep is the same outline, drawn
    /// as text even where the terminal has an image protocol.
    #[test]
    fn under_no_color_talqi_is_the_outline_above_the_sentence_awake_and_asleep() {
        let mut mascot = Mascot::new(kitty());

        let awake = drawn_without_colours(Which::Base, &mut mascot, 80, 30);
        let asleep = drawn_without_colours(Which::Sleeping, &mut mascot, 80, 30);

        assert_eq!(awake, asleep);
        let (outline, sentence) = outline_screen_areas(awake.area);
        let outline = outline.expect("room for the outline");
        let mut expected = Buffer::empty(awake.area);
        Outline.render(outline, &mut expected);
        for position in outline.positions() {
            assert_eq!(awake[position], expected[position], "{position:?}");
        }
        assert!(
            outline
                .positions()
                .any(|position| awake[position] != Cell::default()),
            "the outline is drawn"
        );
        assert_eq!(find(&awake, "Nothing to answer").1, sentence.y);
        assert!(placeholders(&awake).is_empty());
        assert_eq!(transmission(&awake), None);
        assert_eq!(mascot.shown(), None, "no picture is made");
    }

    #[test]
    fn under_no_color_a_small_main_area_shows_the_sentence_alone() {
        let mut mascot = Mascot::new(Picker::halfblocks());

        let buf = drawn_without_colours(Which::Base, &mut mascot, 80, 25);

        let (_, sentence) = outline_screen_areas(buf.area);
        let mut expected = Buffer::empty(buf.area);
        Paragraph::new(SENTENCE)
            .centered()
            .wrap(Wrap { trim: true })
            .render(sentence, &mut expected);
        assert_eq!(buf, expected);
    }
}
