// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Images of context entries (spec section 7.7). The graphics protocol is
//! detected once, in the binary, by querying the terminal; the image
//! viewers receive the resulting [`Picker`], so tests draw with the
//! halfblock protocol and never talk to a terminal. A path that is
//! relative, missing, unreadable or not a regular file, or a file that does
//! not decode, shows a placeholder and never becomes an error.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::hash::{BuildHasher, RandomState};
#[cfg(unix)]
use std::io::{self, Write};
use std::rc::Rc;
#[cfg(unix)]
use std::time::{Duration, Instant};

use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;
use ratatui_image::picker::cap_parser::Parser;
use ratatui_image::picker::{Capability, Picker, ProtocolType};
use ratatui_image::protocol::kitty::StatefulKitty;
use ratatui_image::protocol::{StatefulProtocol, StatefulProtocolType};
use ratatui_image::{Resize, StatefulImage};

use super::source_file::{Unreadable, regular_file};

/// One image file, decoded and encoded for the terminal's graphics
/// protocol.
pub struct Image {
    protocol: Box<StatefulProtocol>,
    /// The id the Kitty protocol sends the image under, `None` under any
    /// other protocol. It is kept for what its drop does: the image's
    /// holder decides when the image goes, and with it the id.
    _kitty_id: Option<HeldId>,
}

impl Image {
    /// Reads and decodes the image at `path` for `encoder`'s protocol, or
    /// says why it cannot be shown. A path is opened by the rule every
    /// viewer of a file follows.
    pub fn load(encoder: &Encoder, path: &str) -> Result<Image, Unreadable> {
        let unreadable = |reason: String| Unreadable {
            path: path.to_owned(),
            reason,
        };
        regular_file(path)?;
        let decoded = image::ImageReader::open(path)
            .and_then(|reader| reader.with_guessed_format())
            .map_err(|error| unreadable(error.to_string()))?
            .decode()
            .map_err(|error| unreadable(error.to_string()))?;
        Ok(encoder.encode(decoded))
    }

    /// Encodes a picture that is decoded already for `picker`'s protocol,
    /// as ratatui-image makes it.
    pub fn from_picture(picker: &Picker, picture: DynamicImage) -> Image {
        Image {
            protocol: Box::new(picker.new_resize_protocol(picture)),
            _kitty_id: None,
        }
    }

    /// Draws the image into `area` of `buf`. With `enlarge`, the image is
    /// scaled to fill the area as far as its proportions allow; without, it
    /// keeps its natural size and only shrinks when it does not fit.
    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, enlarge: bool) {
        let resize = if enlarge {
            Resize::Scale(None)
        } else {
            Resize::Fit(None)
        };
        StatefulImage::default()
            .resize(resize)
            .render(area, buf, self.protocol.as_mut());
    }
}

// ---------------------------------------------------------------------
// Kitty image ids
// ---------------------------------------------------------------------

/// Makes the [`Image`]s of context entries for the terminal's graphics
/// protocol, as `picker` speaks it. Its clones share one book of the Kitty
/// image ids handed out in this run.
#[derive(Clone)]
pub struct Encoder {
    picker: Picker,
    kitty_ids: Rc<RefCell<KittyIds>>,
}

impl Encoder {
    /// An encoder for `picker`'s protocol, with a book of its own.
    pub fn new(picker: Picker) -> Self {
        Encoder {
            picker,
            kitty_ids: Rc::new(RefCell::new(KittyIds::new(random_ids()))),
        }
    }

    /// An encoder whose Kitty ids come from `ids` in turn, so a test knows
    /// them.
    #[cfg(test)]
    pub(super) fn with_ids(picker: Picker, ids: Vec<u32>) -> Self {
        let mut ids = ids.into_iter();
        Encoder {
            picker,
            kitty_ids: Rc::new(RefCell::new(KittyIds::new(Box::new(move || {
                ids.next().expect("the test gave enough ids")
            })))),
        }
    }

