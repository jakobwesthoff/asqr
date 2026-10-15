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

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};

pub use exit::Exit;
pub use watch::Watch;

use crate::queue::{QueueLocation, Selection, platform_data_dir};
use crate::tui::render::PaneLayout;

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

    /// Do not show the number of waiting sessions in the terminal title.
    #[arg(long)]
    no_title: bool,

    /// Show context beside or below the question; auto, the default,
    /// picks by the terminal's shape. ASQR_LAYOUT sets it too.
    #[arg(long, value_enum, value_name = "LAYOUT")]
    layout: Option<PaneLayout>,
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

    /// Print a session skeleton to start from.
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

/// The settings the environment makes. Empty values count as unset.
#[derive(Debug, Default)]
pub struct Environment {
    /// The queue: `ASQR_QUEUE` or `ASQR_DIR`.
    pub selection: Selection,
    /// `ASQR_LAYOUT` as it is set. It is checked only when asqr watches,
    /// so a bad value never stands in the way of the other commands.
    pub layout: Option<String>,
    /// `NO_COLOR` (<https://no-color.org>): the TUI draws without colours.
    pub no_color: bool,
}

/// Reads the [`Environment`] of this process.
pub fn environment() -> Environment {
    environment_from(|name| std::env::var_os(name))
}

/// Reads the [`Environment`] through `lookup`, which gives a variable's
/// value by its name, so tests need not change the process environment.
fn environment_from(lookup: impl Fn(&str) -> Option<OsString>) -> Environment {
    // asqr's own variables are read as UTF-8 text, and one that is not
    // counts as unset.
    let variable = |name: &str| {
        lookup(name)
            .and_then(|value| value.into_string().ok())
            .filter(|value| !value.is_empty())
    };
    Environment {
        selection: Selection {
            queue: variable("ASQR_QUEUE"),
            dir: variable("ASQR_DIR").map(PathBuf::from),
        },
        layout: variable("ASQR_LAYOUT"),
        // no-color.org asks only that the variable is present and not
        // empty, so any value counts, also one that is not UTF-8.
        no_color: lookup("NO_COLOR").is_some_and(|value| !value.is_empty()),
    }
}

/// Runs the parsed command line. `environment` is what [`environment`]
/// read, passed in so the precedence of flags over the environment is
/// decided in one place. `terminal_ui` runs the TUI; only the binary has a
/// terminal to hand it.
pub fn run(cli: Cli, environment: Environment, terminal_ui: impl FnOnce(Watch) -> Exit) -> Exit {
    log::init();
    dispatch(cli, environment, terminal_ui)
}

