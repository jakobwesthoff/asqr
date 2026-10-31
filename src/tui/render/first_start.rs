// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The first-start screen's frame (spec section 7.1, ADR 106): the queue
//! switcher's frame in the middle of the screen under a title line, with
//! the questions open, a button, and the settings file's path on the
//! bottom edge. The key bar is the start picker's.

use std::path::Path;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::keys::{first_start_hints, key_line_with_version};
use super::overlay::{overlay_block, popup};
use super::parts::POINTER;
use super::screen::shorten_from_left;
use super::source_line::columns;
use super::theme;
use super::wrap::wrap;
use crate::settings::Setting;
use crate::tui::{
    BUTTON, Cursor, FirstStart, LEAD, QUESTIONS, QUIT_LINE, absolute_with_home_as_tilde,
    default_choice, value_name, value_text,
};

/// The marker at the end of a question's default value.
const DEFAULT_MARKER: &str = "◀ default";

/// The frame's width unless the footer makes it wider or the screen
/// narrower.
const WIDTH: usize = 72;

/// The columns a frame takes besides what it holds: a border and a blank
/// on either side.
const FRAME_COLUMNS: usize = 4;

/// The columns the bottom edge takes besides the footer: the corners, a
/// blank on either side and at least one stroke of the edge.
const FOOTER_EDGE: usize = 5;

/// What the footer says before the path.
const WRITES: &str = "writes ";

/// The columns a value's description stands in from the frame's text: the
/// pointer and the mark `(•) ` of its row.
const DESCRIPTION_INDENT: usize = 6;

/// The blanks between a value's name and its default marker, at least.
const MARKER_GAP: usize = 2;

/// Draws the first-start screen `screen` over the whole of `frame`: asqr's
/// name in the title line, the frame in the middle and the keys, with
/// `version`, in the key bar. `path` is the settings file, with `home`
/// shown as `~`. Under `no_color` the screen draws without colours.
pub fn draw_first_start(
    frame: &mut Frame,
    screen: &FirstStart,
    path: Option<&Path>,
    home: Option<&Path>,
    version: &str,
    no_color: bool,
) {
    let area = frame.area();
    let [title, middle, _, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let buf = frame.buffer_mut();
    Line::from(Span::styled(" asqr", theme::app_name())).render(title, buf);
    let hints = first_start_hints(screen.cursor() == Cursor::Button);
    key_line_with_version(&hints, keys.width, version).render(keys, buf);
    FirstStartFrame {
        screen,
        path: path.map(|path| absolute_with_home_as_tilde(path, home)),
    }
    .render(middle, buf);
    if no_color {
        theme::strip_colours(buf);
    }
}

/// The frame of the first start, built from the state for one draw.
struct FirstStartFrame<'a> {
    screen: &'a FirstStart,
    /// The settings file as the footer shows it.
    path: Option<String>,
}

impl Widget for FirstStartFrame<'_> {
    /// The frame is as wide as [`WIDTH`], or as the footer needs, up to
    /// the screen, and a path that still does not fit loses its start, so
    /// the file's name stays. A frame taller than `area` shows the lines
    /// that keep the cursor's row and what belongs to it in view.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let most = usize::from(area.width.saturating_sub(2));
        let wanted = self
            .path
            .as_ref()
            .map_or(0, |path| columns(WRITES) + columns(path) + FOOTER_EDGE);
        let width = WIDTH.max(wanted).min(most);
        let inner = width.saturating_sub(FRAME_COLUMNS).max(1);
        let footer = self.path.as_ref().map(|path| {
            let room = width.saturating_sub(FOOTER_EDGE + columns(WRITES));
            format!("{WRITES}{}", shorten_from_left(path, room))
        });

        let Body { lines, blocks } = body_lines(self.screen, inner);
        let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
        let place = popup(area, u16::try_from(width).unwrap_or(u16::MAX), height, buf);
        let block = overlay_block("First start", footer.as_deref());
        let body = block.inner(place);
        block.render(place, buf);

        let (start, end) = blocks
            .iter()
            .find(|(cursor, _)| *cursor == self.screen.cursor())
            .map(|(_, lines)| *lines)
            .expect("the cursor stands on a row that has lines");
        let first = (end + 1)
            .saturating_sub(usize::from(body.height))
            .min(start);
        for (y, line) in (body.y..body.bottom()).zip(lines.into_iter().skip(first)) {
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
}

/// The lines of the frame's body, and for each row the cursor can stand on
/// the first and last line that belong to it: a value's own line with its
/// description, the button down to the last line.
struct Body {
    lines: Vec<Line<'static>>,
    blocks: Vec<(Cursor, (usize, usize))>,
}

