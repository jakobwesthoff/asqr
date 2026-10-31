// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The settings pane's frame (spec section 7.6, ADR 92): one row per
//! setting, a dotted rule, and below it what the setting under the cursor
//! does and what its value means, in the queue switcher's frame with the
//! settings file's path on the bottom edge.

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::overlay::{overlay_block, popup};
use super::parts::POINTER;
use super::screen::shorten_from_left;
use super::source_line::columns;
use super::theme;
use super::wrap::wrap;
use crate::settings::{Setting, SettingsStore};
use crate::tui::{
    SettingsPane, about, absolute_with_home_as_tilde, label, lock_label, lock_text, meaning,
    value_name,
};

/// The frame's width unless a path makes it wider or the screen narrower.
const WIDTH: usize = 72;

/// The columns of the longest setting name, which every value stands after.
const LABEL_COLUMNS: usize = 18;

/// The blanks between a row's name and its value.
const GAP: &str = "  ";

/// The columns a frame takes besides what it holds: a border and a blank
/// on either side.
const FRAME_COLUMNS: usize = 4;

/// The columns the bottom edge takes besides the path: the corners, a
/// blank on either side and at least one stroke of the edge.
const FOOTER_EDGE: usize = 5;

/// Draws the pane in the middle of `area`. `home` shows as `~` in the
/// settings file's path.
///
/// The frame is as tall as the tallest footer any row can show in this
/// run, so it keeps its height as the cursor moves. It grows to hold the
/// path whole, up to the screen; a path that still does not fit loses its
/// start, so the file's name stays.
pub(super) fn draw_settings(
    area: Rect,
    buf: &mut Buffer,
    pane: &SettingsPane,
    store: &SettingsStore,
    home: Option<&Path>,
) {
    let path = store
        .path()
        .map(|path| absolute_with_home_as_tilde(path, home));
    let most = usize::from(area.width.saturating_sub(2));
    let wanted = path.as_ref().map_or(0, |path| columns(path) + FOOTER_EDGE);
    let width = WIDTH.max(wanted).min(most);
    let inner = width.saturating_sub(FRAME_COLUMNS).max(1);
    let path = path.map(|path| shorten_from_left(&path, width.saturating_sub(FOOTER_EDGE)));

    let mut lines: Vec<Line<'static>> = Setting::ALL
        .iter()
        .enumerate()
        .map(|(at, setting)| row(*setting, at == pane.cursor(), store, inner))
        .collect();
    lines.push(Line::styled("┄".repeat(inner), theme::secondary()));
    lines.extend(footer(pane.setting(), store, inner));

    let rows = Setting::ALL.len() + 1 + tallest_footer(store, inner);
    let height = u16::try_from(rows + 2).unwrap_or(u16::MAX);
    let place = popup(area, u16::try_from(width).unwrap_or(u16::MAX), height, buf);
    let block = overlay_block("Settings", path.as_deref());
    let body = block.inner(place);
    block.render(place, buf);
    for (y, line) in (body.y..body.bottom()).zip(lines) {
        line.render(
            Rect {
                y,
                height: 1,
                ..body
            },
            buf,
        );
    }
}

/// The row of `setting`: its name and its value, with `‹ ›` round the
/// value under the cursor, which `←` and `→` change. The value a flag or a
/// variable holds shows dim, with what sets it beside it, and the cursor's
/// fill ends with the name. Where `inner` columns leave no room for three
/// blanks before what sets a held value, it gets two, then one.
fn row(setting: Setting, current: bool, store: &SettingsStore, inner: usize) -> Line<'static> {
    let value = value_name(setting, store.choice(setting));
    let head = vec![
        Span::raw(if current { POINTER } else { "  " }),
        Span::raw(format!("{:<LABEL_COLUMNS$}{GAP}", label(setting))),
    ];
    let head: Vec<Span<'static>> = if current {
        head.into_iter().map(theme::focus_span).collect()
    } else {
        head
    };
    let tail = match store.lock(setting) {
        Some(lock) => {
            let label = lock_label(lock);
            let used = columns(POINTER) + LABEL_COLUMNS + columns(GAP) + columns(value);
            let blanks = inner.saturating_sub(used + columns(&label)).clamp(1, 3);
            vec![
                Span::styled(value, theme::secondary()),
                Span::styled(format!("{}{label}", " ".repeat(blanks)), theme::secondary()),
            ]
        }
        None if current => vec![theme::focus_span(Span::styled(
            format!("‹ {value} ›"),
            theme::strong(),
        ))],
        None => vec![Span::styled(value, theme::strong())],
    };
    Line::from([head, tail].concat())
}

/// What the footer says of `setting` as the cursor stands on it: what it
/// does and what its value means, or for a row a flag or a variable holds,
/// what holds it and what to do.
fn footer(setting: Setting, store: &SettingsStore, inner: usize) -> Vec<Line<'static>> {
    let paragraph = |text: String| wrap(Line::from(text), inner, 0, 0);
    if let Some(lock) = store.lock(setting) {
        return paragraph(lock_text(lock, store.effective().context));
    }
    let mut lines = paragraph(about(setting).to_owned());
    lines.push(Line::default());
    lines.extend(paragraph(meaning(setting, store.choice(setting))));
    lines
}

