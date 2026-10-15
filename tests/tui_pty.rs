// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The TUI in a real pseudo-terminal: the part of asqr that only runs with
//! a terminal, from start to quit.
//!
//! asqr runs as if inside tmux, with a stand-in `tmux` on the path. When
//! it reports passthrough off, asqr skips the graphics query; when it
//! reports passthrough on, the test answers the query as a terminal would.
//! Stand-ins for `open` and `xdg-open` record what the image viewer was
//! asked to show.

#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use asqr::format::{SessionResult, Status};
use common::Sandbox;
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

const SESSION: &str = r#"{"asqr": 2, "id": "pty-test", "questions": [{"id": "q",
    "text": "Pick a fruit", "kind": "single", "custom": true,
    "options": [{"id": "a", "label": "Apples"}, {"id": "b", "label": "Bananas"}]}]}"#;

const PATIENCE: Duration = Duration::from_secs(20);

struct Tui {
    size: (u16, u16),
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: Arc<Mutex<Vec<u8>>>,
    // The pty closes with this; it has to live as long as the child.
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Tui {
    /// Starts `asqr` with `queue` (such as `--dir <path>`) inside a
    /// stand-in tmux that reports `tmux_state`.
    fn start(sandbox: &Sandbox, queue: &[&str], tmux_state: &str) -> Self {
        Self::start_with(sandbox, queue, tmux_state, &[])
    }

    /// Like [`Tui::start`], with the variables in `environment` set.
    fn start_with(
        sandbox: &Sandbox,
        queue: &[&str],
        tmux_state: &str,
        environment: &[(&str, &str)],
    ) -> Self {
        let bin = sandbox.home().join("bin");
        std::fs::create_dir_all(&bin).expect("bin dir");
        let viewed = sandbox.home().join("viewed");
        let scripts = [
            ("tmux", format!("echo '{tmux_state}'")),
            ("open", format!("echo \"$1\" > '{}'", viewed.display())),
            ("xdg-open", format!("echo \"$1\" > '{}'", viewed.display())),
        ];
        for (name, body) in scripts {
            let script = bin.join(name);
            std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("script");
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("a pty");
        let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin("asqr"));
        command.args(queue);
        command.env("HOME", sandbox.home());
        command.env("XDG_DATA_HOME", sandbox.home().join(".local/share"));
        command.env("XDG_CACHE_HOME", sandbox.home().join(".cache"));
        command.env_remove("ASQR_QUEUE");
        command.env_remove("ASQR_DIR");
        command.env_remove("NO_COLOR");
        command.env("TERM", "xterm-256color");
        command.env("TMUX", "/tmp/tmux-test/default,1,0");
        let path = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{}:{path}", bin.display()));
        for (name, value) in environment {
            command.env(name, value);
        }
        let child = pair.slave.spawn_command(command).expect("asqr starts");
        drop(pair.slave);

        // The pty has to be drained, or asqr blocks once its buffer is
        // full.
        let output = Arc::new(Mutex::new(Vec::new()));
        let mut reader = pair.master.try_clone_reader().expect("reader");
        let sink = Arc::clone(&output);
        std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                sink.lock()
                    .expect("not poisoned")
                    .extend_from_slice(&buffer[..read]);
            }
        });
        let writer = pair.master.take_writer().expect("writer");
        Tui {
            size: (30, 100),
            child,
            writer,
            output,
            _master: pair.master,
        }
    }

    fn output(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().expect("not poisoned")).into_owned()
    }

    /// The screen as a terminal shows it after all output so far. asqr
    /// only redraws cells that change, so the raw output is no text to
    /// search.
    fn screen(&self) -> String {
        self.terminal().screen().contents()
    }

    /// A terminal that took all output so far.
    fn terminal(&self) -> vt100::Parser {
        let mut parser = vt100::Parser::new(self.size.0, self.size.1, 0);
        parser.process(&self.output.lock().expect("not poisoned"));
        parser
    }

    /// Every cell of the screen.
    fn cells(&self) -> Vec<vt100::Cell> {
        let terminal = self.terminal();
        let screen = terminal.screen();
        (0..self.size.0)
            .flat_map(|row| (0..self.size.1).map(move |column| (row, column)))
            .filter_map(|(row, column)| screen.cell(row, column).cloned())
            .collect()
    }

    /// The cell where `text` starts on the screen, failing when it does not
    /// show. `text` and what stands before it on its row are narrow
    /// characters, one cell each.
    fn cell_of(&self, text: &str) -> vt100::Cell {
        let terminal = self.terminal();
        let screen = terminal.screen();
        let (row, column) = screen
            .rows(0, self.size.1)
            .enumerate()
            .find_map(|(row, line)| {
                let start = line.find(text)?;
                Some((row, line[..start].chars().count()))
            })
            .unwrap_or_else(|| panic!("{text:?} is not on the screen:\n{}", screen.contents()));
        let row = u16::try_from(row).expect("a screen row");
        let column = u16::try_from(column).expect("a screen column");
        screen.cell(row, column).expect("on the screen").clone()
    }

    /// Waits until the screen shows `text`.
    fn wait_for(&self, text: &str) {
        let start = Instant::now();
        while !self.screen().contains(text) {
            assert!(
                start.elapsed() < PATIENCE,
                "{text:?} never showed up; the screen:\n{}",
                self.screen()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Answers the terminal query with only the status report that ends
    /// it, as a terminal without graphics would.
    fn answer_the_query(&mut self) {
        let start = Instant::now();
        while !self.output().contains("[5n") {
            assert!(
                start.elapsed() < PATIENCE,
                "asqr never queried the terminal"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        self.writer.write_all(b"\x1b[0n").expect("answered");
        self.writer.flush().expect("flushed");
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self._master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resized");
        self.size = (rows, cols);
    }

    fn press(&mut self, keys: &str) {
        for key in keys.chars() {
            write!(self.writer, "{key}").expect("key written");
            self.writer.flush().expect("flushed");
            // One key at a time, as a person types.
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Writes `sequence` in one write, as a terminal sends what it
    /// reports. Written a key at a time, its escape would arrive alone and
    /// read as the `esc` key.
    fn send(&mut self, sequence: &str) {
        self.writer
            .write_all(sequence.as_bytes())
            .expect("sequence written");
        self.writer.flush().expect("flushed");
        std::thread::sleep(Duration::from_millis(30));
    }

    fn wait_for_exit(&mut self) -> u32 {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().expect("status") {
                return status.exit_code();
            }
            assert!(start.elapsed() < PATIENCE, "asqr did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn answers_a_session_that_arrives_while_it_runs() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start(&sandbox, &["--dir", dir.to_str().expect("UTF-8")], "off 1");
    tui.wait_for("Nothing to answer");

    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("Pick a fruit");

    // Inside tmux the notification travels in the passthrough wrapper,
    // followed by the bell.
    assert!(
        tui.output()
            .contains("\x1bPtmux;\x1b\x1b]9;asqr: new session pty-test\x07\x1b\\\x07"),
        "{:?}",
        tui.output()
    );

    // asqr asks for focus reports, so it can send its images again when
    // it comes back into view (spec section 7.7). A report is no key: in
    // the middle of typing an own answer, its escape would leave the
    // field and the y would go nowhere.
    assert!(
        tui.output().contains("\x1b[?1004h"),
        "focus reporting is on"
    );
    tui.press("jjx");
    tui.send("\x1b[I");
    tui.press("y");
    tui.wait_for("Own answer: xy");

    // asqr asks for bracketed paste, so a pasted line break is no enter
    // that would pick the own answer, but a line break in it. Terminals
    // send a pasted line break as CR, the byte of `enter` outside a paste,
    // and a pasted CRLF arrives as it was copied; both become LF.
    assert!(
        tui.output().contains("\x1b[?2004h"),
        "bracketed paste is on"
    );
    tui.send("\x1b[200~ z\rw\x1b[201~");
    tui.send("\x1b[200~\r\nv\x1b[201~");
    tui.wait_for("Own answer: xy z");

    // Pick the own answer, which moves on to the review, where the cursor
    // rests on Submit.
    tui.press("\r");
    tui.wait_for("Review your answers");
    tui.press("\r");
    tui.wait_for("Nothing to answer");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    assert!(
        tui.output().contains("\x1b[?1049l"),
        "the terminal leaves the alternate screen again"
    );
    assert!(
        tui.output().contains("\x1b[?1004l"),
        "focus reporting is off again"
    );
    assert!(
        tui.output().contains("\x1b[?2004l"),
        "bracketed paste is off again"
    );
    let result: SessionResult = serde_json::from_slice(
        &std::fs::read(sandbox.queue_dir().join("outbox/pty-test.json")).expect("result"),
    )
    .expect("parses");
    assert_eq!(result.status, Status::Submitted);
    assert_eq!(result.answers[0].custom.as_deref(), Some("xy z\nw\nv"));
}

/// Where `needle` first occurs in `haystack`, failing when it does not.
fn first(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is missing: {haystack:?}"))
}

/// Where `needle` last occurs in `haystack`, failing when it does not.
fn last(haystack: &str, needle: &str) -> usize {
    haystack
        .rfind(needle)
        .unwrap_or_else(|| panic!("{needle:?} is missing: {haystack:?}"))
}

const PUSH_TITLE: &str = "\x1b[22;0t";
const EMPTY_TITLE: &str = "\x1b]0;\x07";
const POP_TITLE: &str = "\x1b[23;0t";

#[test]
fn the_title_counts_the_waiting_sessions_and_is_given_back_on_quit() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start(&sandbox, &["--dir", dir.to_str().expect("UTF-8")], "off 1");
    tui.wait_for("Nothing to answer");
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("Pick a fruit");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    // `Tui` runs asqr as if inside tmux, which takes the title as the
    // pane's own, so it goes out plain, not in the passthrough wrapper.
    let output = tui.output();
    let alternate_screen = first(&output, "\x1b[?1049h");
    let push = first(&output, PUSH_TITLE);
    let none_waiting = first(&output, "\x1b]0;asqr · 0 waiting\x07");
    let one_waiting = first(&output, "\x1b]0;asqr · 1 waiting\x07");
    let cleared = last(&output, EMPTY_TITLE);
    let pop = last(&output, POP_TITLE);
    let main_screen = last(&output, "\x1b[?1049l");
    assert!(
        alternate_screen < push
            && push < none_waiting
            && none_waiting < one_waiting
            && one_waiting < cleared
            && cleared < pop
            && pop < main_screen,
        "{output:?}"
    );
    assert_eq!(output.matches(PUSH_TITLE).count(), 1, "{output:?}");
    assert_eq!(output.matches(POP_TITLE).count(), 1, "{output:?}");
}

#[test]
fn keys_still_arrive_after_the_terminal_answered_the_query() {
    let sandbox = Sandbox::new();
    let mut tui = Tui::start(
        &sandbox,
        &["--queue", "pty", "--no-notify", "--no-title"],
        "on 1",
    );
    tui.answer_the_query();
    tui.wait_for("queue: pty");

    // Only a resize that reached the loop draws the size message, and
    // growing again brings the layout back.
    tui.resize(10, 40);
    tui.wait_for("asqr needs at least 60×15");
    tui.resize(24, 80);
    tui.wait_for("Nothing to answer");
    let file = sandbox.file(
        "pictured.json",
        &SESSION.replace(
            r#""kind": "single","#,
            r#""kind": "single", "context": [{"type": "image", "path": "/pictures/fruit.png"}],"#,
        ),
    );
    sandbox
        .asqr()
        .args(["--queue", "pty", "ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("Pick a fruit");

    tui.press("o");
    // The stand-in viewer creates the file before it writes the line, so
    // only a whole line in it says that the viewer is done.
    let viewed = sandbox.home().join("viewed");
    let start = Instant::now();
    let recorded = loop {
        let recorded = std::fs::read_to_string(&viewed).unwrap_or_default();
        if recorded.ends_with('\n') && !recorded.trim().is_empty() {
            break recorded;
        }
        assert!(
            start.elapsed() < PATIENCE,
            "the viewer never recorded a path"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    assert_eq!(recorded.trim(), "/pictures/fruit.png");
    assert!(
        !tui.output().contains("]9;"),
        "--no-notify sends no notification"
    );
    let output = tui.output();
    for (sequence, name) in [
        (PUSH_TITLE, "push"),
        ("\x1b]0;", "title"),
        (POP_TITLE, "pop"),
    ] {
        assert!(
            !output.contains(sequence),
            "--no-title writes no {name}: {output:?}"
        );
    }
}

/// What a terminal shows of asqr under `NO_COLOR` set to `no_color`.
struct Colours {
    /// The first cell of the title bar.
    title: vt100::Cell,
    /// The first cell of a notice in the status line.
    notice: vt100::Cell,
    /// The first cell of inline code in the dim intro.
    intro_code: vt100::Cell,
    /// Whether the red test image shows in its colour.
    red_image: bool,
}

/// Starts asqr under `NO_COLOR` set to `no_color` and has it show a notice
/// and then a session with inline code in its intro and an image, which
/// the half blocks draw in RGB colours.
fn colours_under(no_color: &str) -> Colours {
    const RED: [u8; 3] = [255, 0, 0];
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let tui = Tui::start_with(
        &sandbox,
        &[
            "--dir",
            dir.to_str().expect("UTF-8"),
            "--no-notify",
            "--no-title",
        ],
        "off 1",
        &[("NO_COLOR", no_color)],
    );
    tui.wait_for("Nothing to answer");

    // An invalid session is answered at once and leaves a notice, which
    // stays over the next session until a key is pressed. It is written
    // under another name and renamed, so asqr never reads half of it.
    let written = dir.join("inbox/.broken.part");
    std::fs::write(&written, "not json").expect("written");
    std::fs::rename(&written, dir.join("inbox/broken.json")).expect("renamed");
    tui.wait_for("broken is invalid");

    let picture = sandbox.home().join("red.png");
    image::RgbImage::from_pixel(40, 40, image::Rgb(RED))
        .save(&picture)
        .expect("png is written");
    let session = SESSION
        .replace(
            r#""id": "pty-test","#,
            r#""id": "pty-test", "intro": "Mind the `stalk`.","#,
        )
        .replace(
            r#""kind": "single","#,
            &format!(
                r#""kind": "single", "context": [{{"type": "image", "path": {:?}}}],"#,
                picture.to_str().expect("UTF-8")
            ),
        );
    let file = sandbox.file("session.json", &session);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("Pick a fruit");

    let red = vt100::Color::Rgb(RED[0], RED[1], RED[2]);
    Colours {
        title: tui.cell_of("asqr"),
        notice: tui.cell_of("broken is invalid"),
        intro_code: tui.cell_of("stalk"),
        red_image: tui
            .cells()
            .iter()
            .any(|cell| cell.fgcolor() == red || cell.bgcolor() == red),
    }
}

#[test]
fn no_color_takes_the_named_colours_and_keeps_modifiers_and_images() {
    let colourless = colours_under("1");
    let Colours {
        title,
        notice,
        intro_code,
        red_image,
    } = &colourless;
    let default = (vt100::Color::Default, vt100::Color::Default);
    assert_eq!((title.fgcolor(), title.bgcolor()), default);
    assert!(title.bold(), "the name keeps its bold");
    assert_eq!((notice.fgcolor(), notice.bgcolor()), default);
    assert_eq!((intro_code.fgcolor(), intro_code.bgcolor()), default);
    assert!(intro_code.dim(), "the intro stays dim");
    assert!(red_image, "the image keeps its colours");

    // An empty NO_COLOR counts as unset.
    let coloured = colours_under("");
    assert_eq!(coloured.title.fgcolor(), vt100::Color::Idx(6), "cyan");
    assert_eq!(coloured.notice.fgcolor(), vt100::Color::Idx(3), "yellow");
    assert_ne!(coloured.intro_code.fgcolor(), vt100::Color::Default);
    assert!(coloured.intro_code.dim());
    assert!(coloured.red_image);
}
