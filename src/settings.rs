// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The settings file (spec section 5.2, ADR 92): which settings asqr has,
//! how `settings.toml` is read, and which of them a flag or a variable
//! holds for a run.

use std::ops::Range;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use toml_edit::{DocumentMut, Item, Value};

use crate::keep::KeepFiles;
use crate::queue::{platform_config_dir, write_atomically_with};
use crate::tui::DiffLayout;
use crate::tui::render::{PaneLayout, ReadingWidth};

/// The name of the settings file in the platform's config directory.
pub const FILE_NAME: &str = "settings.toml";

/// The settings file in the platform's config directory, where there is a
/// config directory.
pub fn settings_file() -> Option<PathBuf> {
    platform_config_dir().map(|dir| dir.join(FILE_NAME))
}

/// A setting asqr keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// How a diff shows when it comes into view.
    DiffLayout,
    /// The widest a question's text gets.
    MaxQuestionWidth,
    /// Where the context of a question goes.
    Context,
    /// What asqr keeps of the files a finished session showed.
    KeepFiles,
}

impl Setting {
    pub const ALL: [Setting; 4] = [
        Setting::DiffLayout,
        Setting::MaxQuestionWidth,
        Setting::Context,
        Setting::KeepFiles,
    ];

    /// The key of the setting in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Setting::DiffLayout => "diff_layout",
            Setting::MaxQuestionWidth => "max_question_width",
            Setting::Context => "context",
            Setting::KeepFiles => "keep_files",
        }
    }

    /// The values the key takes, as a warning and the comment of a new
    /// key name them.
    fn allowed(self) -> &'static str {
        match self {
            Setting::DiffLayout => "unified or side-by-side",
            Setting::MaxQuestionWidth => "80, 100, 120 or \"full\"",
            Setting::Context => "auto, beside or below",
            Setting::KeepFiles => "compact, exact or off",
        }
    }

    /// How many values the setting has.
    pub fn choices(self) -> usize {
        match self {
            Setting::DiffLayout => DIFF_LAYOUTS.len(),
            Setting::MaxQuestionWidth => READING_WIDTHS.len(),
            Setting::Context => CONTEXTS.len(),
            Setting::KeepFiles => KEEP_FILES.len(),
        }
    }
}

/// The values of each setting, in the order the pane cycles through them.
const DIFF_LAYOUTS: [DiffLayout; 2] = [DiffLayout::Unified, DiffLayout::SideBySide];
const READING_WIDTHS: [ReadingWidth; 4] = [
    ReadingWidth::Columns80,
    ReadingWidth::Columns100,
    ReadingWidth::Columns120,
    ReadingWidth::Full,
];
const CONTEXTS: [PaneLayout; 3] = [PaneLayout::Auto, PaneLayout::Beside, PaneLayout::Below];
const KEEP_FILES: [KeepFiles; 3] = [KeepFiles::Compact, KeepFiles::Exact, KeepFiles::Off];

/// The values of the settings. The default is every setting's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Settings {
    pub diff_layout: DiffLayout,
    pub max_question_width: ReadingWidth,
    pub context: PaneLayout,
    pub keep_files: KeepFiles,
}

impl Settings {
    /// Which of the values of `setting`, counted from 0 in the order the
    /// pane cycles through them, this is.
    pub fn choice(&self, setting: Setting) -> usize {
        let found = match setting {
            Setting::DiffLayout => DIFF_LAYOUTS
                .iter()
                .position(|value| *value == self.diff_layout),
            Setting::MaxQuestionWidth => READING_WIDTHS
                .iter()
                .position(|value| *value == self.max_question_width),
            Setting::Context => CONTEXTS.iter().position(|value| *value == self.context),
            Setting::KeepFiles => KEEP_FILES
                .iter()
                .position(|value| *value == self.keep_files),
        };
        found.expect("every value of a setting is among its choices")
    }

    /// These settings with `setting` at its value number `choice`.
    fn with_choice(self, setting: Setting, choice: usize) -> Settings {
        match setting {
            Setting::DiffLayout => Settings {
                diff_layout: DIFF_LAYOUTS[choice],
                ..self
            },
            Setting::MaxQuestionWidth => Settings {
                max_question_width: READING_WIDTHS[choice],
                ..self
            },
            Setting::Context => Settings {
                context: CONTEXTS[choice],
                ..self
            },
            Setting::KeepFiles => Settings {
                keep_files: KEEP_FILES[choice],
                ..self
            },
        }
    }

    /// The value of `setting` as the file writes it.
    fn file_value(&self, setting: Setting) -> Value {
        match setting {
            Setting::DiffLayout => Value::from(match self.diff_layout {
                DiffLayout::Unified => "unified",
                DiffLayout::SideBySide => "side-by-side",
            }),
            Setting::MaxQuestionWidth => match self.max_question_width {
                ReadingWidth::Columns80 => Value::from(80),
                ReadingWidth::Columns100 => Value::from(100),
                ReadingWidth::Columns120 => Value::from(120),
                ReadingWidth::Full => Value::from("full"),
            },
            Setting::Context => Value::from(
                self.context
                    .to_possible_value()
                    .expect("no value of PaneLayout is skipped")
                    .get_name()
                    .to_owned(),
            ),
            Setting::KeepFiles => Value::from(self.keep_files.to_string()),
        }
    }
}

/// What holds a setting for the run from outside the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lock {
    /// An environment variable, by its name.
    Variable(&'static str),
    /// A command-line flag, by its name.
    Flag(&'static str),
}

/// The settings of a run: what the file holds, and what a flag or a
/// variable fixes over it. A store without a path keeps the values in
/// memory only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsStore {
    path: Option<PathBuf>,
    values: Settings,
    locks: Locks,
}

/// The settings a flag or a variable fixes for a run, each with the value
/// it fixes and what fixes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Locks {
    pub context: Option<(PaneLayout, Lock)>,
    pub keep_files: Option<(KeepFiles, Lock)>,
}

/// Which way a change of a setting goes through its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Previous,
    Next,
}

