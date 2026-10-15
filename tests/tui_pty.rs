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

/// A terminal's answer to the graphics query without any graphics: only
/// the status report that ends it.
const NO_GRAPHICS: &str = "\x1b[0n";

/// A terminal's answer to the graphics query with Kitty graphics and
/// cells of 10×20 pixels, then the status report.
const KITTY_GRAPHICS: &str = "\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n";

/// How long asqr waits for the answer to the graphics query.
const QUERY_WAIT: Duration = Duration::from_secs(5);

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

    /// Waits until asqr has asked the terminal for its graphics protocol.
    /// The status request ends the query.
    fn wait_for_the_query(&self) {
        let start = Instant::now();
        while !self.output().contains("[5n") {
            assert!(
                start.elapsed() < PATIENCE,
                "asqr never queried the terminal"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Answers the terminal query with `answer` in one write, as a
    /// terminal sends it.
    fn answer_the_query(&mut self, answer: &str) {
        self.wait_for_the_query();
        self.send(answer);
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
    let mut tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
    );
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
    let mut tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
    );
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

/// A signal sent from outside, as `kill` sends it, quits as `q` does: the
/// title is given back, focus reports and bracketed paste are off, and the
/// terminal leaves the alternate screen. ctrl-c from the keyboard is a key
/// in raw mode, not a SIGINT.
#[test]
fn sigterm_sigint_and_sighup_quit_and_give_the_terminal_back() {
    for signal in ["TERM", "INT", "HUP"] {
        let sandbox = Sandbox::new();
        let dir = sandbox.queue_dir();
        let mut tui = Tui::start(&sandbox, &["--dir", dir.to_str().expect("UTF-8")], "off 1");
        tui.wait_for("Nothing to answer");
        let pid = tui.child.process_id().expect("asqr runs");

        let sent = std::process::Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(pid.to_string())
            .status()
            .expect("kill runs");
        assert!(sent.success(), "SIG{signal} sent");

        assert_eq!(tui.wait_for_exit(), 0, "SIG{signal}");
        let output = tui.output();
        let pop = last(&output, POP_TITLE);
        let main_screen = last(&output, "\x1b[?1049l");
        assert!(pop < main_screen, "SIG{signal}: {output:?}");
        assert!(
            output.contains("\x1b[?1004l") && output.contains("\x1b[?2004l"),
            "SIG{signal}: focus reports and bracketed paste off: {output:?}"
        );
    }
}

#[test]
fn keys_still_arrive_after_the_terminal_answered_the_query() {
    let sandbox = Sandbox::new();
    let mut tui = Tui::start(
        &sandbox,
        &["--queue", "pty", "--no-notify", "--no-title"],
        "on\t1\t",
    );
    tui.answer_the_query(NO_GRAPHICS);
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

/// A terminal that leaves the graphics query unanswered costs the query's
/// whole wait, and then the keys typed go to asqr: nothing goes on reading
/// stdin for the query.
#[test]
fn a_key_typed_after_an_unanswered_query_reaches_asqr() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        "on\t1\t",
    );
    tui.wait_for_the_query();
    let asked = Instant::now();
    tui.wait_for("Nothing to answer");
    assert!(
        asked.elapsed() >= QUERY_WAIT - Duration::from_millis(200),
        "asqr waits for the answer: {:?}",
        asked.elapsed()
    );

    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
}

/// An answer that arrives just after the query gave up is no input: its
/// digits would pick an option.
#[test]
fn an_answer_after_the_query_gave_up_picks_no_option() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    let mut tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        "on\t1\t",
    );
    tui.wait_for_the_query();
    std::thread::sleep(QUERY_WAIT);

    tui.send(KITTY_GRAPHICS);
    tui.wait_for("Pick a fruit");
    std::thread::sleep(Duration::from_millis(500));

    let screen = tui.screen();
    assert!(!screen.contains("Review your answers"), "{screen}");
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);
    assert!(
        !sandbox.queue_dir().join("outbox/pty-test.json").exists(),
        "nothing answered"
    );
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
        "off\t1\t",
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

/// The parameters of every SGR sequence (`ESC [ … m`) in `output` that
/// sets a colour other than the terminal's default: by 38 (foreground) or
/// 48 (background), the forms that carry an indexed or an RGB colour, or
/// by one of the palette's codes, 30 to 37, 40 to 47, 90 to 97 and 100 to
/// 107. 39 and 49, the defaults, do not count.
fn colour_sequences(output: &str) -> Vec<&str> {
    output
        .split("\x1b[")
        .skip(1)
        .filter_map(|rest| {
            let end = rest.find(|c: char| ('\x40'..='\x7e').contains(&c))?;
            rest[end..].starts_with('m').then(|| &rest[..end])
        })
        .filter(|parameters| {
            parameters.split([';', ':']).any(|parameter| {
                matches!(
                    parameter.parse::<u16>(),
                    Ok(30..=38 | 40..=48 | 90..=97 | 100..=107)
                )
            })
        })
        .collect()
}

/// Starts asqr on an empty queue under `NO_COLOR` set to `no_color`,
/// inside a stand-in tmux whose client's terminal answered XTVERSION with
/// `termtype`, in a terminal of 40 rows, whose main area has room for
/// Talqi. Waits until `talqi` shows and quits. Returns all output.
fn empty_queue_under(no_color: &str, termtype: &str, talqi: &str) -> String {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start_with(
        &sandbox,
        &[
            "--dir",
            dir.to_str().expect("UTF-8"),
            "--no-notify",
            "--no-title",
        ],
        &format!("off\t1\t{termtype}"),
        &[("NO_COLOR", no_color)],
    );
    tui.wait_for("Nothing to answer");
    tui.resize(40, 100);
    tui.wait_for(talqi);
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);
    tui.output()
}

