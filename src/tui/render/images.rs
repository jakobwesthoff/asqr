// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Images of context entries (spec section 7.7). The graphics protocol is
//! detected once, in the binary, by querying the terminal; the image
//! viewers receive the resulting [`Picker`], so tests draw with the
//! halfblock protocol and never talk to a terminal. A path that is
//! relative, missing, unreadable or not a regular file shows a placeholder
//! and never becomes an error.

use std::fs;
#[cfg(unix)]
use std::io::{self, Write};
use std::path::Path;
#[cfg(unix)]
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, StatefulWidget, Widget, Wrap};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

/// One image file, decoded and encoded for the terminal's graphics
/// protocol, or the reason it cannot be shown.
pub enum Image {
    Ready(Box<StatefulProtocol>),
    Unavailable(String),
}

impl Image {
    /// Reads and decodes the image at `path` for `picker`'s protocol.
    pub fn load(picker: &Picker, path: &str) -> Image {
        // A relative path depends on where asqr was started, so it is
        // never resolved (spec section 7.7).
        if Path::new(path).is_relative() {
            return Image::Unavailable("relative path".to_owned());
        }
        // Opening a FIFO waits for a writer, so only a regular file is
        // opened at all, as for the text viewers.
        match fs::metadata(path) {
            Err(error) => return Image::Unavailable(error.to_string()),
            Ok(metadata) if !metadata.is_file() => {
                return Image::Unavailable("not a regular file".to_owned());
            }
            Ok(_) => {}
        }
        match image::ImageReader::open(path).and_then(|reader| reader.with_guessed_format()) {
            Err(error) => Image::Unavailable(error.to_string()),
            Ok(reader) => match reader.decode() {
                Ok(decoded) => Image::Ready(Box::new(picker.new_resize_protocol(decoded))),
                Err(error) => Image::Unavailable(error.to_string()),
            },
        }
    }

    /// Draws the image into `area` of `buf`, or a placeholder that names
    /// `path` and the reason it is not shown. With `enlarge`, the image is
    /// scaled to fill the area as far as its proportions allow; without, it
    /// keeps its natural size and only shrinks when it does not fit.
    pub fn draw(&mut self, path: &str, area: Rect, buf: &mut Buffer, enlarge: bool) {
        match self {
            Image::Ready(protocol) => {
                let resize = if enlarge {
                    Resize::Scale(None)
                } else {
                    Resize::Fit(None)
                };
                StatefulImage::default()
                    .resize(resize)
                    .render(area, buf, protocol.as_mut());
                // Works around images cropped by a changed cell size; see
                // the function for when it can go.
                size_kitty_placement(buf, area);
            }
            Image::Unavailable(reason) => {
                let text = vec![
                    Line::from("image not shown").dim(),
                    Line::from(path.to_owned()),
                    Line::from(reason.clone()).dim(),
                ];
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .render(area, buf);
            }
        }
    }
}

/// The character the Kitty protocol reserves for image placeholders.
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

/// The start of the Kitty command that transmits an image and creates its
/// virtual placement, as `ratatui-image` 11.1.0 writes it.
const KITTY_PLACEMENT: &str = "a=T,U=1,";

