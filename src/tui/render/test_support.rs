// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What the render tests read off a drawn buffer: its rows as text, where a
//! text starts and the style it starts in. The tests draw into buffers whose
//! area starts at the origin, so a cell's column and row index the buffer
//! directly. Tests of lines not yet drawn read their text here too. Also
//! the paths that every loader of a file must refuse.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

use super::source_line::columns;
use super::viewer::Viewer;
use crate::tui::Offset;

// ---------------------------------------------------------------------
// Drawing a viewer
// ---------------------------------------------------------------------

/// `viewer` laid out for `width`×`height` cells and drawn into them from
/// `offset` on, as the context pane draws it into its body.
pub(super) fn drawn(viewer: &mut dyn Viewer, width: u16, height: u16, offset: Offset) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    viewer.lay_out(width, height);
    viewer.draw(area, &mut buf, offset);
    buf
}

/// The whole content of `viewer`, every line of it, `width` columns wide
/// from column `left` on, each row in quotes so its width shows.
pub(super) fn shown(viewer: &mut dyn Viewer, width: u16, left: usize) -> String {
    let size = viewer.lay_out(width, 1);
    let height = u16::try_from(size.lines).expect("a test's content is short");
    rows(&drawn(viewer, width, height, Offset { top: 0, left }))
        .iter()
        .map(|row| format!("\"{row}\""))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------
// Reading the text
// ---------------------------------------------------------------------

/// Each row of `buf` as the terminal shows it, blanks after the text
/// included.
pub(super) fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| shown_row(buf, y)).collect()
}

/// The text of row `y` as the terminal shows it, without the blanks after
/// it.
pub(super) fn row(buf: &Buffer, y: u16) -> String {
    shown_row(buf, y).trim_end().to_owned()
}

/// Row `y` of `buf` as the terminal shows it. A wide character takes two
/// cells, and the terminal shows nothing of the cell it covers, which the
/// buffer keeps as a blank.
fn shown_row(buf: &Buffer, y: u16) -> String {
    let mut row = String::new();
    let mut x = 0;
    while x < buf.area.width {
        let symbol = buf[(x, y)].symbol();
        row.push_str(symbol);
        x += u16::try_from(columns(symbol).max(1)).expect("a cell is narrow");
    }
    row
}

/// The text of each line, spans joined.
pub(super) fn line_texts(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------
// Finding a text
// ---------------------------------------------------------------------

/// The column and row of the cell where `text` first starts in `buf`.
pub(super) fn find(buf: &Buffer, text: &str) -> (u16, u16) {
    find_from(buf, 0, text)
}

/// The column and row of the cell where `text` first starts in `buf`,
/// searching the rows from `from` on.
pub(super) fn find_from(buf: &Buffer, from: u16, text: &str) -> (u16, u16) {
    for y in from..buf.area.height {
        // The row's symbols one after another, with the byte each cell's
        // symbol starts at, so a match in the text leads back to its cell
        // whatever the symbols' lengths in bytes.
        let mut line = String::new();
        let mut starts = Vec::new();
        for x in 0..buf.area.width {
            starts.push((line.len(), x));
            line.push_str(buf[(x, y)].symbol());
        }
        if let Some(start) = line.find(text) {
            let (_, x) = starts
                .iter()
                .rev()
                .find(|(at, _)| *at <= start)
                .expect("the first cell starts at byte 0");
            return (*x, y);
        }
    }
    panic!("{text:?} is not in the buffer:\n{}", rows(buf).join("\n"));
}

/// The style of the cell where `text` first starts in `buf`.
pub(super) fn style_at(buf: &Buffer, text: &str) -> Style {
    buf[find(buf, text)].style()
}

// ---------------------------------------------------------------------
// Paths no viewer opens
// ---------------------------------------------------------------------

/// One path for each reason the rule for opening a file refuses it: a
/// relative path, a missing file and a directory. The directory lives as
/// long as the returned one. A FIFO is left out, as a loader that opened
/// it would hang instead of failing; the rule's own tests cover it.
pub(super) fn refused_paths() -> (tempfile::TempDir, [String; 3]) {
    let directory = tempfile::tempdir().expect("temporary directory is creatable");
    let inside = directory.path().to_str().expect("a UTF-8 path").to_owned();
    let paths = [
        "pictures/cat.png".to_owned(),
        "/nowhere/cat.png".to_owned(),
        inside,
    ];
    (directory, paths)
}
