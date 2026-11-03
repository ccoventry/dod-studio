//! The light boxes of a `<map>_light.txt`: a box line, and how much a luxel
//! at a given point is brightened.
//!
//! `x1 y1 z1 x2 y2 z2 scale [fade]`: two opposite corners in world units
//! (any order), the factor the baked light inside is multiplied by, and
//! optionally how far outside the box, in world units, the change fades out
//! (0, a hard edge, when left out).
//!
//! Where boxes overlap, they apply in the file's order, each pulling the
//! factor towards its own by its weight there: a later box wins where it
//! fully covers, which lets a small box inside a big one set its own scale.

/// A box's scale must be above 0 (0 would turn the light off: a typo, not
/// a wish) and at most this; 255 is the most a luxel holds anyway.
pub(crate) const MAX_SCALE: f32 = 16.0;
/// The longest fade accepted, in world units.
const MAX_FADE: f32 = 1024.0;
/// A point this close outside a box counts as inside: a luxel on a wall the
/// box's face was drawn along, give or take float error.
const EDGE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LightBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub scale: f32,
    pub fade: f32,
}

/// A box line's numbers as a box, or why they aren't one.
pub(crate) fn parse_box(numbers: &[f32]) -> Result<LightBox, String> {
    if !(7..=8).contains(&numbers.len()) {
        return Err(format!(
            "{} numbers; a box is x1 y1 z1 x2 y2 z2 scale [fade]",
            numbers.len()
        ));
    }
    let scale = numbers[6];
    if !(scale > 0.0 && scale <= MAX_SCALE) {
        return Err(format!(
            "scale {scale} is not above 0 and at most {MAX_SCALE}"
        ));
    }
    let fade = numbers.get(7).copied().unwrap_or(0.0);
    if !(0.0..=MAX_FADE).contains(&fade) {
        return Err(format!("fade {fade} is not 0 to {MAX_FADE}"));
    }
    Ok(LightBox {
        min: std::array::from_fn(|i| numbers[i].min(numbers[i + 3])),
        max: std::array::from_fn(|i| numbers[i].max(numbers[i + 3])),
        scale,
        fade,
    })
}

impl LightBox {
    /// How fully the box covers `point`: 1 inside it, falling to 0 at
    /// `fade` units outside (at once without a fade).
    pub(crate) fn weight(&self, point: [f32; 3]) -> f32 {
        let outside: f32 = (0..3)
            .map(|i| {
                (self.min[i] - point[i])
                    .max(point[i] - self.max[i])
                    .max(0.0)
            })
            .map(|d| d * d)
            .sum::<f32>()
            .sqrt();
        let past_edge = (outside - EDGE).max(0.0);
        if past_edge == 0.0 {
            1.0
        } else if self.fade > 0.0 {
            (1.0 - past_edge / self.fade).max(0.0)
        } else {
            0.0
        }
    }

    /// Whether any point of the axis-aligned box `min..max` is within the
    /// box's reach (the box, its fade and the edge tolerance).
    pub(crate) fn reaches(&self, min: [f32; 3], max: [f32; 3]) -> bool {
        let reach = self.fade + EDGE;
        (0..3).all(|i| max[i] >= self.min[i] - reach && min[i] <= self.max[i] + reach)
    }
}

/// The factor a luxel at `point` is multiplied by: 1 where no box reaches.
pub(crate) fn gain(boxes: &[LightBox], point: [f32; 3]) -> f32 {
    boxes.iter().fold(1.0, |gain, light| {
        let weight = light.weight(point);
        gain + (light.scale - gain) * weight
    })
}

/// One luxel channel multiplied by `gain`, rounded, at most 255.
pub(crate) fn scaled(value: u8, gain: f32) -> u8 {
    (value as f32 * gain).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> LightBox {
        let numbers: Vec<f32> = line
            .split_whitespace()
            .map(|w| w.parse().expect("a number"))
            .collect();
        parse_box(&numbers).expect("a box")
    }

    #[test]
    fn a_box_line_gives_its_corners_in_order_and_its_scale() {
        let found = one("1712 414 -200 1584 30 -290 1.5");
        assert_eq!(found.min, [1584.0, 30.0, -290.0]);
        assert_eq!(found.max, [1712.0, 414.0, -200.0]);
        assert_eq!(found.scale, 1.5);
        assert_eq!(found.fade, 0.0);
    }

    #[test]
    fn an_eighth_number_is_the_fade() {
        assert_eq!(one("0 0 0 10 10 10 2 32").fade, 32.0);
    }

    #[test]
    fn a_scale_out_of_range_or_a_wrong_count_is_refused() {
        for numbers in [
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 17.0][..],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, -4.0],
            &[0.0, 0.0, 0.0, 1.0, 1.0],
        ] {
            assert!(parse_box(numbers).is_err(), "{numbers:?}");
        }
    }

    #[test]
    fn inside_a_box_the_weight_is_one_and_outside_zero_without_a_fade() {
        let light = one("0 0 0 100 100 100 2");
        assert_eq!(light.weight([50.0, 50.0, 50.0]), 1.0);
        assert_eq!(
            light.weight([100.5, 50.0, 50.0]),
            1.0,
            "on the face, within EDGE"
        );
        assert_eq!(light.weight([110.0, 50.0, 50.0]), 0.0);
    }

    #[test]
    fn a_fade_falls_off_with_the_distance_outside() {
        let light = one("0 0 0 100 100 100 2 40");
        let half = light.weight([100.0 + EDGE + 20.0, 50.0, 50.0]);
        assert!((half - 0.5).abs() < 1e-4, "{half}");
        assert_eq!(light.weight([200.0, 50.0, 50.0]), 0.0);
    }

    #[test]
    fn the_gain_is_one_where_no_box_reaches_and_the_scale_inside() {
        let boxes = [one("0 0 0 100 100 100 2.5")];
        assert_eq!(gain(&boxes, [500.0, 0.0, 0.0]), 1.0);
        assert_eq!(gain(&boxes, [50.0, 50.0, 50.0]), 2.5);
    }

    #[test]
    fn a_later_box_wins_where_it_fully_covers() {
        let boxes = [one("0 0 0 100 100 100 2"), one("40 40 40 60 60 60 1.2")];
        assert_eq!(gain(&boxes, [50.0, 50.0, 50.0]), 1.2);
        assert_eq!(gain(&boxes, [10.0, 10.0, 10.0]), 2.0);
    }

    #[test]
    fn reach_covers_the_fade() {
        let light = one("0 0 0 100 100 100 2 32");
        assert!(light.reaches([120.0, 0.0, 0.0], [130.0, 10.0, 10.0]));
        assert!(!light.reaches([140.0, 0.0, 0.0], [150.0, 10.0, 10.0]));
    }

    #[test]
    fn a_scaled_channel_rounds_and_stops_at_255() {
        assert_eq!(scaled(20, 1.5), 30);
        assert_eq!(scaled(25, 1.5), 38);
        assert_eq!(scaled(200, 2.0), 255);
        assert_eq!(scaled(100, 0.5), 50);
    }
}