/// The body of the frame at `inner` columns.
fn body_lines(screen: &FirstStart, inner: usize) -> Body {
    let mut lines = wrap(Line::from(LEAD), inner, 0, 0);
    let mut blocks = Vec::new();
    for (question, asked) in QUESTIONS.iter().enumerate() {
        lines.push(Line::default());
        lines.push(Line::styled(asked.text, theme::strong()));
        lines.extend(wrap(
            Line::styled(asked.why, theme::secondary()),
            inner,
            0,
            0,
        ));
        for value in 0..asked.setting.choices() {
            let cursor = Cursor::Value { question, value };
            let first = lines.len();
            lines.push(value_row(
                asked.setting,
                value,
                screen.picked(question) == value,
                screen.cursor() == cursor,
                inner,
            ));
            lines.extend(wrap(
                Line::styled(value_text(asked.setting, value), theme::secondary()),
                inner,
                DESCRIPTION_INDENT,
                DESCRIPTION_INDENT,
            ));
            blocks.push((cursor, (first, lines.len() - 1)));
        }
    }
    lines.push(Line::default());
    let button_at = lines.len();
    lines.push(button_row(screen.cursor() == Cursor::Button));
    lines.push(Line::default());
    lines.push(Line::styled(QUIT_LINE, theme::secondary()));
    blocks.push((Cursor::Button, (button_at, lines.len() - 1)));
    Body { lines, blocks }
}

/// The row of value number `value` of `setting`: the pointer where the
/// cursor stands, the mark of a picked value, its name, and `◀ default`
/// at the right end of the default's. The cursor's row is under the focus
/// fill up to the end of the name.
fn value_row(
    setting: Setting,
    value: usize,
    picked: bool,
    current: bool,
    inner: usize,
) -> Line<'static> {
    let name = value_name(setting, value);
    let mark = if picked { "(•) " } else { "( ) " };
    let mut head = vec![
        Span::raw(if current { POINTER } else { "  " }),
        Span::styled(
            mark,
            if picked {
                theme::chosen()
            } else {
                theme::strong()
            },
        ),
        Span::styled(name, theme::strong()),
    ];
    let used = columns(POINTER) + columns(mark) + columns(name);
    if current {
        head = head.into_iter().map(theme::focus_span).collect();
    }
    let room = inner.saturating_sub(used + columns(DEFAULT_MARKER));
    if value == default_choice(setting) && room >= MARKER_GAP {
        head.push(Span::raw(" ".repeat(room)));
        head.push(Span::styled(DEFAULT_MARKER, theme::default_marker()));
    }
    Line::from(head)
}

