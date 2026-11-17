//! The view data a repaired demo plays (#15): where the player is, where they
//! look and their health, carried by every network frame's `demo_info` and
//! every client-data frame.
//!
//! Damage that reaches a frame's body without breaking its header leaves
//! frames that read and carry nonsense: in `m3_h2` the first network frame
//! after the hole holds a view origin of 6.3e10, a yaw of 1.1e12 and a health
//! of -2147483548 (the top byte of each flipped), and every ramp frame copied
//! it, so the screen stayed black for the whole 22 s cut on both engine
//! builds. A frame whose view is out of range takes the view of a clean frame
//! of its own kind.

use dem::types::{ClientData, Demo, DemoInfo, Frame, FrameData};

/// The furthest from the world's origin a real position is, in units. A map's
/// playable space is at most ±4096 units with stock compile tools and ±8192
/// with extended ones; twice the larger leaves a spectator or a body past a
/// map's edge in range, and the damage measured (2.4e9 and beyond, a flipped
/// exponent bit) is nowhere near it.
const MAX_COORD: f32 = 16_384.0;
/// The largest angle a real view holds, in degrees. The engine keeps angles
/// within a turn of 0; ten turns allows for an angle never wrapped back, and
/// the damage measured (1.1e12 and 4.9e21 degrees of yaw) is far past it.
const MAX_ANGLE: f32 = 3_600.0;
/// The largest health a real player has, either sign. DoD's players have
/// 100 and a dead one can read below 0; ±1000 leaves room for a server's own
/// settings, while the damage measured (-2147483548, the sign bit set on 100)
/// is far outside.
const MAX_HEALTH: i32 = 1_000;

fn coords_in_range(v: &[f32]) -> bool {
    v.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORD)
}

fn angles_in_range(v: &[f32]) -> bool {
    v.iter().all(|a| a.is_finite() && a.abs() <= MAX_ANGLE)
}

/// Whether a network frame's view (its `demo_info`) is in range: view and
/// simulated origin, view and client angles, and health.
pub(crate) fn info_in_range(info: &DemoInfo) -> bool {
    let r = &info.refparams;
    coords_in_range(&r.view_origin)
        && coords_in_range(&r.sim_org)
        && angles_in_range(&r.view_angles)
        && angles_in_range(&r.cl_viewangles)
        && (-MAX_HEALTH..=MAX_HEALTH).contains(&r.health)
}

/// Whether a client-data frame's origin and view angles are in range.
pub(crate) fn client_data_in_range(data: &ClientData) -> bool {
    coords_in_range(&data.origin) && angles_in_range(&data.viewangles)
}

/// The kinds of frame that carry a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewKind {
    Network,
    ClientData,
}

/// A frame's view: its kind and whether it is in range. `None` for a frame
/// that carries none.
fn judge(frame: &Frame) -> Option<(ViewKind, bool)> {
    match &frame.frame_data {
        FrameData::NetworkMessage(n) => Some((ViewKind::Network, info_in_range(&n.1.info))),
        FrameData::ClientData(c) => Some((ViewKind::ClientData, client_data_in_range(c))),
        _ => None,
    }
}

/// For each frame whose view is out of range, the clean frame of the same
/// kind it takes its view from: `(damaged, source)` indices into `judged`,
/// in order.
///
/// The next clean frame, else the one before. For a ramp that is the first
/// clean frame after the hole, the spot it holds the view on anyway (#1375):
/// the cut then lands where the recording resumes rather than jumping back
/// before the hole and forward again. The one before is for damage that
/// nothing clean follows, at the end of a directory entry. A frame with no
/// clean frame of its kind on either side is left out.
fn view_sources(judged: &[Option<(ViewKind, bool)>]) -> Vec<(usize, usize)> {
    if !judged.iter().any(|j| matches!(j, Some((_, false)))) {
        return Vec::new();
    }
    let mut pairs = Vec::new();
    for kind in [ViewKind::Network, ViewKind::ClientData] {
        let of_kind: Vec<(usize, bool)> = judged
            .iter()
            .enumerate()
            .filter_map(|(i, j)| match j {
                Some((k, in_range)) if *k == kind => Some((i, *in_range)),
                _ => None,
            })
            .collect();
        let mut next_clean = vec![None; of_kind.len()];
        let mut found = None;
        for (n, &(i, in_range)) in of_kind.iter().enumerate().rev() {
            if in_range {
                found = Some(i);
            }
            next_clean[n] = found;
        }
        let mut last_clean = None;
        for (n, &(i, in_range)) in of_kind.iter().enumerate() {
            if in_range {
                last_clean = Some(i);
            } else if let Some(source) = next_clean[n].or(last_clean) {
                pairs.push((i, source));
            }
        }
    }
    pairs.sort_unstable();
    pairs
}

