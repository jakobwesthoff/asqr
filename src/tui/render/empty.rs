// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The main area while no session waits (spec section 7.5, ADR 43): a
//! sentence that says so, with Talqi above it where there is room.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::mascot::{EMBLEM_COLUMNS, EMBLEM_ROWS, Glyphs};
use super::screen::shorten_from_left;
use super::source_line::columns;
use super::theme;

/// What the empty queue of the default queue says.
const SENTENCE: &str = "Nothing to answer. New sessions show up here as soon as they arrive.";

/// The start of the sentence of a chosen queue, which its name follows,
/// and the sentence's second line (ADR 81).
const NAMING: &str = "Nothing to answer in queue";
const NEW_SESSIONS: &str = "New sessions show up here as soon as they arrive.";

/// The rows kept for the sentence, which wraps on a narrow screen.
const SENTENCE_ROWS: u16 = 3;

/// The empty queue: Talqi's emblem in `glyphs` above the sentence, where
/// `talqi` holds.
pub(super) struct EmptyQueue<'a> {
    /// The chosen queue the sentence names, `None` for the default queue
    /// (ADR 81).
    pub(super) queue: Option<&'a str>,
    pub(super) glyphs: Glyphs,
    /// Whether Talqi shows: the dialog of an invalid session hides him
    /// while it is open (ADR 49).
    pub(super) talqi: bool,
    /// Whether the screen is drawn without colours (`NO_COLOR`). Talqi is
    /// then the emblem's outline, which has no colour at all.
    pub(super) no_color: bool,
}

impl Widget for EmptyQueue<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (emblem, sentence) = if self.talqi {
            emblem_screen_areas(area)
        } else {
            (None, sentence_alone(area))
        };
        if let Some(emblem) = emblem {
            self.glyphs.template(self.no_color).render(emblem, buf);
        }
        match self.queue {
            None => Paragraph::new(SENTENCE)
                .centered()
                .wrap(Wrap { trim: true })
                .render(sentence, buf),
            Some(queue) => Paragraph::new(naming_lines(queue, sentence.width))
                .centered()
                .render(sentence, buf),
        }
    }
}