/// Why a setting could not be written to the file.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    #[error("asqr has no config directory to keep settings.toml in")]
    NoPlace,
    #[error("{0}")]
    Io(String),
    #[error("settings.toml does not parse{}, so it is left as it is", .0.map(|line| format!(" (line {line})")).unwrap_or_default())]
    DoesNotParse(Option<usize>),
    #[error("{0} in settings.toml is a table, so the file is left as it is")]
    NotAValue(&'static str),
    #[error("settings.toml is read-only")]
    ReadOnly,
}

/// A store read from the file and what was wrong with the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    pub store: SettingsStore,
    /// One line for each part of the file that could not be used, which
    /// says which default applies instead.
    pub warnings: Vec<String>,
}

impl SettingsStore {
    /// Reads the file at `path`, where there is one, and holds the settings
    /// in `locks` at the values a flag or a variable gave them. What is
    /// wrong with the file never fails the read: every setting the file
    /// cannot give takes its default, and `warnings` says why.
    pub fn load(path: Option<PathBuf>, locks: Locks) -> Loaded {
        let (values, warnings) = match path.as_deref() {
            Some(path) => read_file(path),
            None => (Settings::default(), Vec::new()),
        };
        Loaded {
            store: SettingsStore {
                path,
                values,
                locks,
            },
            warnings,
        }
    }

    /// The settings as this run has them: the file's, with the values a
    /// flag or a variable holds.
    pub fn effective(&self) -> Settings {
        Settings {
            context: self
                .locks
                .context
                .map_or(self.values.context, |(layout, _)| layout),
            keep_files: self
                .locks
                .keep_files
                .map_or(self.values.keep_files, |(mode, _)| mode),
            ..self.values
        }
    }

    /// What holds `setting` for this run, if anything does.
    pub fn lock(&self, setting: Setting) -> Option<Lock> {
        match setting {
            Setting::Context => self.locks.context.map(|(_, lock)| lock),
            Setting::KeepFiles => self.locks.keep_files.map(|(_, lock)| lock),
            Setting::DiffLayout | Setting::MaxQuestionWidth => None,
        }
    }

    /// The settings file, where there is a place for one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Which of the values of `setting`, in the order the pane cycles
    /// through them, is in effect.
    pub fn choice(&self, setting: Setting) -> usize {
        self.effective().choice(setting)
    }

    /// Moves `setting` to its next or previous value, wrapping around at
    /// the ends, for this run. Says whether it moved: a setting a flag or
    /// a variable holds does not.
    pub fn step(&mut self, setting: Setting, direction: Direction) -> bool {
        if self.lock(setting).is_some() {
            return false;
        }
        let count = setting.choices();
        let current = self.values.choice(setting);
        let next = match direction {
            Direction::Next => (current + 1) % count,
            Direction::Previous => (current + count - 1) % count,
        };
        self.values = self.values.with_choice(setting, next);
        true
    }

    /// Writes the value `setting` has now into the settings file.
    pub fn save(&self, setting: Setting) -> Result<(), SaveError> {
        let path = self.path.as_deref().ok_or(SaveError::NoPlace)?;
        write_setting(path, setting, &self.values)
    }
}

/// What a new settings file starts with.
const HEADER: &str =
    "# asqr settings, written by the settings pane (`,` in asqr) and fine to edit by hand.\n";

/// The most links `link_target` follows, as many as the operating system
/// follows before it reports a loop.
const MAX_LINKS: usize = 40;

/// The path the chain of symbolic links from `path` ends in, which is
/// `path` itself where it is no link. A link's target is taken from the
/// link's own directory when it is relative, and need not exist: a file
/// is made there. Links that lead round in a circle are an error.
fn link_target(path: &Path) -> std::io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        let is_link = std::fs::symlink_metadata(&current).is_ok_and(|meta| meta.is_symlink());
        if !is_link {
            return Ok(current);
        }
        let to = std::fs::read_link(&current)?;
        // An absolute target replaces the directory it is joined to.
        current = current.parent().unwrap_or(Path::new("")).join(to);
    }
    Err(std::io::Error::other("too many levels of symbolic links"))
}

/// Sets the key of `setting` in the settings file at `path` to its value
/// in `values`. The file is read again first, so what the person or
/// another asqr changed since asqr read it stays, and every comment, key
/// and blank line the file has stays with it. A file that does not parse
/// is never written over.
fn write_setting(path: &Path, setting: Setting, values: &Settings) -> Result<(), SaveError> {
    let io = |what: &str, error: std::io::Error| {
        SaveError::Io(format!("cannot {what} {FILE_NAME}: {error}"))
    };
    // A file that is a link, as a dotfile manager makes it, stays one: the
    // file it points to is written, also where that one is not there yet.
    let target = link_target(path).map_err(|error| io("read", error))?;
    // A file the person made read-only is theirs to change: renaming a new
    // file over it would write it anyway.
    let permissions = std::fs::metadata(&target)
        .ok()
        .map(|meta| meta.permissions());
    if permissions
        .as_ref()
        .is_some_and(|permissions| permissions.readonly())
    {
        return Err(SaveError::ReadOnly);
    }
    let text = match std::fs::read_to_string(&target) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(io("read", error)),
    };

    let key = setting.key();
    let mut value = values.file_value(setting);
    let written = match &text {
        None => {
            value
                .decor_mut()
                .set_suffix(format!("  # {}", setting.allowed()));
            let mut document = DocumentMut::new();
            document[key] = Item::Value(value);
            format!("{HEADER}{document}")
        }
        Some(text) => {
            let mut document = text
                .parse::<DocumentMut>()
                .map_err(|error| SaveError::DoesNotParse(line_of(text, error.span())))?;
            match document.get(key) {
                None => value
                    .decor_mut()
                    .set_suffix(format!("  # {}", setting.allowed())),
                // The comment on the key's line stays with the new value.
                Some(item) => match item.as_value() {
                    Some(old) => *value.decor_mut() = old.decor().clone(),
                    None => return Err(SaveError::NotAValue(key)),
                },
            }
            // A file with comments and no key keeps them above the new key:
            // the document would write them after it.
            let written = if document.as_table().is_empty() && !text.is_empty() {
                let line = format!(
                    "{key} = {}  # {}\n",
                    value.decorated("", ""),
                    setting.allowed()
                );
                let separator = if text.ends_with('\n') { "" } else { "\n" };
                format!("{text}{separator}{line}")
            } else {
                document[key] = Item::Value(value);
                document.to_string()
            };
            // The document writes `\n` for a line it changed, and a file
            // that used `\r\n` keeps them everywhere.
            if text.contains("\r\n") {
                written.replace("\r\n", "\n").replace('\n', "\r\n")
            } else {
                written
            }
        }
    };

    // Reading a path with no directory above it, the root, failed already.
    let dir = target
        .parent()
        .expect("a settings file lies in a directory");
    std::fs::create_dir_all(dir).map_err(|error| io("create the directory of", error))?;
    write_atomically_with(&target, written.as_bytes(), permissions)
        .map_err(|error| io("write", error))
}

