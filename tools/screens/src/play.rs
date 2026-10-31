// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Playing a scene through asqr's own code: the app with the scene's
//! sessions, its keys, and the frames asqr draws between them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use asqr::format::{Answer, SessionResult, Status, same_session_id};
use asqr::graphics::{CellSize, Graphics, Images};
use asqr::queue::{HistoryOutcome, ReplayFile};
use asqr::queue::{QueueLocation, WaitingQueue};
use asqr::settings::{Lock, Locks, SettingsStore};
use asqr::tui::ReplayInfo;
use asqr::tui::render::{Drawn, Glyphs, PaneLayout, View, draw, draw_picker};
use asqr::tui::{
    App, AppEffect, Invalid, Notice, Outcome, PanePage, Place, QueueChoice, QueueRow, REFUSED_NAME,
    SessionState, Switcher, SwitcherKey, TOO_LONG_NAME, TakenQueue, Tone,
};
use clap::CommandFactory;
use jiff::tz::TimeZone;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::fonts::{CELL_HEIGHT, CELL_WIDTH};
use crate::scene::{
    ContextHeldBy, Emblem, Key, Layout, ReplayOutcome, ReplayScene, Scene, load_sessions,
};

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
        // The session leaves the screen as it does in asqr's inbox, with the
        // notice that says how it ended. No result is written: a picture
        // leaves nothing on the disk.
        AppEffect::Finish { id, outcome } => {
            let confirmation = app
                .sessions()
                .iter()
                .find(|state| state.id() == id)
                .map(|state| confirmation(state, &outcome));
            app.remove(&id);
            if let Some(confirmation) = confirmation {
                app.show(confirmation);
            }
            Ok(())
        }
        AppEffect::Quit => Err(refused("quits asqr")),
        AppEffect::Open(_) => Err(refused("opens an entry")),
        AppEffect::Edit(_) => Err(refused("opens an editor")),
        AppEffect::Switch(_) => Err(refused("switches the queue")),
        // The value changed in memory already; a picture never writes the person's settings file.
        AppEffect::SaveSetting(_) => Ok(()),
    }
}

