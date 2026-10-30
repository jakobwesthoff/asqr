// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The settings file (spec section 5.2, ADR 92): which settings asqr has,
//! how `settings.toml` is read, and which of them a flag or a variable
//! holds for a run.

use std::ops::Range;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use toml_edit::{DocumentMut, Value};

use crate::queue::platform_config_dir;
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
}

/// The values of the settings. The default is every setting's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Settings {
    pub diff_layout: DiffLayout,
    pub max_question_width: ReadingWidth,
    pub context: PaneLayout,
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
    context_lock: Option<(PaneLayout, Lock)>,
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
    /// Reads the file at `path`, where there is one, and holds the context
    /// placement at `context_lock` where a flag or a variable gave one.
    /// What is wrong with the file never fails the read: every setting
    /// the file cannot give takes its default, and `warnings` says why.
    pub fn load(path: Option<PathBuf>, context_lock: Option<(PaneLayout, Lock)>) -> Loaded {
        let (values, warnings) = match path.as_deref() {
            Some(path) => read_file(path),
            None => (Settings::default(), Vec::new()),
        };
        Loaded {
            store: SettingsStore {
                path,
                values,
                context_lock,
            },
            warnings,
        }
    }

    /// The settings as this run has them: the file's, with the context
    /// placement a flag or a variable holds.
    pub fn effective(&self) -> Settings {
        Settings {
            context: self
                .context_lock
                .map_or(self.values.context, |(layout, _)| layout),
            ..self.values
        }
    }

    /// What holds `setting` for this run, if anything does.
    pub fn lock(&self, setting: Setting) -> Option<Lock> {
        match setting {
            Setting::Context => self.context_lock.map(|(_, lock)| lock),
            Setting::DiffLayout | Setting::MaxQuestionWidth => None,
        }
    }

    /// The settings file, where there is a place for one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

/// Reads the settings from the file at `path`: a file that is not there
/// holds none, and one that cannot be read gives every default.
fn read_file(path: &Path) -> (Settings, Vec<String>) {
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
                key: "diff_layout",
                allowed: "unified or side-by-side",
                default: (defaults.diff_layout, "unified"),
                from: diff_layout_from,
            },
            &mut warnings,
        ),
        max_question_width: read_key(
            &document,
            &KeyReading {
                key: "max_question_width",
                allowed: "80, 100, 120 or \"full\"",
                default: (defaults.max_question_width, "100"),
                from: reading_width_from,
            },
            &mut warnings,
        ),
        context: read_key(
            &document,
            &KeyReading {
                key: "context",
                allowed: "auto, beside or below",
                default: (defaults.context, "auto"),
                from: context_from,
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
        (SettingsStore::load(Some(path), None), scratch)
    }

    fn defaults() -> Settings {
        Settings::default()
    }

    #[test]
    fn the_defaults_are_one_column_100_columns_and_beside_or_below() {
        assert_eq!(
            defaults(),
            Settings {
                diff_layout: DiffLayout::Unified,
                max_question_width: ReadingWidth::Columns100,
                context: PaneLayout::Auto,
            }
        );
    }

    #[test]
    fn without_a_file_every_setting_takes_its_default_and_nothing_is_said() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let missing = scratch.path().join("asqr").join(FILE_NAME);

        let loaded = SettingsStore::load(Some(missing.clone()), None);

        assert_eq!(loaded.store.effective(), defaults());
        assert_eq!(loaded.store.path(), Some(missing.as_path()));
        assert_eq!(loaded.warnings, Vec::<String>::new());
        let nowhere = SettingsStore::load(None, None);
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

        let scratch = tempfile::tempdir().expect("temp dir");
        let path = scratch.path().join(FILE_NAME);
        std::fs::write(&path, "context = \"below\"\n").expect("written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let loaded = SettingsStore::load(Some(path), None);

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

        let free = SettingsStore::load(Some(path.clone()), None).store;
        assert_eq!(free.effective().context, PaneLayout::Below);
        assert_eq!(free.lock(Setting::Context), None);

        let variable = Lock::Variable("ASQR_LAYOUT");
        let by_variable =
            SettingsStore::load(Some(path.clone()), Some((PaneLayout::Beside, variable))).store;
        assert_eq!(by_variable.effective().context, PaneLayout::Beside);
        assert_eq!(by_variable.lock(Setting::Context), Some(variable));

        // `auto` holds the setting too: the person asked for it.
        let flag = Lock::Flag("--layout");
        let by_flag = SettingsStore::load(Some(path), Some((PaneLayout::Auto, flag))).store;
        assert_eq!(by_flag.effective().context, PaneLayout::Auto);
        assert_eq!(by_flag.lock(Setting::Context), Some(flag));

        for store in [&free, &by_variable, &by_flag] {
            assert_eq!(store.effective().diff_layout, DiffLayout::SideBySide);
            assert_eq!(store.lock(Setting::DiffLayout), None);
            assert_eq!(store.lock(Setting::MaxQuestionWidth), None);
        }
    }
}
