// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The main area while no session waits (spec section 7.1, ADR 39): a
//! sentence that says so, with Talqi above it where there is room.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, StatefulWidget, Widget, Wrap};
use ratatui_image::FontSize;

use super::mascot::{Mascot, Which, mascot_rect};

/// What the empty queue says.
const SENTENCE: &str = "Nothing to answer. New sessions show up here as soon as they arrive.";

/// The rows kept for the sentence, which wraps on a narrow screen.
const SENTENCE_ROWS: u16 = 3;

/// The empty queue: Talqi, picture `which`, drawn in cells of `font`'s
/// size in pixels, above the sentence.
pub(super) struct EmptyQueue {
    pub(super) which: Which,
    pub(super) font: FontSize,
}

impl StatefulWidget for EmptyQueue {
    type State = Mascot;

    fn render(self, area: Rect, buf: &mut Buffer, mascot: &mut Mascot) {
        let (picture, sentence) = empty_screen_areas(area, self.font);
        if let Some(picture) = picture {
            mascot.draw(self.which, picture, buf);
        }
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
/// and [`mascot_rect`] sizes it there. Picture, blank row and sentence then
/// form one group, centred in `main`, so the picture stands directly above
/// the sentence however small it turns out.
pub(super) fn empty_screen_areas(main: Rect, font: FontSize) -> (Option<Rect>, Rect) {
    let sentence_width = Constraint::Length(main.width.saturating_sub(4));
    if main.height < PICTURE_MIN_ROWS || main.width < PICTURE_MIN_COLUMNS {
        let alone = main.centered(sentence_width, Constraint::Length(SENTENCE_ROWS));
        return (None, alone);
    }
    let above_sentence = Rect {
        height: main.height - SENTENCE_ROWS - 1,
        ..main
    };
    let picture = mascot_rect(above_sentence, font);
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
        sentence.centered_horizontally(sentence_width),
    )
}

#[cfg(test)]
mod tests {
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
        EmptyQueue { which, font }.render(area, &mut buf, mascot);
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
}
