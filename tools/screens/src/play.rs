// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Playing a scene through asqr's own code: the app with the scene's
//! sessions, its keys, and the frames asqr draws between them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::{Result, anyhow};
use asqr::format::same_session_id;
use asqr::graphics::{CellSize, Graphics, Images};
use asqr::queue::{QueueLocation, WaitingQueue};
use asqr::tui::render::{Drawn, Glyphs, PaneLayout, View, draw};
use asqr::tui::{App, AppEffect, PanePage, Place, QueueChoice, QueueRow, TakenQueue};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::fonts::{CELL_HEIGHT, CELL_WIDTH};
use crate::scene::{Emblem, Key, Layout, Scene, load_sessions};

// =====================================================================
// One frame
// =====================================================================

/// One frame as asqr's run loop draws it (`src/tui/run.rs`, `run`):
/// `draw` draws the app, and what the frame measured goes back to it.
/// While the frame moved a field's text cursor, the next frame shows it
/// where it went; while the context pane shows `loading…`, the next frame
/// comes once a load finished, at most `cap` later.
pub fn step(
    draw: &mut dyn FnMut(&App) -> Result<Drawn>,
    app: &mut App,
    wakes: &Receiver<()>,
    cap: Duration,
) -> Result<()> {
    loop {
        let drawn = draw(app)?;
        let loading = matches!(drawn.pane_page, Some(Some(PanePage { loading: true, .. })));
        // As `run.rs` hands a frame's measurements to the app: a frame
        // without a tab says nothing of the intro, the pane or the row.
        if let Some(page) = drawn.intro_page {
            app.set_intro_page(page);
        }
        if let Some(page) = drawn.pane_page {
            app.set_pane_page(page);
        }
        if let Some(cut) = drawn.row_cut {
            app.set_row_cut(cut);
        }
        app.set_dialog_shown(drawn.invalid_dialog);
        app.set_session_shown(drawn.session_shown);
        // A field moves its cursor in the frame after it was entered from
        // above, and only then (`TextField::set_shape`), so this draws one
        // frame more at most, as the run loop does.
        if app.set_field_shape(drawn.field_shape) {
            continue;
        }
        if loading {
            wakes
                .recv_timeout(cap)
                .map_err(|_| anyhow!("the context pane still shows loading… after {cap:?}"))?;
            continue;
        }
        return Ok(());
    }
}

// =====================================================================
// The keys
// =====================================================================

