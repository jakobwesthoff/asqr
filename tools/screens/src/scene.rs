// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The scene files: one TOML file per picture, naming the sessions, the
//! keys to press, the terminal's size, the queue state and the caption.
//! The tool's README describes every field.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use asqr::format::{make_context_paths_absolute, parse_session, same_session_id, validate};
use asqr::tui::SessionState;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

/// A picture: what asqr shows and what the picture is of.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// What the picture shows, for the index of the pictures.
    pub caption: String,
    /// The terminal's size in cells.
    pub columns: u16,
    pub rows: u16,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub emblem: Emblem,
    /// The queue asqr watches, by name; the default queue without one.
    pub queue: Option<String>,
    /// The person's home folder, which asqr names `~` where a path lies
    /// below it. Without one nothing is named `~`, so a picture does not
    /// depend on the machine that draws it.
    pub home: Option<PathBuf>,
    #[serde(default)]
    pub keys: Vec<Key>,
    #[serde(default)]
    pub sessions: Vec<SessionEntry>,
    #[serde(default)]
    pub elsewhere: Vec<Elsewhere>,
    pub switcher: Option<Switcher>,
    /// Draws asqr's start picker, which comes before any session, over the
    /// `[switcher]`'s queues instead of asqr's screen; the keys go to it.
    #[serde(default)]
    pub start_picker: bool,
    /// Session files asqr answered with an error, whose dialogs show.
    #[serde(default)]
    pub invalid: Vec<InvalidSession>,
    pub settings: Option<SettingsFile>,
    /// Shows the scene's one session as `asqr replay` shows a finished
    /// session (spec section 7.11).
    pub replay: Option<ReplayScene>,
}

/// What the archive knows about the finished session a scene replays.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReplayScene {
    /// The REF `asqr history` prints for the session.
    pub reference: String,
    pub outcome: ReplayOutcome,
    /// When the session was finished, as the status line writes it, such
    /// as `Thu 29 Oct 16:41`.
    pub finished: String,
    /// The result file, in the result format, relative to the scene file,
    /// whose answers fill the session in as a replay fills them in.
    pub result: Option<PathBuf>,
    /// The month bundle the session came from, such as `2026-10.zip`.
    pub bundle: Option<String>,
    /// The sessions waiting in the queue, which the title bar counts.
    #[serde(default)]
    pub waiting: usize,
    /// The files the session showed, as the archive recorded them.
    #[serde(default)]
    pub files: Vec<ReplayFileScene>,
}

/// How a finished session ended, as `asqr history` words it.
#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ReplayOutcome {
    Sent,
    Rejected,
    Error,
    Withdrawn,
    #[serde(rename = "none")]
    NoResult,
}

/// A file a replayed session showed and what the archive recorded of it.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReplayFileScene {
    /// The path the session names the file at, as the session file writes
    /// it: relative to the session file, or absolute.
    pub path: String,
    pub original_path: String,
    /// `kept`, `not_kept`, `missing`, `unreadable` or `not_recorded`.
    pub state: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub modified: Option<String>,
    pub kept_size: Option<u64>,
    pub reason: Option<String>,
    /// Where the copy lay in the archive, such as `files/9f2c….rs`.
    pub copy: Option<String>,
    /// Why a kept copy cannot be shown.
    pub damaged: Option<String>,
}

/// Where the context pane goes, as asqr's `--layout`.
#[derive(Debug, Default, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Auto,
    Beside,
    Below,
}

/// The glyphs Talqi is drawn in on the empty queue. asqr draws sextants in
/// Ghostty (`TERM_PROGRAM=ghostty`), quadrants where it cannot tell.
#[derive(Debug, Default, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Emblem {
    #[default]
    Sextant,
    Quadrant,
}

/// A waiting session: its file, relative to the scene file, and an id
/// in place of the session's own.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionEntry {
    pub file: PathBuf,
    pub id: Option<String>,
}

/// A queue no asqr watches in which sessions wait.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Elsewhere {
    pub queue: String,
    pub waiting: usize,
}

/// A session file asqr found invalid and answered with an error, as the
/// dialog shows it.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InvalidSession {
    pub id: String,
    /// Each problem the file has, as validation words it.
    pub errors: Vec<String>,
}

