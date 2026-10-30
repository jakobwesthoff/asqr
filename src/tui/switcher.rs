// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The queue switcher behind `w` (spec section 7.6, ADR 86): the queues
//! this asqr can switch to, the ones other asqr instances watch apart, and
//! a row to name a new queue. Driven by keys and pastes alone; the switch
//! itself is the run loop's (see [`super::switch_queue`]).

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::format::MAX_SESSION_ID_BYTES;
use crate::queue::QueueLocation;

/// What the person is told about the queues: where asqr runs decides
/// what a switch does to the agents, which the footer says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// Inside tmux session `0`, whose agents follow the queue asqr pins.
    Tmux(String),
    /// Outside tmux, or inside a tmux that did not answer, where agents
    /// reach a queue only through what they are told.
    Outside,
}

/// A queue the cursor can reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRow {
    pub location: QueueLocation,
    /// The queue's name, or the path of a queue given by `--dir`, with `~`
    /// for the home folder.
    pub label: String,
    /// The sessions waiting in it.
    pub waiting: usize,
    /// Whether this asqr watches it.
    pub watched: bool,
}

/// A queue another asqr watches, which no switch can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenQueue {
    pub label: String,
    /// The tmux session of the asqr watching it, where tmux named one.
    pub session: Option<String>,
}

/// The queues as the switcher shows them, looked at when it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueChoice {
    /// A `--dir` queue of this run first, then the named queues by name.
    pub rows: Vec<QueueRow>,
    /// By name.
    pub taken: Vec<TakenQueue>,
    /// The root of the named queues, where a new queue goes; without one,
    /// there is no row for a new queue.
    pub root: Option<PathBuf>,
    pub place: Place,
}

/// What a key or a paste did in the switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitcherKey {
    /// It moved the cursor, typed, or did nothing.
    Stay,
    /// The switcher closes without a switch.
    Close,
    Quit,
    /// The switcher closes, and asqr switches to this queue.
    Switch(QueueLocation),
    /// The name row refused a key or a paste: a queue name cannot hold it.
    Refused,
    /// The name row refused a key or a paste that would make the name
    /// longer than a queue name can be.
    TooLong,
}

/// What the status line says about a refused key or paste on the name row.
pub const REFUSED_NAME: &str =
    "a queue name has letters, digits, -, _ and . and does not start with .";

/// What the status line says about a name that would grow past the
/// longest queue name.
pub const TOO_LONG_NAME: &str = "a queue name has at most 200 characters";

/// The switcher's state: the queues and the cursor among them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switcher {
    choice: QueueChoice,
    /// An index into the rows; one past the last is the new-queue row.
    cursor: usize,
    /// The name typed on the new-queue row.
    name: String,
    /// Whether the new-queue row has the keys.
    typing: bool,
    /// Whether this is the start picker, which watches the queue picked
    /// and whose `q` quits asqr.
    picking: bool,
}

impl Switcher {
    /// The switcher over `choice`, the cursor on the queue this asqr
    /// watches.
    pub fn new(choice: QueueChoice) -> Self {
        let cursor = choice.rows.iter().position(|row| row.watched).unwrap_or(0);
        Switcher {
            choice,
            cursor,
            name: String::new(),
            typing: false,
            picking: false,
        }
    }

    /// The start picker over `choice` (ADR 86), the cursor on `default`
    /// where this asqr may watch it, else on the first queue it may, or on
    /// the new-queue row, which then has the keys.
    pub fn picker(choice: QueueChoice) -> Self {
        let cursor = choice
            .rows
            .iter()
            .position(|row| row.location.is_default())
            .unwrap_or(0);
        let mut picker = Switcher {
            cursor,
            picking: true,
            ..Switcher::new(choice)
        };
        picker.typing = picker.on_name_row();
        picker
    }

    /// Whether this is the start picker.
    pub fn picking(&self) -> bool {
        self.picking
    }

    pub fn choice(&self) -> &QueueChoice {
        &self.choice
    }

    /// Where the cursor is: an index into the rows, one past the last for
    /// the new-queue row.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The name typed on the new-queue row.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the new-queue row has the keys.
    pub fn typing(&self) -> bool {
        self.typing
    }