/// Plays `keys` on `app`, with a `step` for the first frame and after
/// every key, as a person waits for each frame before the next key.
pub fn play_keys(
    app: &mut App,
    keys: &[Key],
    scene: &Scene,
    step: &mut dyn FnMut(&mut App) -> Result<()>,
) -> Result<()> {
    step(app)?;
    for (index, key) in keys.iter().enumerate() {
        let number = index + 1;
        match key {
            Key::Press { event, .. } => {
                let effect = app.handle(*event);
                apply(app, effect, scene, number, key)?;
                step(app)?;
            }
            Key::Text(text) => {
                for ch in text.chars() {
                    let effect = app.handle(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
                    apply(app, effect, scene, number, key)?;
                    step(app)?;
                }
            }
            Key::Paste(text) => {
                let effect = app.paste(text);
                apply(app, effect, scene, number, key)?;
                step(app)?;
            }
        }
    }
    Ok(())
}

/// Does what asqr's run loop does with the `effect` of key `number`, as far
/// as a picture can show it, and refuses the rest.
fn apply(app: &mut App, effect: AppEffect, scene: &Scene, number: usize, key: &Key) -> Result<()> {
    let refused = |what: &str| anyhow!("key {number} ({key}) {what}, which scenes do not play");
    match effect {
        // A picture writes no draft; the screen shows the answer anyway.
        AppEffect::None | AppEffect::SaveDraft(_) => Ok(()),
        AppEffect::OpenQueues => {
            let switcher = scene.switcher.as_ref().ok_or_else(|| {
                anyhow!(
                    "key {number} ({key}) opens the queue switcher, and the scene has no [switcher]"
                )
            })?;
            app.open_queues(queue_choice(switcher));
            Ok(())
        }
        AppEffect::QueuesClosed => {
            app.set_elsewhere(elsewhere(scene));
            Ok(())
        }
        // TODO: the full tour may want the notice of a sent session, which
        // needs an inbox on a temporary queue to finish the session in.
        AppEffect::Finish { .. } => Err(refused("finishes the session")),
        AppEffect::Quit => Err(refused("quits asqr")),
        AppEffect::Open(_) => Err(refused("opens an entry")),
        AppEffect::Edit(_) => Err(refused("opens an editor")),
        AppEffect::Switch(_) => Err(refused("switches the queue")),
    }
}

fn elsewhere(scene: &Scene) -> Vec<WaitingQueue> {
    scene
        .elsewhere
        .iter()
        .map(|queue| WaitingQueue {
            name: queue.queue.clone(),
            waiting: queue.waiting,
        })
        .collect()
}

/// The queues the switcher shows, as asqr's host looks them up.
fn queue_choice(switcher: &crate::scene::Switcher) -> QueueChoice {
    let root = switcher.root.clone();
    let rows = switcher
        .queues
        .iter()
        .map(|queue| {
            let (location, label) = match (&queue.name, &queue.dir) {
                (Some(name), _) => (
                    QueueLocation::named(
                        root.as_deref()
                            .expect("a scene with named queues has a root"),
                        name,
                    ),
                    name.clone(),
                ),
                (None, dir) => {
                    let dir = dir.clone().expect("a queue without a name has a dir");
                    (QueueLocation::at(PathBuf::from(&dir)), dir)
                }
            };
            QueueRow {
                location,
                label,
                waiting: queue.waiting,
                watched: queue.watched,
            }
        })
        .collect();
    let taken = switcher
        .taken
        .iter()
        .map(|queue| TakenQueue {
            label: queue.name.clone(),
            session: queue.tmux_session.clone(),
        })
        .collect();
    QueueChoice {
        rows,
        taken,
        root,
        place: switcher
            .tmux_session
            .clone()
            .map_or(Place::Outside, Place::Tmux),
    }
}

// =====================================================================
// The scene
// =====================================================================

/// The screen asqr shows after the keys of `scene`, read from
/// `scene_file`, waiting at most `cap` for each context entry to load.
///
/// The picture is the buffer of the last frame. The test backend's own
/// buffer only takes the cells a frame changed, and keeps old text in a
/// cell a wide character covers.
pub fn play(scene: &Scene, scene_file: &Path, cap: Duration) -> Result<Buffer> {
    let mut app = App::default();
    for state in load_sessions(scene, scene_file)? {
        app.add(state);
    }
    // A follow-up names the title of the session it follows while that one
    // waits too, as asqr's inbox gives it (`src/tui/inbox.rs`).
    let titles: Vec<(String, String)> = app
        .sessions()
        .iter()
        .filter_map(|state| {
            let follows = state.session().follows.as_deref()?;
            let followed = app.sessions().iter().find(|other| {
                same_session_id(other.id(), follows) && !same_session_id(other.id(), state.id())
            })?;
            Some((state.id().to_owned(), followed.session().title.clone()?))
        })
        .collect();
    for (id, title) in titles {
        app.set_follows_title(&id, Some(title));
    }
    app.set_elsewhere(elsewhere(scene));

    let (wake, wakes) = mpsc::channel();
    // Half blocks are sized for the picture's cells, so an image keeps its
    // proportions there.
    let images = Images::new(Graphics {
        cell: CellSize {
            width: CELL_WIDTH as u16,
            height: CELL_HEIGHT as u16,
        },
        ..Graphics::HALF_BLOCKS
    });
    let layout = match scene.layout {
        Layout::Auto => PaneLayout::Auto,
        Layout::Beside => PaneLayout::Beside,
        Layout::Below => PaneLayout::Below,
    };
    let glyphs = match scene.emblem {
        Emblem::Sextant => Glyphs::Sextant,
        Emblem::Quadrant => Glyphs::Quadrant,
    };
    let queue = scene.queue.as_deref().unwrap_or("default");
    let mut view = View::new(queue, layout, images)
        .chosen_queue(queue != "default")
        .glyphs(glyphs)
        .home(std::env::var_os("HOME").map(PathBuf::from))
        .wake_on_load(move || {
            // The receiver lives as long as the scene is played.
            let _ = wake.send(());
        });
    let mut terminal = Terminal::new(TestBackend::new(scene.columns, scene.rows))?;
    let mut picture = Buffer::empty(terminal.get_frame().area());

    let mut frame = |app: &App| -> Result<Drawn> {
        let mut drawn = Drawn::default();
        let completed = terminal.draw(|frame| drawn = draw(frame, app, &mut view))?;
        picture = completed.buffer.clone();
        Ok(drawn)
    };
    play_keys(&mut app, &scene.keys, scene, &mut |app| {
        step(&mut frame, app, &wakes, cap)
    })?;
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::sync::mpsc;

    use asqr::tui::{Page, PanePage};
    use ratatui::style::Modifier;

    use super::*;
    use crate::scene::parse_scene;

    const CAP: Duration = Duration::from_secs(60);

    /// What a frame measured of a pane that shows `loading…` or not.
    fn pane(loading: bool) -> Drawn {
        Drawn {
            pane_page: Some(Some(PanePage {
                page: Page::default(),
                loading,
                diff: None,
            })),
            ..Drawn::default()
        }
    }

    // =================================================================
    // The step
    // =================================================================

    #[test]
    fn a_step_draws_again_after_a_wake_until_the_pane_has_loaded() {
        let (wake, wakes) = mpsc::channel();
        wake.send(()).expect("the channel is open");
        let draws = Cell::new(0);
        let mut draw = |_: &App| {
            draws.set(draws.get() + 1);
            Ok(pane(draws.get() == 1))
        };

        step(&mut draw, &mut App::default(), &wakes, CAP).expect("the pane loads");

        assert_eq!(draws.get(), 2);
    }

    #[test]
    fn a_step_gives_up_after_its_cap() {
        let (_wake, wakes) = mpsc::channel();
        let mut draw = |_: &App| Ok(pane(true));

        let error = step(&mut draw, &mut App::default(), &wakes, Duration::ZERO)
            .map(|()| "no error".to_owned())
            .unwrap_or_else(|error| format!("{error:#}"));

        assert!(
            error.contains("the context pane still shows loading… after"),
            "{error}"
        );
    }

    #[test]
    fn a_step_settles_before_the_next_key() {
        let scene =
            parse_scene("caption = \"c\"\ncolumns = 80\nrows = 24\nkeys = [\"j\", \"k\"]\n")
                .expect("the scene parses");
        let steps = Cell::new(0);
        let mut step = |_: &mut App| {
            steps.set(steps.get() + 1);
            Ok(())
        };

        play_keys(&mut App::default(), &scene.keys, &scene, &mut step).expect("plays");

        assert_eq!(steps.get(), 3, "the first frame and one after each key");
    }

    // =================================================================
    // Scenes
    // =================================================================

    /// The scene in `text` played from a scene file in `tools/screens/scenes`.
    fn played(text: &str) -> Result<Buffer> {
        let scene = parse_scene(text).expect("the scene parses");
        let scene_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenes/test.toml");
        play(&scene, &scene_file, CAP)
    }

    fn screen(text: &str) -> Buffer {
        played(text).expect("the scene plays")
    }

    /// The error playing `text` gives.
    fn error(text: &str) -> String {
        match played(text) {
            Ok(_) => "no error".to_owned(),
            Err(error) => format!("{error:#}"),
        }
    }

    /// The buffer's rows as text.
    fn text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    const RELEASE: &str =
        "[[sessions]]\nfile = \"../../../examples/sessions/release-checklist.json\"\n";
    const SIZE: &str = "caption = \"c\"\ncolumns = 80\nrows = 24\n";

    fn scene_with(keys: &str, rest: &str) -> String {
        format!("{SIZE}keys = [{keys}]\n{rest}")
    }

    const SWITCHER: &str = "[switcher]\ntmux_session = \"asqr\"\nroot = \"/data/asqr\"\n\
        [[switcher.queues]]\nname = \"default\"\nwatched = true\n\
        [[switcher.queues]]\nname = \"other\"\nwaiting = 3\n";

    #[test]
    fn keys_reach_the_app() {
        let buf = screen(&scene_with("\"?\"", RELEASE));

        assert!(text(&buf).contains("Keys"), "{}", text(&buf));
    }

    #[test]
    fn the_session_s_question_shows() {
        let buf = screen(&scene_with("", RELEASE));

        assert!(text(&buf).contains("Publish 0.2.0 now?"), "{}", text(&buf));
    }

    #[test]
    fn text_types_each_character_into_a_field() {
        let buf = screen(&scene_with(
            "\"l\", \"l\", { text = \"Fewer retries\" }",
            RELEASE,
        ));

        assert!(text(&buf).contains("Fewer retries"), "{}", text(&buf));
    }

    #[test]
    fn paste_pastes_into_a_field() {
        let buf = screen(&scene_with(
            "\"l\", \"l\", { paste = \"Pasted words\" }",
            RELEASE,
        ));

        assert!(text(&buf).contains("Pasted words"), "{}", text(&buf));
    }

    /// The text cursor of a field is a plain reversed cell (`theme::cursor`).
    #[test]
    fn a_field_entered_from_above_shows_its_cursor_on_its_first_row() {
        // The own answer of the second question takes text over several
        // rows; leaving it upwards and coming back down puts the cursor at
        // the end of its first row, which asqr finds only in a second frame.
        let words = "word ".repeat(30);
        let buf = screen(&scene_with(
            &format!(
                "\"l\", \"j\", \"j\", \"j\", {{ text = \"{words}\" }}, \
                 \"up\", \"up\", \"up\", \"down\""
            ),
            RELEASE,
        ));

        let cursor = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .find(|&(x, y)| {
                let cell = &buf[(x, y)];
                cell.modifier.contains(Modifier::REVERSED)
                    && cell.fg == ratatui::style::Color::Reset
            });
        let first_row = (0..buf.area.height).find(|&y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("word word")
        });
        assert!(
            matches!((cursor, first_row), (Some((_, row)), Some(first)) if row == first),
            "cursor {cursor:?}, first row of the answer {first_row:?}:\n{}",
            text(&buf)
        );
    }

    #[test]
    fn w_opens_the_scene_s_switcher() {
        let buf = screen(&scene_with("\"w\"", &format!("{RELEASE}{SWITCHER}")));

        assert!(text(&buf).contains("other"), "{}", text(&buf));
    }

    #[test]
    fn a_dir_queue_shows_as_written() {
        let buf = screen(&scene_with(
            "\"w\"",
            &format!("{RELEASE}[switcher]\n[[switcher.queues]]\ndir = \"~/work/queue\"\n"),
        ));

        assert!(text(&buf).contains("~/work/queue"), "{}", text(&buf));
    }

    /// The row of the first line that holds `needle`.
    fn row_of(buf: &Buffer, needle: &str) -> Option<usize> {
        text(buf).lines().position(|line| line.contains(needle))
    }

    const REVIEW: &str = "[[sessions]]\nfile = \"../../../examples/sessions/retry-review.json\"\n";

    #[test]
    fn layout_below_puts_the_context_under_the_question() {
        let buf = screen(&format!(
            "caption = \"c\"\ncolumns = 120\nrows = 40\nlayout = \"below\"\n{REVIEW}"
        ));

        let (pane, option) = (row_of(&buf, "Retry with backoff"), row_of(&buf, "Not yet"));
        assert!(
            matches!((pane, option), (Some(pane), Some(option)) if pane > option),
            "{}",
            text(&buf)
        );
    }

    #[test]
    fn layout_beside_puts_the_context_beside_the_question() {
        // A portrait screen, where `auto` puts it below.
        let buf = screen(&format!(
            "caption = \"c\"\ncolumns = 120\nrows = 80\nlayout = \"beside\"\n{REVIEW}"
        ));

        let (pane, question) = (
            row_of(&buf, "Retry with backoff"),
            row_of(&buf, "The fetcher retries"),
        );
        assert!(
            matches!((pane, question), (Some(pane), Some(question)) if pane <= question),
            "{}",
            text(&buf)
        );
    }

    /// Whether `buf` holds a character of `range`.
    fn holds(buf: &Buffer, range: std::ops::RangeInclusive<u32>) -> bool {
        text(buf).chars().any(|ch| range.contains(&u32::from(ch)))
    }

    #[test]
    fn talqi_is_drawn_in_sextants_or_in_quadrants_as_the_scene_says() {
        let tall = "caption = \"c\"\ncolumns = 80\nrows = 32\n";
        let sextants = screen(tall);
        let quadrants = screen(&format!("{tall}emblem = \"quadrant\"\n"));

        assert!(holds(&sextants, 0x1FB00..=0x1FB3B), "{}", text(&sextants));
        assert!(
            !holds(&quadrants, 0x1FB00..=0x1FB3B),
            "{}",
            text(&quadrants)
        );
        assert!(holds(&quadrants, 0x2596..=0x259F), "{}", text(&quadrants));
    }

    #[test]
    fn w_without_a_switcher_is_an_error() {
        let error = error(&scene_with("\"w\"", RELEASE));

        assert!(
            error.contains(
                "key 1 (\"w\") opens the queue switcher, and the scene has no [switcher]"
            ),
            "{error}"
        );
    }

    #[test]
    fn esc_in_the_switcher_shows_the_scene_s_elsewhere_again() {
        let buf = screen(&scene_with(
            "\"w\", \"esc\"",
            &format!("{RELEASE}[[elsewhere]]\nqueue = \"far-away\"\nwaiting = 4\n{SWITCHER}"),
        ));

        assert!(text(&buf).contains("far-away"), "{}", text(&buf));
    }

    #[test]
    fn a_pick_shows_and_writes_no_draft() {
        let buf = screen(&scene_with("\"j\", \"space\"", RELEASE));

        assert!(text(&buf).contains("Later"), "{}", text(&buf));
    }

    #[test]
    fn elsewhere_shows_in_the_status_line() {
        let buf = screen(&format!(
            "{SIZE}{RELEASE}[[elsewhere]]\nqueue = \"far-away\"\nwaiting = 4\n"
        ));

        assert!(text(&buf).contains("far-away"), "{}", text(&buf));
    }

    #[test]
    fn q_is_refused() {
        let error = error(&scene_with("\"q\"", RELEASE));

        assert!(
            error.contains("key 1 (\"q\") quits asqr, which scenes do not play"),
            "{error}"
        );
    }

    #[test]
    fn submitting_is_refused() {
        let error = error(&scene_with(
            "\"enter\", \"enter\"",
            "[[sessions]]\nfile = \"../../../examples/sessions/minimal.json\"\n",
        ));

        assert!(
            error.contains("finishes the session, which scenes do not play"),
            "{error}"
        );
    }

    #[test]
    fn o_is_refused() {
        // The first question's options show its diff.
        let error = error(&scene_with(
            "\"o\"",
            "[[sessions]]\nfile = \"../../../examples/sessions/retry-review.json\"\n",
        ));

        assert!(
            error.contains("key 1 (\"o\") opens an entry, which scenes do not play"),
            "{error}"
        );
    }

    #[test]
    fn ctrl_g_is_refused() {
        let error = error(&scene_with("\"l\", \"l\", \"ctrl-g\"", RELEASE));

        assert!(
            error.contains("key 3 (\"ctrl-g\") opens an editor, which scenes do not play"),
            "{error}"
        );
    }

    #[test]
    fn switching_is_refused() {
        let error = error(&scene_with(
            "\"w\", \"j\", \"enter\"",
            &format!("{RELEASE}{SWITCHER}"),
        ));

        assert!(
            error.contains("key 3 (\"enter\") switches the queue, which scenes do not play"),
            "{error}"
        );
    }

    #[test]
    fn a_follow_up_shows_the_title_of_the_session_it_follows() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(
            dir.path().join("first.json"),
            r#"{"asqr": 2, "id": "first", "title": "The first round",
                "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
        )
        .expect("written");
        std::fs::write(
            dir.path().join("second.json"),
            r#"{"asqr": 2, "id": "second", "title": "The second round", "follows": "first",
                "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#,
        )
        .expect("written");
        let scene = parse_scene(&format!(
            "{SIZE}keys = [\"L\", \"j\", \"enter\"]\n\
             [[sessions]]\nfile = \"first.json\"\n[[sessions]]\nfile = \"second.json\"\n"
        ))
        .expect("the scene parses");

        let buf = play(&scene, &dir.path().join("scene.toml"), CAP).expect("plays");

        assert!(text(&buf).contains("The first round"), "{}", text(&buf));
    }

    #[test]
    fn the_default_queue_is_not_named_and_another_is() {
        let named = screen(&format!("{SIZE}queue = \"asqr-todos\"\n"));
        let default = screen(&format!("{SIZE}queue = \"default\"\n"));

        // The title bar names the queue either way; the empty queue below
        // it names a chosen one only.
        let below_the_title =
            |buf: &Buffer| text(buf).lines().skip(1).collect::<Vec<_>>().join("\n");
        assert!(
            below_the_title(&named).contains("asqr-todos"),
            "{}",
            text(&named)
        );
        assert!(
            !below_the_title(&default).contains("default"),
            "{}",
            text(&default)
        );
    }

    /// Guard: the picture is the frame drawn after the last key, here the
    /// session list over the question.
    #[test]
    fn the_picture_is_the_last_frame_of_the_screen() {
        let buf = screen(&scene_with("\"L\"", RELEASE));

        assert!(text(&buf).contains("answered"), "{}", text(&buf));
    }

    /// Guard over the real loading path: a key after a load acts on the
    /// loaded entry.
    #[test]
    fn a_key_after_a_load_acts_on_the_loaded_entry() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let code: String = (1..=200).map(|n| format!("line_{n}\n")).collect();
        std::fs::write(dir.path().join("long.txt"), code).expect("written");
        std::fs::write(
            dir.path().join("code.json"),
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
                "context": [{"type": "code", "title": "long.txt", "path": "long.txt"}]}]}"#,
        )
        .expect("written");
        let scene = parse_scene(&format!(
            "{SIZE}keys = [\"tab\", \"ctrl-d\"]\n[[sessions]]\nfile = \"code.json\"\n"
        ))
        .expect("the scene parses");

        let buf = play(&scene, &dir.path().join("scene.toml"), CAP).expect("plays");

        // The pane shows 9 lines; half a page down starts it at line 5.
        let shown = text(&buf);
        assert!(shown.contains("5–13 of 200"), "{shown}");
    }

    #[test]
    fn a_half_block_image_keeps_its_proportions() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        image::RgbImage::from_pixel(300, 300, image::Rgb([200, 40, 40]))
            .save(dir.path().join("square.png"))
            .expect("written");
        std::fs::write(
            dir.path().join("image.json"),
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
                "context": [{"type": "image", "path": "square.png"}]}]}"#,
        )
        .expect("written");
        let scene = parse_scene(
            "caption = \"c\"\ncolumns = 120\nrows = 40\n[[sessions]]\nfile = \"image.json\"\n",
        )
        .expect("the scene parses");

        let buf = play(&scene, &dir.path().join("scene.toml"), CAP).expect("plays");

        // The cells the picture's red covers, as half blocks paint it.
        let red = ratatui::style::Color::Rgb(200, 40, 40);
        let cells: Vec<(u16, u16)> = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .filter(|&(x, y)| buf[(x, y)].fg == red || buf[(x, y)].bg == red)
            .collect();
        let columns = cells
            .iter()
            .map(|&(x, _)| x)
            .max()
            .zip(cells.iter().map(|&(x, _)| x).min());
        let rows = cells
            .iter()
            .map(|&(_, y)| y)
            .max()
            .zip(cells.iter().map(|&(_, y)| y).min());
        let (Some((right, left)), Some((bottom, top))) = (columns, rows) else {
            panic!("no red cells:\n{}", text(&buf));
        };
        let ratio = f64::from(right - left + 1) / f64::from(bottom - top + 1);
        assert!(
            (2.3..=2.9).contains(&ratio),
            "a square takes {} by {} cells, a ratio of {ratio}",
            right - left + 1,
            bottom - top + 1
        );
    }
}
