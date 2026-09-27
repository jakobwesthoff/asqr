// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `asqr prune` (spec section 3.9).

use std::time::{Duration, SystemTime};

use super::Exit;
use crate::queue::{QueueLocation, prune};

/// Parses an age such as `30d`: a whole number and one unit, `s`, `m`, `h`,
/// `d` or `w`. Used as clap's value parser, so a malformed age is a usage
/// error.
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

pub(super) fn run(location: &QueueLocation, older_than: Duration, include_results: bool) -> Exit {
    match prune(location, older_than, include_results, SystemTime::now()) {
        Ok(removed) => {
            for path in removed {
                println!("removed {}", path.display());
            }
            Exit::Success
        }
        Err(error) => {
            eprintln!("error: cannot prune the queue: {error}");
            Exit::Failure
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
