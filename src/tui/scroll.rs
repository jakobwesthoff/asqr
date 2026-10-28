// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scrolling a view that is larger than its place on screen (spec section
//! 7.6). How far a view can scroll depends on
//! the size it was drawn at, so each draw records what it showed as a
//! [`Page`], and the keys that follow move within it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// How much of a scrolling view one draw showed in one direction: `shown`
/// lines (or columns) of `total`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Extent {
    pub shown: usize,
    pub total: usize,
}

impl Extent {
    /// The furthest start that still fills the view.
    pub fn last_start(self) -> usize {
        self.total.saturating_sub(self.shown)
    }

    /// How far `ctrl-u` and `ctrl-d` move: half of what shows, rounded
    /// down as in Vim, and at least one line.
    fn half(self) -> usize {
        (self.shown / 2).max(1)
    }

    /// How far `pgup` and `pgdn` move: what shows less [`PAGE_OVERLAP`]
    /// lines, so the lines at the edge show again at the other end, as
    /// with Vim's `ctrl-f`, and at least one line, so a view of a few
    /// rows still moves on.
    fn page(self) -> usize {
        self.shown.saturating_sub(PAGE_OVERLAP).max(1)
    }

    /// Which lines show from `start` on, counted from 1, as "4–15 of 120";
    /// `None` where all of them fit, as then there is nothing to say.
    pub fn position(self, start: usize) -> Option<String> {
        (self.total > self.shown)
            .then(|| format!("{}–{} of {}", start + 1, start + self.shown, self.total))
    }
}

/// What one draw of a scrolling view showed, down and sideways.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Page {
    pub rows: Extent,
    pub columns: Extent,
}

impl Page {
    /// `offset` brought back within this page, so that the view stays full.
    /// An offset goes past the end where a larger terminal shows more of
    /// the view than the one it was scrolled in.
    pub fn clamp(self, offset: Offset) -> Offset {
        Offset {
            top: offset.top.min(self.rows.last_start()),
            left: offset.left.min(self.columns.last_start()),
        }
    }
}

/// Where a scrolling view starts: the first line and the first column
/// shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Offset {
    pub top: usize,
    pub left: usize,
}

/// How many columns `←`/`→` and `h`/`l` move sideways. A column at a time
/// is too slow to read along a long line, and half the width, as `less`
/// does, loses the place in it.
const SIDEWAYS_STEP: usize = 4;

/// How many lines a page that `pgup` or `pgdn` turns shares with the page
/// before it (ADR 72).
const PAGE_OVERLAP: usize = 2;

