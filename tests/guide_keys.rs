// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The key table of the guide lists the keys of the specification's key
//! table (section 7.6), row for row.
//!
//! Only the keys are compared. What a key does is written in the guide's
//! own words, so a changed action in the specification does not fail this
//! test; a key that is added, removed, renamed or moved does.

use std::fs;
use std::path::Path;

/// The heading above the specification's key table.
const SPEC_HEADING: &str = "### 7.6 Keys";
/// The heading above the key table in the guide's Keys note.
const GUIDE_HEADING: &str = "## Keys on a question";
/// The title of the guide's Keys note, which is how the test finds it: the
/// file name carries an id that is not known in advance.
const GUIDE_TITLE: &str = "Keys";

/// The key cells, first column, of the first table under `heading`.
///
/// The table's header and separator rows are not keys and are skipped.
fn key_cells(markdown: &str, heading: &str) -> Result<Vec<String>, String> {
    let mut lines = markdown.lines();
    if !lines.any(|line| line.trim_end() == heading) {
        return Err(format!("no heading `{heading}`"));
    }

    let mut table = lines
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .peekable();
    if table.peek().is_none() {
        return Err(format!("no table with rows under `{heading}`"));
    }

    Ok(table
        .map(|row| {
            row.trim_start_matches('|')
                .split('|')
                .next()
                .expect("split yields at least one piece")
                .trim()
                .to_string()
        })
        .collect())
}

/// Says which row of the guide differs from the specification, or nothing
/// when the two tables list the same keys in the same order.
fn compare(spec: &[String], guide: &[String]) -> Result<(), String> {
    if let Some((row, (wanted, found))) = spec
        .iter()
        .zip(guide)
        .enumerate()
        .find(|(_, (wanted, found))| wanted != found)
    {
        return Err(format!(
            "row {} of the guide's key table is \"{found}\", the specification's is \"{wanted}\"",
            row + 1
        ));
    }
    if spec.len() != guide.len() {
        return Err(format!(
            "the guide's key table has {} rows, the specification's has {}",
            guide.len(),
            spec.len()
        ));
    }
    Ok(())
}

/// The text of the one note of the guide whose title is `GUIDE_TITLE`.
fn guide_keys_note(root: &Path) -> Result<String, String> {
    let notes = root.join("docs/guide/all-notes");
    let mut found = Vec::new();
    for entry in fs::read_dir(&notes).map_err(|e| format!("reading {}: {e}", notes.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let has_title = text
            .lines()
            .skip(1)
            .take_while(|line| *line != "---")
            .any(|line| {
                line.strip_prefix("title:")
                    .is_some_and(|title| title.trim().trim_matches('"') == GUIDE_TITLE)
            });
        if has_title {
            found.push(text);
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!(
            "no note titled `{GUIDE_TITLE}` in {}",
            notes.display()
        )),
        n => Err(format!(
            "{n} notes titled `{GUIDE_TITLE}` in {}",
            notes.display()
        )),
    }
}

#[test]
fn the_guide_lists_the_keys_of_the_specification() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let spec = fs::read_to_string(root.join("docs/spec.md")).expect("the specification exists");
    let spec_keys = key_cells(&spec, SPEC_HEADING).expect("the specification has its key table");

    let note = guide_keys_note(root).unwrap_or_else(|error| panic!("{error}"));
    let guide_keys = key_cells(&note, GUIDE_HEADING).unwrap_or_else(|error| panic!("{error}"));

    compare(&spec_keys, &guide_keys).unwrap_or_else(|error| panic!("{error}"));
}

fn cells(table: &str) -> Vec<String> {
    key_cells(&format!("## T\n\n{table}"), "## T").expect("the table parses")
}

#[test]
fn a_table_gives_the_first_column_of_each_row_without_its_header() {
    let table = "| Key | Action |\n|---|---|\n| `a`, `b` | one |\n| `c` | two |\n\ntext\n";

    assert_eq!(cells(table), ["`a`, `b`", "`c`"]);
}

#[test]
fn only_the_first_table_under_the_heading_counts() {
    let table = "| K | A |\n|---|---|\n| `a` | x |\n\n| K | A |\n|---|---|\n| `z` | y |\n";

    assert_eq!(cells(table), ["`a`"]);
}

#[test]
fn a_missing_heading_or_table_is_named() {
    assert_eq!(
        key_cells("text", "## T"),
        Err("no heading `## T`".to_string())
    );
    assert_eq!(
        key_cells("## T\n\nno table\n", "## T"),
        Err("no table with rows under `## T`".to_string())
    );
}

#[test]
fn a_changed_key_is_reported_with_both_spellings_and_its_row() {
    let spec = cells("| K | A |\n|---|---|\n| `a` | x |\n| `b` | y |\n");
    let guide = cells("| K | A |\n|---|---|\n| `a` | x |\n| `c` | y |\n");

    assert_eq!(
        compare(&spec, &guide),
        Err(
            "row 2 of the guide's key table is \"`c`\", the specification's is \"`b`\"".to_string()
        )
    );
}

#[test]
fn a_missing_or_extra_row_is_reported() {
    let spec = cells("| K | A |\n|---|---|\n| `a` | x |\n| `b` | y |\n");
    let guide = cells("| K | A |\n|---|---|\n| `a` | x |\n");

    assert_eq!(
        compare(&spec, &guide),
        Err("the guide's key table has 1 rows, the specification's has 2".to_string())
    );
    assert_eq!(compare(&spec, &spec), Ok(()));
}
