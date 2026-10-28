// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The context pane (spec section 8.1): the entry's viewer on a ledge, a
//! dim header rule that names the entry shown and a bottom rule that says
//! which of its lines show, joined by a left edge beside the question
//! (ADR 54). The same pane, edge included, fills the tab's area when the
//! person shows the entry full screen with `z`. Where the terminal has
//! room for no pane at all, a one-line hint stands in for it.

use std::path::Path;

use patine::QuoteStyle;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::problem::Severity;
use super::source_line::columns;
use super::theme;
use super::viewer::{Shape, Viewers};
use super::wrap::cut;
use crate::format::ContextEntry;
use crate::graphics::PlacedImage;
use crate::tui::{DrawnDiff, Extent, Offset, Page, SessionState};

/// What the pane showed: how far it scrolls, whether its entry was still
/// loading, and the Kitty image it drew, which the screen mends once the
/// overlays are drawn (ADR 48).
pub struct DrawnPane {
    pub page: Page,
    pub loading: bool,
    pub image: Option<PlacedImage>,
    pub diff: Option<DrawnDiff>,
}

/// Which form of the ledge a pane takes, by where it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ledge {
    /// Beside the question, and full screen: the header rule turns down at
    /// a `╭` into an edge along the pane's left side, which ends in the
    /// bottom rule's `╰`, and the body starts two columns right of it.
    Beside,
    /// Below the question: a blank row, then the header rule, the body over
    /// the whole width and the bottom rule, without corners or edge.
    Below,
}

/// Draws the pane for the entry `state` shows into `area`, on the ledge
/// `ledge`, scrolled as far as the person scrolled it, and says what it
/// showed. The viewer fills the body with Markdown quotes in `quote`. A
/// row without context leaves the pane empty, its ledge included, so it
/// keeps its place while the cursor moves on.
pub fn draw_pane(
    area: Rect,
    buf: &mut Buffer,
    state: &SessionState,
    viewers: &mut Viewers,
    quote: QuoteStyle,
    ledge: Ledge,
) -> Option<DrawnPane> {
    let (key, entry) = state.shown_entry()?;
    let parts = parts(area, ledge);
    let body = parts.body;

    // The full-screen view reached from a pane shows an image without its
    // caption, which the pane showed (ADR 69), and a diff shows side by
    // side while the person switched it so (ADR 70).
    let shape = Shape {
        alone: state.full_screen_alone(),
        side_by_side: state.side_by_side(),
    };
    let (viewer, size) = viewers.laid_out_shaped(
        state.id(),
        key,
        entry,
        (body.width, body.height),
        shape,
        quote,
    );
    // Content reports lines wider than the body only where it cuts them, so
    // its size alone says whether the pane scrolls sideways.
    let page = Page {
        rows: Extent {
            shown: usize::from(body.height),
            total: size.lines,
        },
        columns: Extent {
            shown: usize::from(body.width),
            total: size.columns,
        },
    };
    // A diff's offset counts the rows of the layout it was last drawn in.
    // Drawn in the other one, it shows from the row that holds the same
    // line, or, while nothing scrolled since the last switch, from where
    // that switch came from, so switching back and forth keeps the line
    // at the top.
    let stored = state.pane_offset();
    let shape = viewer.diff_shape();
    let tag = state.pane_layout_tag();
    let translated = shape
        .filter(|shape| shape.layout != tag.layout)
        .map(|_| match tag.back {
            Some((set, from)) if set == stored.top => (from, stored.top),
            _ => (viewer.top_from(stored.top, tag.layout), stored.top),
        });
    let top = translated.map_or(stored.top, |(top, _)| top);
    // A smaller terminal may have left the offset past the end.
    let offset = page.clamp(Offset { top, ..stored });
    let image = viewer.draw(body, buf, offset);

    // The focused pane fills its name and draws its lines in the focus
    // colour, so the person sees where the keys go.
    let focused = state.pane_focused();
    if let Some(header) = parts.header {
        header_line(entry, header.width, viewer.failed(), focused, ledge).render(header, buf);
    }
    if let Some(edge) = parts.edge {
        for y in edge.top()..edge.bottom() {
            buf.set_string(edge.x, y, "│", lines(focused));
        }
    }
    if let Some(bottom) = parts.bottom {
        bottom_line(page, offset, bottom.width, !focused, focused, ledge).render(bottom, buf);
    }
    Some(DrawnPane {
        page,
        loading: viewer.loading(),
        image,
        diff: shape.map(|shape| DrawnDiff {
            layout: shape.layout,
            fits_side_by_side: shape.fits_side_by_side,
            translated,
        }),
    })
}