#[cfg(test)]
thread_local! {
    static FILE_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times this thread read a settings file, which tests ask to see
/// that a command leaves the file alone.
#[cfg(test)]
pub(crate) fn file_reads_on_this_thread() -> usize {
    FILE_READS.with(std::cell::Cell::get)
}

/// Reads the settings from the file at `path`: a file that is not there
/// holds none, and one that cannot be read gives every default.
fn read_file(path: &Path) -> (Settings, Vec<String>) {
    #[cfg(test)]
    FILE_READS.with(|reads| reads.set(reads.get() + 1));
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (Settings::default(), Vec::new())
        }
        Err(error) => (
            Settings::default(),
            vec![format!(
                "cannot read {FILE_NAME}: {error}; every setting takes its default"
            )],
        ),
    }
}

/// The settings `text` holds. A text that is no TOML gives every default,
/// and a key whose value is none of the setting's gives that setting's
/// default, so one wrong line never costs the others. Keys asqr does not
/// know are left alone.
fn parse(text: &str) -> (Settings, Vec<String>) {
    let document = match text.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => {
            let place = line_of(text, error.span())
                .map(|line| format!("line {line}: "))
                .unwrap_or_default();
            return (
                Settings::default(),
                vec![format!(
                    "{FILE_NAME} does not parse ({place}{}); every setting takes its default",
                    error.message()
                )],
            );
        }
    };
    let mut warnings = Vec::new();
    let defaults = Settings::default();
    let settings = Settings {
        diff_layout: read_key(
            &document,
            &KeyReading {
                key: Setting::DiffLayout.key(),
                allowed: Setting::DiffLayout.allowed(),
                default: (defaults.diff_layout, "unified"),
                from: diff_layout_from,
            },
            &mut warnings,
        ),
        max_question_width: read_key(
            &document,
            &KeyReading {
                key: Setting::MaxQuestionWidth.key(),
                allowed: Setting::MaxQuestionWidth.allowed(),
                default: (defaults.max_question_width, "100"),
                from: reading_width_from,
            },
            &mut warnings,
        ),
        context: read_key(
            &document,
            &KeyReading {
                key: Setting::Context.key(),
                allowed: Setting::Context.allowed(),
                default: (defaults.context, "auto"),
                from: context_from,
            },
            &mut warnings,
        ),
        keep_files: read_key(
            &document,
            &KeyReading {
                key: Setting::KeepFiles.key(),
                allowed: Setting::KeepFiles.allowed(),
                default: (defaults.keep_files, "compact"),
                from: keep_files_from,
            },
            &mut warnings,
        ),
    };
    (settings, warnings)
}