#[test]
fn no_color_shows_talqi_as_an_outline_without_a_colour_sequence() {
    // A piece of the quadrant outline's bottom row: the stand-in tmux's
    // client is a terminal asqr does not know.
    let colourless = empty_queue_under("1", "", "▄▄▄▄▄▄▄▄");
    assert_eq!(colour_sequences(&colourless), Vec::<&str>::new());

    // With colours, the title bar and Talqi write some, so the search
    // finds what there is.
    let coloured = empty_queue_under("", "", "▀▀▀▀▀▀▀▀");
    let sequences = colour_sequences(&coloured);
    assert!(
        sequences.iter().any(|sgr| sgr.contains("38;2;")),
        "{sequences:?}"
    );
}

/// Inside tmux, the terminal of the client tmux draws for decides the
/// glyphs: Ghostty draws sextants itself.
#[test]
fn talqi_is_drawn_in_sextants_for_a_terminal_that_draws_them() {
    // A piece of the sextant outline's bottom row.
    let output = empty_queue_under("1", "ghostty 1.1.3", "🬹🬹🬹🬹🬹🬹🬹🬹");
    assert_eq!(colour_sequences(&output), Vec::<&str>::new());
}

/// The id of the `nth` Kitty image asqr sent, counted from 0, once it is
/// in `tui`'s output.
fn sent_kitty_image(tui: &Tui, nth: usize) -> u32 {
    const SENT: &str = "\x1b_Gq=2,i=";
    let start = Instant::now();
    loop {
        let output = tui.output();
        if let Some((_, after)) = output.split(SENT).enumerate().nth(nth + 1) {
            let id = &after[..after.find(',').expect("more keys follow the id")];
            return id.parse().expect("a numeric id");
        }
        assert!(
            start.elapsed() < PATIENCE,
            "asqr never sent image {nth}: {output:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The command that deletes the Kitty image `id`.
fn kitty_delete(id: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

/// Starts asqr in a terminal that answers the graphics query as one with
/// Kitty graphics and cells of 10×20 pixels, and has it show a session
/// with a red image.
fn showing_a_kitty_image(sandbox: &Sandbox) -> Tui {
    let dir = sandbox.queue_dir();
    // asqr inherits the variables of the terminal the tests run in. A
    // `TERM_PROGRAM` of tmux makes ratatui-image wrap every command for
    // passthrough, and WezTerm's or Konsole's variable rules the Kitty
    // protocol out whatever the terminal answers (ratatui-image
    // 12.0.0-rc.2 `picker.rs` l.131-138), where empty counts as unset.
    let mut tui = Tui::start_with(
        sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        "on\t1\t",
        &[
            ("TERM_PROGRAM", "pty-test"),
            ("WEZTERM_EXECUTABLE", ""),
            ("KONSOLE_VERSION", ""),
        ],
    );
    tui.answer_the_query(KITTY_GRAPHICS);
    tui.wait_for("Nothing to answer");

    let picture = sandbox.home().join("red.png");
    image::RgbImage::from_pixel(40, 40, image::Rgb([255, 0, 0]))
        .save(&picture)
        .expect("png is written");
    let session = SESSION.replace(
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
    tui
}

/// The Kitty placement names its size in cells, so the terminal draws the
/// image into exactly its placeholder cells, whatever its own cell size.
#[test]
fn a_kitty_image_is_placed_with_its_size_in_cells() {
    let sandbox = Sandbox::new();
    let mut tui = showing_a_kitty_image(&sandbox);
    sent_kitty_image(&tui, 0);
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);

    let output = tui.output();
    let placement = &output[first(&output, "a=T,U=1,")..];
    let keys = &placement[..placement.find(';').expect("the keys end before the data")];
    let cells = |key: &str| {
        keys.split(',')
            .find_map(|pair| pair.strip_prefix(key))
            .unwrap_or_else(|| panic!("no {key} in {keys:?}"))
            .parse::<u16>()
            .expect("a number of cells")
    };
    assert!(cells("c=") > 0 && cells("r=") > 0, "{keys:?}");
}

/// Under the Kitty protocol, an image sent again on regaining the focus
/// has its old copy deleted after the frame that shows the new one, and
/// the one shown when asqr quits is deleted before the title is given
/// back and asqr leaves the alternate screen (ADR 42).
#[test]
fn kitty_images_are_deleted_after_the_frame_and_before_quitting() {
    let sandbox = Sandbox::new();
    let mut tui = showing_a_kitty_image(&sandbox);
    let old = sent_kitty_image(&tui, 0);

    tui.send("\x1b[I");
    let again = sent_kitty_image(&tui, 1);
    let start = Instant::now();
    while !tui.output().contains(&kitty_delete(old)) {
        assert!(
            start.elapsed() < PATIENCE,
            "the first copy is never deleted"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let output = tui.output();
    assert_ne!(old, again);
    let sent_again = first(&output, &format!("\x1b_Gq=2,i={again},"));
    let old_deleted = first(&output, &kitty_delete(old));
    let again_deleted = first(&output, &kitty_delete(again));
    let pop = last(&output, POP_TITLE);
    let main_screen = last(&output, "\x1b[?1049l");
    assert!(
        sent_again < old_deleted
            && old_deleted < again_deleted
            && again_deleted < pop
            && pop < main_screen,
        "{output:?}"
    );
}