    /// The index of the last row the cursor reaches: the new-queue row
    /// where there is a root to put a new queue under.
    fn last(&self) -> usize {
        let rows = self.choice.rows.len();
        if self.choice.root.is_some() {
            rows
        } else {
            rows.saturating_sub(1)
        }
    }

    /// Whether the cursor stands on the new-queue row.
    pub fn on_name_row(&self) -> bool {
        self.choice.root.is_some() && self.cursor == self.choice.rows.len()
    }

    /// Handles one key. The new-queue row is a field as soon as the cursor
    /// stands on it, as the own answer of a question is: letters type
    /// there, `q` and `w` too, and `esc` gives the keys back to the list.
    pub fn handle(&mut self, key: KeyEvent) -> SwitcherKey {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('c') {
            return SwitcherKey::Quit;
        }
        if control {
            return SwitcherKey::Stay;
        }
        if self.typing {
            return match key.code {
                KeyCode::Char(c) => self.type_text(&c.to_string()),
                KeyCode::Backspace => {
                    self.name.pop();
                    SwitcherKey::Stay
                }
                KeyCode::Esc => {
                    self.typing = false;
                    SwitcherKey::Stay
                }
                KeyCode::Up => self.move_cursor(false),
                KeyCode::Enter => self.enter(),
                _ => SwitcherKey::Stay,
            };
        }
        match key.code {
            // The picker has no queue to go back to: `q` quits asqr, and
            // `esc` and `w` do nothing.
            KeyCode::Char('q') if self.picking => SwitcherKey::Quit,
            KeyCode::Esc | KeyCode::Char('w') if self.picking => SwitcherKey::Stay,
            KeyCode::Esc | KeyCode::Char('q' | 'w') => SwitcherKey::Close,
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(false),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(true),
            KeyCode::Enter => self.enter(),
            _ => SwitcherKey::Stay,
        }
    }

    /// Takes text the person pasted: on the new-queue row it goes into the
    /// name whole, when the name can hold all of it, and that row has the
    /// keys from then on; elsewhere it goes nowhere.
    pub fn paste(&mut self, text: &str) -> SwitcherKey {
        if !self.on_name_row() {
            return SwitcherKey::Stay;
        }
        self.typing = true;
        self.type_text(text)
    }

    /// Adds `text` to the name where every character of it may stand
    /// there and the name stays within the longest queue name.
    fn type_text(&mut self, text: &str) -> SwitcherKey {
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
        if !text.chars().all(allowed) || (self.name.is_empty() && text.starts_with('.')) {
            return SwitcherKey::Refused;
        }
        if self.name.len() + text.len() > MAX_SESSION_ID_BYTES {
            return SwitcherKey::TooLong;
        }
        self.name.push_str(text);
        SwitcherKey::Stay
    }

    /// Moves the cursor one row down or up. Landing on the new-queue row
    /// gives it the keys, and so does a move down that stays on it, as on
    /// the own answer of a question left with `esc`.
    fn move_cursor(&mut self, down: bool) -> SwitcherKey {
        self.cursor = if down {
            (self.cursor + 1).min(self.last())
        } else {
            self.cursor.saturating_sub(1)
        };
        self.typing = self.on_name_row();
        SwitcherKey::Stay
    }

