// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What asqr knows about the terminal it runs in before it draws: the
//! environment variables that name the terminal and tmux, and what tmux
//! says about asqr's pane. The alerts, the glyphs of Talqi's emblem and
//! the graphics protocol all decide from these, each read once at start,
//! so every reader applies the same rule to the same values.

use std::collections::BTreeMap;

// ---------------------------------------------------------------------
// The environment
// ---------------------------------------------------------------------

/// The variables a [`TerminalEnv`] holds. A reader of another variable
/// adds it here, or it always reads as unset.
const NAMES: [&str; 12] = [
    "TMUX",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "VTE_VERSION",
    "KONSOLE_VERSION",
    "TERMINAL_NAME",
    "KITTY_WINDOW_ID",
    "ALACRITTY_WINDOW_ID",
    "ZELLIJ",
];

/// The environment variables that name the terminal and tmux, as they
/// were when it was taken. A variable that is empty counts as unset, as
/// every reader here treats both alike.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalEnv {
    set: BTreeMap<&'static str, String>,
}

impl TerminalEnv {
    /// The variables of this process.
    pub fn from_process() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// An environment that holds `pairs` and nothing else, for tests and
    /// for callers that know the variables from elsewhere. Names asqr does
    /// not read are left out, as [`TerminalEnv::from_process`] leaves them.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Self::from_lookup(|name| {
            pairs
                .iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let set = NAMES
            .into_iter()
            .filter_map(|name| {
                lookup(name)
                    .filter(|value| !value.is_empty())
                    .map(|value| (name, value))
            })
            .collect();
        TerminalEnv { set }
    }

    /// The value of the variable `name` when it is set and not empty.
    pub fn set(&self, name: &str) -> Option<&str> {
        debug_assert!(
            NAMES.contains(&name),
            "{name} is not among the variables a TerminalEnv holds"
        );
        self.set.get(name).map(String::as_str)
    }

    /// Whether asqr runs in a tmux pane it can ask about itself: tmux sets
    /// `TMUX` in its panes, and ssh does not carry it along.
    pub fn in_tmux_pane(&self) -> bool {
        self.set("TMUX").is_some()
    }

    /// Whether the terminal names tmux: a `TERM` that starts with `tmux`,
    /// or a `TERM_PROGRAM` of `tmux`. ratatui-image takes exactly this for
    /// tmux, wraps every command for tmux's passthrough then, and runs
    /// `tmux set -p allow-passthrough on` in every picker it builds
    /// (12.0.0-rc.3 `picker.rs` l.336-350). Over ssh from a tmux pane,
    /// `TERM` comes along and `TMUX` does not.
    pub fn tmux_like_term(&self) -> bool {
        self.set("TERM")
            .is_some_and(|term| term.starts_with("tmux"))
            || self.set("TERM_PROGRAM") == Some("tmux")
    }
}

/// `sequence` wrapped for tmux's passthrough: tmux hands what is inside
/// the wrapper to the terminal outside, with every escape inside doubled,
/// and only where the pane's `allow-passthrough` lets it (spec section
/// 8.9).
pub fn passthrough(sequence: &[u8]) -> Vec<u8> {
    let mut wrapped = b"\x1bPtmux;".to_vec();
    for &byte in sequence {
        wrapped.push(byte);
        if byte == 0x1b {
            wrapped.push(byte);
        }
    }
    wrapped.extend_from_slice(b"\x1b\\");
    wrapped
}

// ---------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------

/// Whether the terminal version `version` is `floor` or later. A version
/// that cannot be read counts as earlier, so a feature that needs a
/// known version stays off for it.
pub fn version_at_least(version: &str, floor: &[u32]) -> bool {
    version_numbers(version).is_some_and(|numbers| !below(&numbers, floor))
}

/// The numbers at the start of `version`, such as `[3, 6, 7]` for
/// `3.6.7beta2` and `[20240203]` for `20240203-110809-5046fc22`: the
/// digits of each part between dots, up to the first part that goes on
/// with something else. That counts a pre-release as its release.
/// `None` when `version` does not start with a digit.
fn version_numbers(version: &str) -> Option<Vec<u32>> {
    let mut numbers = Vec::new();
    for part in version.split('.') {
        let digits = part.len() - part.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let Ok(number) = part[..digits].parse() else {
            break;
        };
        numbers.push(number);
        if digits < part.len() {
            break;
        }
    }
    (!numbers.is_empty()).then_some(numbers)
}

/// Whether `numbers` come before `floor`, a missing number counting as 0.
fn below(numbers: &[u32], floor: &[u32]) -> bool {
    let length = numbers.len().max(floor.len());
    let padded = |of: &[u32]| {
        let mut padded = of.to_vec();
        padded.resize(length, 0);
        padded
    };
    padded(numbers) < padded(floor)
}

