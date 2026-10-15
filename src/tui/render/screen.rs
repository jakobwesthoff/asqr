// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The screen (spec sections 7.1 to 7.4, ADR 22): a title bar, the session
//! header, the tab bar, the current tab, a status line and the key bar.
//! Each part is drawn by its own module; this one lays them out, picks the
//! overlay and keeps what the screen remembers from one draw to the next.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use ratatui_image::picker::Picker;
use unicode_segmentation::UnicodeSegmentation;

use super::content::{TabState, draw_tab};
use super::context::draw_pane;
use super::empty::EmptyQueue;
use super::header::header_lines;
use super::keys::{draw_help, empty_queue_hints, intro_hints, key_hints, key_line};
use super::mascot::{Mascot, Which};
use super::overlay::{draw_intro, draw_session_list};
use super::panes::{PaneLayout, READING_WIDTH};
use super::source_line::columns;
use super::tabs::tab_bar;
use super::theme;
use super::wrap::cut;
use crate::format::ContextEntry;
use crate::tui::text_field::FieldShape;
use crate::tui::{App, Extent, Overlay, Page, SessionState};

/// Below this size the layout does not fit; a message says so instead.
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;

/// `text` in at most `room` columns: whole as long as it fits, otherwise
/// its end behind `…`, cut at a `/` where one is in reach, since the end
/// of a queue's path is what tells queues apart.
fn shorten_from_left(text: &str, room: usize) -> String {
    if columns(text) <= room {
        return text.to_owned();
    }
    // The end that fits behind the `…`, taken grapheme by grapheme from the
    // last one, so a wide character that would not fit stays out whole.
    let keep = room.saturating_sub(1);
    let mut start = text.len();
    let mut used = 0;
    for (index, grapheme) in text.grapheme_indices(true).rev() {
        used += columns(grapheme);
        if used > keep {
            break;
        }
        start = index;
    }
    let tail = &text[start..];
    let tail = match tail.find('/') {
        Some(slash) => &tail[slash..],
        None => tail,
    };
    format!("…{tail}")
}

/// What one draw measured that the app needs for the keys that follow: how
/// far the intro and the context pane can scroll depends on the size they
/// were drawn at, and where `↑`/`↓` go in a field on where its rows break.
///
/// A frame that shows no tab, as below the minimum size or with an empty
/// queue, says nothing about the intro or the pane: both are `None` then,
/// so a moment at a small size keeps the intro's scroll range, the pane's
/// focus and the full-screen view.
#[derive(Debug, Default)]
pub struct Drawn {
    /// What the intro overlay showed, for [`App::set_intro_page`]: an empty
    /// extent while it is closed.
    pub intro_page: Option<Extent>,
    /// What the context pane showed, for [`App::set_pane_page`]:
    /// `Some(None)` for a tab without a pane.
    pub pane_page: Option<Option<Page>>,
    /// Where the field or note that has the keys was drawn, for
    /// [`App::set_field_shape`].
    pub field_shape: Option<FieldShape>,
    /// The Kitty ids of the images that went since the last draw, for the
    /// terminal to delete once this frame is on the screen. This frame
    /// wrote other cells where they showed, so the terminal no longer
    /// needs them. Every frame says this, whatever it shows.
    pub deleted_images: Vec<u32>,
}

/// What the screen shows besides the state, and what it keeps from one
/// draw to the next.
pub struct View<'a> {
    /// The queue name, or its directory for a queue given by `--dir`.
    queue: &'a str,
    tab: TabState,
    /// Talqi, drawn while the queue is empty.
    mascot: Mascot,
    /// Whether the last draw showed the help, the intro or the session
    /// list over the screen.
    overlay_shown: bool,
    /// Whether the last draw showed an image entry in the context pane.
    image_shown: bool,
    /// Whether the screen is drawn without colours (`NO_COLOR`).
    no_color: bool,
}

impl<'a> View<'a> {
    /// The view of `queue`, with images sent in the graphics protocol
    /// `picker` speaks.
    pub fn new(queue: &'a str, layout: PaneLayout, picker: Picker) -> Self {
        View {
            queue,
            mascot: Mascot::new(picker.clone()),
            tab: TabState::new(layout, picker),
            overlay_shown: false,
            image_shown: false,
            no_color: false,
        }
    }

    /// This view, drawn without colours when `no_color` is set.
    #[must_use]
    pub fn no_color(self, no_color: bool) -> Self {
        View { no_color, ..self }
    }

    /// Whether the terminal must be cleared before `app` is drawn: an
    /// overlay that the last draw showed over an image entry is closed.
    ///
    /// The Sixel and iTerm2 protocols of `ratatui-image` mark the cells an
    /// image covers as skipped, so ratatui never writes them again, also
    /// not where an overlay's text stood over the image. A cleared terminal
    /// is drawn whole, image included. The image entry is the one the last
    /// draw showed under the overlay; a session picked in the list may
    /// have none, and then the clear only costs a full redraw.
    pub fn must_clear(&self, app: &App) -> bool {
        self.overlay_shown && self.image_shown && app.overlay().is_none()
    }

    /// The Kitty ids of every image of a context entry the terminal may
    /// still keep, each once, for deleting them before asqr leaves the
    /// alternate screen (ADR 42). Talqi's picture is not among them.
    pub fn take_kitty_images(&mut self) -> Vec<u32> {
        self.tab.viewers.take_all_images()
    }

    /// Reads and sends every image again as it is next shown (ADR 24).
    pub fn forget_images(&mut self) {
        self.tab.viewers.forget_images();
        self.mascot.forget();
    }
}

/// Draws `app` into `frame`. Under `NO_COLOR` the colours go after every
/// way [`draw_screen`] can end, the message of a too small terminal
/// included.
pub fn draw(frame: &mut Frame, app: &App, view: &mut View) -> Drawn {
    let mut drawn = draw_screen(frame, app, view);
    if view.no_color {
        theme::strip_colours(frame.buffer_mut());
    }
    drawn.deleted_images = view.tab.viewers.dropped_images();
    drawn
}