/// Makes the Kitty placement drawn into `area` span exactly the cells its
/// placeholders cover, by adding `c=` and `r=` to its command. This is a
/// workaround for a gap in `ratatui-image` 11.1.0 and is meant to go away:
/// upstream fixed it in pull request
/// <https://github.com/ratatui/ratatui-image/pull/215> (commit `2d7d7f6`,
/// merged on 2026-10-03), which no release contains yet. The newest one
/// then was 12.0.0-rc.0, which predates the merge.
///
/// # The problem
///
/// With the Kitty protocol, `ratatui-image` sends the image once, with a
/// command that also creates a "virtual placement" (`a=T,U=1`), and then
/// fills the image's area with placeholder characters (`U+10EEEE`) that
/// tell the terminal which part of the image goes into which cell. The
/// image is resized beforehand to the area's cells times the cell size in
/// pixels that asqr asked the terminal for once, at start.
///
/// The placement command names the image's pixel size but not its size in
/// cells. The terminal then works the cells out itself, from the pixels
/// and the cell size it has right now. As long as both cell sizes agree,
/// that gives exactly the placeholder cells. When they differ, it does
/// not:
///
/// - after a font size change while asqr runs, as asqr never asks again;
/// - in tmux with several clients attached whose cell sizes differ, as
///   only one of them can match the cell size asqr got.
///
/// Smaller cells than expected crop the image to its top-left part, larger
/// ones leave it short of its area. Both were seen in Ghostty, with and
/// without tmux, on 2026-10-03.
///
/// # The workaround
///
/// `c=` and `r=` name the placement's size in cells, and the terminal then
/// scales the image into exactly those cells, whatever its own cell size.
/// `ratatui-image` writes the command into the symbol of the first
/// placeholder cell of the frame that sends the image. So after the widget
/// has drawn, this looks for that cell inside `area`, counts the
/// placeholder columns and rows around it, and inserts `c=` and `r=` right
/// after `a=T,U=1,`. Inside tmux the command is wrapped for passthrough,
/// which doubles the escape characters but leaves this plain-text part
/// alone. Frames that send nothing, and every other protocol, have no such
/// cell, and the buffer stays as it is.
///
/// This depends on the exact text `ratatui-image` writes, which is why
/// `Cargo.toml` pins it to `=11.1.0`. A version bump has to check this
/// function, and the test
/// `a_kitty_image_is_placed_over_exactly_its_placeholder_cells` in
/// `screen.rs` fails when the command changes shape.
///
/// # Why not patch `ratatui-image`
///
/// The proper fix belongs in `ratatui-image` itself, which is what pull
/// request 215 does. Until a release carries it, only a git dependency or
/// a `[patch.crates-io]` entry would pull it in, and that works for local
/// builds only: `cargo publish` drops patches, and crates on crates.io
/// cannot depend on git sources. Releases on crates.io would have kept the
/// bug.
///
/// # Removing it
///
/// TODO: Remove this once a `ratatui-image` release contains PR 215 (see
/// the todo `01M4169ZBJ9P5QFR20761YE0TH-drop-the-kitty-placement-workaround-once-ratatui-image-has-the-fix.md`):
///
/// 1. Bump `ratatui-image` to that release and drop the `=` pin in
///    `Cargo.toml`. The release is likely a 12.x, which may change the
///    `Picker` API that `src/terminal.rs` and this module use.
/// 2. Delete this function, its two constants and the call in
///    [`Image::draw`].
/// 3. Run the test named above. It must stay green without this
///    function, as the crate then writes `c=` and `r=` itself.
fn size_kitty_placement(buffer: &mut Buffer, area: Rect) {
    let area = area.intersection(buffer.area);
    let mut command = None;
    let (mut columns, mut rows) = (0, 0);
    for y in area.top()..area.bottom() {
        let mut in_row = 0;
        for x in area.left()..area.right() {
            let symbol = buffer[(x, y)].symbol();
            if !symbol.contains(KITTY_PLACEHOLDER) {
                continue;
            }
            in_row += 1;
            if symbol.contains(KITTY_PLACEMENT) {
                command = Some((x, y));
            }
        }
        if in_row > 0 {
            columns = columns.max(in_row);
            rows += 1;
        }
    }
    let Some(position) = command else {
        return;
    };
    let cell = &mut buffer[position];
    let symbol = cell.symbol().replacen(
        KITTY_PLACEMENT,
        &format!("{KITTY_PLACEMENT}c={columns},r={rows},"),
        1,
    );
    cell.set_symbol(&symbol);
}

/// The environment variables that tell whether asqr runs inside tmux.
pub struct TerminalNames<'a> {
    /// `TMUX`, which tmux sets in its panes; empty counts as unset.
    pub tmux: Option<&'a str>,
    /// `TERM`.
    pub term: Option<&'a str>,
    /// `TERM_PROGRAM`.
    pub term_program: Option<&'a str>,
}