/// The settings file as the settings pane names it, and what holds the
/// `Context` setting for the run.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SettingsFile {
    /// The path the frame's bottom edge names. The tool reads the file at
    /// that path if there is one, so a path in a folder that exists on no
    /// machine, such as `/home/someone/.config/asqr/settings.toml`, keeps a
    /// picture from depending on the machine that draws it.
    pub path: Option<PathBuf>,
    /// What holds `Context` at the scene's `layout`, as `--layout` or
    /// `ASQR_LAYOUT` does in asqr.
    pub context_held_by: Option<ContextHeldBy>,
}

/// What can hold the `Context` setting for a run.
#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ContextHeldBy {
    Flag,
    Variable,
}

/// What the queue switcher shows when a key opens it.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Switcher {
    /// The tmux session asqr runs in; without one, asqr runs outside tmux.
    pub tmux_session: Option<String>,
    /// The directory of the named queues, where a new queue goes.
    pub root: Option<PathBuf>,
    #[serde(default)]
    pub queues: Vec<SwitcherQueue>,
    #[serde(default)]
    pub taken: Vec<TakenQueue>,
}

/// A row of the switcher: a named queue under the root or a `--dir` queue.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SwitcherQueue {
    pub name: Option<String>,
    pub dir: Option<String>,
    #[serde(default)]
    pub waiting: usize,
    #[serde(default)]
    pub watched: bool,
}

/// A queue another asqr watches.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TakenQueue {
    pub name: String,
    pub tmux_session: Option<String>,
}

// =====================================================================
// Keys
// =====================================================================

/// What a scene does between two pictures of the screen.
#[derive(Debug, PartialEq)]
pub enum Key {
    /// One key, with the name the scene gave it.
    Press {
        name: String,
        event: KeyEvent,
    },
    /// Each character typed as a key of its own.
    Text(String),
    Paste(String),
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Press { name, .. } => write!(formatter, "{name:?}"),
            Key::Text(text) => write!(formatter, "text {text:?}"),
            Key::Paste(text) => write!(formatter, "paste {text:?}"),
        }
    }
}

/// The key `name` stands for: a single character, or a key's name as asqr's
/// spec writes it, with `ctrl-` or `alt-` in front.
pub fn parse_key(name: &str) -> Result<KeyEvent> {
    let (modifiers, key) = if let Some(key) = name.strip_prefix("ctrl-") {
        (KeyModifiers::CONTROL, key)
    } else if let Some(key) = name.strip_prefix("alt-") {
        (KeyModifiers::ALT, key)
    } else {
        (KeyModifiers::NONE, name)
    };
    let mut chars = key.chars();
    let code = match (chars.next(), chars.next()) {
        (Some(ch), None) => KeyCode::Char(ch),
        _ => match key {
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            // Terminals report shift-tab as a back tab with shift held.
            "shift-tab" => {
                return Ok(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
            }
            "space" => KeyCode::Char(' '),
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pgup" => KeyCode::PageUp,
            "pgdn" => KeyCode::PageDown,
            _ => anyhow::bail!("unknown key {name:?}"),
        },
    };
    Ok(KeyEvent::new(code, modifiers))
}

/// A key is a string, a key's name, or a table with exactly one of `text`
/// and `paste`. Written by hand, as serde's untagged enums report only
/// that nothing matched, never which field was wrong.
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Key, D::Error> {
        struct KeyVisitor;

        impl<'de> serde::de::Visitor<'de> for KeyVisitor {
            type Value = Key;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a key's name, { text = \"...\" } or { paste = \"...\" }")
            }

            fn visit_str<E: serde::de::Error>(self, name: &str) -> Result<Key, E> {
                let event = parse_key(name).map_err(|error| E::custom(format!("{error:#}")))?;
                Ok(Key::Press {
                    name: name.to_owned(),
                    event,
                })
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Key, A::Error> {
                use serde::de::Error;
                let mut fields: Vec<(String, String)> = Vec::new();
                while let Some(field) = map.next_entry::<String, String>()? {
                    fields.push(field);
                }
                let names: Vec<&str> = fields.iter().map(|(name, _)| name.as_str()).collect();
                match fields.as_slice() {
                    [(name, text)] if name == "text" => {
                        if text.chars().any(char::is_control) {
                            return Err(A::Error::custom(
                                "`text` holds a control character; press it by its name, such as \"enter\" or \"tab\"",
                            ));
                        }
                        Ok(Key::Text(text.clone()))
                    }
                    [(name, text)] if name == "paste" => Ok(Key::Paste(text.clone())),
                    _ => Err(A::Error::custom(format!(
                        "a key table holds `text` or `paste`, not `{}`",
                        names.join("`, `")
                    ))),
                }
            }
        }

        deserializer.deserialize_any(KeyVisitor)
    }
}

