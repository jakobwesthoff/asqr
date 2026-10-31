// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a replay says about a file the session showed (ADR 101): the card
//! in place of a file it cannot show, the words in the pane's bottom rule
//! and the frame `d` opens.

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::overlay::{overlay_block, popup};
use super::problem::{Card, Severity};
use super::theme;
use crate::queue::ReplayFile;
use crate::tui::{ReplayInfo, human_size, modified_text, short_sha};

/// The card that stands in place of a file the replay cannot show: one
/// the archive could not read, one it did not keep and one whose copy is
/// gone or damaged. `None` for a kept file, which shows as itself.
pub(super) fn card_for(file: &ReplayFile, info: &ReplayInfo) -> Option<Card> {
    if let Some(damaged) = &file.damaged {
        let mut headline = damaged.clone();
        if let Some(first) = headline.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        return Some(Card::told(
            Severity::Error,
            headline,
            vec![format!("original {}", file.original_path)],
        ));
    }
    let reason = |default: &str| file.reason.clone().unwrap_or_else(|| default.to_owned());
    match file.state.as_str() {
        "missing" => Some(Card::told(
            Severity::Error,
            "Missing when the session finished",
            vec![
                file.original_path.clone(),
                "asqr could not read it: no copy, no fingerprint".to_owned(),
            ],
        )),
        "unreadable" => Some(Card::told(
            Severity::Error,
            "Could not be read when the session finished",
            vec![
                file.original_path.clone(),
                format!(
                    "asqr could not read it: {}",
                    reason("no copy, no fingerprint")
                ),
            ],
        )),
        "not_kept" => {
            let name = Path::new(&file.original_path).file_name().map_or_else(
                || file.original_path.clone(),
                |name| name.to_string_lossy().into_owned(),
            );
            let mut facts = vec![name];
            facts.extend(file.size.map(human_size));
            facts.extend(
                file.modified
                    .as_deref()
                    .map(|time| format!("modified {}", modified_text(time, &info.zone))),
            );
            let mut lines = vec![facts.join(" · ")];
            if let Some(sha) = &file.sha256 {
                lines.push(format!(
                    "sha256 {} · {}",
                    short_sha(sha),
                    file.original_path
                ));
            } else {
                lines.push(file.original_path.clone());
            }
            lines.extend(file.reason.clone());
            Some(Card::told(
                Severity::Warning,
                "Not kept, only the fingerprint was recorded",
                lines,
            ))
        }
        _ => None,
    }
}

/// The words the pane's bottom rule carries for `file`.
pub(super) fn bottom_words(file: &ReplayFile) -> String {
    let what = if file.damaged.is_some() {
        "damaged copy".to_owned()
    } else {
        match file.state.as_str() {
            "kept" => match file.kept_size {
                Some(size) => format!("kept copy · {}", human_size(size)),
                None => "kept copy".to_owned(),
            },
            "not_kept" => "fingerprint only".to_owned(),
            _ => "no copy".to_owned(),
        }
    };
    format!("{what} · d details")
}

/// The frame `d` opens over the screen: where the copy lies, where the
/// original lay, its size, modification time and SHA-256, and the SHA-256
/// of the copy. Any key closes it.
pub(super) fn draw_details(area: Rect, buf: &mut Buffer, file: &ReplayFile, info: &ReplayInfo) {
    let name = Path::new(&file.original_path).file_name().map_or_else(
        || file.original_path.clone(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut rows: Vec<(&str, String)> = Vec::new();
    if let Some(copy) = &file.copy {
        rows.push(("Kept copy", copy.clone()));
        rows.push((
            "Stored in",
            info.bundle
                .clone()
                .unwrap_or_else(|| "the archive's files folder".to_owned()),
        ));
    }
    rows.push(("Original", file.original_path.clone()));
    if let Some(size) = file.size {
        rows.push((
            "Size",
            match file.kept_size {
                Some(kept) => format!("{size} bytes (kept {kept} bytes)"),
                None => format!("{size} bytes"),
            },
        ));
    }
    if let Some(time) = &file.modified {
        rows.push(("Modified", modified_text(time, &info.zone)));
    }
    if let Some(sha) = &file.sha256 {
        rows.push(("SHA-256", sha.clone()));
    }
    let copy_hash = file
        .copy
        .as_deref()
        .and_then(|copy| copy.strip_prefix("files/"))
        .and_then(|name| name.split_once('.'))
        .map(|(hash, _)| hash.to_owned());
    if let Some(hash) = copy_hash {
        rows.push(("Copy SHA-256", hash));
    }
    if let Some(damaged) = &file.damaged {
        rows.push(("Problem", damaged.clone()));
    }
    if let Some(reason) = &file.reason {
        rows.push(("Reason", reason.clone()));
    }

    let label = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or_default();
    let lines: Vec<Line<'static>> = rows
        .into_iter()
        .map(|(row, value)| {
            Line::from(vec![
                Span::styled(format!("{row:<label$}  "), theme::secondary()),
                Span::from(value),
            ])
        })
        .collect();
    // The frame's inner width is its own less the border and the padding;
    // a value longer than the rest of its row wraps onto a second row.
    let width = 100.min(area.width);
    let inner = usize::from(width.saturating_sub(4)).max(1);
    let rows_taken: usize = lines
        .iter()
        .map(|line| line.width().div_ceil(inner).max(1))
        .sum();
    let height = u16::try_from(rows_taken + 2).unwrap_or(u16::MAX);
    let area = popup(area, width, height.min(area.height), buf);
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(overlay_block(&name, Some("any key closes")))
        .render(area, buf);
}
