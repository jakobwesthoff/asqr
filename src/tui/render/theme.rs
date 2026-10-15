// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The colour roles of the design guide's terminal section, each as the
//! `Style` that draws it. Drawing code asks for a role rather than for a
//! colour or a modifier, so the look of a role is decided here alone.
//!
//! Every role is a whole `Style`, never a bare `Color`: a call site patches
//! it onto the style it has, and a role may be more than a colour, such as
//! a fill.
//!
//! Code and Markdown keep the colours patine gives them (ADR 34) and take
//! no role.

use ratatui::style::{Color, Style};

use crate::tui::text_field::Level;

/// The row, head or header the cursor or focus is on, and the other filled
/// areas of the screen.
pub(crate) fn focus_fill() -> Style {
    Style::new().reversed()
}

/// The cell of the text cursor in a field that has the keys. A role of its
/// own, apart from [`focus_fill`]: it marks where typing goes, not which
/// row is current.
pub(crate) fn cursor() -> Style {
    Style::new().reversed()
}

/// Something to look at but no failure: a notice in the status line, a
/// length counter above its target.
pub(crate) fn warning() -> Style {
    Style::new().fg(Color::Yellow)
}

/// Something in order: a length counter within its target, an added line
/// of a diff.
pub(crate) fn success() -> Style {
    Style::new().fg(Color::Green)
}

/// Something wrong or gone: a length counter above `warn`, a removed line
/// of a diff.
pub(crate) fn error() -> Style {
    Style::new().fg(Color::Red)
}

/// Text that explains or annotates rather than being the content itself:
/// hints, line numbers, notes in place of content, markers of cut text.
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

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::*;

    #[test]
    fn the_fills_and_the_text_cursor_are_plain_reversed_cells() {
        let reversed = Style::new().add_modifier(Modifier::REVERSED);
        assert_eq!(focus_fill(), reversed);
        assert_eq!(cursor(), reversed);
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
}
