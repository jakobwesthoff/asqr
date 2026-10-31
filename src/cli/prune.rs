// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr prune` (spec section 5.9).

use std::path::Path;
use std::time::{Duration, SystemTime};

use super::Exit;
use super::replay::find_entry;
use crate::queue::{QueueLocation, Removal, prune, prune_empty_queues, prune_session};

/// The shortest age `prune` takes: 30 days, the age after which the
/// compaction bundles sessions and results.
const PRUNE_FLOOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Parses the age of `prune --older-than`: a whole number and one unit,
/// `d` (days), `w` (weeks), `mo` (months of 30 days) or `y` (years of 365
/// days), of at least 30 days. Used as clap's value parser, so a bad age
/// is a usage error that names the form that works.
pub(super) fn parse_prune_age(input: &str) -> Result<Duration, String> {
    let malformed = || "expected a number and a unit (d, w, mo or y), such as 90d".to_owned();
    let split = input
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(malformed)?;
    let (number, unit) = input.split_at(split);
    let number: u64 = number.parse().map_err(|_| malformed())?;
    let days_per_unit: u64 = match unit {
        "d" => 1,
        "w" => 7,
        "mo" => 30,
        "y" => 365,
        "s" | "m" | "h" => {
            return Err("the units s, m and h are gone; use d, w, mo or y, such as 90d".to_owned());
        }
        _ => return Err(malformed()),
    };
    let age = number
        .checked_mul(days_per_unit * 24 * 60 * 60)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("{input} is too long an age"))?;
    if age < PRUNE_FLOOR {
        return Err("an age must be at least 30 days, such as 30d, 5w or 1mo".to_owned());
    }
    Ok(age)
}

/// Parses an age such as `30m`: a whole number and one unit, `s`, `m`, `h`,
/// `d` or `w`. Used as clap's value parser for `status --since`, so a
/// malformed age is a usage error.
pub(super) fn parse_age(input: &str) -> Result<Duration, String> {
    let malformed = || "expected a number and a unit (s, m, h, d or w), such as 30d".to_owned();
    let split = input
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(malformed)?;
    let (number, unit) = input.split_at(split);
    let number: u64 = number.parse().map_err(|_| malformed())?;
    let seconds_per_unit = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        "w" => 7 * 24 * 60 * 60,
        _ => return Err(malformed()),
    };
    number
        .checked_mul(seconds_per_unit)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("{input} is too long an age"))
}

/// Prunes the queue at `location`, then removes the named queues under
/// `root` that nobody watches and that hold no file. Without `yes` it only
/// says what it would remove.
pub(super) fn run(
    location: &QueueLocation,
    root: Option<&Path>,
    older_than: Duration,
    yes: bool,
) -> Exit {
    let pruned = prune(location, older_than, SystemTime::now(), yes).and_then(|mut found| {
        if let Some(root) = root {
            let gone = found.clone();
            found.extend(prune_empty_queues(root, yes, &gone)?);
        }
        Ok(found)
    });
    match pruned {
        Ok(found) if yes => {
            for path in found {
                tracing::info!(path = %path.display(), "pruned");
                println!("removed {}", path.display());
            }
            Exit::Success
        }
        Ok(found) if found.is_empty() => {
            println!("nothing to remove");
            Exit::Success
        }
        Ok(found) => {
            for path in found {
                println!("would remove {}", path.display());
            }
            println!("nothing was removed; run again with --yes to remove these");
            Exit::Success
        }
        Err(error) => {
            eprintln!("error: cannot prune the queue: {error}");
            Exit::Failure
        }
    }
}