fn dispatch(cli: Cli, environment: Environment, terminal_ui: impl FnOnce(Watch) -> Exit) -> Exit {
    let flags = Selection {
        queue: cli.queue,
        dir: cli.dir,
    };
    let location = match QueueLocation::resolve(flags, environment.selection, platform_data_dir()) {
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
    if !matches!(command, Command::Watch(_))
        && (before.no_notify || before.no_bell || before.no_title || before.layout.is_some())
    {
        eprintln!(
            "error: --no-notify, --no-bell, --no-title and --layout only apply to watching the queue"
        );
        return Exit::Usage;
    }

    match command {
        Command::Watch(after) => {
            // The flag closest to `watch` wins, and any flag wins over the
            // environment.
            let variable = match environment
                .layout
                .as_deref()
                .map(|value| PaneLayout::from_str(value, false))
            {
                None => None,
                Some(Ok(layout)) => Some(layout),
                Some(Err(_)) => {
                    eprintln!(
                        "error: ASQR_LAYOUT must be auto, beside or below, not {:?}",
                        environment.layout.unwrap_or_default()
                    );
                    return Exit::Usage;
                }
            };
            let args = WatchArgs {
                no_notify: before.no_notify || after.no_notify,
                no_bell: before.no_bell || after.no_bell,
                no_title: before.no_title || after.no_title,
                layout: after.layout.or(before.layout).or(variable),
            };
            watch::run(location, &args, environment.no_color, terminal_ui)
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
    use std::ffi::OsStr;
    use std::path::Path;

    use super::*;
    use crate::queue::{lock_holder, lock_queue};
    use crate::tui::Alerts;

    /// Runs `args` against a queue in `dir` and returns the exit and the
    /// alerts the terminal UI was started with, if it was.
    fn watched(dir: &Path, args: &[&str]) -> (Exit, Option<Alerts>) {
        let (exit, started) = watched_in(dir, args, Environment::default());
        (exit, started.map(|(alerts, _, _)| alerts))
    }

    /// Like [`watched`], with `environment` set, returning the layout and
    /// whether the terminal title is on too.
    fn watched_in(
        dir: &Path,
        args: &[&str],
        environment: Environment,
    ) -> (Exit, Option<(Alerts, PaneLayout, bool)>) {
        let mut argv = vec!["asqr", "--dir", dir.to_str().expect("UTF-8 temp path")];
        argv.extend(args);
        let cli = Cli::try_parse_from(argv).expect("parses");
        let mut started = None;
        let exit = dispatch(cli, environment, |watch| {
            let location = watch.location();
            assert!(location.inbox().is_dir(), "the layout exists");
            assert!(
                lock_holder(location).expect("readable").is_some(),
                "the TUI runs with the queue locked"
            );
            started = Some((watch.alerts(), watch.layout(), watch.title()));
            Exit::Success
        });
        (exit, started)
    }

    /// The layout `args` start the TUI with under `ASQR_LAYOUT=variable`.
    fn layout(args: &[&str], variable: Option<&str>) -> (Exit, Option<PaneLayout>) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let environment = Environment {
            layout: variable.map(str::to_owned),
            ..Environment::default()
        };
        let (exit, started) = watched_in(scratch.path(), args, environment);
        (exit, started.map(|(_, layout, _)| layout))
    }

    /// Whether `args` start the TUI with the terminal title on.
    fn titled(args: &[&str]) -> (Exit, Option<bool>) {
        let scratch = tempfile::tempdir().expect("temp dir");
        let (exit, started) = watched_in(scratch.path(), args, Environment::default());
        (exit, started.map(|(_, _, title)| title))
    }

    #[test]
    fn the_title_is_on_unless_no_title_stands_before_or_after_watch() {
        assert_eq!(titled(&[]), (Exit::Success, Some(true)));
        assert_eq!(titled(&["watch"]), (Exit::Success, Some(true)));
        assert_eq!(titled(&["--no-title"]), (Exit::Success, Some(false)));
        assert_eq!(
            titled(&["--no-title", "watch"]),
            (Exit::Success, Some(false))
        );
        assert_eq!(
            titled(&["watch", "--no-title"]),
            (Exit::Success, Some(false))
        );
        assert_eq!(titled(&["--no-title", "status"]), (Exit::Usage, None));
    }

    #[test]
    fn the_layout_is_auto_unless_a_flag_or_the_environment_says_otherwise() {
        assert_eq!(layout(&[], None), (Exit::Success, Some(PaneLayout::Auto)));
        assert_eq!(
            layout(&["--layout", "below"], None),
            (Exit::Success, Some(PaneLayout::Below))
        );
        assert_eq!(
            layout(&["watch", "--layout", "beside"], None),
            (Exit::Success, Some(PaneLayout::Beside))
        );
        assert_eq!(
            layout(&[], Some("beside")),
            (Exit::Success, Some(PaneLayout::Beside))
        );
    }

    #[test]
    fn a_layout_flag_wins_over_the_environment_and_after_watch_over_before() {
        assert_eq!(
            layout(&["--layout", "auto"], Some("below")),
            (Exit::Success, Some(PaneLayout::Auto))
        );
        assert_eq!(
            layout(&["--layout", "below", "watch", "--layout", "beside"], None),
            (Exit::Success, Some(PaneLayout::Beside))
        );
    }

    #[test]
    fn an_unknown_layout_is_a_usage_error() {
        assert_eq!(layout(&[], Some("sideways")), (Exit::Usage, None));
        assert!(Cli::try_parse_from(["asqr", "--layout", "sideways"]).is_err());
    }

    #[test]
    fn the_layout_variable_only_matters_to_watching() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let cli = Cli::try_parse_from([
            "asqr",
            "--dir",
            scratch.path().to_str().expect("UTF-8 temp path"),
            "paths",
        ])
        .expect("parses");
        let environment = Environment {
            layout: Some("sideways".into()),
            ..Environment::default()
        };

        assert_eq!(
            dispatch(cli, environment, |_| unreachable!("no TUI for paths")),
            Exit::Success
        );
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
        assert_eq!(
            watched(scratch.path(), &["--layout", "below", "status"]),
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

    /// The [`Environment`] read from the variables in `set`.
    fn environment_of(set: &[(&str, &OsStr)]) -> Environment {
        environment_from(|name| {
            set.iter()
                .find(|(variable, _)| *variable == name)
                .map(|(_, value)| value.to_os_string())
        })
    }

    #[test]
    fn no_color_counts_when_present_and_not_empty_whatever_its_value() {
        let no_color = |value: &str| environment_of(&[("NO_COLOR", OsStr::new(value))]).no_color;

        assert!(!environment_of(&[]).no_color, "unset");
        assert!(!no_color(""), "empty counts as unset");
        assert!(no_color("1"));
        assert!(no_color("0"), "any value, even one that reads as off");
    }

    #[cfg(unix)]
    #[test]
    fn no_color_counts_with_a_value_that_is_no_utf_8() {
        use std::os::unix::ffi::OsStrExt;

        let latin1 = OsStr::from_bytes(b"ja\xe4");
        assert!(environment_of(&[("NO_COLOR", latin1)]).no_color);
    }

    #[test]
    fn the_environment_reads_the_queue_and_the_layout_and_skips_empty_values() {
        let environment = environment_of(&[
            ("ASQR_QUEUE", OsStr::new("work")),
            ("ASQR_DIR", OsStr::new("")),
            ("ASQR_LAYOUT", OsStr::new("below")),
        ]);

        assert_eq!(environment.selection.queue.as_deref(), Some("work"));
        assert_eq!(environment.selection.dir, None);
        assert_eq!(environment.layout.as_deref(), Some("below"));
    }

    #[test]
    fn the_tui_starts_without_colours_under_no_color() {
        let colourless = |no_color: bool| {
            let scratch = tempfile::tempdir().expect("temp dir");
            let cli = Cli::try_parse_from([
                "asqr",
                "--dir",
                scratch.path().to_str().expect("UTF-8 temp path"),
            ])
            .expect("parses");
            let environment = Environment {
                no_color,
                ..Environment::default()
            };
            let mut started = None;
            dispatch(cli, environment, |watch| {
                started = Some(watch.no_color());
                Exit::Success
            });
            started
        };

        assert_eq!(colourless(true), Some(true));
        assert_eq!(colourless(false), Some(false));
    }
}
