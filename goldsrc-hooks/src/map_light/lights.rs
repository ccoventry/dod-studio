//! The point lights of a `<map>_light.txt` (#1310): a light line, and how
//! much light one adds to a luxel.
//!
//! `light x y z r g b strength radius`: where the light is (world units),
//! its colour (0 to 255 a channel), how strong it is and how far it reaches.
//! At the light itself a surface facing it gains `colour * strength` (in the
//! lightmap's own 0 to 255 units, before the engine's light gamma), so
//! `light ... 255 200 140 0.5 160` adds up to 128, 100, 70: a warm lamp.
//!
//! The light falls off smoothly to nothing at `radius`, `(1 - (d / r)^2)^2`:
//! no hard rim, and most of it near the middle, as a real lamp looks. A
//! surface gets it by how squarely it faces the light (Lambert's cosine), so
//! the back of a wall and a floor seen edge-on stay as they were. There are
//! no shadows: anything within the radius that faces the light is lit,
//! through walls too, so keep a light's radius inside the room it is in.

/// The most a light's strength may be: 4 x a full channel is already past
/// the 255 a luxel holds.
const MAX_STRENGTH: f32 = 4.0;
/// The longest reach accepted, in world units.
const MAX_RADIUS: f32 = 2048.0;
/// The numbers after the `light` keyword.
const NUMBERS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointLight {
    pub origin: [f32; 3],
    /// Colour times strength: what a surface facing the light gains at it.
    pub peak: [f32; 3],
    pub radius: f32,
}

/// A light line's numbers (after `light`) as a light, or why they aren't one.
pub(crate) fn parse_light(numbers: &[f32]) -> Result<PointLight, String> {
    if numbers.len() != NUMBERS {
        return Err(format!(
            "{} numbers after \"light\"; a light is light x y z r g b strength radius",
            numbers.len()
        ));
    }
    let colour = [numbers[3], numbers[4], numbers[5]];
    if colour.iter().any(|c| !(0.0..=255.0).contains(c)) {
        return Err(format!("colour {colour:?} is not 0 to 255 a channel"));
    }
    let strength = numbers[6];
    if !(strength > 0.0 && strength <= MAX_STRENGTH) {
        return Err(format!(
            "strength {strength} is not above 0 and at most {MAX_STRENGTH}"
        ));
    }
    let radius = numbers[7];
    if !(radius > 0.0 && radius <= MAX_RADIUS) {
        return Err(format!(
            "radius {radius} is not above 0 and at most {MAX_RADIUS}"
        ));
    }
    Ok(PointLight {
        origin: [numbers[0], numbers[1], numbers[2]],
        peak: colour.map(|c| c * strength),
        radius,
    })
}

impl PointLight {
    /// What the light adds to a luxel at `point` on a surface facing
    /// `facing` (unit length), per channel.
    pub(crate) fn added(&self, point: [f32; 3], facing: [f32; 3]) -> [f32; 3] {
        let to_light: [f32; 3] = std::array::from_fn(|i| self.origin[i] - point[i]);
        let distance_sq: f32 = to_light.iter().map(|d| d * d).sum();
        let reach = 1.0 - distance_sq / (self.radius * self.radius);
        if reach <= 0.0 {
            return [0.0; 3];
        }
        let distance = distance_sq.sqrt();
        // A light on the surface itself lights it fully.
        let cosine = if distance < 1e-3 {
            1.0
        } else {
            (0..3).map(|i| facing[i] * to_light[i]).sum::<f32>() / distance
        };
        if cosine <= 0.0 {
            return [0.0; 3];
        }
        let amount = reach * reach * cosine;
        self.peak.map(|p| p * amount)
    }

    /// Whether any point of the axis-aligned box `min..max` is within the
    /// light's radius.
    pub(crate) fn reaches(&self, min: [f32; 3], max: [f32; 3]) -> bool {
        let distance_sq: f32 = (0..3)
            .map(|i| {
                (min[i] - self.origin[i])
                    .max(self.origin[i] - max[i])
                    .max(0.0)
            })
            .map(|d| d * d)
            .sum();
        distance_sq < self.radius * self.radius
    }
}

