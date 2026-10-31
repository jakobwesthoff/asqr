// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The settings pane behind `,` (spec section 7.6, ADR 92): one row per
//! setting, the cursor on one of them, `←` and `→` changing its value. The
//! pane is driven by keys alone; changing and saving a value is the
//! app's (see [`super::App`]). This module also holds every text the
//! pane shows.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use clap::ValueEnum;

use crate::settings::{Direction, Lock, Setting, Settings};
use crate::tui::render::PaneLayout;

/// What a key did in the settings pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsKey {
    /// It moved the cursor, or did nothing.
    Stay,
    /// The pane closes.
    Close,
    Quit,
    /// The value of this setting moves one step.
    Change(Setting, Direction),
}

/// The pane's state: which row the cursor stands on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsPane {
    cursor: usize,
}

impl SettingsPane {
    /// The index of the row under the cursor, in [`Setting::ALL`].
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The setting under the cursor.
    pub fn setting(&self) -> Setting {
        Setting::ALL[self.cursor]
    }

    /// Handles one key. `locked` says whether a flag or a variable holds
    /// the setting under the cursor: `←` and `→` do nothing there. The
    /// vim keys work as everywhere else in asqr.
    pub fn handle(&mut self, key: KeyEvent, locked: bool) -> SettingsKey {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('c') {
            return SettingsKey::Quit;
        }
        if control {
            return SettingsKey::Stay;
        }
        let change = |direction| {
            if locked {
                SettingsKey::Stay
            } else {
                SettingsKey::Change(Setting::ALL[self.cursor], direction)
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q' | ',') => SettingsKey::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                SettingsKey::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(Setting::ALL.len() - 1);
                SettingsKey::Stay
            }
            KeyCode::Left | KeyCode::Char('h') => change(Direction::Previous),
            KeyCode::Right | KeyCode::Char('l') => change(Direction::Next),
            _ => SettingsKey::Stay,
        }
    }
}

/// The name of the setting on its row.
pub fn label(setting: Setting) -> &'static str {
    match setting {
        Setting::DiffLayout => "Diffs open as",
        Setting::MaxQuestionWidth => "Max question width",
        Setting::Context => "Context",
    }
}

/// The names of the values of `setting` on its row, in the order `←` and
/// `→` go through them, each with what it means.
fn values(setting: Setting) -> &'static [(&'static str, &'static str)] {
    match setting {
        Setting::DiffLayout => &[
            (
                "one column",
                "One column: removed and added lines one below the other.",
            ),
            (
                "side by side",
                "Side by side: the old version on the left, the new one on the right.",
            ),
        ],
        Setting::MaxQuestionWidth => &[
            (
                "80 columns",
                "80 columns: lines wrap after at most 80 characters.",
            ),
            (
                "100 columns",
                "100 columns: lines wrap after at most 100 characters.",
            ),
            (
                "120 columns",
                "120 columns: lines wrap after at most 120 characters.",
            ),
            (
                "full width",
                "Full width: lines use the whole width of the terminal.",
            ),
        ],
        Setting::Context => &[
            (
                "beside or below",
                "Beside or below: beside the question when the terminal is wide enough, otherwise below it.",
            ),
            (
                "always beside",
                "Always beside: next to the question whenever it fits, even on a tall terminal.",
            ),
            (
                "always below",
                "Always below: under the question whenever it fits, even on a wide terminal.",
            ),
        ],
    }
}

/// The name of value number `choice` of `setting` on its row.
pub fn value_name(setting: Setting, choice: usize) -> &'static str {
    values(setting)[choice].0
}

/// The value of `setting` the settings start with, as its number.
pub fn default_choice(setting: Setting) -> usize {
    Settings::default().choice(setting)
}

/// What the footer says the setting does.
pub fn about(setting: Setting) -> &'static str {
    match setting {
        Setting::DiffLayout => {
            "How a diff shows when you come to it. The s key still switches the diff you look at."
        }
        Setting::MaxQuestionWidth => {
            "The widest a question's text may get; on a narrower terminal it gets narrower."
        }
        Setting::Context => "Where the files, images and diffs that come with a question show.",
    }
}

/// What the footer says value number `choice` of `setting` means, with
/// `(default)` after the default.
pub fn meaning(setting: Setting, choice: usize) -> String {
    let meaning = values(setting)[choice].1;
    if choice == default_choice(setting) {
        format!("{meaning} (default)")
    } else {
        meaning.to_owned()
    }
}

/// What stands beside a locked row's value.
pub fn lock_label(lock: Lock) -> String {
    match lock {
        Lock::Variable(name) => format!("set by {name}"),
        Lock::Flag(name) => format!("set by {name}"),
    }
}

