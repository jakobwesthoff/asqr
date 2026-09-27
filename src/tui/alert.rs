// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Telling the person that sessions arrived (spec section 7.5): a desktop
//! notification through an OSC escape sequence, and the terminal bell.

use std::io::{self, Write};

/// Which alerts are on; `--no-notify` and `--no-bell` switch them off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alerts {
    pub notify: bool,
    pub bell: bool,
}

/// What the environment says about the terminal the alerts go to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlertTerminal {
    /// The `TERM` variable, which picks the escape sequence.
    pub term: Option<String>,
    /// Whether asqr runs inside tmux (`TMUX` is set).
    pub tmux: bool,
}

/// Writes the alerts for the sessions in `arrived` to `out`.
pub fn alert(
    out: &mut impl Write,
    alerts: Alerts,
    terminal: &AlertTerminal,
    arrived: &[String],
) -> io::Result<()> {
    let term = terminal.term.as_deref();
    let body = match arrived {
        [] => return Ok(()),
        [id] => format!("new session {id}"),
        many => format!("{} new sessions", many.len()),
    };
    if alerts.notify {
        // OSC 9 is what Ghostty, iTerm2, kitty and WezTerm show. rxvt,
        // through its notify extension, and foot document OSC 777, which
        // takes a title of its own. Session ids hold no control
        // characters, so the body cannot end the sequence early.
        let sequence =
            if term.is_some_and(|term| term.starts_with("rxvt") || term.starts_with("foot")) {
                format!("\x1b]777;notify;asqr;{body}\x07")
            } else {
                format!("\x1b]9;asqr: {body}\x07")
            };
        if terminal.tmux {
            // tmux hands a sequence to the outer terminal only inside its
            // passthrough wrapper, with every escape doubled, and only
            // with `allow-passthrough on` (spec section 7.7).
            write!(
                out,
                "\x1bPtmux;{}\x1b\\",
                sequence.replace('\x1b', "\x1b\x1b")
            )?;
        } else {
            out.write_all(sequence.as_bytes())?;
        }
    }
    if alerts.bell {
        out.write_all(b"\x07")?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Alerts = Alerts {
        notify: true,
        bell: true,
    };

    fn written(alerts: Alerts, term: Option<&str>, arrived: &[&str]) -> String {
        let arrived: Vec<String> = arrived.iter().map(|id| (*id).to_owned()).collect();
        let terminal = AlertTerminal {
            term: term.map(str::to_owned),
            tmux: false,
        };
        let mut out = Vec::new();
        alert(&mut out, alerts, &terminal, &arrived).expect("written");
        String::from_utf8(out).expect("UTF-8")
    }

    #[test]
    fn one_session_is_named_in_an_osc_9_notification_and_rings_the_bell() {
        assert_eq!(
            written(ALL, Some("xterm-ghostty"), &["batch-01"]),
            "\x1b]9;asqr: new session batch-01\x07\x07"
        );
    }

    #[test]
    fn several_sessions_are_counted() {
        assert_eq!(
            written(ALL, None, &["a", "b", "c"]),
            "\x1b]9;asqr: 3 new sessions\x07\x07"
        );
    }

    #[test]
    fn terminals_that_only_know_osc_777_get_that() {
        for term in ["rxvt-unicode-256color", "foot", "foot-extra"] {
            assert_eq!(
                written(ALL, Some(term), &["batch-01"]),
                "\x1b]777;notify;asqr;new session batch-01\x07\x07",
                "{term}"
            );
        }
    }

    #[test]
    fn each_alert_can_be_switched_off() {
        let no_bell = Alerts {
            notify: true,
            bell: false,
        };
        let no_notify = Alerts {
            notify: false,
            bell: true,
        };
        let none = Alerts {
            notify: false,
            bell: false,
        };

        assert_eq!(
            written(no_bell, None, &["x"]),
            "\x1b]9;asqr: new session x\x07"
        );
        assert_eq!(written(no_notify, None, &["x"]), "\x07");
        assert_eq!(written(none, None, &["x"]), "");
    }

    #[test]
    fn inside_tmux_the_notification_is_wrapped_for_passthrough() {
        let mut out = Vec::new();
        let tmux = AlertTerminal {
            term: Some("tmux-256color".into()),
            tmux: true,
        };

        alert(&mut out, ALL, &tmux, &["batch-01".to_owned()]).expect("written");

        // Every escape inside the wrapper is doubled; the bell goes to
        // tmux itself, which passes it on without passthrough.
        assert_eq!(
            String::from_utf8(out).expect("UTF-8"),
            "\x1bPtmux;\x1b\x1b]9;asqr: new session batch-01\x07\x1b\\\x07"
        );
    }

    #[test]
    fn nothing_arrived_means_no_alert() {
        assert_eq!(written(ALL, None, &[]), "");
    }
}
