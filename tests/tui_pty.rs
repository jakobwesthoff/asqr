// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The TUI in a real pseudo-terminal: the part of asqr that only runs with
//! a terminal, from start to quit.
//!
//! asqr runs as if inside tmux, with a stand-in `tmux` on the path. When
//! it reports passthrough off, asqr skips the graphics query; when it
//! reports passthrough on, the test answers the query as a terminal would.
//! Each `tmux set` it gets shows in the output as a [`tmux_set`] mark, so
//! a test sees where among asqr's own writes it ran.
//! Stand-ins for `open` and `xdg-open` record what the image viewer was
//! asked to show, and exit with `VIEWER_EXIT` where it is set.

#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::num::NonZeroU16;
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

/// What the stand-in tmux reports for a pane with passthrough on, one
/// client attached, a client terminal that names itself not, the pane
/// showing in the current window, unzoomed, and tmux 3.5.
const PASSTHROUGH_ON: &str = "on\t1\t\t1\t1\t0\t3.5";

/// The file in the sandbox's home where the stand-in tmux notes each
/// command it gets, one line each.
const TMUX_COMMANDS: &str = "tmux-commands";

/// The mark the stand-in tmux writes to the terminal for `tmux set` with
/// `arguments`: an OSC sequence no terminal acts on.
fn tmux_set(arguments: &str) -> String {
    format!("\x1b]7777;tmux set {arguments}\x07")
}

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
    /// stand-in tmux that reports `tmux_state`, shows `on` as the pane's
    /// passthrough and notes every command in [`TMUX_COMMANDS`] in the
    /// sandbox's home.
    fn start(sandbox: &Sandbox, queue: &[&str], tmux_state: &str) -> Self {
        Self::start_with(sandbox, queue, tmux_state, &[])
    }

    /// Like [`Tui::start`], with the variables in `environment` set. The
    /// sandbox gets an empty settings file first, unless it has one, so
    /// asqr does not ask the first start (ADR 106).
    fn start_with(
        sandbox: &Sandbox,
        queue: &[&str],
        tmux_state: &str,
        environment: &[(&str, &str)],
    ) -> Self {
        std::fs::create_dir_all(sandbox.config_dir()).expect("config dir");
        let created = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(sandbox.config_dir().join("settings.toml"));
        match created {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("settings file: {error}"),
        }
        Self::start_first_time(sandbox, queue, tmux_state, environment)
    }

    /// Like [`Tui::start_with`], and leaves the settings file as it is in
    /// the sandbox, which is no file at the first start.
    fn start_first_time(
        sandbox: &Sandbox,
        queue: &[&str],
        tmux_state: &str,
        environment: &[(&str, &str)],
    ) -> Self {
        let bin = sandbox.home().join("bin");
        std::fs::create_dir_all(&bin).expect("bin dir");
        let viewed = sandbox.home().join("viewed");
        let commands = sandbox.home().join(TMUX_COMMANDS);
        let scripts = [
            (
                "tmux",
                format!(
                    "echo \"$*\" >> '{}'\n\
                     case \"$1\" in\n\
                     display-message) echo '{tmux_state}' ;;\n\
                     show-options) echo on ;;\n\
                     set) printf '\\033]7777;tmux %s\\007' \"$*\" > /dev/tty ;;\n\
                     esac",
                    commands.display()
                ),
            ),
            (
                "open",
                format!(
                    "echo \"$1\" > '{}'\nexit \"${{VIEWER_EXIT:-0}}\"",
                    viewed.display()
                ),
            ),
            (
                "xdg-open",
                format!(
                    "echo \"$1\" > '{}'\nexit \"${{VIEWER_EXIT:-0}}\"",
                    viewed.display()
                ),
            ),
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
        // The files `o` writes, and the start's removal of old ones, stay
        // in the sandbox.
        command.env("TMPDIR", sandbox.temp_dir());
        command.env("XDG_DATA_HOME", sandbox.home().join(".local/share"));
        command.env("XDG_CACHE_HOME", sandbox.home().join(".cache"));
        command.env("XDG_CONFIG_HOME", sandbox.home().join(".config"));
        // The stand-in tmux below may start slowly beside a second test
        // run, and asqr would give it up at the 500 ms it has in use.
        command.env(
            asqr::pairing::TIMEOUT_VARIABLE,
            common::STAND_IN_PATIENCE_MS,
        );
        command.env_remove("ASQR_QUEUE");
        command.env_remove("ASQR_LAYOUT");
        command.env_remove("ASQR_KEEP_FILES");
        command.env_remove("ASQR_DIR");
        command.env_remove("ASQR_LOG");
        command.env_remove("NO_COLOR");
        command.env("TERM", "xterm-256color");
        command.env("TMUX", "/tmp/tmux-test/default,1,0");
        // The pane of the tmux the tests may run in would name a session
        // of the person's own tmux server.
        command.env_remove("TMUX_PANE");
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
        let (rows, columns) = self.size;
        let mut parser = vt100::Parser::new(
            NonZeroU16::new(rows).expect("the pty has rows"),
            NonZeroU16::new(columns).expect("the pty has columns"),
            0,
        );
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

    /// Waits until the screen no longer shows `text`.
    fn wait_until_gone(&self, text: &str) {
        let start = Instant::now();
        while self.screen().contains(text) {
            assert!(
                start.elapsed() < PATIENCE,
                "{text:?} never went; the screen:\n{}",
                self.screen()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The row of the screen where `text` first shows, counted from the
    /// top.
    fn row_of(&self, text: &str) -> u16 {
        let terminal = self.terminal();
        let row = terminal
            .screen()
            .rows(0, self.size.1)
            .position(|line| line.contains(text))
            .unwrap_or_else(|| panic!("{text:?} is not on the screen"));
        u16::try_from(row).expect("a screen row")
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

    /// Answers the terminal query with `answer` if asqr asks it, or
    /// returns once asqr has exited without asking.
    fn answer_the_query_unless_it_exits(&mut self, answer: &str) {
        let start = Instant::now();
        while !self.output().contains("[5n") {
            if self.child.try_wait().expect("status").is_some() {
                return;
            }
            assert!(
                start.elapsed() < PATIENCE,
                "asqr neither queried the terminal nor exited"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
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
fn a_settings_file_that_does_not_parse_is_said_and_asqr_runs() {
    let sandbox = Sandbox::new();
    std::fs::create_dir_all(sandbox.config_dir()).expect("config dir");
    std::fs::write(sandbox.config_dir().join("settings.toml"), "context =\n")
        .expect("settings file");
    let dir = sandbox.queue_dir();

    let tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
    );

    tui.wait_for("settings.toml does not parse (line 1");
    tui.wait_for("Nothing to answer");
}

#[test]
fn the_settings_pane_changes_a_setting_and_keeps_it_in_the_file() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
    );
    tui.wait_for("Nothing to answer");

    tui.press(",");
    tui.wait_for("Diffs open as");
    tui.send("\x1b[C");

    tui.wait_for("saved diffs open as: side by side");
    let file = sandbox.config_dir().join("settings.toml");
    let written = std::fs::read_to_string(&file).expect("the settings file is written");
    assert!(
        written.contains("diff_layout = \"side-by-side\""),
        "{written}"
    );
    tui.press(",");
    tui.wait_for("Nothing to answer");
    assert!(!tui.screen().contains("Diffs open as"), "{}", tui.screen());
}

// ---------------------------------------------------------------------
// The first start (ADR 106)
// ---------------------------------------------------------------------

const FIRST_START_QUESTION: &str = "Keep copies of the files a session showed you?";

fn settings_file(sandbox: &Sandbox) -> std::path::PathBuf {
    sandbox.config_dir().join("settings.toml")
}

#[test]
fn the_first_start_asks_once_and_writes_the_file() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let queue = ["--dir", dir.to_str().expect("UTF-8")];
    let mut tui = Tui::start_first_time(&sandbox, &queue, "off\t1\t", &[]);

    tui.wait_for(FIRST_START_QUESTION);
    assert!(!settings_file(&sandbox).exists(), "nothing is written yet");
    tui.send("\r");
    tui.wait_for("enter  save");
    tui.send("\r");

    tui.wait_for("saved keep files: compact copies");
    tui.wait_for("Nothing to answer");
    let written = std::fs::read_to_string(settings_file(&sandbox)).expect("the file is written");
    assert!(written.starts_with("# asqr settings"), "{written}");
    assert!(written.contains("keep_files = \"compact\""), "{written}");
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);

    let again = Tui::start_first_time(&sandbox, &queue, "off\t1\t", &[]);
    again.wait_for("Nothing to answer");
    assert!(
        !again.screen().contains("First start"),
        "{}",
        again.screen()
    );
}

#[test]
fn quitting_the_first_start_writes_nothing_and_the_next_start_asks_again() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let queue = ["--dir", dir.to_str().expect("UTF-8")];
    let mut tui = Tui::start_first_time(&sandbox, &queue, "off\t1\t", &[]);
    tui.wait_for(FIRST_START_QUESTION);
    tui.press("j");

    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    assert!(!settings_file(&sandbox).exists());
    let again = Tui::start_first_time(&sandbox, &queue, "off\t1\t", &[]);
    again.wait_for(FIRST_START_QUESTION);
}

#[test]
fn a_held_keep_files_variable_skips_the_first_start_and_writes_nothing() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let tui = Tui::start_first_time(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
        &[("ASQR_KEEP_FILES", "exact")],
    );

    tui.wait_for("Nothing to answer");

    assert!(!tui.screen().contains("First start"), "{}", tui.screen());
    assert!(!settings_file(&sandbox).exists());
}

#[test]
fn a_settings_file_without_keep_files_skips_the_first_start() {
    let sandbox = Sandbox::new();
    std::fs::create_dir_all(sandbox.config_dir()).expect("config dir");
    std::fs::write(settings_file(&sandbox), "diff_layout = \"unified\"\n").expect("settings file");
    let dir = sandbox.queue_dir();
    let tui = Tui::start_first_time(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
        &[],
    );

    tui.wait_for("Nothing to answer");

    assert!(!tui.screen().contains("First start"), "{}", tui.screen());
    assert_eq!(
        std::fs::read_to_string(settings_file(&sandbox)).expect("still there"),
        "diff_layout = \"unified\"\n"
    );
}

#[test]
fn a_replay_never_asks_the_first_start() {
    let sandbox = Sandbox::new();
    let location = asqr::queue::QueueLocation::named(&sandbox.data_dir(), "work");
    location.create_layout().expect("layout");
    let ulid = ulid::Ulid::from_parts(1_700_000_000_000, 0x1234_5678_9abc);
    let name = asqr::queue::archive_name("replayed", ulid, asqr::queue::Archived::Session);
    std::fs::write(
        location.archive().join(name),
        r#"{"asqr": 2, "id": "replayed", "title": "Replayed session", "questions": [{"id": "q", "text": "Pick a fruit", "kind": "text"}]}"#,
    )
    .expect("archived");
    let text = ulid.to_string();
    let reference = text[text.len() - 8..].to_lowercase();
    let tui = Tui::start_first_time(&sandbox, &["replay", &reference], "off\t1\t", &[]);

    tui.wait_for("Replayed session");

    assert!(!tui.screen().contains("First start"), "{}", tui.screen());
    assert!(!settings_file(&sandbox).exists());
}

#[test]
fn a_first_start_that_cannot_write_says_so_and_goes_on() {
    if rustix::process::geteuid().is_root() {
        // Root writes into a directory whatever its mode says.
        return;
    }
    let sandbox = Sandbox::new();
    std::fs::create_dir_all(sandbox.config_dir()).expect("config dir");
    std::fs::set_permissions(sandbox.config_dir(), std::fs::Permissions::from_mode(0o555))
        .expect("made read-only");
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start_first_time(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
        &[],
    );
    tui.wait_for(FIRST_START_QUESTION);

    tui.send("\r");
    tui.send("\r");

    tui.wait_for("cannot save the setting");
    tui.wait_for("Nothing to answer");
    assert!(!settings_file(&sandbox).exists());
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);
    std::fs::set_permissions(sandbox.config_dir(), std::fs::Permissions::from_mode(0o755))
        .expect("made writable again");
}

#[test]
fn the_first_start_comes_before_the_start_picker() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "other", "first");
    let mut tui =
        Tui::start_first_time(&sandbox, &["--no-notify"], PASSTHROUGH_ON, &[("TMUX", "")]);
    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for(FIRST_START_QUESTION);
    assert!(!tui.screen().contains("enter  watch"), "{}", tui.screen());

    tui.send("\r");
    tui.send("\r");

    tui.wait_for("enter  watch");
    assert!(settings_file(&sandbox).exists());
    tui.press("j");
    tui.send("\r");
    tui.wait_for("Pick a fruit");
    tui.wait_for("saved keep files: compact copies");
}