    /// The Kitty ids of the images that went since the last call, for the
    /// terminal to delete: a context image drops its id when its viewer
    /// goes, on regaining the focus, when its session leaves or when its
    /// entry is replaced (ADR 42).
    pub fn take_dropped(&self) -> Vec<u32> {
        std::mem::take(&mut self.kitty_ids.borrow_mut().dropped)
    }

    /// The Kitty ids of every image the terminal may still keep, each
    /// once: those that went since [`Encoder::take_dropped`] last took
    /// them, then those that exist. asqr deletes them all before it leaves
    /// the alternate screen, as the images stay there after it (ADR 42).
    pub fn take_all(&self) -> Vec<u32> {
        let mut book = self.kitty_ids.borrow_mut();
        let mut all = std::mem::take(&mut book.dropped);
        all.extend(std::mem::take(&mut book.held));
        all
    }

    /// The Kitty ids of the images that exist.
    #[cfg(test)]
    pub(super) fn held(&self) -> Vec<u32> {
        self.kitty_ids.borrow().held.iter().copied().collect()
    }

    /// Encodes `picture` for the picker's protocol.
    ///
    /// Under the Kitty protocol asqr builds the protocol itself, so it
    /// knows the image's id. It passes what
    /// `Picker::new_resize_protocol` passes (ratatui-image 12.0.0-rc.2
    /// `picker.rs` l.301-320) apart from the id and shared memory: the
    /// tmux wrapping, and compression only where the terminal said it
    /// inflates `o=z`, as a terminal that cannot refuses the
    /// transmission. Every other protocol comes from the picker as it is.
    ///
    /// Shared memory stays off. The picker only reports it when the query
    /// asked for it, it works only with a terminal on the same machine,
    /// and every transmission leaves an object behind for the terminal
    /// to unlink.
    pub fn encode(&self, picture: DynamicImage) -> Image {
        let picker = &self.picker;
        if picker.protocol_type() != ProtocolType::Kitty {
            return Image::from_picture(picker, picture);
        }
        let held = HeldId::issued(&self.kitty_ids);
        let kitty = StatefulKitty::new(
            held.id,
            picker.tmux_detected(),
            picker
                .capabilities()
                .contains(&Capability::KittyCompression),
            false,
        );
        // The picker's background colour, which asqr never sets, so it is
        // none, and pictures are padded with transparent pixels.
        let protocol = StatefulProtocol::new(
            picture,
            picker.font_size(),
            None,
            StatefulProtocolType::Kitty(kitty),
        );
        Image {
            protocol: Box::new(protocol),
            _kitty_id: Some(held),
        }
    }
}

/// Where Kitty ids come from: 32 random bits per call. `RandomState` is
/// seeded from the operating system once per thread, so two asqr
/// processes, which may share one terminal through tmux, draw different
/// ids.
fn random_ids() -> Box<dyn FnMut() -> u32> {
    let random = RandomState::new();
    let mut drawn: u64 = 0;
    Box::new(move || {
        drawn += 1;
        let hash = random.hash_one(drawn);
        u32::try_from(hash >> 32).expect("32 bits are left of 64 shifted by 32")
    })
}

/// The book of the Kitty image ids of one run.
struct KittyIds {
    source: Box<dyn FnMut() -> u32>,
    /// Every id handed out in this run. An id is never handed out twice:
    /// a delete for an id written late, or written again, would remove
    /// the image that took it over.
    issued: HashSet<u32>,
    /// The ids of the images that exist.
    held: BTreeSet<u32>,
    /// The ids of the images that went since [`Encoder::take_dropped`]
    /// last took them.
    dropped: Vec<u32>,
}

impl KittyIds {
    fn new(source: Box<dyn FnMut() -> u32>) -> Self {
        KittyIds {
            source,
            issued: HashSet::new(),
            held: BTreeSet::new(),
            dropped: Vec::new(),
        }
    }