/// How one key of the file is read.
struct KeyReading<T> {
    key: &'static str,
    /// The values the key takes, as a warning names them.
    allowed: &'static str,
    /// What applies where the key holds none of them, and its name.
    default: (T, &'static str),
    from: fn(&Value) -> Option<T>,
}

/// The value of a key, its default where the key is not there, and the
/// default with a warning where the key holds something else.
fn read_key<T: Copy>(
    document: &DocumentMut,
    reading: &KeyReading<T>,
    warnings: &mut Vec<String>,
) -> T {
    let Some(item) = document.get(reading.key) else {
        return reading.default.0;
    };
    if let Some(value) = item.as_value().and_then(reading.from) {
        return value;
    }
    // The value as the person wrote it, without the blanks and comment
    // around it.
    let shown = match item.as_value() {
        Some(value) => value.clone().decorated("", "").to_string(),
        None => "a table".to_owned(),
    };
    warnings.push(format!(
        "{FILE_NAME}: {} = {shown} is not {}; {} applies",
        reading.key, reading.allowed, reading.default.1
    ));
    reading.default.0
}

fn diff_layout_from(value: &Value) -> Option<DiffLayout> {
    match value.as_str()? {
        "unified" => Some(DiffLayout::Unified),
        "side-by-side" => Some(DiffLayout::SideBySide),
        _ => None,
    }
}

fn reading_width_from(value: &Value) -> Option<ReadingWidth> {
    if value.as_str() == Some("full") {
        return Some(ReadingWidth::Full);
    }
    match value.as_integer()? {
        80 => Some(ReadingWidth::Columns80),
        100 => Some(ReadingWidth::Columns100),
        120 => Some(ReadingWidth::Columns120),
        _ => None,
    }
}

fn context_from(value: &Value) -> Option<PaneLayout> {
    PaneLayout::from_str(value.as_str()?, false).ok()
}

fn keep_files_from(value: &Value) -> Option<KeepFiles> {
    value.as_str()?.parse().ok()
}

/// The line, counted from 1, that byte `span` starts in; `None` where the
/// parser gave no place.
fn line_of(text: &str, span: Option<Range<usize>>) -> Option<usize> {
    let start = span?.start.min(text.len());
    let before = &text.as_bytes()[..start];
    Some(before.iter().filter(|&&byte| byte == b'\n').count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store read from a file holding `text`, in a directory of its
    /// own.
    fn loaded(text: &[u8]) -> (Loaded, tempfile::TempDir) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let path = scratch.path().join(FILE_NAME);
        std::fs::write(&path, text).expect("written");
        (SettingsStore::load(Some(path), Locks::default()), scratch)
    }

    fn defaults() -> Settings {
        Settings::default()
    }

    /// Whether the tests run as root, who reads and writes through the
    /// mode bits that the tests below set to refuse a file or a directory
    /// to its owner. They end early for root, as they would find nothing
    /// refused.
    #[cfg(unix)]
    fn running_as_root() -> bool {
        rustix::process::geteuid().is_root()
    }

    #[test]
    fn the_defaults_are_one_column_100_columns_and_beside_or_below() {
        assert_eq!(
            defaults(),
            Settings {
                diff_layout: DiffLayout::Unified,
                max_question_width: ReadingWidth::Columns100,
                context: PaneLayout::Auto,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn without_a_file_every_setting_takes_its_default_and_nothing_is_said() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let missing = scratch.path().join("asqr").join(FILE_NAME);

        let loaded = SettingsStore::load(Some(missing.clone()), Locks::default());

        assert_eq!(loaded.store.effective(), defaults());
        assert_eq!(loaded.store.path(), Some(missing.as_path()));
        assert_eq!(loaded.warnings, Vec::<String>::new());
        let nowhere = SettingsStore::load(None, Locks::default());
        assert_eq!(nowhere.store.effective(), defaults());
        assert_eq!(nowhere.store.path(), None);
        assert_eq!(nowhere.warnings, Vec::<String>::new());
    }

    #[test]
    fn a_file_with_every_setting_is_read() {
        let (loaded, _scratch) = loaded(
            b"# my settings\ndiff_layout = \"side-by-side\"\nmax_question_width = 120\ncontext = \"below\"\n",
        );

        assert_eq!(
            loaded.store.effective(),
            Settings {
                diff_layout: DiffLayout::SideBySide,
                max_question_width: ReadingWidth::Columns120,
                context: PaneLayout::Below,
                ..Settings::default()
            }
        );
        assert_eq!(loaded.warnings, Vec::<String>::new());
    }

    #[test]
    fn each_value_of_each_setting_is_read() {
        let cases = [
            (
                "diff_layout = \"unified\"",
                Settings {
                    diff_layout: DiffLayout::Unified,
                    ..defaults()
                },
            ),
            (
                "diff_layout = \"side-by-side\"",
                Settings {
                    diff_layout: DiffLayout::SideBySide,
                    ..defaults()
                },
            ),
            (
                "max_question_width = 80",
                Settings {
                    max_question_width: ReadingWidth::Columns80,
                    ..defaults()
                },
            ),
            (
                "max_question_width = 100",
                Settings {
                    max_question_width: ReadingWidth::Columns100,
                    ..defaults()
                },
            ),
            (
                "max_question_width = 120",
                Settings {
                    max_question_width: ReadingWidth::Columns120,
                    ..defaults()
                },
            ),
            (
                "max_question_width = \"full\"",
                Settings {
                    max_question_width: ReadingWidth::Full,
                    ..defaults()
                },
            ),
            (
                "context = \"auto\"",
                Settings {
                    context: PaneLayout::Auto,
                    ..defaults()
                },
            ),
            (
                "context = \"beside\"",
                Settings {
                    context: PaneLayout::Beside,
                    ..defaults()
                },
            ),
            (
                "context = \"below\"",
                Settings {
                    context: PaneLayout::Below,
                    ..defaults()
                },
            ),
        ];
        for (line, expected) in cases {
            let (loaded, _scratch) = loaded(line.as_bytes());
            assert_eq!(loaded.store.effective(), expected, "{line}");
            assert_eq!(loaded.warnings, Vec::<String>::new(), "{line}");
        }
    }

    #[test]
    fn an_unknown_value_gives_the_default_for_that_setting_only() {
        let (loaded, _scratch) =
            loaded(b"diff_layout = \"sideways\"\nmax_question_width = 120\ncontext = \"below\"\n");

        assert_eq!(
            loaded.store.effective(),
            Settings {
                diff_layout: DiffLayout::Unified,
                max_question_width: ReadingWidth::Columns120,
                context: PaneLayout::Below,
                ..Settings::default()
            }
        );
        assert_eq!(
            loaded.warnings,
            [
                r#"settings.toml: diff_layout = "sideways" is not unified or side-by-side; unified applies"#
            ]
        );
    }

    #[test]
    fn a_value_of_the_wrong_kind_or_outside_the_steps_is_an_unknown_value() {
        let cases = [
            (
                "diff_layout = 3",
                "diff_layout = 3 is not unified or side-by-side; unified applies",
            ),
            (
                "max_question_width = 90",
                "max_question_width = 90 is not 80, 100, 120 or \"full\"; 100 applies",
            ),
            (
                "max_question_width = \"wide\"",
                "max_question_width = \"wide\" is not 80, 100, 120 or \"full\"; 100 applies",
            ),
            (
                "max_question_width = true",
                "max_question_width = true is not 80, 100, 120 or \"full\"; 100 applies",
            ),
            (
                "context = [\"below\"]",
                "context = [\"below\"] is not auto, beside or below; auto applies",
            ),
            (
                "context = \"Below\"",
                "context = \"Below\" is not auto, beside or below; auto applies",
            ),
        ];
        for (line, warning) in cases {
            let (loaded, _scratch) = loaded(line.as_bytes());
            assert_eq!(loaded.store.effective(), defaults(), "{line}");
            assert_eq!(
                loaded.warnings,
                [format!("settings.toml: {warning}")],
                "{line}"
            );
        }
    }

    #[test]
    fn a_file_that_does_not_parse_gives_every_default_and_names_the_line() {
        let (loaded, _scratch) = loaded(b"diff_layout = \"side-by-side\"\n\ncontext =\n");

        assert_eq!(loaded.store.effective(), defaults());
        assert_eq!(loaded.warnings.len(), 1);
        let warning = &loaded.warnings[0];
        assert!(
            warning.starts_with("settings.toml does not parse (line 3: ")
                && warning.ends_with("); every setting takes its default"),
            "{warning}"
        );
    }

    #[test]
    fn unknown_keys_and_tables_and_an_empty_file_are_fine() {
        for text in [
            "",
            "\n# only a comment\n",
            "colour = \"red\"\n[later]\nkeep_files = \"compact\"\n",
        ] {
            let (loaded, _scratch) = loaded(text.as_bytes());
            assert_eq!(loaded.store.effective(), defaults(), "{text:?}");
            assert_eq!(loaded.warnings, Vec::<String>::new(), "{text:?}");
        }
    }

    #[test]
    fn a_table_where_a_setting_belongs_is_an_unknown_value() {
        let (loaded, _scratch) = loaded(b"[context]\nbelow = true\n");

        assert_eq!(loaded.store.effective(), defaults());
        assert_eq!(
            loaded.warnings,
            ["settings.toml: context = a table is not auto, beside or below; auto applies"]
        );
    }

    #[test]
    fn the_line_of_an_error_counts_the_newlines_before_it() {
        let text = "a = 1\nb = 2\nc = 3\n";

        assert_eq!(line_of(text, Some(0..1)), Some(1));
        assert_eq!(line_of(text, Some(6..7)), Some(2));
        assert_eq!(line_of(text, Some(12..13)), Some(3));
        assert_eq!(line_of(text, Some(99..100)), Some(4), "past the end");
        assert_eq!(line_of(text, None), None);
    }

    #[test]
    fn a_file_that_is_not_text_gives_every_default_and_says_so() {
        let (loaded, _scratch) = loaded(b"diff_layout = \"\xff\"\n");

        assert_eq!(loaded.store.effective(), defaults());
        assert_eq!(
            loaded.warnings,
            [
                "cannot read settings.toml: stream did not contain valid UTF-8; every setting takes its default"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_read_gives_every_default_and_says_so() {
        use std::os::unix::fs::PermissionsExt;

        if running_as_root() {
            return;
        }

        let scratch = tempfile::tempdir().expect("temp dir");
        let path = scratch.path().join(FILE_NAME);
        std::fs::write(&path, "context = \"below\"\n").expect("written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let loaded = SettingsStore::load(Some(path), Locks::default());

        assert_eq!(loaded.store.effective(), defaults());
        assert_eq!(loaded.warnings.len(), 1);
        assert!(
            loaded.warnings[0].starts_with("cannot read settings.toml: "),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn a_flag_or_a_variable_holds_the_context_over_the_file() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let path = scratch.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "context = \"below\"\ndiff_layout = \"side-by-side\"\n",
        )
        .expect("written");

        let free = SettingsStore::load(Some(path.clone()), Locks::default()).store;
        assert_eq!(free.effective().context, PaneLayout::Below);
        assert_eq!(free.lock(Setting::Context), None);

        let variable = Lock::Variable("ASQR_LAYOUT");
        let by_variable = SettingsStore::load(
            Some(path.clone()),
            Locks {
                context: Some((PaneLayout::Beside, variable)),
                ..Locks::default()
            },
        )
        .store;
        assert_eq!(by_variable.effective().context, PaneLayout::Beside);
        assert_eq!(by_variable.lock(Setting::Context), Some(variable));

        // `auto` holds the setting too: the person asked for it.
        let flag = Lock::Flag("--layout");
        let by_flag = SettingsStore::load(
            Some(path),
            Locks {
                context: Some((PaneLayout::Auto, flag)),
                ..Locks::default()
            },
        )
        .store;
        assert_eq!(by_flag.effective().context, PaneLayout::Auto);
        assert_eq!(by_flag.lock(Setting::Context), Some(flag));

        for store in [&free, &by_variable, &by_flag] {
            assert_eq!(store.effective().diff_layout, DiffLayout::SideBySide);
            assert_eq!(store.lock(Setting::DiffLayout), None);
            assert_eq!(store.lock(Setting::MaxQuestionWidth), None);
        }
    }

    // -----------------------------------------------------------------
    // Changing and saving
    // -----------------------------------------------------------------

    /// A store over a settings file holding `text` in a directory of its
    /// own.
    fn store_over(text: &str) -> (SettingsStore, std::path::PathBuf, tempfile::TempDir) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let path = scratch.path().join(FILE_NAME);
        std::fs::write(&path, text).expect("written");
        let store = SettingsStore::load(Some(path.clone()), Locks::default()).store;
        (store, path, scratch)
    }

    fn saved_after_next(text: &str, setting: Setting) -> (Result<(), SaveError>, String) {
        let (mut store, path, _scratch) = store_over(text);
        assert!(store.step(setting, Direction::Next), "the value changed");
        let result = store.save(setting);
        (result, std::fs::read_to_string(path).expect("readable"))
    }

    #[test]
    fn a_setting_cycles_through_its_values_and_wraps_at_the_ends() {
        let mut store = SettingsStore::default();
        let mut seen = |setting, direction, count| {
            (0..count)
                .map(|_| {
                    assert!(store.step(setting, direction));
                    store.choice(setting)
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(seen(Setting::DiffLayout, Direction::Next, 3), [1, 0, 1]);
        assert_eq!(seen(Setting::DiffLayout, Direction::Previous, 2), [0, 1]);
        assert_eq!(
            seen(Setting::MaxQuestionWidth, Direction::Next, 4),
            [2, 3, 0, 1]
        );
        assert_eq!(
            seen(Setting::MaxQuestionWidth, Direction::Previous, 2),
            [0, 3]
        );
        assert_eq!(seen(Setting::Context, Direction::Next, 4), [1, 2, 0, 1]);
        assert_eq!(seen(Setting::Context, Direction::Previous, 2), [0, 2]);
        assert_eq!(
            store.effective(),
            Settings {
                diff_layout: DiffLayout::SideBySide,
                max_question_width: ReadingWidth::Full,
                context: PaneLayout::Below,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn a_setting_a_flag_or_a_variable_holds_does_not_change() {
        let mut store = SettingsStore::load(
            None,
            Locks {
                context: Some((PaneLayout::Below, Lock::Variable("ASQR_LAYOUT"))),
                ..Locks::default()
            },
        )
        .store;

        assert!(!store.step(Setting::Context, Direction::Next));
        assert!(!store.step(Setting::Context, Direction::Previous));

        assert_eq!(store.effective().context, PaneLayout::Below);
        assert_eq!(store.choice(Setting::Context), 2, "the value in effect");
        assert!(
            store.step(Setting::DiffLayout, Direction::Next),
            "others do"
        );
    }

    #[test]
    fn keep_files_reads_its_three_words() {
        for (word, mode) in [
            ("compact", KeepFiles::Compact),
            ("exact", KeepFiles::Exact),
            ("off", KeepFiles::Off),
        ] {
            let (loaded, _scratch) = loaded(format!("keep_files = \"{word}\"\n").as_bytes());

            assert_eq!(loaded.warnings, Vec::<String>::new(), "{word}");
            assert_eq!(loaded.store.effective().keep_files, mode, "{word}");
        }
        assert_eq!(Setting::ALL.len(), 4);
        assert_eq!(Setting::KeepFiles.key(), "keep_files");
    }

    #[test]
    fn a_wrong_keep_files_value_gives_the_default_and_a_warning() {
        let cases = [
            ("keep_files = \"tiny\"", "\"tiny\""),
            ("keep_files = \"Exact\"", "\"Exact\""),
            ("keep_files = true", "true"),
        ];
        for (line, shown) in cases {
            let (loaded, _scratch) = loaded(line.as_bytes());

            assert_eq!(loaded.store.effective(), defaults(), "{line}");
            assert_eq!(
                loaded.warnings,
                [format!(
                    "settings.toml: keep_files = {shown} is not compact, exact or off; compact applies"
                )],
                "{line}"
            );
        }
    }

    #[test]
    fn keep_files_goes_through_compact_exact_and_off() {
        let mut store = SettingsStore::default();
        let mut seen = |direction| {
            store.step(Setting::KeepFiles, direction);
            store.effective().keep_files
        };

        assert_eq!(seen(Direction::Next), KeepFiles::Exact);
        assert_eq!(seen(Direction::Next), KeepFiles::Off);
        assert_eq!(seen(Direction::Next), KeepFiles::Compact);
        assert_eq!(seen(Direction::Previous), KeepFiles::Off);
    }

    #[test]
    fn the_variable_holds_keep_files_over_the_file() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let path = scratch.path().join(FILE_NAME);
        std::fs::write(&path, "keep_files = \"exact\"\n").expect("written");
        let variable = Lock::Variable("ASQR_KEEP_FILES");

        let free = SettingsStore::load(Some(path.clone()), Locks::default()).store;
        assert_eq!(free.lock(Setting::KeepFiles), None);
        assert_eq!(free.effective().keep_files, KeepFiles::Exact);

        let held = SettingsStore::load(
            Some(path),
            Locks {
                keep_files: Some((KeepFiles::Off, variable)),
                ..Locks::default()
            },
        )
        .store;
        assert_eq!(held.lock(Setting::KeepFiles), Some(variable));
        assert_eq!(held.effective().keep_files, KeepFiles::Off);
        assert_eq!(held.choice(Setting::KeepFiles), 2, "the value in effect");
        assert_eq!(held.lock(Setting::Context), None, "the others stay free");
    }

    #[test]
    fn step_refuses_a_held_keep_files() {
        let mut store = SettingsStore::load(
            None,
            Locks {
                keep_files: Some((KeepFiles::Off, Lock::Variable("ASQR_KEEP_FILES"))),
                ..Locks::default()
            },
        )
        .store;

        assert!(!store.step(Setting::KeepFiles, Direction::Next));
        assert!(!store.step(Setting::KeepFiles, Direction::Previous));

        assert_eq!(store.effective().keep_files, KeepFiles::Off);
        assert!(store.step(Setting::Context, Direction::Next), "others move");
    }

    #[test]
    fn save_writes_the_key_and_keeps_the_others() {
        let (mut store, path, _scratch) = store_over("# mine\ndiff_layout = \"side-by-side\"\n");

        store.step(Setting::KeepFiles, Direction::Next);
        store.save(Setting::KeepFiles).expect("saved");

        assert_eq!(
            std::fs::read_to_string(&path).expect("written"),
            "# mine\ndiff_layout = \"side-by-side\"\nkeep_files = \"exact\"  # compact, exact or off\n"
        );
        let read = SettingsStore::load(Some(path), Locks::default());
        assert_eq!(read.warnings, Vec::<String>::new());
        assert_eq!(read.store.effective().keep_files, KeepFiles::Exact);
        assert_eq!(read.store.effective().diff_layout, DiffLayout::SideBySide);
    }

    #[test]
    fn a_new_file_gets_a_header_and_the_key_that_changed_with_its_values() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let path = scratch.path().join("asqr").join(FILE_NAME);
        let mut store = SettingsStore::load(Some(path.clone()), Locks::default()).store;

        store.step(Setting::MaxQuestionWidth, Direction::Next);
        store.save(Setting::MaxQuestionWidth).expect("saved");

        assert_eq!(
            std::fs::read_to_string(&path).expect("written"),
            "# asqr settings, written by the settings pane (`,` in asqr) and fine to edit by hand.\n\
             max_question_width = 120  # 80, 100, 120 or \"full\"\n"
        );
        let read = SettingsStore::load(Some(path), Locks::default());
        assert_eq!(read.warnings, Vec::<String>::new());
        assert_eq!(
            read.store.effective().max_question_width,
            ReadingWidth::Columns120
        );
    }

    #[test]
    fn saving_keeps_the_comments_the_unknown_keys_and_the_order() {
        let text = "# mine\nkeep_files = \"exact\"\n\n[later]\nx = 1\n";
        let (mut store, path, _scratch) = store_over(text);

        store.step(Setting::Context, Direction::Next);
        store.save(Setting::Context).expect("saved");

        let written = std::fs::read_to_string(path).expect("readable");
        assert!(
            written.starts_with("# mine\nkeep_files = \"exact\"\n"),
            "{written}"
        );
        assert!(written.contains("[later]\nx = 1\n"), "{written}");
        assert!(written.contains("context = \"beside\""), "{written}");
    }

    #[test]
    fn replacing_a_value_keeps_the_comment_on_its_line() {
        let (result, written) = saved_after_next(
            "# mine\nother = 1\ndiff_layout = \"unified\"   # my note\n",
            Setting::DiffLayout,
        );

        assert_eq!(result, Ok(()));
        assert_eq!(
            written,
            "# mine\nother = 1\ndiff_layout = \"side-by-side\"   # my note\n"
        );
    }

    #[test]
    fn a_second_key_goes_in_without_touching_the_first() {
        let (result, written) =
            saved_after_next("diff_layout = \"side-by-side\"\n", Setting::Context);

        assert_eq!(result, Ok(()));
        assert_eq!(
            written,
            "diff_layout = \"side-by-side\"\ncontext = \"beside\"  # auto, beside or below\n"
        );
    }

    #[test]
    fn the_file_is_read_again_before_writing_so_other_edits_survive() {
        let (mut store, path, _scratch) = store_over("");
        std::fs::write(&path, "context = \"below\"\n# added by hand\n").expect("edited");

        store.step(Setting::DiffLayout, Direction::Next);
        store.save(Setting::DiffLayout).expect("saved");

        let written = std::fs::read_to_string(path).expect("readable");
        // A comment after the last key stays at the end of the file.
        assert_eq!(
            written,
            "context = \"below\"\ndiff_layout = \"side-by-side\"  # unified or side-by-side\n# added by hand\n"
        );
    }

    #[test]
    fn a_file_of_comments_keeps_them_above_the_new_key() {
        for text in ["# my notes\n", "# my notes"] {
            let (result, written) = saved_after_next(text, Setting::DiffLayout);

            assert_eq!(result, Ok(()), "{text:?}");
            assert_eq!(
                written, "# my notes\ndiff_layout = \"side-by-side\"  # unified or side-by-side\n",
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_file_that_does_not_parse_is_left_as_it_is() {
        let text = "diff_layout = \"unified\"\ncontext =\n";
        let (mut store, path, _scratch) = store_over(text);

        store.step(Setting::DiffLayout, Direction::Next);
        let result = store.save(Setting::DiffLayout);

        assert_eq!(result, Err(SaveError::DoesNotParse(Some(2))));
        assert_eq!(std::fs::read_to_string(path).expect("readable"), text);
        assert_eq!(
            store.effective().diff_layout,
            DiffLayout::SideBySide,
            "the choice holds for this run"
        );
    }

    #[test]
    fn a_table_where_a_key_belongs_is_left_as_it_is() {
        let text = "[context]\nbelow = true\n";
        let (mut store, path, _scratch) = store_over(text);

        store.step(Setting::Context, Direction::Next);
        let result = store.save(Setting::Context);

        assert_eq!(result, Err(SaveError::NotAValue("context")));
        assert_eq!(std::fs::read_to_string(path).expect("readable"), text);
    }

    #[test]
    fn a_file_with_crlf_line_ends_keeps_them() {
        let (result, written) = saved_after_next(
            "# mine\r\ndiff_layout = \"unified\"\r\n",
            Setting::DiffLayout,
        );

        assert_eq!(result, Ok(()));
        assert_eq!(written, "# mine\r\ndiff_layout = \"side-by-side\"\r\n");
    }

    #[test]
    fn without_a_place_for_the_file_saving_says_so() {
        let mut store = SettingsStore::default();

        store.step(Setting::DiffLayout, Direction::Next);

        assert_eq!(store.save(Setting::DiffLayout), Err(SaveError::NoPlace));
        assert_eq!(store.effective().diff_layout, DiffLayout::SideBySide);
    }

    #[test]
    fn a_save_leaves_no_temp_file_and_a_stale_one_does_not_stop_it() {
        let (mut store, path, scratch) = store_over("");
        let stale = scratch.path().join(".asqr-stale.tmp");
        std::fs::write(&stale, "left by a crash").expect("written");

        store.step(Setting::DiffLayout, Direction::Next);
        store.save(Setting::DiffLayout).expect("saved");

        let mut names: Vec<_> = std::fs::read_dir(scratch.path())
            .expect("readable")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .into_string()
                    .expect("UTF-8")
            })
            .collect();
        names.sort();
        assert_eq!(names, [".asqr-stale.tmp", FILE_NAME]);
        assert!(
            std::fs::read_to_string(path)
                .expect("readable")
                .contains("side-by-side")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_stays_a_link_and_its_target_is_written() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let target = scratch.path().join("dotfiles").join("asqr.toml");
        std::fs::create_dir_all(target.parent().expect("parent")).expect("dir");
        std::fs::write(&target, "# from my dotfiles\n").expect("written");
        let link = scratch.path().join("config").join(FILE_NAME);
        std::fs::create_dir_all(link.parent().expect("parent")).expect("dir");
        std::os::unix::fs::symlink(&target, &link).expect("linked");
        let mut store = SettingsStore::load(Some(link.clone()), Locks::default()).store;

        store.step(Setting::DiffLayout, Direction::Next);
        store.save(Setting::DiffLayout).expect("saved");

        assert!(std::fs::symlink_metadata(&link).expect("meta").is_symlink());
        let written = std::fs::read_to_string(&target).expect("readable");
        assert!(written.starts_with("# from my dotfiles\n"), "{written}");
        assert!(written.contains("side-by-side"), "{written}");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_file_that_is_not_there_stays_a_link_and_the_file_is_made() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let link = scratch.path().join("config").join(FILE_NAME);
        std::fs::create_dir_all(link.parent().expect("parent")).expect("dir");
        // A relative target, resolved from the link's directory, in a
        // directory that does not exist either.
        std::os::unix::fs::symlink("../dotfiles/deep/asqr.toml", &link).expect("linked");
        let mut store = SettingsStore::load(Some(link.clone()), Locks::default()).store;

        store.step(Setting::DiffLayout, Direction::Next);
        let result = store.save(Setting::DiffLayout);

        assert_eq!(result, Ok(()));
        assert!(std::fs::symlink_metadata(&link).expect("meta").is_symlink());
        let target = scratch.path().join("dotfiles/deep/asqr.toml");
        let written = std::fs::read_to_string(target).expect("the target is made");
        assert!(
            written.contains("diff_layout = \"side-by-side\""),
            "{written}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_chain_of_links_is_followed_to_its_end() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let end = scratch.path().join("end.toml");
        let middle = scratch.path().join("middle.toml");
        let link = scratch.path().join(FILE_NAME);
        std::os::unix::fs::symlink(&end, &middle).expect("linked");
        std::os::unix::fs::symlink(&middle, &link).expect("linked");
        let mut store = SettingsStore::load(Some(link.clone()), Locks::default()).store;

        store.step(Setting::Context, Direction::Next);
        store.save(Setting::Context).expect("saved");

        for stays in [&link, &middle] {
            assert!(std::fs::symlink_metadata(stays).expect("meta").is_symlink());
        }
        let written = std::fs::read_to_string(end).expect("the end of the chain is made");
        assert!(written.contains("context = \"beside\""), "{written}");
    }

    #[cfg(unix)]
    #[test]
    fn links_that_lead_round_in_a_circle_are_an_error_and_stay_as_they_are() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let link = scratch.path().join(FILE_NAME);
        let other = scratch.path().join("other.toml");
        std::os::unix::fs::symlink(&other, &link).expect("linked");
        std::os::unix::fs::symlink(&link, &other).expect("linked");
        let mut store = SettingsStore::load(Some(link.clone()), Locks::default()).store;

        store.step(Setting::DiffLayout, Direction::Next);
        let result = store.save(Setting::DiffLayout);

        assert!(
            matches!(&result, Err(SaveError::Io(text)) if text.starts_with("cannot read settings.toml: ")),
            "{result:?}"
        );
        assert!(std::fs::symlink_metadata(&link).expect("meta").is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_is_read_only_is_left_as_it_is() {
        use std::os::unix::fs::PermissionsExt;

        let text = "# kept\ncontext = \"below\"\n";
        let (mut store, path, _scratch) = store_over(text);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");

        store.step(Setting::DiffLayout, Direction::Next);
        let result = store.save(Setting::DiffLayout);

        assert_eq!(result, Err(SaveError::ReadOnly));
        assert_eq!(std::fs::read_to_string(&path).expect("readable"), text);
        let mode = std::fs::metadata(path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o444, "the file is not replaced");
        assert_eq!(
            store.effective().diff_layout,
            DiffLayout::SideBySide,
            "the choice holds for this run"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_file_behind_a_link_is_left_as_it_is_too() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = tempfile::tempdir().expect("temp dir");
        let target = scratch.path().join("asqr.toml");
        std::fs::write(&target, "").expect("written");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444)).expect("chmod");
        let link = scratch.path().join(FILE_NAME);
        std::os::unix::fs::symlink(&target, &link).expect("linked");
        let mut store = SettingsStore::load(Some(link), Locks::default()).store;

        store.step(Setting::DiffLayout, Direction::Next);

        assert_eq!(store.save(Setting::DiffLayout), Err(SaveError::ReadOnly));
        assert_eq!(std::fs::read_to_string(target).expect("readable"), "");
    }

    #[cfg(unix)]
    #[test]
    fn the_permissions_of_an_existing_file_stay() {
        use std::os::unix::fs::PermissionsExt;

        let (mut store, path, _scratch) = store_over("");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod");

        store.step(Setting::DiffLayout, Direction::Next);
        store.save(Setting::DiffLayout).expect("saved");

        let mode = std::fs::metadata(path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_written_is_an_error_and_the_value_holds() {
        use std::os::unix::fs::PermissionsExt;

        if running_as_root() {
            return;
        }

        let scratch = tempfile::tempdir().expect("temp dir");
        let dir = scratch.path().join("locked");
        std::fs::create_dir(&dir).expect("dir");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        let mut store = SettingsStore::load(Some(dir.join(FILE_NAME)), Locks::default()).store;

        store.step(Setting::DiffLayout, Direction::Next);
        let result = store.save(Setting::DiffLayout);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(matches!(result, Err(SaveError::Io(_))), "{result:?}");
        assert_eq!(store.effective().diff_layout, DiffLayout::SideBySide);
        assert!(!dir.join(FILE_NAME).exists());
    }

    #[test]
    fn every_value_of_every_setting_reads_back_as_it_was_saved() {
        for setting in Setting::ALL {
            for choice in 0..setting.choices() {
                let scratch = tempfile::tempdir().expect("temp dir");
                let path = scratch.path().join(FILE_NAME);
                let mut store = SettingsStore::load(Some(path.clone()), Locks::default()).store;
                while store.choice(setting) != choice {
                    store.step(setting, Direction::Next);
                }

                store.save(setting).expect("saved");

                let read = SettingsStore::load(Some(path), Locks::default());
                assert_eq!(read.warnings, Vec::<String>::new(), "{setting:?} {choice}");
                assert_eq!(read.store.choice(setting), choice, "{setting:?} {choice}");
            }
        }
    }

    #[test]
    fn a_directory_where_the_file_belongs_is_an_error_naming_what_failed() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let mut store =
            SettingsStore::load(Some(scratch.path().to_path_buf()), Locks::default()).store;
        store.step(Setting::DiffLayout, Direction::Next);
        let unreadable = store.save(Setting::DiffLayout);
        assert!(
            matches!(&unreadable, Err(SaveError::Io(text)) if text.starts_with("cannot read settings.toml: ")),
            "a directory where the file belongs: {unreadable:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_made_is_an_error_naming_what_failed() {
        use std::os::unix::fs::PermissionsExt;

        if running_as_root() {
            return;
        }

        let scratch = tempfile::tempdir().expect("temp dir");
        let locked = scratch.path().join("locked");
        std::fs::create_dir(&locked).expect("dir");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        let mut store =
            SettingsStore::load(Some(locked.join("asqr").join(FILE_NAME)), Locks::default()).store;
        store.step(Setting::DiffLayout, Direction::Next);

        let result = store.save(Setting::DiffLayout);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(
            matches!(&result, Err(SaveError::Io(text)) if text.starts_with("cannot create the directory of settings.toml: ")),
            "{result:?}"
        );
    }

    #[test]
    fn what_cannot_be_saved_is_said_in_words() {
        assert_eq!(
            SaveError::DoesNotParse(Some(3)).to_string(),
            "settings.toml does not parse (line 3), so it is left as it is"
        );
        assert_eq!(
            SaveError::DoesNotParse(None).to_string(),
            "settings.toml does not parse, so it is left as it is"
        );
        assert_eq!(
            SaveError::ReadOnly.to_string(),
            "settings.toml is read-only"
        );
    }
}
