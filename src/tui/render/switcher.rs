// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The queue switcher's frame (spec section 7.6, ADR 86): one row per
//! queue this asqr can switch to, the row for a new queue, and below a
//! blank line the queues other asqr instances watch, dim. The footer on
//! the frame's bottom edge says what `enter` does to the agents.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::keys::{key_line_with_version, picker_hints};
use super::overlay::{overlay_block, popup};
use super::parts::POINTER;
use super::problem;
use super::screen::shorten_from_left;
use super::source_line::columns;
use super::theme;
use crate::queue::DEFAULT_QUEUE;
use crate::tui::{Notice, Place, QueueRow, Switcher};

/// The marker at the end of the queue this asqr watches.
const THIS_ASQR: &str = "◀ this asqr";

/// The head of the new-queue row, and what stands for an empty name.
const NEW_QUEUE: &str = "✎ New queue: ";
const NO_NAME: &str = "—";

/// The blanks between the columns of a row.
const GAP: &str = "   ";

/// The columns a frame takes besides what it holds: a border and a blank
/// on either side.
const FRAME_COLUMNS: usize = 4;

/// The columns the bottom edge takes besides the footer: the corners, a
/// blank on either side and at least one stroke of the edge.
const FOOTER_EDGE: usize = 5;

/// Draws the start picker `picker` over the whole of `frame`: asqr's name
/// in the title line, the queues in the middle, `notice` in the status
/// line and the picker's keys with `version` in the key bar.
pub fn draw_picker(frame: &mut Frame, picker: &Switcher, notice: Option<&Notice>, version: &str) {
    let area = frame.area();
    let [title, _, status, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let buf = frame.buffer_mut();
    Line::from(Span::styled(" asqr", theme::app_name())).render(title, buf);
    if let Some(notice) = notice {
        problem::status_line(notice, status.width).render(status, buf);
    }
    key_line_with_version(&picker_hints(picker.typing()), keys.width, version).render(keys, buf);
    draw_switcher(area, buf, picker);
}

/// Draws `switcher` in a frame in the middle of `area`.
pub(super) fn draw_switcher(area: Rect, buf: &mut Buffer, switcher: &Switcher) {
    let most = usize::from(area.width.saturating_sub(4));
    let width = frame_width(switcher, most);
    let inner = width.saturating_sub(FRAME_COLUMNS);
    let lines = lines(switcher, inner);
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let place = popup(area, u16::try_from(width).unwrap_or(u16::MAX), height, buf);
    let footer = footer(switcher, switcher.cursor(), width);
    let block = overlay_block("Queues", footer.as_deref());
    let rows = block.inner(place);
    block.render(place, buf);
    // A list taller than the frame starts where the cursor's row shows.
    let first = (switcher.cursor() + 1).saturating_sub(usize::from(rows.height));
    for (y, line) in (rows.y..rows.bottom()).zip(lines.into_iter().skip(first)) {
        line.render(
            Rect {
                y,
                height: 1,
                ..rows
            },
            buf,
        );
    }
}

/// The frame's width for `switcher`, at most `most` columns: as wide as
/// its rows and as the widest footer any row the cursor reaches gives, so
/// the frame keeps its width as the cursor moves.
fn frame_width(switcher: &Switcher, most: usize) -> usize {
    let choice = switcher.choice();
    let layout = Columns::of(switcher, usize::MAX);
    let rows = choice
        .rows
        .iter()
        .map(|row| layout.row_width(row))
        .chain(
            choice
                .taken
                .iter()
                .map(|taken| layout.taken_width(&taken_text(taken.session.as_deref()))),
        )
        .chain([2 + columns(NEW_QUEUE) + columns(switcher.name()).max(1) + 1])
        .max()
        .unwrap_or_default()
        + FRAME_COLUMNS;
    let reached = choice.rows.len() + usize::from(choice.root.is_some());
    let footers = (0..reached)
        .filter_map(|at| footer(switcher, at, most))
        .map(|footer| columns(&footer) + FOOTER_EDGE)
        .max()
        .unwrap_or_default();
    rows.max(footers).min(most)
}

/// Says on the frame's bottom edge what `enter` on row `at` does to the
/// agents, in a frame at most `width` columns wide, where that changes
/// with the row: inside tmux the agents of the session follow a named
/// queue and stay on their queue for a `--dir` one. Outside tmux they reach
/// a queue other than `default` only when told it. A path or a new name
/// is never shortened there, as an agent cannot use a shortened one;
/// where it does not fit, the footer points to the row, which shows as
/// much of it as fits.
fn footer(switcher: &Switcher, at: usize, width: usize) -> Option<String> {
    let choice = switcher.choice();
    let row = choice.rows.get(at);
    let dir = row.is_some_and(|row| row.location.name().is_none());
    let room = width.saturating_sub(FOOTER_EDGE);
    let fitting = |text: String| (columns(&text) <= room).then_some(text);
    let tell_name = |name: &str| {
        (name != DEFAULT_QUEUE).then(|| {
            fitting(format!("tell your agents: --queue {name}"))
                .unwrap_or_else(|| "tell your agents --queue with this name".to_owned())
        })
    };
    match &choice.place {
        Place::Tmux(session) if dir => Some(format!(
            "agents in tmux session {session} follow only a named queue"
        )),
        Place::Tmux(session) => Some(format!("agents in tmux session {session} follow this asqr")),
        Place::Outside => match row {
            Some(row) if dir => fitting(format!(
                "tell your agents: --dir {}",
                row.agent_dir.as_deref().unwrap_or(&row.label)
            ))
            .or_else(|| Some("tell your agents --dir with this path".to_owned())),
            Some(row) => tell_name(&row.label),
            None if switcher.name().is_empty() => None,
            None => tell_name(switcher.name()),
        },
    }
}

/// What a taken row says about the asqr that watches it.
fn taken_text(session: Option<&str>) -> String {
    match session {
        Some(session) => format!("watched in tmux {session}"),
        None => "watched by another asqr".to_owned(),
    }
}

/// The widths of the columns the rows line up in: the queue's name, the
/// waiting sessions where any wait, and the marker of the watched queue.
/// A `--dir` queue's path takes the room its row has, as long as a path
/// can be: the names keep their column below it.
struct Columns {
    name: usize,
    waiting: usize,
    /// The columns a row has besides its name.
    inner: usize,
}

impl Columns {
    /// The columns of `switcher`'s rows, with the name column cut so a row
    /// fits `inner` columns.
    fn of(switcher: &Switcher, inner: usize) -> Self {
        let choice = switcher.choice();
        let waiting = choice
            .rows
            .iter()
            .map(|row| columns(&waiting_text(row.waiting)))
            .max()
            .unwrap_or_default();
        let named = |row: &&QueueRow| row.location.name().is_some();
        let longest = choice
            .rows
            .iter()
            .filter(named)
            .map(|row| columns(&row.label))
            .chain(choice.taken.iter().map(|taken| columns(&taken.label)))
            .max()
            .unwrap_or_default();
        let waiting_part = if waiting > 0 { GAP.len() + waiting } else { 0 };
        // The start picker has no watched queue, and no marker to leave
        // room for.
        let marker = if choice.rows.iter().any(|row| row.watched) {
            GAP.len() + columns(THIS_ASQR)
        } else {
            0
        };
        let others = (marker + waiting_part).max(
            choice
                .taken
                .iter()
                .map(|taken| GAP.len() + columns(&taken_text(taken.session.as_deref())))
                .max()
                .unwrap_or_default(),
        );
        let room = inner.saturating_sub(2 + others).max(1);
        Columns {
            name: longest.min(room),
            waiting,
            inner,
        }
    }

    /// What `row` shows after its name.
    fn after_name(&self, row: &QueueRow) -> usize {
        let waiting = if self.waiting > 0 {
            GAP.len() + self.waiting
        } else {
            0
        };
        let marker = if row.watched {
            GAP.len() + columns(THIS_ASQR)
        } else {
            0
        };
        waiting + marker
    }

    /// The columns `row`'s name takes: the name column, or for a path as
    /// much of the row as it needs and has.
    fn name_width(&self, row: &QueueRow) -> usize {
        if row.location.name().is_some() {
            return self.name;
        }
        let room = self.inner.saturating_sub(2 + self.after_name(row)).max(1);
        columns(&row.label).min(room).max(self.name)
    }

    fn row_width(&self, row: &QueueRow) -> usize {
        2 + self.name_width(row) + self.after_name(row)
    }

    fn taken_width(&self, text: &str) -> usize {
        2 + self.name + GAP.len() + columns(text)
    }
}

/// `N waiting`, or nothing for none.
fn waiting_text(waiting: usize) -> String {
    if waiting > 0 {
        format!("{waiting} waiting")
    } else {
        String::new()
    }
}

/// `text` padded with blanks to `width` columns.
fn padded(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(columns(text))))
}

