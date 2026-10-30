// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The command line (spec section 6). The binary parses the arguments and
//! hands them to [`run`]; everything else happens here, so the commands
//! are reachable from tests.

mod ask;
mod exit;
mod format;
mod hint;
pub(crate) mod log;
mod paths;
mod prune;
mod queues;
mod skill;
mod status;
mod wait;
mod watch;
mod withdraw;

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};

pub use exit::Exit;
pub use watch::{Pin, Watch};

use crate::pairing::{self, Multiplexer, SystemTmux, Tmux, TmuxFailure};
use crate::queue::{Origin, Paired, QueueLocation, Selection, platform_data_dir};
use crate::tui::render::PaneLayout;

/// The line `asqr --help` and `asqr ask --help` show agents, below the
/// line for the person (spec section 9). The core of the skill names its
/// last line, so "to the last line" is something an agent can check.
macro_rules! agent_line {
    () => {
        "Agents: run `asqr skill` and read its output to the last line before you ask; it describes this version of asqr"
    };
}

/// Answer questions an AI agent or a script prepared for you, right in your
/// terminal.
#[derive(Debug, Parser)]
#[command(
    name = "asqr",
    version,
    about = concat!(env!("CARGO_PKG_DESCRIPTION"), "\n\n", agent_line!())
)]
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

    /// Do not show the queue and the number of waiting sessions in the
    /// terminal title.
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
    // A second doc paragraph would keep this line's period in the long
    // help only, so the long help is spelled out.
    #[command(long_about = concat!(
        "Drop a session file into the queue and print its id",
        "\n\n",
        agent_line!()
    ))]
    Ask {
        /// The session file.
        file: PathBuf,

        /// Wait for the result and print it instead of the id.
        #[arg(long)]
        wait: bool,

        /// Stop waiting after this many seconds (exit code 12).
        #[arg(long, value_name = "SECONDS", requires = "wait")]
        timeout: Option<u64>,

        /// Move an unread result with the same id into the archive first;
        /// with --replaces, also withdraw the old session while it is being
        /// answered.
        #[arg(long)]
        force: bool,

        /// Withdraw this waiting session and drop the new one in its place.
        #[arg(long, value_name = "ID")]
        replaces: Option<String>,
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

    /// Take back a waiting session and print its result, status withdrawn.
    Withdraw {
        /// The session id.
        id: String,

        /// Withdraw it even though the person has started answering it.
        #[arg(long)]
        force: bool,
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

    /// List every queue, what waits in it and which asqr watches it.
    Queues {
        /// Print the queues as JSON.
        #[arg(long)]
        json: bool,
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

    /// Print the agent skill of this asqr or one of its parts, or install
    /// the skill under a skills directory.
    Skill {
        /// Print this part of the skill instead of its core, such as
        /// context; the skill names its parts.
        #[arg(value_name = "PART", conflicts_with = "install")]
        part: Option<String>,

        /// Write the skill to asqr/ under this directory, for example
        /// .claude/skills: asqr/SKILL.md and asqr/references/, which asqr
        /// replaces whole. Mostly not needed: `asqr skill` prints the
        /// skill of this asqr.
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
    /// `ASQR_LOG`: which events reach the log (ADR 83).
    pub log: Option<String>,
    /// `NO_COLOR` (<https://no-color.org>): the TUI draws without colours.
    pub no_color: bool,
    /// `HOME`, which the TUI shows as `~` in the paths of context entries
    /// it cannot show.
    pub home: Option<PathBuf>,
    /// The multiplexer `TMUX` and `TMUX_PANE` name, through which asqr
    /// pairs with the agents of the same session (ADR 85).
    pub multiplexer: Multiplexer,
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
        log: variable("ASQR_LOG"),
        // no-color.org asks only that the variable is present and not
        // empty, so any value counts, also one that is not UTF-8.
        no_color: lookup("NO_COLOR").is_some_and(|value| !value.is_empty()),
        // A path is any bytes, so a home that is not UTF-8 counts too.
        home: lookup("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        multiplexer: Multiplexer::from_variables(
            variable("TMUX").as_deref(),
            variable("TMUX_PANE").as_deref(),
        ),
    }
}

/// Runs the parsed command line. `environment` is what [`environment`]
/// read, passed in so the precedence of flags over the environment is
/// decided in one place. `terminal_ui` runs the TUI; only the binary has a
/// terminal to hand it.
pub fn run(cli: Cli, environment: Environment, terminal_ui: impl FnOnce(Watch) -> Exit) -> Exit {
    log::init(environment.log.as_deref());
    dispatch(
        cli,
        environment,
        &SystemTmux::default(),
        platform_data_dir(),
        terminal_ui,
    )
}

/// Runs the command with `tmux` to ask and the named queues under `root`,
/// which tests choose.
fn dispatch(
    cli: Cli,
    environment: Environment,
    tmux: &dyn Tmux,
    root: Option<PathBuf>,
    terminal_ui: impl FnOnce(Watch) -> Exit,
) -> Exit {
    let flags = Selection {
        queue: cli.queue,
        dir: cli.dir,
    };
    // tmux is asked only where its answer can decide: inside tmux, when no
    // flag and no variable chose, for a command that works on a queue.
    let works_on_a_queue = !matches!(
        cli.command,
        Some(
            Command::New
                | Command::Validate { .. }
                | Command::Schema { .. }
                | Command::Skill { .. }
                | Command::Queues { .. }
        )
    );
    // `ask` asks tmux also when a variable chose, for the session's pin:
    // the agent's variable may send it past the session's asqr.
    let is_ask = matches!(cli.command, Some(Command::Ask { .. }));
    let unchosen = flags.is_empty() && environment.selection.is_empty();
    let variable = match (&flags, &environment.selection) {
        (flags, _) if !flags.is_empty() => None,
        (_, Selection { queue: Some(_), .. }) => Some("ASQR_QUEUE"),
        (_, Selection { dir: Some(_), .. }) => Some("ASQR_DIR"),
        _ => None,
    };
    let tmux_answer = match &environment.multiplexer {
        Multiplexer::Tmux(pane)
            if works_on_a_queue && (unchosen || (is_ask && variable.is_some())) =>
        {
            Some((pane, pairing::ask_session(tmux, pane)))
        }
        _ => None,
    };
    let paired = match &tmux_answer {
        Some((_, answer)) if unchosen => pairing::paired_by(answer),
        // Asked for the hint only: a tmux that does not answer gives none.
        Some((_, Err(TmuxFailure(reason)))) => {
            tracing::info!(reason, "tmux did not answer; no word on the session's pin");
            Paired::NotAsked
        }
        _ => Paired::NotAsked,
    };
    let (location, origin) =
        match QueueLocation::resolve_paired(flags, environment.selection, paired, root.clone()) {
            Ok(resolved) => resolved,
            Err(error) => {
                eprintln!("error: {error}");
                return Exit::Usage;
            }
        };
    let invocation = hint::Invocation::new(&location, named_in_commands(origin));

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
            watch::run(
                location,
                &args,
                environment.no_color,
                environment.home,
                watch::Pairing {
                    multiplexer: &environment.multiplexer,
                    origin,
                    tmux,
                },
                terminal_ui,
            )
        }
        Command::Paths { json } => paths::run(&location, json),
        Command::New => format::run_new(),
        Command::Ask {
            file,
            wait,
            timeout,
            force,
            replaces,
        } => ask::run(
            &location,
            &invocation,
            &file,
            ask::AskOptions {
                wait,
                timeout: timeout.map(Duration::from_secs),
                force,
                replaces,
            },
            &ask::Surroundings {
                root: root.as_deref(),
                tmux,
                origin,
                tmux_session: tmux_answer.and_then(|(pane, answer)| Some((pane, answer.ok()?))),
                variable,
            },
        ),
        Command::Wait { id, timeout } => wait::run_wait(
            &location,
            &invocation,
            root.as_deref(),
            &id,
            timeout.map(Duration::from_secs),
        ),
        Command::Result { id } => wait::run_result(&location, &invocation, root.as_deref(), &id),
        Command::Withdraw { id, force } => {
            withdraw::run(&location, &invocation, root.as_deref(), &id, force)
        }
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
        Command::Queues { json } => queues::run(root.as_deref(), tmux, json),
        Command::Prune {
            older_than,
            results,
        } => prune::run(&location, older_than, results),
        Command::Skill { part, install } => skill::run(part.as_deref(), install.as_deref()),
        Command::Validate { file } => format::run_validate(&file),
        Command::Schema { result } => format::run_schema(result),
    }
}

