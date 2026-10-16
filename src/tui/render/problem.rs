// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The one component that shows what went wrong (ADR 49), in three forms:
//! a card in the context pane, a line in the status line and a dialog
//! over the screen.
//!
//! A problem has a severity, which gives it its mark and its colour, and
//! a headline: the reason in the words of whoever reported it, the
//! system, the image decoder or asqr itself, as the person sees them.

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph, Widget};
use unicode_segmentation::UnicodeSegmentation;

use super::overlay::popup;
use super::panes::READING_WIDTH;
use super::source_line::columns;
use super::theme;
use super::viewer::{ContentSize, Viewer};
use super::wrap::{cut, wrap};
use crate::graphics::PlacedImage;
use crate::tui::{Notice, Offset, Tone};

// ---------------------------------------------------------------------
// The problem
// ---------------------------------------------------------------------

/// How bad a problem is, as the kinds of notices sort it (ADR 38). The
/// mark carries it where colours are off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Severity {
    /// Something failed, such as a file that cannot be read.
    Error,
    /// asqr refused something or set it aside on purpose, such as an
    /// invalid session it answered with an error.
    Warning,
}

impl Severity {
    /// The severity of a notice of `tone`. Plain news and a sent session
    /// are no problem, and their lines carry their own marks where they
    /// have one, such as `✔ sent`.
    pub(super) fn of(tone: Tone) -> Option<Severity> {
        match tone {
            Tone::Error => Some(Severity::Error),
            Tone::Warning => Some(Severity::Warning),
            Tone::Info | Tone::Success => None,
        }
    }

    pub(super) fn mark(self) -> &'static str {
        match self {
            Severity::Error => "✗",
            Severity::Warning => "!",
        }
    }

    pub(super) fn style(self) -> Style {
        match self {
            Severity::Error => theme::error(),
            Severity::Warning => theme::warning(),
        }
    }
}

/// What went wrong, in the words it was reported in, and the details that
/// make it up, such as each error of an invalid session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Problem {
    severity: Severity,
    headline: String,
    details: Vec<String>,
}

impl Problem {
    pub(super) fn error(headline: impl Into<String>) -> Self {
        Problem {
            severity: Severity::Error,
            headline: headline.into(),
            details: Vec::new(),
        }
    }

    pub(super) fn warning(headline: impl Into<String>) -> Self {
        Problem {
            severity: Severity::Warning,
            ..Problem::error(headline)
        }
    }

    pub(super) fn with_details(self, details: Vec<String>) -> Self {
        Problem { details, ..self }
    }

    /// The problem of one side of a diff, `old` or `new`, which the
    /// headline names first, as a diff shows the first side that fails.
    pub(super) fn on_side(self, side: &str) -> Self {
        Problem {
            headline: format!("{side} side: {}", self.headline),
            ..self
        }
    }
}

// ---------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------

