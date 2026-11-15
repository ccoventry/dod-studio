//! The review's playback speed keys (#1437): Up plays the highlight one step
//! faster, Down one step slower, through the speeds the demo player's VCR
//! bar offers ([`SPEEDS`], `dem_speed`'s 0.25 to 4). The keys themselves are
//! `review_keys`'; this file holds the steps and how a speed reads on the box.
//!
//! A speed chosen with the keys wins over the review's own fast-forward over
//! long gaps between kills (#665) until the highlight ends: the review stops
//! switching speeds for it, and the next highlight (or a Replay) starts at
//! normal speed again. The step goes from the speed the demo plays at now,
//! so Down inside a fast-forwarded gap slows it from there.
//!
//! That fast-forward is here too: the stretches between two kills far apart
//! that the queue asks to play faster ([`fast_spans`], #665).

/// The speed a gap plays at when the queue names none (#665).
pub(crate) const FAST_SPEED: f32 = 4.0;

/// The speeds the keys step through, slowest first: the VCR bar's.
pub(crate) const SPEEDS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// The speed one step faster (`faster`) or slower than `now`; `now` itself
/// past the end of [`SPEEDS`] that way. A speed between two steps (the queue's own
/// fast-forward speed can be any) goes to the next step that way.
pub(crate) fn step(now: f32, faster: bool) -> f32 {
    let next = if faster {
        SPEEDS.iter().copied().find(|&s| s > now + f32::EPSILON)
    } else {
        SPEEDS
            .iter()
            .rev()
            .copied()
            .find(|&s| s < now - f32::EPSILON)
    };
    next.unwrap_or(now)
}

/// The stretches of a highlight to fast-forward: between two kills more than
/// `gap` seconds apart, from [`TAIL`](crate::review) after the first to [`LEAD`](crate::review) before the
/// second, the same margins a highlight starts and ends with.
pub(crate) fn fast_spans(kills: &[f64], gap: f64) -> Vec<(f64, f64)> {
    if gap <= 0.0 {
        return Vec::new();
    }
    kills
        .windows(2)
        .filter(|pair| pair[1] - pair[0] > gap)
        .map(|pair| (pair[0] + crate::review::TAIL, pair[1] - crate::review::LEAD))
        .filter(|(start, end)| end > start)
        .collect()
}

/// The log's note on what will be fast-forwarded, or nothing.
pub(crate) fn describe_spans(spans: &[(f64, f64)], speed: f32) -> String {
    if spans.is_empty() {
        return String::new();
    }
    let list: Vec<String> = spans
        .iter()
        .map(|(s, e)| format!("{s:.1}..{e:.1}"))
        .collect();
    format!("; at {speed}x: {}", list.join(", "))
}

/// The speed keys' step (#1437): while a highlight is `playing`, sets the
/// demo player one step faster or slower than it plays now, keeps that as
/// the highlight's `chosen` speed and returns the log's line; else says why
/// not.
pub(crate) fn change(
    faster: bool,
    playing: bool,
    chosen: &mut Option<f32>,
) -> Result<String, String> {
    if !playing {
        return Err("the speed changes only while a highlight plays".into());
    }
    let now = crate::demo_seek::clock().map_or(1.0, |c| c.time_scale);
    let speed = step(now, faster);
    crate::demo_seek::set_time_scale(speed).map_err(|why| format!("speed {speed}: {why}"))?;
    *chosen = Some(speed);
    let how = if faster { "faster" } else { "slower" };
    Ok(format!("speed {now}x -> {speed}x ({how})"))
}

/// Whether `speed` is normal speed, which the box doesn't mention.
pub(crate) fn is_normal(speed: f32) -> bool {
    (speed - 1.0).abs() < 0.01
}

/// A speed as the box shows it: `2x`, `0.25x`.
pub(crate) fn text(speed: f32) -> String {
    let rounded = (speed * 100.0).round() / 100.0;
    format!("{rounded}x")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_and_down_step_through_the_vcr_bars_speeds() {
        assert_eq!(step(1.0, true), 2.0);
        assert_eq!(step(2.0, true), 4.0);
        assert_eq!(step(1.0, false), 0.5);
        assert_eq!(step(0.5, false), 0.25);
        // Held at either end.
        assert_eq!(step(4.0, true), 4.0);
        assert_eq!(step(0.25, false), 0.25);
    }

    #[test]
    fn a_speed_between_steps_goes_to_the_next_one_that_way() {
        // The queue's fast-forward speed is any from 1 to 16 (#665).
        assert_eq!(step(3.0, true), 4.0);
        assert_eq!(step(3.0, false), 2.0);
        assert_eq!(step(8.0, false), 4.0);
        // Past the fastest step already: kept, never slowed by Up.
        assert_eq!(step(8.0, true), 8.0);
    }

    #[test]
    fn only_gaps_longer_than_the_setting_are_fast_forwarded() {
        // Kills at 217, 234, 247, 254: gaps of 17, 13 and 7 s.
        let kills = [217.0, 234.0, 247.0, 254.0];
        assert_eq!(
            fast_spans(&kills, 8.0),
            vec![
                (217.0 + crate::review::TAIL, 234.0 - crate::review::LEAD),
                (234.0 + crate::review::TAIL, 247.0 - crate::review::LEAD)
            ]
        );
        assert_eq!(fast_spans(&kills, 0.0), vec![]);
        assert_eq!(fast_spans(&kills, 20.0), vec![]);
    }

    #[test]
    fn a_gap_too_short_for_its_margins_plays_at_normal_speed() {
        // 5 s apart, over a 3 s setting, but crate::review::TAIL + crate::review::LEAD is 6 s.
        assert_eq!(fast_spans(&[10.0, 15.0], 3.0), vec![]);
    }

    #[test]
    fn the_speed_changes_only_while_a_highlight_plays() {
        let mut chosen = None;
        assert!(change(true, false, &mut chosen).is_err());
        assert_eq!(chosen, None);
    }

    #[test]
    fn a_speed_reads_as_a_multiple() {
        assert_eq!(text(2.0), "2x");
        assert_eq!(text(0.25), "0.25x");
        assert_eq!(text(0.5), "0.5x");
        assert!(is_normal(1.0));
        assert!(!is_normal(2.0));
    }
}