/// Prunes the one finished session `reference` names, with the kept files
/// only it used. Without `yes` it only says what it would remove.
pub(super) fn run_session(
    location: &QueueLocation,
    root: Option<&Path>,
    reference: &str,
    yes: bool,
) -> Exit {
    let entry = match find_entry(location, root, reference) {
        Ok(entry) => entry,
        Err(exit) => return exit,
    };
    let removals = match prune_session(&entry, yes) {
        Ok(removals) => removals,
        Err(error) => {
            eprintln!("error: cannot prune the session: {error}");
            return Exit::Failure;
        }
    };
    let shown = |removal: &Removal| match removal {
        Removal::File(path) => path.display().to_string(),
        Removal::BundleEntry { bundle, name } => format!("{}: {name}", bundle.display()),
    };
    if yes {
        for removal in &removals {
            tracing::info!(removed = %shown(removal), "pruned");
            println!("removed {}", shown(removal));
        }
    } else if removals.is_empty() {
        println!("nothing to remove");
    } else {
        for removal in &removals {
            println!("would remove {}", shown(removal));
        }
        println!("nothing was removed; run again with --yes to remove these");
    }
    Exit::Success
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prune_age_takes_days_weeks_months_and_years() {
        const DAY: u64 = 86_400;
        assert_eq!(parse_prune_age("30d"), Ok(Duration::from_secs(30 * DAY)));
        assert_eq!(parse_prune_age("90d"), Ok(Duration::from_secs(90 * DAY)));
        assert_eq!(parse_prune_age("5w"), Ok(Duration::from_secs(35 * DAY)));
        assert_eq!(parse_prune_age("1mo"), Ok(Duration::from_secs(30 * DAY)));
        assert_eq!(parse_prune_age("6mo"), Ok(Duration::from_secs(180 * DAY)));
        assert_eq!(parse_prune_age("1y"), Ok(Duration::from_secs(365 * DAY)));
    }

    #[test]
    fn a_prune_age_below_30_days_names_the_floor() {
        for input in ["29d", "4w", "0d", "0w", "0mo", "0y"] {
            let error = parse_prune_age(input).expect_err(input);
            assert!(error.contains("at least 30 days"), "{input}: {error}");
        }
    }

    #[test]
    fn the_units_seconds_minutes_and_hours_name_the_new_form() {
        for input in ["30s", "90m", "12h"] {
            let error = parse_prune_age(input).expect_err(input);
            assert!(
                error.contains("s, m and h are gone") && error.contains("d, w, mo or y"),
                "{input}: {error}"
            );
        }
    }

    #[test]
    fn a_malformed_prune_age_names_the_units() {
        for input in ["", "30", "d", "-1d", "1.5d", "3x", "1d2h", "mo", "5mm"] {
            let error = parse_prune_age(input).expect_err(input);
            assert!(error.contains("d, w, mo or y"), "{input:?}: {error}");
        }
    }

    #[test]
    fn a_prune_age_too_large_to_count_is_refused() {
        assert!(parse_prune_age(&format!("{}y", u64::MAX)).is_err());
    }

    #[test]
    fn parses_a_number_with_a_unit() {
        assert_eq!(parse_age("45s"), Ok(Duration::from_secs(45)));
        assert_eq!(parse_age("90m"), Ok(Duration::from_secs(90 * 60)));
        assert_eq!(parse_age("12h"), Ok(Duration::from_secs(12 * 3600)));
        assert_eq!(parse_age("30d"), Ok(Duration::from_secs(30 * 86_400)));
        assert_eq!(parse_age("2w"), Ok(Duration::from_secs(14 * 86_400)));
        assert_eq!(parse_age("0s"), Ok(Duration::ZERO));
    }

    #[test]
    fn rejects_anything_else() {
        for input in ["", "30", "d", "-1d", "1.5d", "3y", "1d2h"] {
            assert!(parse_age(input).is_err(), "{input:?}");
        }
        assert_eq!(
            parse_age("3y"),
            Err("expected a number and a unit (s, m, h, d or w), such as 30d".to_owned())
        );
    }

    #[test]
    fn refuses_ages_too_large_to_count() {
        assert!(parse_age(&format!("{}w", u64::MAX)).is_err());
    }
}
