//! The clock offset between two recordings of the same half (#708).
//!
//! Every POV recording of a half stamps its messages with the server's clock
//! (`svc_time` is `sv.time`), so two POV demos agree to the millisecond with
//! no offset. The HLTV recording does not: its `svc_time` is the proxy's own
//! clock, which starts when the proxy (re)joined the server plus the HLTV
//! delay, so it is behind or ahead of every POV demo of that half by one
//! constant (-26.8 s to +2.3 s over the 23 WSOD25 halves, drift under
//! 0.3 ms/min). Anything that maps a moment in one demo onto another needs
//! that constant; the kill feed gives it to about a millisecond, because both
//! recordings carry the same `DeathMsg` for every kill.

use crate::{Analysis, PlayerGlobalId, Weapon};
use std::collections::HashMap;

/// The result of [`kill_clock_offset`]: how far `b`'s `viewdemo` clock is
/// ahead of `a`'s, with how much the pairing it rests on is worth.
#[derive(Clone, Debug, PartialEq)]
pub struct ClockOffset {
    /// Median of `b - a` over the paired kills, in seconds on the `viewdemo`
    /// clock (`GameTime::viewdemo_offset`, the last `svc_time` before the
    /// message). Add it to a time in `a` to get the same moment in `b`.
    pub seconds: f64,
    /// Kills paired between the two analyses.
    pub pairs: usize,
    /// `max - min` of the paired differences, in seconds. About 0.1 s for a
    /// real pair: `svc_time` is stamped per packet, so the same `DeathMsg`
    /// lands up to a packet interval apart in two recordings.
    pub spread: f64,
    /// Kills in `a` with no partner in `b`.
    pub unpaired_a: usize,
    /// Kills in `b` with no partner in `a`.
    pub unpaired_b: usize,
}

/// Fewer paired kills than this and the result is `None`: a handful of pairs
/// cannot tell a real offset from a chance match of the same players and
/// weapon in two unrelated demos.
pub const MIN_PAIRS: usize = 10;

/// How far from the seed offset a candidate pair may fall, in seconds. It has
/// to be wider than the per-kill jitter (about 0.1 s) and narrower than the
/// gap between two kills with the same killer, victim and weapon, which is
/// tens of seconds in a match. Taken from the WSOD25 measurement script.
const PAIR_WINDOW_SECS: f64 = 6.0;

/// One kill as the pairing sees it: the key both recordings agree on, and
/// its time on the `viewdemo` clock.
#[derive(Clone, Debug)]
struct Kill {
    key: (PlayerGlobalId, PlayerGlobalId, Weapon),
    time: f64,
}

/// Every kill in `analysis` in order of its `viewdemo` time, keyed by the
/// players' global ids (SteamID64 where the demo carries one) and the weapon.
/// Teamkills and world kills are not in `kill_streaks` in either recording,
/// so leaving them out loses nothing.
fn kills(analysis: &Analysis) -> Vec<Kill> {
    let mut out: Vec<Kill> = analysis
        .state
        .players
        .iter()
        .flat_map(|killer| {
            killer.kill_streaks.iter().flat_map(move |streak| {
                streak.kills.iter().map(move |(time, weapon, victim)| Kill {
                    key: (killer.id.clone(), victim.clone(), weapon.clone()),
                    time: time.viewdemo_offset.as_secs_f64(),
                })
            })
        })
        .collect();
    out.sort_by(|x, y| x.time.total_cmp(&y.time));
    out
}

/// Median of `values`, which must not be empty.
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