/// Where the ledge and the body of a pane in `area` go. An area too low
/// for the ledge gives up the body's rows first, then the bottom rule, and
/// keeps the blank row and the header rule longest.
struct Parts {
    header: Option<Rect>,
    body: Rect,
    edge: Option<Rect>,
    bottom: Option<Rect>,
}

fn parts(area: Rect, ledge: Ledge) -> Parts {
    // Below the question, a blank row stands between the question and the
    // header rule.
    let ruled = match ledge {
        Ledge::Beside => area,
        Ledge::Below => without_top_row(area),
    };
    let header = (ruled.height > 0).then_some(Rect { height: 1, ..ruled });
    let bottom = (ruled.height > 1).then(|| Rect {
        y: ruled.bottom() - 1,
        height: 1,
        ..ruled
    });
    let between = Rect {
        height: ruled.height.saturating_sub(2),
        ..without_top_row(ruled)
    };
    match ledge {
        Ledge::Beside => {
            // The edge, then a blank column before the body.
            let indent = ruled.width.min(2);
            Parts {
                header,
                body: Rect {
                    x: between.x + indent,
                    width: between.width - indent,
                    ..between
                },
                edge: Some(Rect {
                    width: ruled.width.min(1),
                    ..between
                }),
                bottom,
            }
        }
        Ledge::Below => Parts {
            header,
            body: between,
            edge: None,
            bottom,
        },
    }
}

/// `area` without its first row, if it has one.
fn without_top_row(area: Rect) -> Rect {
    let top = area.height.min(1);
    Rect {
        y: area.y + top,
        height: area.height - top,
        ..area
    }
}

/// The body of a pane in `area` on the ledge `ledge`: what the viewers of
/// its entries are laid out for.
pub fn pane_body(area: Rect, ledge: Ledge) -> Rect {
    parts(area, ledge).body
}

/// The style of the ledge's lines: dim, and in the focus colour while the
/// pane has the focus.
fn lines(focused: bool) -> Style {
    if focused {
        theme::focus_rule()
    } else {
        theme::secondary()
    }
}

/// The pane's header rule: `╭─ name ────` on the ledge beside the question,
/// `── name ────` below it, the name in bold with a blank on each side and
/// the rule running on to the right edge. The name gets what the rule's
/// pieces leave of `width` and is cut with `…` where it is longer. Where
/// the viewer shows a problem in place of the entry, `failed`, the
/// problem's mark in its colour stands before the name (ADR 49).
///
/// While the pane has the focus, `focused`, the name, its blanks and the
/// mark take the focus fill and the rule the focus colour.
fn header_line(
    entry: &ContextEntry,
    width: u16,
    failed: bool,
    focused: bool,
    ledge: Ledge,
) -> Line<'static> {
    let lead = match ledge {
        Ledge::Beside => "╭─",
        Ledge::Below => "──",
    };
    let mark = failed.then(|| {
        let severity = Severity::Error;
        Span::styled(format!("{} ", severity.mark()), severity.style().bold())
    });
    let mark_width = mark.as_ref().map_or(0, Span::width);
    // The lead, a blank on each side of the name and at least one `─` of
    // the rule after it.
    let room = usize::from(width).saturating_sub(columns(lead) + 3 + mark_width);
    let name = cut(&entry_name(entry), room);
    let run = usize::from(width).saturating_sub(columns(lead) + 2 + mark_width + columns(&name));

    let title: Vec<Span<'static>> = match mark {
        Some(mark) => vec![
            Span::styled(" ", theme::title()),
            mark,
            Span::styled(format!("{name} "), theme::title()),
        ],
        None => vec![Span::styled(format!(" {name} "), theme::title())],
    };
    let title = title.into_iter().map(|span| {
        if focused {
            theme::focus_span(span)
        } else {
            span
        }
    });
    let mut spans = vec![Span::styled(lead, lines(focused))];
    spans.extend(title);
    spans.push(Span::styled("─".repeat(run), lines(focused)));
    Line::from(spans)
}