/// What every light together adds to a luxel at `point` facing `facing`.
pub(crate) fn added(lights: &[PointLight], point: [f32; 3], facing: [f32; 3]) -> [f32; 3] {
    lights.iter().fold([0.0; 3], |sum, light| {
        let more = light.added(point, facing);
        std::array::from_fn(|i| sum[i] + more[i])
    })
}

/// One luxel channel with `amount` added, rounded, at most 255.
pub(crate) fn brightened(value: u8, amount: f32) -> u8 {
    (value as f32 + amount).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: [f32; 3] = [0.0, 0.0, 1.0];

    /// A warm lamp 32 units above the floor at z = 0, reaching 160.
    fn lamp() -> PointLight {
        parse_light(&[0.0, 0.0, 32.0, 255.0, 200.0, 100.0, 0.5, 160.0]).expect("a light")
    }

    #[test]
    fn a_light_line_gives_its_origin_peak_and_radius() {
        let light = lamp();
        assert_eq!(light.origin, [0.0, 0.0, 32.0]);
        assert_eq!(light.peak, [127.5, 100.0, 50.0]);
        assert_eq!(light.radius, 160.0);
    }

    #[test]
    fn a_light_line_with_bad_numbers_is_refused() {
        for numbers in [
            &[0.0, 0.0, 0.0, 255.0, 255.0, 255.0, 1.0][..],
            &[0.0, 0.0, 0.0, 300.0, 255.0, 255.0, 1.0, 100.0],
            &[0.0, 0.0, 0.0, 255.0, 255.0, 255.0, 0.0, 100.0],
            &[0.0, 0.0, 0.0, 255.0, 255.0, 255.0, 5.0, 100.0],
            &[0.0, 0.0, 0.0, 255.0, 255.0, 255.0, 1.0, 0.0],
        ] {
            assert!(parse_light(numbers).is_err(), "{numbers:?}");
        }
    }

    #[test]
    fn straight_below_the_light_the_floor_gets_nearly_the_peak() {
        let added = lamp().added([0.0, 0.0, 0.0], UP);
        // reach = 1 - (32/160)^2 = 0.96; 0.96^2 = 0.9216
        assert!((added[0] - 127.5 * 0.9216).abs() < 1e-3, "{added:?}");
    }

    #[test]
    fn the_light_fades_smoothly_to_nothing_at_its_radius() {
        let light = lamp();
        let near = light.added([40.0, 0.0, 0.0], UP)[0];
        let far = light.added([120.0, 0.0, 0.0], UP)[0];
        assert!(near > far && far > 0.0, "{near} {far}");
        assert_eq!(light.added([170.0, 0.0, 0.0], UP), [0.0; 3]);
    }

    #[test]
    fn a_surface_facing_away_gets_nothing() {
        assert_eq!(lamp().added([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]), [0.0; 3]);
    }

    #[test]
    fn a_surface_seen_at_a_slant_gets_less() {
        let light = lamp();
        let square = light.added([0.0, 0.0, 0.0], UP)[0];
        let slant = light.added([0.0, 0.0, 0.0], [0.6, 0.0, 0.8])[0];
        assert!((slant - square * 0.8).abs() < 1e-3, "{square} {slant}");
    }

    #[test]
    fn lights_add_up_and_a_channel_stops_at_255() {
        let both = added(&[lamp(), lamp()], [0.0, 0.0, 0.0], UP);
        assert!((both[1] - 2.0 * 100.0 * 0.9216).abs() < 1e-3, "{both:?}");
        assert_eq!(brightened(200, 100.0), 255);
        assert_eq!(brightened(20, 10.4), 30);
    }

    #[test]
    fn reach_is_the_radius_from_the_box() {
        let light = lamp();
        assert!(light.reaches([100.0, -10.0, 0.0], [110.0, 10.0, 0.0]));
        assert!(!light.reaches([200.0, -10.0, 0.0], [210.0, 10.0, 0.0]));
    }
}