#[test]
fn a_session_that_arrives_on_the_first_start_shows_after_the_answer() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start_first_time(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
        &[],
    );
    tui.wait_for(FIRST_START_QUESTION);
    let file = sandbox.file("arrived.json", &SESSION.replace("pty-test", "arrived"));
    sandbox
        .asqr()
        .args(["--dir", dir.to_str().expect("UTF-8"), "ask"])
        .arg(&file)
        .assert()
        .success();

    tui.send("\r");
    tui.send("\r");

    tui.wait_for("Pick a fruit");
}

/// A queue whose inbox cannot be read ends asqr with the error, and the
/// session's agents stay where they were: nobody watches that queue.
#[test]
fn a_failed_first_read_of_the_inbox_leaves_the_pin_as_it_was() {
    if rustix::process::geteuid().is_root() {
        // Root reads a directory whatever its mode says.
        return;
    }
    let sandbox = Sandbox::new();
    let location = asqr::queue::QueueLocation::named(&sandbox.data_dir(), "work");
    location.create_layout().expect("layout");
    std::fs::set_permissions(location.inbox(), std::fs::Permissions::from_mode(0o000))
        .expect("made unreadable");
    let mut tui = Tui::start_with(
        &sandbox,
        &["--queue", "work", "--no-notify"],
        PASSTHROUGH_ON,
        &[("TMUX_PANE", "%7")],
    );
    // Where the watch on an unreadable inbox fails (inotify on Linux),
    // asqr ends before it queries the terminal; where the watch succeeds,
    // the first read of the inbox fails after the query. The pin stays
    // as it was either way.
    tui.answer_the_query_unless_it_exits(NO_GRAPHICS);

    let exit = tui.wait_for_exit();

    std::fs::set_permissions(location.inbox(), std::fs::Permissions::from_mode(0o755))
        .expect("made readable again");
    assert_eq!(exit, 1, "the failed read ends asqr");
    let commands = std::fs::read_to_string(sandbox.home().join(TMUX_COMMANDS)).unwrap_or_default();
    assert!(!commands.contains("set-environment"), "{commands}");
}

