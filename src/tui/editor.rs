// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The file a typed field goes to the person's editor in (spec section
//! 7.7, ADR 74): the field's text on top, then a cut line, and below it
//! what the field belongs to, as `#` lines. What stands above the cut
//! line comes back into the field when the editor exits; what stands
//! below it is ignored, as in a git commit message.

use super::text_field::cleaned;

/// The line that parts the field's text from what asqr writes below it.
pub const SCISSORS: &str = "# ------------------------ >8 ------------------------";

/// The line under the cut line that says what it is for.
const CUT_EXPLAINED: &str =
    "# Do not change or remove the line above. Everything below it is ignored.";

/// What `ctrl-g` asks for: the field's text, and what it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRequest {
    pub text: String,
    pub about: FieldAbout,
}

/// The session, question and field a text being edited belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldAbout {
    /// The session's title, if it has one.
    pub title: Option<String>,
    pub id: String,
    /// The question, for the fields of a question; the review's fields
    /// belong to the whole session.
    pub question: Option<QuestionAbout>,
    pub field: FieldName,
}

/// The question a field belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionAbout {
    /// Counted from 1, of `count` questions.
    pub number: usize,
    pub count: usize,
    /// The question's `header`, or its id.
    pub label: String,
    pub text: String,
}

/// Which typed field a text belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldName {
    OwnAnswer,
    Answer,
    Note,
    SessionNote,
    RejectReason,
}

impl FieldName {
    fn name(self) -> &'static str {
        match self {
            FieldName::OwnAnswer => "own answer",
            FieldName::Answer => "answer",
            FieldName::Note => "note",
            FieldName::SessionNote => "note to the asker",
            FieldName::RejectReason => "reason for rejecting",
        }
    }
}

/// The file for `request`. Every field gets the same layout, an empty one
/// included: its text, a blank line, the cut line. What the session wrote
/// is cleaned as a paste is, so it brings no control character into the
/// person's editor, and a value of one line stays on its line.
pub fn editor_file(request: &EditRequest) -> String {
    let about = &request.about;
    let one_line = |value: &str| cleaned(value).replace('\n', " ");
    let mut file = format!("{}\n\n{SCISSORS}\n{CUT_EXPLAINED}\n#\n", request.text);
    file.push_str(&match &about.title {
        Some(title) => format!("# Session: {} ({})\n", one_line(title), one_line(&about.id)),
        None => format!("# Session: {}\n", one_line(&about.id)),
    });
    if let Some(question) = &about.question {
        file.push_str(&format!(
            "# Question {} of {}: {}\n",
            question.number,
            question.count,
            one_line(&question.label)
        ));
    }
    file.push_str(&format!("# Field: {}\n", about.field.name()));
    if let Some(question) = &about.question {
        file.push_str("#\n");
        for line in cleaned(&question.text).split('\n') {
            if line.is_empty() {
                file.push_str("#\n");
            } else {
                file.push_str(&format!("# {line}\n"));
            }
        }
    }
    file
}

/// The text above the cut line of `file`, as the editor left it, without
/// the line breaks at its end, among them the blank line asqr wrote; `None`
/// where the cut line is gone. The first cut line counts. An editor that
/// writes `\r\n` or `\r` gets its lines read like `\n` ones.
pub fn edited_text(file: &str) -> Option<String> {
    let file = file.replace("\r\n", "\n").replace('\r', "\n");
    let cut = file.split('\n').position(|line| line == SCISSORS)?;
    let kept: Vec<&str> = file.split('\n').take(cut).collect();
    Some(kept.join("\n").trim_end_matches('\n').to_owned())
}

