// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr history` (spec section 6.5): the finished sessions of a queue,
//! newest first, each with the reference that `asqr replay` takes.

use std::io::IsTerminal;
use std::path::Path;
use std::time::{Duration, SystemTime};

use jiff::tz::TimeZone;
use serde_json::{Value, json};

use super::Exit;
use super::plain::plain_line;
use crate::queue::{
    FileCounts, History, HistoryEntry, HistoryOutcome, QueueLocation, list_history, list_queues,
};

/// How many finished sessions `history` lists per queue without `--limit`.
pub(super) const DEFAULT_LIMIT: usize = 10;

/// Parses `--limit`: a whole number of at least 1, since a list that shows
/// nothing is no list.
pub(super) fn parse_limit(input: &str) -> Result<usize, String> {
    match input.parse::<usize>() {
        Ok(0) => Err("the limit must be at least 1".to_owned()),
        Ok(limit) => Ok(limit),
        Err(_) => Err("expected a whole number, such as 10".to_owned()),
    }
}

pub(super) struct HistoryOptions {
    pub all: bool,
    pub since: Option<Duration>,
    pub limit: usize,
    pub json: bool,
}

/// One queue's list with the name it is shown under.
struct Block<'a> {
    queue: &'a str,
    history: &'a History,
}

/// How a session's end is worded in the STATUS column. A star marks a
/// withdrawn session whose unsent draft the result carries.
fn status_word(entry: &HistoryEntry) -> String {
    match (entry.outcome, entry.has_draft) {
        (HistoryOutcome::Sent, _) => "sent".to_owned(),
        (HistoryOutcome::Rejected, _) => "rejected".to_owned(),
        (HistoryOutcome::Error, _) => "error".to_owned(),
        (HistoryOutcome::Withdrawn, false) => "withdrawn".to_owned(),
        (HistoryOutcome::Withdrawn, true) => "withdrawn*".to_owned(),
        (HistoryOutcome::NoResult, _) => "no result".to_owned(),
    }
}

/// The word `--json` uses for a session's end.
fn status_json(outcome: HistoryOutcome) -> &'static str {
    match outcome {
        HistoryOutcome::Sent => "sent",
        HistoryOutcome::Rejected => "rejected",
        HistoryOutcome::Error => "error",
        HistoryOutcome::Withdrawn => "withdrawn",
        HistoryOutcome::NoResult => "none",
    }
}

/// The FILES column: what the session recorded about the files it showed.
/// A file that is gone is the news worth the room, then the files that
/// were not kept, then the kept ones.
fn files_text(files: FileCounts) -> String {
    let total = files.kept + files.gone + files.not_kept;
    if total == 0 {
        "-".to_owned()
    } else if files.gone > 0 {
        format!("{} of {total} gone", files.gone)
    } else if files.not_kept > 0 && files.kept == 0 {
        format!("{} not kept", files.not_kept)
    } else if files.not_kept > 0 {
        format!("{} of {total} not kept", files.not_kept)
    } else {
        format!("{} kept", files.kept)
    }
}

/// `age` in the largest unit that holds it exactly, in the units `--since`
/// takes.
fn age_text(age: Duration) -> String {
    let seconds = age.as_secs();
    let (unit, size) = [("w", 604_800), ("d", 86_400), ("h", 3_600), ("m", 60)]
        .into_iter()
        .find(|(_, size)| seconds != 0 && seconds.is_multiple_of(*size))
        .unwrap_or(("s", 1));
    format!("{}{unit}", seconds / size)
}

const FOOTNOTE: &str = "* withdrawn by the asker; the answers you had entered are kept as a draft";

