// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::path::PathBuf;
use std::time::SystemTime;

use ulid::Ulid;

use super::*;
use crate::queue::{EntryPlace, EntryRef};

fn at(text: &str) -> SystemTime {
    SystemTime::from(text.parse::<jiff::Timestamp>().expect("a timestamp"))
}

/// An entry finished at `when` (UTC) in the queue `/q`.
fn entry(
    id: &str,
    random: u128,
    when: &str,
    title: Option<&str>,
    outcome: HistoryOutcome,
    files: FileCounts,
) -> HistoryEntry {
    let finished = at(when);
    let millis = finished
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after the epoch")
        .as_millis() as u64;
    HistoryEntry {
        entry: EntryRef {
            queue: QueueLocation::at("/q"),
            id: id.to_owned(),
            ulid: Ulid::from_parts(millis, random),
            place: EntryPlace::Loose(PathBuf::from("/q/archive/x.json")),
        },
        title: title.map(str::to_owned),
        finished,
        outcome,
        files,
        has_draft: false,
    }
}

fn kept(count: usize) -> FileCounts {
    FileCounts {
        kept: count,
        ..FileCounts::default()
    }
}

fn history(entries: Vec<HistoryEntry>) -> History {
    let total = entries.len();
    History {
        entries,
        total,
        unreadable: Vec::new(),
    }
}

const HINT: &str = "asqr replay <REF>";

fn drawn(blocks: &[Block<'_>], since: Option<Duration>, width: Option<usize>) -> String {
    render(blocks, since, width, &TimeZone::UTC, HINT)
}

#[test]
fn a_queue_is_drawn_as_a_table_with_the_hint_to_open_one() {
    let first = entry(
        "hero-2",
        0x01,
        "2026-10-29T16:41:10Z",
        Some("README hero image"),
        HistoryOutcome::Sent,
        kept(3),
    );
    let mut second = entry(
        "migrate",
        0x02,
        "2026-10-29T11:20:00Z",
        Some("Migration plan"),
        HistoryOutcome::Withdrawn,
        FileCounts {
            kept: 1,
            gone: 1,
            not_kept: 0,
        },
    );
    second.has_draft = true;
    let list = history(vec![first.clone(), second.clone()]);

    let text = drawn(
        &[Block {
            queue: "default",
            history: &list,
        }],
        None,
        None,
    );

    let (one, two) = (first.entry.reference(), second.entry.reference());
    assert_eq!(
        text,
        format!(
            "queue: default\n\
             finished (2):\n\
             REF       WHEN          STATUS      FILES        TITLE\n\
             {one}  29 Oct 16:41  sent        3 kept       README hero image\n\
             {two}  29 Oct 11:20  withdrawn*  1 of 2 gone  Migration plan\n\
             \n\
             * withdrawn by the asker; the answers you had entered are kept as a draft\n\
             \n\
             Open one with: asqr replay <REF>\n"
        )
    );
}

#[test]
fn every_queue_of_all_gets_its_block() {
    let one = entry(
        "a",
        0x01,
        "2026-10-30T10:02:00Z",
        Some("First"),
        HistoryOutcome::Sent,
        FileCounts::default(),
    );
    let two = entry(
        "b",
        0x02,
        "2026-10-29T14:55:00Z",
        Some("Second"),
        HistoryOutcome::Rejected,
        FileCounts::default(),
    );
    let (first, second) = (history(vec![one]), history(vec![two]));

    let text = drawn(
        &[
            Block {
                queue: "default",
                history: &first,
            },
            Block {
                queue: "asqr-todos",
                history: &second,
            },
        ],
        None,
        None,
    );

    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "queue: default");
    assert_eq!(lines[5], "queue: asqr-todos");
    assert!(text.ends_with("\nOpen one with: asqr replay <REF>\n"));
    assert_eq!(text.matches("Open one with").count(), 1);
}

#[test]
fn the_header_says_how_many_of_how_many_and_since_when() {
    let entries = vec![
        entry(
            "a",
            1,
            "2026-10-30T10:02:00Z",
            Some("A"),
            HistoryOutcome::Sent,
            FileCounts::default(),
        ),
        entry(
            "b",
            2,
            "2026-10-29T10:02:00Z",
            Some("B"),
            HistoryOutcome::Sent,
            FileCounts::default(),
        ),
    ];
    let cut = History {
        entries: entries.clone(),
        total: 5,
        unreadable: Vec::new(),
    };
    let whole = history(entries);
    let day = Duration::from_secs(3 * 24 * 60 * 60);
    let header = |list: &History, since| {
        drawn(
            &[Block {
                queue: "q",
                history: list,
            }],
            since,
            None,
        )
        .lines()
        .nth(1)
        .expect("a header line")
        .to_owned()
    };

    assert_eq!(header(&cut, None), "finished (2 of 5):");
    assert_eq!(header(&whole, None), "finished (2):");
    assert_eq!(header(&whole, Some(day)), "finished (2 in 3d):");
    assert_eq!(header(&cut, Some(day)), "finished (2 of 5 in 3d):");
}

#[test]
fn an_empty_queue_has_a_header_and_nothing_else() {
    let list = history(Vec::new());

    let text = drawn(
        &[Block {
            queue: "default",
            history: &list,
        }],
        None,
        None,
    );

    assert_eq!(text, "queue: default\nfinished (0):\n");
}

