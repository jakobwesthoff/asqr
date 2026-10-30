// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The line that says how many sessions wait in queues no asqr watches
//! (ADR 86). It stands in the status line while that has no notice and
//! no message. It is news, never a warning: several queues can be fine,
//! so it takes no colour beyond bold and dim.

use ratatui::text::{Line, Span};

use super::screen::shorten_from_left;
use super::source_line::columns;
use super::theme;
use crate::queue::WaitingQueue;

/// The fewest columns a cut queue name keeps before the line names the
/// queues as "other queues" instead.
const NAME_COLUMNS: usize = 12;

/// The line for the sessions waiting in `queues`, in `width` columns:
/// nothing when none wait. `key` adds `w  queues`, where the key opens the
/// queue switcher; it always stays whole. Where the line does not fit, the
/// queue's name is cut from the left down to [`NAME_COLUMNS`], then the
/// queues are "other queues", then the count alone is `N wait elsewhere`.
pub(super) fn other_queues_line(
    queues: &[WaitingQueue],
    key: bool,
    width: u16,
) -> Option<Line<'static>> {
    let count: usize = queues.iter().map(|queue| queue.waiting).sum();
    if count == 0 {
        return None;
    }
    let (noun, verb) = if count == 1 {
        ("session", "waits")
    } else {
        ("sessions", "wait")
    };
    let words = |rest: String| (count.to_string(), format!(" {noun} {verb} in {rest}"));
    let key_spans = if key {
        vec![
            Span::raw("   "),
            Span::styled("w", theme::key()),
            Span::styled("  queues", theme::secondary()),
        ]
    } else {
        Vec::new()
    };
    let key_width: usize = key_spans.iter().map(Span::width).sum();
    let room = usize::from(width).saturating_sub(key_width);
    let fits = |(number, rest): &(String, String)| columns(number) + columns(rest) <= room;

    let full = match queues {
        [queue] => words(queue.name.clone()),
        _ => words(format!("{} other queues", queues.len())),
    };
    let candidates = [
        Some(full),
        // One queue's name gives way from its start, as the title line
        // cuts it, while enough of it is left to tell queues apart.
        match queues {
            [queue] => {
                let before = columns(&count.to_string()) + columns(&format!(" {noun} {verb} in "));
                let left = room.saturating_sub(before);
                (left >= NAME_COLUMNS).then(|| words(shorten_from_left(&queue.name, left)))
            }
            _ => None,
        },
        Some(words("other queues".to_owned())),
        Some((count.to_string(), format!(" {verb} elsewhere"))),
    ];
    let (number, rest) = candidates
        .into_iter()
        .flatten()
        .find(fits)
        .unwrap_or_else(|| (count.to_string(), format!(" {verb} elsewhere")));
    let mut spans = vec![
        Span::styled(number, theme::strong()),
        Span::styled(rest, theme::secondary()),
    ];
    spans.extend(key_spans);
    Some(Line::from(spans))
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::*;

    fn waiting(name: &str, waiting: usize) -> WaitingQueue {
        WaitingQueue {
            name: name.into(),
            waiting,
        }
    }

    fn text(queues: &[WaitingQueue], key: bool, width: u16) -> Option<String> {
        other_queues_line(queues, key, width).map(|line| line.to_string())
    }

    #[test]
    fn nothing_shows_while_no_session_waits_elsewhere() {
        assert_eq!(text(&[], true, 80), None);
        assert_eq!(text(&[waiting("work", 0)], true, 80), None);
    }

    #[test]
    fn one_queue_is_named() {
        assert_eq!(
            text(&[waiting("asqr-todos", 2)], true, 80).as_deref(),
            Some("2 sessions wait in asqr-todos   w  queues")
        );
        assert_eq!(
            text(&[waiting("asqr-todos", 1)], false, 80).as_deref(),
            Some("1 session waits in asqr-todos")
        );
    }

    #[test]
    fn several_queues_are_counted() {
        let queues = [waiting("a", 2), waiting("b", 1), waiting("c", 2)];

        assert_eq!(
            text(&queues, true, 80).as_deref(),
            Some("5 sessions wait in 3 other queues   w  queues")
        );
    }

    #[test]
    fn the_line_gives_way_in_its_order_and_keeps_the_key() {
        let long = [waiting("notes-for-the-0.15-cycle", 3)];
        let several = [waiting("a", 2), waiting("b", 3)];

        assert_eq!(
            text(&long, true, 55).as_deref(),
            Some("3 sessions wait in notes-for-the-0.15-cycle   w  queues")
        );
        assert_eq!(
            text(&long, true, 50).as_deref(),
            Some("3 sessions wait in …for-the-0.15-cycle   w  queues")
        );
        // Twelve columns of the name take the room "other queues" would.
        assert_eq!(
            text(&long, true, 42).as_deref(),
            Some("3 wait elsewhere   w  queues")
        );
        assert_eq!(
            text(&several, true, 44).as_deref(),
            Some("5 sessions wait in other queues   w  queues")
        );
        assert_eq!(
            text(&several, true, 40).as_deref(),
            Some("5 wait elsewhere   w  queues")
        );
        assert_eq!(
            text(&several, true, 10).as_deref(),
            Some("5 wait elsewhere   w  queues"),
            "the shortest form shows even where it does not fit"
        );
    }

    #[test]
    fn the_number_is_bold_the_words_dim_and_the_key_a_key() {
        let line = other_queues_line(&[waiting("work", 2)], true, 80).expect("a line");
        let style = |index: usize| line.spans[index].style;

        assert_eq!(style(0).add_modifier, Modifier::BOLD);
        assert_eq!(style(1).add_modifier, Modifier::DIM);
        assert_eq!(
            (style(3).fg, style(3).add_modifier),
            (Some(Color::Cyan), Modifier::BOLD)
        );
        assert_eq!(style(4).add_modifier, Modifier::DIM);
        assert!(
            line.spans
                .iter()
                .all(|span| span.style.fg != Some(Color::Yellow)),
            "never a warning"
        );
    }
}