/// The text `history` prints for `blocks`. Titles are cut to `width`
/// columns when it is given. `open_hint` is the command that opens an
/// entry.
fn render(
    blocks: &[Block<'_>],
    since: Option<Duration>,
    width: Option<usize>,
    zone: &TimeZone,
    open_hint: &str,
) -> String {
    let mut out = String::new();
    for (position, block) in blocks.iter().enumerate() {
        if position > 0 {
            out.push('\n');
        }
        let list = block.history;
        let shown = list.entries.len();
        let count = if shown < list.total {
            format!("{shown} of {}", list.total)
        } else {
            shown.to_string()
        };
        let within = since.map_or_else(String::new, |age| format!(" in {}", age_text(age)));
        out.push_str(&format!(
            "queue: {}\nfinished ({count}{within}):\n",
            block.queue
        ));
        if list.entries.is_empty() {
            continue;
        }

        let rows: Vec<[String; 4]> = list
            .entries
            .iter()
            .map(|entry| {
                [
                    entry.entry.reference(),
                    when_text(entry, zone),
                    status_word(entry),
                    files_text(entry.files),
                ]
            })
            .collect();
        let headers = ["REF", "WHEN", "STATUS", "FILES"];
        let widths: Vec<usize> = (0..4)
            .map(|column| {
                rows.iter()
                    .map(|row| row[column].chars().count())
                    .chain([headers[column].len()])
                    .max()
                    .unwrap_or_default()
            })
            .collect();
        let before_title: usize = widths.iter().map(|width| width + 2).sum();
        let line = |cells: &[&str; 4], title: &str| {
            let mut line = String::new();
            for (cell, width) in cells.iter().zip(&widths) {
                line.push_str(&format!("{cell:<width$}  "));
            }
            line.push_str(title);
            line.push('\n');
            line
        };
        out.push_str(&line(&headers, "TITLE"));
        for (row, entry) in rows.iter().zip(&list.entries) {
            let title = plain_line(entry.title.as_deref().unwrap_or(&entry.entry.id));
            let title = match width {
                Some(width) => cut(&title, width.saturating_sub(before_title).max(1)),
                None => title,
            };
            out.push_str(&line(&[&row[0], &row[1], &row[2], &row[3]], &title));
        }
    }

    let any = blocks.iter().any(|block| !block.history.entries.is_empty());
    if blocks
        .iter()
        .flat_map(|block| &block.history.entries)
        .any(|entry| entry.has_draft)
    {
        out.push_str(&format!("\n{FOOTNOTE}\n"));
    }
    if any {
        out.push_str(&format!("\nOpen one with: {open_hint}\n"));
    }
    out
}

/// `title` cut to `room` columns, the last of them `…` when it was cut.
fn cut(title: &str, room: usize) -> String {
    if title.chars().count() <= room {
        return title.to_owned();
    }
    let kept: String = title.chars().take(room.saturating_sub(1)).collect();
    format!("{kept}…")
}

fn when_text(entry: &HistoryEntry, zone: &TimeZone) -> String {
    jiff::Timestamp::try_from(entry.finished)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(zone.clone())
        .strftime("%d %b %H:%M")
        .to_string()
}

/// The list of one queue as JSON.
fn json_of(block: &Block<'_>) -> Value {
    let entries: Vec<Value> = block
        .history
        .entries
        .iter()
        .map(|entry| {
            // Whole seconds, as the results write their times.
            let finished = jiff::Timestamp::try_from(entry.finished)
                .ok()
                .and_then(|time| jiff::Timestamp::from_second(time.as_second()).ok());
            json!({
                "ref": entry.entry.reference(),
                "id": entry.entry.id,
                "finished_at": finished.map(|time| time.to_string()),
                "status": status_json(entry.outcome),
                "files": {
                    "kept": entry.files.kept,
                    "gone": entry.files.gone,
                    "not_kept": entry.files.not_kept,
                },
                "title": entry.title,
                "draft": entry.has_draft,
            })
        })
        .collect();
    json!({
        "queue": block.queue,
        "total": block.history.total,
        "entries": entries,
    })
}

/// The columns of the terminal behind stdout, when stdout is one.
fn terminal_width() -> Option<usize> {
    let stdout = std::io::stdout();
    if !stdout.is_terminal() {
        return None;
    }
    let size = rustix::termios::tcgetwinsize(&stdout).ok()?;
    Some(usize::from(size.ws_col)).filter(|columns| *columns > 0)
}

pub(super) fn run(location: &QueueLocation, root: Option<&Path>, options: HistoryOptions) -> Exit {
    // The queue in use comes first, then the others by name.
    let mut queues = vec![location.clone()];
    if options.all {
        let Some(root) = root else {
            eprintln!("error: no platform data directory found, so there are no named queues");
            return Exit::Failure;
        };
        match list_queues(root) {
            Ok(summaries) => queues.extend(
                summaries
                    .into_iter()
                    .map(|summary| summary.location)
                    .filter(|queue| !queue.same_queue(location)),
            ),
            Err(error) => {
                eprintln!("error: cannot read the queues: {error}");
                return Exit::Failure;
            }
        }
    }

    let cutoff = options.since.map(|age| {
        SystemTime::now()
            .checked_sub(age)
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    let mut lists = Vec::new();
    for queue in &queues {
        let name = queue.name().map_or_else(
            || {
                std::path::absolute(queue.dir())
                    .unwrap_or_else(|_| queue.dir().to_path_buf())
                    .display()
                    .to_string()
            },
            str::to_owned,
        );
        match list_history(queue, Some(options.limit), cutoff) {
            Ok(history) => lists.push((name, history)),
            Err(error) => {
                eprintln!("error: cannot read the queue {name}: {error}");
                return Exit::Failure;
            }
        }
    }
    for (_, history) in &lists {
        for error in &history.unreadable {
            eprintln!("warning: {error}");
        }
    }
    let blocks: Vec<Block<'_>> = lists
        .iter()
        .map(|(queue, history)| Block { queue, history })
        .collect();

    if options.json {
        // One shape for a reader to handle, whether one queue is listed
        // or all of them.
        let printed = Value::Array(blocks.iter().map(json_of).collect());
        println!("{printed}");
        return Exit::Success;
    }

    // A queue outside the data directory is only found by `replay` when
    // it is named.
    let open_hint = if location.root().is_none() {
        let dir =
            std::path::absolute(location.dir()).unwrap_or_else(|_| location.dir().to_path_buf());
        format!(
            "asqr --dir {} replay <REF>",
            super::shell_word(&dir, "<dir>")
        )
    } else {
        "asqr replay <REF>".to_owned()
    };
    print!(
        "{}",
        render(
            &blocks,
            options.since,
            terminal_width(),
            &TimeZone::system(),
            &open_hint
        )
    );
    Exit::Success
}

#[cfg(test)]
mod tests;
