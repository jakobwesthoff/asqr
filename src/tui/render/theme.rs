// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The colour roles of the design guide's terminal section, each as the
//! `Style` that draws it (ADR 37). Drawing code asks for a role rather
//! than for a colour or a modifier, so the look of a role is decided here
//! alone.
//!
//! Every role is a whole `Style`, never a bare `Color`: a call site patches
//! it onto the style it has, and a role may be more than a colour, such as
//! a fill.
//!
//! The colours are the terminal's named ones, so the person's theme
//! decides how they look. Code and Markdown keep the colours patine gives
//! them (ADR 34) and take no role.

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::tui::Tone;
use crate::tui::text_field::Level;

/// What marks asqr and what can be acted on: the name in the title line,
/// the current tab, chosen marks, the keys in the key bar, overlay
/// borders.
pub(crate) fn primary() -> Style {
    Style::new().fg(Color::Cyan)
}

/// The colour of the focus as text, where a fill would make a second
/// focus: the pointer before the submit button, which has a fill of its
/// own.
pub(crate) fn focus() -> Style {
    Style::new().fg(Color::Yellow)
}

/// The row, head or header the cursor or the focus is on, the one focus
/// fill on the screen. The fill is reversed text in the focus colour
/// rather than a background, so it stays a plain reversed fill where
/// colours are off. It lifts dim, so text under it is one colour.
pub(crate) fn focus_fill() -> Style {
    focus().reversed().not_dim()
}

/// The submit button: always filled in the primary colour, the one fill
/// that is not the focus. Its brackets keep it apart from the focus fill
/// where colours are off.
pub(crate) fn button() -> Style {
    primary().reversed().bold()
}