/// The rows of the tallest footer any row of this run can show, each value
/// of an open setting counted, and a held one by its one text.
fn tallest_footer(store: &SettingsStore, inner: usize) -> usize {
    Setting::ALL
        .iter()
        .flat_map(|setting| {
            if store.lock(*setting).is_some() {
                vec![footer(*setting, store, inner).len()]
            } else {
                (0..setting.choices())
                    .map(|choice| {
                        let mut stepped = store.clone();
                        while stepped.choice(*setting) != choice {
                            stepped.step(*setting, crate::settings::Direction::Next);
                        }
                        footer(*setting, &stepped, inner).len()
                    })
                    .collect()
            }
        })
        .max()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;

    use super::super::panes::PaneLayout;
    use super::super::test_support::{find, row, rows};
    use super::*;
    use crate::settings::Lock;

    const CONFIG: &str = "/home/me/Library/Application Support/asqr/settings.toml";

    fn store(lock: Option<Lock>) -> SettingsStore {
        let lock = lock.map(|lock| (PaneLayout::Below, lock));
        SettingsStore::load(Some(PathBuf::from(CONFIG)), lock).store
    }

    fn pane_at(rows_down: usize) -> SettingsPane {
        let mut pane = SettingsPane::default();
        for _ in 0..rows_down {
            pane.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), false);
        }
        pane
    }

    fn drawn_in(width: u16, height: u16, pane: &SettingsPane, store: &SettingsStore) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        draw_settings(area, &mut buf, pane, store, Some(Path::new("/home/me")));
        buf
    }

    fn drawn(rows_down: usize, store: &SettingsStore) -> Buffer {
        drawn_in(80, 24, &pane_at(rows_down), store)
    }

    /// The rows of the frame, without the blanks around it.
    fn frame(buf: &Buffer) -> Vec<String> {
        rows(buf)
            .into_iter()
            .map(|row| row.trim().to_owned())
            .filter(|row| !row.is_empty())
            .collect()
    }

    #[test]
    fn the_rows_name_each_setting_and_the_cursor_row_shows_arrows_around_its_value() {
        let frame = frame(&drawn(0, &store(None)));

        assert!(frame[0].starts_with("╭ Settings ─"), "{frame:#?}");
        assert_eq!(
            frame[1].trim_end_matches(['│', ' ']),
            "│ ❯ Diffs open as       ‹ one column ›"
        );
        assert_eq!(
            frame[2].trim_end_matches(['│', ' ']),
            "│   Max question width  100 columns"
        );
        assert_eq!(
            frame[3].trim_end_matches(['│', ' ']),
            "│   Context             beside or below"
        );
    }

    #[test]
    fn a_dotted_rule_and_the_footer_for_the_row_under_the_cursor_follow() {
        let frame = frame(&drawn(0, &store(None)));

        assert!(frame[4].starts_with("│ ┄┄┄"), "{frame:#?}");
        assert!(frame[4].trim_end_matches(['│', ' ']).ends_with('┄'));
        let footer = frame[5..frame.len() - 1].join("\n");
        assert!(
            footer.contains("How a diff shows when you come to it."),
            "{footer}"
        );
        assert!(
            footer.contains("One column: removed and added lines one below the other. (default)"),
            "{footer}"
        );

        let moved = self::frame(&drawn(1, &store(None))).join("\n");
        assert!(
            moved.contains("The widest a question's text may get;"),
            "{moved}"
        );
        assert!(moved.contains("100 columns: lines wrap after at most 100 characters. (default)"));
        assert!(
            moved.contains("│   Max question width  ‹ 100 columns ›")
                || moved.contains("❯ Max question width")
        );
    }

    #[test]
    fn a_locked_row_shows_its_value_dimmed_with_what_sets_it_and_no_arrows() {
        let locked = store(Some(Lock::Variable("ASQR_LAYOUT")));
        let buf = drawn(2, &locked);
        let frame = frame(&buf);

        let row = &frame[3];
        assert!(row.contains("❯ Context"), "{row}");
        assert!(row.contains("always below   set by ASQR_LAYOUT"), "{row}");
        assert!(!row.contains('‹') && !row.contains('›'), "{row}");
        let footer = frame[5..frame.len() - 1].join(" ");
        assert!(
            footer.contains("Your shell sets ASQR_LAYOUT=below"),
            "{footer}"
        );
        for text in ["always below", "set by ASQR_LAYOUT"] {
            assert!(
                buf[find(&buf, text)].modifier.contains(Modifier::DIM),
                "{text}"
            );
        }
        assert!(
            !buf[find(&buf, "always below")]
                .modifier
                .contains(Modifier::REVERSED),
            "the fill ends with the label"
        );
        assert!(
            buf[find(&buf, "Context")]
                .modifier
                .contains(Modifier::REVERSED)
        );

        let flagged = frame_of_flag();
        assert!(flagged.contains("set by --layout"), "{flagged}");
        assert!(
            flagged.contains("You started asqr with --layout below"),
            "{flagged}"
        );
    }

    fn frame_of_flag() -> String {
        frame(&drawn(2, &store(Some(Lock::Flag("--layout"))))).join(" ")
    }

    #[test]
    fn the_cursor_row_is_the_one_row_under_the_focus_fill_and_other_values_are_bold() {
        let buf = drawn(0, &store(None));

        for text in ["❯", "Diffs open as", "‹ one column ›"] {
            assert!(
                buf[find(&buf, text)].modifier.contains(Modifier::REVERSED),
                "{text}"
            );
        }
        assert!(
            !buf[find(&buf, "Max question width")]
                .modifier
                .contains(Modifier::REVERSED)
        );
        let value = &buf[find(&buf, "100 columns")];
        assert!(value.modifier.contains(Modifier::BOLD));
        assert!(!value.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn the_frame_keeps_its_height_wherever_the_cursor_stands() {
        for lock in [
            None,
            Some(Lock::Variable("ASQR_LAYOUT")),
            Some(Lock::Flag("--layout")),
        ] {
            let store = store(lock);
            let heights: Vec<_> = (0..3)
                .map(|down| frame(&drawn(down, &store)).len())
                .collect();
            assert_eq!(heights[0], heights[1], "{lock:?}");
            assert_eq!(heights[1], heights[2], "{lock:?}");
        }
        let unlocked = frame(&drawn(0, &store(None))).len();
        let locked = frame(&drawn(0, &store(Some(Lock::Variable("ASQR_LAYOUT"))))).len();
        assert!(
            unlocked <= locked,
            "a frame with no lock is no taller than it needs"
        );
    }

    #[test]
    fn the_settings_file_stands_on_the_bottom_edge_with_the_home_folder_as_a_tilde() {
        let frame = frame(&drawn(0, &store(None)));

        let edge = frame.last().expect("a bottom edge");
        assert!(edge.starts_with("╰─"), "{edge}");
        assert!(
            edge.ends_with(" ~/Library/Application Support/asqr/settings.toml ╯"),
            "{edge}"
        );
    }

    #[test]
    fn without_a_place_for_the_file_the_edge_names_none() {
        let frame = frame(&drawn(0, &SettingsStore::default()));

        let edge = frame.last().expect("a bottom edge");
        assert!(edge.starts_with("╰──") && edge.ends_with("──╯"), "{edge}");
    }

    #[test]
    fn a_path_too_long_for_the_frame_widens_it_and_then_loses_its_start() {
        let long = PathBuf::from(format!("/home/me/{}settings.toml", "folder/".repeat(12)));
        let long_store = SettingsStore::load(Some(long), None).store;

        let wide = frame(&drawn_in(120, 30, &pane_at(0), &long_store));
        assert!(wide.last().expect("edge").contains("folder/settings.toml"));
        assert!(
            wide[0].chars().count() > 72,
            "the frame widened: {}",
            wide[0]
        );

        let narrow = frame(&drawn_in(60, 20, &pane_at(0), &long_store));
        let edge = narrow.last().expect("edge");
        assert!(
            edge.contains("…") && edge.ends_with("settings.toml ╯"),
            "{edge}"
        );
        assert!(narrow[0].chars().count() <= 58, "{}", narrow[0]);
    }

    #[test]
    fn a_locked_row_keeps_what_sets_it_whole_where_the_screen_is_narrow() {
        for variable in [true, false] {
            let lock = if variable {
                Lock::Variable("ASQR_LAYOUT")
            } else {
                Lock::Flag("--layout")
            };
            let buf = drawn_in(60, 15, &pane_at(2), &store(Some(lock)));

            let frame = frame(&buf);
            let row = &frame[3];
            let sets = if variable {
                "set by ASQR_LAYOUT"
            } else {
                "set by --layout"
            };
            assert!(
                row.contains(&format!("always below {sets}"))
                    || row.contains(&format!("always below  {sets}"))
                    || row.contains(&format!("always below   {sets}")),
                "{row}"
            );
            assert!(row.ends_with('│'), "{row}");
        }
    }

    #[test]
    fn the_frame_is_centred_in_the_area_it_gets() {
        let buf = drawn_in(100, 30, &pane_at(0), &store(None));

        let first = rows(&buf)
            .into_iter()
            .position(|row| row.contains("╭ Settings"))
            .expect("the frame is drawn");
        let left = row(&buf, u16::try_from(first).expect("a row"))
            .find('╭')
            .expect("a corner");
        assert_eq!(left, (100 - 72) / 2);
    }

    #[test]
    fn a_small_area_gets_the_frame_it_has_room_for() {
        let buf = drawn_in(
            60,
            13,
            &pane_at(0),
            &store(Some(Lock::Variable("ASQR_LAYOUT"))),
        );

        let frame = frame(&buf);
        assert!(frame[0].starts_with("╭ Settings"), "{frame:#?}");
        assert!(frame.len() <= 13);
    }
}