// ---------------------------------------------------------------------
// What tmux says about the pane
// ---------------------------------------------------------------------

/// What tmux said about asqr's pane, asked once at start with
/// [`TmuxState::FORMAT`]. A field that is missing or reads otherwise than
/// tmux writes it is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxState {
    passthrough: Option<String>,
    clients: Option<u32>,
    termtype: Option<String>,
    shown: Option<bool>,
    version: Option<String>,
}

impl TmuxState {
    /// What asqr asks tmux about its pane, for `tmux display-message -p`:
    /// the pane's passthrough option, the number of clients attached to
    /// the session, the client terminal's answer to XTVERSION, such as
    /// `iTerm2 3.6.7`, whether the pane's window is the current one of its
    /// session, whether the pane is the current one of its window, whether
    /// the window is zoomed, and tmux's version. The answer to XTVERSION
    /// holds blanks, so tabs keep the fields apart.
    pub const FORMAT: &str = "#{allow-passthrough}\t#{session_attached}\t#{client_termtype}\t\
         #{window_active}\t#{pane_active}\t#{window_zoomed_flag}\t#{version}";

    /// Reads what tmux printed for [`TmuxState::FORMAT`].
    pub fn parse(answer: &str) -> Self {
        let mut fields = answer.trim_end_matches('\n').split('\t');
        let passthrough = fields.next().map(str::to_owned);
        let clients = fields.next().and_then(|clients| clients.parse().ok());
        let termtype = fields.next().map(str::to_owned);
        let mut flag = || match fields.next() {
            Some("1") => Some(true),
            Some("0") => Some(false),
            _ => None,
        };
        let (window_active, pane_active, zoomed) = (flag(), flag(), flag());
        let shown = window_active.zip(pane_active).zip(zoomed).map(
            |((window_active, pane_active), zoomed)| window_active && (pane_active || !zoomed),
        );
        TmuxState {
            passthrough,
            clients,
            termtype,
            shown,
            version: fields.next().map(str::to_owned),
        }
    }

    /// The pane's `allow-passthrough`: `off`, `on` or `all`.
    pub fn passthrough(&self) -> Option<&str> {
        self.passthrough.as_deref()
    }

    /// How many clients are attached to the pane's session.
    pub fn clients(&self) -> Option<u32> {
        self.clients
    }

    /// The client terminal's answer to XTVERSION, empty when it gave none.
    pub fn termtype(&self) -> Option<&str> {
        self.termtype.as_deref()
    }

    /// Whether a client attached to the session shows the pane: its window
    /// is the session's current one, and no other pane of it is zoomed.
    pub fn shown(&self) -> Option<bool> {
        self.shown
    }

    /// Whether tmux is 3.4 or later, which knows `allow-passthrough all`.
    /// A development build names the version it leads to, as `next-3.6`.
    /// A version tmux names otherwise, as OpenBSD's base system does, does
    /// not count.
    pub fn knows_passthrough_all(&self) -> bool {
        let Some(version) = self.version.as_deref() else {
            return false;
        };
        let version = version.strip_prefix("next-").unwrap_or(version);
        let Some((major, rest)) = version.split_once('.') else {
            return false;
        };
        let minor_digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        match (major.parse::<u32>(), rest[..minor_digits].parse::<u32>()) {
            (Ok(major), Ok(minor)) => (major, minor) >= (3, 4),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_variable_counts_as_unset_and_others_are_not_kept() {
        let env = TerminalEnv::from_pairs(&[
            ("TERM", "xterm-kitty"),
            ("TMUX", ""),
            ("SSH_TTY", "/dev/pts/3"),
        ]);

        assert_eq!(env.set("TERM"), Some("xterm-kitty"));
        assert_eq!(env.set("TMUX"), None);
        assert_eq!(env, TerminalEnv::from_pairs(&[("TERM", "xterm-kitty")]));
    }

    #[test]
    fn the_process_environment_is_read_by_the_same_rule() {
        let env = TerminalEnv::from_process();

        for name in NAMES {
            let value = std::env::var(name).ok().filter(|value| !value.is_empty());
            assert_eq!(env.set(name), value.as_deref(), "{name}");
        }
    }

    #[test]
    fn a_tmux_pane_is_known_by_its_variable() {
        let pane = |tmux: &str| TerminalEnv::from_pairs(&[("TMUX", tmux)]).in_tmux_pane();

        assert!(pane("/tmp/tmux-501/default,1234,0"));
        assert!(!pane(""), "an empty TMUX is unset");
        assert!(!TerminalEnv::default().in_tmux_pane());
    }

    /// ratatui-image's test for tmux, which ssh from a tmux pane passes
    /// without `TMUX`.
    #[test]
    fn a_term_names_tmux_as_ratatui_image_reads_it() {
        let tmux_like = |pairs: &[(&str, &str)]| TerminalEnv::from_pairs(pairs).tmux_like_term();

        assert!(tmux_like(&[("TERM", "tmux-256color")]));
        assert!(tmux_like(&[("TERM", "tmux")]));
        assert!(tmux_like(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "tmux")
        ]));
        for pairs in [
            &[("TERM", "screen-256color")][..],
            &[("TERM", "xterm-ghostty"), ("TERM_PROGRAM", "ghostty")],
            &[("TMUX", "/tmp/tmux-501/default,1234,0")],
            &[],
        ] {
            assert!(!tmux_like(pairs), "{pairs:?}");
        }
    }