/// Whether the terminal may be asked for its graphics protocol.
/// `tmux_state` asks tmux for `#{allow-passthrough} #{session_attached}`:
/// the pane's passthrough option and the number of clients attached to the
/// session.
///
/// Inside tmux the query only reaches a terminal through passthrough, and
/// only while a client is attached. Without an answer the query's reader
/// goes on taking the keys the person types, so the query is skipped and
/// images fall back to half blocks (spec section 7.7).
///
/// A `TERM` that starts with `tmux`, or a `TERM_PROGRAM` of `tmux`,
/// without `TMUX` means a tmux that asqr cannot ask, as after ssh from a
/// tmux pane, which carries `TERM` along but not `TMUX`. ratatui-image
/// wraps its query for tmux on exactly this test. There `probe` finds out
/// whether a sequence sent through tmux's passthrough is answered, as
/// [`passthrough_answers`] does, and the terminal is queried only when it
/// is. A `screen` `TERM` gets the query unwrapped and without a probe.
pub fn may_query_protocol(
    names: &TerminalNames<'_>,
    tmux_state: impl FnOnce() -> Option<String>,
    probe: impl FnOnce() -> bool,
) -> bool {
    if names.tmux.is_none_or(str::is_empty) {
        let tmux_like = names.term.is_some_and(|term| term.starts_with("tmux"))
            || names.term_program == Some("tmux");
        return !tmux_like || probe();
    }
    let Some(state) = tmux_state() else {
        return false;
    };
    let mut words = state.split_whitespace();
    // `on` lets the pane pass sequences through, `all` any pane.
    let passthrough = matches!(words.next(), Some("on" | "all"));
    let attached = words
        .next()
        .and_then(|clients| clients.parse::<u32>().ok())
        .is_some_and(|clients| clients > 0);
    passthrough && attached
}

/// A device status report request, `ESC [5n`, wrapped for tmux's
/// passthrough, which doubles every escape inside the wrapper. The
/// terminal outside answers `ESC [0n` only when the pane passes the
/// sequence through to it.
#[cfg(unix)]
const PASSTHROUGH_PROBE: &[u8] = b"\x1bPtmux;\x1b\x1b[5n\x1b\\";

/// The answer to [`PASSTHROUGH_PROBE`]: the terminal is fine.
#[cfg(unix)]
const PROBE_ANSWER: &[u8] = b"\x1b[0n";