#[test]
fn a_title_is_cut_to_the_width_and_without_one_it_is_not() {
    let long = "A title that runs far beyond what the terminal has room for";
    let list = history(vec![entry(
        "a",
        1,
        "2026-10-30T10:02:00Z",
        Some(long),
        HistoryOutcome::Sent,
        FileCounts::default(),
    )]);
    let blocks = [Block {
        queue: "q",
        history: &list,
    }];
    let row = |width| {
        drawn(&blocks, None, width)
            .lines()
            .nth(3)
            .expect("a row")
            .to_owned()
    };

    let cut = row(Some(60));

    assert_eq!(cut.chars().count(), 60);
    assert!(cut.ends_with('…'));
    assert!(row(None).ends_with(long));
}

#[test]
fn a_session_without_a_title_shows_its_id() {
    let list = history(vec![entry(
        "batch-01",
        1,
        "2026-10-30T10:02:00Z",
        None,
        HistoryOutcome::NoResult,
        FileCounts::default(),
    )]);

    let text = drawn(
        &[Block {
            queue: "q",
            history: &list,
        }],
        None,
        None,
    );

    assert!(
        text.lines()
            .nth(3)
            .expect("a row")
            .ends_with("no result  -      batch-01"),
        "{text}"
    );
}

#[test]
fn the_files_column_names_what_the_session_recorded() {
    assert_eq!(files_text(FileCounts::default()), "-");
    assert_eq!(files_text(kept(3)), "3 kept");
    assert_eq!(
        files_text(FileCounts {
            kept: 1,
            gone: 1,
            not_kept: 0
        }),
        "1 of 2 gone"
    );
    assert_eq!(
        files_text(FileCounts {
            kept: 0,
            gone: 0,
            not_kept: 2
        }),
        "2 not kept"
    );
    assert_eq!(
        files_text(FileCounts {
            kept: 1,
            gone: 0,
            not_kept: 1
        }),
        "1 of 2 not kept"
    );
}

#[test]
fn the_status_words_are_the_ones_the_list_prints() {
    let word = |outcome, draft| {
        let mut entry = entry(
            "a",
            1,
            "2026-10-30T10:02:00Z",
            None,
            outcome,
            FileCounts::default(),
        );
        entry.has_draft = draft;
        status_word(&entry)
    };

    assert_eq!(word(HistoryOutcome::Sent, false), "sent");
    assert_eq!(word(HistoryOutcome::Rejected, false), "rejected");
    assert_eq!(word(HistoryOutcome::Error, false), "error");
    assert_eq!(word(HistoryOutcome::Withdrawn, false), "withdrawn");
    assert_eq!(word(HistoryOutcome::Withdrawn, true), "withdrawn*");
    assert_eq!(word(HistoryOutcome::NoResult, false), "no result");
}

#[test]
fn an_age_is_written_in_the_largest_unit_that_holds_it() {
    let seconds = Duration::from_secs;
    assert_eq!(age_text(seconds(3 * 86_400)), "3d");
    assert_eq!(age_text(seconds(14 * 86_400)), "2w");
    assert_eq!(age_text(seconds(5 * 3_600)), "5h");
    assert_eq!(age_text(seconds(90 * 60)), "90m");
    assert_eq!(age_text(seconds(45)), "45s");
}

#[test]
fn json_has_one_object_per_queue_with_its_entries() {
    let mut withdrawn = entry(
        "migrate",
        2,
        "2026-10-29T11:20:00Z",
        Some("Migration plan"),
        HistoryOutcome::Withdrawn,
        FileCounts {
            kept: 1,
            gone: 1,
            not_kept: 0,
        },
    );
    withdrawn.has_draft = true;
    let sent = entry(
        "hero-2",
        1,
        "2026-10-29T16:41:10Z",
        None,
        HistoryOutcome::Sent,
        FileCounts::default(),
    );
    let (sent_ref, withdrawn_ref) = (sent.entry.reference(), withdrawn.entry.reference());
    let list = History {
        entries: vec![sent, withdrawn],
        total: 7,
        unreadable: Vec::new(),
    };

    let value = json_of(&Block {
        queue: "default",
        history: &list,
    });

    assert_eq!(
        value,
        json!({
            "queue": "default",
            "total": 7,
            "entries": [
                {"ref": sent_ref, "id": "hero-2", "finished_at": "2026-10-29T16:41:10Z",
                 "status": "sent", "files": {"kept": 0, "gone": 0, "not_kept": 0},
                 "title": null, "draft": false},
                {"ref": withdrawn_ref, "id": "migrate", "finished_at": "2026-10-29T11:20:00Z",
                 "status": "withdrawn", "files": {"kept": 1, "gone": 1, "not_kept": 0},
                 "title": "Migration plan", "draft": true},
            ]
        })
    );
}

#[test]
fn json_words_for_every_way_a_session_can_end() {
    assert_eq!(status_json(HistoryOutcome::Sent), "sent");
    assert_eq!(status_json(HistoryOutcome::Rejected), "rejected");
    assert_eq!(status_json(HistoryOutcome::Error), "error");
    assert_eq!(status_json(HistoryOutcome::Withdrawn), "withdrawn");
    assert_eq!(status_json(HistoryOutcome::NoResult), "none");
}

#[test]
fn a_title_that_fits_is_left_alone() {
    assert_eq!(cut("short", 5), "short");
    assert_eq!(cut("shorter", 5), "shor…");
}

#[test]
fn a_limit_is_a_whole_number_of_at_least_one() {
    assert_eq!(parse_limit("3"), Ok(3));
    assert_eq!(
        parse_limit("0"),
        Err("the limit must be at least 1".to_owned())
    );
    assert_eq!(
        parse_limit("many"),
        Err("expected a whole number, such as 10".to_owned())
    );
}
