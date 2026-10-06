// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The command line (spec section 8). The binary parses the arguments and
//! hands them to [`run`]; everything else happens here, so the commands
//! are reachable from tests.

mod ask;
mod exit;
mod format;
mod log;
mod paths;
mod prune;
mod skill;
mod status;
mod wait;
mod watch;

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};

pub use exit::Exit;
pub use watch::Watch;

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

    /// Without a command, asqr answers sessions like `asqr watch`.
    #[command(flatten)]
    watch: WatchArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Default, Args)]
struct WatchArgs {
    /// Do not send desktop notifications when sessions arrive.
    #[arg(long)]
    no_notify: bool,

    /// Do not ring the terminal bell when sessions arrive.
    #[arg(long)]
    no_bell: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Answer waiting sessions in the terminal (the default).
    Watch(WatchArgs),

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

    /// Wait for the result of a session and print it.
    Wait {
        /// The session id.
        id: String,

        /// Stop waiting after this many seconds (exit code 12).
        #[arg(long, value_name = "SECONDS")]
        timeout: Option<u64>,
    },

    /// Print the result of a session if it is there.
    Result {
        /// The session id.
        id: String,
    },

    /// List waiting and answered sessions and who watches the queue.
    Status {
        /// Print the status as JSON.
        #[arg(long)]
        json: bool,

        /// List at most this many answered sessions, the newest first.
        #[arg(long, value_name = "COUNT", default_value_t = status::DEFAULT_LIMIT)]
        limit: usize,

        /// List every answered session.
        #[arg(long, conflicts_with = "limit")]
        all: bool,

        /// List only sessions answered within this age, such as 2h or 1d.
        #[arg(long, value_name = "AGE", value_parser = prune::parse_age)]
        since: Option<Duration>,
    },

    /// Remove archived sessions, and with --results old results too.
    Prune {
        /// Remove what is older than this age, such as 30d, 12h or 2w.
        #[arg(long, value_name = "AGE", value_parser = prune::parse_age)]
        older_than: Duration,

        /// Also remove results in the outbox, unread ones included.
        #[arg(long)]
        results: bool,
    },

    /// Print the agent skill, or install it under a skills directory.
    Skill {
        /// Write it to <DIR>/asqr/SKILL.md, for example .claude/skills.
        #[arg(long, value_name = "DIR")]
        install: Option<PathBuf>,
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
/// over the environment is decided in one place. `terminal_ui` runs the
/// TUI; only the binary has a terminal to hand it.
pub fn run(cli: Cli, environment: Selection, terminal_ui: impl FnOnce(Watch) -> Exit) -> Exit {
    log::init();
    dispatch(cli, environment, terminal_ui)
}

fn dispatch(cli: Cli, environment: Selection, terminal_ui: impl FnOnce(Watch) -> Exit) -> Exit {
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

    // Plain `asqr` takes the watch flags, so they may stand before
    // `watch` too; next to any other command they would do nothing.
    let before = cli.watch;
    let command = cli.command.unwrap_or(Command::Watch(WatchArgs::default()));
    if !matches!(command, Command::Watch(_)) && (before.no_notify || before.no_bell) {
        eprintln!("error: --no-notify and --no-bell only apply to watching the queue");
        return Exit::Usage;
    }

    match command {
        Command::Watch(after) => {
            let args = WatchArgs {
                no_notify: before.no_notify || after.no_notify,
                no_bell: before.no_bell || after.no_bell,
            };
            watch::run(location, &args, terminal_ui)
        }
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
        Command::Wait { id, timeout } => {
            wait::run_wait(&location, &id, timeout.map(Duration::from_secs))
        }
        Command::Result { id } => wait::run_result(&location, &id),
        Command::Status {
            json,
            limit,
            all,
            since,
        } => status::run(
            &location,
            status::StatusOptions {
                json,
                limit: (!all).then_some(limit),
                since,
            },
        ),
        Command::Prune {
            older_than,
            results,
        } => prune::run(&location, older_than, results),
        Command::Skill { install } => skill::run(install.as_deref()),
        Command::Validate { file } => format::run_validate(&file),
        Command::Schema { result } => format::run_schema(result),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::queue::{lock_holder, lock_queue};
    use crate::tui::Alerts;

    /// Runs `args` against a queue in `dir` and returns the exit and what
    /// the terminal UI was started with, if it was.
    fn watched(dir: &Path, args: &[&str]) -> (Exit, Option<Alerts>) {
        let mut argv = vec!["asqr", "--dir", dir.to_str().expect("UTF-8 temp path")];
        argv.extend(args);
        let cli = Cli::try_parse_from(argv).expect("parses");
        let mut started = None;
        let exit = dispatch(cli, Selection::default(), |watch| {
            let location = watch.location();
            assert!(location.inbox().is_dir(), "the layout exists");
            assert!(
                lock_holder(location).expect("readable").is_some(),
                "the TUI runs with the queue locked"
            );
            started = Some(watch.alerts());
            Exit::Success
        });
        (exit, started)
    }

    const ALL: Alerts = Alerts {
        notify: true,
        bell: true,
    };

    #[test]
    fn without_a_command_asqr_watches_with_every_alert() {
        let scratch = tempfile::tempdir().expect("temp dir");

        assert_eq!(watched(scratch.path(), &[]), (Exit::Success, Some(ALL)));
        assert_eq!(
            watched(scratch.path(), &["watch"]),
            (Exit::Success, Some(ALL))
        );
        assert!(
            lock_holder(&QueueLocation::at(scratch.path()))
                .expect("readable")
                .is_none(),
            "the lock ends with the TUI"
        );
    }

    #[test]
    fn alerts_can_be_switched_off_with_or_without_the_command() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let no_bell = Alerts { bell: false, ..ALL };
        let neither = Alerts {
            notify: false,
            bell: false,
        };

        assert_eq!(watched(scratch.path(), &["--no-bell"]).1, Some(no_bell));
        assert_eq!(
            watched(scratch.path(), &["watch", "--no-notify", "--no-bell"]).1,
            Some(neither)
        );
    }

    #[test]
    fn watch_flags_belong_to_watching_only() {
        let scratch = tempfile::tempdir().expect("temp dir");

        assert_eq!(
            watched(scratch.path(), &["--no-bell", "status"]),
            (Exit::Usage, None)
        );
    }

    #[test]
    fn watch_flags_before_and_after_the_command_add_up() {
        let scratch = tempfile::tempdir().expect("temp dir");

        assert_eq!(
            watched(scratch.path(), &["--no-bell", "watch", "--no-notify"]).1,
            Some(Alerts {
                notify: false,
                bell: false
            })
        );
    }

    #[test]
    fn a_queue_someone_else_watches_is_refused() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let _held = lock_queue(&QueueLocation::at(scratch.path())).expect("locked");

        assert_eq!(watched(scratch.path(), &[]), (Exit::Failure, None));
    }

    #[test]
    fn a_queue_that_cannot_be_set_up_is_a_failure() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let blocked = scratch.path().join("blocked");
        std::fs::write(&blocked, "a file where the queue should be").expect("written");

        assert_eq!(watched(&blocked, &[]), (Exit::Failure, None));
    }
}