    /// A new id, drawn again while it is 0, which names no image in the
    /// Kitty protocol, or one handed out before.
    fn issue(&mut self) -> u32 {
        loop {
            let id = (self.source)();
            if id != 0 && self.issued.insert(id) {
                self.held.insert(id);
                return id;
            }
        }
    }

    /// Notes that the image with `id` went, for the terminal to delete.
    /// Every way an image goes ends here, through the drop of its
    /// [`HeldId`].
    fn release(&mut self, id: u32) {
        if self.held.remove(&id) {
            self.dropped.push(id);
        }
    }
}

/// The Kitty id of one [`Image`], handed back to its book when the image
/// goes.
struct HeldId {
    id: u32,
    book: Rc<RefCell<KittyIds>>,
}

impl HeldId {
    fn issued(book: &Rc<RefCell<KittyIds>>) -> Self {
        HeldId {
            id: book.borrow_mut().issue(),
            book: Rc::clone(book),
        }
    }
}

impl Drop for HeldId {
    fn drop(&mut self) {
        self.book.borrow_mut().release(self.id);
    }
}

/// The Kitty commands that delete the images `ids` names, wrapped for
/// tmux's passthrough when `tmux` says asqr runs inside tmux, as
/// ratatui-image wraps the transmissions (ADR 42).
///
/// `d=I` with an id and no placement id removes every placement of the
/// image, the virtual one its placeholders refer to included, and frees
/// its data. `q=2` keeps the terminal from answering into asqr's input. A
/// delete of an id the terminal does not know does nothing.
pub fn kitty_deletes(ids: &[u32], tmux: bool) -> Vec<u8> {
    let (start, escape, end) = Parser::tmux_start_escape_end(tmux);
    ids.iter()
        .map(|id| format!("{start}{escape}_Ga=d,d=I,i={id},q=2{escape}\\{end}"))
        .collect::<String>()
        .into_bytes()
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

/// What asqr asks tmux about its pane once at start, for
/// `tmux display-message -p`: the pane's passthrough option, the number of
/// clients attached to the session, and the client terminal's answer to
/// XTVERSION, such as `iTerm2 3.6.7`. That answer holds blanks, so tabs
/// keep the three apart.
pub const TMUX_STATE_FORMAT: &str = "#{allow-passthrough}\t#{session_attached}\t#{client_termtype}";

/// The fields of what tmux printed for [`TMUX_STATE_FORMAT`], without the
/// line break it ends in.
pub(super) fn tmux_state_fields(state: &str) -> std::str::Split<'_, char> {
    state.trim_end_matches('\n').split('\t')
}

/// Whether the terminal may be asked for its graphics protocol.
/// `tmux_state` asks tmux for [`TMUX_STATE_FORMAT`], of which this needs
/// the pane's passthrough option and the number of clients attached to the
/// session.
///
/// Inside tmux the query only reaches a terminal through passthrough, and
/// only while a client is attached. A query that gets no answer holds up
/// the start for its whole wait, and an answer later than that may reach
/// asqr as typed keys ([`answers_may_follow_the_query`]), so where none
/// can come the query is skipped and images fall back to half blocks
/// (spec section 7.7).
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
    let mut fields = tmux_state_fields(&state);
    // `on` lets the pane pass sequences through, `all` any pane.
    let passthrough = matches!(fields.next(), Some("on" | "all"));
    let attached = fields
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

/// Whether answers to the graphics query may still be on their way once
/// ratatui-image is done reading, so [`discard_late_answers`] has to run
/// before keys are read. ratatui-image reads up to the first status
/// report or its deadline; whatever comes after it reaches the key reader,
/// which takes a Kitty answer such as `ESC _Gi=31;OK ESC \` for keys, and
/// its digits pick options.
///
/// `found_capabilities` is whether the picker found any capability. A
/// picker without one got no usable answer, which every query that ran
/// into its deadline is, and its terminal may still answer. Inside tmux,
/// `tmux_state` is what tmux printed for [`TMUX_STATE_FORMAT`]: with more
/// than one client attached, each client's terminal answers the query
/// tmux passed on, and only the first answer was read. A lone terminal
/// that answered sends nothing more, and its start costs no wait.
pub fn answers_may_follow_the_query(found_capabilities: bool, tmux_state: Option<&str>) -> bool {
    let clients = tmux_state
        .and_then(|state| tmux_state_fields(state).nth(1))
        .and_then(|clients| clients.parse::<u32>().ok())
        .unwrap_or(0);
    !found_capabilities || clients > 1
}