// =====================================================================
// Reading a scene
// =====================================================================

/// The scene in `text`, checked.
pub fn parse_scene(text: &str) -> Result<Scene> {
    let scene: Scene = toml::from_str(text)?;
    if let Some(switcher) = &scene.switcher {
        for queue in &switcher.queues {
            match (&queue.name, &queue.dir) {
                (Some(_), Some(_)) => {
                    anyhow::bail!("a switcher queue has a `name` or a `dir`, not both")
                }
                (None, None) => anyhow::bail!("a switcher queue needs a `name` or a `dir`"),
                (Some(_), None) if switcher.root.is_none() => {
                    anyhow::bail!("the switcher needs a `root` for its named queues")
                }
                _ => {}
            }
        }
    }
    if scene.replay.is_some() && scene.sessions.len() != 1 {
        anyhow::bail!("a replay needs a session: the scene lists exactly one [[sessions]]");
    }
    if scene.start_picker && scene.switcher.is_none() {
        anyhow::bail!("`start_picker` needs a [switcher] with the queues to pick from");
    }
    Ok(scene)
}

/// The scene file at `path`.
pub fn read_scene(path: &Path) -> Result<Scene> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the scene file {}", path.display()))?;
    parse_scene(&text).with_context(|| format!("reading the scene file {}", path.display()))
}

/// The sessions of `scene`, read from their files next to `scene_file`, in
/// the order the scene lists them.
///
/// A session's id is the scene's `id` for it, else the session's own, else
/// its file's name: an inbox file is named after its session's id, and the
/// session is checked as if it were one. Relative paths in its context
/// entries are resolved against its file's directory, as `asqr ask` does.
pub fn load_sessions(scene: &Scene, scene_file: &Path) -> Result<Vec<SessionState>> {
    let base = scene_file.parent().unwrap_or(Path::new(""));
    let mut sessions: Vec<SessionState> = Vec::new();
    for entry in &scene.sessions {
        let file = base.join(&entry.file);
        let state = load_session(&file, entry.id.as_deref())
            .with_context(|| format!("reading the session {}", file.display()))?;
        if sessions
            .iter()
            .any(|other| same_session_id(other.id(), state.id()))
        {
            anyhow::bail!("two sessions have the id {:?}", state.id());
        }
        sessions.push(state);
    }
    Ok(sessions)
}