/// The frame's rows, `inner` columns wide.
fn lines(switcher: &Switcher, inner: usize) -> Vec<Line<'static>> {
    let choice = switcher.choice();
    let layout = Columns::of(switcher, inner);
    let mut lines = Vec::new();
    for (at, row) in choice.rows.iter().enumerate() {
        let current = at == switcher.cursor();
        let width = layout.name_width(row);
        let name = shorten_from_left(&row.label, width);
        let mut text = format!(
            "{}{}",
            if current { POINTER } else { "  " },
            padded(&name, width)
        );
        if layout.waiting > 0 {
            text.push_str(GAP);
            text.push_str(&padded(&waiting_text(row.waiting), layout.waiting));
        }
        let mut spans = vec![Span::raw(text.clone())];
        if row.watched {
            let blank = inner.saturating_sub(columns(&text) + columns(THIS_ASQR));
            spans.push(Span::raw(" ".repeat(blank.max(GAP.len()))));
            spans.push(Span::styled(THIS_ASQR, theme::default_marker()));
        }
        let line = Line::from(spans);
        lines.push(if current {
            theme::focus_row(line)
        } else {
            line
        });
    }
    if choice.root.is_some() {
        lines.push(name_row(switcher, inner));
    }
    if !choice.taken.is_empty() {
        lines.push(Line::default());
    }
    for taken in &choice.taken {
        let name = shorten_from_left(&taken.label, layout.name);
        lines.push(Line::styled(
            format!(
                "  {}{GAP}{}",
                padded(&name, layout.name),
                taken_text(taken.session.as_deref())
            ),
            theme::secondary(),
        ));
    }
    lines
}