#[test]
fn quitting_the_first_start_still_pins_the_queue() {
    let sandbox = Sandbox::new();
    let mut tui = Tui::start_first_time(
        &sandbox,
        &["--queue", "work", "--no-notify"],
        PASSTHROUGH_ON,
        &[("TMUX_PANE", "%7")],
    );
    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for(FIRST_START_QUESTION);

    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let commands =
        std::fs::read_to_string(sandbox.home().join(TMUX_COMMANDS)).expect("tmux was run");
    let pins: Vec<&str> = commands
        .lines()
        .filter(|command| command.starts_with("set-environment"))
        .collect();
    assert_eq!(pins, ["set-environment -t %7 ASQR_SESSION_QUEUE work"]);
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
    // The empty queue's key bar names the asqr that runs.
    tui.wait_for(concat!("asqr ", env!("CARGO_PKG_VERSION")));

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
    // it comes back into view (spec section 8.8). A report is no key: in
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

/// The binary keeps the files of a session the person finished, and waits
/// for that when it quits: the wiring of the worker in `terminal.rs`, which
/// no test below the terminal reaches.
#[test]
fn a_finished_session_has_its_files_kept_before_asqr_quits() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let shown = sandbox.file("notes.txt", "what the person saw\n");
    let session = serde_json::json!({
        "asqr": 2, "id": "keep-test",
        "questions": [{"id": "q", "text": "Pick a fruit", "kind": "single",
            "context": [{"type": "code", "path": shown}],
            "options": [{"id": "a", "label": "Apple"}, {"id": "b", "label": "Pear"}]}]
    });
    let file = sandbox.file("session.json", &session.to_string());
    let mut tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
    );
    tui.wait_for("Nothing to answer");
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("Pick a fruit");

    tui.press("1");
    tui.wait_for("Review your answers");
    tui.press("\r");
    tui.wait_for("Nothing to answer");
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);

    let archive = dir.join("archive");
    let archived: Vec<_> = std::fs::read_dir(&archive)
        .expect("archive")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(archived.len(), 1, "{archived:?}");
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&archived[0]).expect("reads")).expect("JSON");
    let path = json["questions"][0]["context"][0]["path"]
        .as_str()
        .expect("a path");
    assert!(
        path.starts_with("files/") && path.ends_with(".txt"),
        "{json}"
    );
    assert_eq!(
        std::fs::read(archive.join(path)).expect("the copy exists"),
        b"what the person saw\n"
    );
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

/// Where the first title of asqr that ends in `end` starts in `haystack`,
/// failing when there is none.
fn first_title(haystack: &str, end: &str) -> usize {
    let start = "\x1b]0;asqr · ";
    haystack
        .match_indices(start)
        .map(|(at, _)| at)
        .find(|&at| {
            haystack[at + start.len()..]
                .split('\x07')
                .next()
                .is_some_and(|title| title.ends_with(end))
        })
        .unwrap_or_else(|| panic!("no title ending in {end:?}: {haystack:?}"))
}

const PUSH_TITLE: &str = "\x1b[22;0t";
const EMPTY_TITLE: &str = "\x1b]0;\x07";
const POP_TITLE: &str = "\x1b[23;0t";

