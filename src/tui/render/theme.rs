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

use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::tui::Tone;
use crate::tui::text_field::Level;

/// What marks asqr and what can be acted on: the name in the title line,
/// the current tab, chosen marks, the marker of a default option, the
/// keys in the key bar, overlay borders.
pub(crate) fn primary() -> Style {
    Style::new().fg(Color::Cyan)
}

/// `asqr` in the title line.
pub(crate) fn app_name() -> Style {
    primary().bold()
}

/// The name of a chosen queue on the empty queue (ADR 81).
pub(crate) fn queue_name() -> Style {
    primary().bold()
}

/// A key in the key bar and in the help.
pub(crate) fn key() -> Style {
    primary().bold()
}

/// The current tab in the tab bar. It lifts dim, so an unanswered mark
/// takes the tab's colour with the rest of it.
pub(crate) fn current_tab() -> Style {
    primary().bold().not_dim()
}

/// A chosen mark: of a chosen option, and of an own answer that counts.
pub(crate) fn chosen() -> Style {
    primary().bold()
}

/// The `◀ default` marker at the end of an option with `default: true`,
/// the asker's recommendation (ADR 76).
pub(crate) fn default_marker() -> Style {
    primary().bold()
}

/// The name of what a part of the screen shows: the session in its
/// header, the entry in the context pane's header, an overlay in its
/// frame.
pub(crate) fn title() -> Style {
    Style::new().bold()
}

/// Text that stands out from the text around it without being acted on:
/// an option's label, the head of a question's note, the numbers among
/// the words of the review's counts.
pub(crate) fn strong() -> Style {
    Style::new().bold()
}

/// The colour of the focus, which [`focus_fill`], [`focus_pointer`] and
/// [`focus_rule`] draw with.
pub(crate) fn focus() -> Style {
    Style::new().fg(Color::Yellow)
}

/// The focus as lines: the header rule, left edge and bottom rule of the
/// focused context pane, whose name has the fill.
pub(crate) fn focus_rule() -> Style {
    focus()
}

/// The focus as text, where a fill would make a second focus: the pointer
/// before the submit button, which has a fill of its own.
pub(crate) fn focus_pointer() -> Style {
    focus().bold()
}

/// The row, head or name the cursor or the focus is on, the one focus
/// fill on the screen. The fill is reversed text in the focus colour
/// rather than a background, so it stays a plain reversed fill where
/// colours are off. It lifts dim, so text under it is one colour.
pub(crate) fn focus_fill() -> Style {
    focus().reversed().not_dim()
}

