// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where a session names a file to show. Reading the files and recording
//! them walk the same places in the same order, so the snapshot taken
//! before the result and the recording written after it line up by
//! position.

use serde_json::Value;

use super::ShownKind;

/// The objects of the raw session that hold a `path`, with what the
/// viewers make of the file: the `image`, `code`, `markdown` and `ansi`
/// entries of every question and option, and both sides of a `diff`. An
/// entry with inline `content` has no `path` and is not listed. The raw
/// JSON is edited, not the typed session, so unknown fields survive.
pub(crate) fn places_mut(raw: &mut Value) -> Vec<(ShownKind, &mut Value)> {
    let mut found = Vec::new();
    if let Some(questions) = raw.get_mut("questions").and_then(Value::as_array_mut) {
        for question in questions {
            collect_holder(question, true, &mut found);
        }
    }
    found
}

/// The path-bearing places in the `context` of a question or an option,
/// and for a question, in its options.
fn collect_holder<'a>(
    holder: &'a mut Value,
    with_options: bool,
    found: &mut Vec<(ShownKind, &'a mut Value)>,
) {
    let Some(fields) = holder.as_object_mut() else {
        return;
    };
    // One pass over the fields, as two `get_mut` calls on one object would
    // not let the borrow checker see that the fields are apart.
    for (name, value) in fields.iter_mut() {
        let Some(items) = value.as_array_mut() else {
            continue;
        };
        match name.as_str() {
            "context" => items
                .iter_mut()
                .for_each(|entry| collect_entry(entry, found)),
            "options" if with_options => items
                .iter_mut()
                .for_each(|option| collect_holder(option, false, found)),
            _ => {}
        }
    }
}

fn collect_entry<'a>(entry: &'a mut Value, found: &mut Vec<(ShownKind, &'a mut Value)>) {
    let kind = match entry.get("type").and_then(Value::as_str) {
        Some("image") => ShownKind::Image,
        Some("code") => ShownKind::Code,
        Some("markdown") => ShownKind::Markdown,
        Some("ansi") => ShownKind::Ansi,
        Some("diff") => {
            for (name, side) in entry.as_object_mut().into_iter().flatten() {
                if matches!(name.as_str(), "old" | "new") && has_path(side) {
                    found.push((ShownKind::DiffSide, side));
                }
            }
            return;
        }
        _ => return,
    };
    if has_path(entry) {
        found.push((kind, entry));
    }
}

fn has_path(object: &Value) -> bool {
    object.get("path").is_some_and(Value::is_string)
}
