// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared setup of the command-line tests: every test runs the `asqr`
//! binary against its own temporary home, so the platform data and cache
//! directories, and with them the default queue and the log, never touch
//! the real ones of the machine running the tests.

// Each test binary uses a different part of this module.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;

pub struct Sandbox {
    home: tempfile::TempDir,
}

impl Sandbox {
    pub fn new() -> Self {
        Sandbox {
            home: tempfile::tempdir().expect("temp home"),
        }
    }

    pub fn home(&self) -> &Path {
        self.home.path()
    }

    /// A queue directory inside the sandbox, for `--dir`.
    pub fn queue_dir(&self) -> PathBuf {
        self.home.path().join("queue")
    }

    /// `asqr` with the sandbox as home and no queue chosen by the
    /// environment of the machine running the tests.
    pub fn asqr(&self) -> Command {
        let mut command = Command::cargo_bin("asqr").expect("the asqr binary is built");
        command
            .env("HOME", self.home.path())
            .env("XDG_DATA_HOME", self.home.path().join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.path().join(".cache"))
            .env_remove("ASQR_QUEUE")
            .env_remove("ASQR_DIR");
        command
    }

    /// `asqr` as a plain process with the same environment, for tests that
    /// run it in the background while they act as the answering side.
    pub fn spawnable_asqr(&self) -> std::process::Command {
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin("asqr"));
        command
            .env("HOME", self.home.path())
            .env("XDG_DATA_HOME", self.home.path().join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.path().join(".cache"))
            .env_remove("ASQR_QUEUE")
            .env_remove("ASQR_DIR");
        command
    }

    /// `asqr --dir <sandbox queue> <args>`.
    pub fn asqr_in_queue(&self, args: &[&str]) -> Command {
        let mut command = self.asqr();
        command.arg("--dir").arg(self.queue_dir()).args(args);
        command
    }

    /// Writes a file into the sandbox and returns its path.
    pub fn file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.home.path().join(name);
        std::fs::create_dir_all(path.parent().expect("file has a directory")).expect("directory");
        std::fs::write(&path, content).expect("file is written");
        path
    }
}

pub fn stdout(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

pub fn stderr(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}