/// The button's row, with the pointer before it where the cursor stands.
fn button_row(current: bool) -> Line<'static> {
    let pointer = if current {
        Span::styled(POINTER, theme::focus_pointer())
    } else {
        Span::raw("  ")
    };
    Line::from(vec![pointer, Span::styled(BUTTON, theme::button())])
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{find, row, rows};
    use super::*;
    use crate::settings::Setting;

    const CONFIG: &str = "/home/me/Library/Application Support/asqr/settings.toml";

    fn pressed(codes: &[KeyCode]) -> FirstStart {
        let mut screen = FirstStart::default();
        for code in codes {
            screen.handle(KeyEvent::new(*code, KeyModifiers::NONE));
        }
        screen
    }

    /// The screen of the first start at `width`×`height` for a settings
    /// file at `path`.
    fn screen_of(
        screen: &FirstStart,
        path: &str,
        width: u16,
        height: u16,
        no_color: bool,
    ) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| {
                draw_first_start(
                    frame,
                    screen,
                    Some(&PathBuf::from(path)),
                    Some(Path::new("/home/me")),
                    "asqr 0.0.0",
                    no_color,
                );
            })
            .expect("draws");
        terminal.backend().clone()
    }

    fn drawn(screen: &FirstStart, width: u16, height: u16) -> TestBackend {
        screen_of(screen, CONFIG, width, height, false)
    }

    const NO_COPIES_AND_BUTTON: [KeyCode; 4] = [
        KeyCode::Down,
        KeyCode::Down,
        KeyCode::Char(' '),
        KeyCode::Down,
    ];

    #[test]
    fn the_first_start_at_the_sizes_of_a_terminal() {
        let first = FirstStart::default();
        let button = pressed(&NO_COPIES_AND_BUTTON);
        let no_copies = pressed(&[KeyCode::Down, KeyCode::Down]);

        insta::assert_snapshot!("first_start_80x24", drawn(&first, 80, 24));
        insta::assert_snapshot!("first_start_120x40", drawn(&first, 120, 40));
        insta::assert_snapshot!("first_start_button_80x24", drawn(&button, 80, 24));
        insta::assert_snapshot!("first_start_button_80x23", drawn(&button, 80, 23));
        insta::assert_snapshot!("first_start_no_copies_60x15", drawn(&no_copies, 60, 15));
        insta::assert_snapshot!("first_start_button_60x15", drawn(&button, 60, 15));
    }

    /// The rows of `backend` without the blanks after them.
    fn trimmed(backend: &TestBackend) -> Vec<String> {
        rows(backend.buffer())
            .iter()
            .map(|row| row.trim_end().to_owned())
            .collect()
    }

    /// The text inside the frame, one row after the other joined with
    /// blanks, so a text that wrapped reads as it was written.
    fn frame_text(backend: &TestBackend) -> String {
        rows(backend.buffer())
            .iter()
            .filter_map(|row| row.trim().strip_prefix('│'))
            .filter_map(|row| row.strip_suffix('│'))
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn every_text_is_the_one_agreed_word_for_word() {
        let text = frame_text(&drawn(&FirstStart::default(), 80, 24));

        for expected in [
            "Before your first session, one setting needs your answer. Press , later to change it.",
            "Keep copies of the files a session showed you?",
            "Files a session shows are often temporary. Replay can show them later only if a copy was kept.",
            "compact copies",
            "Compact copies: images are kept as WebP copies, about a fifth of their size; every other file is kept as it is.",
            "exact copies",
            "Exact copies: every file is kept byte for byte, images included.",
            "no copies",
            "No copies: no file is kept. asqr still records each file's size and checksum, so a replay can tell the file is gone.",
            "[ Save and start ]",
            "q quits without writing; the next start asks again.",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        assert!(text.contains("compact copies ◀ default"), "{text}");
        assert_eq!(text.matches("◀ default").count(), 1);
    }

    #[test]
    fn the_frame_has_a_title_a_footer_with_the_file_and_a_key_bar() {
        let backend = drawn(&FirstStart::default(), 80, 24);
        let rows = trimmed(&backend);

        assert_eq!(rows[0].trim(), "asqr");
        let top = rows.iter().find(|row| row.contains('╭')).expect("a frame");
        assert!(top.contains("╭ First start ─"), "{top}");
        let bottom = rows.iter().find(|row| row.contains('╰')).expect("a frame");
        assert!(
            bottom.ends_with(" writes ~/Library/Application Support/asqr/settings.toml ─╯")
                || bottom.contains(" writes ~/Library/Application Support/asqr/settings.toml "),
            "{bottom}"
        );
        assert!(
            rows[23].starts_with(" ↑/↓  move   enter  pick   q  quit"),
            "{}",
            rows[23]
        );
        assert!(rows[23].ends_with("asqr 0.0.0"), "{}", rows[23]);
    }

    #[test]
    fn the_key_bar_says_save_where_the_cursor_stands_on_the_button() {
        let backend = drawn(&pressed(&NO_COPIES_AND_BUTTON), 80, 24);

        let bar = row(backend.buffer(), 23);

        assert!(
            bar.starts_with(" ↑/↓  move   enter  save   q  quit"),
            "{bar}"
        );
    }

    #[test]
    fn one_row_has_the_focus_fill_and_the_picked_value_has_a_mark() {
        let backend = drawn(&pressed(&[KeyCode::Down]), 80, 24);
        let buf = backend.buffer();

        for text in ["❯", "( ) exact copies"] {
            assert!(
                buf[find(buf, text)].modifier.contains(Modifier::REVERSED),
                "{text}"
            );
        }
        for text in ["(•) compact copies", "no copies", "Compact copies:"] {
            assert!(
                !buf[find(buf, text)].modifier.contains(Modifier::REVERSED),
                "{text}"
            );
        }
        assert!(
            buf[find(buf, "(•)")].modifier.contains(Modifier::BOLD),
            "the picked mark stands out"
        );
    }

    #[test]
    fn the_why_and_the_descriptions_are_dim_and_the_question_is_bold() {
        let backend = drawn(&FirstStart::default(), 80, 24);
        let buf = backend.buffer();

        for text in [
            "Files a session shows",
            "Compact copies:",
            "No copies:",
            "q quits without writing",
        ] {
            assert!(
                buf[find(buf, text)].modifier.contains(Modifier::DIM),
                "{text}"
            );
        }
        assert!(
            buf[find(buf, "Keep copies")]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            !buf[find(buf, "Before your first")]
                .modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn the_button_is_a_fill_of_its_own_and_the_pointer_marks_it_under_the_cursor() {
        let at_rest = drawn(&FirstStart::default(), 80, 24);
        let button = &at_rest.buffer()[find(at_rest.buffer(), BUTTON)];
        assert!(
            button
                .modifier
                .contains(Modifier::REVERSED | Modifier::BOLD)
        );
        let y = find(at_rest.buffer(), BUTTON).1;
        assert!(!row(at_rest.buffer(), y).contains("❯ [ Save"));

        let on_it = drawn(&pressed(&NO_COPIES_AND_BUTTON), 80, 24);
        let y = find(on_it.buffer(), BUTTON).1;
        assert!(row(on_it.buffer(), y).contains("❯ [ Save and start ]"));
    }

    #[test]
    fn a_footer_too_long_for_the_frame_widens_it_and_then_loses_its_start() {
        let long = format!("/home/me/{}settings.toml", "folder/".repeat(12));
        let screen = FirstStart::default();

        let wide = screen_of(&screen, &long, 140, 30, false);
        let wide_rows = trimmed(&wide);
        let top = wide_rows
            .iter()
            .find(|row| row.contains('╭'))
            .expect("frame");
        let edge = wide_rows
            .iter()
            .find(|row| row.contains('╰'))
            .expect("frame");
        assert!(edge.contains("writes ~/folder/folder/"), "{edge}");
        assert!(edge.contains("settings.toml ╯"), "{edge}");
        assert!(top.trim().chars().count() > 72, "the frame widened: {top}");

        let narrow = screen_of(&screen, &long, 60, 20, false);
        let narrow_rows = trimmed(&narrow);
        let edge = narrow_rows
            .iter()
            .find(|row| row.contains('╰'))
            .expect("frame");
        assert!(edge.contains("writes …"), "{edge}");
        assert!(edge.ends_with("settings.toml ╯"), "{edge}");
        let top = narrow_rows
            .iter()
            .find(|row| row.contains('╭'))
            .expect("frame");
        assert!(top.trim().chars().count() <= 58, "{top}");
    }

    #[test]
    fn a_short_screen_keeps_the_value_under_the_cursor_with_its_description() {
        let no_copies = frame_text(&drawn(&pressed(&[KeyCode::Down, KeyCode::Down]), 60, 15));
        assert!(
            no_copies.contains("size and checksum, so a replay can tell the file is gone."),
            "{no_copies}"
        );
        assert!(no_copies.contains("no copies"), "{no_copies}");

        let first = frame_text(&drawn(&FirstStart::default(), 60, 15));
        assert!(
            first.contains("of their size; every other file is kept as it is."),
            "{first}"
        );
    }

    #[test]
    fn a_short_screen_keeps_the_button_and_the_line_about_quitting_in_view() {
        for (width, height) in [(60, 15), (80, 23), (80, 12)] {
            let text = frame_text(&drawn(&pressed(&NO_COPIES_AND_BUTTON), width, height));

            assert!(
                text.contains("[ Save and start ]"),
                "{width}x{height} {text}"
            );
            assert!(
                text.contains("q quits without writing; the next start asks again."),
                "{width}x{height} {text}"
            );
        }
    }

    #[test]
    fn without_colours_the_screen_keeps_its_marks_and_its_fill() {
        let backend = screen_of(&FirstStart::default(), CONFIG, 80, 24, true);
        let buf = backend.buffer();

        for cell in &buf.content {
            assert_eq!(cell.fg, Color::Reset, "{}", cell.symbol());
            assert_eq!(cell.bg, Color::Reset, "{}", cell.symbol());
        }
        for text in ["❯", "compact copies"] {
            assert!(
                buf[find(buf, text)].modifier.contains(Modifier::REVERSED),
                "{text}"
            );
        }
        assert!(find(buf, "(•)") != find(buf, "( )"));
    }

    #[test]
    fn without_a_place_for_the_file_the_edge_names_none() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        terminal
            .draw(|frame| {
                draw_first_start(
                    frame,
                    &FirstStart::default(),
                    None,
                    None,
                    "asqr 0.0.0",
                    false,
                );
            })
            .expect("draws");

        let rows = trimmed(terminal.backend());
        let edge = rows.iter().find(|row| row.contains('╰')).expect("frame");
        assert!(
            edge.contains("╰──") && edge.ends_with("──╯") && !edge.contains("writes"),
            "{edge}"
        );
    }

    #[test]
    fn the_questions_are_the_settings_the_screen_asks() {
        assert_eq!(QUESTIONS.len(), 1);
        assert_eq!(QUESTIONS[0].setting, Setting::KeepFiles);
    }
}