/// The sentence that names the chosen `queue`, in rows of `width`
/// columns: two, and three where the name does not fit beside the
/// sentence's start.
///
/// A name on a row of its own stays whole, since a cut name tells queues
/// apart less well than a wrapped one. Only a name too long for that row
/// shows its end behind `…`, as the title bar shortens it. The second
/// line, 49 columns, fits the narrowest sentence, 56 columns on a screen
/// of 60, so the sentence never needs more than the rows kept for it.
fn naming_lines(queue: &str, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let name = Span::styled(
        shorten_from_left(queue, width.saturating_sub(1)),
        theme::queue_name(),
    );
    let beside = columns(&format!("{NAMING} {}.", name.content)) <= width;
    let mut lines = if beside {
        vec![Line::from(vec![
            Span::raw(format!("{NAMING} ")),
            name,
            Span::raw("."),
        ])]
    } else {
        vec![Line::from(NAMING), Line::from(vec![name, Span::raw(".")])]
    };
    lines.push(Line::from(NEW_SESSIONS));
    lines
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
    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{find, look_at, row};
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
            queue: None,
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

    // -----------------------------------------------------------------
    // A chosen queue (ADR 81)
    // -----------------------------------------------------------------

    const NEW_SESSIONS_LINE: &str = "New sessions show up here as soon as they arrive.";

    /// The empty queue of the chosen `queue`, with colours, drawn into a
    /// buffer of `width`×`height`.
    fn drawn_for(queue: &str, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        EmptyQueue {
            queue: Some(queue),
            glyphs: Glyphs::Sextant,
            talqi: true,
            no_color: false,
        }
        .render(area, &mut buf);
        buf
    }

    /// The rows of the sentence in `buf`, from the one that starts it,
    /// without the blanks around them, up to the first empty row.
    fn sentence_rows(buf: &Buffer) -> Vec<String> {
        let (_, top) = find(buf, "Nothing to answer");
        (top..buf.area.height)
            .map(|y| row(buf, y).trim_start().to_owned())
            .take_while(|text| !text.is_empty())
            .collect()
    }

    #[test]
    fn a_chosen_queue_is_named_in_the_first_of_two_lines() {
        let buf = drawn_for("asqr-todos", 80, 30);

        let (_, sentence) = emblem_screen_areas(buf.area);
        let first = "Nothing to answer in queue asqr-todos.";
        assert_eq!(find(&buf, first).1, sentence.y, "under Talqi's blank row");
        assert_eq!(sentence_rows(&buf), [first, NEW_SESSIONS_LINE]);
        for line in [first, NEW_SESSIONS_LINE] {
            let left = find(&buf, line).0;
            let right = 80 - left - u16::try_from(line.len()).expect("short");
            assert!(left.abs_diff(right) <= 1, "centred: {line}");
        }
        assert_eq!(look_at(&buf, "asqr-todos"), (Color::Cyan, Modifier::BOLD));
        assert_eq!(look_at(&buf, "Nothing"), (Color::Reset, Modifier::empty()));
        assert_eq!(look_at(&buf, "New"), (Color::Reset, Modifier::empty()));
    }

    /// Wide characters take two columns each: this name of 21 columns, 31
    /// bytes, fits beside the start of the sentence on a narrow screen.
    #[test]
    fn a_name_of_wide_characters_is_measured_in_columns() {
        let buf = drawn_for("日本語のキュー/日本語", 60, 15);

        assert_eq!(
            sentence_rows(&buf),
            [
                "Nothing to answer in queue 日本語のキュー/日本語.",
                NEW_SESSIONS_LINE
            ]
        );
    }

    #[test]
    fn a_name_too_long_for_the_line_takes_a_line_of_its_own() {
        let buf = drawn_for("asqr-todos-worked-off-by-agents", 60, 15);

        assert_eq!(
            sentence_rows(&buf),
            [
                "Nothing to answer in queue",
                "asqr-todos-worked-off-by-agents.",
                NEW_SESSIONS_LINE
            ]
        );
        assert_eq!(
            look_at(&buf, "asqr-todos-worked"),
            (Color::Cyan, Modifier::BOLD)
        );
    }

    /// The name's row holds 56 columns at the smallest width, the name 55
    /// of them before its full stop.
    #[test]
    fn a_name_too_long_for_a_line_is_shortened_from_the_left() {
        let name = format!("{}{}", "a".repeat(16), "b".repeat(54));
        let buf = drawn_for(&name, 60, 15);
        assert_eq!(
            sentence_rows(&buf),
            [
                "Nothing to answer in queue".to_owned(),
                format!("…{}.", "b".repeat(54)),
                NEW_SESSIONS_LINE.to_owned()
            ]
        );
        assert_eq!(look_at(&buf, "…"), (Color::Cyan, Modifier::BOLD));

        let path = format!("/srv/{}/{}/queue", "x".repeat(40), "y".repeat(30));
        let buf = drawn_for(&path, 60, 15);
        assert_eq!(
            sentence_rows(&buf)[1],
            format!("…/{}/queue.", "y".repeat(30)),
            "cut at a slash"
        );
    }

    /// The default queue's screen stays as it was, Talqi and the sentence
    /// in one line.
    #[test]
    fn without_a_chosen_queue_the_sentence_is_today_s() {
        let buf = drawn(Glyphs::Sextant, false, 80, 30);

        let (emblem, sentence) = emblem_screen_areas(buf.area);
        let mut expected = Buffer::empty(buf.area);
        Glyphs::Sextant
            .template(false)
            .render(emblem.expect("room for the emblem"), &mut expected);
        Paragraph::new(SENTENCE)
            .centered()
            .wrap(Wrap { trim: true })
            .render(sentence, &mut expected);
        assert_eq!(buf, expected);
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