/// Pairs the kills of two analyses of one half and returns `b`'s clock
/// offset from `a`'s, or `None` when fewer than [`MIN_PAIRS`] kills pair.
///
/// The rule, from the script that measured WSOD25 (`local/r-and-d/708`):
/// kills are grouped by (killer, victim, weapon). A seed offset is the median
/// of `b - a` for the keys whose kills count the same on both sides, paired
/// in order of occurrence (when no key does, the densest cluster of all
/// cross differences). Then every kill of `a` takes the unused kill of `b` with
/// the same key whose difference is nearest the seed, within
/// [`PAIR_WINDOW_SECS`]. The window is what lets a kill missing from one
/// side (a recording that started late, stopped early or dropped a packet)
/// leave the rest of that key's pairs alone instead of shifting them all by
/// one. The result is a median, not a mean, so one wrong pair or one demo
/// with a jumped frame clock (the rebuilt `wsod25_ply2_m3_h2_m00cat.dem` is
/// 12 ms off its co-recorders) does not move it.
pub fn kill_clock_offset(a: &Analysis, b: &Analysis) -> Option<ClockOffset> {
    let kills_a = kills(a);
    let kills_b = kills(b);

    let mut by_key_b: HashMap<&(PlayerGlobalId, PlayerGlobalId, Weapon), Vec<(f64, bool)>> =
        HashMap::new();
    for kill in &kills_b {
        by_key_b
            .entry(&kill.key)
            .or_default()
            .push((kill.time, false));
    }
    let mut by_key_a: HashMap<&(PlayerGlobalId, PlayerGlobalId, Weapon), Vec<f64>> = HashMap::new();
    for kill in &kills_a {
        by_key_a.entry(&kill.key).or_default().push(kill.time);
    }

    let mut seed_diffs: Vec<f64> = by_key_a
        .iter()
        .filter_map(|(key, times_a)| {
            let times_b = by_key_b.get(key)?;
            (times_a.len() == times_b.len())
                .then_some(times_a.iter().zip(times_b).map(|(ta, (tb, _))| tb - ta))
        })
        .flatten()
        .collect();
    if seed_diffs.is_empty() {
        // No key counts the same on both sides: one recording is missing a
        // large part of the half. Pairing in order would then put the first
        // kill of one against a later kill of the other for every key, so
        // the seed is the densest cluster of all cross differences instead,
        // which the true offset dominates (one hit per kill in common).
        let mut cross: Vec<f64> = by_key_a
            .iter()
            .filter_map(|(key, times_a)| {
                let times_b = by_key_b.get(key)?;
                Some(
                    times_a
                        .iter()
                        .flat_map(move |ta| times_b.iter().map(move |(tb, _)| tb - ta)),
                )
            })
            .flatten()
            .collect();
        cross.sort_by(f64::total_cmp);
        let mut best = 0..0;
        let mut start = 0;
        for end in 0..cross.len() {
            while cross[end] - cross[start] >= PAIR_WINDOW_SECS {
                start += 1;
            }
            if end + 1 - start > best.len() {
                best = start..end + 1;
            }
        }
        seed_diffs = cross[best].to_vec();
    }
    if seed_diffs.is_empty() {
        return None;
    }
    let seed = median(&mut seed_diffs);

    let mut diffs = Vec::new();
    let mut unpaired_a = 0;
    for kill in &kills_a {
        let Some(candidates) = by_key_b.get_mut(&kill.key) else {
            unpaired_a += 1;
            continue;
        };
        let best = candidates
            .iter()
            .enumerate()
            .filter(|(_, (_, used))| !used)
            .map(|(i, (tb, _))| (i, tb - kill.time))
            .filter(|(_, diff)| (diff - seed).abs() < PAIR_WINDOW_SECS)
            .min_by(|x, y| (x.1 - seed).abs().total_cmp(&(y.1 - seed).abs()));
        match best {
            Some((i, diff)) => {
                candidates[i].1 = true;
                diffs.push(diff);
            }
            None => unpaired_a += 1,
        }
    }
    if diffs.len() < MIN_PAIRS {
        return None;
    }
    let pairs = diffs.len();
    let unpaired_b = kills_b.len() - pairs;
    let spread = diffs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - diffs.iter().copied().fold(f64::INFINITY, f64::min);
    Some(ClockOffset {
        seconds: median(&mut diffs),
        pairs,
        spread,
        unpaired_a,
        unpaired_b,
    })
}

