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
    fn a_speed_reads_as_a_multiple() {
        assert_eq!(text(2.0), "2x");
        assert_eq!(text(0.25), "0.25x");
        assert_eq!(text(0.5), "0.5x");
        assert!(is_normal(1.0));
        assert!(!is_normal(2.0));
    }
}
