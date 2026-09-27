// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Session ids (spec section 3.3). A session id is also a file stem in
//! the queue, so its syntax keeps it a safe, visible file name on every
//! platform.

/// The longest id in bytes. Together with the `.<ulid>.json` suffix of
/// archive names it stays well below the common 255-byte file name limit.
pub const MAX_SESSION_ID_BYTES: usize = 200;

/// Whether `id` is a valid session id: letters, digits, `-`, `_` and `.`,
/// not starting with `.`, at most [`MAX_SESSION_ID_BYTES`] long.
pub fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SESSION_ID_BYTES
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Whether two session ids name the same session. Ids compare
/// case-insensitively on every platform, so a case-insensitive file system
/// (macOS by default) and a case-sensitive one follow the same rules. The
/// id syntax is ASCII only, so ASCII case folding is complete.
pub fn same_session_id(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_letters_digits_dash_underscore_and_dot() {
        assert!(is_valid_session_id("alt-rework-3_batch.01"));
        assert!(is_valid_session_id("01K6BX5V2JQ7Z9H4M3N8P0R1ST"));
    }

    #[test]
    fn rejects_empty_ids_and_other_characters() {
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("with space"));
        assert!(!is_valid_session_id("slash/inside"));
        assert!(!is_valid_session_id("umlaut-ä"));
    }

    #[test]
    fn rejects_a_leading_dot() {
        // A dotfile in the inbox is ignored by the watcher (spec section 3.4),
        // so such an id could never be answered.
        assert!(!is_valid_session_id(".hidden"));
        assert!(is_valid_session_id("not.hidden"));
    }

    #[test]
    fn allows_at_most_200_bytes() {
        assert!(is_valid_session_id(&"a".repeat(200)));
        assert!(!is_valid_session_id(&"a".repeat(201)));
    }

    #[test]
    fn compares_ids_case_insensitively() {
        assert!(same_session_id("Batch-01", "batch-01"));
        assert!(!same_session_id("batch-01", "batch-02"));
    }
}