/// What the footer says of a context placement `lock` holds at `layout`:
/// what holds, why, and what to do about it.
pub fn lock_text(lock: Lock, layout: PaneLayout) -> String {
    let value = layout
        .to_possible_value()
        .expect("no value of PaneLayout is skipped")
        .get_name()
        .to_owned();
    match lock {
        Lock::Variable(name) => {
            let effect = match layout {
                PaneLayout::Below => "always shows context below the question",
                PaneLayout::Beside => "shows context beside the question when it fits",
                PaneLayout::Auto => "picks beside or below by the terminal's shape",
            };
            format!(
                "Your shell sets {name}={value}, so asqr {effect}, whatever you choose here. To choose here, remove {name} from your shell setup (for example ~/.zshrc) and start asqr again."
            )
        }
        Lock::Flag(name) => {
            let effect = match layout {
                PaneLayout::Below => "shows context below the question",
                PaneLayout::Beside => "shows context beside the question when it fits",
                PaneLayout::Auto => "picks beside or below by the terminal's shape",
            };
            format!(
                "You started asqr with {name} {value}, so this run {effect}. Without the flag, the next start uses your choice here."
            )
        }
    }
}

/// The status line's news of a saved change.
pub fn saved_text(setting: Setting, choice: usize) -> String {
    format!(
        "✔ saved {}: {}",
        label(setting).to_lowercase(),
        value_name(setting, choice)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn the_cursor_starts_on_the_first_row_and_moves_without_leaving_the_rows() {
        let mut pane = SettingsPane::default();
        assert_eq!(pane.setting(), Setting::DiffLayout);

        assert_eq!(pane.handle(key(KeyCode::Up), false), SettingsKey::Stay);
        assert_eq!(pane.cursor(), 0, "no row above the first");
        for (code, cursor) in [
            (KeyCode::Down, 1),
            (KeyCode::Char('j'), 2),
            (KeyCode::Down, 2),
            (KeyCode::Up, 1),
            (KeyCode::Char('k'), 0),
        ] {
            assert_eq!(pane.handle(key(code), false), SettingsKey::Stay);
            assert_eq!(pane.cursor(), cursor, "{code:?}");
        }
        pane.handle(key(KeyCode::Down), false);
        assert_eq!(pane.setting(), Setting::MaxQuestionWidth);
    }

    #[test]
    fn left_and_right_change_the_setting_under_the_cursor() {
        let mut pane = SettingsPane::default();
        pane.handle(key(KeyCode::Down), false);

        for (code, direction) in [
            (KeyCode::Right, Direction::Next),
            (KeyCode::Char('l'), Direction::Next),
            (KeyCode::Left, Direction::Previous),
            (KeyCode::Char('h'), Direction::Previous),
        ] {
            assert_eq!(
                pane.handle(key(code), false),
                SettingsKey::Change(Setting::MaxQuestionWidth, direction),
                "{code:?}"
            );
        }
    }

    #[test]
    fn a_locked_row_does_not_change() {
        let mut pane = SettingsPane::default();

        for code in [
            KeyCode::Right,
            KeyCode::Left,
            KeyCode::Char('l'),
            KeyCode::Char('h'),
        ] {
            assert_eq!(pane.handle(key(code), true), SettingsKey::Stay, "{code:?}");
        }
        assert_eq!(pane.handle(key(KeyCode::Down), true), SettingsKey::Stay);
        assert_eq!(pane.cursor(), 1, "the cursor still moves");
    }

    #[test]
    fn esc_q_and_the_comma_close_and_ctrl_c_quits() {
        let mut pane = SettingsPane::default();

        for code in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char(',')] {
            assert_eq!(
                pane.handle(key(code), false),
                SettingsKey::Close,
                "{code:?}"
            );
        }
        assert_eq!(pane.handle(ctrl('c'), false), SettingsKey::Quit);
    }

    #[test]
    fn any_other_key_does_nothing() {
        let mut pane = SettingsPane::default();

        for event in [
            key(KeyCode::Enter),
            key(KeyCode::Char('x')),
            key(KeyCode::Char('w')),
            key(KeyCode::Tab),
            ctrl('l'),
            ctrl('q'),
        ] {
            assert_eq!(pane.handle(event, false), SettingsKey::Stay, "{event:?}");
        }
        assert_eq!(pane.cursor(), 0);
    }

    #[test]
    fn the_rows_and_their_values_are_named_as_agreed() {
        let names = |setting: Setting| {
            (0..setting.choices())
                .map(|choice| value_name(setting, choice))
                .collect::<Vec<_>>()
        };

        assert_eq!(label(Setting::DiffLayout), "Diffs open as");
        assert_eq!(label(Setting::MaxQuestionWidth), "Max question width");
        assert_eq!(label(Setting::Context), "Context");
        assert_eq!(names(Setting::DiffLayout), ["one column", "side by side"]);
        assert_eq!(
            names(Setting::MaxQuestionWidth),
            ["80 columns", "100 columns", "120 columns", "full width"]
        );
        assert_eq!(
            names(Setting::Context),
            ["beside or below", "always beside", "always below"]
        );
    }

    #[test]
    fn the_default_of_each_setting_is_the_one_the_settings_start_with() {
        let defaults = crate::settings::Settings::default();

        for setting in Setting::ALL {
            assert_eq!(
                default_choice(setting),
                defaults.choice(setting),
                "{setting:?}"
            );
        }
    }

    #[test]
    fn the_footer_says_what_a_setting_does_and_what_each_value_means() {
        assert_eq!(
            about(Setting::DiffLayout),
            "How a diff shows when you come to it. The s key still switches the diff you look at."
        );
        assert_eq!(
            about(Setting::MaxQuestionWidth),
            "The widest a question's text may get; on a narrower terminal it gets narrower."
        );
        assert_eq!(
            about(Setting::Context),
            "Where the files, images and diffs that come with a question show."
        );
        let meanings = |setting: Setting| {
            (0..setting.choices())
                .map(|choice| meaning(setting, choice))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            meanings(Setting::DiffLayout),
            [
                "One column: removed and added lines one below the other. (default)",
                "Side by side: the old version on the left, the new one on the right.",
            ]
        );
        assert_eq!(
            meanings(Setting::MaxQuestionWidth),
            [
                "80 columns: lines wrap after at most 80 characters.",
                "100 columns: lines wrap after at most 100 characters. (default)",
                "120 columns: lines wrap after at most 120 characters.",
                "Full width: lines use the whole width of the terminal.",
            ]
        );
        assert_eq!(
            meanings(Setting::Context),
            [
                "Beside or below: beside the question when the terminal is wide enough, otherwise below it. (default)",
                "Always beside: next to the question whenever it fits, even on a tall terminal.",
                "Always below: under the question whenever it fits, even on a wide terminal.",
            ]
        );
    }

    #[test]
    fn a_locked_row_says_what_sets_it() {
        assert_eq!(
            lock_label(Lock::Variable("ASQR_LAYOUT")),
            "set by ASQR_LAYOUT"
        );
        assert_eq!(lock_label(Lock::Flag("--layout")), "set by --layout");
    }

    #[test]
    fn the_footer_of_a_locked_row_explains_what_holds_and_what_to_do() {
        let variable = Lock::Variable("ASQR_LAYOUT");
        let flag = Lock::Flag("--layout");
        let cases = [
            (
                variable,
                PaneLayout::Below,
                "Your shell sets ASQR_LAYOUT=below, so asqr always shows context below the question, whatever you choose here. To choose here, remove ASQR_LAYOUT from your shell setup (for example ~/.zshrc) and start asqr again.",
            ),
            (
                variable,
                PaneLayout::Beside,
                "Your shell sets ASQR_LAYOUT=beside, so asqr shows context beside the question when it fits, whatever you choose here. To choose here, remove ASQR_LAYOUT from your shell setup (for example ~/.zshrc) and start asqr again.",
            ),
            (
                variable,
                PaneLayout::Auto,
                "Your shell sets ASQR_LAYOUT=auto, so asqr picks beside or below by the terminal's shape, whatever you choose here. To choose here, remove ASQR_LAYOUT from your shell setup (for example ~/.zshrc) and start asqr again.",
            ),
            (
                flag,
                PaneLayout::Below,
                "You started asqr with --layout below, so this run shows context below the question. Without the flag, the next start uses your choice here.",
            ),
            (
                flag,
                PaneLayout::Beside,
                "You started asqr with --layout beside, so this run shows context beside the question when it fits. Without the flag, the next start uses your choice here.",
            ),
            (
                flag,
                PaneLayout::Auto,
                "You started asqr with --layout auto, so this run picks beside or below by the terminal's shape. Without the flag, the next start uses your choice here.",
            ),
        ];
        for (lock, layout, text) in cases {
            assert_eq!(lock_text(lock, layout), text, "{lock:?} {layout:?}");
        }
    }

    #[test]
    fn a_saved_change_is_news_in_the_words_of_the_pane() {
        assert_eq!(
            saved_text(Setting::DiffLayout, 1),
            "✔ saved diffs open as: side by side"
        );
        assert_eq!(
            saved_text(Setting::MaxQuestionWidth, 0),
            "✔ saved max question width: 80 columns"
        );
        assert_eq!(
            saved_text(Setting::Context, 2),
            "✔ saved context: always below"
        );
    }
}
