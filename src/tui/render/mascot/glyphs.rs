// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Which glyphs Talqi's emblem is drawn in: sextants where asqr knows the
//! terminal draws them itself, quadrants everywhere else (ADR 43).
//!
//! No escape sequence tells whether a terminal can show a glyph, and
//! where neither the terminal nor its font draws sextants they come out
//! as boxes of a missing glyph. So asqr goes by the terminal's identity,
//! from the list of terminals that draw sextants without a font and the
//! first version that does. A terminal not on the list gets quadrants,
//! even if its font has sextants.

use super::template::Glyphs;
use crate::tui::render::images::tmux_state_fields;

impl Glyphs {
    /// The glyphs for the terminal asqr runs in, decided once at start.
    /// `var` reads an environment variable, and `tmux_state` is what tmux
    /// said about the pane in the format of
    /// [`TMUX_STATE_FORMAT`](crate::tui::render::TMUX_STATE_FORMAT), if
    /// asqr runs inside tmux and tmux answered.
    ///
    /// Inside tmux, the environment holds what the terminal tmux was
    /// started from set, which may be another than the one a client
    /// attached from later. tmux knows the client's terminal by its
    /// answer to XTVERSION, so that answer decides there. Without one,
    /// as without tmux's answer, the emblem is drawn in quadrants.
    pub fn for_terminal(var: impl Fn(&str) -> Option<String>, tmux_state: Option<&str>) -> Glyphs {
        let set = |name: &str| var(name).filter(|value| !value.is_empty());
        if set("TMUX").is_none() {
            return Glyphs::from_env(set);
        }
        tmux_state
            .and_then(|state| tmux_state_fields(state).nth(2))
            .map_or(Glyphs::Quadrant, Glyphs::from_xtversion)
    }

    /// The glyphs for the terminal the environment names through `set`,
    /// which reads a variable that is set and not empty. The first rule
    /// that names a terminal decides.
    fn from_env(set: impl Fn(&str) -> Option<String>) -> Glyphs {
        let term = set("TERM").unwrap_or_default();
        let term_program = set("TERM_PROGRAM");
        let version = || set("TERM_PROGRAM_VERSION").unwrap_or_default();
        // GNU screen and Zellij draw their own screen in a terminal asqr
        // cannot ask about, and pass on the variables of whichever
        // terminal they were started from.
        if term.starts_with("screen") || set("ZELLIJ").is_some() {
            return Glyphs::Quadrant;
        }
        // A terminal sets `TERM_PROGRAM` itself, while a `KITTY_WINDOW_ID`
        // or an `ALACRITTY_WINDOW_ID` may be left from the terminal another
        // one was started from. So a `TERM_PROGRAM` decides first, also
        // one of a terminal not on the list.
        if let Some(program) = term_program {
            return match program.as_str() {
                "ghostty" => Glyphs::Sextant,
                "WezTerm" => Glyphs::from_version(&version(), &[20_210_814]),
                "iTerm.app" => Glyphs::from_version(&version(), &[3, 6, 7]),
                "rio" => Glyphs::from_version(&version(), &[0, 4, 6]),
                _ => Glyphs::Quadrant,
            };
        }
        // iTerm2's variables come along over ssh, where `TERM_PROGRAM`
        // stays behind.
        if set("LC_TERMINAL").as_deref() == Some("iTerm2") {
            let version = set("LC_TERMINAL_VERSION").unwrap_or_default();
            return Glyphs::from_version(&version, &[3, 6, 7]);
        }
        if let Some(vte) = set("VTE_VERSION") {
            return Glyphs::from_version(&vte, &[6000]);
        }
        if let Some(konsole) = set("KONSOLE_VERSION") {
            return Glyphs::from_version(&konsole, &[220_400]);
        }
        // Ghostty's and kitty's `TERM` come along over ssh too. kitty and
        // Alacritty name no version, and Contour is on the list without a
        // first version.
        let known = term == "xterm-ghostty"
            || term == "xterm-kitty"
            || term.starts_with("foot")
            || term == "alacritty"
            || set("TERMINAL_NAME").as_deref() == Some("contour")
            || set("KITTY_WINDOW_ID").is_some()
            || set("ALACRITTY_WINDOW_ID").is_some();
        if known {
            Glyphs::Sextant
        } else {
            Glyphs::Quadrant
        }
    }