/// `span` under the focus fill. A span's own colour wins over its line's
/// style, so a filled head has the fill patched onto each of its spans.
pub(crate) fn focus_span(span: Span<'static>) -> Span<'static> {
    span.patch_style(focus_fill())
}

/// `line` under the focus fill, to the edge of the area it is drawn in:
/// each span gets the fill, as in [`focus_span`], and so does the line's
/// own style, which alone paints the blanks after its text where the line
/// is drawn as a widget of its own.
pub(crate) fn focus_row(line: Line<'static>) -> Line<'static> {
    let spans = line.spans.into_iter().map(focus_span).collect::<Vec<_>>();
    Line {
        spans,
        style: line.style.patch(focus_fill()),
        ..line
    }
}

/// The cell of the text cursor in a field that has the keys. A role of its
/// own, apart from [`focus_fill`]: it marks where typing goes, not which
/// row is current.
pub(crate) fn cursor() -> Style {
    Style::new().reversed()
}

/// Something to look at but no failure: a refused key, a session set
/// aside, a length counter above its target.
pub(crate) fn warning() -> Style {
    Style::new().fg(Color::Yellow)
}

/// Something in order: a sent session, a length counter within its
/// target, an added line of a diff.
pub(crate) fn success() -> Style {
    Style::new().fg(Color::Green)
}

/// Something wrong or gone: a failure, a length counter above `warn`, a
/// removed line of a diff.
pub(crate) fn error() -> Style {
    Style::new().fg(Color::Red)
}

/// Text that explains or annotates rather than being the content itself:
/// hints, line numbers, notes in place of content, markers of cut text,
/// unanswered tabs, the descriptions in the key bar.
pub(crate) fn secondary() -> Style {
    Style::new().dim()
}

/// The style of a length counter at `level` (spec section 7.3).
pub(crate) fn for_level(level: Level) -> Style {
    match level {
        Level::Fine => success(),
        Level::Warn => warning(),
        Level::Over => error(),
    }
}

/// The style of a notice or a session's message of `tone` in the status
/// line (ADR 38). Plain news keeps the text colour.
pub(crate) fn for_tone(tone: Tone) -> Style {
    match tone {
        Tone::Info => Style::new(),
        Tone::Warning => warning(),
        Tone::Error => error(),
        Tone::Success => success(),
    }
}

/// Takes every named and indexed colour out of the drawn `buf`, for
/// `NO_COLOR` (spec section 8). The modifiers stay, and with them what
/// marks, pointers, bold, dim and reversed say.
///
/// One pass over the finished buffer covers every colour, those of the
/// roles, those patine gives code and Markdown, and any a later change
/// writes without a role.
///
/// RGB colours stay. Only images draw with them: half blocks paint the
/// picture in RGB, and the Kitty protocol names each image by the RGB
/// foreground of its placeholder cells, so taking it would lose the image.
pub(crate) fn strip_colours(buf: &mut Buffer) {
    let plain = |colour: Color| match colour {
        Color::Rgb(..) => colour,
        _ => Color::Reset,
    };
    for cell in &mut buf.content {
        cell.fg = plain(cell.fg);
        cell.bg = plain(cell.bg);
        cell.underline_color = plain(cell.underline_color);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;
    use ratatui::widgets::Widget;

    use super::*;

    #[test]
    fn primary_is_cyan_and_the_focus_yellow() {
        assert_eq!(primary(), Style::new().fg(Color::Cyan));
        assert_eq!(focus(), Style::new().fg(Color::Yellow));
    }

    #[test]
    fn the_focus_fill_is_reversed_yellow_that_lifts_dim() {
        assert_eq!(
            focus_fill(),
            Style::new()
                .fg(Color::Yellow)
                .add_modifier(Modifier::REVERSED)
                .remove_modifier(Modifier::DIM)
        );
    }

    #[test]
    fn the_submit_button_is_bold_reversed_cyan() {
        assert_eq!(
            button(),
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD)
        );
    }

    #[test]
    fn the_text_cursor_is_a_plain_reversed_cell() {
        assert_eq!(cursor(), Style::new().add_modifier(Modifier::REVERSED));
    }

    #[test]
    fn a_filled_span_takes_the_fill_colour_and_loses_its_dim() {
        let chosen = Span::styled("(•)", primary().bold());
        let number = Span::styled(" 1. ", secondary());

        let chosen = focus_span(chosen).style;
        let number = focus_span(number).style;

        for style in [chosen, number] {
            assert_eq!(style.fg, Some(Color::Yellow));
            assert!(style.add_modifier.contains(Modifier::REVERSED));
            assert!(!style.add_modifier.contains(Modifier::DIM));
        }
        assert!(
            chosen.add_modifier.contains(Modifier::BOLD),
            "keeps its bold"
        );
    }

    #[test]
    fn a_filled_row_is_one_colour_to_the_edge_of_its_area() {
        let row = Line::from(vec![
            Span::from("❯ ☐ "),
            Span::styled("-", secondary()),
            Span::styled("x", primary()),
        ]);
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        focus_row(row).render(area, &mut buf);

        for x in 0..10 {
            let cell = &buf[(x, 0)];
            assert_eq!(
                (cell.fg, cell.modifier),
                (Color::Yellow, Modifier::REVERSED),
                "column {x}"
            );
        }
    }

    #[test]
    fn the_status_roles_are_yellow_green_and_red_text() {
        assert_eq!(warning(), Style::new().fg(Color::Yellow));
        assert_eq!(success(), Style::new().fg(Color::Green));
        assert_eq!(error(), Style::new().fg(Color::Red));
    }

    #[test]
    fn secondary_text_is_dim() {
        assert_eq!(secondary(), Style::new().add_modifier(Modifier::DIM));
    }

    #[test]
    fn a_counter_is_green_in_range_yellow_above_its_target_and_red_above_warn() {
        assert_eq!(for_level(Level::Fine), Style::new().fg(Color::Green));
        assert_eq!(for_level(Level::Warn), Style::new().fg(Color::Yellow));
        assert_eq!(for_level(Level::Over), Style::new().fg(Color::Red));
    }

    #[test]
    fn news_is_plain_a_refusal_yellow_a_failure_red_and_a_sent_session_green() {
        assert_eq!(for_tone(Tone::Info), Style::new());
        assert_eq!(for_tone(Tone::Warning), Style::new().fg(Color::Yellow));
        assert_eq!(for_tone(Tone::Error), Style::new().fg(Color::Red));
        assert_eq!(for_tone(Tone::Success), Style::new().fg(Color::Green));
    }

    #[test]
    fn stripping_resets_named_and_indexed_colours_and_keeps_rgb_and_modifiers() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf[(0, 0)].set_style(
            Style::new()
                .fg(Color::Yellow)
                .bg(Color::Blue)
                .underline_color(Color::Red)
                .bold()
                .reversed(),
        );
        buf[(1, 0)].set_style(
            Style::new()
                .fg(Color::Indexed(6))
                .bg(Color::Indexed(236))
                .dim(),
        );
        buf[(2, 0)].set_style(
            Style::new()
                .fg(Color::Rgb(0, 0, 1))
                .bg(Color::Rgb(255, 0, 0)),
        );

        strip_colours(&mut buf);

        let named = &buf[(0, 0)];
        assert_eq!(
            (named.fg, named.bg, named.underline_color),
            (Color::Reset, Color::Reset, Color::Reset)
        );
        assert_eq!(named.modifier, Modifier::BOLD | Modifier::REVERSED);
        let indexed = &buf[(1, 0)];
        assert_eq!((indexed.fg, indexed.bg), (Color::Reset, Color::Reset));
        assert_eq!(indexed.modifier, Modifier::DIM);
        let rgb = &buf[(2, 0)];
        assert_eq!(
            (rgb.fg, rgb.bg),
            (Color::Rgb(0, 0, 1), Color::Rgb(255, 0, 0)),
            "image cells keep their colours, and Kitty its image id"
        );
    }
}