fn load_session(file: &Path, id: Option<&str>) -> Result<SessionState> {
    let bytes = std::fs::read(file)?;
    let mut raw: serde_json::Value = serde_json::from_slice(&bytes)?;
    make_context_paths_absolute(&mut raw, file);
    let bytes = serde_json::to_vec(&raw)?;
    let (_, session) = parse_session(&bytes)?;
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let id = id
        .map(str::to_owned)
        .or_else(|| session.id.clone())
        .unwrap_or(stem);
    if let Err(errors) = validate(&session, Some(&id)) {
        let errors: Vec<String> = errors.iter().map(ToString::to_string).collect();
        anyhow::bail!("{}", errors.join("; "));
    }
    Ok(SessionState::new(id, session, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// The message of the error `text` gives, causes included.
    fn error_of(text: &str) -> String {
        match parse_scene(text) {
            Ok(_) => "no error".to_owned(),
            Err(error) => format!("{error:#}"),
        }
    }

    const SIZE: &str = "caption = \"c\"\ncolumns = 80\nrows = 24\n";

    // =================================================================
    // Fields
    // =================================================================

    #[test]
    fn the_switcher_example_reads_every_field_it_has() {
        let scene =
            parse_scene(include_str!("../scenes/queue-switcher.toml")).expect("the scene parses");

        assert_eq!(
            scene.caption,
            "The queue switcher over a session, inside tmux"
        );
        assert_eq!((scene.columns, scene.rows), (80, 24));
        assert_eq!(
            (scene.layout, scene.emblem),
            (Layout::Auto, Emblem::Sextant)
        );
        assert_eq!(scene.queue.as_deref(), Some("asqr-todos"));
        assert_eq!(
            scene.keys,
            [Key::Press {
                name: "w".to_owned(),
                event: press(KeyCode::Char('w'), KeyModifiers::NONE)
            }]
        );
        assert_eq!(
            scene.sessions,
            [SessionEntry {
                file: "../../../examples/sessions/release-checklist.json".into(),
                id: None
            }]
        );
        assert_eq!(
            scene.elsewhere,
            [Elsewhere {
                queue: "default".to_owned(),
                waiting: 1
            }]
        );
        let switcher = scene.switcher.expect("the scene has a switcher");
        assert_eq!(switcher.tmux_session.as_deref(), Some("asqr"));
        assert_eq!(switcher.root, Some(PathBuf::from("/data/asqr")));
        assert_eq!(
            switcher.queues[1],
            SwitcherQueue {
                name: Some("asqr-todos".to_owned()),
                dir: None,
                waiting: 2,
                watched: true
            }
        );
        assert_eq!(
            (switcher.queues[0].waiting, switcher.queues[0].watched),
            (0, false),
            "the defaults"
        );
        assert_eq!(
            switcher.taken,
            [TakenQueue {
                name: "ntropy".to_owned(),
                tmux_session: Some("ntropy".to_owned())
            }]
        );
    }

    #[test]
    fn invalid_sessions_and_the_settings_file_read() {
        let scene = parse_scene(&format!(
            "{SIZE}layout = \"below\"\n\
             [[invalid]]\nid = \"broken\"\nerrors = [\"asqr: must be 2\"]\n\
             [settings]\npath = \"~/.config/asqr/settings.toml\"\ncontext_held_by = \"flag\"\n"
        ))
        .expect("the scene parses");

        assert_eq!(
            scene.invalid,
            [InvalidSession {
                id: "broken".to_owned(),
                errors: vec!["asqr: must be 2".to_owned()]
            }]
        );
        assert_eq!(
            scene.settings,
            Some(SettingsFile {
                path: Some(PathBuf::from("~/.config/asqr/settings.toml")),
                context_held_by: Some(ContextHeldBy::Flag)
            })
        );
    }

    #[test]
    fn a_start_picker_needs_a_switcher() {
        let error = error_of(&format!("{SIZE}start_picker = true\n"));

        assert!(
            error.contains("`start_picker` needs a [switcher]"),
            "{error}"
        );
    }

    #[test]
    fn a_context_hold_other_than_a_flag_or_a_variable_is_an_error() {
        let error = error_of(&format!("{SIZE}[settings]\ncontext_held_by = \"config\"\n"));

        assert!(
            error.contains("context_held_by") || error.contains("config"),
            "{error}"
        );
    }

    #[test]
    fn a_dir_queue_outside_tmux_with_quadrants_and_beside_reads() {
        let scene = parse_scene(&format!(
            "{SIZE}layout = \"beside\"\nemblem = \"quadrant\"\n\
             keys = [{{ text = \"ab\" }}, {{ paste = \"x y\" }}]\n\
             [[sessions]]\nfile = \"s.json\"\nid = \"chosen\"\n\
             [switcher]\n[[switcher.queues]]\ndir = \"~/work/queue\"\n"
        ))
        .expect("the scene parses");

        assert_eq!(
            (scene.layout, scene.emblem),
            (Layout::Beside, Emblem::Quadrant)
        );
        assert_eq!(
            scene.keys,
            [Key::Text("ab".to_owned()), Key::Paste("x y".to_owned())]
        );
        assert_eq!(
            scene.sessions.first().and_then(|entry| entry.id.as_deref()),
            Some("chosen")
        );
        let switcher = scene.switcher.expect("the scene has a switcher");
        assert_eq!(switcher.tmux_session, None);
        assert_eq!(switcher.queues[0].dir.as_deref(), Some("~/work/queue"));
    }

    #[test]
    fn key_names_read_as_asqr_s_keys() {
        let none = KeyModifiers::NONE;
        let rows = [
            ("j", press(KeyCode::Char('j'), none)),
            ("?", press(KeyCode::Char('?'), none)),
            ("L", press(KeyCode::Char('L'), none)),
            ("1", press(KeyCode::Char('1'), none)),
            ("enter", press(KeyCode::Enter, none)),
            ("esc", press(KeyCode::Esc, none)),
            ("tab", press(KeyCode::Tab, none)),
            ("shift-tab", press(KeyCode::BackTab, KeyModifiers::SHIFT)),
            ("space", press(KeyCode::Char(' '), none)),
            ("backspace", press(KeyCode::Backspace, none)),
            ("delete", press(KeyCode::Delete, none)),
            ("up", press(KeyCode::Up, none)),
            ("down", press(KeyCode::Down, none)),
            ("left", press(KeyCode::Left, none)),
            ("right", press(KeyCode::Right, none)),
            ("home", press(KeyCode::Home, none)),
            ("end", press(KeyCode::End, none)),
            ("pgup", press(KeyCode::PageUp, none)),
            ("pgdn", press(KeyCode::PageDown, none)),
            ("ctrl-d", press(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            ("ctrl-g", press(KeyCode::Char('g'), KeyModifiers::CONTROL)),
            ("alt-x", press(KeyCode::Char('x'), KeyModifiers::ALT)),
        ];
        let failures: Vec<String> = rows
            .into_iter()
            .filter_map(|(name, expected)| match parse_key(name) {
                Ok(event) if event == expected => None,
                Ok(event) => Some(format!("{name}: {event:?}, not {expected:?}")),
                Err(error) => Some(format!("{name}: {error:#}")),
            })
            .collect();
        assert!(
            failures.is_empty(),
            "failing rows:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn keys_name_themselves_in_messages() {
        let names: Vec<String> = [
            Key::Press {
                name: "ctrl-d".to_owned(),
                event: press(KeyCode::Char('d'), KeyModifiers::CONTROL),
            },
            Key::Text("ab".to_owned()),
            Key::Paste("x".to_owned()),
        ]
        .iter()
        .map(ToString::to_string)
        .collect();

        assert_eq!(names, ["\"ctrl-d\"", "text \"ab\"", "paste \"x\""]);
    }

    // =================================================================
    // Errors
    // =================================================================

    #[test]
    fn a_key_of_another_type_says_what_a_key_is() {
        let error = error_of(&format!("{SIZE}keys = [1]\n"));
        assert!(
            error.contains("a key's name, { text = \"...\" } or { paste = \"...\" }"),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_field_is_an_error_naming_it() {
        let error = error_of(&format!("{SIZE}colour = 1\n"));
        assert!(error.contains("unknown field `colour`"), "{error}");
    }

    #[test]
    fn an_unknown_key_name_is_an_error() {
        let error = error_of(&format!("{SIZE}keys = [\"page-down\"]\n"));
        assert!(error.contains("unknown key \"page-down\""), "{error}");
    }

    #[test]
    fn a_typo_in_a_key_table_names_it() {
        let error = error_of(&format!("{SIZE}keys = [{{ txt = \"abc\" }}]\n"));
        assert!(
            error.contains("a key table holds `text` or `paste`, not `txt`"),
            "{error}"
        );
    }

    #[test]
    fn a_control_character_in_text_is_an_error() {
        let error = error_of(&format!("{SIZE}keys = [{{ text = \"a\\n\" }}]\n"));
        assert!(error.contains("control character"), "{error}");
    }

    #[test]
    fn an_unknown_emblem_is_an_error() {
        let error = error_of(&format!("{SIZE}emblem = \"octant\"\n"));
        assert!(error.contains("unknown variant `octant`"), "{error}");
    }

    #[test]
    fn an_unknown_layout_is_an_error() {
        let error = error_of(&format!("{SIZE}layout = \"left\"\n"));
        assert!(error.contains("unknown variant `left`"), "{error}");
    }

    #[test]
    fn a_queue_with_name_and_dir_is_an_error() {
        let error = error_of(&format!(
            "{SIZE}[switcher]\nroot = \"/r\"\n[[switcher.queues]]\nname = \"a\"\ndir = \"/d\"\n"
        ));
        assert!(
            error.contains("a switcher queue has a `name` or a `dir`, not both"),
            "{error}"
        );
    }

    #[test]
    fn a_queue_with_neither_name_nor_dir_is_an_error() {
        let error = error_of(&format!(
            "{SIZE}[switcher]\n[[switcher.queues]]\nwaiting = 1\n"
        ));
        assert!(
            error.contains("a switcher queue needs a `name` or a `dir`"),
            "{error}"
        );
    }

    #[test]
    fn a_named_queue_without_a_root_is_an_error() {
        let error = error_of(&format!(
            "{SIZE}[switcher]\n[[switcher.queues]]\nname = \"a\"\n"
        ));
        assert!(
            error.contains("the switcher needs a `root` for its named queues"),
            "{error}"
        );
    }

    // =================================================================
    // Sessions
    // =================================================================

    /// A directory for session files, with a scene file beside them.
    struct Files {
        dir: tempfile::TempDir,
    }

    impl Files {
        fn new() -> Files {
            Files {
                dir: tempfile::tempdir().expect("a temporary directory"),
            }
        }

        fn session(&self, name: &str, json: &str) {
            std::fs::write(self.dir.path().join(name), json).expect("written");
        }

        fn load(&self, sessions: &str) -> Result<Vec<SessionState>> {
            let scene = parse_scene(&format!("{SIZE}{sessions}")).expect("the scene parses");
            load_sessions(&scene, &self.dir.path().join("scene.toml"))
        }

        /// The message of the error loading `sessions` gives.
        fn error(&self, sessions: &str) -> String {
            match self.load(sessions) {
                Ok(_) => "no error".to_owned(),
                Err(error) => format!("{error:#}"),
            }
        }
    }

    const PLAIN: &str = r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;
    const RELEASE: &str = r#"{"asqr": 2, "id": "release-0-2-0",
        "questions": [{"id": "q", "text": "?", "kind": "text"}]}"#;

    fn ids(sessions: &[SessionState]) -> Vec<&str> {
        sessions.iter().map(SessionState::id).collect()
    }

    #[test]
    fn a_session_with_its_own_id_keeps_it() {
        let files = Files::new();
        files.session("release.json", RELEASE);

        let sessions = files
            .load("[[sessions]]\nfile = \"release.json\"\n")
            .expect("loads");

        assert_eq!(ids(&sessions), ["release-0-2-0"]);
    }

    #[test]
    fn a_session_without_an_id_takes_its_file_name() {
        let files = Files::new();
        files.session("retry-review.json", PLAIN);

        let sessions = files
            .load("[[sessions]]\nfile = \"retry-review.json\"\n")
            .expect("loads");

        assert_eq!(ids(&sessions), ["retry-review"]);
    }

    #[test]
    fn relative_context_paths_resolve_against_the_session_file() {
        let files = Files::new();
        std::fs::create_dir(files.dir.path().join("sessions")).expect("made");
        files.session(
            "sessions/pictures.json",
            r#"{"asqr": 2, "questions": [{"id": "q", "text": "?", "kind": "text",
                "context": [{"type": "image", "path": "cat.png"}]}]}"#,
        );

        let sessions = files
            .load("[[sessions]]\nfile = \"sessions/pictures.json\"\n")
            .expect("loads");

        let entry = sessions
            .first()
            .and_then(|state| state.session().questions.first())
            .and_then(|question| question.context.first())
            .map(|entry| serde_json::to_value(entry).expect("an entry serializes"));
        let path = entry
            .as_ref()
            .and_then(|entry| entry["path"].as_str())
            .map(PathBuf::from);
        assert_eq!(path, Some(files.dir.path().join("sessions/cat.png")));
    }

    #[test]
    fn two_sessions_with_one_id_are_an_error() {
        let files = Files::new();
        files.session("a.json", PLAIN);

        let error =
            files.error("[[sessions]]\nfile = \"a.json\"\n[[sessions]]\nfile = \"a.json\"\n");

        assert!(error.contains("two sessions have the id \"a\""), "{error}");
    }

    #[test]
    fn an_invalid_session_is_an_error_naming_its_file() {
        let files = Files::new();
        files.session("broken.json", r#"{"asqr": 2, "questions": []}"#);

        let error = files.error("[[sessions]]\nfile = \"broken.json\"\n");

        assert!(error.contains("broken.json"), "{error}");
        assert!(error.contains("questions"), "{error}");
    }

    #[test]
    fn a_scene_id_that_differs_from_the_session_s_is_an_error() {
        let files = Files::new();
        files.session("release.json", RELEASE);

        let error = files.error("[[sessions]]\nfile = \"release.json\"\nid = \"other\"\n");

        assert!(error.contains("release.json"), "{error}");
        assert!(error.contains("release-0-2-0"), "{error}");
    }
}