/// The sandbox is the home folder, so its queue is `~/queue` on the
/// waiting screen: in the sentence and in the title bar.
#[test]
fn a_dir_queue_is_named_by_its_path_with_a_tilde_for_home() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
    );

    tui.wait_for("Nothing to answer in queue ~/queue.");
    tui.wait_for("queue: ~/queue");
}

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
    // A queue given by `--dir` is named by its directory. The sandbox is
    // the home folder, so the title names it `~/queue`.
    let none_waiting = first_title(&output, "~/queue · 0 waiting");
    let one_waiting = first_title(&output, "~/queue · 1 waiting");
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
        let mut tui = Tui::start(
            &sandbox,
            &["--dir", dir.to_str().expect("UTF-8")],
            "off\t1\t",
        );
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
fn a_viewer_that_exits_with_an_error_is_named_in_the_status_line() {
    let sandbox = Sandbox::new();
    let mut tui = Tui::start_with(
        &sandbox,
        &["--queue", "pty", "--no-notify", "--no-title"],
        PASSTHROUGH_ON,
        &[("VIEWER_EXIT", "3")],
    );
    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for("queue: pty");
    let file = sandbox.file(
        "coded.json",
        &SESSION.replace(
            r#""kind": "single","#,
            r#""kind": "single", "context": [{"type": "code", "content": "fn main() {}\n", "language": "rust"}],"#,
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

    tui.wait_for("cannot open code.rs: the opener exited with 3");
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);
}

#[test]
fn inline_code_reaches_the_viewer_as_a_file_in_the_temp_directory() {
    let sandbox = Sandbox::new();
    let mut tui = Tui::start(
        &sandbox,
        &["--queue", "pty", "--no-notify", "--no-title"],
        PASSTHROUGH_ON,
    );
    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for("queue: pty");
    let file = sandbox.file(
        "coded.json",
        &SESSION.replace(
            r#""kind": "single","#,
            r#""kind": "single", "context": [{"type": "code", "content": "fn main() {}\n", "language": "rust"}],"#,
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
    let recorded = viewed_path(&sandbox);
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let written = std::path::Path::new(recorded.trim());
    assert!(
        written.starts_with(sandbox.home().join("tmp")),
        "{}",
        written.display()
    );
    assert_eq!(
        written.file_name().and_then(|name| name.to_str()),
        Some("code.rs")
    );
    assert_eq!(
        std::fs::read_to_string(written).expect("kept after quitting"),
        "fn main() {}\n"
    );
}

/// The path the stand-in viewer recorded, once it has. It creates its file
/// before it writes the line, so only a whole line says that it is done.
fn viewed_path(sandbox: &Sandbox) -> String {
    let viewed = sandbox.home().join("viewed");
    let start = Instant::now();
    loop {
        let recorded = std::fs::read_to_string(&viewed).unwrap_or_default();
        if recorded.ends_with('\n') && !recorded.trim().is_empty() {
            return recorded;
        }
        assert!(
            start.elapsed() < PATIENCE,
            "the viewer never recorded a path"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn keys_still_arrive_after_the_terminal_answered_the_query() {
    let sandbox = Sandbox::new();
    let mut tui = Tui::start(
        &sandbox,
        &["--queue", "pty", "--no-notify", "--no-title"],
        PASSTHROUGH_ON,
    );
    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for("queue: pty");

    // Only a resize that reached the loop draws the size message, and
    // growing again brings the layout back.
    tui.resize(10, 40);
    tui.wait_for("asqr needs at least 60×15");
    tui.resize(24, 80);
    tui.wait_for("Nothing to answer");
    // `o` opens only a file that is there; the image does not decode,
    // which does not keep it from being opened.
    let image = sandbox.file("fruit.png", "not decoded here");
    let file = sandbox.file(
        "pictured.json",
        &SESSION.replace(
            r#""kind": "single","#,
            &format!(
                r#""kind": "single", "context": [{{"type": "image", "path": "{}"}}],"#,
                image.display()
            ),
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
    let recorded = viewed_path(&sandbox);
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    assert_eq!(recorded.trim(), image.display().to_string());
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
        PASSTHROUGH_ON,
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

/// How often [`an_answer_after_the_query_gave_up_picks_no_option`] starts
/// asqr anew when its answer missed the window asqr discards in.
const LATE_ANSWER_ATTEMPTS: usize = 3;

/// An answer that arrives just after the query gave up is no input: its
/// digits would pick an option.
///
/// asqr discards only what arrives within a short quiet time after the
/// query gave up, and whether the answer gets there depends on how
/// promptly the test runs. The log tells: an attempt proves something
/// only when asqr discarded the whole answer. One that missed the window
/// is started anew; a wrong screen after a whole discard fails at once.
#[test]
fn an_answer_after_the_query_gave_up_picks_no_option() {
    let discarded_all = format!(
        "discarded late answers to the graphics query bytes={}",
        KITTY_GRAPHICS.len()
    );
    for _ in 0..LATE_ANSWER_ATTEMPTS {
        let sandbox = Sandbox::new();
        let dir = sandbox.queue_dir();
        let file = sandbox.file("session.json", SESSION);
        sandbox
            .asqr_in_queue(&["ask"])
            .arg(&file)
            .assert()
            .success();
        let log = sandbox.log_path();
        let mut tui = Tui::start(
            &sandbox,
            &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
            PASSTHROUGH_ON,
        );

        // asqr logs this once the query gave up and before it starts
        // discarding, so the answer goes out at the start of the window.
        wait_for_log(&log, "no graphics protocol detected");
        tui.send(KITTY_GRAPHICS);
        // Logged once the discarding is over, whatever it took.
        wait_for_log(&log, "graphics protocol detected protocol=");
        if !read_log(&log).contains(&discarded_all) {
            continue;
        }

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
        return;
    }
    panic!(
        "the late answer never landed in asqr's discard window in {LATE_ANSWER_ATTEMPTS} attempts"
    );
}

/// The log at `log` so far; empty while asqr has not created it.
fn read_log(log: &std::path::Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

/// Waits until the log at `log` contains `text`. It polls often, as the
/// test acts on what asqr logged within the discard window.
fn wait_for_log(log: &std::path::Path, text: &str) {
    let start = Instant::now();
    while !read_log(log).contains(text) {
        assert!(
            start.elapsed() < PATIENCE,
            "{text:?} never showed in the log:\n{}",
            read_log(log)
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Over ssh from a tmux pane, `TERM` names tmux and `TMUX` is not there:
/// asqr cannot ask that tmux, and asks the terminal for its graphics
/// protocol at once, through tmux's passthrough. Only the query asks for a
/// status report.
/// The TUI pins its queue in the tmux session of its pane once it runs
/// (ADR 85), and the pin stays when it quits.
#[test]
fn the_tui_pins_its_queue_in_the_session_of_its_pane() {
    let sandbox = Sandbox::new();
    let mut tui = Tui::start_with(
        &sandbox,
        &["--queue", "work", "--no-notify"],
        PASSTHROUGH_ON,
        &[("TMUX_PANE", "%7")],
    );

    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for("Nothing to answer");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let commands =
        std::fs::read_to_string(sandbox.home().join(TMUX_COMMANDS)).expect("tmux was run");
    let pins: Vec<&str> = commands
        .lines()
        .filter(|command| command.starts_with("set-environment"))
        .collect();
    assert_eq!(pins, ["set-environment -t %7 ASQR_SESSION_QUEUE work"]);
}

/// Asks session `id`, `SESSION` under that id, into queue `queue`.
fn ask_into(sandbox: &Sandbox, queue: &str, id: &str) {
    let file = sandbox.file(
        &format!("{queue}-{id}.json"),
        &SESSION.replace("pty-test", id),
    );
    sandbox
        .asqr()
        .args(["--queue", queue, "ask"])
        .arg(&file)
        .assert()
        .success();
}

/// Starts asqr on queue `work` with `environment`, and switches with `w`,
/// `k` and `enter` to queue `other`, which holds a session.
fn switched_to_other(sandbox: &Sandbox, environment: &[(&str, &str)]) -> Tui {
    ask_into(sandbox, "other", "first");
    let mut tui = Tui::start_with(
        sandbox,
        &["--queue", "work", "--no-notify"],
        PASSTHROUGH_ON,
        environment,
    );
    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for("Nothing to answer");
    tui.press("w");
    tui.wait_for("◀ this asqr");
    // The rows: default, other, work; the cursor opens on work.
    tui.press("k");
    tui.send("\r");
    tui.wait_for("Pick a fruit");
    tui.wait_for("watching other");
    tui
}

/// `w` opens the queue switcher, `enter` on another queue switches to it
/// and pins it in the session of the pane (ADR 86), and the new queue's
/// sessions arrive from then on.
#[test]
fn w_switches_to_another_queue_and_pins_it() {
    let sandbox = Sandbox::new();
    let mut tui = switched_to_other(&sandbox, &[("TMUX_PANE", "%7")]);

    ask_into(&sandbox, "other", "second");
    tui.wait_for("other · 2 waiting");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let commands =
        std::fs::read_to_string(sandbox.home().join(TMUX_COMMANDS)).expect("tmux was run");
    let pins: Vec<&str> = commands
        .lines()
        .filter(|command| command.starts_with("set-environment"))
        .collect();
    assert_eq!(
        pins,
        [
            "set-environment -t %7 ASQR_SESSION_QUEUE work",
            "set-environment -t %7 ASQR_SESSION_QUEUE other"
        ]
    );
}

/// Outside tmux, with a queue besides `default` and none chosen, asqr
/// asks which queue to watch first (ADR 86); `default` is not taken.
#[test]
fn the_picker_watches_the_queue_picked() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "other", "first");
    let mut tui = Tui::start_with(&sandbox, &["--no-notify"], PASSTHROUGH_ON, &[("TMUX", "")]);

    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for("enter  watch");
    // The rows: default, other; the cursor opens on default.
    tui.press("j");
    tui.wait_for("tell your agents: --queue other");
    tui.send("\r");
    tui.wait_for("Pick a fruit");
    tui.wait_for("other · 1 waiting");
    ask_into(&sandbox, "other", "second");
    tui.wait_for("other · 2 waiting");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
}

/// A signal while the picker shows ends asqr as it ends the run, and the
/// terminal is given back.
#[test]
fn sigterm_while_the_picker_shows_gives_the_terminal_back() {
    let sandbox = Sandbox::new();
    ask_into(&sandbox, "other", "first");
    let mut tui = Tui::start_with(&sandbox, &[], PASSTHROUGH_ON, &[("TMUX", "")]);
    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for("enter  watch");
    let pid = tui.child.process_id().expect("asqr runs");

    let sent = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .expect("kill runs");
    assert!(sent.success());

    assert_eq!(tui.wait_for_exit(), 0);
    let output = tui.output();
    let main_screen = last(&output, "\x1b[?1049l");
    assert!(
        last(&output, "\x1b[?2004l") < main_screen,
        "bracketed paste off before the main screen: {output:?}"
    );
}

/// Without the pane, tmux would answer for another session: a switch
/// pins nothing.
#[test]
fn without_a_tmux_pane_a_switch_pins_nothing() {
    let sandbox = Sandbox::new();
    let mut tui = switched_to_other(&sandbox, &[]);
    assert!(
        !tui.screen().contains("tmux kept the old pin"),
        "no pin is tried, so none fails:\n{}",
        tui.screen()
    );

    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let commands = std::fs::read_to_string(sandbox.home().join(TMUX_COMMANDS)).unwrap_or_default();
    assert!(!commands.contains("set-environment"), "{commands}");
}

#[test]
fn over_ssh_from_a_tmux_pane_the_terminal_is_queried_through_the_passthrough() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start_with(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        PASSTHROUGH_ON,
        &[
            ("TMUX", ""),
            ("TERM", "tmux-256color"),
            ("TERM_PROGRAM", "pty-test"),
            ("WEZTERM_EXECUTABLE", ""),
        ],
    );

    tui.answer_the_query(NO_GRAPHICS);
    tui.wait_for("Nothing to answer");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let output = tui.output();
    assert!(output.contains("\x1bPtmux;"), "wrapped: {output:?}");
    assert_eq!(output.matches("[5n").count(), 1, "{output:?}");
    let commands =
        std::fs::read_to_string(sandbox.home().join(TMUX_COMMANDS)).expect("tmux was run");
    assert!(
        !commands.contains("display-message"),
        "that tmux is not asked: {commands}"
    );
}

/// What a terminal shows of asqr under `NO_COLOR` set to `no_color`.
struct Colours {
    /// The first cell of the title bar.
    title: vt100::Cell,
    /// The first cell of a notice in the status line.
    notice: vt100::Cell,
    /// The first cell of the title of the dialog of an invalid session.
    dialog: vt100::Cell,
    /// The first cell of inline code in the intro.
    intro_code: vt100::Cell,
    /// The first cell of the dim hint under the question.
    hint: vt100::Cell,
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
    let mut tui = Tui::start_with(
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

    // An invalid session is answered at once and opens a dialog, with its
    // line in the status line, until a key is pressed. It is written under
    // another name and renamed, so asqr never reads half of it. The
    // dialog's title holds the words "broken is invalid" too, so the
    // status line is found by the words only it has.
    let written = dir.join("inbox/.broken.part");
    std::fs::write(&written, "not json").expect("written");
    std::fs::rename(&written, dir.join("inbox/broken.json")).expect("renamed");
    tui.wait_for("Session broken is invalid");
    tui.wait_for("was answered with an error");
    assert_eq!(
        tui.row_of("was answered with an error"),
        tui.size.0 - 2,
        "the status line stays free"
    );
    let notice = tui.cell_of("! broken is invalid and was answered");
    let dialog = tui.cell_of("Session broken is invalid");
    tui.press("x");
    tui.wait_until_gone("Session broken is invalid");

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

    // The image loads apart from drawing and may show a moment later.
    let red = vt100::Color::Rgb(RED[0], RED[1], RED[2]);
    let shows_red = |tui: &Tui| {
        tui.cells()
            .iter()
            .any(|cell| cell.fgcolor() == red || cell.bgcolor() == red)
    };
    let start = Instant::now();
    while !shows_red(&tui) && start.elapsed() < PATIENCE {
        std::thread::sleep(Duration::from_millis(20));
    }
    Colours {
        title: tui.cell_of("asqr"),
        notice,
        dialog,
        intro_code: tui.cell_of("stalk"),
        hint: tui.cell_of("pick one"),
        red_image: shows_red(&tui),
    }
}

#[test]
fn no_color_takes_the_named_colours_and_keeps_modifiers_and_images() {
    let colourless = colours_under("1");
    let Colours {
        title,
        notice,
        dialog,
        intro_code,
        hint,
        red_image,
    } = &colourless;
    let default = (vt100::Color::Default, vt100::Color::Default);
    assert_eq!((title.fgcolor(), title.bgcolor()), default);
    assert!(title.bold(), "the name keeps its bold");
    assert_eq!((notice.fgcolor(), notice.bgcolor()), default);
    assert!(notice.bold(), "the mark keeps its bold");
    assert_eq!((dialog.fgcolor(), dialog.bgcolor()), default);
    assert!(dialog.bold(), "the dialog's title keeps its bold");
    assert_eq!((intro_code.fgcolor(), intro_code.bgcolor()), default);
    assert!(!intro_code.dim(), "the intro is in the text colour");
    assert_eq!((hint.fgcolor(), hint.bgcolor()), default);
    assert!(hint.dim(), "the hint keeps its dim");
    assert!(red_image, "the image keeps its colours");

    // An empty NO_COLOR counts as unset.
    let coloured = colours_under("");
    assert_eq!(coloured.title.fgcolor(), vt100::Color::Idx(6), "cyan");
    assert_eq!(coloured.notice.fgcolor(), vt100::Color::Idx(3), "yellow");
    assert_eq!(coloured.dialog.fgcolor(), vt100::Color::Idx(3), "yellow");
    assert_ne!(coloured.intro_code.fgcolor(), vt100::Color::Default);
    assert!(!coloured.intro_code.dim());
    assert!(coloured.hint.dim());
    assert!(coloured.red_image);
}

/// What a terminal shows of an `ansi` entry under `NO_COLOR` set to
/// `no_color`: the cells of its two words, and all output.
fn ansi_entry_under(no_color: &str) -> (vt100::Cell, vt100::Cell, String) {
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

    // A window title, a cleared screen, then an RGB and a bold named
    // colour. Only the colours may reach the terminal, as cells.
    let session = SESSION.replace(
        r#""kind": "single","#,
        r#""kind": "single", "context": [{"type": "ansi", "content":
            "\u001b]0;pwned\u0007\u001b[2J\u001b[38;2;255;0;0mzq-red\u001b[0m \u001b[1;34mzq-blue\u001b[0m"}],"#,
    );
    let file = sandbox.file("session.json", &session);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("zq-blue");

    (tui.cell_of("zq-red"), tui.cell_of("zq-blue"), tui.output())
}

#[test]
fn an_ansi_entry_reaches_the_terminal_as_cells_and_nothing_else() {
    let (red, blue, output) = ansi_entry_under("");
    assert_eq!(red.fgcolor(), vt100::Color::Rgb(255, 0, 0));
    assert_eq!(blue.fgcolor(), vt100::Color::Idx(4));
    assert!(blue.bold());
    for written in ["\x1b]0;", "\x1b]2;", "pwned"] {
        assert!(!output.contains(written), "{written:?} in {output:?}");
    }

    let (red, blue, output) = ansi_entry_under("1");
    let default = (vt100::Color::Default, vt100::Color::Default);
    assert_eq!((red.fgcolor(), red.bgcolor()), default);
    assert_eq!((blue.fgcolor(), blue.bgcolor()), default);
    assert!(blue.bold(), "the bold stays");
    assert_eq!(colour_sequences(&output), Vec::<&str>::new());
    assert!(!output.contains("pwned"), "{output:?}");
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

/// `sequence` wrapped for tmux's passthrough, with the escapes inside
/// doubled.
fn wrapped(sequence: &str) -> String {
    format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
}

/// Starts asqr in a terminal that answers the graphics query as one with
/// Kitty graphics and cells of 10×20 pixels, and has it show a session
/// with a red image.
fn showing_a_kitty_image(sandbox: &Sandbox) -> Tui {
    showing_a_kitty_image_with(sandbox, PASSTHROUGH_ON, &[])
}

/// Like [`showing_a_kitty_image`], inside a tmux that reports
/// `tmux_state`, with the variables in `environment` set.
fn showing_a_kitty_image_with(
    sandbox: &Sandbox,
    tmux_state: &str,
    environment: &[(&str, &str)],
) -> Tui {
    showing_a_kitty_image_before(sandbox, tmux_state, environment, None)
}

/// Like [`showing_a_kitty_image_with`], with `next`, a question in JSON,
/// after the question that shows the image.
fn showing_a_kitty_image_before(
    sandbox: &Sandbox,
    tmux_state: &str,
    environment: &[(&str, &str)],
    next: Option<&str>,
) -> Tui {
    let dir = sandbox.queue_dir();
    // asqr inherits the variables of the terminal the tests run in. A
    // `TERM_PROGRAM` of tmux makes ratatui-image wrap every command for
    // passthrough, and WezTerm's or Konsole's variable rules the Kitty
    // protocol out whatever the terminal answers (ratatui-image
    // 12.0.0-rc.3 `picker.rs` l.133-140), where empty counts as unset.
    let mut variables = vec![
        ("TERM_PROGRAM", "pty-test"),
        ("WEZTERM_EXECUTABLE", ""),
        ("KONSOLE_VERSION", ""),
    ];
    variables.extend_from_slice(environment);
    let mut tui = Tui::start_with(
        sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        tmux_state,
        &variables,
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
    // The session ends with the `]}` that closes its questions.
    let session = match next {
        Some(next) => format!("{}, {next}]}}", &session[..session.len() - 2]),
        None => session,
    };
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

/// The line that names the detected protocol also says whether asqr
/// compresses Kitty images, so a log tells what went out. asqr does not
/// ask the terminal for `o=z`, so it is `false`.
#[test]
fn the_detected_protocol_s_log_line_says_whether_kitty_images_are_compressed() {
    let sandbox = Sandbox::new();
    let mut tui = showing_a_kitty_image(&sandbox);
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);

    let log = read_log(&sandbox.log_path());
    let line = log
        .lines()
        .find(|line| line.contains("graphics protocol detected protocol="))
        .expect("the detected protocol is logged");
    assert!(line.contains("kitty_compression=false"), "{line}");
}

/// Without `ASQR_LOG`, the frame that sends a Kitty image leaves no line
/// in the log (ADR 83). asqr quits only once the image went out, so a
/// missing line is not a frame that never came.
#[test]
fn without_asqr_log_the_log_has_no_kitty_frame_lines() {
    let sandbox = Sandbox::new();
    let mut tui = showing_a_kitty_image(&sandbox);
    sent_kitty_image(&tui, 0);
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);

    let log = read_log(&sandbox.log_path());
    assert!(
        log.contains("graphics protocol detected protocol="),
        "{log}"
    );
    assert!(!log.contains("frame sent a Kitty image"), "{log}");
}

/// `ASQR_LOG=debug` brings the frame that sends a Kitty image into the
/// log, through the variable as asqr reads it.
#[test]
fn asqr_log_debug_writes_the_frames_that_send_a_kitty_image() {
    let sandbox = Sandbox::new();
    let mut tui = showing_a_kitty_image_with(&sandbox, PASSTHROUGH_ON, &[("ASQR_LOG", "debug")]);
    sent_kitty_image(&tui, 0);
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);

    let log = read_log(&sandbox.log_path());
    assert!(log.contains("frame sent a Kitty image"), "{log}");
}

/// Under the Kitty protocol, an image sent again on regaining the focus
/// has its old copy deleted after a frame that no longer shows it, and
/// the one shown when asqr quits is deleted before the title is given
/// back and asqr leaves the alternate screen (ADR 42). The new copy loads
/// behind the placeholder (ADR 46), so the old one may go before the new
/// one is sent.
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
        sent_again < again_deleted
            && old_deleted < again_deleted
            && again_deleted < pop
            && pop < main_screen,
        "{output:?}"
    );
}

/// Inside tmux 3.4 or later, asqr writes the deletes of its Kitty images
/// while its pane has `allow-passthrough all`, so they pass from a hidden
/// pane: the deletes, then the title given back and the main screen, and
/// only then the pane's own value back (ADRs 42 and 44).
#[test]
fn inside_tmux_the_deletes_on_quitting_go_out_before_the_pane_s_passthrough_is_put_back() {
    let sandbox = Sandbox::new();
    // `TERM` makes ratatui-image wrap its commands for tmux.
    let mut tui = showing_a_kitty_image_with(
        &sandbox,
        "on\t1\t\t1\t1\t0\t3.4",
        &[("TERM", "tmux-256color")],
    );
    let shown = sent_kitty_image(&tui, 0);
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let output = tui.output();
    let deleted = first(&output, &wrapped(&kitty_delete(shown)));
    let pop = last(&output, POP_TITLE);
    let main_screen = last(&output, "\x1b[?1049l");
    let all = first(&output, &tmux_set("-p allow-passthrough all"));
    let put_back = last(&output, &tmux_set("-p allow-passthrough on"));
    assert!(
        all < deleted && deleted < pop && pop < main_screen && main_screen < put_back,
        "{output:?}"
    );
}

/// A panic in the run loop gives the terminal back as quitting does: the
/// Kitty images are deleted and the title given back while the alternate
/// screen is on, focus reports and bracketed paste are turned off, and
/// the pane gets its passthrough back as the panic unwinds (ADRs 40, 42
/// and 44). Only a debug build of asqr panics on `o` when told to.
#[cfg(debug_assertions)]
#[test]
fn a_panic_deletes_the_kitty_images_and_gives_the_title_back_on_the_alternate_screen() {
    let sandbox = Sandbox::new();
    let mut tui = showing_a_kitty_image_with(
        &sandbox,
        "on\t1\t\t1\t1\t0\t3.4",
        &[("TERM", "tmux-256color"), ("ASQR_TEST_PANIC_ON_OPEN", "1")],
    );
    let shown = sent_kitty_image(&tui, 0);
    tui.press("o");

    assert_eq!(tui.wait_for_exit(), 101, "a panic");
    let output = tui.output();
    let deleted = first(&output, &wrapped(&kitty_delete(shown)));
    let pop = first(&output, POP_TITLE);
    let focus_off = first(&output, "\x1b[?1004l");
    let paste_off = first(&output, "\x1b[?2004l");
    let main_screen = first(&output, "\x1b[?1049l");
    let put_back = last(&output, &tmux_set("-p allow-passthrough on"));
    assert!(
        deleted < pop
            && pop < main_screen
            && focus_off < main_screen
            && paste_off < main_screen
            && main_screen < put_back,
        "{output:?}"
    );
    assert_eq!(output.matches(POP_TITLE).count(), 1, "{output:?}");
    assert_eq!(
        output.matches(&wrapped(&kitty_delete(shown))).count(),
        1,
        "deleted once: {output:?}"
    );
}

/// A panic while an entry loads, apart from drawing, gives the terminal
/// back as a panic in the run loop does, and once: the drawing thread
/// panics with the load's message (ADR 46). Only a debug build of asqr
/// panics on loading the entry the variable names.
#[cfg(debug_assertions)]
#[test]
fn a_panic_while_loading_gives_the_terminal_back_once() {
    let sandbox = Sandbox::new();
    let doomed = sandbox.home().join("doomed.png");
    let doomed = doomed.to_str().expect("UTF-8");
    // The doomed entry is two questions on, outside the window of the
    // first, so the image shows before anything loads it. The question
    // between has no pane to load ahead for; the one after it loads the
    // entry as it shows it.
    let next = format!(
        r#"{{"id": "next", "text": "And this one", "kind": "single",
            "options": [{{"id": "x", "label": "X"}}]}},
           {{"id": "last", "text": "And the last", "kind": "text",
            "context": [{{"type": "image", "path": {doomed:?}}}]}}"#
    );
    let mut tui = showing_a_kitty_image_before(
        &sandbox,
        "on\t1\t\t1\t1\t0\t3.4",
        &[
            ("TERM", "tmux-256color"),
            ("ASQR_TEST_PANIC_ON_LOAD", doomed),
        ],
        Some(&next),
    );
    let shown = sent_kitty_image(&tui, 0);
    tui.press("l");
    tui.wait_for("And this one");
    tui.press("l");

    assert_eq!(tui.wait_for_exit(), 101, "a panic");
    let output = tui.output();
    let deleted = first(&output, &wrapped(&kitty_delete(shown)));
    let pop = first(&output, POP_TITLE);
    let main_screen = first(&output, "\x1b[?1049l");
    let put_back = last(&output, &tmux_set("-p allow-passthrough on"));
    assert!(
        deleted < pop && pop < main_screen && main_screen < put_back,
        "{output:?}"
    );
    assert_eq!(output.matches(POP_TITLE).count(), 1, "{output:?}");
    assert_eq!(
        output.matches("ASQR_TEST_PANIC_ON_LOAD names").count(),
        1,
        "the message once: {output:?}"
    );
}

/// An entry that takes longer than the pane waits shows `loading…`, and
/// then itself once loaded, with no key pressed: the load wakes asqr.
#[test]
fn a_slow_entry_shows_loading_then_itself() {
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
        "off\t1\t",
        &[],
    );
    tui.wait_for("Nothing to answer");

    // Highlighting a thousand lines of a log takes a debug build well
    // over the pane's 100 ms.
    let log = sandbox.home().join("service.log");
    let lines: String = (1..=1000)
        .map(|n| format!("2026-10-16T10:00:00+02:00 INFO request {n:06} served\n"))
        .collect();
    std::fs::write(&log, lines).expect("log is written");
    let session = SESSION.replace(
        r#""kind": "single","#,
        &format!(
            r#""kind": "single", "context": [{{"type": "code", "path": {:?}}}],"#,
            log.to_str().expect("UTF-8")
        ),
    );
    let file = sandbox.file("session.json", &session);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();

    tui.wait_for("loading…");
    tui.wait_for("request 000001 served");
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);
}

/// A Markdown entry the Markdown parser panics on shows as plain text
/// below a note, and asqr goes on. `markdown` 1.0.0 panics on an unclosed
/// fence in a numbered item that a bullet follows; where a later version
/// handles that text, another text that panics takes its place here, and
/// the tests of the viewer keep the fallback pinned with a renderer that
/// panics on any text.
#[test]
fn a_markdown_entry_the_parser_panics_on_shows_as_plain_text_and_asqr_goes_on() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start_with(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        "off\t1\t",
        &[],
    );
    tui.wait_for("Nothing to answer");
    let session = SESSION.replace(
        r#""kind": "single","#,
        r#""kind": "single", "context": [{"type": "markdown", "content": "1. ~~~\n*"}],"#,
    );
    let file = sandbox.file("session.json", &session);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();

    tui.wait_for("Markdown failed: shown as plain text");
    tui.wait_for("1. ~~~");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let log = read_log(&sandbox.log_path());
    assert!(
        log.contains("the Markdown parser panicked; showing the text as plain text"),
        "{log}"
    );
    assert!(!log.contains("1. ~~~"), "the log holds no text: {log}");
}

/// The text of a question that the Markdown parser panics on shows as
/// written, and asqr goes on. The texts of the fields render on the
/// drawing thread, every frame; the log gets one warning for the text. See
/// the test above for the text.
#[test]
fn a_question_text_the_parser_panics_on_shows_as_written_and_asqr_goes_on() {
    let sandbox = Sandbox::new();
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start_with(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        "off\t1\t",
        &[],
    );
    tui.wait_for("Nothing to answer");
    let session = SESSION.replace("Pick a fruit", "1. ~~~\\n*");
    let file = sandbox.file("session.json", &session);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();

    tui.wait_for("1. ~~~");
    tui.press("j");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let log = read_log(&sandbox.log_path());
    assert_eq!(
        log.matches("the Markdown parser panicked; showing the text as plain text")
            .count(),
        1,
        "{log}"
    );
}

/// Inside tmux 3.4 or later, asqr sets `allow-passthrough all` for its
/// pane once the terminal answered as one with Kitty graphics, so images
/// and their deletes pass while the pane is hidden, and puts the pane's
/// own value back when it quits. A terminal without Kitty graphics leaves
/// the pane alone.
#[test]
fn inside_tmux_kitty_images_pass_from_a_hidden_pane_until_asqr_quits() {
    for (answer, all) in [(KITTY_GRAPHICS, true), (NO_GRAPHICS, false)] {
        let sandbox = Sandbox::new();
        let dir = sandbox.queue_dir();
        // `TERM` makes ratatui-image wrap its commands for tmux.
        let mut tui = Tui::start_with(
            &sandbox,
            &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
            "on\t1\t\t1\t1\t0\t3.4",
            &[
                ("TERM", "tmux-256color"),
                ("TERM_PROGRAM", "pty-test"),
                ("WEZTERM_EXECUTABLE", ""),
                ("KONSOLE_VERSION", ""),
            ],
        );
        tui.answer_the_query(answer);
        tui.wait_for("Nothing to answer");
        tui.press("q");
        assert_eq!(tui.wait_for_exit(), 0);

        let commands =
            std::fs::read_to_string(sandbox.home().join(TMUX_COMMANDS)).expect("tmux was run");
        let sets: Vec<&str> = commands
            .lines()
            .filter(|command| command.starts_with("set "))
            .collect();
        // ratatui-image sets `on` while it queries, and the stand-in shows
        // `on` as the pane's own value, which asqr puts back.
        let expected: &[&str] = if all {
            &[
                "set -p allow-passthrough on",
                "set -p allow-passthrough all",
                "set -p allow-passthrough on",
            ]
        } else {
            &["set -p allow-passthrough on", "set -p allow-passthrough on"]
        };
        assert_eq!(sets, expected, "{answer:?}: {commands}");
    }
}

/// Starts asqr inside a stand-in tmux whose one client answered XTVERSION
/// with `termtype`, asks the session of [`SESSION`] and waits until it
/// shows.
fn answering_under(sandbox: &Sandbox, termtype: &str) -> Tui {
    let dir = sandbox.queue_dir();
    let tui = Tui::start(
        sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        &format!("off\t1\t{termtype}"),
    );
    tui.wait_for("Nothing to answer");
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("Pick a fruit");
    tui
}

/// Waits until `tui`'s output holds `needle` at least `count` times.
fn wait_for_output(tui: &Tui, needle: &str, count: usize) {
    let start = Instant::now();
    while tui.output().matches(needle).count() < count {
        assert!(
            start.elapsed() < PATIENCE,
            "{needle:?} never came {count} times: {:?}",
            tui.output()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Ghostty inside tmux gets the progress in the passthrough wrapper, again
/// every second while it shows, as it clears it after a while without an
/// update, and its removal before asqr leaves the alternate screen.
#[test]
fn progress_reaches_a_terminal_that_shows_it_through_tmux() {
    let sandbox = Sandbox::new();
    let mut tui = answering_under(&sandbox, "ghostty 1.2.0");
    let set = wrapped("\x1b]9;4;1;0\x1b\\");

    // No key is pressed: the second one is the keep-alive's.
    wait_for_output(&tui, &set, 2);
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let output = tui.output();
    let removed = last(&output, &wrapped("\x1b]9;4;0\x1b\\"));
    assert!(
        last(&output, &set) < removed && removed < last(&output, "\x1b[?1049l"),
        "{output:?}"
    );
}

/// What the stand-in editor writes to the terminal when it starts: an OSC
/// sequence no terminal acts on.
const EDITOR_STARTED: &str = "\x1b]7777;editor\x07";

/// `ctrl-g` hands the terminal to the editor `VISUAL` names: a script that
/// reads one line from the terminal and puts it on top of the file. That
/// the line reaches the script, and not asqr, shows that asqr's key reader
/// paused; asqr leaves the alternate screen for the editor and enters it
/// again after.
#[test]
fn ctrl_g_edits_the_own_answer_in_the_editor() {
    let sandbox = Sandbox::new();
    let bin = sandbox.home().join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    let editor = bin.join("stand-in-editor");
    std::fs::write(
        &editor,
        "#!/bin/sh\n\
         printf '\\033]7777;editor\\007' > /dev/tty\n\
         IFS= read -r line < /dev/tty\n\
         { printf '%s\\n' \"$line\"; cat \"$1\"; } > \"$1.new\" && mv \"$1.new\" \"$1\"\n",
    )
    .expect("script");
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let dir = sandbox.queue_dir();
    let mut tui = Tui::start_with(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "--no-notify"],
        "off\t1\t",
        &[("VISUAL", editor.to_str().expect("UTF-8"))],
    );
    tui.wait_for("Nothing to answer");
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("Pick a fruit");

    tui.press("jjx");
    tui.wait_for("Own answer: x");
    tui.send("\x07");
    wait_for_output(&tui, EDITOR_STARTED, 1);
    tui.press("typed in the editor\r");
    tui.wait_for("Own answer: typed in the editor");

    let output = tui.output();
    let started = first(&output, EDITOR_STARTED);
    assert!(
        output[..started].contains("\x1b[?1049l") && output[started..].contains("\x1b[?1049h"),
        "{output:?}"
    );
    // Pick the own answer, which moves on to the review, and submit.
    tui.press("\r");
    tui.wait_for("Review your answers");
    tui.press("\r");
    tui.wait_for("Nothing to answer");
    tui.press("q");
    assert_eq!(tui.wait_for_exit(), 0);
    let result: SessionResult = serde_json::from_slice(
        &std::fs::read(sandbox.queue_dir().join("outbox/pty-test.json")).expect("result"),
    )
    .expect("parses");
    assert_eq!(
        result.answers[0].custom.as_deref(),
        Some("typed in the editor\nx")
    );
}

#[test]
fn a_terminal_not_known_to_show_progress_gets_none() {
    let sandbox = Sandbox::new();
    let mut tui = answering_under(&sandbox, "");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    assert!(!tui.output().contains("]9;4"), "{:?}", tui.output());
}

// =========================================================
// A replay
// =========================================================

/// A queue in the sandbox with one finished session, answered `Sent`; its
/// reference.
fn finished_session(sandbox: &Sandbox) -> String {
    use asqr::queue::{Archived, QueueLocation, archive_name, session_sha256};

    let location = QueueLocation::at(sandbox.queue_dir());
    location.create_layout().expect("layout");
    let body = r#"{"asqr": 2, "id": "old-1", "title": "An old question", "questions": [{"id": "q",
        "text": "Pick a fruit", "kind": "single",
        "options": [{"id": "a", "label": "Apples"}, {"id": "b", "label": "Bananas"}]}]}"#;
    let ulid = ulid::Ulid::from_parts(1_788_000_000_000, 0x1234_5678_9abc);
    std::fs::write(
        location
            .archive()
            .join(archive_name("old-1", ulid, Archived::Session)),
        body,
    )
    .expect("written");
    let result = SessionResult::submitted(
        "old-1",
        &session_sha256(body.as_bytes()),
        "2026-08-29T10:39:00+00:00",
        None,
        vec![asqr::format::Answer {
            selected: vec!["b".to_owned()],
            ..asqr::format::Answer::new("q")
        }],
    );
    std::fs::write(
        location.outbox().join("old-1.json"),
        serde_json::to_vec_pretty(&result).expect("serialises"),
    )
    .expect("written");
    let text = ulid.to_string();
    text[text.len() - 8..].to_owned()
}

#[test]
fn a_replay_opens_read_only_counts_what_waits_and_leaves_the_queue_alone() {
    let sandbox = Sandbox::new();
    let reference = finished_session(&sandbox);
    let dir = sandbox.queue_dir();
    let file = sandbox.file("session.json", SESSION);
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    let listing = |dir: &std::path::Path| {
        let mut names = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for entry in std::fs::read_dir(&next).expect("listed") {
                let path = entry.expect("entry").path();
                names.push(path.clone());
                if path.is_dir() {
                    stack.push(path);
                }
            }
        }
        names.sort();
        names
    };
    let before = listing(&dir);

    let mut tui = Tui::start(
        &sandbox,
        &["--dir", dir.to_str().expect("UTF-8"), "replay", &reference],
        "off\t1\t",
    );
    tui.wait_for("◷ read-only");
    tui.wait_for("Pick a fruit");
    tui.wait_for(&format!("replay {reference}"));
    tui.wait_for("1 waiting");
    tui.press("1");
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    let output = tui.output();
    assert!(
        output.contains(&format!("asqr replay {reference}")),
        "{output:?}"
    );
    assert_eq!(
        listing(&dir),
        before,
        "a replay writes nothing into the queue"
    );
}

