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

const SESSION: &str = r#"{"asqr": 1, "id": "pty-test", "questions": [{"id": "q",
    "text": "Pick a fruit", "kind": "single",
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
        command.env("TERM", "xterm-256color");
        command.env("TMUX", "/tmp/tmux-test/default,1,0");
        let path = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{}:{path}", bin.display()));
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
        let mut parser = vt100::Parser::new(self.size.0, self.size.1, 0);
        parser.process(&self.output.lock().expect("not poisoned"));
        parser.screen().contents()
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
    // it comes back into view (spec section 7.7); a report is no key.
    assert!(
        tui.output().contains("\x1b[?1004h"),
        "focus reporting is on"
    );
    tui.press("\x1b[I");

    // Pick the second option, which moves on to the review, where the
    // cursor rests on Submit.
    tui.press("2");
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
    let result: SessionResult = serde_json::from_slice(
        &std::fs::read(sandbox.queue_dir().join("outbox/pty-test.json")).expect("result"),
    )
    .expect("parses");
    assert_eq!(result.status, Status::Submitted);
    assert_eq!(result.answers[0].selected, ["b"]);
}

#[test]
fn keys_still_arrive_after_the_terminal_answered_the_query() {
    let sandbox = Sandbox::new();
    let mut tui = Tui::start(&sandbox, &["--queue", "pty", "--no-notify"], "on 1");
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
            r#""kind": "single", "image": "/pictures/fruit.png","#,
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
    let viewed = sandbox.home().join("viewed");
    let start = Instant::now();
    while !viewed.exists() {
        assert!(start.elapsed() < PATIENCE, "the viewer was never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    tui.press("q");

    assert_eq!(tui.wait_for_exit(), 0);
    // The viewer may still be writing when the file appears.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        std::fs::read_to_string(viewed).expect("recorded").trim(),
        "/pictures/fruit.png"
    );
    assert!(
        !tui.output().contains("]9;"),
        "--no-notify sends no notification"
    );
}
