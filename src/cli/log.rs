// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The log file (ADR 15). The TUI owns the terminal, so everything worth
//! keeping goes into `asqr.log` in the platform cache directory. Logging
//! is a convenience: when the file cannot be opened, the command runs
//! without it. `ASQR_LOG` sets which events reach it (ADR 83).

use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Mutex;

use tracing::Dispatch;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::util::SubscriberInitExt;

use super::paths::log_file;

/// Sends `tracing` events of this process to the log file, appending.
/// `variable` is the value of `ASQR_LOG`.
pub(super) fn init(variable: Option<&str>) {
    if let Some(path) = log_file() {
        let _ = try_init(&path, variable);
    }
}

fn try_init(path: &Path, variable: Option<&str>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    // Only fails when a subscriber is set already, which then keeps
    // logging.
    let _ = dispatch(Mutex::new(file), variable).try_init();
    Ok(())
}

/// The log as asqr writes it into `writer`: without colours, and with the
/// events that `variable`, the value of `ASQR_LOG`, lets through, `info`
/// and above without one. A value that does not parse lets `info` and
/// above through, and the log says so.
pub(crate) fn dispatch<W>(writer: W, variable: Option<&str>) -> Dispatch
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let value = variable.unwrap_or_default();
    // tracing's parser takes a space around a directive as part of it, so
    // ` debug` would name a target and ` asqr=debug` would not parse.
    let directives = value
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(",");
    let builder = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into());
    let (filter, rejected) = match builder.parse(&directives) {
        Ok(filter) => (filter, None),
        Err(error) => (builder.parse_lossy(""), Some(error)),
    };
    let dispatch = Dispatch::new(
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(writer)
            .with_ansi(false)
            .finish(),
    );
    // The TUI owns the terminal, so the warning goes into the log.
    if let Some(error) = rejected {
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::warn!(
                value,
                %error,
                "ASQR_LOG does not parse; the log takes info and above"
            );
        });
    }
    dispatch
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::EnvFilter;

    use crate::tui::render::test_support::logged_with;

    /// What the log holds with `variable` as `ASQR_LOG` after asqr logged
    /// a line at `debug` and one at `info`, and another library one line
    /// at each of them.
    fn log_with(variable: Option<&str>) -> String {
        logged_with(variable, || {
            tracing::debug!("asqr debug");
            tracing::info!("asqr info");
            tracing::debug!(target: "elsewhere", "library debug");
            tracing::info!(target: "elsewhere", "library info");
        })
    }

    #[test]
    fn without_asqr_log_the_log_takes_info_and_above() {
        let log = log_with(None);

        assert!(log.contains("asqr info"), "{log}");
        assert!(log.contains("library info"), "{log}");
        assert!(!log.contains("asqr debug"), "{log}");
        assert!(!log.contains("library debug"), "{log}");
    }

    #[test]
    fn a_level_in_asqr_log_applies_to_every_target() {
        let log = log_with(Some("debug"));

        assert!(log.contains("asqr debug"), "{log}");
        assert!(log.contains("library debug"), "{log}");
    }

    #[test]
    fn a_value_with_directives_replaces_the_info_default() {
        let log = log_with(Some("asqr::cli::log=debug"));

        assert!(log.contains("asqr debug"), "{log}");
        assert!(!log.contains("library info"), "{log}");
    }

    #[test]
    fn a_level_in_front_keeps_the_other_targets_at_it() {
        let log = log_with(Some("info,asqr=debug"));

        assert!(log.contains("asqr debug"), "{log}");
        assert!(log.contains("library info"), "{log}");
        assert!(!log.contains("library debug"), "{log}");
    }

    #[test]
    fn an_asqr_log_that_does_not_parse_takes_info_and_warns_about_it() {
        let error = EnvFilter::builder()
            .parse("asqr=loud")
            .expect_err("loud is no level");

        let log = log_with(Some("asqr=loud"));

        assert!(log.contains("asqr info"), "{log}");
        assert!(!log.contains("asqr debug"), "{log}");
        let warnings: Vec<&str> = log.lines().filter(|line| line.contains("WARN")).collect();
        assert_eq!(warnings.len(), 1, "{log}");
        assert!(warnings[0].contains("ASQR_LOG"), "{log}");
        assert!(warnings[0].contains(r#"value="asqr=loud""#), "{log}");
        assert!(warnings[0].contains(&error.to_string()), "{log}");
    }

    /// A bare word that is no level names a target, so a misspelled level
    /// lets nothing through, and no warning either.
    #[test]
    fn a_bare_word_that_is_no_level_is_a_target() {
        assert_eq!(log_with(Some("degub")), "");
    }

    #[test]
    fn spaces_around_directives_do_not_count() {
        let log = log_with(Some(" info , asqr=debug "));

        assert!(log.contains("asqr debug"), "{log}");
        assert!(log.contains("library info"), "{log}");
        assert!(!log.contains("WARN"), "{log}");
    }
}