/// The pane's bottom rule: on the ledge beside the question it starts at
/// the edge's `╰`, below it is a plain rule, and either way it runs to the
/// right edge. Where the content is taller than the body, the rule ends in
/// which of its lines show, dim: `╰──── 4–15 of 120 ─`, below `──── 4–15 of
/// 120 ──`. Where the content fits, the rule carries no text, so the pane
/// keeps its shape from entry to entry.
///
/// Where the content is taller or wider than the body and `offer_tab`
/// holds, as it does while the pane lacks the focus, the text also names
/// `tab`, the key that moves into the pane to scroll it. The key bar is the
/// first to drop its `tab` hint on a narrow terminal, which leaves the
/// bottom rule the only place that names it. The hint goes before the
/// position would have to be cut.
fn bottom_line(
    page: Page,
    offset: Offset,
    width: u16,
    offer_tab: bool,
    focused: bool,
    ledge: Ledge,
) -> Line<'static> {
    let (lead, end) = match ledge {
        Ledge::Beside => ("╰", "─"),
        Ledge::Below => ("", "──"),
    };
    let width = usize::from(width);
    // A body without rows, in an area too low for it, shows no lines to
    // name.
    let position = page
        .rows
        .position(offset.top)
        .filter(|_| page.rows.shown > 0);
    let wider = page.columns.total > page.columns.shown;
    let offered = offer_tab && (position.is_some() || wider);
    // The lead, at least one `─`, a blank on each side of the text and the
    // end.
    let room = width.saturating_sub(columns(lead) + 3 + columns(end));
    let text = match position {
        Some(position) if offered => {
            let with_tab = format!("{position} · tab");
            Some(if columns(&with_tab) <= room {
                with_tab
            } else {
                position
            })
        }
        Some(position) => Some(position),
        None => offered.then(|| "tab".to_owned()),
    };

    let Some(text) = text else {
        let run = width.saturating_sub(columns(lead));
        return Line::from(Span::styled(
            format!("{lead}{}", "─".repeat(run)),
            lines(focused),
        ));
    };
    let text = cut(&text, room);
    let run = width.saturating_sub(columns(lead) + 2 + columns(&text) + columns(end));
    Line::from(vec![
        Span::styled(format!("{lead}{}", "─".repeat(run)), lines(focused)),
        Span::styled(format!(" {text} "), theme::secondary()),
        Span::styled(end, lines(focused)),
    ])
}

/// What the header calls the entry: its `title`, the file name of its
/// path (a diff's `name`), or what kind of entry it is.
fn entry_name(entry: &ContextEntry) -> String {
    let (kind, name) = kind_and_name(entry);
    name.unwrap_or_else(|| kind.to_owned())
}

/// The kind of `entry` and its name, when it has one.
fn kind_and_name(entry: &ContextEntry) -> (&'static str, Option<String>) {
    // A path names the entry by its file name, which fits a line where
    // the whole path often does not.
    let file_name = |path: &str| {
        Path::new(path).file_name().map_or_else(
            || path.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    };
    match entry {
        ContextEntry::Image(image) => (
            "image",
            image.title.clone().or_else(|| Some(file_name(&image.path))),
        ),
        ContextEntry::Code(code) => (
            "code",
            code.title
                .clone()
                .or_else(|| code.path.as_deref().map(file_name)),
        ),
        ContextEntry::Markdown(markdown) => (
            "markdown",
            markdown
                .title
                .clone()
                .or_else(|| markdown.path.as_deref().map(file_name)),
        ),
        ContextEntry::Diff(diff) => ("diff", diff.title.clone().or_else(|| diff.name.clone())),
        ContextEntry::Ansi(ansi) => (
            "ansi",
            ansi.title
                .clone()
                .or_else(|| ansi.path.as_deref().map(file_name)),
        ),
    }
}

/// The one line that stands in for the pane when the terminal has room
/// for it neither beside nor below the question: what the entry is, and
/// that `z`, as `key` names it, shows it.
pub fn context_hint(entry: Option<&ContextEntry>, key: &str, width: u16) -> Line<'static> {
    let Some(entry) = entry else {
        return Line::default();
    };
    let text = match kind_and_name(entry) {
        (kind, Some(name)) => format!("▸ {kind}: {name} · {key}: full screen"),
        (kind, None) => format!("▸ {kind} · {key}: full screen"),
    };
    Line::styled(cut(&text, usize::from(width)), theme::secondary())
}

#[cfg(test)]
mod tests {
    use crate::graphics::Images;
    use crate::tui::DiffLayout;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    use super::super::patine_adapter::quote_style;
    use super::super::test_support::{Gate, find, row};
    use super::super::viewer::Placeholder;
    use super::*;