    /// Every escape inside the wrapper is doubled.
    #[test]
    fn a_sequence_for_the_outer_terminal_is_wrapped_for_tmux() {
        assert_eq!(
            passthrough(b"\x1b]9;asqr\x07"),
            b"\x1bPtmux;\x1b\x1b]9;asqr\x07\x1b\\"
        );
        assert_eq!(passthrough(b""), b"\x1bPtmux;\x1b\\");
    }

    #[test]
    fn the_format_keeps_the_fields_apart_with_tabs() {
        assert_eq!(
            TmuxState::FORMAT,
            "#{allow-passthrough}\t#{session_attached}\t#{client_termtype}\t\
             #{window_active}\t#{pane_active}\t#{window_zoomed_flag}\t#{version}"
        );
    }

    /// The client's answer to XTVERSION holds blanks, and only tabs part
    /// the fields.
    #[test]
    fn every_field_of_tmux_s_answer_is_read() {
        let state = TmuxState::parse("all\t2\tWezTerm 20240203-110809-5046fc22\t1\t1\t0\t3.5a\n");

        assert_eq!(state.passthrough(), Some("all"));
        assert_eq!(state.clients(), Some(2));
        assert_eq!(state.termtype(), Some("WezTerm 20240203-110809-5046fc22"));
        assert_eq!(state.shown(), Some(true));
        assert!(state.knows_passthrough_all());
    }

    #[test]
    fn a_field_that_reads_otherwise_is_none() {
        let state = TmuxState::parse("on 1 kitty(0.35.2)\n");
        assert_eq!(state.passthrough(), Some("on 1 kitty(0.35.2)"));
        assert_eq!(
            (state.clients(), state.termtype(), state.shown()),
            (None, None, None),
            "blanks do not part the fields"
        );

        let state = TmuxState::parse("on\tmany\t\tyes\t1\t0\n");
        assert_eq!(state.clients(), None);
        assert_eq!(state.termtype(), Some(""));
        assert_eq!(state.shown(), None);
        assert_eq!(
            TmuxState::parse(""),
            TmuxState {
                passthrough: Some(String::new()),
                ..TmuxState::default()
            }
        );
    }

    /// With `on`, tmux passes nothing through from a pane no client shows:
    /// one in a window that is not the current one of its session, or one
    /// that another pane's zoom hides.
    #[test]
    fn a_pane_shows_in_the_current_window_unless_another_is_zoomed() {
        let shown = |flags: &str| TmuxState::parse(&format!("on\t1\t\t{flags}\t3.5")).shown();

        assert_eq!(shown("1\t1\t0"), Some(true));
        assert_eq!(
            shown("1\t0\t0"),
            Some(true),
            "a pane beside the current one"
        );
        assert_eq!(shown("1\t1\t1"), Some(true), "the zoomed pane");
        assert_eq!(shown("0\t1\t0"), Some(false), "a window in the background");
        assert_eq!(shown("1\t0\t1"), Some(false), "a pane hidden by a zoom");
    }

    #[test]
    fn tmux_knows_passthrough_all_from_3_4() {
        let knows = |version: &str| {
            TmuxState::parse(&format!("on\t1\t\t1\t1\t0\t{version}")).knows_passthrough_all()
        };
        for version in ["3.4", "3.5a", "3.10", "next-3.6", "4.0"] {
            assert!(knows(version), "{version}");
        }
        for version in ["3.3a", "3.3", "2.9", "openbsd-7.5", "", "three", "3"] {
            assert!(!knows(version), "{version:?}");
        }
        assert!(
            !TmuxState::parse("on\t1\t\t1\t1\t0\n").knows_passthrough_all(),
            "no version"
        );
    }
}
