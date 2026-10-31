// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Text an agent wrote, as the commands print it to a terminal.

/// `text` with every control character replaced by U+FFFD. A title is
/// chosen by an agent, and an escape sequence or a newline in it would
/// change what the terminal shows around the line it stands in.
pub(crate) fn plain_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_escape_sequence_loses_its_escape() {
        assert_eq!(plain_line("a\x1b[31mb"), "a\u{fffd}[31mb");
    }

    #[test]
    fn a_newline_and_a_tab_are_replaced() {
        assert_eq!(plain_line("one\ntwo\tthree"), "one\u{fffd}two\u{fffd}three");
    }

    #[test]
    fn a_c1_control_is_replaced() {
        assert_eq!(plain_line("a\u{9b}b"), "a\u{fffd}b");
    }

    #[test]
    fn text_without_controls_is_left_as_it_is() {
        assert_eq!(
            plain_line("Pick the hero – ✔ 日本語"),
            "Pick the hero – ✔ 日本語"
        );
    }
}