/// Finishes a session that shows a file and quits at once, with the keeping
/// worker held until the file `gate` names exists (`ASQR_TEST_KEEP_GATE`),
/// so the job is still running when asqr quits whatever the machine's
/// speed.
fn finished_with_the_keeping_held(
    sandbox: &Sandbox,
    gate: &std::path::Path,
) -> (Tui, std::path::PathBuf) {
    let dir = sandbox.queue_dir();
    let shown = sandbox.file("notes.txt", "what the person saw\n");
    let session = serde_json::json!({
        "asqr": 2, "id": "held",
        "questions": [{"id": "q", "text": "Pick a fruit", "kind": "single",
            "context": [{"type": "code", "path": shown}],
            "options": [{"id": "a", "label": "Apple"}, {"id": "b", "label": "Pear"}]}]
    });
    let file = sandbox.file("session.json", &session.to_string());
    let mut tui = Tui::start_with(
        sandbox,
        &["--dir", dir.to_str().expect("UTF-8")],
        "off\t1\t",
        &[("ASQR_TEST_KEEP_GATE", gate.to_str().expect("UTF-8"))],
    );
    tui.wait_for("Nothing to answer");
    sandbox
        .asqr_in_queue(&["ask"])
        .arg(&file)
        .assert()
        .success();
    tui.wait_for("Pick a fruit");
    tui.press("1");
    tui.wait_for("Review your answers");
    tui.press("\r");
    tui.wait_for("Nothing to answer");
    tui.press("q");
    (tui, dir)
}