/// Whether `cell` shows the focus fill: reversed text in the focus
/// colour. No other role draws so, which the test over every role keeps
/// true, so the screen behind an overlay can drop the fill by its look
/// (ADR 84).
pub(crate) fn has_focus_fill(cell: &Cell) -> bool {
    cell.modifier.contains(Modifier::REVERSED) && focus_fill().fg == Some(cell.fg)
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

/// `line` under the focus fill: each span gets the fill, as in
/// [`focus_span`], and so does the line's own style. The line's style
/// paints the blanks after the text only where the line is drawn as a
/// widget of its own; in a [`Paragraph`](ratatui::widgets::Paragraph), as
/// on the review, the fill ends with the text.
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
/// unanswered tabs, the descriptions in the key bar, asqr's version on the
/// empty queue and in the help, the context pane's rules and its position.
pub(crate) fn secondary() -> Style {
    Style::new().dim()
}

/// The style of a length counter at `level` (spec section 7.7).
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
/// `NO_COLOR` (spec section 7.3). The modifiers stay, and with them what
/// marks, pointers, bold, dim and reversed say.
///
/// One pass over the finished buffer covers every colour, those of the
/// roles, those patine gives code and Markdown, and any a later change
/// writes without a role.
///
/// RGB colours stay. Images draw with them: half blocks paint the
/// picture in RGB, and the Kitty protocol names each image by the RGB
/// foreground of its placeholder cells, so taking it would lose the image.
/// Talqi's emblem draws in RGB too, but only with colours: under
/// `NO_COLOR` it is an outline without any colour. So does an `ansi`
/// entry whose text sets RGB colours; its viewer is built without any
/// colour under `NO_COLOR`, so none reaches this pass.
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

    /// Every role but the focus fill, by name.
    fn other_roles() -> Vec<(String, Style)> {
        let mut roles = vec![
            ("primary".to_owned(), primary()),
            ("app_name".to_owned(), app_name()),
            ("queue_name".to_owned(), queue_name()),
            ("key".to_owned(), key()),
            ("current_tab".to_owned(), current_tab()),
            ("chosen".to_owned(), chosen()),
            ("default_marker".to_owned(), default_marker()),
            ("title".to_owned(), title()),
            ("strong".to_owned(), strong()),
            ("focus".to_owned(), focus()),
            ("focus_rule".to_owned(), focus_rule()),
            ("focus_pointer".to_owned(), focus_pointer()),
            ("button".to_owned(), button()),
            ("cursor".to_owned(), cursor()),
            ("warning".to_owned(), warning()),
            ("success".to_owned(), success()),
            ("error".to_owned(), error()),
            ("secondary".to_owned(), secondary()),
        ];
        for level in [Level::Fine, Level::Warn, Level::Over] {
            roles.push((format!("for_level({level:?})"), for_level(level)));
        }
        for tone in [Tone::Info, Tone::Warning, Tone::Error, Tone::Success] {
            roles.push((format!("for_tone({tone:?})"), for_tone(tone)));
        }
        roles
    }

    /// The screen behind an overlay drops the focus fill by its look, so a
    /// role that looked the same would lose its own fill there.
    #[test]
    fn no_other_role_looks_like_the_focus_fill() {
        let filled = |style: Style| {
            let mut cell = Cell::default();
            cell.set_style(style);
            has_focus_fill(&cell)
        };

        assert!(filled(focus_fill()));
        for (name, style) in other_roles() {
            assert!(!filled(style), "{name} looks like the focus fill");
        }
    }

    #[test]
    fn primary_is_cyan_and_the_focus_yellow() {
        assert_eq!(primary(), Style::new().fg(Color::Cyan));
        assert_eq!(focus(), Style::new().fg(Color::Yellow));
    }

    #[test]
    fn the_app_name_keys_chosen_marks_and_the_default_marker_are_bold_primary() {
        let bold_cyan = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
        assert_eq!(app_name(), bold_cyan);
        assert_eq!(key(), bold_cyan);
        assert_eq!(chosen(), bold_cyan);
        assert_eq!(default_marker(), bold_cyan);
    }

    #[test]
    fn the_queue_name_is_bold_primary() {
        assert_eq!(
            queue_name(),
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn the_current_tab_is_bold_primary_that_lifts_dim() {
        assert_eq!(
            current_tab(),
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::DIM)
        );
    }

    #[test]
    fn titles_and_strong_text_are_bold_in_the_text_colour() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        assert_eq!(title(), bold);
        assert_eq!(strong(), bold);
    }

    #[test]
    fn the_focused_pane_s_rules_are_yellow() {
        assert_eq!(focus_rule(), Style::new().fg(Color::Yellow));
    }

    #[test]
    fn the_pointer_before_the_button_is_bold_yellow() {
        assert_eq!(
            focus_pointer(),
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        );
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
        let mark = Span::styled("(•)", chosen());
        let number = Span::styled(" 1. ", secondary());

        let mark = focus_span(mark).style;
        let number = focus_span(number).style;

        for style in [mark, number] {
            assert_eq!(style.fg, Some(Color::Yellow));
            assert!(style.add_modifier.contains(Modifier::REVERSED));
            assert!(!style.add_modifier.contains(Modifier::DIM));
        }
        assert!(mark.add_modifier.contains(Modifier::BOLD), "keeps its bold");
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