/// Gives every playback frame whose view is out of range the view of a clean
/// frame of its kind in the same directory entry ([`view_sources`]): a
/// network frame its `demo_info`, a client-data frame its whole body. The
/// messages a network frame carries are left as they are. Returns how many
/// frames were restated; one with no clean frame of its kind in its entry is
/// left alone.
pub(crate) fn restate_damaged_views(demo: &mut Demo) -> usize {
    let mut restated = 0;
    for entry in demo.directory.entries.iter_mut().skip(1) {
        let judged: Vec<Option<(ViewKind, bool)>> = entry.frames.iter().map(judge).collect();
        for (damaged, source) in view_sources(&judged) {
            // Only the view is cloned: a network frame's messages can be
            // kilobytes, and a ramp is up to 3,000 frames.
            let clean = match &entry.frames[source].frame_data {
                FrameData::NetworkMessage(n) => CleanView::Info(Box::new(n.1.info.clone())),
                FrameData::ClientData(c) => CleanView::ClientData(c.clone()),
                _ => continue,
            };
            match (&mut entry.frames[damaged].frame_data, clean) {
                (FrameData::NetworkMessage(n), CleanView::Info(info)) => n.1.info = *info,
                (FrameData::ClientData(c), CleanView::ClientData(clean)) => *c = clean,
                _ => continue,
            }
            restated += 1;
        }
    }
    restated
}

/// The view a damaged frame takes from a clean one.
enum CleanView {
    Info(Box<DemoInfo>),
    ClientData(ClientData),
}

#[cfg(test)]
mod tests {
    use super::*;

    const NET_OK: Option<(ViewKind, bool)> = Some((ViewKind::Network, true));
    const NET_BAD: Option<(ViewKind, bool)> = Some((ViewKind::Network, false));
    const CD_OK: Option<(ViewKind, bool)> = Some((ViewKind::ClientData, true));
    const CD_BAD: Option<(ViewKind, bool)> = Some((ViewKind::ClientData, false));

    #[test]
    fn a_damaged_view_takes_the_next_clean_frame_of_its_kind() {
        // A ramp of damaged network frames, a damaged client-data frame, then
        // the recording resuming clean.
        let judged = [NET_OK, NET_BAD, NET_BAD, None, CD_BAD, CD_OK, NET_OK];
        assert_eq!(view_sources(&judged), [(1, 6), (2, 6), (4, 5)]);
    }

    #[test]
    fn a_damaged_view_with_nothing_clean_after_takes_the_one_before() {
        let judged = [CD_OK, NET_OK, NET_BAD, CD_BAD, None];
        assert_eq!(view_sources(&judged), [(2, 1), (3, 0)]);
    }

    #[test]
    fn a_damaged_view_with_no_clean_frame_of_its_kind_is_left_alone() {
        assert_eq!(view_sources(&[NET_BAD, CD_OK]), []);
        assert_eq!(view_sources(&[NET_OK, CD_OK, None]), []);
    }

    #[test]
    fn the_damage_measured_in_m3_h2_is_out_of_range_and_real_views_are_not() {
        let real = ClientData {
            origin: vec![1240.7578, 235.45313, -589.96875],
            viewangles: vec![-0.63891006, 264.74854, 0.0],
            weapon_bits: 0,
            fov: 90.0,
        };
        assert!(client_data_in_range(&real));
        let damaged = ClientData {
            viewangles: vec![-0.6772866, 4.8837485e21, 0.0],
            ..real.clone()
        };
        assert!(!client_data_in_range(&damaged));
        let off_the_world = ClientData {
            origin: vec![1240.7891, 6.321_235_6e10, -9151.0],
            ..real.clone()
        };
        assert!(!client_data_in_range(&off_the_world));
        let not_a_number = ClientData {
            origin: vec![f32::NAN, 0.0, 0.0],
            ..real
        };
        assert!(!client_data_in_range(&not_a_number));
    }
}