/// The one archived session of the queue at `dir`, as text.
fn archived_text(dir: &std::path::Path) -> String {
    let archived = std::fs::read_dir(dir.join("archive"))
        .expect("archive")
        .map(|entry| entry.expect("entry").path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .expect("an archived session");
    std::fs::read_to_string(archived).expect("reads")
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the gate that holds the keeping job exists in debug builds only"
)]
fn quitting_names_the_files_still_being_kept_on_the_normal_screen_and_waits_for_them() {
    let sandbox = Sandbox::new();
    let gate = sandbox.home().join("keep-gate");
    let (mut tui, dir) = finished_with_the_keeping_held(&sandbox, &gate);

    // The job is held, so the notice is the only thing asqr can be doing.
    tui.wait_for("keeping the files of 1 finished session, ctrl-c lets go of it...");
    assert!(
        !archived_text(&dir).contains("\"archived\""),
        "nothing is kept yet"
    );
    std::fs::write(&gate, "").expect("the gate opens");

    assert_eq!(tui.wait_for_exit(), 0);
    let output = tui.output();
    let left = last(&output, "\x1b[?1049l");
    assert!(
        output[left..].contains("keeping the files of 1 finished session"),
        "the line comes after the terminal was given back: {output:?}"
    );
    assert!(
        archived_text(&dir).contains("\"archived\""),
        "quit waited for the job"
    );
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the gate that holds the keeping job exists in debug builds only"
)]
fn ctrl_c_while_the_files_are_kept_lets_go_of_them() {
    let sandbox = Sandbox::new();
    let gate = sandbox.home().join("keep-gate");
    let (mut tui, dir) = finished_with_the_keeping_held(&sandbox, &gate);
    tui.wait_for("keeping the files of 1 finished session");

    let sent = Instant::now();
    tui.send("\x03");

    assert_eq!(tui.wait_for_exit(), 0);
    assert!(
        sent.elapsed() < Duration::from_secs(5),
        "ctrl-c ended the wait, which would otherwise take the 10 s out"
    );
    assert!(
        !archived_text(&dir).contains("\"archived\""),
        "the unfinished session stays plain"
    );
}