/// Whether a command in advice names the queue with a flag (spec section
/// 6.7). Plain `asqr` reaches a queue nothing chose, and from the same
/// tmux pane the queue tmux gave. After tmux did not answer it would reach
/// the session's queue as soon as tmux answers again, so the default queue
/// taken then is named.
fn named_in_commands(origin: Origin) -> bool {
    match origin {
        Origin::Flag | Origin::Environment | Origin::Unreachable => true,
        Origin::Pin | Origin::SessionName | Origin::Default => false,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::*;
    use crate::pairing::TmuxPane;
    use crate::pairing::testing::{StandIn, Unasked};
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
        let exit = dispatch(cli, environment, &Unasked, None, |watch| {
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
            dispatch(cli, environment, &Unasked, None, |_| unreachable!(
                "no TUI for paths"
            )),
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
        let _held = lock_queue(&QueueLocation::at(scratch.path()), None).expect("locked");

        assert_eq!(watched(scratch.path(), &[]), (Exit::Failure, None));
    }

    #[test]
    fn a_queue_that_cannot_be_set_up_is_a_failure() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let blocked = scratch.path().join("blocked");
        std::fs::write(&blocked, "a file where the queue should be").expect("written");

        assert_eq!(watched(&blocked, &[]), (Exit::Failure, None));
    }

    #[test]
    fn a_queue_whose_directories_cannot_be_made_is_a_failure() {
        let scratch = tempfile::tempdir().expect("temp dir");
        // The lock goes in, the inbox cannot.
        std::fs::write(scratch.path().join("inbox"), "a file where the inbox goes")
            .expect("written");

        assert_eq!(watched(scratch.path(), &[]), (Exit::Failure, None));
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
    fn the_environment_reads_asqr_log_and_skips_an_empty_one() {
        let log = |value: &str| environment_of(&[("ASQR_LOG", OsStr::new(value))]).log;

        assert_eq!(log("info,asqr=debug").as_deref(), Some("info,asqr=debug"));
        assert_eq!(log(""), None, "empty counts as unset");
        assert_eq!(environment_of(&[]).log, None, "unset");
    }

    #[cfg(unix)]
    #[test]
    fn an_asqr_log_that_is_no_utf_8_counts_as_unset() {
        use std::os::unix::ffi::OsStrExt;

        let latin1 = OsStr::from_bytes(b"deb\xe4");
        assert_eq!(environment_of(&[("ASQR_LOG", latin1)]).log, None);
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
            dispatch(cli, environment, &Unasked, None, |watch| {
                started = Some(watch.no_color());
                Exit::Success
            });
            started
        };

        assert_eq!(colourless(true), Some(true));
        assert_eq!(colourless(false), Some(false));
    }

    #[test]
    fn home_is_read_from_the_environment() {
        let home = |value: &str| environment_of(&[("HOME", OsStr::new(value))]).home;

        assert_eq!(home("/home/me"), Some(PathBuf::from("/home/me")));
        assert_eq!(home(""), None, "empty counts as unset");
        assert_eq!(environment_of(&[]).home, None, "unset");
    }

    #[cfg(unix)]
    #[test]
    fn home_is_read_with_a_value_that_is_no_utf_8() {
        use std::os::unix::ffi::OsStrExt;

        let latin1 = OsStr::from_bytes(b"/home/j\xe4");
        assert_eq!(
            environment_of(&[("HOME", latin1)]).home,
            Some(PathBuf::from(latin1))
        );
    }

    #[test]
    fn the_tui_starts_with_the_home_folder() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let cli = Cli::try_parse_from([
            "asqr",
            "--dir",
            scratch.path().to_str().expect("UTF-8 temp path"),
        ])
        .expect("parses");
        let environment = Environment {
            home: Some(PathBuf::from("/home/me")),
            ..Environment::default()
        };
        let mut started = None;

        dispatch(cli, environment, &Unasked, None, |watch| {
            started = Some(watch.home().map(Path::to_path_buf));
            Exit::Success
        });

        assert_eq!(started, Some(Some(PathBuf::from("/home/me"))));
    }

    // -----------------------------------------------------------------
    // Pairing through tmux
    // -----------------------------------------------------------------

    fn in_tmux_pane() -> Environment {
        Environment {
            multiplexer: Multiplexer::Tmux(TmuxPane {
                socket: "/tmp/tmux-501/default".into(),
                pane: Some("%3".into()),
            }),
            ..Environment::default()
        }
    }

    /// The name of the queue `args` start the TUI on, with `tmux` to ask
    /// and named queues under a temporary root.
    fn watched_queue(args: &[&str], environment: Environment, tmux: &dyn Tmux) -> Option<String> {
        let root = tempfile::tempdir().expect("temp dir");
        let mut argv = vec!["asqr"];
        argv.extend(args);
        let cli = Cli::try_parse_from(argv).expect("parses");
        let mut started = None;
        dispatch(
            cli,
            environment,
            tmux,
            Some(root.path().to_path_buf()),
            |watch| {
                started = watch.location().name().map(str::to_owned);
                Exit::Success
            },
        );
        started
    }

    #[test]
    fn inside_tmux_without_a_choice_asqr_uses_the_session_queue() {
        let tmux = StandIn::answering("my work\n");

        assert_eq!(
            watched_queue(&[], in_tmux_pane(), &tmux),
            Some("my-work".to_owned())
        );
    }

    #[test]
    fn inside_tmux_the_pinned_queue_wins_over_the_session_name() {
        let tmux = StandIn::answering("my work\nASQR_SESSION_QUEUE=asqr-todos\n");

        assert_eq!(
            watched_queue(&[], in_tmux_pane(), &tmux),
            Some("asqr-todos".to_owned())
        );
    }

    #[test]
    fn a_tmux_that_does_not_answer_leaves_the_default_queue() {
        let tmux = StandIn::failing("tmux is not installed");

        assert_eq!(
            watched_queue(&[], in_tmux_pane(), &tmux),
            Some("default".to_owned())
        );
    }

    #[test]
    fn a_flag_or_the_environment_never_asks_tmux() {
        assert_eq!(
            watched_queue(&["--queue", "work"], in_tmux_pane(), &Unasked),
            Some("work".to_owned())
        );
        let environment = Environment {
            selection: Selection {
                queue: Some("env".into()),
                dir: None,
            },
            ..in_tmux_pane()
        };
        assert_eq!(
            watched_queue(&[], environment, &Unasked),
            Some("env".to_owned())
        );
    }

    #[test]
    fn commands_without_a_queue_never_ask_tmux() {
        let root = tempfile::tempdir().expect("temp dir");
        let cli = Cli::try_parse_from(["asqr", "new"]).expect("parses");

        let exit = dispatch(
            cli,
            in_tmux_pane(),
            &Unasked,
            Some(root.path().into()),
            |_| unreachable!("no TUI for new"),
        );

        assert_eq!(exit, Exit::Success);
    }

    #[test]
    fn outside_tmux_tmux_is_never_asked() {
        assert_eq!(
            watched_queue(&[], Environment::default(), &Unasked),
            Some("default".to_owned())
        );
    }

    #[test]
    fn the_environment_reads_tmux_and_its_pane() {
        let environment = environment_of(&[
            ("TMUX", OsStr::new("/tmp/tmux-501/default,3614,4")),
            ("TMUX_PANE", OsStr::new("%66")),
        ]);

        assert_eq!(
            environment.multiplexer,
            Multiplexer::Tmux(TmuxPane {
                socket: "/tmp/tmux-501/default".into(),
                pane: Some("%66".into()),
            })
        );
    }

    /// The pin the TUI started by `args` is to set, and the calls `tmux`
    /// got before the TUI started.
    fn pin_of(args: &[&str], tmux: &StandIn) -> Option<Pin> {
        let root = tempfile::tempdir().expect("temp dir");
        let mut argv = vec!["asqr"];
        argv.extend(args);
        let cli = Cli::try_parse_from(argv).expect("parses");
        let mut started = None;
        dispatch(
            cli,
            in_tmux_pane(),
            tmux,
            Some(root.path().to_path_buf()),
            |watch| {
                started = Some(watch.pin().cloned());
                Exit::Success
            },
        );
        started.expect("the TUI started")
    }

    fn pin(queue: &str) -> Option<Pin> {
        Some(Pin {
            pane: TmuxPane {
                socket: "/tmp/tmux-501/default".into(),
                pane: Some("%3".into()),
            },
            queue: queue.into(),
        })
    }

    #[test]
    fn the_watch_carries_the_pin_of_a_queue_from_the_session() {
        let tmux = StandIn::answering("my work\n");

        assert_eq!(pin_of(&[], &tmux), pin("my-work"));
    }

    #[test]
    fn the_watch_carries_the_pin_of_the_default_queue_named_by_flag() {
        let tmux = StandIn::answering("my work\n");

        assert_eq!(pin_of(&["--queue", "default"], &tmux), pin("default"));
    }

    #[test]
    fn a_queue_directory_carries_no_pin() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let dir = scratch.path().to_str().expect("UTF-8 temp path");
        let tmux = StandIn::answering("my work\n");

        assert_eq!(pin_of(&["--dir", dir], &tmux), None);
    }

    #[test]
    fn after_tmux_did_not_answer_nothing_is_pinned() {
        let tmux = StandIn::failing("tmux is not installed");

        assert_eq!(pin_of(&[], &tmux), None);
    }

    #[test]
    fn outside_tmux_nothing_is_pinned() {
        let root = tempfile::tempdir().expect("temp dir");
        let cli = Cli::try_parse_from(["asqr"]).expect("parses");
        let mut started = None;

        dispatch(
            cli,
            Environment::default(),
            &Unasked,
            Some(root.path().to_path_buf()),
            |watch| {
                started = Some(watch.pin().cloned());
                Exit::Success
            },
        );

        assert_eq!(started, Some(None));
    }

    #[test]
    fn starting_never_pins_before_the_terminal_ui() {
        let tmux = StandIn::answering("my work\n");

        pin_of(&[], &tmux);

        assert!(
            tmux.calls()
                .iter()
                .all(|call| !call.contains(&"set-environment".to_owned())),
            "{:?}",
            tmux.calls()
        );
    }

    #[test]
    fn queues_without_a_data_directory_is_a_failure() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let dir = scratch.path().to_str().expect("UTF-8 temp path");
        let cli = Cli::try_parse_from(["asqr", "--dir", dir, "queues"]).expect("parses");

        let exit = dispatch(cli, Environment::default(), &Unasked, None, |_| {
            unreachable!("no TUI for queues")
        });

        assert_eq!(exit, Exit::Failure);
    }

    #[test]
    fn commands_in_advice_name_the_queue_unless_plain_asqr_reaches_it() {
        let named: Vec<bool> = [
            Origin::Flag,
            Origin::Environment,
            Origin::Pin,
            Origin::SessionName,
            Origin::Default,
            Origin::Unreachable,
        ]
        .into_iter()
        .map(named_in_commands)
        .collect();

        assert_eq!(named, [true, true, false, false, false, true]);
    }
}