/// Writes [`PASSTHROUGH_PROBE`] to `out` and whether `input` brings the
/// answer within `patience`. It waits with `poll` and reads a byte at a
/// time only once one is there, so no read is left blocked when the answer
/// never comes, and the bytes after the answer stay for the next reader.
/// Bytes before the answer, such as keys typed in the meantime, are lost.
/// `input` must be in raw mode, or a terminal hands the answer over only
/// with a line.
#[cfg(unix)]
pub fn passthrough_answers(
    out: &mut impl Write,
    input: impl std::os::fd::AsFd,
    patience: Duration,
) -> io::Result<bool> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};

    out.write_all(PASSTHROUGH_PROBE)?;
    out.flush()?;
    let deadline = Instant::now() + patience;
    let mut seen = Vec::new();
    while !seen.ends_with(PROBE_ANSWER) {
        // Input that keeps coming, such as a paste, must not keep the
        // probe reading past its deadline.
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        let left = Timespec::try_from(left).map_err(io::Error::other)?;
        let mut fds = [PollFd::new(&input, PollFlags::IN)];
        match poll(&mut fds, Some(&left)) {
            Ok(0) => return Ok(false),
            Ok(_) => {}
            // A signal, such as a resize, ends the wait early; the
            // deadline still holds.
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        }
        let mut byte = [0];
        if rustix::io::read(&input, &mut byte)? == 0 {
            return Ok(false);
        }
        seen.push(byte[0]);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TMUX: Option<&str> = Some("/tmp/tmux-501/default,1234,0");

    /// The reason `path` is not shown, or `None` when it loads.
    fn refused(path: &str) -> Option<String> {
        match Image::load(&Picker::halfblocks(), path) {
            Image::Ready(_) => None,
            Image::Unavailable(reason) => Some(reason),
        }
    }

    #[test]
    fn a_directory_is_not_an_image() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");

        assert_eq!(
            refused(directory.path().to_str().expect("a UTF-8 path")).as_deref(),
            Some("not a regular file")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let fifo = directory.path().join("pipe.png");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        let path = fifo.to_str().expect("a UTF-8 path").to_owned();

        // Opening a FIFO for reading blocks until a writer comes, so the
        // load runs apart and a hang fails the test instead of stalling it.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sender
                .send(refused(&path))
                .expect("the test waits for the result");
        });
        let reason = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("loading a FIFO must not block");

        assert_eq!(reason.as_deref(), Some("not a regular file"));
    }

    #[test]
    fn a_missing_and_a_relative_file_say_why() {
        assert!(refused("/nowhere/cat.png").is_some_and(|reason| !reason.is_empty()));
        assert_eq!(refused("cat.png").as_deref(), Some("relative path"));
    }

    #[cfg(unix)]
    #[test]
    fn a_regular_file_that_may_not_be_read_says_why() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("secret.png");
        fs::write(&path, b"").expect("file is writable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))
            .expect("permissions are settable");

        let reason = refused(path.to_str().expect("a UTF-8 path")).expect("refused");

        assert!(
            reason.to_lowercase().contains("permission denied"),
            "{reason}"
        );
    }

    #[test]
    fn a_regular_image_file_loads() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("dot.png");
        image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]))
            .save(&path)
            .expect("image written");

        assert_eq!(refused(path.to_str().expect("a UTF-8 path")), None);
    }

    /// `TERM` and `TERM_PROGRAM` of a terminal that is not tmux.
    const PLAIN: (Option<&str>, Option<&str>) = (Some("xterm-ghostty"), Some("ghostty"));

    /// The decision where asqr asks tmux and never probes.
    fn may_query(
        tmux: Option<&str>,
        names: (Option<&str>, Option<&str>),
        tmux_state: impl FnOnce() -> Option<String>,
    ) -> bool {
        decided(tmux, names, tmux_state, || {
            unreachable!("no probe where tmux can be asked or is not there")
        })
    }

    /// The decision without `TMUX`, where the probe gets the `answered`
    /// it is given, if it runs.
    fn probed(names: (Option<&str>, Option<&str>), answered: bool) -> bool {
        decided(
            None,
            names,
            || unreachable!("tmux cannot be asked without TMUX"),
            || answered,
        )
    }

    fn decided(
        tmux: Option<&str>,
        (term, term_program): (Option<&str>, Option<&str>),
        tmux_state: impl FnOnce() -> Option<String>,
        probe: impl FnOnce() -> bool,
    ) -> bool {
        may_query_protocol(
            &TerminalNames {
                tmux,
                term,
                term_program,
            },
            tmux_state,
            probe,
        )
    }

    #[test]
    fn outside_tmux_the_terminal_is_asked() {
        assert!(may_query(None, PLAIN, || unreachable!(
            "not asked outside tmux"
        )));
        assert!(may_query(Some(""), PLAIN, || unreachable!(
            "an empty TMUX is unset"
        )));
        assert!(may_query(None, (None, None), || unreachable!(
            "not asked outside tmux"
        )));
    }

    /// `TERM` and `TERM_PROGRAM` of a tmux that asqr cannot ask without
    /// `TMUX`, as after ssh from a tmux pane, which carries `TERM` along but
    /// not `TMUX`.
    const MULTIPLEXED: [(Option<&str>, Option<&str>); 2] = [
        (Some("tmux-256color"), None),
        (Some("xterm-256color"), Some("tmux")),
    ];

    /// ratatui-image sends the query unwrapped there, and GNU screen, or a
    /// tmux reached over ssh with such a `TERM`, answers it, so a probe
    /// would only cost time and keys.
    #[test]
    fn a_screen_term_without_tmux_is_asked_without_a_probe() {
        for term in ["screen.xterm-256color", "screen-256color", "screen"] {
            assert!(
                may_query(None, (Some(term), None), || unreachable!(
                    "not asked without TMUX"
                )),
                "{term}"
            );
        }
    }

    #[test]
    fn a_tmux_without_its_variable_is_asked_once_the_probe_is_answered() {
        for names in MULTIPLEXED {
            assert!(probed(names, true), "{names:?}");
        }
        assert!(
            decided(Some(""), (Some("tmux"), None), || unreachable!(), || true),
            "an empty TMUX is unset"
        );
    }

    #[test]
    fn a_tmux_without_its_variable_is_not_asked_without_an_answer() {
        for names in MULTIPLEXED {
            assert!(!probed(names, false), "{names:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_probe_asks_through_tmux_passthrough_and_reads_only_up_to_the_answer() {
        use std::io::Read;

        let (mut input, mut terminal) = std::io::pipe().expect("a pipe");
        terminal
            .write_all(b"typed\x1b[0nafter")
            .expect("the pipe takes it");
        drop(terminal);
        let mut sent = Vec::new();

        let answered = passthrough_answers(&mut sent, &input, Duration::from_secs(5));

        assert!(answered.expect("the probe runs"));
        assert_eq!(sent, b"\x1bPtmux;\x1b\x1b[5n\x1b\\");
        let mut rest = String::new();
        input.read_to_string(&mut rest).expect("the pipe reads");
        assert_eq!(rest, "after", "what comes after the answer stays");
    }

    #[cfg(unix)]
    #[test]
    fn the_probe_gives_up_without_an_answer() {
        let (input, mut terminal) = std::io::pipe().expect("a pipe");
        terminal.write_all(b"typed").expect("the pipe takes it");

        let answered = passthrough_answers(&mut Vec::new(), &input, Duration::from_millis(50));
        assert!(!answered.expect("the probe runs"), "silence");

        drop(terminal);
        let answered = passthrough_answers(&mut Vec::new(), &input, Duration::from_secs(5));
        assert!(!answered.expect("the probe runs"), "the input ended");
    }

    #[cfg(unix)]
    #[test]
    fn the_probe_reads_nothing_past_its_deadline() {
        use std::io::Read;

        let (mut input, mut terminal) = std::io::pipe().expect("a pipe");
        terminal.write_all(b"\x1b[0n").expect("the pipe takes it");
        drop(terminal);

        let answered = passthrough_answers(&mut Vec::new(), &input, Duration::ZERO);

        assert!(!answered.expect("the probe runs"));
        let mut rest = Vec::new();
        input.read_to_end(&mut rest).expect("the pipe reads");
        assert_eq!(rest, b"\x1b[0n", "the answer stays unread");
    }

    #[test]
    fn inside_tmux_only_with_passthrough_and_a_client() {
        let tmux_term = (Some("tmux-256color"), Some("tmux"));
        assert!(may_query(TMUX, tmux_term, || Some("on 1\n".into())));
        assert!(may_query(TMUX, tmux_term, || Some("all 2".into())));
        assert!(!may_query(TMUX, tmux_term, || Some("off 1\n".into())));
        assert!(
            !may_query(TMUX, tmux_term, || Some("on 0\n".into())),
            "detached"
        );
        assert!(
            may_query(TMUX, PLAIN, || Some("on 1\n".into())),
            "TMUX decides, whatever TERM says"
        );
    }

    #[test]
    fn inside_tmux_anything_unexpected_counts_as_no() {
        assert!(!may_query(TMUX, PLAIN, || None));
        assert!(!may_query(TMUX, PLAIN, || Some(String::new())));
        assert!(!may_query(TMUX, PLAIN, || Some("on".into())));
        assert!(!may_query(TMUX, PLAIN, || Some("on many".into())));
    }
}