    /// `enter`: a switch to the queue under the cursor, or to the one the
    /// name names, which is an existing queue where it names the same
    /// directory as one. The queue this asqr watches only closes.
    fn enter(&mut self) -> SwitcherKey {
        let target = match (&self.choice.root, self.choice.rows.get(self.cursor)) {
            (_, Some(row)) => row.location.clone(),
            (Some(_), None) if self.name.is_empty() => return SwitcherKey::Stay,
            (Some(root), None) => {
                let named = QueueLocation::named(root, &self.name);
                self.choice
                    .rows
                    .iter()
                    .find(|row| row.location.same_queue(&named))
                    .map_or(named, |row| row.location.clone())
            }
            (None, None) => return SwitcherKey::Stay,
        };
        let watched = self
            .choice
            .rows
            .iter()
            .any(|row| row.watched && row.location == target);
        if watched {
            SwitcherKey::Close
        } else {
            SwitcherKey::Switch(target)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const ROOT: &str = "/data/asqr";

    fn row(name: &str, waiting: usize, watched: bool) -> QueueRow {
        QueueRow {
            location: QueueLocation::named(Path::new(ROOT), name),
            label: name.to_owned(),
            waiting,
            watched,
        }
    }

    /// `asqr-todos` watched, `default` and `work` free, `ntropy` taken,
    /// inside tmux session `asqr`.
    fn choice() -> QueueChoice {
        QueueChoice {
            rows: vec![
                row("asqr-todos", 2, true),
                row("default", 1, false),
                row("work", 0, false),
            ],
            taken: vec![TakenQueue {
                label: "ntropy".into(),
                session: Some("ntropy".into()),
            }],
            root: Some(PathBuf::from(ROOT)),
            place: Place::Tmux("asqr".into()),
        }
    }

    fn switcher() -> Switcher {
        Switcher::new(choice())
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn char(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    /// `keys` typed one character after another.
    fn typed(switcher: &mut Switcher, keys: &str) -> Vec<SwitcherKey> {
        keys.chars().map(|c| switcher.handle(char(c))).collect()
    }

    fn named(name: &str) -> QueueLocation {
        QueueLocation::named(Path::new(ROOT), name)
    }

    /// The switcher with its cursor on the new-queue row, which then has
    /// the keys.
    fn on_the_name_row() -> Switcher {
        let mut switcher = switcher();
        for _ in 0..3 {
            switcher.handle(key(KeyCode::Down));
        }
        assert_eq!(switcher.cursor(), 3, "the new-queue row");
        switcher
    }

    #[test]
    fn the_cursor_opens_on_the_watched_queue() {
        let mut choice = choice();
        choice.rows[0].watched = false;
        choice.rows[2].watched = true;

        assert_eq!(Switcher::new(choice).cursor(), 2);
        assert_eq!(switcher().cursor(), 0);
    }

    #[test]
    fn arrows_and_j_k_move_within_the_free_rows() {
        let mut switcher = switcher();

        switcher.handle(key(KeyCode::Down));
        assert_eq!(switcher.cursor(), 1);
        switcher.handle(char('j'));
        assert_eq!(switcher.cursor(), 2);
        switcher.handle(char('k'));
        switcher.handle(key(KeyCode::Up));
        switcher.handle(key(KeyCode::Up));
        assert_eq!(switcher.cursor(), 0, "stops at the first row");
    }

    #[test]
    fn the_cursor_skips_the_taken_queues_and_ends_on_the_new_queue_row() {
        let mut switcher = switcher();

        for _ in 0..6 {
            switcher.handle(key(KeyCode::Down));
        }

        assert_eq!(switcher.cursor(), 3, "the new-queue row, the last");
        assert!(switcher.typing(), "which has the keys");
    }

    #[test]
    fn enter_switches_to_the_queue_under_the_cursor() {
        let mut switcher = switcher();
        switcher.handle(char('j'));

        assert_eq!(
            switcher.handle(key(KeyCode::Enter)),
            SwitcherKey::Switch(named("default"))
        );
    }

    #[test]
    fn enter_on_the_watched_queue_only_closes() {
        assert_eq!(switcher().handle(key(KeyCode::Enter)), SwitcherKey::Close);
    }

    #[test]
    fn esc_q_and_w_close() {
        for close in [key(KeyCode::Esc), char('q'), char('w')] {
            assert_eq!(switcher().handle(close), SwitcherKey::Close, "{close:?}");
        }
    }

    #[test]
    fn ctrl_c_quits_also_while_typing() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        assert_eq!(switcher().handle(ctrl_c), SwitcherKey::Quit);
        assert_eq!(on_the_name_row().handle(ctrl_c), SwitcherKey::Quit);
    }

    #[test]
    fn without_any_row_enter_does_nothing() {
        let mut choice = choice();
        choice.rows.clear();
        choice.root = None;

        assert_eq!(
            Switcher::new(choice).handle(key(KeyCode::Enter)),
            SwitcherKey::Stay
        );
    }

    #[test]
    fn other_keys_do_nothing() {
        let mut switcher = switcher();
        let ctrl_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        for ignored in [char('x'), char('1'), key(KeyCode::Tab), char('L'), ctrl_j] {
            assert_eq!(switcher.handle(ignored), SwitcherKey::Stay, "{ignored:?}");
            assert_eq!(switcher.cursor(), 0);
        }
    }

    #[test]
    fn letters_type_on_the_new_queue_row_q_and_w_included() {
        let mut switcher = on_the_name_row();

        let keys = typed(&mut switcher, "weekly-q_1.x");

        assert!(keys.iter().all(|key| *key == SwitcherKey::Stay), "{keys:?}");
        assert_eq!(switcher.name(), "weekly-q_1.x");
        switcher.handle(key(KeyCode::Backspace));
        assert_eq!(switcher.name(), "weekly-q_1.");
    }

    #[test]
    fn a_key_a_queue_name_cannot_hold_is_refused() {
        let mut switcher = on_the_name_row();
        typed(&mut switcher, "work");

        assert_eq!(switcher.handle(char(' ')), SwitcherKey::Refused);
        assert_eq!(switcher.handle(char('ä')), SwitcherKey::Refused);
        assert_eq!(switcher.name(), "work");
    }

    #[test]
    fn a_name_cannot_start_with_a_dot() {
        let mut switcher = on_the_name_row();

        assert_eq!(switcher.handle(char('.')), SwitcherKey::Refused);
        assert_eq!(switcher.name(), "");
    }

    #[test]
    fn a_name_stops_at_the_longest_queue_name() {
        let mut switcher = on_the_name_row();
        typed(&mut switcher, &"a".repeat(MAX_SESSION_ID_BYTES));

        assert_eq!(switcher.handle(char('b')), SwitcherKey::TooLong);
        assert_eq!(switcher.name().len(), MAX_SESSION_ID_BYTES);
    }

    #[test]
    fn up_leaves_the_name_row_and_down_there_types_again() {
        let mut switcher = on_the_name_row();
        typed(&mut switcher, "new");

        switcher.handle(key(KeyCode::Up));
        assert_eq!((switcher.cursor(), switcher.typing()), (2, false));
        switcher.handle(key(KeyCode::Down));
        assert_eq!((switcher.cursor(), switcher.typing()), (3, true));
        assert_eq!(switcher.name(), "new", "the name stays");
    }

    #[test]
    fn esc_stops_typing_and_the_keys_act_as_on_other_rows() {
        let mut switcher = on_the_name_row();
        typed(&mut switcher, "new");

        assert_eq!(switcher.handle(key(KeyCode::Esc)), SwitcherKey::Stay);
        assert!(!switcher.typing());
        assert_eq!(switcher.handle(char('j')), SwitcherKey::Stay);
        assert!(switcher.typing(), "down on the last row types again");
        switcher.handle(key(KeyCode::Esc));
        assert_eq!(switcher.handle(char('q')), SwitcherKey::Close);
    }

    #[test]
    fn enter_creates_the_queue_named() {
        let mut switcher = on_the_name_row();
        typed(&mut switcher, "weekly-review");

        assert_eq!(
            switcher.handle(key(KeyCode::Enter)),
            SwitcherKey::Switch(named("weekly-review"))
        );
    }

    #[test]
    fn enter_with_a_name_also_after_esc() {
        let mut switcher = on_the_name_row();
        typed(&mut switcher, "weekly-review");
        switcher.handle(key(KeyCode::Esc));

        assert_eq!(
            switcher.handle(key(KeyCode::Enter)),
            SwitcherKey::Switch(named("weekly-review"))
        );
    }

    #[test]
    fn enter_on_an_empty_name_does_nothing() {
        let mut switcher = on_the_name_row();

        assert_eq!(switcher.handle(key(KeyCode::Enter)), SwitcherKey::Stay);
        assert_eq!(switcher.cursor(), 3);
    }

    #[test]
    fn a_name_of_an_existing_queue_switches_to_that_queue() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let work = QueueLocation::named(scratch.path(), "work");
        work.create_layout().expect("layout");
        let mut choice = choice();
        choice.root = Some(scratch.path().to_path_buf());
        choice.rows[2].location = work.clone();
        let mut switcher = Switcher::new(choice);
        for _ in 0..3 {
            switcher.handle(key(KeyCode::Down));
        }
        // On a filesystem that ignores case `Work` is the directory of
        // `work`; elsewhere it is a queue of its own.
        typed(&mut switcher, "Work");

        let target = switcher.handle(key(KeyCode::Enter));

        let same = QueueLocation::named(scratch.path(), "Work").same_queue(&work);
        let expected = if same {
            work
        } else {
            QueueLocation::named(scratch.path(), "Work")
        };
        assert_eq!(target, SwitcherKey::Switch(expected));
    }

    #[test]
    fn the_name_of_the_watched_queue_only_closes() {
        let mut switcher = on_the_name_row();
        typed(&mut switcher, "asqr-todos");

        assert_eq!(switcher.handle(key(KeyCode::Enter)), SwitcherKey::Close);
    }

    // -----------------------------------------------------------------
    // The start picker
    // -----------------------------------------------------------------

    /// `asqr-todos`, `default` and `work`, none watched, outside tmux.
    fn picker() -> Switcher {
        let mut choice = choice();
        choice.rows[0].watched = false;
        choice.place = Place::Outside;
        Switcher::picker(choice)
    }

    #[test]
    fn the_picker_starts_on_default() {
        let picker = picker();

        assert_eq!(picker.cursor(), 1);
        assert!(picker.picking());
    }

    #[test]
    fn with_default_taken_the_picker_starts_on_the_first_free_queue() {
        let mut choice = choice();
        choice.rows.remove(1);
        choice.rows[0].watched = false;

        assert_eq!(Switcher::picker(choice).cursor(), 0);
    }

    #[test]
    fn with_no_free_queue_the_picker_starts_typing_a_new_one() {
        let mut choice = choice();
        choice.rows.clear();

        let picker = Switcher::picker(choice);

        assert_eq!((picker.cursor(), picker.typing()), (0, true));
    }

    #[test]
    fn in_the_picker_q_quits_and_esc_and_w_do_nothing() {
        let mut picker = picker();

        assert_eq!(picker.handle(key(KeyCode::Esc)), SwitcherKey::Stay);
        assert_eq!(picker.handle(char('w')), SwitcherKey::Stay);
        assert_eq!(picker.handle(char('q')), SwitcherKey::Quit);
    }

    #[test]
    fn in_the_picker_q_types_in_the_name_and_esc_stops_typing() {
        let mut picker = picker();
        for _ in 0..2 {
            picker.handle(key(KeyCode::Down));
        }

        assert_eq!(picker.handle(char('q')), SwitcherKey::Stay);
        assert_eq!(picker.name(), "q");
        picker.handle(key(KeyCode::Esc));
        assert!(!picker.typing());
        assert_eq!(picker.handle(char('q')), SwitcherKey::Quit);
    }

    #[test]
    fn enter_in_the_picker_watches_the_queue_under_the_cursor() {
        let mut picker = picker();

        assert_eq!(
            picker.handle(key(KeyCode::Enter)),
            SwitcherKey::Switch(named("default"))
        );
    }

    #[test]
    fn without_a_root_there_is_no_new_queue_row() {
        let mut choice = choice();
        choice.root = None;
        let mut switcher = Switcher::new(choice);

        for _ in 0..5 {
            switcher.handle(key(KeyCode::Down));
        }

        assert_eq!((switcher.cursor(), switcher.typing()), (2, false));
    }

    #[test]
    fn a_paste_types_into_the_new_queue_name() {
        let mut switcher = on_the_name_row();
        typed(&mut switcher, "weekly");

        assert_eq!(switcher.paste("-review"), SwitcherKey::Stay);
        assert_eq!(switcher.name(), "weekly-review");
    }

    #[test]
    fn a_paste_on_the_name_row_left_with_esc_gives_it_the_keys() {
        let mut switcher = on_the_name_row();
        switcher.handle(key(KeyCode::Esc));

        switcher.paste("work");

        assert!(switcher.typing());
        assert_eq!(switcher.name(), "work");
    }

    #[test]
    fn a_paste_with_a_refused_character_inserts_nothing() {
        let mut switcher = on_the_name_row();

        assert_eq!(switcher.paste("my work"), SwitcherKey::Refused);
        assert_eq!(switcher.paste(".hidden"), SwitcherKey::Refused);
        assert_eq!(switcher.paste("work\n"), SwitcherKey::Refused);
        assert_eq!(
            switcher.paste(&"a".repeat(MAX_SESSION_ID_BYTES + 1)),
            SwitcherKey::TooLong
        );
        assert_eq!(switcher.name(), "");
    }

    #[test]
    fn a_paste_on_a_queue_row_goes_nowhere() {
        let mut switcher = switcher();

        assert_eq!(switcher.paste("work"), SwitcherKey::Stay);
        assert_eq!((switcher.cursor(), switcher.name()), (0, ""));
    }
}