/// `path` with the home folder as `~`. Only a path inside the home folder
/// counts, not one that merely starts with the same letters. A home of `/`
/// would turn every path into one starting with `~`, so it counts as none.
fn shown_path(path: &str, home: Option<&Path>) -> String {
    // Entry paths are UTF-8 text, so a home that is not can never be the
    // start of one.
    let home = home
        .and_then(Path::to_str)
        .map(|home| home.trim_end_matches('/'))
        .filter(|home| !home.is_empty());
    match home.and_then(|home| path.strip_prefix(home)) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

/// The file name of `shown` and the folder it is in, `/` included, so
/// each gets lines of its own. A path without a folder, and one that ends
/// in a `/` and so names no file, is all name.
fn name_and_folder(shown: &str) -> (String, String) {
    match shown.rfind('/') {
        Some(slash) if slash + 1 < shown.len() => {
            (shown[slash + 1..].to_owned(), shown[..=slash].to_owned())
        }
        _ => (shown.to_owned(), String::new()),
    }
}

/// `folder` in rows of at most `width` columns, each ending after a `/`
/// where it can, so a row holds whole folder names. A name that does not
/// fit beside the row so far starts a row of its own, and one wider than
/// a whole row breaks where the row ends. The folder is never shortened:
/// the rows put together are the folder.
fn folder_rows(folder: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row = String::new();
    for piece in folder.split_inclusive('/') {
        if !row.is_empty() && columns(&row) + columns(piece) > width {
            rows.push(std::mem::take(&mut row));
        }
        for grapheme in piece.graphemes(true) {
            // Every row takes at least one grapheme, so a character wider
            // than the whole row still gets a row.
            if !row.is_empty() && columns(&row) + columns(grapheme) > width {
                rows.push(std::mem::take(&mut row));
            }
            row.push_str(grapheme);
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

// ---------------------------------------------------------------------
// The card
// ---------------------------------------------------------------------

/// The widest a card gets, frame included (ADR 49).
const CARD_WIDTH: u16 = 60;

/// The columns a card's frame takes from its width: the border and a
/// blank column inside it on either side, as the overlays have.
const FRAME_COLUMNS: u16 = 4;

/// A context entry that cannot be shown, in place of its content: a
/// framed card with the problem's mark and headline, then the file's
/// name and its folder (ADR 49).
pub(super) struct Card {
    problem: Problem,
    name: String,
    folder: String,
    /// The rows inside the frame, laid out for the last body.
    rows: Vec<Line<'static>>,
    /// Where the card sits in the content.
    at: Rect,
    /// The lines of the content: the body's, or the card's where it is
    /// taller.
    lines: usize,
}

impl Card {
    /// The card of `problem` with the file at `path`, the home folder in
    /// it shown as `~`.
    pub(super) fn new(problem: Problem, path: &str, home: Option<&Path>) -> Self {
        let (name, folder) = name_and_folder(&shown_path(path, home));
        Card {
            problem,
            name,
            folder,
            rows: Vec::new(),
            at: Rect::default(),
            lines: 0,
        }
    }

    /// The rows inside the frame for `width` columns: the headline under
    /// the mark, a blank row, the name, then the folder.
    fn rows_for(&self, width: usize) -> Vec<Line<'static>> {
        let severity = self.problem.severity;
        let strong = severity.style().bold();
        let headline = Line::from(vec![
            Span::styled(format!("{} ", severity.mark()), strong),
            Span::styled(self.problem.headline.clone(), strong),
        ]);
        let mut rows = wrap(headline, width, 0, 2);
        rows.push(Line::default());
        rows.extend(wrap(Line::from(self.name.clone()), width, 0, 0));
        rows.extend(
            folder_rows(&self.folder, width)
                .into_iter()
                .map(|row| Line::styled(row, theme::secondary())),
        );
        rows
    }
}

/// The columns `line` shows. `wrap` lets the blanks after a row's last
/// word hang past the row, and they show nothing.
fn shown_columns(line: &Line) -> usize {
    columns(line.to_string().trim_end())
}

impl Viewer for Card {
    fn lay_out(&mut self, width: u16, height: u16) -> ContentSize {
        let most = width.min(CARD_WIDTH);
        let inner = usize::from(most.saturating_sub(FRAME_COLUMNS)).max(1);
        self.rows = self.rows_for(inner);
        let widest = self.rows.iter().map(shown_columns).max().unwrap_or(0);
        let card_width = u16::try_from(widest)
            .unwrap_or(u16::MAX)
            .saturating_add(FRAME_COLUMNS)
            .min(most);
        let card_height = u16::try_from(self.rows.len() + 2).unwrap_or(u16::MAX);
        let body = Rect::new(0, 0, width, height);
        self.at = if card_height <= height {
            body.centered(
                Constraint::Length(card_width),
                Constraint::Length(card_height),
            )
        } else {
            // Taller than the body: the card starts at the top, and the
            // pane scrolls it like any content.
            Rect {
                y: 0,
                height: card_height,
                ..body.centered_horizontally(Constraint::Length(card_width))
            }
        };
        self.lines = usize::from(card_height.max(height));
        ContentSize {
            lines: self.lines,
            columns: usize::from(width),
        }
    }

    /// Draws the whole content into a buffer of its own and copies the
    /// lines from `offset.top` on, as a frame cannot be drawn from a line
    /// in its middle.
    fn draw(&mut self, area: Rect, buf: &mut Buffer, offset: Offset) -> Option<PlacedImage> {
        let tall = u16::try_from(self.lines).unwrap_or(u16::MAX);
        let mut whole = Buffer::empty(Rect::new(0, 0, area.width, tall));
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.problem.severity.style())
            .padding(Padding::horizontal(1));
        let at = self.at.intersection(whole.area);
        let inside = block.inner(at);
        block.render(at, &mut whole);
        Paragraph::new(self.rows.clone()).render(inside, &mut whole);

        let top = u16::try_from(offset.top).unwrap_or(u16::MAX);
        for (y, from) in (0..area.height).zip(top..tall) {
            for x in 0..area.width {
                buf[(area.x + x, area.y + y)] = whole[(x, from)].clone();
            }
        }
        None
    }

    fn failed(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------
// The status line
// ---------------------------------------------------------------------

/// `notice` as the status line shows it in `width` columns: in its tone's
/// colour, an error or a warning after its mark. A notice that names a
/// session is cut in the name, which ends in `…` then, to the room the
/// mark and the rest leave, so they show whole (ADR 38); any other is cut
/// at the edge.
pub(super) fn status_line(notice: &Notice, width: u16) -> Line<'static> {
    let mark = Severity::of(notice.tone)
        .map(|severity| Span::styled(format!("{} ", severity.mark()), Style::new().bold()));
    let text = match notice.name.clone() {
        None => notice.text.clone(),
        Some(name) => {
            let (before, after) = (&notice.text[..name.start], &notice.text[name.end..]);
            let taken = mark.as_ref().map_or(0, Span::width) + columns(before) + columns(after);
            let name = cut(&notice.text[name], usize::from(width).saturating_sub(taken));
            format!("{before}{name}{after}")
        }
    };
    let spans: Vec<Span<'static>> = mark.into_iter().chain([Span::from(text)]).collect();
    Line::from(spans).style(theme::for_tone(notice.tone))
}

// ---------------------------------------------------------------------
// The dialog
// ---------------------------------------------------------------------

/// The dialog of `problem` titled `title`, over the middle of `area`: the
/// headline, a blank row and each detail behind a `•`, in a frame in the
/// problem's colour with its mark before the title. It is as wide as what
/// it holds, up to the reading width. Details that do not fit are left out
/// whole, and a last row counts them.
pub(super) fn draw_dialog(area: Rect, buf: &mut Buffer, title: &str, problem: &Problem) {
    let severity = problem.severity;
    let most = area
        .width
        .saturating_sub(4)
        .min(READING_WIDTH + FRAME_COLUMNS);
    let inner = usize::from(most.saturating_sub(FRAME_COLUMNS)).max(1);

    let mut rows = wrap(Line::from(problem.headline.clone()), inner, 0, 0);
    rows.push(Line::default());
    let details: Vec<Vec<Line<'static>>> = problem
        .details
        .iter()
        .map(|detail| {
            let line = Line::from(vec![
                Span::styled("• ", severity.style()),
                Span::from(detail.clone()),
            ]);
            wrap(line, inner, 0, 2)
        })
        .collect();
    let room = usize::from(area.height.saturating_sub(2));
    let all: usize = details.iter().map(Vec::len).sum();
    if rows.len() + all <= room {
        rows.extend(details.into_iter().flatten());
    } else {
        // One row is kept for the count, and a detail that does not fit
        // whole goes into it, so no detail shows in part.
        let left = room.saturating_sub(rows.len() + 1);
        let mut used = 0;
        let shown = details
            .iter()
            .take_while(|detail| {
                used += detail.len();
                used <= left
            })
            .count();
        let more = details.len() - shown;
        rows.extend(details.into_iter().take(shown).flatten());
        rows.push(Line::styled(format!("… {more} more"), theme::secondary()));
    }

    // The title stands on the frame between the corners, ` ! ` before it
    // and a blank after it.
    let title_width = columns(title) + 6;
    let widest = rows.iter().map(shown_columns).max().unwrap_or(0);
    let width = u16::try_from((widest + usize::from(FRAME_COLUMNS)).max(title_width))
        .unwrap_or(u16::MAX)
        .min(most);
    let height = u16::try_from(rows.len() + 2).unwrap_or(u16::MAX);
    let at = popup(area, width, height, buf);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(severity.style())
        .title(Line::from(vec![
            Span::styled(format!(" {} ", severity.mark()), severity.style().bold()),
            Span::styled(format!("{title} "), theme::title()),
        ]))
        .title_bottom(Line::styled(" any key closes ", theme::secondary()).right_aligned())
        .padding(Padding::horizontal(1));
    Paragraph::new(rows).block(block).render(at, buf);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ratatui::style::{Color, Modifier};

    use super::super::test_support::{self, row};
    use super::super::viewer::{ContentSize, Viewer};
    use super::*;
    use crate::tui::Offset;

    const MISSING: &str = "No such file or directory (os error 2)";

    fn card(reason: &str, path: &str) -> Card {
        Card::new(Problem::error(reason), path, None)
    }

    /// The rows of `card` laid out and drawn into `width`×`height` cells
    /// from line `top` on.
    fn drawn(card: &mut Card, width: u16, height: u16, top: usize) -> Vec<String> {
        let buf = test_support::drawn(card, width, height, Offset { top, left: 0 });
        (0..height).map(|y| row(&buf, y)).collect()
    }

    // -----------------------------------------------------------------
    // The card
    // -----------------------------------------------------------------

    #[test]
    fn the_card_shows_the_reason_then_the_file_and_its_folder() {
        let mut card = card(MISSING, "/srv/app/src/fetch.rs");

        assert_eq!(
            card.lay_out(50, 8),
            ContentSize {
                lines: 8,
                columns: 50
            }
        );
        let border = "─".repeat(42);
        assert_eq!(
            drawn(&mut card, 50, 8, 0),
            [
                String::new(),
                format!("   ╭{border}╮"),
                format!("   │ ✗ {MISSING} │"),
                format!("   │{}│", " ".repeat(42)),
                format!("   │ fetch.rs{}│", " ".repeat(33)),
                format!("   │ /srv/app/src/{}│", " ".repeat(28)),
                format!("   ╰{border}╯"),
                String::new(),
            ]
        );
    }

    #[test]
    fn the_card_is_red_its_reason_bold_and_its_folder_dim() {
        let mut card = card(MISSING, "/srv/app/src/fetch.rs");
        let buf = test_support::drawn(&mut card, 50, 8, Offset::default());

        for corner in [(3, 1), (46, 1), (3, 6), (46, 6), (3, 3), (20, 6)] {
            assert_eq!(buf[corner].fg, Color::Red, "frame at {corner:?}");
        }
        for x in 5..45 {
            let cell = &buf[(x, 2)];
            assert_eq!(
                (cell.fg, cell.modifier),
                (Color::Red, Modifier::BOLD),
                "headline column {x}"
            );
        }
        let name = &buf[(5, 4)];
        assert_eq!((name.fg, name.modifier), (Color::Reset, Modifier::empty()));
        for x in 5..18 {
            assert_eq!(buf[(x, 5)].modifier, Modifier::DIM, "folder column {x}");
        }
    }

    #[test]
    fn a_card_is_at_most_60_columns_wide() {
        let reason = "word ".repeat(20);
        let mut card = card(reason.trim_end(), "/a.rs");

        let rows = drawn(&mut card, 120, 12, 0);

        let top = rows.iter().find(|row| row.contains('╭')).expect("a frame");
        assert_eq!(top.trim_start().chars().count(), 60, "{rows:#?}");
        let second = rows
            .iter()
            .position(|row| row.contains("│ ✗ word"))
            .expect("the headline")
            + 1;
        assert!(
            rows[second].trim_start().starts_with("│   word"),
            "wrapped under its start: {rows:#?}"
        );
    }

    /// `wrap` lets the blank after a row's last word hang past the row, and
    /// that blank must not make the card wider than its text.
    #[test]
    fn a_card_is_as_wide_as_its_visible_text() {
        // At 30 columns the inner width is 26: "✗ aaaa … " breaks right
        // after a blank, and the widest visible row is 24 columns.
        let mut card = card("aaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbb", "/x");

        let rows = drawn(&mut card, 30, 8, 0);

        let top = rows.iter().find(|row| row.contains('╭')).expect("a frame");
        assert_eq!(top.trim_start().chars().count(), 28, "{rows:#?}");
    }

    #[test]
    fn a_card_centres_in_a_body_it_fits() {
        let mut card = card("gone", "/srv/fetch.rs");

        assert_eq!(
            card.lay_out(40, 12),
            ContentSize {
                lines: 12,
                columns: 40
            }
        );
        let rows = drawn(&mut card, 40, 12, 0);
        assert!(rows[3].contains('╭'), "{rows:#?}");
        assert!(rows[8].contains('╰'), "{rows:#?}");
    }

    #[test]
    fn a_card_taller_than_the_body_starts_at_the_top_and_scrolls() {
        let folder = "/".to_owned() + &"deeper/".repeat(14);
        let mut card = card("gone", &format!("{folder}x.rs"));

        let size = card.lay_out(30, 4);

        // Headline, blank, name and five rows of folder in the frame.
        assert_eq!(size.lines, 10);
        let rows = drawn(&mut card, 30, 4, 0);
        assert!(rows[0].contains('╭'), "{rows:#?}");
        let scrolled = drawn(&mut card, 30, 4, 6);
        assert!(scrolled[0].contains("deeper/"), "{scrolled:#?}");
        assert!(scrolled[3].contains('╰'), "{scrolled:#?}");
    }

    #[test]
    fn a_narrow_body_still_draws_a_card() {
        for width in 1..=6 {
            let mut card = card(MISSING, "/srv/app/src/fetch.rs");
            let size = card.lay_out(width, 3);
            assert!(size.lines >= 3, "{width}: {size:?}");
            drawn(&mut card, width, 3, 0);
        }
    }

    #[test]
    fn a_card_shows_a_problem_in_place_of_content() {
        assert!(card(MISSING, "/a.rs").failed());
    }

    // -----------------------------------------------------------------
    // The dialog
    // -----------------------------------------------------------------

    /// The rows of the dialog of `problem` drawn into `width`×`height`
    /// cells.
    fn dialog(problem: &Problem, width: u16, height: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        draw_dialog(area, &mut buf, "Session broken is invalid", problem);
        (0..height).map(|y| row(&buf, y)).collect()
    }

    #[test]
    fn a_dialog_with_no_room_for_a_detail_counts_them_all() {
        let problem = Problem::warning("Why:").with_details(vec!["one".into(), "two".into()]);

        // 3 rows inside the frame: the headline, the blank row and the
        // count.
        let rows = dialog(&problem, 40, 5);

        assert!(rows[3].contains("│ … 2 more"), "{rows:#?}");
        assert!(!rows.iter().any(|row| row.contains('•')), "{rows:#?}");
    }

    #[test]
    fn a_dialog_is_as_wide_as_its_title_when_that_is_its_widest_part() {
        let problem = Problem::warning("Why:").with_details(vec!["one".into()]);

        let rows = dialog(&problem, 80, 7);

        // ` ! ` and the title with a blank after it stand between the
        // corners: 25 + 4 + 2 columns.
        let top = rows.iter().find(|row| row.contains('╭')).expect("a frame");
        assert_eq!(super::super::columns(top.trim_start()), 31, "{rows:#?}");
    }

    #[test]
    fn a_detail_wraps_under_its_start() {
        let problem = Problem::warning("Why:").with_details(vec!["word ".repeat(12)]);

        let rows = dialog(&problem, 40, 9);

        let first = rows
            .iter()
            .position(|row| row.contains("• word"))
            .expect("the detail");
        assert!(rows[first + 1].contains("│   word"), "{rows:#?}");
    }

    // -----------------------------------------------------------------
    // Paths
    // -----------------------------------------------------------------

    #[test]
    fn the_home_folder_shows_as_a_tilde() {
        let home = |home: &str, path: &str| shown_path(path, Some(Path::new(home)));

        assert_eq!(home("/home/me", "/home/me/src/a.rs"), "~/src/a.rs");
        assert_eq!(
            home("/home/me", "/home/meadow/a.rs"),
            "/home/meadow/a.rs",
            "another folder that starts alike"
        );
        assert_eq!(home("/home/me/", "/home/me/src/a.rs"), "~/src/a.rs");
        assert_eq!(home("/", "/srv/a.rs"), "/srv/a.rs", "the root is no home");
        assert_eq!(home("", "/srv/a.rs"), "/srv/a.rs");
        assert_eq!(shown_path("/home/me/a.rs", None), "/home/me/a.rs");
    }

    #[test]
    fn a_path_without_a_folder_shows_only_its_name() {
        assert_eq!(
            name_and_folder("/srv/app/fetch.rs"),
            ("fetch.rs".to_owned(), "/srv/app/".to_owned())
        );
        assert_eq!(
            name_and_folder("Cargo.toml"),
            ("Cargo.toml".to_owned(), String::new())
        );
        assert_eq!(name_and_folder("/"), ("/".to_owned(), String::new()));

        let rows = drawn(&mut card("relative path", "Cargo.toml"), 40, 7, 0);
        assert!(rows[4].contains("Cargo.toml"), "{rows:#?}");
        assert!(rows[5].contains('╰'), "no folder row: {rows:#?}");
    }

    #[test]
    fn a_folder_breaks_after_a_slash_and_is_never_shortened() {
        let folder = "/srv/applications/source/render/widgets/cards/";

        let rows = folder_rows(folder, 20);

        assert_eq!(
            rows,
            ["/srv/applications/", "source/render/", "widgets/cards/"]
        );
        assert!(rows.iter().all(|row| super::super::columns(row) <= 20));
        assert_eq!(rows.concat(), folder);
    }

    #[test]
    fn a_piece_longer_than_a_row_breaks_where_the_row_ends() {
        let long = "x".repeat(44) + "/";

        assert_eq!(
            folder_rows(&long, 20),
            ["x".repeat(20), "x".repeat(20), "xxxx/".to_owned()]
        );
        assert_eq!(
            folder_rows(&format!("/srv/{long}"), 20),
            [
                "/srv/".to_owned(),
                "x".repeat(20),
                "x".repeat(20),
                "xxxx/".to_owned()
            ],
            "a piece that does not fit starts a row of its own"
        );
        // Each 語 takes two columns: nine fit into 19, and the tenth
        // starts the next row rather than cross the end.
        assert_eq!(
            folder_rows(&("語".repeat(12) + "/"), 19),
            ["語".repeat(9), "語".repeat(3) + "/"]
        );
        assert_eq!(folder_rows("語/", 1), ["語", "/"], "one per row at least");
    }
}