/// The offset after `key` in a view that showed `page` from `offset` on,
/// or `None` when `key` does not scroll.
///
/// `pgup`/`pgdn` scroll by a page less two lines and `ctrl-u`/`ctrl-d` by
/// half a page, as in Vim, in any case, as they do not type text. The line,
/// sideways and end keys (`↑`/`↓`, `j`/`k`, `←`/`→`, `h`/`l`, `g`/`G`,
/// `home`/`end`) only scroll a view that is `focused`, as elsewhere they
/// move the cursor or type. The result never goes past either end, and
/// neither does an offset that a smaller terminal left past the end: it is
/// brought back first, so the first key after a resize already moves the
/// view.
pub fn scroll(key: KeyEvent, offset: Offset, page: Page, focused: bool) -> Option<Offset> {
    let Offset { top, left } = page.clamp(offset);
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let (top, left) = match key.code {
        KeyCode::Char('u') if control => (top.saturating_sub(page.rows.half()), left),
        KeyCode::Char('d') if control => (top + page.rows.half(), left),
        KeyCode::PageUp => (top.saturating_sub(page.rows.page()), left),
        KeyCode::PageDown => (top + page.rows.page(), left),
        _ if control || !focused => return None,
        KeyCode::Char('k') | KeyCode::Up => (top.saturating_sub(1), left),
        KeyCode::Char('j') | KeyCode::Down => (top + 1, left),
        KeyCode::Char('h') | KeyCode::Left => (top, left.saturating_sub(SIDEWAYS_STEP)),
        KeyCode::Char('l') | KeyCode::Right => (top, left + SIDEWAYS_STEP),
        KeyCode::Char('g') | KeyCode::Home => (0, left),
        KeyCode::Char('G') | KeyCode::End => (page.rows.last_start(), left),
        _ => return None,
    };
    Some(page.clamp(Offset { top, left }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// 10 of 40 lines and 20 of 50 columns.
    const PAGE: Page = Page {
        rows: Extent {
            shown: 10,
            total: 40,
        },
        columns: Extent {
            shown: 20,
            total: 50,
        },
    };

    fn at(top: usize, left: usize) -> Offset {
        Offset { top, left }
    }

    #[test]
    fn a_focused_view_scrolls_by_line_page_and_to_its_ends() {
        let cases = [
            (key('j'), at(5, 8), at(6, 8)),
            (code(KeyCode::Down), at(5, 8), at(6, 8)),
            (key('k'), at(5, 8), at(4, 8)),
            (code(KeyCode::Up), at(0, 8), at(0, 8)),
            (code(KeyCode::PageDown), at(5, 8), at(13, 8)),
            (code(KeyCode::PageUp), at(5, 8), at(0, 8)),
            (key('g'), at(25, 8), at(0, 8)),
            (code(KeyCode::Home), at(25, 8), at(0, 8)),
            (key('G'), at(5, 8), at(30, 8)),
            (code(KeyCode::End), at(5, 8), at(30, 8)),
            (key('j'), at(30, 8), at(30, 8)),
        ];
        for (event, from, to) in cases {
            assert_eq!(
                scroll(event, from, PAGE, true),
                Some(to),
                "{event:?} from {from:?}"
            );
        }
    }

    #[test]
    fn a_focused_view_scrolls_sideways_in_steps_within_its_width() {
        let cases = [
            (key('l'), at(5, 8), at(5, 12)),
            (code(KeyCode::Right), at(5, 28), at(5, 30)),
            (key('h'), at(5, 8), at(5, 4)),
            (code(KeyCode::Left), at(5, 2), at(5, 0)),
        ];
        for (event, from, to) in cases {
            assert_eq!(
                scroll(event, from, PAGE, true),
                Some(to),
                "{event:?} from {from:?}"
            );
        }

        let narrow = Page {
            columns: Extent {
                shown: 20,
                total: 20,
            },
            ..PAGE
        };
        assert_eq!(
            scroll(key('l'), at(5, 0), narrow, true),
            Some(at(5, 0)),
            "content that fits the width does not move"
        );
    }

    #[test]
    fn an_unfocused_view_scrolls_only_by_the_page_keys() {
        assert_eq!(
            scroll(code(KeyCode::PageDown), at(0, 0), PAGE, false),
            Some(at(8, 0))
        );
        assert_eq!(
            scroll(code(KeyCode::PageUp), at(10, 0), PAGE, false),
            Some(at(2, 0))
        );
        for event in [key('j'), key('G'), code(KeyCode::Right), code(KeyCode::End)] {
            assert_eq!(scroll(event, at(5, 0), PAGE, false), None, "{event:?}");
        }
    }

    /// How far `down` and `up` move a view that shows `shown` of 100
    /// lines from line 50, so that neither end gets in the way.
    fn steps(down: KeyEvent, up: KeyEvent, shown: usize, focused: bool) -> (usize, usize) {
        let page = Page {
            rows: Extent { shown, total: 100 },
            columns: Extent::default(),
        };
        let top = |event| {
            scroll(event, at(50, 0), page, focused)
                .expect("scrolls")
                .top
        };
        (top(down) - 50, 50 - top(up))
    }

    #[test]
    fn ctrl_u_and_ctrl_d_move_half_an_even_view() {
        assert_eq!(steps(ctrl('d'), ctrl('u'), 10, true), (5, 5));
    }

    #[test]
    fn half_an_odd_view_rounds_down() {
        assert_eq!(steps(ctrl('d'), ctrl('u'), 11, true), (5, 5));
    }

    #[test]
    fn half_a_view_of_one_row_is_one_line() {
        assert_eq!(steps(ctrl('d'), ctrl('u'), 1, true), (1, 1));
    }

    #[test]
    fn ctrl_u_and_ctrl_d_move_half_a_view_also_unfocused() {
        assert_eq!(steps(ctrl('d'), ctrl('u'), 10, false), (5, 5));
    }

    /// How far `pgdn` and `pgup` move a view that shows `shown` lines.
    fn page_steps(shown: usize, focused: bool) -> (usize, usize) {
        steps(
            code(KeyCode::PageDown),
            code(KeyCode::PageUp),
            shown,
            focused,
        )
    }

    #[test]
    fn pgup_and_pgdn_keep_two_lines_of_an_even_view() {
        assert_eq!(page_steps(10, true), (8, 8));
    }

    #[test]
    fn pgup_and_pgdn_keep_two_lines_of_an_odd_view() {
        assert_eq!(page_steps(11, true), (9, 9));
    }

    #[test]
    fn pgdn_moves_on_by_a_line_in_a_view_of_three_rows() {
        assert_eq!(page_steps(3, true), (1, 1));
    }

    #[test]
    fn pgdn_moves_on_by_a_line_in_a_view_of_two_rows() {
        assert_eq!(page_steps(2, true), (1, 1));
    }

    #[test]
    fn pgdn_moves_on_by_a_line_in_a_view_of_one_row() {
        assert_eq!(page_steps(1, true), (1, 1));
    }

    #[test]
    fn pgup_and_pgdn_keep_the_overlap_also_unfocused() {
        assert_eq!(page_steps(10, false), (8, 8));
    }

    #[test]
    fn ctrl_d_and_ctrl_u_stop_at_the_ends() {
        assert_eq!(
            (
                scroll(ctrl('d'), at(28, 8), PAGE, true),
                scroll(ctrl('u'), at(3, 8), PAGE, true)
            ),
            (Some(at(30, 8)), Some(at(0, 8)))
        );
    }

    #[test]
    fn other_keys_do_not_scroll() {
        for event in [
            key('x'),
            key('q'),
            code(KeyCode::Esc),
            code(KeyCode::Tab),
            ctrl('j'),
        ] {
            assert_eq!(scroll(event, at(5, 0), PAGE, true), None, "{event:?}");
        }
    }

    #[test]
    fn an_offset_past_the_end_comes_back_before_the_key_applies() {
        // A taller terminal had scrolled to line 35; now the last start is 30.
        assert_eq!(scroll(key('k'), at(35, 40), PAGE, true), Some(at(29, 30)));
    }

    #[test]
    fn a_page_brings_an_offset_back_within_it_in_both_directions() {
        assert_eq!(PAGE.clamp(at(5, 8)), at(5, 8), "inside");
        assert_eq!(PAGE.clamp(at(30, 30)), at(30, 30), "at the last start");
        assert_eq!(PAGE.clamp(at(35, 8)), at(30, 8), "below");
        assert_eq!(PAGE.clamp(at(5, 40)), at(5, 30), "to the right");
        assert_eq!(Page::default().clamp(at(3, 3)), at(0, 0), "nothing shown");
    }

    #[test]
    fn the_position_names_the_lines_shown_only_where_some_are_not() {
        assert_eq!(PAGE.rows.position(0).as_deref(), Some("1–10 of 40"));
        assert_eq!(PAGE.rows.position(30).as_deref(), Some("31–40 of 40"));
        let fits = Extent {
            shown: 10,
            total: 10,
        };
        assert_eq!(fits.position(0), None);
        assert_eq!(Extent::default().position(0), None);
    }

    #[test]
    fn a_view_that_fits_does_not_scroll() {
        let fits = Page {
            rows: Extent {
                shown: 10,
                total: 4,
            },
            columns: Extent::default(),
        };
        for event in [key('j'), key('G'), code(KeyCode::PageDown), key('l')] {
            assert_eq!(
                scroll(event, at(0, 0), fits, true),
                Some(at(0, 0)),
                "{event:?}"
            );
        }
    }
}