/// The row for a new queue: the own-answer idiom of a question, a field
/// while it has the keys, with the name cut from its start where it is
/// longer than the row.
fn name_row(switcher: &Switcher, inner: usize) -> Line<'static> {
    let current = switcher.on_name_row();
    let name = switcher.name();
    if switcher.typing() {
        let room = inner.saturating_sub(2 + columns(NEW_QUEUE) + 1);
        return Line::from(vec![
            Span::styled(POINTER, theme::focus_pointer()),
            Span::raw(format!("{NEW_QUEUE}{}", shorten_from_left(name, room))),
            Span::styled(" ", theme::cursor()),
        ]);
    }
    let room = inner.saturating_sub(2 + columns(NEW_QUEUE));
    let shown = if name.is_empty() {
        Span::styled(NO_NAME, theme::secondary())
    } else {
        Span::raw(shorten_from_left(name, room))
    };
    let line = Line::from(vec![
        Span::raw(format!(
            "{}{NEW_QUEUE}",
            if current { POINTER } else { "  " }
        )),
        shown,
    ]);
    if current {
        theme::focus_row(line)
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{find, look_at, row, rows};
    use super::*;
    use crate::queue::QueueLocation;
    use crate::tui::{QueueChoice, TakenQueue};

    const ROOT: &str = "/data/asqr";

    fn named(name: &str, waiting: usize, watched: bool) -> QueueRow {
        QueueRow {
            location: QueueLocation::named(Path::new(ROOT), name),
            label: name.to_owned(),
            agent_dir: None,
            waiting,
            watched,
        }
    }

    /// The queues of the mockups: `asqr-todo`, `asqr-todos` watched with
    /// two sessions, `default` with one, and `ntropy` taken.
    fn choice(place: Place) -> QueueChoice {
        QueueChoice {
            rows: vec![
                named("asqr-todo", 0, false),
                named("asqr-todos", 2, true),
                named("default", 1, false),
            ],
            taken: vec![TakenQueue {
                label: "ntropy".into(),
                session: Some("ntropy".into()),
            }],
            root: Some(PathBuf::from(ROOT)),
            place,
        }
    }

    fn inside_tmux() -> Switcher {
        Switcher::new(choice(Place::Tmux("asqr".into())))
    }

    fn outside_tmux() -> Switcher {
        Switcher::new(choice(Place::Outside))
    }

    fn press(switcher: &mut Switcher, code: KeyCode) {
        switcher.handle(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn drawn(switcher: &Switcher) -> Buffer {
        let area = Rect::new(0, 0, 80, 22);
        let mut buf = Buffer::empty(area);
        draw_switcher(area, &mut buf, switcher);
        buf
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
    fn inside_tmux_the_frame_shows_every_queue_and_says_the_agents_follow() {
        assert_eq!(
            frame(&drawn(&inside_tmux())),
            [
                "╭ Queues ───────────────────────────────────────╮",
                "│   asqr-todo                                   │",
                "│ ❯ asqr-todos   2 waiting          ◀ this asqr │",
                "│   default      1 waiting                      │",
                "│   ✎ New queue: —                              │",
                "│                                               │",
                "│   ntropy       watched in tmux ntropy         │",
                "╰─ agents in tmux session asqr follow this asqr ╯",
            ]
        );
    }

    #[test]
    fn the_taken_queues_stand_apart_below_a_blank_row() {
        let buf = drawn(&inside_tmux());
        let (_, new_queue) = find(&buf, "✎ New queue");
        let (_, taken) = find(&buf, "ntropy");

        assert_eq!(taken, new_queue + 2);
        assert_eq!(
            row(&buf, new_queue + 1).trim(),
            "│                                               │"
        );
    }

    #[test]
    fn a_taken_queue_without_a_tmux_session_is_watched_by_another_asqr() {
        let mut choice = choice(Place::Outside);
        choice.taken[0].session = None;

        let buf = drawn(&Switcher::new(choice));

        find(&buf, "ntropy       watched by another asqr");
    }

    #[test]
    fn without_taken_queues_there_is_no_blank_row() {
        let mut choice = choice(Place::Outside);
        choice.taken.clear();

        let shown = frame(&drawn(&Switcher::new(choice)));

        assert_eq!(shown.len(), 6, "{shown:#?}");
        assert!(shown[4].contains("✎ New queue"), "{shown:#?}");
    }

    #[test]
    fn the_cursor_row_has_the_fill_and_the_watched_queue_the_marker() {
        let mut switcher = inside_tmux();
        press(&mut switcher, KeyCode::Down);
        let buf = drawn(&switcher);

        assert_eq!(look_at(&buf, "◀ this asqr"), (Color::Cyan, Modifier::BOLD));
        let (x, y) = find(&buf, "❯ default");
        assert!(theme::has_focus_fill(&buf[(x, y)]));
        assert!(
            theme::has_focus_fill(&buf[(x + 44, y)]),
            "the fill runs to the frame's blank column"
        );
        assert!(!theme::has_focus_fill(&buf[(x + 45, y)]));
    }

    #[test]
    fn a_taken_queue_is_dim_and_the_empty_name_a_dim_dash() {
        let buf = drawn(&inside_tmux());

        assert_eq!(look_at(&buf, "ntropy").1, Modifier::DIM);
        assert_eq!(look_at(&buf, "watched in tmux").1, Modifier::DIM);
        assert_eq!(look_at(&buf, "—").1, Modifier::DIM);
        assert_eq!(look_at(&buf, "New queue").1, Modifier::empty());
    }

    #[test]
    fn while_typing_the_name_row_has_a_bold_pointer_and_a_text_cursor() {
        let mut switcher = inside_tmux();
        for _ in 0..2 {
            press(&mut switcher, KeyCode::Down);
        }
        for c in "weekly-review".chars() {
            press(&mut switcher, KeyCode::Char(c));
        }
        let buf = drawn(&switcher);

        let (x, y) = find(&buf, "❯ ✎ New queue: weekly-review");
        assert_eq!(
            (buf[(x, y)].fg, buf[(x, y)].modifier),
            (Color::Yellow, Modifier::BOLD)
        );
        assert!(!theme::has_focus_fill(&buf[(x + 2, y)]), "no fill");
        let cursor = &buf[(x + 2 + 13 + 13, y)];
        assert_eq!(
            (cursor.symbol(), cursor.modifier),
            (" ", Modifier::REVERSED)
        );
    }

    #[test]
    fn outside_tmux_the_footer_tells_the_agents_the_queue_under_the_cursor() {
        let mut switcher = outside_tmux();
        assert_eq!(
            frame(&drawn(&switcher)).last().map(String::as_str),
            Some("╰── tell your agents: --queue asqr-todos ╯")
        );

        press(&mut switcher, KeyCode::Up);
        assert_eq!(
            frame(&drawn(&switcher)).last().map(String::as_str),
            Some("╰─── tell your agents: --queue asqr-todo ╯")
        );
    }

    #[test]
    fn outside_tmux_the_default_queue_has_no_footer() {
        let mut switcher = outside_tmux();
        press(&mut switcher, KeyCode::Down);

        assert_eq!(
            frame(&drawn(&switcher)).last().map(String::as_str),
            Some("╰────────────────────────────────────────╯")
        );
    }

    /// `outside_tmux` with `name` typed on the new-queue row.
    fn typing_outside_tmux(name: &str) -> Switcher {
        let mut switcher = outside_tmux();
        for _ in 0..2 {
            press(&mut switcher, KeyCode::Down);
        }
        for c in name.chars() {
            press(&mut switcher, KeyCode::Char(c));
        }
        switcher
    }

    #[test]
    fn outside_tmux_the_new_queue_row_has_a_footer_once_a_name_is_typed() {
        let empty = frame(&drawn(&typing_outside_tmux("")));
        assert!(
            empty.last().is_some_and(|edge| !edge.contains("tell")),
            "{empty:#?}"
        );

        let named = frame(&drawn(&typing_outside_tmux("weekly-review")));
        assert!(
            named
                .last()
                .is_some_and(|edge| edge.ends_with(" tell your agents: --queue weekly-review ╯")),
            "{named:#?}"
        );
    }

    #[test]
    fn the_name_row_left_with_esc_has_the_fill_and_keeps_the_name() {
        let mut switcher = typing_outside_tmux("weekly");
        press(&mut switcher, KeyCode::Esc);
        let buf = drawn(&switcher);

        let (x, y) = find(&buf, "❯ ✎ New queue: weekly");
        assert!(theme::has_focus_fill(&buf[(x, y)]));
    }

    #[test]
    fn a_new_name_too_long_for_the_footer_is_named_by_the_row_alone() {
        let shown = frame(&drawn(&typing_outside_tmux(&"a".repeat(90))));

        assert!(
            shown
                .last()
                .is_some_and(|edge| edge.ends_with(" tell your agents --queue with this name ╯")),
            "{shown:#?}"
        );
        assert_eq!(shown[0].chars().count(), 76, "as wide as the screen allows");
    }

    #[test]
    fn a_queue_name_too_long_for_the_footer_is_named_by_the_row_alone() {
        let mut choice = choice(Place::Outside);
        choice.rows[0] = named(&"a".repeat(90), 0, false);
        let mut switcher = Switcher::new(choice);
        press(&mut switcher, KeyCode::Up);

        let shown = frame(&drawn(&switcher));

        assert!(
            shown
                .last()
                .is_some_and(|edge| edge.ends_with(" tell your agents --queue with this name ╯")),
            "{shown:#?}"
        );
    }

    #[test]
    fn a_long_path_keeps_the_names_in_their_column() {
        let mut choice = choice(Place::Outside);
        choice.rows.insert(
            0,
            QueueRow {
                location: QueueLocation::at("~/projects/clients/some-client/2026/asqr/queue"),
                label: "~/projects/clients/some-client/2026/asqr/queue".into(),
                agent_dir: Some("~/projects/clients/some-client/2026/asqr/queue".into()),
                waiting: 0,
                watched: false,
            },
        );

        let shown = frame(&drawn(&Switcher::new(choice)));

        assert!(
            shown[3].starts_with("│ ❯ asqr-todos   2 waiting   "),
            "{shown:#?}"
        );
        assert!(
            shown[1].starts_with("│   ~/projects/clients/some-client/2026/asqr/queue"),
            "{shown:#?}"
        );
    }

    /// The start picker over the mockup's queues, none watched, drawn at
    /// 80×24 with `notice`.
    fn picker_screen(keys: &[KeyCode], notice: Option<&Notice>) -> ratatui::backend::TestBackend {
        let mut choice = choice(Place::Outside);
        choice.rows[1].watched = false;
        let mut picker = Switcher::picker(choice);
        for &code in keys {
            press(&mut picker, code);
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).expect("terminal");
        terminal
            .draw(|frame| draw_picker(frame, &picker, notice, "asqr 0.0.0"))
            .expect("draws");
        terminal.backend().clone()
    }

    #[test]
    fn the_picker_shows_the_queues_under_asqr_s_name_with_its_keys() {
        let backend = picker_screen(&[KeyCode::Up], None);
        let buf = backend.buffer();

        find(buf, "❯ asqr-todos   2 waiting");
        insta::assert_snapshot!("picker", backend);
        assert_eq!(row(buf, 0).trim_end(), " asqr");
        assert_eq!(look_at(buf, "asqr"), (Color::Cyan, Modifier::BOLD));
        assert_eq!(
            row(buf, 23),
            format!(
                " ↑/↓  move   enter  watch   q  quit{}asqr 0.0.0",
                " ".repeat(34)
            )
        );
        assert!(
            !buf[find(buf, "asqr-todos")]
                .modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn the_picker_on_default_has_no_footer() {
        let backend = picker_screen(&[], None);

        insta::assert_snapshot!("picker_on_default", backend);
        assert!(!backend.to_string().contains("tell your agents"));
    }

    #[test]
    fn the_picker_shows_a_notice_and_the_keys_of_a_typed_name() {
        let notice = Notice::new(crate::tui::Tone::Error, "another asqr is watching queue x");
        let backend = picker_screen(&[KeyCode::Down], Some(&notice));
        let buf = backend.buffer();

        assert_eq!(
            row(buf, 22).trim_end(),
            "✗ another asqr is watching queue x"
        );
        assert!(row(buf, 23).starts_with(" enter  create   esc  stop typing"));
    }

    #[test]
    fn the_frame_keeps_its_width_as_the_cursor_moves() {
        let mut switcher = outside_tmux();
        let width = |switcher: &Switcher| frame(&drawn(switcher))[0].chars().count();
        let first = width(&switcher);

        press(&mut switcher, KeyCode::Down);

        assert_eq!(width(&switcher), first);
    }

    /// A `--dir` queue `path` first, then `default` watched.
    fn with_a_dir(path: &str, place: Place) -> Switcher {
        with_a_dir_told(path, path, place)
    }

    /// Like [`with_a_dir`], with `word` as what the footer tells agents.
    fn with_a_dir_told(path: &str, word: &str, place: Place) -> Switcher {
        let dir = QueueRow {
            location: QueueLocation::at(path),
            label: path.to_owned(),
            agent_dir: Some(word.to_owned()),
            waiting: 0,
            watched: false,
        };
        let mut choice = choice(place);
        choice.rows = vec![dir, named("default", 0, true)];
        choice.taken.clear();
        let mut switcher = Switcher::new(choice);
        press(&mut switcher, KeyCode::Up);
        switcher
    }

    #[test]
    fn inside_tmux_a_dir_queue_says_the_agents_follow_only_a_named_queue() {
        let switcher = with_a_dir("~/work/queue", Place::Tmux("asqr".into()));

        assert_eq!(
            frame(&drawn(&switcher)).last().map(String::as_str),
            Some("╰─ agents in tmux session asqr follow only a named queue ╯")
        );
    }

    #[test]
    fn outside_tmux_a_dir_queue_tells_the_agents_its_whole_path() {
        let switcher = with_a_dir("~/work/queue", Place::Outside);

        let shown = frame(&drawn(&switcher));

        assert!(
            shown
                .last()
                .is_some_and(|edge| edge.ends_with(" tell your agents: --dir ~/work/queue ╯")),
            "{shown:#?}"
        );
    }

    /// The footer tells the word an agent can paste after `--dir`, which
    /// the row carries beside the label it shows.
    #[test]
    fn outside_tmux_the_footer_tells_the_agents_word_not_the_label() {
        let switcher = with_a_dir_told("~/my q", "~/'my q'", Place::Outside);

        let shown = frame(&drawn(&switcher));

        assert!(
            shown
                .last()
                .is_some_and(|edge| edge.ends_with(" tell your agents: --dir ~/'my q' ╯")),
            "{shown:#?}"
        );
    }

    #[test]
    fn a_path_too_long_for_the_footer_is_named_by_the_row_alone() {
        let long = format!("~/{}queue", "deep/".repeat(20));
        let switcher = with_a_dir(&long, Place::Outside);

        let shown = frame(&drawn(&switcher));

        assert!(
            shown
                .last()
                .is_some_and(|edge| edge.ends_with(" tell your agents --dir with this path ╯")),
            "{shown:#?}"
        );
        assert!(shown[1].contains("…"), "the row cuts the path: {shown:#?}");
        assert!(shown[1].contains("deep/queue"), "{shown:#?}");
    }
}