fn draw_screen(frame: &mut Frame, app: &App, view: &mut View) -> Drawn {
    // Whatever returns early draws no pane and no overlay.
    view.overlay_shown = false;
    view.image_shown = false;
    // Every frame lets the viewers of sessions that left go, also one that
    // shows no tab: the images of the last session to leave are deleted
    // once the frame without them is on the screen.
    view.tab.retain(app.sessions());
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = format!(
            "asqr needs at least {MIN_WIDTH}×{MIN_HEIGHT}; this terminal is {}×{}.",
            area.width, area.height
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return Drawn::default();
    }

    // The header text keeps to the reading width the question has, after
    // the margin on the left and before the one on the right.
    let header_width = usize::from(area.width.min(READING_WIDTH + 2));
    let header_lines = app
        .active()
        .map(|state| header_lines(state, header_width, area.height))
        .unwrap_or_default();
    let header_height = u16::try_from(header_lines.len()).expect("at most five header lines");

    let [title, header, tabs, main, status, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(header_height.max(1)),
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_title(title, frame.buffer_mut(), view.queue, app.sessions().len());
    // The intro overlay covers the status line, so the line comes first.
    frame.render_widget(status_line(app, app.active(), status.width), status);

    let overlay = app.overlay();
    let (drawn, hints) = match app.active() {
        None => {
            let which = if app.had_sessions() {
                Which::Sleeping
            } else {
                Which::Base
            };
            let empty = EmptyQueue {
                which,
                no_color: view.no_color,
            };
            frame.render_stateful_widget(empty, main, &mut view.mascot);
            (Drawn::default(), empty_queue_hints())
        }
        Some(state) => {
            frame.render_widget(Paragraph::new(header_lines), header);
            frame.render_widget(tab_bar(state), tabs);

            let main = main.inner(ratatui::layout::Margin::new(1, 0));
            let (pane_page, field_shape) = if state.full_screen() && state.shown_context().is_some()
            {
                (
                    draw_pane(main, frame.buffer_mut(), state, &mut view.tab.viewers),
                    None,
                )
            } else {
                draw_tab(main, frame.buffer_mut(), state, &mut view.tab)
            };
            view.image_shown = pane_page.is_some()
                && matches!(state.shown_entry(), Some((_, ContextEntry::Image(_))));

            // The intro overlay leaves the key bar free, which says how to
            // scroll and close it, so it is drawn before the key bar: what
            // of it fits decides the hints.
            let (intro_page, hints) = match overlay {
                Some(Overlay::Intro(scroll)) => {
                    let [above_keys, _] =
                        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
                    let intro = state.session().intro.as_deref();
                    let intro = intro.expect("the intro overlay is open only on an intro");
                    let page = draw_intro(above_keys, frame.buffer_mut(), intro, scroll);
                    (page, intro_hints(page))
                }
                Some(Overlay::SessionList(_)) => {
                    (Extent::default(), key_hints(state, true, pane_page))
                }
                Some(Overlay::Help) | None => {
                    (Extent::default(), key_hints(state, false, pane_page))
                }
            };
            let drawn = Drawn {
                intro_page: Some(intro_page),
                pane_page: Some(pane_page),
                field_shape,
                deleted_images: Vec::new(),
            };
            (drawn, hints)
        }
    };
    frame.render_widget(key_line(&hints, keys.width), keys);

    // These overlays use the whole screen, key bar included, so they fit
    // the smallest one. Like every overlay, they open only over a session.
    match overlay.zip(app.active()) {
        Some((Overlay::Help, state)) => draw_help(area, frame.buffer_mut(), state),
        Some((Overlay::SessionList(cursor), _)) => {
            draw_session_list(area, frame.buffer_mut(), app, cursor);
        }
        Some((Overlay::Intro(_), _)) | None => {}
    }
    view.overlay_shown = overlay.is_some();

    drawn
}

/// The title bar: the queue and how many sessions wait. A queue too long
/// for the bar gives way to the count, shortened from its start. The bar
/// has no fill: `asqr` stands out in the primary colour, the queue is
/// secondary, and the count, which changes, keeps the text colour.
fn draw_title(area: Rect, buf: &mut Buffer, queue: &str, waiting: usize) {
    let count = format!(
        " · {waiting} session{} waiting",
        if waiting == 1 { "" } else { "s" }
    );
    let (name, prefix) = (" asqr", " · queue: ");
    let room =
        usize::from(area.width).saturating_sub(columns(name) + columns(prefix) + columns(&count));
    Line::from(vec![
        Span::styled(name, theme::app_name()),
        Span::styled(
            format!("{prefix}{}", shorten_from_left(queue, room)),
            theme::secondary(),
        ),
        Span::from(count),
    ])
    .render(area, buf);
}

/// The status line: the app's notice, else the message of the session
/// shown, if there is one. A notice matters most over an empty queue: an
/// invalid session that was the only one leaves the queue empty, and the
/// notice is all that says why. Its tone gives the line its colour.
///
/// A notice that names a session is cut in the name, which ends in `…`
/// then, to what the rest of it leaves of `width`: the rest, such as a
/// confirmation's counts and the quotes around the name, shows whole.
/// Any other notice is cut at the edge.
fn status_line<'a>(app: &'a App, state: Option<&'a SessionState>, width: u16) -> Line<'a> {
    let Some(notice) = app
        .current_notice()
        .or_else(|| state.and_then(SessionState::message))
    else {
        return Line::default();
    };
    let style = theme::for_tone(notice.tone);
    let Some(name) = notice.name.clone() else {
        return Line::styled(notice.text.as_str(), style);
    };
    let (before, after) = (&notice.text[..name.start], &notice.text[name.end..]);
    let room = usize::from(width).saturating_sub(columns(before) + columns(after));
    let name = cut(&notice.text[name], room);
    Line::styled(format!("{before}{name}{after}"), style)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier, Style};

    use super::super::mascot::{self, Which};
    use super::super::test_support::{self, find, find_from, look_at, row, style_at};
    use super::*;
    use crate::format::{Session, result_answers};
    use crate::tui::{Focus, Notice, SessionState, Tone};

    const EXAMPLE: &str = include_str!("../../../examples/sessions/alt-rework-batch.json");
    const RELEASE: &str = include_str!("../../../examples/sessions/release-checklist.json");

    fn app(sessions: &[(&str, &str)]) -> App {
        let mut app = App::default();
        for (id, json) in sessions {
            let session: Session = serde_json::from_str(json).expect("example parses");
            app.add(SessionState::new(*id, session, None));
        }
        app
    }

    fn keys(app: &mut App, keys: &str) {
        for c in keys.chars() {
            app.handle(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    fn special(app: &mut App, code: KeyCode) {
        app.handle(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn screen(app: &App, width: u16, height: u16) -> TestBackend {
        screen_of_queue(app, "default", width, height)
    }

    fn screen_of_queue(app: &App, queue: &str, width: u16, height: u16) -> TestBackend {
        screen_in(
            app,
            &mut View::new(queue, PaneLayout::Auto, Picker::halfblocks()),
            width,
            height,
        )
    }

    /// Draws `app` with `view`, then hands it what the draw showed of the
    /// pane, as the run loop does after each frame.
    fn screen_handing_on_pane(
        app: &mut App,
        view: &mut View,
        width: u16,
        height: u16,
    ) -> TestBackend {
        let (backend, drawn) = measured_in(app, view, width, height);
        if let Some(page) = drawn.pane_page {
            app.set_pane_page(page);
        }
        backend
    }

    /// The screen drawn with `view`, which keeps what it remembers for the
    /// next draw.
    fn screen_in(app: &App, view: &mut View, width: u16, height: u16) -> TestBackend {
        measured_in(app, view, width, height).0
    }

    /// The screen drawn with `view`, and what the draw measured.
    fn measured_in(app: &App, view: &mut View, width: u16, height: u16) -> (TestBackend, Drawn) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        let mut drawn = Drawn::default();
        terminal
            .draw(|frame| drawn = draw(frame, app, view))
            .expect("draws");
        (terminal.backend().clone(), drawn)
    }

    fn title_line(backend: &TestBackend) -> String {
        row(backend.buffer(), 0)
    }

    // -----------------------------------------------------------------
    // Sizes
    // -----------------------------------------------------------------

    #[test]
    fn the_intro_shows_under_the_title_as_far_as_there_is_room() {
        let long_intro = RELEASE.replace(
            "Three decisions before the release. Skip what you cannot decide `yet`.",
            "Three decisions before the release. Skip what you cannot decide `yet`. \
             The **channels** question takes several answers, and the highlights \
             line goes into the release notes as it is typed, so write it the way \
             it should read on the blog. Nothing is published before you submit.",
        );
        let app = app(&[("release", &long_intro)]);

        for (width, height) in [(60, 15), (80, 24)] {
            insta::assert_snapshot!(
                format!("intro_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    /// The release session with a title and an intro that run far past the
    /// reading width.
    fn long_header() -> App {
        let session = RELEASE
            .replace(
                r#""title": "Release 0.2.0""#,
                r#""title": "Release 0.2.0 of the queue watcher, the new session format and the reworked review tab with its note field and the layout that keeps questions readable""#,
            )
            .replace(
                "Three decisions before the release. Skip what you cannot decide `yet`.",
                &"Three decisions before the release, each with its own question. ".repeat(4),
            );
        app(&[("release", &session)])
    }

    #[test]
    fn the_header_wraps_at_the_reading_width_and_the_tab_bar_does_not() {
        let backend = screen(&long_header(), 160, 30);
        let lines = test_support::rows(backend.buffer());

        for (row, line) in lines.iter().enumerate().take(6).skip(1) {
            let used = line.trim_end().chars().count();
            assert!(used <= 101, "header row {row} ends at {used}:\n{backend}");
        }
        insta::assert_snapshot!("header_wrapped_160x30", backend);

        let questions: Vec<String> = (1..=20)
            .map(|n| format!(r#"{{"id": "question-{n}", "text": "?", "kind": "text"}}"#))
            .collect();
        let many_tabs = app(&[(
            "tabs",
            &format!(r#"{{"asqr": 2, "questions": [{}]}}"#, questions.join(",")),
        )]);
        let tab_bar = test_support::rows(screen(&many_tabs, 160, 30).buffer())[2].clone();
        assert!(
            tab_bar.trim_end().chars().count() > 150,
            "the tab bar uses the full width: {tab_bar}"
        );
    }

    #[test]
    fn a_long_title_takes_two_lines_or_one_on_short_terminals() {
        let app = long_header();
        let lines = test_support::rows(screen(&app, 80, 24).buffer());
        assert!(lines[2].trim_end().ends_with('…'), "{lines:#?}");
        assert!(lines[5].trim_end().ends_with("… i: more"), "{lines:#?}");

        let lines = test_support::rows(screen(&app, 80, 20).buffer());
        assert!(lines[1].trim_end().ends_with('…'), "{lines:#?}");
        assert!(lines[2].trim_end().ends_with("… i: more"), "{lines:#?}");
    }

    /// A session whose intro takes far more lines than a 60×15 screen.
    fn longer_intro() -> App {
        let intro = (1..=8)
            .map(|round| {
                format!(
                    "- Round {round} asked about the **queue watcher** and the review tab, \
                     and the answers went into the plan."
                )
            })
            .collect::<Vec<_>>()
            .join("\\n");
        app(&[(
            "release",
            &RELEASE.replace(
                "Three decisions before the release. Skip what you cannot decide `yet`.",
                &intro,
            ),
        )])
    }

    /// Draws `app` with `view`, then hands the app what the intro overlay
    /// showed, as the running app does after every draw.
    fn drawn(app: &mut App, view: &mut View, width: u16, height: u16) -> TestBackend {
        let (backend, drawn) = measured_in(app, view, width, height);
        if let Some(page) = drawn.intro_page {
            app.set_intro_page(page);
        }
        app.set_field_shape(drawn.field_shape);
        backend
    }

    #[test]
    fn i_shows_the_whole_intro_and_scrolls_it_on_a_small_screen() {
        let mut app = longer_intro();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        keys(&mut app, "i");
        insta::assert_snapshot!("intro_overlay_top", drawn(&mut app, &mut view, 60, 15));

        keys(&mut app, "jjj");
        insta::assert_snapshot!("intro_overlay_scrolled", drawn(&mut app, &mut view, 60, 15));

        keys(&mut app, "G");
        insta::assert_snapshot!("intro_overlay_end", drawn(&mut app, &mut view, 60, 15));

        keys(&mut app, "i");
        let (backend, closed) = measured_in(&app, &mut view, 60, 15);
        assert!(!backend.to_string().contains("Intro"), "{backend}");
        assert_eq!(closed.intro_page, Some(Extent::default()));
    }

    #[test]
    fn an_intro_that_fits_shows_without_a_position() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "i");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_question_adapts_to_every_terminal_size() {
        let app = app(&[("release", RELEASE)]);
        for (width, height) in [(60, 15), (80, 24), (120, 40), (200, 50)] {
            insta::assert_snapshot!(
                format!("question_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    #[test]
    fn the_review_shows_labels_and_text_cut_to_the_line() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "223");
        special(&mut app, KeyCode::Enter);
        keys(
            &mut app,
            "Queue scans are faster and the review shows what you picked",
        );
        special(&mut app, KeyCode::Enter);

        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    #[test]
    fn the_review_adapts_to_every_terminal_size() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");
        for (width, height) in [(60, 15), (100, 30)] {
            insta::assert_snapshot!(
                format!("review_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    #[test]
    fn a_small_screen_keeps_the_cursor_row_in_view() {
        let options: Vec<String> = (1..=12)
            .map(|n| format!(r#"{{"id": "o{n}", "label": "Option {n}", "description": "What option {n} means."}}"#))
            .collect();
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Which one?", "kind": "single", "custom": true,
                "options": [{}]}}]}}"#,
            options.join(",")
        );
        let mut app = app(&[("many", &json)]);
        for _ in 0..12 {
            special(&mut app, KeyCode::Down);
        }
        insta::assert_snapshot!(screen(&app, 60, 15));

        // A wrapped own answer counts with all its lines.
        keys(&mut app, LONG_OWN_ANSWER);
        insta::assert_snapshot!("cursor_row_wrapped_typed", screen(&app, 60, 15));
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!("cursor_row_wrapped_left", screen(&app, 60, 15));
    }

    #[test]
    fn a_long_session_scrolls_the_tab_bar_to_the_current_tab() {
        let questions: Vec<String> = (1..=40)
            .map(|n| format!(r#"{{"id": "q{n}", "text": "Question {n}?", "kind": "single", "options": [{{"id": "a", "label": "A"}}]}}"#))
            .collect();
        let json = format!(r#"{{"asqr": 2, "questions": [{}]}}"#, questions.join(","));
        let mut app = app(&[("long", &json)]);
        for _ in 0..30 {
            special(&mut app, KeyCode::Right);
        }
        insta::assert_snapshot!(screen(&app, 80, 16));
    }

    #[test]
    fn narrow_screens_put_descriptions_below_the_label() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Which approach?", "kind": "single",
            "options": [
                {"id": "a", "label": "Rewrite the parser from scratch", "description": "Cleanest result, takes the longest."},
                {"id": "b", "label": "Patch the tokenizer", "description": "Quick, but the grammar stays odd."}]}]}"#;
        let app = app(&[("approach", json)]);

        insta::assert_snapshot!("descriptions_below_60x15", screen(&app, 60, 15));
        insta::assert_snapshot!("descriptions_beside_120x15", screen(&app, 120, 15));
    }

    #[test]
    fn an_option_with_an_empty_description_still_shows() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Which one?", "kind": "single",
            "options": [
                {"id": "a", "label": "Alpha option", "description": ""},
                {"id": "b", "label": "Beta option", "description": "   "},
                {"id": "c", "label": "Gamma option", "description": "Has a description."}]}]}"#;
        let app = app(&[("probe", json)]);

        let text = format!("{:?}", screen(&app, 120, 20));
        assert!(text.contains("Gamma option"), "control option shows");
        assert!(
            text.contains("Alpha option"),
            "option with empty description shows"
        );
        assert!(
            text.contains("Beta option"),
            "option with blank description shows"
        );
    }

    // -----------------------------------------------------------------
    // Markdown in the intro, question texts and option descriptions
    // -----------------------------------------------------------------

    /// A session with `intro` and one question of `text`, whose first
    /// option is labelled `label` and described by `description`.
    fn markdown_session(intro: &str, text: &str, label: &str, description: &str) -> App {
        let json = serde_json::json!({
            "asqr": 2,
            "title": "Markdown",
            "intro": intro,
            "questions": [{"id": "q", "text": text, "kind": "single", "options": [
                {"id": "a", "label": label, "description": description},
                {"id": "b", "label": "Beta"}
            ]}]
        });
        app(&[("markdown", &json.to_string())])
    }

    const PLAN: &str = "## Plan\n\nRun `cargo test` **now**, see [docs](https://example.com).\n\n> Quoted\n\n```rust\nlet x = 1;\n```";

    #[test]
    fn a_question_text_renders_full_markdown_in_patines_styles() {
        let backend = screen(&markdown_session("Intro", PLAN, "Alpha", ""), 100, 30);
        let buffer = backend.buffer();
        let text = backend.to_string();

        for source in ["##", "**", "```", "[docs]", "> Quoted"] {
            assert!(!text.contains(source), "{source} shows:\n{backend}");
        }
        let heading = find(backend.buffer(), "Plan");
        assert!(
            buffer[heading]
                .modifier
                .contains(Modifier::BOLD | Modifier::UNDERLINED),
            "a level-two heading"
        );
        let code = find(backend.buffer(), "cargo test");
        assert_eq!(buffer[code].fg, Color::Indexed(3), "inline code in yellow");
        assert!(
            !buffer[code].modifier.contains(Modifier::REVERSED),
            "inline code is not reversed"
        );
        assert!(
            buffer[find(backend.buffer(), "now")]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            buffer[find(backend.buffer(), "docs")]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        assert!(
            buffer[find(backend.buffer(), "(https://example.com)")]
                .modifier
                .contains(Modifier::DIM)
        );
        let (x, y) = find(backend.buffer(), "│ Quoted");
        assert!(buffer[(x, y)].modifier.contains(Modifier::DIM), "the bar");
        assert!(buffer[(x + 2, y)].modifier.contains(Modifier::ITALIC));
        let (x, y) = find(backend.buffer(), "let x = 1;");
        assert_eq!(x, 3, "the block is indented by two after the margin");
        assert_eq!(
            buffer[(x, y)].fg,
            Color::Indexed(5),
            "a highlighted keyword"
        );
    }

    #[test]
    fn a_wide_code_line_in_a_question_wraps_behind_a_marker() {
        let code = "let total = first + second + third + fourth + fifth + sixth + seventh;";
        let backend = screen(
            &markdown_session("Intro", &format!("```\n{code}\n```"), "Alpha", ""),
            60,
            24,
        );
        let buffer = backend.buffer();

        let (x, y) = find(backend.buffer(), "let total");
        assert_eq!(x, 3);
        let first = &test_support::rows(backend.buffer())[usize::from(y)];
        let second = &test_support::rows(backend.buffer())[usize::from(y) + 1];
        assert!(second.starts_with("   ↪ "), "{backend}");
        assert!(
            buffer[(3, y + 1)].modifier.contains(Modifier::DIM),
            "the marker"
        );
        let shown = format!(
            "{}{}",
            first[3..].trim_end(),
            second["   ↪ ".len()..].trim_end()
        );
        assert_eq!(shown, code, "nothing is cut:\n{backend}");
        assert!(!backend.to_string().contains('›'), "{backend}");
    }

    #[test]
    fn the_intro_is_dim_over_its_markdown_styles() {
        let app = markdown_session("Round **two** of `layout`.", "Which?", "Alpha", "");
        let backend = screen(&app, 80, 24);
        let buffer = backend.buffer();

        let two = find(backend.buffer(), "two");
        assert!(
            buffer[two]
                .modifier
                .contains(Modifier::BOLD | Modifier::DIM),
            "{backend}"
        );
        let layout = find(backend.buffer(), "layout");
        assert_eq!(buffer[layout].fg, Color::Indexed(3));
        assert!(buffer[layout].modifier.contains(Modifier::DIM));
        assert!(!backend.to_string().contains('`'), "{backend}");
    }

    #[test]
    fn the_intro_overlay_shows_the_markdown_styles_without_dim() {
        let mut app = markdown_session("Round **two** of `layout`.", "Which?", "Alpha", "");
        keys(&mut app, "i");
        let backend = screen(&app, 80, 24);
        let buffer = backend.buffer();

        let (_, y) = find(backend.buffer(), "│ Round two");
        let two = find_from(backend.buffer(), y, "two");
        assert!(buffer[two].modifier.contains(Modifier::BOLD), "{backend}");
        assert!(!buffer[two].modifier.contains(Modifier::DIM), "{backend}");
        let (layout, _) = find_from(backend.buffer(), y, "layout");
        assert_eq!(buffer[(layout, y)].fg, Color::Indexed(3));
    }

    const STEPS: &str = "Keeps **all** of it:\n\n- `first` step\n- second step";

    #[test]
    fn a_description_beside_its_label_renders_markdown_under_its_own_start() {
        let backend = screen(
            &markdown_session("Intro", "Which?", "Alpha", STEPS),
            120,
            20,
        );
        let buffer = backend.buffer();
        let lines = test_support::rows(backend.buffer());

        // The margin, then "❯ ( ) 1. Alpha" and two blanks.
        let (x, y) = find(backend.buffer(), "Keeps all of it:");
        assert_eq!(x, 17, "{backend}");
        assert!(
            lines[usize::from(y)].contains("Alpha"),
            "on the label's row"
        );
        assert!(
            buffer[find(backend.buffer(), "all")]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(
            find(backend.buffer(), "• first step"),
            (17, y + 2),
            "{backend}"
        );
        assert_eq!(
            find(backend.buffer(), "• second step"),
            (17, y + 3),
            "{backend}"
        );
        assert_eq!(buffer[(19, y + 2)].fg, Color::Indexed(3), "inline code");
        assert!(lines[usize::from(y) + 4].contains("Beta"), "{backend}");
    }

    #[test]
    fn a_description_below_its_label_renders_markdown_from_column_six() {
        let label = "Rewrite the parser from scratch, every part of it";
        let backend = screen(&markdown_session("Intro", "Which?", label, STEPS), 60, 24);

        let (_, label_row) = find(backend.buffer(), label);
        assert_eq!(
            find(backend.buffer(), "Keeps all of it:"),
            (7, label_row + 1),
            "{backend}"
        );
        assert_eq!(
            find(backend.buffer(), "• first step"),
            (7, label_row + 3),
            "{backend}"
        );
    }

    #[test]
    fn a_wide_code_line_in_a_description_wraps_inside_the_description_column() {
        let description = format!("```\n{}\n```", "word ".repeat(30));
        let backend = screen(
            &markdown_session("Intro", "Which?", "Alpha", &description),
            120,
            20,
        );

        let (x, y) = find(backend.buffer(), "word word");
        assert_eq!(x, 19, "the block's indent inside the description column");
        assert_eq!(
            find_from(backend.buffer(), y + 1, "↪"),
            (19, y + 1),
            "{backend}"
        );
        assert!(!backend.to_string().contains('›'), "{backend}");
    }

    #[test]
    fn single_newlines_break_and_raw_html_shows_as_written() {
        let text = "First line\nsecond line, which returns a Vec<String>\n\n<details>\nmore\n</details>\n\n- an item\n  that goes on";
        let description = "Short.\nKeeps <b>tags</b>.";
        let app = markdown_session("One\ntwo", text, "Alpha", description);

        insta::assert_snapshot!("newlines_and_html_80x24", screen(&app, 80, 24));
    }

    #[test]
    fn a_question_with_an_image_placeholder_at_60_columns() {
        insta::assert_snapshot!(screen(&app(&[("batch", EXAMPLE)]), 60, 24));
    }

    #[test]
    fn a_small_terminal_says_what_it_needs() {
        insta::assert_snapshot!(screen(&App::default(), 40, 10));
    }

    #[test]
    fn an_empty_queue_says_so() {
        // 17 rows leave the main area 11, one short of Talqi.
        insta::assert_snapshot!(screen(&App::default(), 80, 17));
    }

    #[test]
    fn a_long_queue_path_gives_way_to_the_count() {
        let path = "/private/tmp/some/deeply/nested/scratch/directory/of/a/session/queue";
        let app = App::default();

        assert_eq!(
            title_line(&screen_of_queue(&app, path, 60, 15)),
            " asqr · queue: …/of/a/session/queue · 0 sessions waiting"
        );
        assert_eq!(
            title_line(&screen_of_queue(&app, &"x".repeat(80), 60, 15)),
            format!(" asqr · queue: …{} · 0 sessions waiting", "x".repeat(23)),
            "a name without separators is cut anywhere"
        );
        assert_eq!(
            title_line(&screen_of_queue(&app, "default", 60, 15)),
            " asqr · queue: default · 0 sessions waiting",
            "a title that fits stays as it is"
        );
    }

    #[test]
    fn an_empty_queue_still_shows_notices() {
        let mut app = App::default();
        app.notice(
            Tone::Warning,
            "broken is invalid and was answered with an error: questions[0].min",
        );

        insta::assert_snapshot!(screen(&app, 80, 17));
    }

    /// A view whose terminal speaks the Kitty protocol.
    fn kitty_view() -> View<'static> {
        View::new("q", PaneLayout::Auto, mascot::tests::kitty())
    }

    #[test]
    fn talqi_is_awake_until_sessions_came_in_and_asleep_after() {
        let mut app = App::default();
        let mut view = kitty_view();

        screen_in(&app, &mut view, 80, 24);
        assert_eq!(view.mascot.shown(), Some(Which::Base));

        app.add(SessionState::new(
            "release",
            serde_json::from_str(RELEASE).expect("example parses"),
            None,
        ));
        app.remove("release");
        screen_in(&app, &mut view, 80, 24);
        assert_eq!(view.mascot.shown(), Some(Which::Sleeping));
    }

    #[test]
    fn notices_show_over_the_empty_queue_with_talqi() {
        let mut app = App::default();
        app.notice(
            Tone::Warning,
            "broken is invalid and was answered with an error",
        );

        let backend = screen_in(&app, &mut kitty_view(), 80, 24);

        let buf = backend.buffer();
        assert!(mascot::tests::transmission(buf).is_some(), "Talqi is drawn");
        assert_eq!(
            row(buf, 22),
            "broken is invalid and was answered with an error"
        );
    }

    #[test]
    fn talqi_is_sent_again_after_the_images_are_forgotten() {
        let app = App::default();
        let mut view = kitty_view();
        let sent = |view: &mut View| {
            mascot::tests::transmission(screen_in(&app, view, 80, 24).buffer()).is_some()
        };

        assert!(sent(&mut view), "first draw");
        assert!(!sent(&mut view), "kept");
        view.forget_images();
        assert!(sent(&mut view), "sent again");
    }

    // -----------------------------------------------------------------
    // Fields
    // -----------------------------------------------------------------

    /// An own answer for the release session's channels question that
    /// needs several lines in every supported width.
    const LONG_OWN_ANSWER: &str = "a mailing list, the release channel, the team chat \
        and a short post on the project page, written by whoever has the time on the day";

    /// The release session with `LONG_OWN_ANSWER` typed into the own answer
    /// of its channels question, the field still focused.
    fn typed_own_answer() -> App {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ljjj");
        keys(&mut app, LONG_OWN_ANSWER);
        app
    }

    #[test]
    fn the_own_answer_wraps_under_its_start_while_typed() {
        let app = typed_own_answer();
        for (width, height) in [(60, 15), (80, 24), (120, 24)] {
            insta::assert_snapshot!(
                format!("own_answer_typed_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    #[test]
    fn a_left_own_answer_shows_completely_on_wrapped_lines() {
        let mut app = typed_own_answer();
        special(&mut app, KeyCode::Up);
        for (width, height) in [(60, 15), (80, 24), (120, 24)] {
            insta::assert_snapshot!(
                format!("own_answer_left_{width}x{height}"),
                screen(&app, width, height)
            );
        }
    }

    /// The reversed cells in the tab's area, which lies between the tab bar
    /// and the status line: the text cursor, or a highlighted row.
    fn reversed_cells_in_tab(backend: &TestBackend) -> Vec<(u16, u16, String)> {
        let buffer = backend.buffer();
        (5..buffer.area.height - 2)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].modifier.contains(Modifier::REVERSED))
            .map(|(x, y)| (x, y, buffer[(x, y)].symbol().to_owned()))
            .collect()
    }

    #[test]
    fn the_text_cursor_shows_on_its_wrapped_line() {
        let mut app = typed_own_answer();
        // Back to the `c` of "channel", the first word that does not fit
        // on the first line at 60 columns.
        let channel = LONG_OWN_ANSWER.find("channel").expect("the word is there");
        for _ in channel..LONG_OWN_ANSWER.chars().count() {
            special(&mut app, KeyCode::Left);
        }
        let backend = screen(&app, 60, 24);
        let buffer = backend.buffer();

        let cursor = reversed_cells_in_tab(&backend);
        assert_eq!(cursor.len(), 1, "only the cursor is reversed:\n{backend}");
        let (x, y, symbol) = &cursor[0];
        assert_eq!(symbol, "c", "{backend}");
        // The text starts after the margin and the 24 columns of
        // "❯ [x] ✎ Somewhere else: ", and the second line under it.
        assert_eq!(*x, 25, "{backend}");
        assert_eq!(buffer[(*x, y - 1)].symbol(), "a", "{backend}");
    }

    #[test]
    fn up_and_down_move_the_text_cursor_through_the_rows_on_screen() {
        // The text fills its last row at 60 columns, so the cursor after it
        // shows on a row of its own.
        let mut app = typed_own_answer();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        let mut cursor = |app: &mut App| {
            let backend = drawn(app, &mut view, 60, 24);
            let cells = reversed_cells_in_tab(&backend);
            assert_eq!(cells.len(), 1, "only the cursor is reversed:\n{backend}");
            (cells[0].0, cells[0].1)
        };
        let (x, y) = cursor(&mut app);

        special(&mut app, KeyCode::Up);
        let (above, row_above) = cursor(&mut app);
        assert_eq!(row_above, y - 1, "one row up, still in the field");
        assert!(above <= x, "at the column it was in, or before");
        special(&mut app, KeyCode::Down);

        assert_eq!(cursor(&mut app), (x, y), "back where it was");
    }

    /// A single question whose own answer has `label` and holds the
    /// typed `LONG_OWN_ANSWER`, the field still focused.
    fn own_answer_labelled(label: &str) -> App {
        let json = format!(
            r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Name it", "kind": "single",
                "custom": {{"label": {label:?}}}, "options": [{{"id": "a", "label": "asqr"}}]}}]}}"#
        );
        let mut app = app(&[("name", &json)]);
        keys(&mut app, "j");
        keys(&mut app, LONG_OWN_ANSWER);
        app
    }

    #[test]
    fn a_head_leaving_less_than_20_columns_wraps_the_text_under_the_pointer() {
        // The head is the pointer, the mark and `✎ ` (8 columns), the
        // label and `: `; the text area of a 60-column screen is 58 wide.
        for (label_length, indent) in [(28, 38), (29, 4)] {
            let backend = screen(&own_answer_labelled(&"x".repeat(label_length)), 60, 24);
            let lines = test_support::rows(backend.buffer());

            let head = lines
                .iter()
                .position(|line| line.contains('✎'))
                .expect("the own answer is on screen");
            let wrapped = &lines[head + 1];
            let leading = wrapped.chars().take_while(|c| *c == ' ').count();
            assert_eq!(
                leading,
                1 + indent,
                "a label of {label_length} wraps after {indent}:\n{backend}"
            );
        }
    }

    #[test]
    fn a_long_own_answer_label_keeps_the_text_readable() {
        let app = own_answer_labelled("Your own suggestion for the name of it");
        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    /// The release session on its review, the cursor on `row` below
    /// Submit: 1 for the session note, 2 for Reject.
    fn review_row_below_submit(row: usize) -> App {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");
        for _ in 0..row {
            special(&mut app, KeyCode::Down);
        }
        app
    }

    #[test]
    fn a_long_session_note_wraps_under_its_start() {
        let mut app = review_row_below_submit(1);
        keys(
            &mut app,
            "fine to ship once the changelog names the new format and the skill is updated",
        );
        insta::assert_snapshot!("session_note_typed_60x15", screen(&app, 60, 15));

        special(&mut app, KeyCode::Up);
        insta::assert_snapshot!("session_note_left_60x24", screen(&app, 60, 24));
    }

    #[test]
    fn a_long_reject_reason_wraps_under_its_start() {
        let mut app = review_row_below_submit(2);
        keys(
            &mut app,
            "the release waits for the security fix, which lands next week at the earliest",
        );
        insta::assert_snapshot!("reject_reason_typed_60x15", screen(&app, 60, 15));

        // Up past the session note, which would take the focus, to Submit.
        special(&mut app, KeyCode::Up);
        special(&mut app, KeyCode::Up);
        insta::assert_snapshot!("reject_reason_left_60x24", screen(&app, 60, 24));
    }

    #[test]
    fn the_session_note_and_the_reject_reason_show_their_lines() {
        let mut app = review_row_below_submit(1);
        keys(&mut app, "fine to ship");
        ctrl_j(&mut app);
        keys(&mut app, "once the changelog names the new format");
        special(&mut app, KeyCode::Down);
        keys(&mut app, "stale");
        ctrl_j(&mut app);
        keys(&mut app, "ask again");
        insta::assert_snapshot!(screen(&app, 60, 24));
    }

    #[test]
    fn line_breaks_in_an_own_answer_show_as_marks_in_the_review() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ljjjblog");
        ctrl_j(&mut app);
        keys(&mut app, "post");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");

        let backend = screen(&app, 80, 24);

        find(backend.buffer(), "\"blog ⏎ post\"");
    }

    #[test]
    fn a_text_answer_shows_its_lines_and_the_counter() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        keys(&mut app, "Ship the queue watcher");
        app.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        keys(&mut app, "and the new format.");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_note_is_written_in_its_line() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "nafter the weekend");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    /// The colour of the counter of the release session's highlights,
    /// whose target ends at 120.
    fn counter_colour(app: &App) -> Color {
        let backend = screen(app, 80, 24);
        backend.buffer()[find(backend.buffer(), "/120")].fg
    }

    #[test]
    fn the_counter_colour_follows_the_length() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "lla");
        assert_eq!(counter_colour(&app), Color::Green);

        keys(&mut app, &"b".repeat(125));
        assert_eq!(
            counter_colour(&app),
            Color::Yellow,
            "above the target of 120"
        );

        keys(&mut app, &"c".repeat(20));
        assert_eq!(counter_colour(&app), Color::Red, "above warn at 140");
    }

    // -----------------------------------------------------------------
    // Overlays and feedback
    // -----------------------------------------------------------------

    #[test]
    fn the_help_fits_the_smallest_screen() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "?");
        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    #[test]
    fn the_session_list_shows_every_waiting_session() {
        let mut app = app(&[("batch", EXAMPLE), ("release", RELEASE)]);
        keys(&mut app, "Lj");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_session_list_taller_than_the_screen_fills_it_and_scrolls_to_the_cursor() {
        let sessions: Vec<(String, String)> = (0..20)
            .map(|n| {
                let title = format!("s{n:02}");
                let json = RELEASE.replace("Release 0.2.0", &title);
                (title, json)
            })
            .collect();
        let sessions: Vec<(&str, &str)> = sessions
            .iter()
            .map(|(id, json)| (id.as_str(), json.as_str()))
            .collect();
        let mut app = app(&sessions);
        keys(&mut app, &format!("L{}", "j".repeat(15)));

        // Twenty sessions and the border want 22 rows; the frame takes the
        // screen's 15, 56 columns wide in the middle, and the 13 rows inside
        // end with the cursor's session.
        let backend = screen(&app, 60, 15);
        let buf = backend.buffer();
        assert_eq!(find(buf, "╭ Waiting sessions"), (2, 0), "{backend}");
        assert_eq!(find(buf, "╰"), (2, 14), "{backend}");
        assert_eq!(find(buf, "╮"), (57, 0), "{backend}");
        assert_eq!(find(buf, "s03"), (6, 1), "{backend}");
        assert_eq!(find(buf, "❯ s15"), (4, 13), "{backend}");
    }

    #[test]
    fn messages_and_notices_show_in_the_status_line() {
        // With `max` 2 and the default ticked, the third pick is refused.
        let mut app = app(&[("release", &RELEASE.replace(r#""max": 3"#, r#""max": 2"#))]);
        keys(&mut app, "l23");
        insta::assert_snapshot!("message", screen(&app, 80, 24));

        app.notice(
            Tone::Warning,
            "batch-01 collided with an unread result and was archived",
        );
        insta::assert_snapshot!("notice", screen(&app, 80, 24));
    }

    #[test]
    fn the_empty_queue_has_a_yellow_notice_and_only_quit_in_the_key_bar() {
        let mut app = App::default();
        let backend = screen(&app, 80, 20);
        assert_eq!(row(backend.buffer(), 18), "", "{backend}");

        app.notice(
            Tone::Warning,
            "broken is invalid and was answered with an error",
        );
        let backend = screen(&app, 80, 20);
        let buf = backend.buffer();
        assert_eq!(
            row(buf, 18),
            "broken is invalid and was answered with an error",
            "{backend}"
        );
        assert_eq!(style_at(buf, "broken").fg, Some(Color::Yellow));
        assert_eq!(row(buf, 19), " q  quit", "{backend}");
        assert_eq!(look_at(buf, "q  quit"), (Color::Cyan, Modifier::BOLD));
    }

    #[test]
    fn the_title_line_has_no_fill_and_names_asqr_in_the_primary_colour() {
        let backend = screen(&app(&[("release", RELEASE)]), 80, 24);
        let buf = backend.buffer();

        assert_eq!(
            title_line(&backend),
            " asqr · queue: default · 1 session waiting"
        );
        assert_eq!(look_at(buf, "asqr"), (Color::Cyan, Modifier::BOLD));
        let secondary = (Color::Reset, Modifier::DIM);
        assert_eq!(look_at(buf, " · queue"), secondary);
        assert_eq!(look_at(buf, "default"), secondary);
        assert_eq!(
            look_at(buf, " · 1 session"),
            (Color::Reset, Modifier::empty())
        );
        assert!(
            (0..80).all(|x| !buf[(x, 0)].modifier.contains(Modifier::REVERSED)),
            "{backend}"
        );
    }

    #[test]
    fn one_focus_fill_shows_at_a_time() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        let filled = |backend: &TestBackend, row: u16| {
            (0..80)
                .filter(|&x| {
                    let cell = &backend.buffer()[(x, row)];
                    cell.modifier.contains(Modifier::REVERSED) && cell.fg == Color::Yellow
                })
                .count()
        };

        let backend = screen_handing_on_pane(&mut app, &mut view, 80, 24);
        let (option, header) = (
            row_starting_with(&backend, "❯ ( ) 1. Yes"),
            row_starting_with(&backend, "Thirty lines"),
        );
        assert_eq!(filled(&backend, option), 12, "the option:\n{backend}");
        assert_eq!(filled(&backend, header), 0, "the header:\n{backend}");

        special(&mut app, KeyCode::Tab);
        let backend = screen_handing_on_pane(&mut app, &mut view, 80, 24);
        assert_eq!(filled(&backend, option), 0, "the option:\n{backend}");
        assert!(row(backend.buffer(), option).contains("❯ ( ) 1. Yes"));
        assert_eq!(filled(&backend, header), 78, "the header:\n{backend}");
    }

    #[test]
    fn a_notice_shows_over_the_session_message_and_the_key_bar_stays() {
        // With `max` 2 and the default ticked, the third pick is refused.
        let mut app = app(&[("release", &RELEASE.replace(r#""max": 3"#, r#""max": 2"#))]);
        keys(&mut app, "l23");
        let message = app
            .active()
            .and_then(SessionState::message)
            .expect("the third pick is refused")
            .text
            .clone();
        let key_bar =
            " ↑/↓  move   space  toggle   enter  next   ←/→  question   ?  help   q  quit";

        let backend = screen(&app, 80, 24);
        let buf = backend.buffer();
        assert_eq!(row(buf, 22), message, "{backend}");
        assert_eq!(style_at(buf, &message).fg, Some(Color::Yellow));
        assert_eq!(row(buf, 23), key_bar, "{backend}");

        app.notice(
            Tone::Warning,
            "batch-01 collided with an unread result and was archived",
        );
        let backend = screen(&app, 80, 24);
        let buf = backend.buffer();
        assert_eq!(
            row(buf, 22),
            "batch-01 collided with an unread result and was archived",
            "{backend}"
        );
        assert_eq!(style_at(buf, "batch-01").fg, Some(Color::Yellow));
        assert_eq!(row(buf, 23), key_bar, "{backend}");
    }

    #[test]
    fn the_status_line_prefers_the_notice_to_the_session_message() {
        let text = |line: Line| line.to_string();
        let mut app = app(&[("release", &RELEASE.replace(r#""max": 3"#, r#""max": 2"#))]);
        let state = app.active().cloned();
        assert_eq!(text(status_line(&app, None, 80)), "");
        assert_eq!(text(status_line(&app, state.as_ref(), 80)), "");

        keys(&mut app, "l23");
        let state = app.active().cloned();
        let line = status_line(&app, state.as_ref(), 80);
        assert_eq!(text(line.clone()), "at most 2 options");
        assert_eq!(line.style.fg, Some(Color::Yellow));
        assert_eq!(
            text(status_line(&app, None, 80)),
            "",
            "without a session only a notice shows"
        );

        app.notice(
            Tone::Warning,
            "batch-01 collided with an unread result and was archived",
        );
        let expected = "batch-01 collided with an unread result and was archived";
        let line = status_line(&app, state.as_ref(), 80);
        assert_eq!(text(line.clone()), expected);
        assert_eq!(line.style.fg, Some(Color::Yellow));
        assert_eq!(text(status_line(&app, None, 80)), expected);
    }

    #[test]
    fn the_status_line_colours_a_notice_by_its_tone() {
        let mut app = App::default();
        let tones = [
            (Tone::Info, Style::new()),
            (Tone::Warning, Style::new().fg(Color::Yellow)),
            (Tone::Error, Style::new().fg(Color::Red)),
            (Tone::Success, Style::new().fg(Color::Green)),
        ];
        for (tone, style) in tones {
            app.notice(tone, "news");
            let line = status_line(&app, None, 80);
            assert_eq!(line.to_string(), "news");
            assert_eq!(line.style, style, "{tone:?}");
        }
    }

    #[test]
    fn the_status_line_cuts_a_name_to_the_room_the_rest_leaves() {
        let mut app = App::default();
        app.show(Notice::naming(
            Tone::Success,
            "✔ sent \"",
            "Release 0.2.0",
            "\" · 1 answered, 1 skipped",
        ));
        let text = |width: u16| status_line(&app, None, width).to_string();

        assert_eq!(text(80), "✔ sent \"Release 0.2.0\" · 1 answered, 1 skipped");
        // `✔ sent "` and `" · 1 answered, 1 skipped` take 33 columns.
        assert_eq!(text(40), "✔ sent \"Releas…\" · 1 answered, 1 skipped");
        assert_eq!(text(34), "✔ sent \"…\" · 1 answered, 1 skipped");
        assert_eq!(
            text(33),
            "✔ sent \"\" · 1 answered, 1 skipped",
            "no room for the name"
        );
        assert_eq!(
            status_line(&app, None, 40).style,
            Style::new().fg(Color::Green)
        );
    }

    #[test]
    fn a_reloaded_session_says_so_in_plain_text() {
        let mut app = app(&[("release", RELEASE)]);
        let replaced: Session = serde_json::from_str(RELEASE).expect("example parses");
        app.add(SessionState::new("release", replaced, None));

        let line = status_line(&app, app.active(), 80);
        assert_eq!(
            line.to_string(),
            "Release 0.2.0 changed on disk and was reloaded"
        );
        assert_eq!(line.style, Style::new());
    }

    #[test]
    fn the_intro_overlay_covers_the_status_line_and_leaves_the_key_bar() {
        let mut app = longer_intro();
        keys(&mut app, "i");
        app.notice(
            Tone::Warning,
            "batch-01 collided with an unread result and was archived",
        );

        // The notice shows only in the margin beside the intro's frame.
        let backend = screen(&app, 60, 15);
        let buf = backend.buffer();
        assert!(row(buf, 13).starts_with("ba╰───"), "{backend}");
        assert!(row(buf, 13).ends_with("╯"), "{backend}");
        assert!(row(buf, 14).starts_with(" ↑/↓  scroll"), "{backend}");
    }

    // -----------------------------------------------------------------
    // Images
    // -----------------------------------------------------------------

    /// A session with one question showing `image`.
    fn with_image(image: &str) -> App {
        app(&[(
            "pictured",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Which one?", "kind": "single",
                    "context": [{{"type": "image", "path": {image:?}}}], "options": [{{"id": "a", "label": "This one"}}]}}]}}"#
            ),
        )])
    }

    /// A two-colour PNG, so the halfblock rendering is visible.
    fn png() -> tempfile::NamedTempFile {
        png_of(160)
    }

    /// A square two-colour PNG of `size` pixels.
    fn png_of(size: u32) -> tempfile::NamedTempFile {
        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        let image = image::RgbImage::from_fn(size, size, |x, _| {
            if x < size / 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        image.save(file.path()).expect("png is written");
        file
    }

    /// The columns and rows of cells the test image painted: its red and
    /// blue halves show as background colours.
    fn image_cells(
        backend: &TestBackend,
    ) -> (std::ops::RangeInclusive<u16>, std::ops::RangeInclusive<u16>) {
        let buffer = backend.buffer();
        let painted: Vec<(u16, u16)> = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| {
                let cell = &buffer[position];
                [cell.bg, cell.fg]
                    .iter()
                    .any(|colour| matches!(colour, Color::Rgb(255, 0, 0) | Color::Rgb(0, 0, 255)))
            })
            .collect();
        assert!(!painted.is_empty(), "the image is drawn");
        let xs = painted.iter().map(|(x, _)| *x);
        let ys = painted.iter().map(|(_, y)| *y);
        (
            xs.clone().min().expect("cells")..=xs.max().expect("cells"),
            ys.clone().min().expect("cells")..=ys.max().expect("cells"),
        )
    }

    #[test]
    fn a_wide_screen_shows_the_image_beside_the_question() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        let (columns, rows) = image_cells(&screen(&app, 140, 20));

        // The 138 columns inside the margins split into 68 for the
        // question, the separator and the rest for the image.
        assert_eq!(
            *columns.start(),
            70,
            "right of the question text: {columns:?}"
        );
        assert_eq!(*rows.start(), 5, "under the pane's header: {rows:?}");
    }

    #[test]
    fn a_small_image_grows_to_fill_the_pane() {
        // The 160 pixels of the test image take 16 columns at the half
        // blocks' cell size, far less than the pane has.
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        let (columns, rows) = image_cells(&screen(&app, 140, 20));

        assert_eq!(rows, 5..=17, "every row of the pane's body");
        assert!(columns.end() - columns.start() > 16, "{columns:?}");
    }

    #[test]
    fn a_session_that_comes_back_loads_its_image_afresh() {
        let file = png();
        let mut app = with_image(file.path().to_str().expect("UTF-8 path"));
        let pictured = app.active().expect("the pictured session").clone();
        let mut view = View::new("q", PaneLayout::Auto, Picker::halfblocks());
        screen_in(&app, &mut view, 140, 20);

        // A green picture under the same path shows only once the image is
        // read again, so the colours say whether the viewer was kept.
        image::RgbImage::from_pixel(160, 160, image::Rgb([0, 255, 0]))
            .save(file.path())
            .expect("png is written");
        let green = |backend: &TestBackend| {
            backend
                .buffer()
                .content()
                .iter()
                .any(|cell| [cell.bg, cell.fg].contains(&Color::Rgb(0, 255, 0)))
        };
        let backend = screen_in(&app, &mut view, 140, 20);
        assert!(!green(&backend), "a redraw keeps the image as loaded");

        // While the other session is drawn, the pictured one has left the
        // queue; when it arrives again, its image is read from the file.
        app.add(SessionState::new(
            "other",
            serde_json::from_str(RELEASE).expect("parses"),
            None,
        ));
        app.remove("pictured");
        screen_in(&app, &mut view, 140, 20);
        app.add(pictured);
        app.remove("other");
        let backend = screen_in(&app, &mut view, 140, 20);
        assert!(green(&backend), "{backend}");
    }

    /// A session with one question showing `image` at its natural size.
    fn with_image_kept_small(image: &str) -> App {
        app(&[(
            "pictured",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Which one?", "kind": "single",
                    "context": [{{"type": "image", "path": {image:?}, "enlarge": false}}],
                    "options": [{{"id": "a", "label": "This one"}}]}}]}}"#
            ),
        )])
    }

    #[test]
    fn an_image_that_does_not_enlarge_keeps_its_size_in_the_pane_and_full_screen() {
        // The 160 pixels take 16 columns and 8 rows at the half blocks'
        // cell size of 10×20 pixels.
        let file = png();
        let mut app = with_image_kept_small(file.path().to_str().expect("UTF-8 path"));

        let (columns, rows) = image_cells(&screen(&app, 140, 20));
        assert_eq!((columns, rows), (70..=85, 5..=12), "in the pane");

        keys(&mut app, "z");
        let (columns, rows) = image_cells(&screen(&app, 140, 20));
        assert_eq!(
            (columns.end() - columns.start(), rows.end() - rows.start()),
            (15, 7),
            "full screen too"
        );
    }

    #[test]
    fn an_image_that_does_not_enlarge_still_shrinks_to_fit() {
        let file = png_of(2000);
        let app = with_image_kept_small(file.path().to_str().expect("UTF-8 path"));

        let (_, rows) = image_cells(&screen(&app, 140, 20));

        assert_eq!(rows, 5..=17, "the pane's body, no more");
    }

    /// With the Kitty protocol the terminal draws the image into the cells
    /// its placement names. They must be the cells asqr fills with
    /// placeholders: left to the terminal, it derives them from its own cell
    /// size, which differs from the one asqr resized the image with after a
    /// font size change or in a second tmux client, and crops the image.
    #[test]
    fn a_kitty_image_is_placed_over_exactly_its_placeholder_cells() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
        let mut view = View::new("q", PaneLayout::Auto, picker);
        let backend = screen_in(&app, &mut view, 140, 24);

        let buffer = backend.buffer();
        let placeholders: Vec<(u16, u16)> = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].symbol().contains('\u{10EEEE}'))
            .collect();
        let columns = placeholders
            .iter()
            .map(|&(x, _)| x)
            .collect::<BTreeSet<_>>();
        let rows = placeholders
            .iter()
            .map(|&(_, y)| y)
            .collect::<BTreeSet<_>>();
        let transmission = placeholders
            .iter()
            .map(|&position| buffer[position].symbol())
            .find(|symbol| symbol.contains("\x1b_G"))
            .expect("the first draw sends the image");

        let placement = format!("c={},r={}", columns.len(), rows.len());
        assert!(
            transmission.contains(&placement),
            "the placement names {placement}"
        );
    }

    /// The Kitty ids of the images `view` holds after drawing `app` with
    /// an image shown.
    fn shown_kitty_images(app: &App, view: &mut View) -> Vec<u32> {
        let (_, drawn) = measured_in(app, view, 140, 24);
        assert_eq!(drawn.deleted_images, Vec::<u32>::new(), "nothing went yet");
        view.tab.viewers.held_images()
    }

    /// The last session leaving draws the empty queue, which shows no
    /// tab, and its images still go with it in that frame.
    #[test]
    fn the_images_of_the_last_session_are_deleted_after_the_empty_queue_is_drawn() {
        let file = png();
        let mut app = with_image(file.path().to_str().expect("UTF-8 path"));
        let mut view = kitty_view();
        let shown = shown_kitty_images(&app, &mut view);
        assert_eq!(shown.len(), 1, "{shown:?}");

        app.remove("pictured");
        let (_, drawn) = measured_in(&app, &mut view, 140, 24);
        assert_eq!(drawn.deleted_images, shown);

        let (_, drawn) = measured_in(&app, &mut view, 140, 24);
        assert_eq!(drawn.deleted_images, Vec::<u32>::new(), "deleted once");
    }

    #[test]
    fn the_images_of_a_session_are_deleted_after_a_frame_too_small_to_draw() {
        let file = png();
        let mut app = with_image(file.path().to_str().expect("UTF-8 path"));
        let mut view = kitty_view();
        let shown = shown_kitty_images(&app, &mut view);

        app.remove("pictured");
        let (_, drawn) = measured_in(&app, &mut view, 40, 10);

        assert_eq!(drawn.deleted_images, shown);
    }

    /// Images forgotten on regaining the focus go with the next draw,
    /// which sends them again.
    #[test]
    fn forgotten_images_are_deleted_after_the_next_frame() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));
        let mut view = kitty_view();
        let shown = shown_kitty_images(&app, &mut view);

        view.forget_images();
        let (_, drawn) = measured_in(&app, &mut view, 140, 24);

        assert_eq!(drawn.deleted_images, shown);
        assert_ne!(view.tab.viewers.held_images(), shown, "sent anew");
    }

    #[test]
    fn a_narrow_screen_shows_the_image_below_the_question() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        let (_, rows) = image_cells(&screen(&app, 80, 24));

        // The question takes the six rows it needs, from row 4 on, and the
        // pane's header the next one.
        assert_eq!(*rows.start(), 11, "below the question text: {rows:?}");
    }

    #[test]
    fn the_option_under_the_cursor_shows_its_image_and_keeps_the_room() {
        let file = png();
        let long =
            "a description long enough to reach the right edge of a wide terminal ".repeat(3);
        let mut app = app(&[(
            "browsing",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Which one?", "kind": "single",
                    "options": [{{"id": "a", "label": "Pictured", "context": [{{"type": "image", "path": {image:?}}}]}},
                                {{"id": "b", "label": "Plain", "description": {long:?}}}]}}]}}"#,
                image = file.path().to_str().expect("UTF-8 path"),
            ),
        )]);

        let (columns, _) = image_cells(&screen(&app, 140, 20));
        assert_eq!(*columns.start(), 70, "beside the options: {columns:?}");

        keys(&mut app, "j");
        let backend = screen(&app, 140, 20);
        let buffer = backend.buffer();
        let right_side_used = (4..buffer.area.height - 2)
            .flat_map(|y| (69..buffer.area.width).map(move |x| (x, y)))
            .any(|position| buffer[position].symbol() != " ");
        assert!(
            !right_side_used,
            "the text keeps its width while the area stays empty:\n{}",
            backend
        );
    }

    #[test]
    fn z_shows_the_image_full_screen() {
        let file = png();
        let mut app = with_image(file.path().to_str().expect("UTF-8 path"));
        keys(&mut app, "z");

        let (columns, rows) = image_cells(&screen(&app, 80, 20));

        assert!(
            *columns.start() < 30,
            "the whole width is used: {columns:?}"
        );
        assert!(
            rows.end() - rows.start() >= 10,
            "it fills the height: {rows:?}"
        );
    }

    #[test]
    fn a_missing_or_unreadable_image_shows_a_placeholder() {
        insta::assert_snapshot!(
            "missing_image",
            screen(&with_image("/nonexistent/x.png"), 80, 24)
        );

        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp file");
        std::fs::write(file.path(), "not a picture").expect("written");
        let text = screen(&with_image(file.path().to_str().expect("UTF-8")), 80, 24).to_string();
        assert!(text.contains("image not shown"), "{text}");
    }

    #[test]
    fn answers_show_on_their_questions_when_coming_back() {
        let mut app = app(&[("release", RELEASE)]);
        // Pick "Publish now", toggle nothing, type an own answer that does
        // not fit its line, then write a text answer and a note.
        keys(
            &mut app,
            "1jjjan own answer much longer than the rest of this line has room",
        );
        special(&mut app, KeyCode::Up);
        insta::assert_snapshot!("own_answer_unfocused_60x15", screen(&app, 60, 15));

        keys(&mut app, "l");
        keys(&mut app, "done");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "nfirst");
        app.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        keys(&mut app, "second");
        special(&mut app, KeyCode::Esc);
        // Leaving the note returns to the answer field; esc leaves it too.
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!("text_answer_unfocused", screen(&app, 80, 24));

        keys(&mut app, "hh");
        insta::assert_snapshot!("single_picked", screen(&app, 80, 24));
    }

    #[test]
    fn a_left_own_answer_row_stays_highlighted_without_notes() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Name it", "kind": "single",
            "note": false, "custom": true, "options": [{"id": "a", "label": "asqr"}]}]}"#;
        let mut app = app(&[("name", json)]);
        keys(&mut app, "jmine");
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!(screen(&app, 60, 15));
    }

    #[test]
    fn an_own_answer_of_blanks_is_no_answer_anywhere() {
        let json = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Name it", "kind": "single",
            "custom": true, "options": [{"id": "a", "label": "asqr"}]}]}"#;
        for (name, blank) in [("blanks", "  "), ("a line break", "\n")] {
            let mut app = app(&[("name", json)]);
            keys(&mut app, "j");
            app.paste(blank);

            let state = app.active().expect("the session");
            assert_eq!(
                state.answers()[0].custom.as_deref(),
                Some(blank),
                "{name}: the draft keeps the text as typed"
            );
            let backend = screen(&app, 60, 15);
            let rows = test_support::rows(backend.buffer());
            let own = rows
                .iter()
                .find(|row| row.contains('✎'))
                .expect("the own answer shows");
            assert!(own.contains("( ) ✎"), "{name}: not chosen\n{backend}");
            assert!(rows[2].contains("☐ q"), "{name}: the tab\n{backend}");
            assert_eq!(state.counts().answered, 0, "{name}");
            assert_eq!(state.counts().skipped, 1, "{name}");
            assert!(
                result_answers(state.session(), &state.answers())[0].skipped,
                "{name}"
            );
        }
    }

    #[test]
    fn an_empty_text_answer_without_focus_shows_a_dash() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn the_dash_of_an_empty_field_or_note_is_dim() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        let backend = screen(&app, 80, 24);

        for head in ["❯ ✎ ", "Notes: "] {
            let (x, y) = find(backend.buffer(), &format!("{head}—"));
            let dash = x + u16::try_from(columns(head)).expect("a short head");
            assert!(
                backend.buffer()[(dash, y)].modifier.contains(Modifier::DIM),
                "after {head:?}:\n{backend}"
            );
        }
    }

    #[test]
    fn a_one_line_field_with_a_length_shows_its_counter() {
        let mut app = app(&[("batch", EXAMPLE)]);
        keys(&mut app, "jjjjSnappy in holly");
        let backend = screen(&app, 100, 24);
        insta::assert_snapshot!(backend);

        // The counter ends the question's rows above the image and the
        // note is cut off. The marker keeps clear of the counter on the
        // right and goes to the blank start of its line.
        let (counter, row) = find(backend.buffer(), "15/125");
        assert_eq!(counter, 93, "{backend}");
        assert_eq!(find(backend.buffer(), "↓ more"), (1, row), "{backend}");
    }

    #[test]
    fn the_reject_field_has_its_own_keys() {
        let mut app = review_row_below_submit(2);
        keys(&mut app, "out of date");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn the_session_note_field_has_its_own_keys() {
        let mut app = review_row_below_submit(1);
        keys(&mut app, "approved");
        insta::assert_snapshot!(screen(&app, 80, 24));
    }

    #[test]
    fn a_diff_entry_shows_its_diff_under_its_title() {
        let review = include_str!("../../../examples/sessions/retry-review.json");
        let titled = app(&[("review", review)]);

        insta::assert_snapshot!("diff_in_pane", screen(&titled, 120, 20));

        // Without a title the header names the file.
        let without_title = review.replace(r#""title": "Retry with backoff","#, "");
        let untitled = app(&[("review", &without_title)]);
        let text = screen(&untitled, 120, 20).to_string();
        assert!(text.contains("fetch.rs"), "{text}");
    }

    #[test]
    fn a_markdown_entry_shows_rendered_under_its_title() {
        let review = include_str!("../../../examples/sessions/retry-review.json");
        let mut titled = app(&[("review", review)]);
        keys(&mut titled, "l");
        insta::assert_snapshot!("markdown_in_pane", screen(&titled, 80, 24));

        // Inline Markdown without a title is named by its kind.
        let without_title = review.replace(r#""title": "short.md", "#, "");
        let mut untitled = app(&[("review", &without_title)]);
        keys(&mut untitled, "l");
        let text = screen(&untitled, 80, 24).to_string();
        assert!(text.contains(" markdown "), "{text}");
    }

    #[test]
    fn a_code_entry_shows_highlighted_with_line_numbers() {
        let app = app(&[(
            "code",
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "Keep the retry?", "kind": "single",
                "context": [{"type": "code", "title": "fetch.rs",
                             "content": "pub fn fetch(url: &str) -> Response {\n    retry(|| get(url))\n}\n"}],
                "options": [{"id": "a", "label": "Yes"}]}]}"#,
        )]);

        let backend = screen(&app, 120, 20);

        insta::assert_snapshot!("code_in_pane", backend);
        let row = row_starting_with(&backend, "Keep the retry?");
        let line = test_support::rows(backend.buffer())[usize::from(row) + 1].clone();
        assert!(line.contains("1 pub fn fetch"), "{backend}");
        let start = line.find("pub fn").expect("the code line");
        let x = line[..start].chars().count() + "pub ".len();
        let x = u16::try_from(x).expect("a narrow screen");
        assert_eq!(
            backend.buffer()[(x, row + 1)].fg,
            Color::Indexed(5),
            "a highlighted keyword:\n{backend}"
        );
    }

    #[test]
    fn z_shows_any_entry_full_screen_with_its_header() {
        let review = include_str!("../../../examples/sessions/retry-review.json");
        let mut app = app(&[("review", review)]);
        keys(&mut app, "lz");
        insta::assert_snapshot!("markdown_full_screen", screen(&app, 80, 24));

        keys(&mut app, "z");
        let text = screen(&app, 80, 24).to_string();
        assert!(text.contains("pick one"), "z goes back:\n{text}");
    }

    /// A question with an own answer and a new file of more lines than
    /// the pane below the question has at 80×24.
    fn long_context() -> App {
        let lines: String = (1..=30).map(|line| format!("line {line}\\n")).collect();
        app(&[(
            "long",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Merge it?", "kind": "single", "custom": true,
                    "context": [{{"type": "diff", "title": "Thirty lines", "new": {{"content": "{lines}"}}}}],
                    "options": [{{"id": "a", "label": "Yes"}}]}}]}}"#
            ),
        )])
    }

    /// The row of the screen whose text starts with `start` after the
    /// margin.
    fn row_starting_with(backend: &TestBackend, start: &str) -> u16 {
        let row = test_support::rows(backend.buffer())
            .iter()
            .position(|line| line.chars().skip(1).collect::<String>().starts_with(start))
            .unwrap_or_else(|| panic!("no row starts with {start}:\n{backend}"));
        u16::try_from(row).expect("a short screen")
    }

    #[test]
    fn the_focused_pane_scrolls_and_shows_its_keys() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        let mut drawn = |app: &mut App| screen_handing_on_pane(app, &mut view, 80, 24);
        insta::assert_snapshot!("pane_unfocused", drawn(&mut app));

        special(&mut app, KeyCode::Tab);
        special(&mut app, KeyCode::PageDown);
        let backend = drawn(&mut app);
        insta::assert_snapshot!("pane_focused_scrolled", backend);
        let header = row_starting_with(&backend, "Thirty lines");
        for x in 1..79 {
            let cell = &backend.buffer()[(x, header)];
            assert_eq!(
                (cell.fg, cell.modifier & !Modifier::BOLD),
                (Color::Yellow, Modifier::REVERSED),
                "the focused pane's header is filled to its end, column {x}:\n{backend}"
            );
        }

        keys(&mut app, "?");
        insta::assert_snapshot!("pane_help", drawn(&mut app));
    }

    #[test]
    fn a_draw_says_whether_it_showed_a_tab_a_pane_and_a_field() {
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());

        let (_, empty) = measured_in(&App::default(), &mut view, 80, 24);
        assert_eq!(empty.pane_page, None, "an empty queue shows no tab");
        assert_eq!(empty.intro_page, None);
        assert_eq!(empty.field_shape, None);

        let (_, without_pane) = measured_in(&app(&[("release", RELEASE)]), &mut view, 80, 24);
        assert_eq!(without_pane.pane_page, Some(None));
        assert_eq!(without_pane.intro_page, Some(Extent::default()));

        let mut app = long_context();
        keys(&mut app, "j");
        let (_, full) = measured_in(&app, &mut view, 80, 24);
        assert!(matches!(full.pane_page, Some(Some(_))), "the pane showed");
        assert!(full.field_shape.is_some(), "the own answer has the keys");

        let (_, small) = measured_in(&app, &mut view, 40, 10);
        assert_eq!(small.pane_page, None, "nothing of the last frame stays");
        assert_eq!(small.intro_page, None);
        assert_eq!(small.field_shape, None);
    }

    #[test]
    fn a_frame_too_small_for_asqr_keeps_the_pane_focus_and_the_full_screen() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        // What the run loop does after each frame.
        let mut drawn = |app: &mut App, width: u16, height: u16| {
            screen_handing_on_pane(app, &mut view, width, height);
        };
        drawn(&mut app, 80, 24);
        special(&mut app, KeyCode::Tab);
        drawn(&mut app, 80, 24);

        drawn(&mut app, 40, 10);
        drawn(&mut app, 80, 24);
        let state = app.active().expect("a session");
        assert!(state.pane_focused(), "the pane keeps the focus");

        keys(&mut app, "z");
        drawn(&mut app, 80, 24);
        drawn(&mut app, 40, 10);
        drawn(&mut app, 80, 24);
        let state = app.active().expect("a session");
        assert!(state.full_screen(), "the full-screen view stays");
        assert!(state.pane_focused());
    }

    #[test]
    fn the_full_screen_help_says_how_to_leave_the_full_screen() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        screen_handing_on_pane(&mut app, &mut view, 80, 24);
        // Entered from the pane, where "back to the question" would be wrong.
        special(&mut app, KeyCode::Tab);
        keys(&mut app, "z?");
        insta::assert_snapshot!("full_screen_help", screen_in(&app, &mut view, 80, 24));
    }

    #[test]
    fn the_text_cursor_hides_while_the_pane_has_the_focus() {
        let mut app = long_context();
        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        screen_handing_on_pane(&mut app, &mut view, 80, 24);
        keys(&mut app, "jmine");

        let typing = screen_in(&app, &mut view, 80, 24);
        let own = row_starting_with(&typing, "❯ (•) ✎");
        let reversed_in = |backend: &TestBackend, row: u16| {
            (0..80)
                .filter(|&x| {
                    backend.buffer()[(x, row)]
                        .modifier
                        .contains(Modifier::REVERSED)
                })
                .count()
        };
        assert_eq!(reversed_in(&typing, own), 1, "the cursor:\n{typing}");

        special(&mut app, KeyCode::Tab);
        let focused = screen_in(&app, &mut view, 80, 24);
        assert_eq!(reversed_in(&focused, own), 0, "no cursor:\n{focused}");
        assert!(focused.to_string().contains("mine"), "{focused}");
    }

    #[test]
    fn an_image_placeholder_taller_than_the_pane_scrolls() {
        let path = format!("/nonexistent/{}x.png", "deeper/".repeat(120));
        let mut app = with_image(&path);
        keys(&mut app, "z");

        let mut view = View::new("default", PaneLayout::Auto, Picker::halfblocks());
        insta::assert_snapshot!(screen_handing_on_pane(&mut app, &mut view, 80, 16));

        for _ in 0..4 {
            special(&mut app, KeyCode::Down);
        }
        let text = screen_in(&app, &mut view, 80, 16).to_string();
        assert!(text.contains("5–13 of 13"), "{text}");
        assert!(text.contains("No such file"), "the reason: {text}");
    }

    #[test]
    fn a_relative_image_path_shows_a_placeholder() {
        let text = screen(&with_image("images/x.png"), 80, 24).to_string();
        assert!(text.contains("relative path"), "{text}");
    }

    // -----------------------------------------------------------------
    // The reading width and the context layout
    // -----------------------------------------------------------------

    /// A question whose text and first option's description run far past
    /// the reading width, with `context` (a JSON member and its comma, or
    /// nothing) and an own answer.
    fn long_question(context: &str) -> App {
        let text = "A question text that goes on and on, ".repeat(8);
        let description = "a description that would fill a very wide line ".repeat(5);
        app(&[(
            "long",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": {text:?}, "kind": "single", "custom": true, {context}
                    "options": [{{"id": "a", "label": "First", "description": {description:?}}}, {{"id": "b", "label": "Second"}}]}}]}}"#
            ),
        )])
    }

    /// The rightmost column with text in the rows of the tab, between the
    /// tab bar and the status line.
    fn rightmost_text_column(backend: &TestBackend) -> u16 {
        let buffer = backend.buffer();
        (4..buffer.area.height - 2)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].symbol() != " ")
            .map(|(x, _)| x)
            .max()
            .expect("the tab has text")
    }

    #[test]
    fn a_question_without_context_stays_within_the_reading_width() {
        let app = long_question("");
        // The text starts after the one-column margin, so its last column
        // is the reading width itself.
        for width in [104, 120, 160, 230] {
            let backend = screen(&app, width, 30);
            let rightmost = rightmost_text_column(&backend);
            assert!(
                (95..=READING_WIDTH).contains(&rightmost),
                "{width} columns end at {rightmost}:\n{backend}"
            );
        }
        insta::assert_snapshot!("reading_width_160x30", screen(&app, 160, 30));
    }

    #[test]
    fn the_review_stays_within_the_reading_width() {
        let mut app = long_question("");
        keys(&mut app, "jj");
        keys(
            &mut app,
            &"an own answer much longer than any line ".repeat(6),
        );
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");

        let backend = screen(&app, 200, 24);
        assert_eq!(rightmost_text_column(&backend), READING_WIDTH, "{backend}");
    }

    #[test]
    fn a_question_with_context_beside_is_narrower_than_one_without() {
        let file = png();
        let image = file.path().to_str().expect("UTF-8 path");
        let with_context = long_question(&format!(
            r#""context": [{{"type": "image", "path": {image:?}}}],"#
        ));

        // 160 columns leave 158 inside the margins: 78 for the question,
        // the separator and 79 for the image.
        let backend = screen(&with_context, 160, 30);
        let (columns, _) = image_cells(&backend);
        assert_eq!(*columns.start(), 80, "{backend}");
        let text_columns = (4..28)
            .flat_map(|y| (0..79).map(move |x| (x, y)))
            .filter(|&position| backend.buffer()[position].symbol() != " ")
            .map(|(x, _)| x)
            .max();
        assert!(
            text_columns.is_some_and(|column| (70..=78).contains(&column)),
            "the question wraps within its 78 columns: {text_columns:?}"
        );
    }

    #[test]
    fn a_terminal_too_small_for_the_context_shows_a_hint() {
        // 80×20 leaves 78×14 for the tab: beside needs 101 columns, below
        // 8 rows for the context with the question at half the height.
        insta::assert_snapshot!(
            "context_hint_80x20",
            screen(&with_image("/pictures/x.png"), 80, 20)
        );
    }

    #[test]
    fn the_context_hint_and_the_cut_intro_put_esc_first_while_a_field_has_the_keys() {
        let mut app = app(&[(
            "pictured",
            r#"{"asqr": 2, "intro": "First line of the intro\nsecond line\nthird line\nfourth line",
                "questions": [{"id": "q", "text": "Which one?", "kind": "single", "custom": true,
                "context": [{"type": "image", "path": "/pictures/x.png"}], "options": [{"id": "a", "label": "This one"}]}]}"#,
        )]);
        let rows = |app: &App| test_support::rows(screen(app, 80, 20).buffer());
        let unfocused = rows(&app);
        assert!(
            unfocused.iter().any(|row| row.contains("… i: more")),
            "{unfocused:#?}"
        );
        assert!(
            unfocused.iter().any(|row| row.contains("· z: full screen")),
            "{unfocused:#?}"
        );

        keys(&mut app, "j");

        let backend = screen(&app, 80, 20);
        let typing = test_support::rows(backend.buffer());
        assert!(
            typing.iter().any(|row| row.contains("… esc, i: more")),
            "{typing:#?}"
        );
        assert!(
            typing
                .iter()
                .any(|row| row.contains("· esc, z: full screen")),
            "{typing:#?}"
        );
        insta::assert_snapshot!("context_hint_in_a_field_80x20", backend);
    }

    #[test]
    fn the_cut_intro_puts_esc_first_in_a_text_answer() {
        let mut app = long_header();
        keys(&mut app, "ll");

        insta::assert_snapshot!("intro_cut_in_a_field_80x24", screen(&app, 80, 24));
    }

    #[test]
    fn at_80_columns_the_pane_header_offers_tab_where_the_key_bar_has_no_room() {
        let lines: String = (1..=30).map(|line| format!("line {line}\\n")).collect();
        let app = app(&[(
            "long",
            &format!(
                r#"{{"asqr": 2, "questions": [{{"id": "q", "text": "Merge which?", "kind": "multi", "custom": true,
                    "context": [{{"type": "diff", "title": "Thirty lines", "new": {{"content": "{lines}"}}}}],
                    "options": [{{"id": "a", "label": "This"}}, {{"id": "b", "label": "That"}}]}}]}}"#
            ),
        )]);

        let backend = screen(&app, 80, 24);
        let rows = test_support::rows(backend.buffer());

        let key_bar = rows.last().expect("a key bar");
        assert!(!key_bar.contains("tab"), "no room for tab: {key_bar}");
        let header = &rows[usize::from(row_starting_with(&backend, "Thirty lines"))];
        assert!(header.trim_end().ends_with(" · tab"), "{backend}");
    }

    #[test]
    fn a_layout_asked_for_wins_while_it_fits() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));

        // 140×40 is landscape, where auto puts the image beside.
        let backend = screen_in(
            &app,
            &mut View::new("q", PaneLayout::Below, Picker::halfblocks()),
            140,
            40,
        );
        let (columns, rows) = image_cells(&backend);
        assert_eq!(
            (*columns.start(), *rows.start()),
            (1, 11),
            "under the question's six rows and the header:\n{backend}"
        );

        // 110×70 is portrait, where auto puts the image below; 108 columns
        // inside the margins give the question its 60 and the image the
        // rest after the separator.
        let backend = screen_in(
            &app,
            &mut View::new("q", PaneLayout::Beside, Picker::halfblocks()),
            110,
            70,
        );
        let (columns, rows) = image_cells(&backend);
        assert_eq!((*columns.start(), *rows.start()), (62, 5), "{backend}");
    }

    #[test]
    fn a_resize_to_a_nearly_square_shape_keeps_the_arrangement_shown() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));
        // 192×106 leaves 190×100 for the tab, a shape of 0.95.
        let (columns, rows) = image_cells(&screen(&app, 192, 106));
        assert_eq!(
            (*columns.start(), *rows.start()),
            (1, 11),
            "shown fresh, it is below"
        );

        let mut view = View::new("q", PaneLayout::Auto, Picker::halfblocks());
        screen_in(&app, &mut view, 230, 60);
        let (columns, rows) = image_cells(&screen_in(&app, &mut view, 192, 106));
        assert_eq!(
            (*columns.start(), *rows.start()),
            (96, 5),
            "after beside, it stays beside"
        );
    }

    // -----------------------------------------------------------------
    // The question's height above its context
    // -----------------------------------------------------------------

    /// The entry of every question with context in these tests.
    fn code_entry() -> serde_json::Value {
        serde_json::json!([{"type": "code", "title": "ctx.rs", "content": "fn main() {}\n"}])
    }

    /// A single question with `options` options without descriptions.
    /// Its text, hint and blank line, the options, and the blank line and
    /// `Notes` take `options + 5` rows.
    fn choice(id: &str, options: usize, context: bool) -> serde_json::Value {
        let options: Vec<_> = (1..=options)
            .map(|index| serde_json::json!({"id": format!("o{index}"), "label": format!("Option {index}")}))
            .collect();
        let mut question =
            serde_json::json!({"id": id, "text": "?", "kind": "single", "options": options});
        if context {
            question["context"] = code_entry();
        }
        question
    }

    /// A text question with context and a length. Its text, hint and blank
    /// line, the answer and the row for its counter, and the blank line
    /// and `Notes` take 7 rows while the answer fits one line.
    fn typed(id: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "text": "?", "kind": "text",
                           "length": {"warn": 500}, "context": code_entry()})
    }

    fn shaped(questions: &[serde_json::Value]) -> String {
        serde_json::json!({"asqr": 2, "questions": questions}).to_string()
    }

    /// The rows the question asks for above its context, measured in the
    /// tab area of a 60×30 terminal, or `width` columns of it.
    fn need(app: &App, view: &mut View, width: u16) -> usize {
        let state = app.active().expect("a session is waiting");
        view.tab
            .question_height
            .need(state, Rect::new(1, 5, width, 24))
    }

    fn ctrl_j(app: &mut App) {
        app.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    }

    #[test]
    fn below_the_question_takes_what_the_tallest_question_with_context_needs() {
        // name, questions, rows on every tab
        let cases = [
            (
                "equal questions",
                vec![
                    choice("a", 3, true),
                    choice("b", 3, true),
                    choice("c", 3, true),
                ],
                8,
            ),
            (
                "one tall question",
                vec![
                    choice("a", 2, true),
                    choice("b", 9, true),
                    choice("c", 2, true),
                ],
                14,
            ),
            ("a text question", vec![typed("t"), choice("a", 1, true)], 7),
            (
                "a taller question without context",
                vec![choice("a", 2, true), choice("b", 12, false)],
                7,
            ),
        ];
        for (name, questions, rows) in cases {
            let mut app = app(&[("shape", &shaped(&questions))]);
            let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
            for tab in 0..questions.len() {
                assert_eq!(need(&app, &mut view, 58), rows, "{name}, tab {tab}");
                special(&mut app, KeyCode::Right);
            }
        }
    }

    #[test]
    fn typing_grows_the_need_at_once_and_it_shrinks_only_after_leaving_the_field() {
        let mut app = app(&[("shape", &shaped(&[typed("t"), choice("a", 1, true)]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        assert_eq!(need(&app, &mut view, 58), 7);

        keys(&mut app, "abc");
        assert_eq!(need(&app, &mut view, 58), 7, "the line is not full yet");
        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 58), 8, "a new line grows it at once");
        keys(&mut app, "d");
        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 58), 9);

        special(&mut app, KeyCode::Backspace);
        special(&mut app, KeyCode::Backspace);
        assert_eq!(
            need(&app, &mut view, 58),
            9,
            "deleting lines keeps the height while the field has the keys"
        );
        special(&mut app, KeyCode::Esc);
        assert_eq!(need(&app, &mut view, 58), 8, "leaving the field lowers it");
    }

    #[test]
    fn the_cursor_after_a_full_line_counts_only_while_typing() {
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());

        // `❯ ✎ ` and 54 characters fill the 58 columns, so the cursor's
        // blank starts a line of its own on the screen.
        keys(&mut app, &"x".repeat(54));
        let backend = screen_in(&app, &mut view, 60, 30);
        let rows = test_support::rows(backend.buffer());
        let answer = rows
            .iter()
            .position(|row| row.contains('✎'))
            .expect("the answer is on screen");
        assert_eq!(rows[answer + 1].trim(), "", "{backend}");
        assert_eq!(need(&app, &mut view, 58), 8);
        special(&mut app, KeyCode::Esc);
        assert_eq!(need(&app, &mut view, 58), 7, "without the cursor");
    }

    #[test]
    fn moving_the_focus_into_the_pane_counts_as_leaving_the_field() {
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        ctrl_j(&mut app);
        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 58), 9);
        special(&mut app, KeyCode::Backspace);
        assert_eq!(need(&app, &mut view, 58), 9);

        screen_handing_on_pane(&mut app, &mut view, 60, 30);
        special(&mut app, KeyCode::Tab);
        assert!(app.active().is_some_and(SessionState::pane_focused));
        assert_eq!(need(&app, &mut view, 58), 8);
    }

    #[test]
    fn typing_to_an_exact_fill_keeps_the_last_row_in_view() {
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        let notes = |backend: &TestBackend| {
            test_support::rows(backend.buffer())
                .iter()
                .any(|row| row.starts_with(" Notes: —"))
        };
        let top = pane_row(&screen_in(&app, &mut view, 80, 24));

        // `❯ ✎ ` and 74 letters fill the 78 columns, so the cursor's blank
        // takes a row of its own, which counts while the field has the
        // keys.
        keys(&mut app, &"x".repeat(74));
        let backend = screen_in(&app, &mut view, 80, 24);
        assert!(notes(&backend), "{backend}");
        assert!(!backend.to_string().contains("↓ more"), "{backend}");
        assert_eq!(pane_row(&backend), top + 1, "{backend}");

        special(&mut app, KeyCode::Backspace);
        let backend = screen_in(&app, &mut view, 80, 24);
        assert_eq!(pane_row(&backend), top + 1, "kept while typing:\n{backend}");
        special(&mut app, KeyCode::Esc);
        let backend = screen_in(&app, &mut view, 80, 24);
        assert_eq!(pane_row(&backend), top, "{backend}");
        assert!(notes(&backend), "{backend}");
    }

    /// A text question typed into until it took 9 rows, then cut back to
    /// 8 while its field keeps the keys.
    fn typed_and_cut_back(id: &str) -> App {
        let mut app = app(&[(id, &shaped(&[typed("t")]))]);
        ctrl_j(&mut app);
        ctrl_j(&mut app);
        special(&mut app, KeyCode::Backspace);
        app
    }

    #[test]
    fn a_resize_a_replaced_file_or_another_session_measure_afresh() {
        let mut app = typed_and_cut_back("shape");
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 58), 9);
        special(&mut app, KeyCode::Backspace);
        assert_eq!(need(&app, &mut view, 58), 9, "kept while typing");
        assert_eq!(need(&app, &mut view, 78), 8, "after a resize");

        ctrl_j(&mut app);
        assert_eq!(need(&app, &mut view, 78), 9);
        special(&mut app, KeyCode::Backspace);
        app.add(SessionState::new(
            "shape",
            serde_json::from_str(&shaped(&[typed("t")])).expect("parses"),
            None,
        ));
        assert_eq!(
            app.active().map(SessionState::focus),
            Some(Focus::Field),
            "the replacement keeps the typed answer, and its field has the keys"
        );
        assert_eq!(need(&app, &mut view, 78), 8, "after the file was replaced");

        let mut other = typed_and_cut_back("other");
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        ctrl_j(&mut other);
        assert_eq!(need(&other, &mut view, 58), 9);
        other.add(SessionState::new(
            "next",
            serde_json::from_str(&shaped(&[typed("t")])).expect("parses"),
            None,
        ));
        other.remove("other");
        assert_eq!(
            other.active().map(SessionState::focus),
            Some(Focus::Field),
            "the next session's field has the keys"
        );
        assert_eq!(need(&other, &mut view, 58), 7, "in another session");
    }

    #[test]
    fn the_rows_of_the_other_questions_are_kept_per_width_and_revision() {
        let questions = [
            choice("a", 2, true),
            choice("b", 2, true),
            choice("c", 9, false),
        ];
        let mut app = app(&[("shape", &shaped(&questions))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());

        assert_eq!(need(&app, &mut view, 58), 7);
        assert_eq!(
            view.tab.question_height.counted(),
            2,
            "only questions with context"
        );
        assert_eq!(need(&app, &mut view, 78), 7);
        assert_eq!(
            view.tab.question_height.counted(),
            2,
            "a resize forgets the old width"
        );

        // The second question grows in the file while the first is shown.
        let taller = [
            choice("a", 2, true),
            choice("b", 6, true),
            choice("c", 9, false),
        ];
        app.add(SessionState::new(
            "shape",
            serde_json::from_str(&shaped(&taller)).expect("parses"),
            None,
        ));
        assert_eq!(need(&app, &mut view, 78), 11);
        screen_in(&app, &mut view, 80, 30);
        assert_eq!(
            view.tab.question_height.counted(),
            2,
            "a draw forgets the replaced revision"
        );
    }

    /// The screen row of the context pane's header, which names `ctx.rs`.
    fn pane_row(backend: &TestBackend) -> usize {
        test_support::rows(backend.buffer())
            .iter()
            .position(|row| row.contains("ctx.rs"))
            .unwrap_or_else(|| panic!("the pane shows:\n{backend}"))
    }

    #[test]
    fn moving_between_questions_keeps_the_split_and_typing_moves_it_by_lines() {
        let questions = [choice("a", 2, true), choice("b", 6, true), typed("t")];
        let mut app = app(&[("shape", &shaped(&questions))]);
        let mut view = View::new("q", PaneLayout::Auto, Picker::halfblocks());

        // A 60×40 terminal is portrait, so the context goes below. The
        // tallest question takes 11 rows under the tab bar's 4.
        let top = pane_row(&screen_in(&app, &mut view, 60, 40));
        assert_eq!(top, 4 + 11);
        for tab in 1..=2 {
            special(&mut app, KeyCode::Right);
            let backend = screen_in(&app, &mut view, 60, 40);
            assert_eq!(pane_row(&backend), top, "tab {tab}:\n{backend}");
        }

        // The text question needs 7 rows; at 5 lines it needs 11, and
        // only its sixth line moves the split.
        for _ in 0..4 {
            ctrl_j(&mut app);
        }
        assert_eq!(pane_row(&screen_in(&app, &mut view, 60, 40)), top);
        ctrl_j(&mut app);
        assert_eq!(pane_row(&screen_in(&app, &mut view, 60, 40)), top + 1);
    }

    // -----------------------------------------------------------------
    // Marks of rows cut off
    // -----------------------------------------------------------------

    /// A session with an intro and one question with context, five
    /// options, an own answer and a note: 11 rows, more than the 8 the
    /// question gets above its context on a 60×24 or 80×24 terminal.
    fn five_options() -> App {
        let options: Vec<_> = ["Holly", "Snow", "Bells", "Candles", "Stars"]
            .iter()
            .enumerate()
            .map(|(index, label)| {
                serde_json::json!({"id": format!("o{index}"), "label": label,
                                   "description": format!("Snappy in {} again.", label.to_lowercase())})
            })
            .collect();
        let json = serde_json::json!({
            "asqr": 2,
            "title": "Mascot lines",
            "intro": "Pick the line the mascot says.",
            "questions": [{"id": "line", "text": "Which line fits the winter mascot?",
                           "kind": "single", "custom": {"label": "Own line"},
                           "context": code_entry(), "options": options}]
        });
        app(&[("mascot", &json.to_string())])
    }

    /// Whether every cell of `text` on the screen is dim and none reversed,
    /// so a marker never looks like the cursor.
    fn dim_only(backend: &TestBackend, text: &str) -> bool {
        let (x, y) = find(backend.buffer(), text);
        let width = u16::try_from(columns(text)).expect("a short text");
        (x..x + width).all(|x| {
            let modifier = backend.buffer()[(x, y)].modifier;
            modifier.contains(Modifier::DIM) && !modifier.contains(Modifier::REVERSED)
        })
    }

    #[test]
    fn rows_cut_off_above_or_below_the_question_are_marked() {
        for width in [60, 80] {
            let mut app = five_options();
            // The title, the intro and the tab bar leave the question rows
            // 5 to 12, the context starting below them.
            let backend = screen(&app, width, 24);
            insta::assert_snapshot!(format!("cut_below_{width}x24"), backend);
            // The marker ends at the right edge of the question's area,
            // which is the screen's but for the margin.
            let right = width - 1 - 6;
            assert_eq!(find(backend.buffer(), "ctx.rs").1, 13, "{backend}");
            assert_eq!(
                find(backend.buffer(), "↓ more"),
                (right, 12),
                "over the blank end of the last row:\n{backend}"
            );
            assert!(dim_only(&backend, "↓ more"), "{backend}");
            assert!(!backend.to_string().contains("↑ more"), "{backend}");

            // On the own answer the question scrolls by a row.
            keys(&mut app, "jjjjj");
            let backend = screen(&app, width, 24);
            insta::assert_snapshot!(format!("cut_both_{width}x24"), backend);
            assert_eq!(find(backend.buffer(), "↑ more"), (right, 5), "{backend}");
            assert_eq!(find(backend.buffer(), "↓ more"), (right, 12), "{backend}");
            assert!(
                find(backend.buffer(), "❯ ( ) ✎ Own line:").1 == 12,
                "{backend}"
            );
            assert!(dim_only(&backend, "↑ more") && dim_only(&backend, "↓ more"));
        }
    }

    #[test]
    fn a_row_full_of_text_keeps_its_start_and_the_marker_covers_its_end() {
        let json = serde_json::json!({
            "asqr": 2,
            "questions": [{"id": "q", "text": "x".repeat(600), "kind": "single",
                           "options": [{"id": "a", "label": "A"}]}]
        });
        let app = app(&[("full", &json.to_string())]);

        // The text fills whole lines of 58 columns, each further one
        // behind `↪`. The option under the cursor ends the 9 rows of the
        // tab on a 60×15 terminal, so the text's first lines are cut off
        // above, under a full line.
        let backend = screen(&app, 60, 15);
        let rows = test_support::rows(backend.buffer());
        assert_eq!(find(backend.buffer(), "↑ more"), (53, 4), "{backend}");
        assert_eq!(rows[4], format!(" ↪ {} ↑ more ", "x".repeat(49)));
        assert_eq!(find(backend.buffer(), "↓ more"), (53, 12), "{backend}");
    }

    #[test]
    fn the_review_marks_its_cut_rows_too() {
        let mut app = app(&[("release", RELEASE)]);
        keys(&mut app, "ll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "l");

        // The review opens on Submit, whose button ends the 8 rows a 60×15
        // terminal leaves it: its first line and the note and reject rows
        // are cut off.
        let backend = screen(&app, 60, 15);
        assert_eq!(find(backend.buffer(), "[ Submit ]").1, 12, "{backend}");
        assert_eq!(find(backend.buffer(), "↑ more"), (53, 5), "{backend}");
        assert_eq!(
            find(backend.buffer(), "↓ more"),
            (53, 12),
            "beside the button under the cursor:\n{backend}"
        );

        special(&mut app, KeyCode::Down);
        special(&mut app, KeyCode::Down);
        let backend = screen(&app, 60, 15);
        assert_eq!(find(backend.buffer(), "↑ more"), (53, 5), "{backend}");
        assert!(!backend.to_string().contains("↓ more"), "{backend}");
    }

    #[test]
    fn a_question_that_fits_has_no_marks() {
        let backend = screen(&five_options(), 120, 40);
        let text = backend.to_string();
        assert!(
            !text.contains("↑ more") && !text.contains("↓ more"),
            "{text}"
        );
    }

    // -----------------------------------------------------------------
    // A field taller than the question's area
    // -----------------------------------------------------------------

    /// Types `count` lines, `line 1` to `line N`, drawing after each key
    /// so `↑`/`↓` go by the rows on screen.
    fn type_lines(app: &mut App, view: &mut View, width: u16, height: u16, count: usize) {
        for line in 1..=count {
            if line > 1 {
                ctrl_j(app);
            }
            keys(app, &format!("line {line}"));
            drawn(app, view, width, height);
        }
    }

    /// The reversed cells under the tab bar of a screen whose header takes
    /// one row, above the status line: the text cursor while a field has
    /// the keys.
    fn cursor_cells(backend: &TestBackend) -> Vec<(u16, u16, String)> {
        let buffer = backend.buffer();
        (4..buffer.area.height - 2)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .filter(|&position| buffer[position].modifier.contains(Modifier::REVERSED))
            .map(|(x, y)| (x, y, buffer[(x, y)].symbol().to_owned()))
            .collect()
    }

    /// A single question with context, an own answer and a note.
    fn own_and_note(id: &str) -> serde_json::Value {
        let mut question = choice(id, 2, true);
        question["custom"] = serde_json::json!(true);
        question
    }

    #[test]
    fn a_field_taller_than_the_area_keeps_its_text_cursor_in_view() {
        // At 80×24 the question gets 9 rows above its context, 4 to 12.
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        type_lines(&mut app, &mut view, 80, 24, 30);

        // On the last line, the counter takes the area's last row.
        let backend = drawn(&mut app, &mut view, 80, 24);
        let rows = test_support::rows(backend.buffer());
        assert_eq!(
            cursor_cells(&backend),
            [(12, 11, " ".to_owned())],
            "{backend}"
        );
        assert!(rows[11].starts_with("     line 30"), "{backend}");
        assert!(rows[4].contains("↑ more"), "{backend}");

        // `↑` keeps the cursor on the last row while the view scrolls up,
        // until the start of the field shows.
        for line in (1..30).rev() {
            special(&mut app, KeyCode::Up);
            let backend = drawn(&mut app, &mut view, 80, 24);
            let y = if line <= 9 { 3 + line } else { 12 };
            let y16 = u16::try_from(y).expect("a row");
            assert_eq!(
                cursor_cells(&backend).first().map(|cell| cell.1),
                Some(y16),
                "line {line}:\n{backend}"
            );
            let rows = test_support::rows(backend.buffer());
            assert!(
                rows[y].contains(&format!("line {line}")),
                "line {line}:\n{backend}"
            );
        }
        let backend = drawn(&mut app, &mut view, 80, 24);
        let rows = test_support::rows(backend.buffer());
        assert!(rows[4].starts_with(" ❯ ✎ line 1 "), "{backend}");
        assert!(
            rows[4].contains("↑ more"),
            "the question is above:\n{backend}"
        );
    }

    #[test]
    fn a_tall_note_or_own_answer_keeps_its_text_cursor_in_view() {
        for (name, start) in [("note", "n"), ("own answer", "jj")] {
            let mut app = app(&[("shape", &shaped(&[own_and_note("q")]))]);
            let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
            keys(&mut app, start);
            type_lines(&mut app, &mut view, 80, 24, 30);

            let backend = drawn(&mut app, &mut view, 80, 24);
            let cursor = cursor_cells(&backend);
            assert_eq!(cursor.len(), 1, "{name}:\n{backend}");
            let (_, y, symbol) = &cursor[0];
            assert_eq!(symbol, " ", "{name}");
            let rows = test_support::rows(backend.buffer());
            assert!(
                rows[usize::from(*y)].contains("line 30"),
                "{name}:\n{backend}"
            );
        }
    }

    #[test]
    fn typing_at_the_end_of_a_tall_field_keeps_its_counter_in_view() {
        let mut own = own_and_note("q");
        own["custom"] = serde_json::json!({"length": {"warn": 500}});
        for (name, question, start) in [("text answer", typed("t"), ""), ("own answer", own, "jj")]
        {
            let mut app = app(&[("shape", &shaped(&[question]))]);
            let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
            keys(&mut app, start);
            type_lines(&mut app, &mut view, 80, 24, 30);

            // The question's area takes rows 4 to 12; the counter takes the
            // last of them, and the cursor stays on the row above.
            let backend = drawn(&mut app, &mut view, 80, 24);
            let rows = test_support::rows(backend.buffer());
            let cursor = cursor_cells(&backend);
            assert_eq!(cursor.len(), 1, "{name}:\n{backend}");
            assert_eq!(cursor[0].1, 11, "{name}:\n{backend}");
            assert!(rows[11].contains("line 30"), "{name}:\n{backend}");
            // The notes below are cut off, and the marker keeps clear of the
            // counter at the row's right end.
            assert!(rows[12].trim_end().ends_with("/500"), "{name}:\n{backend}");
            assert!(rows[12].contains("↓ more"), "{name}:\n{backend}");
        }
    }

    #[test]
    fn a_text_answer_of_twelve_lines_keeps_the_cursor_on_a_small_screen() {
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Auto, Picker::halfblocks());
        type_lines(&mut app, &mut view, 60, 24, 12);

        let backend = drawn(&mut app, &mut view, 60, 24);
        let cursor = cursor_cells(&backend);
        assert_eq!(
            cursor.len(),
            1,
            "the reversed blank is on screen:\n{backend}"
        );
        assert_eq!(cursor[0].2, " ");
    }

    #[test]
    fn a_marker_never_covers_the_text_cursor() {
        // 74 letters fill the 78 columns after `❯ ✎ `; every line after
        // the first starts 4 columns in.
        let full = "x".repeat(74);
        let mut app = app(&[("shape", &shaped(&[typed("t")]))]);
        let mut view = View::new("q", PaneLayout::Below, Picker::halfblocks());
        keys(&mut app, &full);
        ctrl_j(&mut app);
        type_lines(&mut app, &mut view, 80, 24, 29);
        ctrl_j(&mut app);
        keys(&mut app, &full);
        // A line after the full one keeps the counter out of the way: on
        // the text's last line the area's last row would hold the counter.
        ctrl_j(&mut app);
        keys(&mut app, "end");
        drawn(&mut app, &mut view, 80, 24);
        special(&mut app, KeyCode::Up);
        special(&mut app, KeyCode::End);

        // The cursor's blank after the full line shows on a row of its own
        // at the bottom, where `↓ more` takes the blank right end.
        let backend = drawn(&mut app, &mut view, 80, 24);
        assert_eq!(
            cursor_cells(&backend),
            [(5, 12, " ".to_owned())],
            "{backend}"
        );
        assert_eq!(find(backend.buffer(), "↓ more"), (73, 12), "{backend}");

        // On the last letter, the right end holds the cursor, so the marker
        // goes over the start of the row.
        special(&mut app, KeyCode::Left);
        let backend = drawn(&mut app, &mut view, 80, 24);
        assert_eq!(
            cursor_cells(&backend),
            [(78, 12, "x".to_owned())],
            "{backend}"
        );
        assert_eq!(find(backend.buffer(), "↓ more"), (1, 12), "{backend}");

        // The same for `↑ more` on the first row, on the first line's last
        // letter.
        for _ in 0..40 {
            special(&mut app, KeyCode::Up);
            drawn(&mut app, &mut view, 80, 24);
        }
        special(&mut app, KeyCode::End);
        special(&mut app, KeyCode::Left);
        let backend = drawn(&mut app, &mut view, 80, 24);
        assert_eq!(
            cursor_cells(&backend),
            [(78, 4, "x".to_owned())],
            "{backend}"
        );
        assert_eq!(find(backend.buffer(), "↑ more"), (1, 4), "{backend}");
    }

    // -----------------------------------------------------------------
    // Wide characters
    // -----------------------------------------------------------------

    /// Thirty characters of two columns each.
    const WIDE_TEXT: &str = "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほ";

    /// A session in Japanese with an emoji in its title, whose intro runs
    /// past the three lines the header gives it at 60×24.
    fn wide_session() -> App {
        let intro = "変更点を確認してから公開してください。 ".repeat(6);
        let json = serde_json::json!({
            "asqr": 2,
            "title": "リリース 🎉 の確認",
            "intro": intro,
            "questions": [
                {"id": "go", "header": "公開", "text": "今すぐ**公開**しますか？", "kind": "single",
                 "options": [
                    {"id": "yes", "label": "今すぐ", "description": "タグを作って公開する"},
                    {"id": "later", "label": "Later", "description": "週末の後で公開する"}],
                 "custom": {"label": "別の答え"}},
                {"id": "notes", "header": "メモ", "text": "リリースノートの一文:", "kind": "text"}
            ]
        });
        app(&[("wide", &json.to_string())])
    }

    #[test]
    fn descriptions_line_up_after_a_label_of_wide_characters() {
        let backend = screen(&wide_session(), 100, 24);
        let rows = test_support::rows(backend.buffer());

        // The widest head is `❯ ( ) 1. ` and 今すぐ, 15 columns, and the
        // descriptions start 2 after it, behind the margin of one.
        for description in ["タグを作って公開する", "週末の後で公開する"] {
            let row = rows
                .iter()
                .find(|row| row.contains(description))
                .expect("the description is on screen");
            let start = row.find(description).expect("it is in the row");
            assert_eq!(columns(&row[..start]), 18, "{description}:\n{backend}");
        }
    }

    #[test]
    fn a_head_of_wide_characters_counts_its_columns() {
        // As with 28 and 29 `x`: 14 wide characters leave the text 20
        // columns, and it wraps under its start; 15 leave it 18.
        for (count, indent) in [(14, 38), (15, 4)] {
            let backend = screen(&own_answer_labelled(&"日".repeat(count)), 60, 24);
            let rows = test_support::rows(backend.buffer());

            let head = rows
                .iter()
                .position(|row| row.contains('✎'))
                .expect("the own answer is on screen");
            let leading = rows[head + 1].chars().take_while(|c| *c == ' ').count();
            assert_eq!(
                leading,
                1 + indent,
                "a label of {count} wraps after {indent}:\n{backend}"
            );
        }
    }

    #[test]
    fn an_own_answer_of_wide_characters_wraps_in_columns() {
        let mut app = wide_session();
        keys(&mut app, "jj");
        keys(&mut app, WIDE_TEXT);

        let backend = screen(&app, 60, 24);
        insta::assert_snapshot!("wide_own_answer_60x24", backend);
        let rows = test_support::rows(backend.buffer());
        assert!(
            rows[4].trim_end().ends_with("… esc, i: more"),
            "the cut intro keeps its marker:\n{backend}"
        );
        // Without blanks, the text is one word longer than the room after
        // the head `❯ (•) ✎ 別の答え: `, so it starts under its start on
        // the next line. Its 18 columns leave 40 of the 58 for 20
        // characters, the last ending at the edge.
        let head = rows
            .iter()
            .position(|row| row.contains('✎'))
            .expect("the own answer is on screen");
        assert_eq!(rows[head].trim_end(), " ❯ (•) ✎ 別の答え:");
        assert_eq!(
            rows[head + 1],
            format!("{}{} ", " ".repeat(19), &WIDE_TEXT[..60]),
            "only the margin follows"
        );
        assert_eq!(rows[head + 2].trim(), &WIDE_TEXT[60..]);

        // The cursor goes back to な, which starts the third line.
        for _ in 0..10 {
            special(&mut app, KeyCode::Left);
        }
        let backend = screen(&app, 60, 24);
        let head = u16::try_from(head).expect("a row on the screen");
        assert_eq!(
            reversed_cells_in_tab(&backend),
            [(19, head + 2, "な".to_owned())],
            "{backend}"
        );
    }

    #[test]
    fn a_text_answer_of_wide_characters_wraps_in_columns() {
        let mut app = wide_session();
        keys(&mut app, "l");
        keys(&mut app, WIDE_TEXT);

        let backend = screen(&app, 60, 24);
        insta::assert_snapshot!("wide_text_answer_60x24", backend);
        // Without blanks, the text is one word longer than the room after
        // `❯ ✎ `, so it starts on the next line, 4 columns in, which leaves
        // 54 of the 58 for 27 characters, the last ending at the edge.
        let rows = test_support::rows(backend.buffer());
        let first = rows
            .iter()
            .position(|row| row.contains('✎'))
            .expect("the answer is on screen");
        assert_eq!(rows[first].trim_end(), " ❯ ✎");
        assert_eq!(
            rows[first + 1],
            format!("     {} ", &WIDE_TEXT[..81]),
            "only the margin follows"
        );
        assert_eq!(rows[first + 2].trim(), &WIDE_TEXT[81..]);
    }

    #[test]
    fn the_review_lines_up_and_cuts_wide_labels_and_answers_in_columns() {
        let json = serde_json::json!({
            "asqr": 2,
            "questions": [
                {"id": "a", "header": "Go", "text": "?", "kind": "text"},
                {"id": "b", "header": "a日本語のとても長い見出し", "text": "?", "kind": "text"}
            ]
        });
        let mut app = app(&[("wide", &json.to_string())]);
        keys(&mut app, "ok");
        special(&mut app, KeyCode::Enter);
        keys(&mut app, WIDE_TEXT);
        special(&mut app, KeyCode::Enter);

        let backend = screen(&app, 60, 24);
        insta::assert_snapshot!("wide_review_60x24", backend);
        let rows = test_support::rows(backend.buffer());
        let row = |start: &str| {
            rows.iter()
                .find(|row| row.contains(start))
                .unwrap_or_else(|| panic!("{start} is on screen:\n{backend}"))
        };
        // The second header takes 25 columns and is cut to 23, as its
        // last character would cross the 24 labels get. Its summary then
        // starts at column 30 of the tab and has 28 columns: the quote,
        // 13 characters and `…`, the last ending at the edge.
        let summary = format!("\"{}…", &WIDE_TEXT[..39]);
        assert_eq!(
            row("見出 ").trim_end(),
            format!("   ☒ a日本語のとても長い見出   {summary}")
        );
        let go = row("\"ok\"");
        let start = go.find("\"ok\"").expect("the first summary");
        assert_eq!(columns(&go[..start]), 31, "{backend}");
    }

    #[test]
    fn a_queue_path_of_wide_characters_is_cut_in_columns() {
        let app = App::default();

        // The path gets 24 columns: `…` and 23 for its end, which hold 11
        // wide characters, as a 12th would cross the edge.
        assert_eq!(
            title_line(&screen_of_queue(
                &app,
                &format!("/q/{}", "日".repeat(20)),
                60,
                15
            )),
            format!(" asqr · queue: …{} · 0 sessions waiting", "日".repeat(11))
        );
        assert_eq!(
            title_line(&screen_of_queue(
                &app,
                &format!("/{}/{}", "日".repeat(15), "本".repeat(8)),
                60,
                15
            )),
            format!(" asqr · queue: …/{} · 0 sessions waiting", "本".repeat(8)),
            "cut at the separator in reach"
        );
    }

    // -----------------------------------------------------------------
    // NO_COLOR
    // -----------------------------------------------------------------

    /// `app` drawn twice at `width`×`height`, with colours and under
    /// NO_COLOR.
    fn with_and_without_colours(app: &App, width: u16, height: u16) -> (Buffer, Buffer) {
        let drawn = |no_color: bool| {
            let mut view =
                View::new("q", PaneLayout::Auto, Picker::halfblocks()).no_color(no_color);
            screen_in(app, &mut view, width, height).buffer().clone()
        };
        (drawn(false), drawn(true))
    }

    /// Every colour of `cell`: foreground, background and underline.
    fn colours(cell: &ratatui::buffer::Cell) -> [Color; 3] {
        [cell.fg, cell.bg, cell.underline_color]
    }

    /// Checks that `colourless` is `coloured` with every colour reset: the
    /// same text and the same modifiers. `coloured` has to have a colour,
    /// or there would be nothing to take away.
    fn assert_only_colours_gone(coloured: &Buffer, colourless: &Buffer) {
        assert!(
            coloured
                .content()
                .iter()
                .flat_map(colours)
                .any(|colour| colour != Color::Reset),
            "the screen has colours to take away"
        );
        for (with, without) in coloured.content().iter().zip(colourless.content()) {
            assert_eq!(without.symbol(), with.symbol());
            assert_eq!(without.modifier, with.modifier, "at {:?}", with.symbol());
            assert_eq!(
                colours(without),
                [Color::Reset; 3],
                "at {:?}",
                with.symbol()
            );
        }
    }

    #[test]
    fn without_colours_the_focus_and_the_submit_button_stay_two_fills_told_apart_by_brackets() {
        let mut app = app(&[("release", RELEASE)]);
        // On to the text question, which takes an `l` as typed, then on to
        // the review and up from the button to that question.
        keys(&mut app, "lll");
        special(&mut app, KeyCode::Esc);
        keys(&mut app, "lk");
        let (coloured, colourless) = with_and_without_colours(&app, 80, 24);
        assert_only_colours_gone(&coloured, &colourless);

        let reversed_rows: BTreeSet<u16> = colourless
            .area
            .positions()
            .filter(|&position| colourless[position].modifier.contains(Modifier::REVERSED))
            .map(|position| position.y)
            .collect();
        let (submit, current) = (
            find(&colourless, "[ Submit ]").1,
            find(&colourless, "❯ ☒ Highlights").1,
        );
        assert_eq!(reversed_rows, BTreeSet::from([current, submit]));
        let button: String = (0..80)
            .filter(|&x| {
                colourless[(x, submit)]
                    .modifier
                    .contains(Modifier::REVERSED)
            })
            .map(|x| colourless[(x, submit)].symbol())
            .collect();
        assert_eq!(button, "[ Submit ]");
    }

    /// One text question with inline code, a diff and a length.
    const TYPED_OVER_A_DIFF: &str = r#"{"asqr": 2, "questions": [{"id": "q",
        "text": "Keep `retry` as it is?", "kind": "text", "length": {"target": [1, 120], "warn": 140},
        "context": [{"type": "diff", "name": "fetch.rs",
                     "old": {"content": "get(url)\n"}, "new": {"content": "retry(|| get(url))\n"}}]}]}"#;

    #[test]
    fn no_color_takes_the_colours_of_a_counter_a_diff_and_inline_code_and_keeps_the_rest() {
        let mut app = app(&[("typed", TYPED_OVER_A_DIFF)]);
        keys(&mut app, "keep it");

        let (coloured, colourless) = with_and_without_colours(&app, 120, 20);

        assert_eq!(style_at(&coloured, "7/120").fg, Some(Color::Green));
        assert_eq!(style_at(&coloured, "+ retry").fg, Some(Color::Green));
        let inline_code = style_at(&coloured, "retry").fg;
        assert!(
            matches!(inline_code, Some(Color::Indexed(_))),
            "{inline_code:?}"
        );
        assert_only_colours_gone(&coloured, &colourless);
    }

    /// At 80×17 the main area is too small for Talqi, who is drawn in
    /// half blocks with colours and as an outline under NO_COLOR
    /// (`no_color_shows_talqi_as_the_colourless_braille_outline`).
    #[test]
    fn no_color_takes_the_colour_of_a_notice_over_the_empty_queue() {
        let mut app = App::default();
        app.notice(
            Tone::Warning,
            "broken is invalid and was answered with an error",
        );

        let (coloured, colourless) = with_and_without_colours(&app, 80, 17);

        assert_eq!(style_at(&coloured, "broken").fg, Some(Color::Yellow));
        assert_only_colours_gone(&coloured, &colourless);
    }

    /// Whether `cell` holds a half block of a picture drawn without an
    /// image protocol.
    fn half_block(cell: &ratatui::buffer::Cell) -> bool {
        matches!(cell.symbol(), "▀" | "▄")
    }

    /// NO_COLOR keeps every RGB colour, since only images draw with them:
    /// no screen without an image has one, and on the empty queue only
    /// Talqi's half blocks do. Under NO_COLOR the empty queue has none at
    /// all (`no_color_shows_talqi_as_the_colourless_braille_outline`).
    #[test]
    fn only_images_draw_in_rgb() {
        let review = include_str!("../../../examples/sessions/retry-review.json");
        const CODE: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "Keep the retry?",
            "kind": "single", "options": [{"id": "a", "label": "Yes"}],
            "context": [{"type": "code", "title": "fetch.rs",
                         "content": "pub fn fetch(url: &str) -> Response {\n    retry(|| get(url))\n}\n"}]}]}"#;
        let mut notice = App::default();
        notice.notice(
            Tone::Warning,
            "broken is invalid and was answered with an error",
        );
        // Each session, the keys that lead to what it shows, and what that
        // is.
        let shown = [
            (vec![("review", review)], "", "a diff"),
            (vec![("review", review)], "l", "Markdown"),
            (vec![("code", CODE)], "", "code"),
            (vec![("code", CODE)], "l", "the review tab"),
            (vec![("typed", TYPED_OVER_A_DIFF)], "keep it", "a counter"),
            (vec![("release", RELEASE)], "i", "the intro overlay"),
            (vec![("release", RELEASE)], "?", "the help"),
            (
                vec![("review", review), ("release", RELEASE)],
                "L",
                "the session list",
            ),
        ];
        let empty = screen(&notice, 120, 30);
        assert!(
            empty.buffer().content().iter().any(half_block),
            "Talqi is drawn"
        );
        for cell in empty.buffer().content() {
            let rgb = colours(cell)
                .iter()
                .any(|colour| matches!(colour, Color::Rgb(..)));
            assert!(!rgb || half_block(cell), "{cell:?} outside Talqi:\n{empty}");
        }

        let mut screens = Vec::new();
        for (sessions, typed, what) in shown {
            let mut app = app(&sessions);
            keys(&mut app, typed);
            screens.push((screen(&app, 120, 30), what));
        }

        for (backend, what) in screens {
            let rgb = backend
                .buffer()
                .content()
                .iter()
                .flat_map(colours)
                .find(|colour| matches!(colour, Color::Rgb(..)));
            assert_eq!(rgb, None, "{what}:\n{backend}");
        }
    }

    #[test]
    fn no_color_keeps_the_rgb_colour_that_names_a_kitty_image() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
        let mut view = View::new("q", PaneLayout::Auto, picker).no_color(true);

        let backend = screen_in(&app, &mut view, 140, 24);

        let placeholders: Vec<_> = backend
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.symbol().contains('\u{10EEEE}'))
            .collect();
        assert!(!placeholders.is_empty(), "the image is drawn");
        for cell in placeholders {
            assert!(matches!(cell.fg, Color::Rgb(..)), "{:?}", cell.fg);
        }
    }

    /// Whether `cell` holds braille with ink, as Talqi's outline does.
    fn braille_ink(cell: &ratatui::buffer::Cell) -> bool {
        cell.symbol()
            .chars()
            .all(|symbol| ('\u{2801}'..='\u{28FF}').contains(&symbol))
    }

    /// The empty queue at `width`×`height` under NO_COLOR, with images
    /// sent in the graphics protocol `picker` speaks.
    fn empty_without_colours(picker: Picker, width: u16, height: u16) -> Buffer {
        let mut view = View::new("q", PaneLayout::Auto, picker).no_color(true);
        screen_in(&App::default(), &mut view, width, height)
            .buffer()
            .clone()
    }

    /// Under NO_COLOR, Talqi is the braille outline of the emblem: text
    /// without a colour or a modifier, also where the terminal has an
    /// image protocol, and nothing on the screen has a colour.
    #[test]
    fn no_color_shows_talqi_as_the_colourless_braille_outline() {
        let mut kitty = Picker::halfblocks();
        kitty.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
        for picker in [Picker::halfblocks(), kitty] {
            let buf = empty_without_colours(picker, 80, 40);

            let outline: Vec<_> = buf
                .content()
                .iter()
                .filter(|cell| braille_ink(cell))
                .collect();
            assert!(!outline.is_empty(), "Talqi is drawn:\n{buf:?}");
            for cell in outline {
                let mut plain = ratatui::buffer::Cell::default();
                plain.set_symbol(cell.symbol());
                assert_eq!(*cell, plain);
            }
            for cell in buf.content() {
                assert_eq!(colours(cell), [Color::Reset; 3], "{cell:?}");
                assert!(!half_block(cell), "{cell:?}");
                assert!(!cell.symbol().contains('\u{10EEEE}'), "{cell:?}");
            }
        }
    }

    /// At 80×31 the main area has 25 rows, one too few for the outline,
    /// the blank row and the sentence.
    #[test]
    fn no_color_shows_the_sentence_alone_where_the_outline_does_not_fit() {
        let small = empty_without_colours(Picker::halfblocks(), 80, 31);
        let large = empty_without_colours(Picker::halfblocks(), 80, 32);

        assert!(!small.content().iter().any(braille_ink), "{small:?}");
        assert!(!small.content().iter().any(half_block), "{small:?}");
        find(&small, "Nothing to answer");
        assert!(large.content().iter().any(braille_ink), "{large:?}");
    }

    #[test]
    fn no_color_keeps_the_rgb_colours_of_half_block_images() {
        let file = png();
        let app = with_image(file.path().to_str().expect("UTF-8 path"));
        let mut view = View::new("q", PaneLayout::Auto, Picker::halfblocks()).no_color(true);

        image_cells(&screen_in(&app, &mut view, 140, 20));
    }
}