/// The status line's word that `state` was finished as `outcome` (spec
/// section 7.8), as `confirmation` in `src/tui/inbox.rs` words it.
fn confirmation(state: &SessionState, outcome: &Outcome) -> Notice {
    let name = state.name();
    match outcome {
        Outcome::Submitted { .. } => {
            let counts = state.counts();
            let after = format!(
                "\" · {} answered, {} skipped",
                counts.answered, counts.skipped
            );
            Notice::naming(Tone::Success, "✔ sent \"", name, &after)
        }
        Outcome::Rejected { .. } => Notice::naming(Tone::Info, "✗ rejected \"", name, "\""),
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
            let (location, label, agent_dir) = match (&queue.name, &queue.dir) {
                (Some(name), _) => (
                    QueueLocation::named(
                        root.as_deref()
                            .expect("a scene with named queues has a root"),
                        name,
                    ),
                    name.clone(),
                    None,
                ),
                (None, dir) => {
                    let dir = dir.clone().expect("a queue without a name has a dir");
                    (
                        QueueLocation::at(PathBuf::from(&dir)),
                        dir.clone(),
                        Some(dir),
                    )
                }
            };
            QueueRow {
                location,
                label,
                agent_dir,
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
    if scene.start_picker {
        return play_picker(scene);
    }
    let mut app = App::with_settings(settings_store(scene));
    for state in load_sessions(scene, scene_file)? {
        match &scene.replay {
            Some(replay) => {
                app.add(replayed(state, replay, scene, scene_file)?);
                app.set_replay_waiting(replay.waiting);
            }
            None => app.add(state),
        }
    }
    for invalid in &scene.invalid {
        app.report_invalid(Invalid {
            id: invalid.id.clone(),
            errors: invalid.errors.clone(),
        });
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
        .home(scene.home.clone())
        .wake_on_load(move || {
            // The receiver lives as long as the scene is played.
            let _ = wake.send(());
        });
    let mut terminal = Terminal::new(TestBackend::new(scene.columns, scene.rows))?;
    let mut picture = Buffer::empty(terminal.get_frame().area());

    // The view takes the settings again once the pane changed one, as the
    // run loop does (`Applied::SettingsChanged` in `src/tui/run.rs`). The
    // view starts with the scene's `layout`, which the settings of a run
    // without a held `Context` do not override before a change.
    let mut applied = app.settings().effective();
    let mut frame = |app: &App| -> Result<Drawn> {
        let settings = app.settings().effective();
        if settings != applied {
            view.set_settings(&settings);
            applied = settings;
        }
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

/// `state` as a finished session shown again, as `asqr replay` builds it
/// (`Replay::state` in `src/cli/replay.rs`): filled in with the answers of
/// the scene's result, and told what the archive recorded.
fn replayed(
    state: SessionState,
    replay: &ReplayScene,
    scene: &Scene,
    scene_file: &Path,
) -> Result<SessionState> {
    let base = scene_file.parent().unwrap_or(Path::new(""));
    let session_dir = base
        .join(&scene.sessions[0].file)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let result = match &replay.result {
        Some(file) => {
            let file = base.join(file);
            let bytes = std::fs::read(&file)
                .with_context(|| format!("reading the result {}", file.display()))?;
            Some(
                serde_json::from_slice::<SessionResult>(&bytes)
                    .with_context(|| format!("reading the result {}", file.display()))?,
            )
        }
        None => None,
    };
    let draft = result
        .as_ref()
        .and_then(|result| draft_of(state.session(), result));

    // The archive keys each file by the path the session names it at, which
    // `asqr ask` made absolute against the session file.
    let files = replay
        .files
        .iter()
        .map(|file| {
            let at = std::path::absolute(session_dir.join(&file.path))
                .with_context(|| format!("making {} absolute", file.path))?;
            let recorded = ReplayFile {
                original_path: file.original_path.clone(),
                state: file.state.clone(),
                sha256: file.sha256.clone(),
                size: file.size,
                modified: file.modified.clone(),
                kept_size: file.kept_size,
                reason: file.reason.clone(),
                copy: file.copy.clone(),
                damaged: file.damaged.clone(),
            };
            Ok((at.to_string_lossy().into_owned(), recorded))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let info = ReplayInfo {
        reference: replay.reference.clone(),
        outcome: match replay.outcome {
            ReplayOutcome::Sent => HistoryOutcome::Sent,
            ReplayOutcome::Rejected => HistoryOutcome::Rejected,
            ReplayOutcome::Error => HistoryOutcome::Error,
            ReplayOutcome::Withdrawn => HistoryOutcome::Withdrawn,
            ReplayOutcome::NoResult => HistoryOutcome::NoResult,
        },
        finished: replay.finished.clone(),
        files,
        bundle: replay.bundle.clone(),
        // UTC, so the times of the files read the same on every machine.
        zone: TimeZone::UTC,
    };
    Ok(
        SessionState::replay(state.id(), state.session().clone(), draft.as_ref())
            .with_replay_info(info),
    )
}

/// The answers a replay fills a session with, as `draft_of` in
/// `src/queue/replay.rs` takes them from a result: those of a sent or a
/// rejected result and the unsent draft of a withdrawn one. A skipped
/// answer is marked cleared, so a default does not show as picked.
fn draft_of(session: &asqr::format::Session, result: &SessionResult) -> Option<SessionResult> {
    let (answers, note, reason) = match result.status {
        Status::Submitted | Status::Cancelled => (
            result.answers.clone(),
            result.note.clone(),
            result.reason.clone(),
        ),
        Status::Withdrawn => {
            let unsent = result.draft.as_ref()?;
            (unsent.answers.clone(), unsent.note.clone(), None)
        }
        Status::Error | Status::Draft => return None,
    };
    let answers = answers
        .into_iter()
        .map(|answer| Answer {
            cleared: answer.cleared || answer.skipped,
            ..answer
        })
        .collect();
    let first = session
        .questions
        .first()
        .map_or("", |question| question.id.as_str());
    let mut draft = SessionResult::draft(&result.id, first, answers);
    draft.note = note;
    draft.reason = reason;
    Some(draft)
}

/// The settings a run of `scene` has: the file the scene names, if any, and
/// the `Context` setting held at the scene's `layout`.
fn settings_store(scene: &Scene) -> SettingsStore {
    let Some(file) = &scene.settings else {
        return SettingsStore::default();
    };
    let held = file.context_held_by.map(|by| {
        let layout = match scene.layout {
            Layout::Auto => PaneLayout::Auto,
            Layout::Beside => PaneLayout::Beside,
            Layout::Below => PaneLayout::Below,
        };
        let lock = match by {
            ContextHeldBy::Flag => Lock::Flag("--layout"),
            ContextHeldBy::Variable => Lock::Variable("ASQR_LAYOUT"),
        };
        (layout, lock)
    });
    let locks = Locks {
        context: held,
        ..Locks::default()
    };
    SettingsStore::load(file.path.clone(), locks).store
}

/// The start picker of `scene` (spec section 7.1) after its keys. The keys
/// go to the picker as they do in `pick_queue` (`src/tui/picker.rs`), and a
/// key that would take a queue is refused: a picture cannot show what comes
/// after it.
fn play_picker(scene: &Scene) -> Result<Buffer> {
    let switcher = scene
        .switcher
        .as_ref()
        .expect("a scene with a start picker has a switcher, which parse_scene checks");
    let mut picker = Switcher::picker(queue_choice(switcher));
    let version = format!(
        "asqr {}",
        asqr::cli::Cli::command().get_version().unwrap_or_default()
    );
    let mut terminal = Terminal::new(TestBackend::new(scene.columns, scene.rows))?;
    let mut picture = Buffer::empty(terminal.get_frame().area());
    // The pictures show the colours of the terminal, so NO_COLOR is not set.
    let mut draw_picker_frame = |picker: &Switcher, notice: Option<&Notice>| -> Result<()> {
        let completed =
            terminal.draw(|frame| draw_picker(frame, picker, notice, &version, false))?;
        picture = completed.buffer.clone();
        Ok(())
    };

    draw_picker_frame(&picker, None)?;
    for (index, key) in scene.keys.iter().enumerate() {
        let number = index + 1;
        let refused = |what: &str| anyhow!("key {number} ({key}) {what}, which scenes do not play");
        let answers = match key {
            Key::Press { event, .. } => vec![picker.handle(*event)],
            Key::Text(text) => text
                .chars()
                .map(|ch| picker.handle(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)))
                .collect(),
            Key::Paste(text) => vec![picker.paste(text)],
        };
        let mut notice = None;
        for answer in answers {
            notice = match answer {
                SwitcherKey::Stay | SwitcherKey::Close => None,
                SwitcherKey::Quit => return Err(refused("quits asqr")),
                SwitcherKey::Switch(_) => return Err(refused("takes a queue")),
                SwitcherKey::Refused => Some(Notice::new(Tone::Warning, REFUSED_NAME)),
                SwitcherKey::TooLong => Some(Notice::new(Tone::Warning, TOO_LONG_NAME)),
            };
        }
        draw_picker_frame(&picker, notice.as_ref())?;
    }
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
    fn a_setting_changed_in_the_pane_shows_and_is_not_saved() {
        let buf = screen(&scene_with("\",\", \"right\"", RELEASE));

        assert!(text(&buf).contains("‹ side by side ›"), "{}", text(&buf));
    }

    #[test]
    fn q_is_refused() {
        let error = error(&scene_with("\"q\"", RELEASE));

        assert!(
            error.contains("key 1 (\"q\") quits asqr, which scenes do not play"),
            "{error}"
        );
    }

    const MINIMAL: &str = "[[sessions]]\nfile = \"../../../examples/sessions/minimal.json\"\n";

    #[test]
    fn a_submitted_session_leaves_with_the_notice_of_a_sent_one() {
        let buf = screen(&scene_with(
            "{ text = \"Wait\" }, \"enter\", \"enter\"",
            MINIMAL,
        ));

        let shown = text(&buf);
        assert!(
            shown.contains("✔ sent \"minimal\" · 1 answered, 0 skipped"),
            "{shown}"
        );
        assert!(shown.contains("Nothing to answer."), "{shown}");
    }

    #[test]
    fn a_session_rejected_with_a_reason_leaves_with_its_notice() {
        let buf = screen(&scene_with(
            "\"enter\", \"down\", \"down\", \"enter\"",
            MINIMAL,
        ));

        assert!(
            text(&buf).contains("✗ rejected \"minimal\""),
            "{}",
            text(&buf)
        );
    }

    #[test]
    fn the_next_session_takes_over_after_a_finish() {
        let buf = screen(&scene_with(
            "\"enter\", \"enter\"",
            "[[sessions]]\nfile = \"../../../examples/sessions/minimal.json\"\n\
             [[sessions]]\nfile = \"../../../examples/sessions/release-checklist.json\"\n",
        ));

        assert!(text(&buf).contains("Release 0.2.0"), "{}", text(&buf));
    }

    #[test]
    fn an_invalid_session_shows_its_dialog_with_each_error() {
        let buf = screen(&scene_with(
            "",
            "[[invalid]]\nid = \"broken\"\n\
             errors = [\"questions: at least one question is needed\", \"asqr: must be 2\"]\n",
        ));

        let shown = text(&buf);
        assert!(shown.contains("broken"), "{shown}");
        assert!(
            shown.contains("• questions: at least one question is needed"),
            "{shown}"
        );
        assert!(shown.contains("• asqr: must be 2"), "{shown}");
        assert!(shown.contains("any key"), "{shown}");
    }

    #[test]
    fn a_key_closes_the_dialog_of_an_invalid_session() {
        let buf = screen(&scene_with(
            "\"x\"",
            &format!("{RELEASE}[[invalid]]\nid = \"broken\"\nerrors = [\"asqr: must be 2\"]\n"),
        ));

        assert!(!text(&buf).contains("asqr: must be 2"), "{}", text(&buf));
    }

    #[test]
    fn the_context_row_names_the_flag_that_holds_it() {
        let buf = screen(&scene_with(
            "\",\", \"j\", \"j\"",
            &format!(
                "layout = \"below\"\n{RELEASE}\
                 [settings]\npath = \"/nowhere/asqr/settings.toml\"\ncontext_held_by = \"flag\"\n"
            ),
        ));

        let shown = text(&buf);
        assert!(shown.contains("set by --layout"), "{shown}");
        assert!(shown.contains("/nowhere/asqr/settings.toml"), "{shown}");
    }

    #[test]
    fn the_settings_path_shows_with_the_scene_s_home_as_a_tilde() {
        let buf = screen(&scene_with(
            "\",\"",
            &format!(
                "home = \"/home/someone\"\n{RELEASE}\
                 [settings]\npath = \"/home/someone/.config/asqr/settings.toml\"\n"
            ),
        ));

        assert!(
            text(&buf).contains("~/.config/asqr/settings.toml"),
            "{}",
            text(&buf)
        );
    }

    #[test]
    fn the_context_row_names_the_variable_that_holds_it() {
        let buf = screen(&scene_with(
            "\",\", \"j\", \"j\"",
            &format!("layout = \"beside\"\n{RELEASE}[settings]\ncontext_held_by = \"variable\"\n"),
        ));

        assert!(text(&buf).contains("set by ASQR_LAYOUT"), "{}", text(&buf));
    }

    #[test]
    fn a_setting_changed_in_the_pane_applies_behind_it() {
        // The context entry of the first question goes below the question
        // once `Context` reads "always below".
        let rows = |keys: &str| {
            let buf = screen(&format!(
                "caption = \"c\"\ncolumns = 120\nrows = 30\nkeys = [{keys}]\n\
                 [[sessions]]\nfile = \"../../../examples/sessions/retry-review.json\"\n"
            ));
            let shown = text(&buf);
            let row_of = |needle: &str| {
                shown
                    .lines()
                    .position(|line| line.contains(needle))
                    .unwrap_or_else(|| panic!("no {needle:?} in\n{shown}"))
            };
            (row_of("Retry with backoff"), row_of("pick one"))
        };

        let (beside_pane, beside_hint) = rows("");
        let (below_pane, below_hint) = rows("\",\", \"j\", \"j\", \"right\", \"right\", \"esc\"");

        assert!(beside_pane <= beside_hint + 1, "beside by default");
        assert!(below_pane > below_hint, "below after the change");
    }

    const SENT: &str = "[replay]\nreference = \"H3WQ8N5X\"\noutcome = \"sent\"\n\
        finished = \"Thu 29 Oct 16:41\"\nresult = \"../results/release-sent.json\"\n";
    const WITHDRAWN: &str = "[replay]\nreference = \"D0M6Z4TB\"\noutcome = \"withdrawn\"\n\
        finished = \"Thu 29 Oct 11:20\"\nresult = \"../results/release-withdrawn.json\"\n";
    const FETCH: &str = "[[sessions]]\nfile = \"../../../examples/sessions/archived-fetch.json\"\n";

    /// A replay of the fetcher session, with the files `files` records.
    fn fetch_replay(keys: &str, files: &str) -> String {
        scene_with(
            keys,
            &format!(
                "{FETCH}[replay]\nreference = \"H3WQ8N5X\"\noutcome = \"sent\"\n\
                 finished = \"Thu 29 Oct 16:41\"\n{files}"
            ),
        )
    }

    const KEPT_CODE: &str = "[[replay.files]]\npath = \"../files/fetch.txt\"\n\
        original_path = \"/tmp/review/fetch.rs\"\nstate = \"kept\"\n\
        sha256 = \"9f2c61d0b7e3a4c8d1e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7e2b41a6\"\n\
        size = 713\nkept_size = 713\nmodified = \"2026-10-29T16:40:11Z\"\n";

    #[test]
    fn a_replay_is_read_only_and_says_so_in_the_title_and_the_status_line() {
        let buf = screen(&scene_with("", &format!("{RELEASE}{SENT}")));

        let shown = text(&buf);
        assert!(shown.contains("asqr ◷ read-only"), "{shown}");
        assert!(shown.contains("replay H3WQ8N5X"), "{shown}");
        assert!(
            shown.contains("◷ replay · sent Thu 29 Oct 16:41 · read-only"),
            "{shown}"
        );
    }

    #[test]
    fn a_replay_fills_in_the_answers_of_the_result_and_keys_change_none() {
        let before = screen(&scene_with("", &format!("{RELEASE}{SENT}")));
        let after = screen(&scene_with(
            "\"j\", \"space\", \"2\"",
            &format!("{RELEASE}{SENT}"),
        ));

        assert!(
            text(&before).contains("(•) 1. Publish now"),
            "{}",
            text(&before)
        );
        assert!(!text(&after).contains("(•) 2."), "{}", text(&after));
    }

    #[test]
    fn the_last_tab_of_a_replay_is_the_result() {
        let buf = screen(&scene_with(
            "\"enter\", \"enter\", \"enter\"",
            &format!("{RELEASE}{SENT}"),
        ));

        let shown = text(&buf);
        assert!(shown.contains("Result"), "{shown}");
        assert!(shown.contains("Sent Thu 29 Oct 16:41"), "{shown}");
        assert!(shown.contains("Startup is twice as fast."), "{shown}");
    }

    #[test]
    fn a_withdrawn_session_marks_the_unsent_pick_as_a_draft() {
        let buf = screen(&scene_with("", &format!("{RELEASE}{WITHDRAWN}")));

        let shown = text(&buf);
        assert!(shown.contains("◀ draft"), "{shown}");
        assert!(shown.contains("dim is your unsent draft"), "{shown}");
    }

    #[test]
    fn the_waiting_sessions_are_counted_in_a_replay() {
        let buf = screen(&scene_with("", &format!("{RELEASE}{SENT}waiting = 2\n")));

        assert!(text(&buf).contains("2 waiting"), "{}", text(&buf));
    }

    #[test]
    fn d_shows_the_details_of_a_kept_file() {
        let buf = screen(&fetch_replay("\"d\"", KEPT_CODE));

        let shown = text(&buf);
        assert!(shown.contains("/tmp/review/fetch.rs"), "{shown}");
        assert!(
            shown.contains("SHA-256   9f2c61d0b7e3a4c8d1e5f607"),
            "{shown}"
        );
    }

    #[test]
    fn a_file_the_archive_did_not_keep_shows_a_card_with_its_original_path() {
        let buf = screen(&fetch_replay(
            "\"right\"",
            "[[replay.files]]\npath = \"../files/notes.md\"\n\
             original_path = \"/tmp/review/notes.md\"\nstate = \"not_kept\"\n\
             sha256 = \"9f2c61d0b7e3a4c8d1e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7e2b41a6\"\n\
             size = 213456\nmodified = \"2026-10-29T16:40:11Z\"\n",
        ));

        assert!(
            text(&buf).contains("/tmp/review/notes.md"),
            "{}",
            text(&buf)
        );
    }

    #[test]
    fn a_file_that_was_missing_shows_the_red_card() {
        let buf = screen(&fetch_replay(
            "\"right\", \"right\"",
            "[[replay.files]]\npath = \"../files/test.log\"\n\
             original_path = \"/tmp/review/test.log\"\nstate = \"missing\"\n",
        ));

        assert!(
            text(&buf).contains("no copy, no fingerprint"),
            "{}",
            text(&buf)
        );
    }

    #[test]
    fn a_replay_needs_a_session_and_an_outcome_it_knows() {
        let none = parse_scene(&scene_with("", SENT)).expect_err("a replay without a session");
        let unknown = parse_scene(&scene_with(
            "",
            &format!("{RELEASE}[replay]\nreference = \"X\"\noutcome = \"lost\"\nfinished = \"\"\n"),
        ))
        .expect_err("an outcome nobody knows");

        assert!(
            format!("{none:#}").contains("a replay needs a session"),
            "{none:#}"
        );
        assert!(format!("{unknown:#}").contains("lost"), "{unknown:#}");
    }

    const PICKER: &str = "start_picker = true\n[switcher]\nroot = \"/data/asqr\"\n\
        [[switcher.queues]]\nname = \"default\"\nwaiting = 1\n\
        [[switcher.queues]]\nname = \"other\"\nwaiting = 3\n";

    #[test]
    fn the_start_picker_shows_its_queues_and_the_version_of_asqr() {
        let buf = screen(&scene_with("", PICKER));

        let shown = text(&buf);
        assert!(shown.contains("enter  watch"), "{shown}");
        assert!(shown.contains("3 waiting"), "{shown}");
        let command = asqr::cli::Cli::command();
        let version = command.get_version().unwrap_or_default();
        assert!(shown.contains(&format!("asqr {version}")), "{shown}");
    }

    #[test]
    fn keys_move_the_cursor_of_the_start_picker() {
        let first = screen(&scene_with("", PICKER));
        let second = screen(&scene_with("\"j\"", PICKER));

        assert_ne!(text(&first), text(&second));
    }

    #[test]
    fn watching_a_queue_in_the_start_picker_is_refused() {
        let error = error(&scene_with("\"enter\"", PICKER));

        assert!(
            error.contains("key 1 (\"enter\") takes a queue, which scenes do not play"),
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