/// Whether `text` holds a line that is the cut line, which would cut the
/// text short when it comes back from the editor.
pub fn holds_scissors(text: &str) -> bool {
    text.split('\n').any(|line| line == SCISSORS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own_answer(text: &str, title: Option<&str>) -> EditRequest {
        EditRequest {
            text: text.to_owned(),
            about: FieldAbout {
                title: title.map(str::to_owned),
                id: "01K7Z0".to_owned(),
                question: Some(QuestionAbout {
                    number: 2,
                    count: 3,
                    label: "Changelog".to_owned(),
                    text: "Which entries belong in the changelog?\n\nList them.".to_owned(),
                }),
                field: FieldName::OwnAnswer,
            },
        }
    }

    #[test]
    fn the_file_holds_the_text_then_the_cut_line_and_what_it_belongs_to() {
        assert_eq!(
            editor_file(&own_answer(
                "Fix the docs.\nAnd the tests.",
                Some("Release 0.2.0")
            )),
            "Fix the docs.\nAnd the tests.\n\
             \n\
             # ------------------------ >8 ------------------------\n\
             # Do not change or remove the line above. Everything below it is ignored.\n\
             #\n\
             # Session: Release 0.2.0 (01K7Z0)\n\
             # Question 2 of 3: Changelog\n\
             # Field: own answer\n\
             #\n\
             # Which entries belong in the changelog?\n\
             #\n\
             # List them.\n"
        );
    }

    #[test]
    fn a_session_without_a_title_is_named_by_its_id() {
        let file = editor_file(&own_answer("x", None));
        assert!(file.contains("\n# Session: 01K7Z0\n"), "{file}");
    }

    #[test]
    fn the_review_s_fields_name_no_question() {
        let request = EditRequest {
            text: "Ship it.".to_owned(),
            about: FieldAbout {
                title: Some("Release 0.2.0".to_owned()),
                id: "01K7Z0".to_owned(),
                question: None,
                field: FieldName::SessionNote,
            },
        };

        assert!(
            editor_file(&request)
                .ends_with("#\n# Session: Release 0.2.0 (01K7Z0)\n# Field: note to the asker\n"),
            "{}",
            editor_file(&request)
        );
    }

    #[test]
    fn every_field_is_named_by_what_it_holds() {
        let named = |field| {
            let mut request = own_answer("x", None);
            request.about.field = field;
            editor_file(&request)
        };
        for (field, name) in [
            (FieldName::OwnAnswer, "own answer"),
            (FieldName::Answer, "answer"),
            (FieldName::Note, "note"),
            (FieldName::SessionNote, "note to the asker"),
            (FieldName::RejectReason, "reason for rejecting"),
        ] {
            assert!(
                named(field).contains(&format!("\n# Field: {name}\n")),
                "{field:?}"
            );
        }
    }

    #[test]
    fn an_empty_field_gives_the_same_layout() {
        assert!(
            editor_file(&own_answer("", None)).starts_with(&format!("\n\n{SCISSORS}\n")),
            "{}",
            editor_file(&own_answer("", None))
        );
    }

    #[test]
    fn control_characters_of_the_session_stay_out_of_the_file() {
        let mut request = own_answer("x", Some("Release\x1b]0;evil\x07 0.2.0\nnext"));
        if let Some(question) = &mut request.about.question {
            question.text = "Pick\x1b[31m one\tnow".to_owned();
        }

        let file = editor_file(&request);

        assert!(!file.contains('\x1b') && !file.contains('\x07'), "{file:?}");
        assert!(
            file.contains("# Session: Release]0;evil 0.2.0 next (01K7Z0)\n"),
            "{file:?}"
        );
        assert!(file.contains("# Pick[31m one    now\n"), "{file:?}");
    }

    #[test]
    fn the_text_above_the_cut_line_comes_back_without_its_trailing_line_breaks() {
        let file = editor_file(&own_answer("Fix the docs.\n", None));

        assert_eq!(edited_text(&file).as_deref(), Some("Fix the docs."));
    }

    #[test]
    fn hash_lines_above_the_cut_line_stay() {
        let file = format!("# Heading\n\n- one\n\n{SCISSORS}\n# ignored\n");

        assert_eq!(edited_text(&file).as_deref(), Some("# Heading\n\n- one"));
    }

    #[test]
    fn a_crlf_file_is_read_like_an_lf_one() {
        let file = format!("one\r\ntwo\r\n\r\n{SCISSORS}\r\n# ignored\r\n");

        assert_eq!(edited_text(&file).as_deref(), Some("one\ntwo"));
    }

    #[test]
    fn a_file_without_the_cut_line_gives_nothing() {
        assert_eq!(edited_text("Fix the docs.\n# Session: x\n"), None);
    }

    #[test]
    fn a_changed_cut_line_counts_as_gone() {
        assert_eq!(edited_text("text\n# ---- >8 ----\n"), None);
        assert_eq!(edited_text(&format!("text\n {SCISSORS}\n")), None);
    }

    #[test]
    fn the_first_cut_line_counts() {
        let file = format!("above\n{SCISSORS}\nbelow\n{SCISSORS}\n");

        assert_eq!(edited_text(&file).as_deref(), Some("above"));
    }

    #[test]
    fn a_text_that_holds_the_cut_line_is_known() {
        assert!(holds_scissors(&format!("pasted\n{SCISSORS}\nmore")));
        assert!(holds_scissors(SCISSORS));
        assert!(!holds_scissors(&format!("pasted {SCISSORS}")));
        assert!(!holds_scissors("plain text"));
    }
}