/// Whether `a` and `b` record the same half: more than half of the shorter
/// kill feed pairs. In WSOD25 every HLTV/POV pair of one half paired every
/// kill (22 of 23 halves, 285-344 kills) or 86% of them (one POV recording
/// that missed part of the half), and the HLTV demo against the same
/// players' other half paired too few kills for an offset at all, so the
/// threshold sits in an empty gap.
pub fn is_same_half(a: &Analysis, b: &Analysis) -> bool {
    let shorter = kills(a).len().min(kills(b).len());
    kill_clock_offset(a, b).is_some_and(|offset| offset.pairs * 2 > shorter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalyzerState, Player, kill::KillStreak, time::GameTime};
    use std::time::Duration;

    /// A kill at `time` seconds: `players[killer]` kills `players[victim]`
    /// with `weapon`.
    type Spec = (f64, usize, usize, Weapon);

    /// An analysis with four players and the given kills, each in its own
    /// streak (the pairing does not care how kills group into streaks).
    fn synthetic(kills: &[Spec]) -> Analysis {
        let mut state = AnalyzerState::default();
        for (slot, name) in ["axis1", "axis2", "allies1", "allies2"].iter().enumerate() {
            state.players.push(Player::new_mock(slot as u8, name));
        }
        for &(time, killer, victim, ref weapon) in kills {
            let victim_id = state.players[victim].id.clone();
            let game_time = GameTime {
                viewdemo_offset: Duration::from_secs_f64(time),
                ..Default::default()
            };
            state.players[killer].kill_streaks.push(KillStreak {
                kills: vec![(game_time, weapon.clone(), victim_id)],
            });
        }
        Analysis {
            demo_info: Default::default(),
            state,
        }
    }

    /// Twenty kills over ten minutes with the same four players and two
    /// weapons, so every key recurs several times.
    fn half() -> Vec<Spec> {
        (0..20)
            .map(|i| {
                let weapon = if i % 3 == 0 {
                    Weapon::K98
                } else {
                    Weapon::Garand
                };
                (30.0 + i as f64 * 31.7, i % 2, 2 + (i / 2) % 2, weapon)
            })
            .collect()
    }

    fn shifted(kills: &[Spec], by: f64) -> Vec<Spec> {
        kills
            .iter()
            .map(|(t, k, v, w)| (t + by, *k, *v, w.clone()))
            .collect()
    }

    #[test]
    fn a_shifted_copy_gives_the_shift_with_every_kill_paired() {
        let a = synthetic(&half());
        let b = synthetic(&shifted(&half(), 1.342));
        let offset = kill_clock_offset(&a, &b).unwrap();
        assert!((offset.seconds - 1.342).abs() < 1e-6, "{offset:?}");
        assert_eq!(
            (offset.pairs, offset.unpaired_a, offset.unpaired_b),
            (20, 0, 0)
        );
        assert!(offset.spread < 1e-6, "{offset:?}");
        // The other way round, the sign flips.
        let back = kill_clock_offset(&b, &a).unwrap();
        assert!((back.seconds + 1.342).abs() < 1e-6, "{back:?}");
    }

    #[test]
    fn missing_and_extra_kills_are_counted_and_do_not_move_the_offset() {
        let a = synthetic(&half());
        let mut b_kills = shifted(&half(), 1.342);
        // b misses three kills a has (one each of three different keys)...
        b_kills.remove(15);
        b_kills.remove(7);
        b_kills.remove(2);
        // ...and has two kills a lacks, with keys a also uses.
        b_kills.push((100.0, 0, 2, Weapon::Garand));
        b_kills.push((400.0, 1, 3, Weapon::K98));
        let b = synthetic(&b_kills);
        let offset = kill_clock_offset(&a, &b).unwrap();
        assert!((offset.seconds - 1.342).abs() < 1e-6, "{offset:?}");
        assert_eq!(
            (offset.pairs, offset.unpaired_a, offset.unpaired_b),
            (17, 3, 2),
            "{offset:?}"
        );
        assert!(offset.spread < 1e-6, "{offset:?}");
    }

    #[test]
    fn a_missed_kill_of_a_repeated_key_does_not_shift_later_pairs() {
        // Ten kills of one key every 30 s; b lacks the fifth. Pairing in
        // order alone would put every later pair 30 s out.
        let a_kills: Vec<Spec> = (0..10)
            .map(|i| (10.0 + 30.0 * i as f64, 0, 2, Weapon::K98))
            .collect();
        let mut b_kills = shifted(&a_kills, -9.982);
        b_kills.remove(4);
        // Padding so the result clears MIN_PAIRS.
        for i in 0..10 {
            a_kills_push(&mut b_kills, 500.0 + 20.0 * i as f64 - 9.982, i);
        }
        let mut a_kills = a_kills;
        for i in 0..10 {
            a_kills_push(&mut a_kills, 500.0 + 20.0 * i as f64, i);
        }
        let offset = kill_clock_offset(&synthetic(&a_kills), &synthetic(&b_kills)).unwrap();
        assert!((offset.seconds + 9.982).abs() < 1e-6, "{offset:?}");
        assert_eq!(
            (offset.pairs, offset.unpaired_a, offset.unpaired_b),
            (19, 1, 0),
            "{offset:?}"
        );
        assert!(offset.spread < 1e-6, "{offset:?}");
    }

    fn a_kills_push(kills: &mut Vec<Spec>, time: f64, i: usize) {
        let weapon = if i.is_multiple_of(2) {
            Weapon::Garand
        } else {
            Weapon::Mp40
        };
        kills.push((time, 1, 3, weapon));
    }

    #[test]
    fn per_kill_jitter_shows_as_spread_not_as_offset() {
        let a = synthetic(&half());
        let jittered: Vec<Spec> = half()
            .iter()
            .enumerate()
            .map(|(i, (t, k, v, w))| {
                let jitter = [-0.05, 0.02, 0.0, 0.04, -0.01][i % 5];
                (t + 1.342 + jitter, *k, *v, w.clone())
            })
            .collect();
        let offset = kill_clock_offset(&a, &synthetic(&jittered)).unwrap();
        assert!((offset.seconds - 1.342).abs() <= 0.02, "{offset:?}");
        assert!((offset.spread - 0.09).abs() < 1e-6, "{offset:?}");
        assert_eq!(offset.pairs, 20);
    }

    #[test]
    fn too_few_pairs_is_none() {
        let few: Vec<Spec> = half().into_iter().take(MIN_PAIRS - 1).collect();
        assert_eq!(kill_clock_offset(&synthetic(&few), &synthetic(&few)), None);
        assert_eq!(
            kill_clock_offset(&synthetic(&[]), &synthetic(&half())),
            None
        );
        // Same players, no kill in common: nothing pairs.
        let other: Vec<Spec> = (0..20)
            .map(|i| (50.0 + 40.0 * i as f64, 2, 0, Weapon::Thompson))
            .collect();
        assert_eq!(
            kill_clock_offset(&synthetic(&half()), &synthetic(&other)),
            None
        );
    }

    #[test]
    fn the_fixture_against_itself_is_zero_with_every_kill_paired() {
        let a = crate::tests_fixture::analysis();
        let total: usize = a
            .state
            .players
            .iter()
            .flat_map(|p| &p.kill_streaks)
            .map(|s| s.kills.len())
            .sum();
        let offset = kill_clock_offset(a, a).unwrap();
        assert_eq!(offset.seconds, 0.0);
        assert_eq!(offset.spread, 0.0);
        assert_eq!(
            (offset.pairs, offset.unpaired_a, offset.unpaired_b),
            (total, 0, 0)
        );
        assert!(total >= MIN_PAIRS, "{total} kills in the fixture");
        assert!(is_same_half(a, a));
    }

    #[test]
    fn same_half_needs_most_of_the_shorter_feed_to_pair() {
        let a = synthetic(&half());
        assert!(is_same_half(&a, &synthetic(&shifted(&half(), 1.342))));
        // A recording that missed the first half of the kills: no key counts
        // the same on both sides, so the seed comes from the densest cluster,
        // and all of its own kills still pair at the right offset.
        let partial: Vec<Spec> = shifted(&half(), 1.342).into_iter().skip(10).collect();
        let offset = kill_clock_offset(&a, &synthetic(&partial)).unwrap();
        assert!((offset.seconds - 1.342).abs() < 1e-6, "{offset:?}");
        assert_eq!(
            (offset.pairs, offset.unpaired_a, offset.unpaired_b),
            (10, 10, 0),
            "{offset:?}"
        );
        assert!(is_same_half(&a, &synthetic(&partial)));
        // Ten kills in common out of forty is not the same half.
        let mut mostly_other: Vec<Spec> = shifted(&half(), 1.342).into_iter().take(10).collect();
        mostly_other.extend((0..30).map(|i| (700.0 + 20.0 * i as f64, 2, 0, Weapon::Thompson)));
        assert!(!is_same_half(&a, &synthetic(&mostly_other)));
    }

    /// The WSOD25 pair the issue measured: HLTV against m00cat's POV of
    /// `grp2_h1`, +1.342 s with all 320 kills paired. Demos come from
    /// `DOD_ANALYSIS_DEMOS` (a folder) or the PRE install's `dod` folder.
    /// `cargo test -p analysis -- --ignored kill_offset`.
    #[test]
    #[ignore]
    fn kill_offset_of_the_wsod25_grp2_h1_pair_is_1_342() {
        let folder = std::env::var_os("DOD_ANALYSIS_DEMOS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
                )
            });
        let load = |name: &str| {
            let path = folder.join(name);
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("{}: {e} (set DOD_ANALYSIS_DEMOS)", path.display()));
            Analysis::try_from_bytes(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        let hltv = load("wsod25_grp2_h1_hltv.dem");
        let pov = load("wsod25_grp2_h1_m00cat.dem");

        let offset = kill_clock_offset(&hltv, &pov).expect("too few kills paired");
        println!("grp2_h1 hltv -> m00cat: {offset:?}");
        assert!((offset.seconds - 1.342).abs() <= 0.02, "{offset:?}");
        assert_eq!((offset.unpaired_a, offset.unpaired_b), (0, 0), "{offset:?}");
        assert!(offset.pairs >= 300, "{offset:?}");
        assert!(offset.spread < 0.2, "{offset:?}");

        // Two POV recordings share the server clock: no offset at all.
        let pov2 = load("wsod25_grp2_h1_stealth.dem");
        let pov_pov = kill_clock_offset(&pov, &pov2).expect("too few kills paired");
        println!("grp2_h1 m00cat -> stealth: {pov_pov:?}");
        assert!(pov_pov.seconds.abs() <= 0.02, "{pov_pov:?}");

        // The same players in another half: what a wrong pairing looks like.
        let other_half = load("wsod25_grp1_h1_m00cat.dem");
        println!(
            "grp2_h1 hltv -> grp1_h1 m00cat: {:?}",
            kill_clock_offset(&hltv, &other_half)
        );
        assert!(is_same_half(&hltv, &pov));
        assert!(is_same_half(&pov, &pov2));
        assert!(!is_same_half(&hltv, &other_half));
    }
}