    /// The glyphs for the terminal that answered XTVERSION with `text`,
    /// such as `kitty(0.35.2)` or `iTerm2 3.6.7`. Alacritty does not
    /// answer XTVERSION, so it gets quadrants here, and so does a tmux
    /// inside tmux, whose client is another tmux.
    fn from_xtversion(text: &str) -> Glyphs {
        let after = |prefix: &str| text.strip_prefix(prefix);
        if after("kitty(")
            .or(after("foot("))
            .or(after("ghostty "))
            .or(after("contour "))
            .is_some()
        {
            return Glyphs::Sextant;
        }
        let floors: [(&str, &[u32]); 5] = [
            ("WezTerm ", &[20_210_814]),
            ("iTerm2 ", &[3, 6, 7]),
            ("VTE(", &[6000]),
            ("Konsole ", &[220_400]),
            ("Rio ", &[0, 4, 6]),
        ];
        floors
            .iter()
            .find_map(|(prefix, floor)| {
                after(prefix).map(|version| Glyphs::from_version(version, floor))
            })
            .unwrap_or(Glyphs::Quadrant)
    }

    /// Sextants when `version` is `floor` or later, otherwise quadrants,
    /// also when `version` cannot be read.
    fn from_version(version: &str, floor: &[u32]) -> Glyphs {
        match version_numbers(version) {
            Some(numbers) if !below(&numbers, floor) => Glyphs::Sextant,
            _ => Glyphs::Quadrant,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The glyphs for a terminal whose environment holds `vars` and
    /// nothing else, outside tmux.
    fn outside_tmux(vars: &[(&str, &str)]) -> Glyphs {
        inside_tmux(vars, None)
    }

    /// The glyphs for a terminal whose environment holds `vars`, where
    /// tmux said `state` about the pane.
    fn inside_tmux(vars: &[(&str, &str)], state: Option<&str>) -> Glyphs {
        let var = |name: &str| {
            vars.iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| (*value).to_owned())
        };
        Glyphs::for_terminal(var, state)
    }

    use Glyphs::{Quadrant, Sextant};

    #[test]
    fn terminals_that_draw_sextants_themselves_get_them() {
        for vars in [
            &[("TERM_PROGRAM", "ghostty")][..],
            &[("TERM", "xterm-ghostty")],
            &[("TERM", "xterm-kitty")],
            &[("KITTY_WINDOW_ID", "1")],
            &[
                ("TERM_PROGRAM", "WezTerm"),
                ("TERM_PROGRAM_VERSION", "20240203-110809-5046fc22"),
            ],
            &[
                ("TERM_PROGRAM", "iTerm.app"),
                ("TERM_PROGRAM_VERSION", "3.6.9"),
            ],
            &[("LC_TERMINAL", "iTerm2"), ("LC_TERMINAL_VERSION", "3.6.9")],
            &[("TERM", "foot")],
            &[("TERM", "foot-extra")],
            &[("VTE_VERSION", "7802")],
            &[("KONSOLE_VERSION", "240802")],
            &[("TERM", "alacritty")],
            &[("ALACRITTY_WINDOW_ID", "4294967299")],
            &[("TERM_PROGRAM", "rio"), ("TERM_PROGRAM_VERSION", "0.4.7")],
            &[("TERMINAL_NAME", "contour")],
        ] {
            assert_eq!(outside_tmux(vars), Sextant, "{vars:?}");
        }
    }

    #[test]
    fn other_terminals_get_quadrants() {
        for vars in [
            &[][..],
            &[("TERM", "xterm-256color")],
            &[
                ("TERM_PROGRAM", "Apple_Terminal"),
                ("TERM", "xterm-256color"),
            ],
            &[("TERM_PROGRAM", "WarpTerminal")],
            &[("TERM_PROGRAM", "vscode")],
            &[("TERM", "linux")],
            &[("WT_SESSION", "1")],
        ] {
            assert_eq!(outside_tmux(vars), Quadrant, "{vars:?}");
        }
    }

    /// Each terminal from the first version that draws sextants itself,
    /// and the version before it.
    #[test]
    fn a_version_before_the_first_with_sextants_gets_quadrants() {
        let at = |program: &str, version: &str| {
            outside_tmux(&[("TERM_PROGRAM", program), ("TERM_PROGRAM_VERSION", version)])
        };
        assert_eq!(at("WezTerm", "20210814-124438-54e29167"), Sextant);
        assert_eq!(at("WezTerm", "20210502-154244-3f7122cb"), Quadrant);
        assert_eq!(at("iTerm.app", "3.6.7"), Sextant);
        assert_eq!(at("iTerm.app", "3.6.6"), Quadrant);
        assert_eq!(at("iTerm.app", "3.7"), Sextant);
        assert_eq!(at("iTerm.app", "3.6"), Quadrant);
        assert_eq!(at("iTerm.app", "4"), Sextant);
        assert_eq!(at("rio", "0.4.6"), Sextant);
        assert_eq!(at("rio", "0.4.5"), Quadrant);

        let lc_terminal = |version: &str| {
            outside_tmux(&[("LC_TERMINAL", "iTerm2"), ("LC_TERMINAL_VERSION", version)])
        };
        assert_eq!(lc_terminal("3.6.7"), Sextant);
        assert_eq!(lc_terminal("3.5.14"), Quadrant);

        let vte = |version: &str| outside_tmux(&[("VTE_VERSION", version)]);
        assert_eq!(vte("6000"), Sextant);
        assert_eq!(vte("5999"), Quadrant);

        let konsole = |version: &str| outside_tmux(&[("KONSOLE_VERSION", version)]);
        assert_eq!(konsole("220400"), Sextant);
        assert_eq!(konsole("211203"), Quadrant);
    }

    /// iTerm2 drew sextants itself from 3.6.7beta2 on, and every
    /// pre-release counts as its release.
    #[test]
    fn a_pre_release_counts_as_its_release() {
        let iterm = |version: &str| {
            outside_tmux(&[
                ("TERM_PROGRAM", "iTerm.app"),
                ("TERM_PROGRAM_VERSION", version),
            ])
        };
        assert_eq!(iterm("3.6.7beta2"), Sextant);
        assert_eq!(iterm("3.6.6beta1"), Quadrant);
        assert_eq!(iterm("3.6.10"), Sextant, "numbers, not text");
    }

    #[test]
    fn a_version_that_cannot_be_read_gets_quadrants() {
        for vars in [
            &[("TERM_PROGRAM", "WezTerm")][..],
            &[("TERM_PROGRAM", "WezTerm"), ("TERM_PROGRAM_VERSION", "")],
            &[
                ("TERM_PROGRAM", "WezTerm"),
                ("TERM_PROGRAM_VERSION", "nightly"),
            ],
            &[("TERM_PROGRAM", "iTerm.app")],
            &[
                ("TERM_PROGRAM", "iTerm.app"),
                ("TERM_PROGRAM_VERSION", "v3.6.7"),
            ],
            &[("LC_TERMINAL", "iTerm2")],
            &[("TERM_PROGRAM", "rio"), ("TERM_PROGRAM_VERSION", "x")],
            &[("VTE_VERSION", "")],
            &[("KONSOLE_VERSION", "twenty")],
        ] {
            assert_eq!(outside_tmux(vars), Quadrant, "{vars:?}");
        }
    }

    /// kitty and Alacritty name no version in the environment; every
    /// release that sets their variables gets sextants.
    #[test]
    fn kitty_and_alacritty_need_no_version() {
        assert_eq!(outside_tmux(&[("TERM", "xterm-kitty")]), Sextant);
        assert_eq!(outside_tmux(&[("TERM", "alacritty")]), Sextant);
    }

    /// Ghostty's `TERM` comes along over ssh, where `TERM_PROGRAM` stays
    /// behind.
    #[test]
    fn ghostty_is_known_by_its_term_over_ssh() {
        assert_eq!(
            outside_tmux(&[("TERM", "xterm-ghostty"), ("SSH_TTY", "/dev/pts/3")]),
            Sextant
        );
    }

    /// A terminal started from another may keep the other's variables,
    /// such as `KITTY_WINDOW_ID`. The terminal's own `TERM_PROGRAM` comes
    /// first.
    #[test]
    fn term_program_decides_before_a_window_id() {
        for vars in [
            &[("TERM_PROGRAM", "Apple_Terminal"), ("KITTY_WINDOW_ID", "1")][..],
            &[
                ("TERM_PROGRAM", "iTerm.app"),
                ("TERM_PROGRAM_VERSION", "3.5.0"),
                ("ALACRITTY_WINDOW_ID", "1"),
            ],
        ] {
            assert_eq!(outside_tmux(vars), Quadrant, "{vars:?}");
        }
        assert_eq!(
            outside_tmux(&[("TERM_PROGRAM", "ghostty"), ("KITTY_WINDOW_ID", "1")]),
            Sextant
        );
    }

    /// GNU screen and Zellij draw their own screen in the terminal around
    /// them, which asqr cannot ask about, and the variables they pass on
    /// may come from another terminal.
    #[test]
    fn screen_and_zellij_get_quadrants() {
        for vars in [
            &[("TERM", "screen-256color"), ("KITTY_WINDOW_ID", "1")][..],
            &[("TERM", "screen.xterm-256color"), ("VTE_VERSION", "7802")],
            &[("ZELLIJ", "0"), ("TERM_PROGRAM", "ghostty")],
            &[("ZELLIJ", "0"), ("TERM", "xterm-kitty")],
        ] {
            assert_eq!(outside_tmux(vars), Quadrant, "{vars:?}");
        }
        assert_eq!(
            outside_tmux(&[("ZELLIJ", ""), ("TERM", "xterm-kitty")]),
            Sextant,
            "an empty ZELLIJ counts as unset"
        );
    }

    // -----------------------------------------------------------------
    // Inside tmux
    // -----------------------------------------------------------------

    const TMUX: (&str, &str) = ("TMUX", "/tmp/tmux-501/default,4242,0");

    /// The state tmux answers for a client whose terminal said `termtype`.
    fn state(termtype: &str) -> String {
        format!("on\t1\t{termtype}\n")
    }

    /// Inside tmux the variables of the terminal tmux was started from
    /// may be stale: tmux keeps them for every pane, also for a client
    /// that attached later from another terminal.
    #[test]
    fn inside_tmux_the_environment_does_not_count() {
        for vars in [
            &[TMUX, ("KITTY_WINDOW_ID", "1")][..],
            &[TMUX, ("TERM_PROGRAM", "ghostty")],
            &[TMUX, ("TERM", "xterm-ghostty")],
        ] {
            assert_eq!(inside_tmux(vars, None), Quadrant, "{vars:?}");
            assert_eq!(inside_tmux(vars, Some(&state(""))), Quadrant, "{vars:?}");
        }
        assert_eq!(
            inside_tmux(&[("TMUX", ""), ("TERM", "xterm-kitty")], None),
            Sextant,
            "an empty TMUX counts as unset"
        );
    }

    /// The terminal of the client tmux draws for, by its answer to
    /// XTVERSION, which holds blanks, and the first version that draws
    /// sextants itself.
    #[test]
    fn inside_tmux_the_client_s_terminal_decides() {
        for (termtype, glyphs) in [
            ("kitty(0.35.2)", Sextant),
            ("foot(1.20.2)", Sextant),
            ("WezTerm 20240203-110809-5046fc22", Sextant),
            ("WezTerm 20210502-154244-3f7122cb", Quadrant),
            ("ghostty 1.1.3", Sextant),
            ("iTerm2 3.6.7", Sextant),
            ("iTerm2 3.6.7beta2", Sextant),
            ("iTerm2 3.6.6", Quadrant),
            ("VTE(7802)", Sextant),
            ("VTE(5999)", Quadrant),
            ("Konsole 220400", Sextant),
            ("Konsole 211203", Quadrant),
            ("contour 0.4.3.6442", Sextant),
            ("Rio 0.4.6", Sextant),
            ("Rio 0.4.5", Quadrant),
            ("", Quadrant),
            ("XTerm(390)", Quadrant),
            ("tmux 3.5a", Quadrant),
            ("iTerm2", Quadrant),
        ] {
            assert_eq!(
                inside_tmux(&[TMUX], Some(&state(termtype))),
                glyphs,
                "{termtype:?}"
            );
        }
    }

    #[test]
    fn inside_tmux_a_state_without_a_termtype_gets_quadrants() {
        for state in ["", "on\t1\n", "on 1 kitty(0.35.2)\n"] {
            assert_eq!(inside_tmux(&[TMUX], Some(state)), Quadrant, "{state:?}");
        }
    }
}
