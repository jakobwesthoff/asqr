// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The main area while no session waits (spec section 7.5, ADR 43): a
//! sentence that says so, with Talqi above it where there is room.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::mascot::{EMBLEM_COLUMNS, EMBLEM_ROWS, Glyphs};

/// What the empty queue says.
const SENTENCE: &str = "Nothing to answer. New sessions show up here as soon as they arrive.";

/// The rows kept for the sentence, which wraps on a narrow screen.
const SENTENCE_ROWS: u16 = 3;

/// The empty queue: Talqi's emblem in `glyphs` above the sentence, where
/// `talqi` holds.
pub(super) struct EmptyQueue {
    pub(super) glyphs: Glyphs,
    /// Whether Talqi shows: the dialog of an invalid session hides him
    /// while it is open (ADR 49).
    pub(super) talqi: bool,
    /// Whether the screen is drawn without colours (`NO_COLOR`). Talqi is
    /// then the emblem's outline, which has no colour at all.
    pub(super) no_color: bool,
}

impl Widget for EmptyQueue {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (emblem, sentence) = if self.talqi {
            emblem_screen_areas(area)
        } else {
            (None, sentence_alone(area))
        };
        if let Some(emblem) = emblem {
            self.glyphs.template(self.no_color).render(emblem, buf);
        }
        Paragraph::new(SENTENCE)
            .centered()
            .wrap(Wrap { trim: true })
            .render(sentence, buf);
    }
}

/// Where the empty queue in `main` draws Talqi, if anywhere, and the
/// sentence.
///
/// The emblem has a fixed size, so it shows only where `main` holds all
/// of it, a blank row and the sentence: 26 rows and
/// 44 columns, with colours and without.
pub(super) fn emblem_screen_areas(main: Rect) -> (Option<Rect>, Rect) {
    if main.height < EMBLEM_ROWS + 1 + SENTENCE_ROWS || main.width < EMBLEM_COLUMNS {
        return (None, sentence_alone(main));
    }
    let emblem = Rect {
        height: EMBLEM_ROWS,
        ..main.centered_horizontally(Constraint::Length(EMBLEM_COLUMNS))
    };
    above_the_sentence(main, emblem)
}

/// The sentence's width in `main`: all of it less a margin of two columns
/// on each side.
fn sentence_width(main: Rect) -> Constraint {
    Constraint::Length(main.width.saturating_sub(4))
}

/// The sentence centred in `main`, where Talqi does not show.
fn sentence_alone(main: Rect) -> Rect {
    main.centered(sentence_width(main), Constraint::Length(SENTENCE_ROWS))
}

/// `emblem`, a rect of `main` centred across, moved down so that it
/// stands directly above the sentence, and the sentence. Emblem, a blank
/// row and the sentence form one group, centred down `main`.
fn above_the_sentence(main: Rect, emblem: Rect) -> (Option<Rect>, Rect) {
    let group = main.centered_vertically(Constraint::Length(emblem.height + 1 + SENTENCE_ROWS));
    let [emblem_row, _, sentence] = Layout::vertical([
        Constraint::Length(emblem.height),
        Constraint::Length(1),
        Constraint::Length(SENTENCE_ROWS),
    ])
    .areas(group);
    let emblem = Rect {
        y: emblem_row.y,
        ..emblem
    };
    (
        Some(emblem),
        sentence.centered_horizontally(sentence_width(main)),
    )
}

#[cfg(test)]
mod tests {
    use super::super::test_support::find;
    use super::*;

    /// The sentence centred in a main area of `width`×`height` that starts
    /// below a title and a tab bar, as it stands without Talqi.
    fn alone(width: u16, height: u16) -> Rect {
        Rect::new(0, 4, width, height).centered(
            Constraint::Length(width - 4),
            Constraint::Length(SENTENCE_ROWS),
        )
    }

    /// The emblem is 22 rows, and below it come a blank row and the 3 rows
    /// of the sentence.
    #[test]
    fn the_emblem_needs_26_rows_and_44_columns() {
        let emblem = |width, height| emblem_screen_areas(Rect::new(0, 4, width, height));

        assert_eq!(emblem(80, 25), (None, alone(80, 25)));
        assert!(emblem(80, 26).0.is_some());
        assert_eq!(emblem(43, 40), (None, alone(43, 40)));
        assert!(emblem(44, 40).0.is_some());
    }

    #[test]
    fn the_emblem_stands_directly_above_the_sentence_and_both_are_centred() {
        for (width, height) in [(44, 26), (80, 30), (81, 41), (200, 60)] {
            let main = Rect::new(0, 4, width, height);
            let (emblem, sentence) = emblem_screen_areas(main);
            let emblem = emblem.expect("room for the emblem");
            let at = format!("{width}×{height}: {emblem:?} {sentence:?}");

            assert_eq!((emblem.width, emblem.height), (44, 22), "{at}");
            assert_eq!(sentence.y, emblem.bottom() + 1, "{at}");
            assert_eq!(sentence, Rect::new(2, sentence.y, width - 4, 3), "{at}");
            let left = emblem.left() - main.left();
            let right = main.right() - emblem.right();
            assert!(left.abs_diff(right) <= 1, "centred across: {at}");
            let above = emblem.top() - main.top();
            let below = main.bottom() - sentence.bottom();
            assert!(above.abs_diff(below) <= 1, "centred down: {at}");
        }
    }

    /// The empty queue drawn into a buffer of `width`×`height`.
    fn drawn(glyphs: Glyphs, no_color: bool, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        EmptyQueue {
            glyphs,
            talqi: true,
            no_color,
        }
        .render(area, &mut buf);
        buf
    }

    /// With colours Talqi is the colour template, under NO_COLOR the
    /// outline, each in the glyphs asked for, above the sentence.
    #[test]
    fn talqi_is_the_template_of_the_glyphs_and_the_colours_above_the_sentence() {
        for glyphs in [Glyphs::Sextant, Glyphs::Quadrant] {
            for no_color in [false, true] {
                let buf = drawn(glyphs, no_color, 80, 30);

                let (emblem, sentence) = emblem_screen_areas(buf.area);
                let emblem = emblem.expect("room for the emblem");
                let mut expected = Buffer::empty(buf.area);
                glyphs.template(no_color).render(emblem, &mut expected);
                for position in emblem.positions() {
                    assert_eq!(
                        buf[position], expected[position],
                        "{glyphs:?} {no_color} {position:?}"
                    );
                }
                assert_eq!(find(&buf, "Nothing to answer").1, sentence.y);
            }
        }
    }

    #[test]
    fn a_small_main_area_shows_the_sentence_alone() {
        for no_color in [false, true] {
            let buf = drawn(Glyphs::Sextant, no_color, 80, 25);

            let (_, sentence) = emblem_screen_areas(buf.area);
            let mut expected = Buffer::empty(buf.area);
            Paragraph::new(SENTENCE)
                .centered()
                .wrap(Wrap { trim: true })
                .render(sentence, &mut expected);
            assert_eq!(buf, expected, "{no_color}");
        }
    }
}