/// Reads and discards what `input` brings until `quiet` passes without a
/// byte, or `cap` passes in all, and returns how many bytes it discarded.
/// It waits with `poll`, so it never blocks in a read, and reads only what
/// is there. A key typed while it runs is discarded with the answers.
#[cfg(unix)]
pub fn discard_late_answers(
    input: impl std::os::fd::AsFd,
    quiet: Duration,
    cap: Duration,
) -> io::Result<usize> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};

    let deadline = Instant::now() + cap;
    let mut discarded = 0;
    loop {
        let wait = quiet.min(deadline.saturating_duration_since(Instant::now()));
        if wait.is_zero() {
            return Ok(discarded);
        }
        let wait = Timespec::try_from(wait).map_err(io::Error::other)?;
        let mut fds = [PollFd::new(&input, PollFlags::IN)];
        match poll(&mut fds, Some(&wait)) {
            Ok(0) => return Ok(discarded),
            Ok(_) => {}
            // A signal, such as a resize, ends the wait early and starts
            // the quiet time again; the cap still holds.
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        }
        let mut chunk = [0; 256];
        match rustix::io::read(&input, &mut chunk)? {
            0 => return Ok(discarded),
            read => discarded += read,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::super::test_support;
    use super::*;

    const TMUX: Option<&str> = Some("/tmp/tmux-501/default,1234,0");

    /// Why the file at `path` is not shown, or `None` when it loads.
    fn refused(path: &str) -> Option<Unreadable> {
        Image::load(&Encoder::new(Picker::halfblocks()), path).err()
    }

    #[test]
    fn an_image_is_refused_by_the_path_rule() {
        let (_directory, paths) = test_support::refused_paths();

        for path in paths {
            assert_eq!(refused(&path), regular_file(&path).err(), "{path}");
        }
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
        let path = path.to_str().expect("a UTF-8 path");

        let unreadable = refused(path).expect("refused");

        assert_eq!(unreadable.path, path);
        assert!(
            unreadable
                .reason
                .to_lowercase()
                .contains("permission denied"),
            "{}",
            unreadable.reason
        );
    }

    #[test]
    fn a_file_that_is_no_image_says_why() {
        let directory = tempfile::tempdir().expect("temporary directory is creatable");
        let path = directory.path().join("notes.png");
        fs::write(&path, b"no picture in here").expect("file is writable");
        let path = path.to_str().expect("a UTF-8 path");

        let unreadable = refused(path).expect("refused");

        assert_eq!(unreadable.path, path);
        assert!(!unreadable.reason.is_empty());
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

    // -----------------------------------------------------------------
    // Kitty image ids
    // -----------------------------------------------------------------

    /// A picker that speaks the Kitty protocol, with cells of 10×20 pixels.
    fn kitty() -> Picker {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        picker
    }

    /// The character the Kitty protocol reserves for image placeholders.
    const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

    /// A red picture of 20×40 pixels, which takes 2×2 cells of 10×20.
    fn red() -> DynamicImage {
        image::RgbaImage::from_pixel(20, 40, image::Rgba([255, 0, 0, 255])).into()
    }

    /// The command that sends the image in `buf`, up to the placeholder
    /// that follows it, and the colours of the placeholder cells.
    fn sent(buf: &Buffer) -> (&str, Vec<ratatui::style::Color>) {
        let placeholders: Vec<_> = buf
            .content()
            .iter()
            .filter(|cell| cell.symbol().contains(KITTY_PLACEHOLDER))
            .collect();
        let command = placeholders
            .iter()
            .map(|cell| cell.symbol())
            .find(|symbol| symbol.contains("\x1b_G"))
            .expect("the first draw sends the image");
        let command = &command[..command.find(KITTY_PLACEHOLDER).expect("a placeholder")];
        (command, placeholders.iter().map(|cell| cell.fg).collect())
    }

    #[test]
    fn a_kitty_id_is_never_0_and_never_handed_out_twice() {
        let mut drawn = vec![0, 7, 7, 9, 0, 7, 11].into_iter();
        let mut ids = KittyIds::new(Box::new(move || drawn.next().expect("enough ids")));

        assert_eq!([ids.issue(), ids.issue(), ids.issue()], [7, 9, 11]);
    }

    #[test]
    fn random_kitty_ids_differ() {
        let mut random = random_ids();
        let drawn: HashSet<u32> = (0..1000).map(|_| random()).collect();

        assert!(drawn.len() > 990, "{} of 1000 differ", drawn.len());
    }

    /// The protocol names an image by its id in the transmission and in
    /// the foreground colour of every placeholder, the id's low three
    /// bytes as red, green and blue (ratatui-image 12.0.0-rc.2
    /// `protocol/kitty.rs` l.132-143).
    #[test]
    fn a_kitty_image_is_sent_under_the_id_its_encoder_issued() {
        let encoder = Encoder::with_ids(kitty(), vec![0x00AB_CDEF]);
        let mut image = encoder.encode(red());
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));

        image.draw(Rect::new(0, 0, 2, 2), &mut buf, false);

        let (command, colours) = sent(&buf);
        assert!(command.contains(",i=11259375,"), "{command:?}");
        assert_eq!(
            colours,
            [ratatui::style::Color::Rgb(0xAB, 0xCD, 0xEF); 4],
            "one per cell"
        );
    }

    /// asqr builds the Kitty protocol as ratatui-image's picker does, so
    /// the transmission is the same but for the id. A `Picker` takes the
    /// compression capability only from the terminal's answer to its
    /// query, so this compares the transmission without compression.
    #[test]
    fn asqr_s_kitty_protocol_transmits_what_ratatui_image_s_does_but_the_id() {
        let picker = kitty();
        let area = Rect::new(0, 0, 2, 2);
        let without_id = |protocol: &mut StatefulProtocol| {
            let mut buf = Buffer::empty(area);
            StatefulImage::default()
                .resize(Resize::Fit(None))
                .render(area, &mut buf, protocol);
            let (command, _) = sent(&buf);
            let id = command.find("i=").expect("the command names its image");
            let after = id + command[id..].find(',').expect("more keys follow the id");
            format!("{}{}", &command[..id], &command[after + 1..])
        };

        let mut ours = Encoder::new(picker.clone()).encode(red());
        let mut theirs = picker.new_resize_protocol(red());

        assert_eq!(without_id(&mut ours.protocol), without_id(&mut theirs));
    }

    #[test]
    fn a_delete_names_its_image_and_asks_for_no_answer() {
        assert_eq!(
            kitty_deletes(&[7, 4_000_000_000], false),
            b"\x1b_Ga=d,d=I,i=7,q=2\x1b\\\x1b_Ga=d,d=I,i=4000000000,q=2\x1b\\"
        );
        assert_eq!(kitty_deletes(&[], false), b"");
    }

    /// Inside tmux, each delete is wrapped for passthrough as the
    /// transmissions are: the escapes inside doubled.
    #[test]
    fn a_delete_inside_tmux_goes_through_the_passthrough() {
        assert_eq!(
            kitty_deletes(&[7], true),
            b"\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=7,q=2\x1b\x1b\\\x1b\\"
        );
    }

    /// Every id the book handed out and has not handed on as deleted, and
    /// those it has, once, for deleting all before asqr quits.
    #[test]
    fn at_the_end_every_image_is_handed_on_once() {
        let encoder = Encoder::with_ids(kitty(), vec![3, 1, 2]);
        let kept = [encoder.encode(red()), encoder.encode(red())];
        drop(encoder.encode(red()));

        assert_eq!(encoder.take_all(), [2, 1, 3]);
        drop(kept);
        assert_eq!(encoder.take_all(), Vec::<u32>::new(), "handed on once");
        assert_eq!(encoder.take_dropped(), Vec::<u32>::new());
    }

    /// Every other protocol comes from the picker, and the encoder hands
    /// out no id for it.
    #[test]
    fn other_protocols_take_no_kitty_id() {
        for protocol in [
            ProtocolType::Halfblocks,
            ProtocolType::Sixel,
            ProtocolType::Iterm2,
        ] {
            let mut picker = Picker::halfblocks();
            picker.set_protocol_type(protocol);
            let encoder = Encoder::with_ids(picker, Vec::new());

            let image = encoder.encode(red());

            assert!(
                matches!(
                    (protocol, image.protocol.protocol_type()),
                    (
                        ProtocolType::Halfblocks,
                        StatefulProtocolType::Halfblocks(_)
                    ) | (ProtocolType::Sixel, StatefulProtocolType::Sixel(_))
                        | (ProtocolType::Iterm2, StatefulProtocolType::ITerm2(_))
                ),
                "{protocol:?}"
            );
        }
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

    /// A terminal's answer to the graphics query, as kitty gives it.
    #[cfg(unix)]
    const KITTY_ANSWER: &[u8] = b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n";

    /// Writes each of `bursts` to a new pipe after its delay, counted
    /// from the one before, on a thread of its own. Returns the pipe's
    /// reading end and the thread, which closes the writing end when done.
    #[cfg(unix)]
    fn answering(
        bursts: Vec<(Duration, &'static [u8])>,
    ) -> (std::io::PipeReader, std::thread::JoinHandle<()>) {
        let (input, mut terminal) = std::io::pipe().expect("a pipe");
        let writer = std::thread::spawn(move || {
            for (delay, burst) in bursts {
                std::thread::sleep(delay);
                terminal.write_all(burst).expect("the pipe takes it");
            }
        });
        (input, writer)
    }

    /// What is left in `input` once its writer is done.
    #[cfg(unix)]
    fn left_over(mut input: std::io::PipeReader, writer: std::thread::JoinHandle<()>) -> Vec<u8> {
        use std::io::Read;

        writer.join().expect("the writer finishes");
        let mut rest = Vec::new();
        input.read_to_end(&mut rest).expect("the pipe reads");
        rest
    }

    /// With several tmux clients showing the pane, every client's terminal
    /// answers the query. The query reads up to the first status report,
    /// and the answers of the other two follow it, one after the other.
    #[cfg(unix)]
    #[test]
    fn the_answers_of_further_clients_are_discarded() {
        let (input, writer) = answering(vec![
            (Duration::ZERO, KITTY_ANSWER),
            (Duration::from_millis(50), KITTY_ANSWER),
        ]);

        let discarded =
            discard_late_answers(&input, Duration::from_millis(400), Duration::from_secs(5));

        assert_eq!(discarded.expect("the pipe reads"), 2 * KITTY_ANSWER.len());
        assert_eq!(left_over(input, writer), b"");
    }

    /// An answer that comes after the query gave up is discarded while it
    /// comes within the quiet time; a key typed after that stays.
    #[cfg(unix)]
    #[test]
    fn an_answer_after_the_query_gave_up_is_discarded_and_later_keys_stay() {
        let (input, writer) = answering(vec![
            (Duration::from_millis(100), KITTY_ANSWER),
            (Duration::from_millis(1500), b"q"),
        ]);

        let discarded =
            discard_late_answers(&input, Duration::from_millis(400), Duration::from_secs(5));

        assert_eq!(discarded.expect("the pipe reads"), KITTY_ANSWER.len());
        assert_eq!(left_over(input, writer), b"q");
    }

    /// Input that never pauses, such as a long paste, ends the discarding
    /// at the cap, and so do a closed input and one that never brings a
    /// byte.
    #[cfg(unix)]
    #[test]
    fn discarding_ends_at_its_cap_at_the_end_of_input_and_without_input() {
        let (input, writer) = answering(vec![(Duration::from_millis(20), b"x"); 100]);
        let started = Instant::now();
        let discarded = discard_late_answers(
            &input,
            Duration::from_millis(400),
            Duration::from_millis(300),
        );
        assert!(discarded.expect("the pipe reads") > 0);
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
        assert!(!left_over(input, writer).is_empty(), "the rest stays");

        let (input, terminal) = std::io::pipe().expect("a pipe");
        drop(terminal);
        let discarded =
            discard_late_answers(&input, Duration::from_secs(5), Duration::from_secs(5));
        assert_eq!(discarded.expect("the pipe reads"), 0, "the input ended");

        let (input, _terminal) = std::io::pipe().expect("a pipe");
        let started = Instant::now();
        let discarded =
            discard_late_answers(&input, Duration::from_millis(50), Duration::from_secs(5));
        assert_eq!(discarded.expect("the pipe reads"), 0, "silence");
        assert!(started.elapsed() >= Duration::from_millis(50));
    }

    /// A terminal that gave no usable answer may still send one after the
    /// query's deadline, and every further tmux client's terminal sends
    /// one of its own. A lone terminal that answered sends nothing more.
    #[test]
    fn answers_may_follow_without_capabilities_or_with_several_clients() {
        assert!(answers_may_follow_the_query(false, None));
        assert!(answers_may_follow_the_query(false, Some("on\t1\t\n")));
        assert!(answers_may_follow_the_query(true, Some("all\t2\t\n")));
        assert!(!answers_may_follow_the_query(true, None));
        assert!(!answers_may_follow_the_query(true, Some("on\t1\tkitty\n")));
        assert!(!answers_may_follow_the_query(true, Some("on\tmany\t\n")));
    }

    #[test]
    fn inside_tmux_only_with_passthrough_and_a_client() {
        let tmux_term = (Some("tmux-256color"), Some("tmux"));
        assert!(may_query(TMUX, tmux_term, || Some("on\t1\t\n".into())));
        assert!(may_query(TMUX, tmux_term, || Some(
            "all\t2\tkitty(0.35.2)".into()
        )));
        assert!(!may_query(TMUX, tmux_term, || Some("off\t1\t\n".into())));
        assert!(
            !may_query(TMUX, tmux_term, || Some("on\t0\t\n".into())),
            "detached"
        );
        assert!(
            may_query(TMUX, PLAIN, || Some("on\t1\t\n".into())),
            "TMUX decides, whatever TERM says"
        );
    }

    /// The client's terminal follows the two fields this needs, and its
    /// answer to XTVERSION holds blanks.
    #[test]
    fn inside_tmux_a_termtype_with_blanks_leaves_the_fields_before_it() {
        assert_eq!(
            TMUX_STATE_FORMAT,
            "#{allow-passthrough}\t#{session_attached}\t#{client_termtype}"
        );
        assert!(may_query(TMUX, PLAIN, || Some(
            "on\t1\tWezTerm 20240203-110809-5046fc22\n".into()
        )));
        assert!(!may_query(TMUX, PLAIN, || Some(
            "off\t1\tiTerm2 3.6.7\n".into()
        )));
    }

    #[test]
    fn inside_tmux_anything_unexpected_counts_as_no() {
        assert!(!may_query(TMUX, PLAIN, || None));
        assert!(!may_query(TMUX, PLAIN, || Some(String::new())));
        assert!(!may_query(TMUX, PLAIN, || Some("on".into())));
        assert!(!may_query(TMUX, PLAIN, || Some("on\tmany\t".into())));
        assert!(
            !may_query(TMUX, PLAIN, || Some("on 1\n".into())),
            "blanks do not separate the fields"
        );
    }
}
