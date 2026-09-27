// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The command line (spec section 8). The binary parses the arguments and
//! hands them to [`run`]; everything else happens here, so the commands
//! are reachable from tests.

mod ask;
mod exit;
mod format;
mod paths;
mod wait;

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};

pub use exit::Exit;

use crate::queue::{QueueLocation, Selection, platform_data_dir};

/// Answer questions an AI agent or a script prepared for you, right in your
/// terminal.
#[derive(Debug, Parser)]
#[command(name = "asqr", version, about)]
pub struct Cli {
    /// Work on the named queue under the platform data directory.
    #[arg(long, global = true, value_name = "NAME")]
    queue: Option<String>,

    /// Work on the queue in this directory (the one holding inbox/).
    #[arg(long, global = true, value_name = "PATH")]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print where the queue and its files are.
    Paths {
        /// Print the paths as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Print a session skeleton with a fresh id.
    New,

    /// Check a session file and report errors and warnings.
    Validate {
        /// The session file.
        file: PathBuf,
    },

    /// Drop a session file into the queue and print its id.
    Ask {
        /// The session file.
        file: PathBuf,

        /// Wait for the result and print it instead of the id.
        #[arg(long)]
        wait: bool,

        /// Stop waiting after this many seconds (exit code 12).
        #[arg(long, value_name = "SECONDS", requires = "wait")]
        timeout: Option<u64>,

        /// Move an unread result with the same id into the archive first.
        #[arg(long)]
        force: bool,
    },

    /// Print the JSON Schema of session files.
    Schema {
        /// Print the schema of result files instead.
        #[arg(long)]
        result: bool,
    },
}

/// The queue the environment chooses: `ASQR_QUEUE` or `ASQR_DIR`. Empty
/// values count as unset.
pub fn environment_selection() -> Selection {
    let variable = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    Selection {
        queue: variable("ASQR_QUEUE"),
        dir: variable("ASQR_DIR").map(PathBuf::from),
    }
}

/// Runs the parsed command line. `environment` is what
/// [`environment_selection`] read, passed in so the precedence of flags
/// over the environment is decided in one place.
pub fn run(cli: Cli, environment: Selection) -> Exit {
    let flags = Selection {
        queue: cli.queue,
        dir: cli.dir,
    };
    let location = match QueueLocation::resolve(flags, environment, platform_data_dir()) {
        Ok(location) => location,
        Err(error) => {
            eprintln!("error: {error}");
            return Exit::Usage;
        }
    };

    match cli.command {
        Command::Paths { json } => paths::run(&location, json),
        Command::New => format::run_new(),
        Command::Ask {
            file,
            wait,
            timeout,
            force,
        } => ask::run(
            &location,
            &file,
            ask::AskOptions {
                wait,
                timeout: timeout.map(Duration::from_secs),
                force,
            },
        ),
        Command::Validate { file } => format::run_validate(&file),
        Command::Schema { result } => format::run_schema(result),
    }
}