    fn entry(json: &str) -> ContextEntry {
        serde_json::from_str(json).expect("entry parses")
    }

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn the_hint_names_the_entry_shown() {
        let hint =
            |entry: Option<&ContextEntry>, width: u16| text(&context_hint(entry, "z", width));

        let cases = [
            (
                r#"{"type": "image", "path": "/pictures/cat.png"}"#,
                "▸ image: cat.png · z: full screen",
            ),
            (
                r#"{"type": "image", "title": "The cat", "path": "/pictures/cat.png"}"#,
                "▸ image: The cat · z: full screen",
            ),
            (
                r#"{"type": "image", "path": "/"}"#,
                "▸ image: / · z: full screen",
            ),
            (
                r#"{"type": "code", "path": "/src/fetch.rs"}"#,
                "▸ code: fetch.rs · z: full screen",
            ),
            (
                r#"{"type": "code", "content": "inline"}"#,
                "▸ code · z: full screen",
            ),
            (
                r#"{"type": "markdown", "path": "/notes/plan.md"}"#,
                "▸ markdown: plan.md · z: full screen",
            ),
            (
                r#"{"type": "markdown", "content": "*inline*"}"#,
                "▸ markdown · z: full screen",
            ),
            (
                r#"{"type": "diff", "name": "src/fetch.rs", "new": {"content": "x"}}"#,
                "▸ diff: src/fetch.rs · z: full screen",
            ),
            (
                r#"{"type": "diff", "new": {"content": "x"}}"#,
                "▸ diff · z: full screen",
            ),
        ];
        for (json, expected) in cases {
            assert_eq!(hint(Some(&entry(json)), 78), expected, "{json}");
        }
        assert_eq!(
            hint(
                Some(&entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#)),
                20
            ),
            "▸ image: cat.png · …",
            "cut to the line"
        );
        assert_eq!(hint(None, 78), "", "a row without context leaves it empty");
    }

    #[test]
    fn an_ansi_entry_is_named_by_title_file_name_or_kind() {
        let hint = |json: &str| text(&context_hint(Some(&entry(json)), "z", 78));

        assert_eq!(
            hint(r#"{"type": "ansi", "title": "Test run", "path": "/logs/run.ansi"}"#),
            "▸ ansi: Test run · z: full screen"
        );
        assert_eq!(
            hint(r#"{"type": "ansi", "path": "/logs/run.ansi"}"#),
            "▸ ansi: run.ansi · z: full screen"
        );
        assert_eq!(
            hint(r#"{"type": "ansi", "content": "x"}"#),
            "▸ ansi · z: full screen"
        );
    }

    #[test]
    fn the_hint_names_the_key_as_given() {
        let image = entry(r#"{"type": "image", "path": "/pictures/cat.png"}"#);

        assert_eq!(
            text(&context_hint(Some(&image), "esc, z", 78)),
            "▸ image: cat.png · esc, z: full screen"
        );
    }

    /// A page of `total` rows of which `shown` show, and `columns` of 20
    /// columns.
    fn page(shown: usize, total: usize, columns: usize) -> Page {
        Page {
            rows: Extent { shown, total },
            columns: Extent {
                shown: 20,
                total: columns,
            },
        }
    }

    fn log() -> ContextEntry {
        entry(r#"{"type": "code", "title": "Log", "content": "x"}"#)
    }

    // -----------------------------------------------------------------
    // The header rule
    // -----------------------------------------------------------------

    #[test]
    fn the_header_names_the_entry_by_title_file_name_or_kind() {
        let header = |json: &str| text(&header_line(&entry(json), 30, false, false, Ledge::Beside));

        let cases = [
            (
                r#"{"type": "image", "path": "/pictures/cat.png"}"#,
                "cat.png",
            ),
            (r#"{"type": "code", "title": "Log", "content": "x"}"#, "Log"),
            (r#"{"type": "code", "content": "x"}"#, "code"),
            (
                r#"{"type": "markdown", "path": "/notes/plan.md"}"#,
                "plan.md",
            ),
            (r#"{"type": "markdown", "content": "x"}"#, "markdown"),
            (
                r#"{"type": "diff", "name": "src/fetch.rs", "new": {"content": "x"}}"#,
                "src/fetch.rs",
            ),
            (r#"{"type": "diff", "new": {"content": "x"}}"#, "diff"),
        ];
        for (json, expected) in cases {
            let line = header(json);
            let name = line
                .strip_prefix("╭─ ")
                .unwrap_or_else(|| panic!("a ruled header: {line:?}"))
                .trim_end_matches('─')
                .trim_end();
            assert_eq!(name, expected, "{json}");
            assert_eq!(line.chars().count(), 30, "fills the line: {json}");
        }
    }

    #[test]
    fn beside_the_header_is_a_dim_rule_with_the_name_set_in_bold() {
        let line = header_line(&log(), 30, false, false, Ledge::Beside);

        assert_eq!(
            line.spans,
            [
                Span::styled("╭─", Style::new().dim()),
                Span::styled(" Log ", Style::new().bold()),
                Span::styled("─".repeat(23), Style::new().dim()),
            ]
        );
    }

    #[test]
    fn below_the_header_rule_has_no_corner() {
        let line = header_line(&log(), 30, false, false, Ledge::Below);

        assert_eq!(text(&line), format!("── Log {}", "─".repeat(23)));
        assert_eq!(line.spans[0], Span::styled("──", Style::new().dim()));
    }

    #[test]
    fn the_name_takes_the_header_less_its_rule_pieces() {
        let titled = entry(
            r#"{"type": "code", "title": "A very long title for a short line", "content": "x"}"#,
        );
        // Of 30 columns the corner, the blanks and one `─` take 5.
        for (ledge, lead) in [(Ledge::Beside, "╭─"), (Ledge::Below, "──")] {
            let line = text(&header_line(&titled, 30, false, false, ledge));
            assert_eq!(
                line,
                format!("{lead} A very long title for a … ─"),
                "{ledge:?}"
            );
        }

        // `ト` would cross into the `…`, so the name takes 24 columns of its
        // 25 and the rule one more.
        let wide = entry(
            r#"{"type": "code", "title": "a日本語のとても長いタイトルです", "content": "x"}"#,
        );
        let line = text(&header_line(&wide, 30, false, false, Ledge::Beside));
        assert_eq!(line, "╭─ a日本語のとても長いタイ… ──");
        assert_eq!(columns(&line), 30);
    }

    #[test]
    fn the_mark_of_an_entry_that_cannot_show_stands_before_its_name() {
        let titled = entry(
            r#"{"type": "code", "title": "A very long title for a short line", "path": "/x.rs"}"#,
        );

        let line = header_line(&titled, 30, true, false, Ledge::Beside);
        assert_eq!(text(&line), "╭─ ✗ A very long title for … ─");
        let mark = line
            .spans
            .iter()
            .find(|span| span.content.contains('✗'))
            .expect("a mark");
        assert_eq!(mark, &Span::styled("✗ ", Style::new().red().bold()));

        let focused = header_line(&titled, 30, true, true, Ledge::Beside);
        let mark = focused
            .spans
            .iter()
            .find(|span| span.content.contains('✗'))
            .expect("a mark");
        assert_eq!(mark.style.fg, Some(Color::Yellow));
        assert!(
            mark.style.add_modifier.contains(Modifier::REVERSED),
            "the mark takes the fill with the name"
        );
    }

    #[test]
    fn a_pane_whose_entry_cannot_show_marks_its_header() {
        let state = session(r#"{"type": "image", "path": "/nowhere/cat.png"}"#);

        let buf = pane(&state, Rect::new(0, 0, 40, 11), Ledge::Beside);

        assert!(
            row(&buf, 0).starts_with("╭─ ✗ cat.png "),
            "{:?}",
            row(&buf, 0)
        );
        assert_eq!(buf[(3, 0)].fg, Color::Red);
    }

    // -----------------------------------------------------------------
    // The bottom rule
    // -----------------------------------------------------------------

    #[test]
    fn the_bottom_rule_says_which_lines_show_and_offers_tab() {
        let taller = page(10, 31, 20);
        let bottom = |offer_tab: bool, ledge: Ledge| {
            bottom_line(taller, Offset::default(), 30, offer_tab, false, ledge)
        };

        let line = bottom(true, Ledge::Beside);
        assert_eq!(
            text(&line),
            format!("╰{} 1–10 of 31 · tab ─", "─".repeat(10))
        );
        assert!(
            line.spans
                .iter()
                .all(|span| span.style == Style::new().dim()),
            "{line:?}"
        );
        assert_eq!(
            text(&bottom(false, Ledge::Beside)),
            format!("╰{} 1–10 of 31 ─", "─".repeat(16)),
            "focused"
        );
        assert_eq!(
            text(&bottom(true, Ledge::Below)),
            format!("{} 1–10 of 31 · tab ──", "─".repeat(10))
        );
    }

    #[test]
    fn the_bottom_rule_offers_tab_alone_for_an_entry_only_wider() {
        let line = bottom_line(
            page(10, 3, 50),
            Offset::default(),
            30,
            true,
            false,
            Ledge::Beside,
        );

        assert_eq!(text(&line), format!("╰{} tab ─", "─".repeat(23)));
    }

    #[test]
    fn the_bottom_rule_stays_without_text_when_the_entry_fits() {
        let fits = page(10, 3, 20);
        let bottom = |offer_tab: bool, ledge: Ledge| {
            bottom_line(fits, Offset::default(), 30, offer_tab, false, ledge)
        };

        for offer_tab in [true, false] {
            let line = bottom(offer_tab, Ledge::Beside);
            assert_eq!(text(&line), format!("╰{}", "─".repeat(29)));
            assert!(
                line.spans
                    .iter()
                    .all(|span| span.style == Style::new().dim()),
                "{line:?}"
            );
            assert_eq!(text(&bottom(offer_tab, Ledge::Below)), "─".repeat(30));
        }
    }

    #[test]
    fn the_bottom_rule_drops_tab_where_it_has_no_room() {
        let taller = page(10, 120, 20);
        let bottom = |width: u16| {
            text(&bottom_line(
                taller,
                Offset { top: 3, left: 0 },
                width,
                true,
                false,
                Ledge::Beside,
            ))
        };

        assert_eq!(bottom(20), format!("╰{} 4–13 of 120 ─", "─".repeat(5)));
        assert_eq!(bottom(12), "╰─ 4–13 o… ─", "the position cut to its room");
    }

    // -----------------------------------------------------------------
    // The pane
    // -----------------------------------------------------------------

    /// A session with one text question whose context is `entry`.
    fn session(entry: &str) -> SessionState {
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "?", "kind": "text",
                "context": [{entry}]}}]}}"#
        );
        SessionState::new("batch", serde_json::from_str(&json).expect("parses"), None)
    }

    /// A session whose context is a code entry of 30 lines, `line 1` on.
    fn thirty_lines() -> SessionState {
        let content = (1..=30)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\\n");
        session(&format!(
            r#"{{"type": "code", "title": "Log", "content": "{content}"}}"#
        ))
    }

    /// `state` with the focus in its pane, which showed `page`.
    fn focused(mut state: SessionState, page: Page) -> SessionState {
        state.set_pane_page(Some(crate::tui::PanePage::shown(page)));
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(state.pane_focused());
        state
    }

    /// The pane of `state` drawn into `area`, which starts at the buffer's
    /// top left.
    fn pane(state: &SessionState, area: Rect, ledge: Ledge) -> Buffer {
        let mut buf = Buffer::empty(area);
        let mut viewers = Viewers::for_tests(Images::half_blocks());
        draw_pane(
            area,
            &mut buf,
            state,
            &mut viewers,
            quote_style(false),
            ledge,
        );
        buf
    }

    /// What the pane of `state` drawn into `area` showed of a diff, and
    /// where its body starts with its first row.
    fn drawn_diff(state: &SessionState, area: Rect) -> (Option<DrawnDiff>, String) {
        let mut buf = Buffer::empty(area);
        let mut viewers = Viewers::for_tests(Images::half_blocks());
        let drawn = draw_pane(
            area,
            &mut buf,
            state,
            &mut viewers,
            quote_style(false),
            Ledge::Beside,
        )
        .expect("a pane shows");
        (drawn.diff, row(&buf, 1))
    }

    /// A diff whose old side ends without a newline, so side by side has
    /// a note of its own and a line facing a blank: unified rows `a`, `b`,
    /// the note, `c` and `d`; side by side `a|a`, `b|c`, the note and `|d`.
    const NOTED_DIFF: &str =
        r#"{"type": "diff", "old": {"content": "a\nb"}, "new": {"content": "a\nc\nd\n"}}"#;

    #[test]
    fn the_pane_hands_back_the_diff_it_drew() {
        let diff = session(NOTED_DIFF);

        assert_eq!(
            drawn_diff(&diff, Rect::new(0, 0, 40, 8)).0,
            Some(DrawnDiff {
                layout: DiffLayout::Unified,
                fits_side_by_side: true,
                translated: None,
            })
        );
        let code = session(r#"{"type": "code", "content": "x"}"#);
        assert_eq!(drawn_diff(&code, Rect::new(0, 0, 40, 8)).0, None);
    }

    /// The session of [`NOTED_DIFF`] whose offset a draw side by side set
    /// to `top`, translated from the unified row `from`.
    fn drawn_side_by_side_at(top: usize, from: usize) -> SessionState {
        let mut state = session(NOTED_DIFF);
        let page = Page {
            rows: Extent { shown: 1, total: 4 },
            columns: Extent::default(),
        };
        state.set_pane_page(Some(crate::tui::PanePage {
            page,
            loading: false,
            diff: Some(DrawnDiff {
                layout: DiffLayout::SideBySide,
                fits_side_by_side: true,
                translated: Some((top, from)),
            }),
        }));
        state
    }

    #[test]
    fn a_diff_back_in_its_layout_shows_the_line_it_came_from() {
        // The note's row side by side, reached from `d`'s unified row.
        let state = drawn_side_by_side_at(2, 4);

        let (diff, first) = drawn_diff(&state, Rect::new(0, 0, 40, 3));

        assert_eq!(first, "│   3 + d");
        assert_eq!(diff.and_then(|diff| diff.translated), Some((4, 2)));
    }

    #[test]
    fn a_diff_scrolled_since_its_switch_shows_the_line_of_its_top() {
        let mut state = drawn_side_by_side_at(2, 4);
        state.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        state.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

        let (diff, first) = drawn_diff(&state, Rect::new(0, 0, 40, 3));

        // Side by side row 3, `|d`, holds the unified row 4.
        assert_eq!(first, "│   3 + d");
        assert_eq!(diff.and_then(|diff| diff.translated), Some((4, 3)));
    }

    #[test]
    fn beside_a_dim_edge_runs_from_the_header_to_the_bottom_rule() {
        let state = session(r#"{"type": "markdown", "title": "Log", "content": "Hello"}"#);

        let buf = pane(&state, Rect::new(0, 0, 40, 11), Ledge::Beside);

        assert_eq!(row(&buf, 0), format!("╭─ Log {}", "─".repeat(33)));
        assert_eq!(row(&buf, 1), "│ Hello", "the content two columns right");
        for y in 1..10 {
            assert_eq!(buf[(0, y)].symbol(), "│", "row {y}");
            assert_eq!(buf[(0, y)].modifier, Modifier::DIM, "row {y}");
            assert_eq!(buf[(1, y)].symbol(), " ", "row {y}");
        }
        assert_eq!(row(&buf, 10), format!("╰{}", "─".repeat(39)));
    }

    #[test]
    fn below_a_blank_row_stands_above_the_header_and_no_edge() {
        let state = session(r#"{"type": "markdown", "title": "Log", "content": "Hello"}"#);

        let buf = pane(&state, Rect::new(0, 0, 40, 11), Ledge::Below);

        assert_eq!(row(&buf, 0), "", "the blank row");
        assert_eq!(row(&buf, 1), format!("── Log {}", "─".repeat(33)));
        assert_eq!(row(&buf, 2), "Hello", "the content from the left");
        assert_eq!(row(&buf, 10), "─".repeat(40));
    }

    #[test]
    fn the_body_leaves_room_for_the_rules_and_edge_beside_and_the_blank_row_below() {
        let area = Rect::new(0, 0, 40, 11);

        assert_eq!(pane_body(area, Ledge::Beside), Rect::new(2, 1, 38, 9));
        assert_eq!(pane_body(area, Ledge::Below), Rect::new(0, 2, 40, 8));
    }

    #[test]
    fn a_pane_too_low_for_its_ledge_draws_what_fits() {
        let state = session(r#"{"type": "markdown", "title": "Log", "content": "Hello"}"#);

        let two = pane(&state, Rect::new(0, 0, 40, 2), Ledge::Beside);
        assert!(row(&two, 0).starts_with("╭─ Log"), "{:?}", row(&two, 0));
        assert_eq!(row(&two, 1), format!("╰{}", "─".repeat(39)));
        assert_eq!(pane_body(Rect::new(0, 0, 40, 2), Ledge::Beside).height, 0);

        let one = pane(&state, Rect::new(0, 0, 40, 1), Ledge::Beside);
        assert!(row(&one, 0).starts_with("╭─ Log"), "{:?}", row(&one, 0));

        let two = pane(&state, Rect::new(0, 0, 40, 2), Ledge::Below);
        assert_eq!(row(&two, 0), "");
        assert!(row(&two, 1).starts_with("── Log"), "{:?}", row(&two, 1));

        let one = pane(&state, Rect::new(0, 0, 40, 1), Ledge::Below);
        assert_eq!(row(&one, 0), "", "only the blank row");
    }

    #[test]
    fn the_focused_pane_fills_its_name_and_turns_its_lines_yellow() {
        let state = focused(thirty_lines(), page(9, 30, 20));

        let buf = pane(&state, Rect::new(0, 0, 40, 11), Ledge::Beside);

        let yellow_line = |x: u16, y: u16| {
            let cell = &buf[(x, y)];
            assert_eq!(
                (cell.fg, cell.modifier),
                (Color::Yellow, Modifier::empty()),
                "a yellow line at {x}, {y}: {:?}",
                cell.symbol()
            );
        };
        for x in [0, 1, 7, 39] {
            yellow_line(x, 0);
        }
        yellow_line(0, 1);
        yellow_line(0, 9);
        yellow_line(0, 10);
        yellow_line(39, 10);
        for x in 2..=6 {
            let cell = &buf[(x, 0)];
            assert_eq!(cell.fg, Color::Yellow, "the name's column {x}");
            assert!(cell.modifier.contains(Modifier::REVERSED), "column {x}");
            assert!(!cell.modifier.contains(Modifier::DIM), "column {x}");
        }
        let (x, y) = find(&buf, "1–9 of 30");
        assert_eq!(y, 10, "the position in the bottom rule");
        assert_eq!(
            (buf[(x, y)].fg, buf[(x, y)].modifier),
            (Color::Reset, Modifier::DIM),
            "the position stays dim"
        );
    }

    #[test]
    fn under_no_color_the_fill_on_the_name_alone_shows_the_focus() {
        let area = Rect::new(0, 0, 40, 11);
        let mut unfocused = pane(&thirty_lines(), area, Ledge::Beside);
        let mut focused = pane(
            &focused(thirty_lines(), page(9, 30, 20)),
            area,
            Ledge::Beside,
        );
        theme::strip_colours(&mut unfocused);
        theme::strip_colours(&mut focused);

        // The corner, a `─` of the header rule, the edge and the bottom rule.
        for (x, y) in [(0, 0), (8, 0), (0, 5), (1, 10)] {
            let (lines, filled) = (&unfocused[(x, y)], &focused[(x, y)]);
            assert_eq!(
                (lines.fg, lines.modifier),
                (Color::Reset, Modifier::DIM),
                "{x}, {y}"
            );
            assert_eq!(
                (filled.fg, filled.modifier),
                (Color::Reset, Modifier::empty()),
                "{x}, {y}"
            );
        }
        let name = &focused[(3, 0)];
        assert_eq!(name.symbol(), "L");
        assert!(name.modifier.contains(Modifier::REVERSED));
        assert!(!unfocused[(3, 0)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn a_pane_whose_entry_loads_shows_loading_under_its_header() {
        let state = session(r#"{"type": "code", "title": "Log", "content": "x"}"#);
        let area = Rect::new(0, 0, 40, 11);
        let mut buf = Buffer::empty(area);
        let gate = Gate::default();
        let mut viewers = Viewers::for_tests(Images::half_blocks());
        let theirs = gate.clone();
        viewers.building(std::sync::Arc::new(move |_, _, _, _| {
            theirs.wait();
            Box::new(Placeholder::note("empty"))
        }));
        viewers.waiting(std::time::Duration::ZERO);

        let drawn = draw_pane(
            area,
            &mut buf,
            &state,
            &mut viewers,
            quote_style(false),
            Ledge::Beside,
        );
        gate.open();

        assert!(drawn.expect("a pane").loading, "it says the entry loads");
        assert_eq!(
            row(&buf, 0),
            format!("╭─ Log {}", "─".repeat(33)),
            "no mark"
        );
        assert_eq!(row(&buf, 10), format!("╰{}", "─".repeat(39)), "no position");
        assert_eq!(
            row(&buf, 5),
            format!("│{}loading…", " ".repeat(16)),
            "the middle of the body's 9 rows and 38 columns"
        );
        assert_eq!(
            (1..10)
                .filter(|&y| !row(&buf, y).trim_start_matches('│').trim().is_empty())
                .count(),
            1,
            "nothing else in the body"
        );
    }

    #[test]
    fn a_pane_scrolled_past_the_end_of_a_taller_body_draws_its_last_page() {
        let mut state = thirty_lines();
        // A terminal 5 lines tall let the person scroll down to line 26.
        state.set_pane_page(Some(crate::tui::PanePage::shown(Page {
            rows: Extent {
                shown: 5,
                total: 30,
            },
            columns: Extent::default(),
        })));
        for code in [KeyCode::Tab, KeyCode::End] {
            state.handle(KeyEvent::new(code, KeyModifiers::NONE));
        }
        assert_eq!(state.pane_offset().top, 25);

        let area = Rect::new(0, 0, 40, 11);
        let mut buf = Buffer::empty(area);
        let mut viewers = Viewers::for_tests(Images::half_blocks());
        let drawn = draw_pane(
            area,
            &mut buf,
            &state,
            &mut viewers,
            quote_style(false),
            Ledge::Beside,
        );

        assert!(!drawn.expect("a pane").loading, "the entry loaded");
        assert!(
            row(&buf, 10).ends_with(" 22–30 of 30 ─"),
            "{:?}",
            row(&buf, 10)
        );
        assert_eq!(find(&buf, "line 22").1, 1);
    }
}
