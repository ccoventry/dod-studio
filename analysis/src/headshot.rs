//! Which kills were headshots (#1128), told from the server's helmet event.
//!
//! The game plays `events/effects/helmet.sc` (the helmet flying off) on the
//! frame a headshot kills, 0.0 to 0.25 s from the kill's `DeathMsg`. Every
//! demo carries it, POV and HLTV alike. Measured on the 14 players' POV demos
//! of the `solifotw-` set against the recorder's own head-hit red screen
//! (#606): 97% agreement over 383 deaths.
//!
//! The event names no entity (its `entindex` is 1023), so a pop is paired to
//! a kill by time, then by position when two kills share a frame. What the
//! demo cannot say is [`Headshot::Unknown`], never a guess:
//!
//! - two kills on one frame and one pop that position cannot tell apart;
//! - a British victim: the British have no helmet, so no pop is no evidence
//!   (`dbo1`'s 21 deaths had none).
//!
//! An HLTV demo loses some events (the same gap as its missing gunshots), so
//! its `No` means "no pop was seen"; its headshot counts are a lower bound.
//!
//! Pairing needs every kill of the demo, so it runs when the demo ends
//! ([`use_headshot_updates`]), after teams are final.

use crate::entity_replay::field_f32;
use crate::position::KillPosition;
use crate::time::GameTime;
use crate::{AnalyzerEvent, AnalyzerState};
use dem::bit::BitSliceCast;
use dem::types::{Delta, EngineMessage};
use dod::Team;
use std::collections::HashSet;

/// The resource name of the helmet-pop event.
const HELMET_EVENT: &str = "events/effects/helmet.sc";
/// The resource type `svc_resourcelist` gives an event.
const RESOURCE_TYPE_EVENT: u8 = 5;
/// How long after a kill's `DeathMsg` its pop may come, in seconds.
const POP_AFTER_KILL_SECS: f32 = 0.25;
/// How long before it, in seconds: a pop and its kill on adjacent frames can
/// arrive in either order, but a pop further back belongs to an earlier kill.
const POP_BEFORE_KILL_SECS: f32 = 0.05;
/// The furthest a pop's origin (the victim's head) may be from the victim's
/// origin, in world units, to count as theirs.
const HEAD_REACH: f32 = 120.0;

/// Whether a kill was a headshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Headshot {
    Yes,
    No,
    /// The demo cannot say (see the module docs). Also what a kill is before
    /// pairing has run.
    #[default]
    Unknown,
}

/// One helmet pop.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HelmetPop {
    pub at_secs: f32,
    /// Where the helmet left from, when the event says.
    pub origin: Option<[f32; 3]>,
}

/// Working state behind `Headshot`. Never serialized.
#[derive(Debug, Default)]
pub(crate) struct HeadshotTracker {
    helmet_event_ids: HashSet<u32>,
    pops: Vec<HelmetPop>,
}

/// An event's `origin[0..3]`, when the event carries all three.
fn event_origin(delta: &Delta) -> Option<[f32; 3]> {
    let mut origin = [None; 3];
    for (key, value) in delta {
        let axis = match key.as_bytes() {
            b"origin[0]\0" => 0,
            b"origin[1]\0" => 1,
            b"origin[2]\0" => 2,
            _ => continue,
        };
        origin[axis] = field_f32(value);
    }
    Some([origin[0]?, origin[1]?, origin[2]?])
}

/// Collects helmet pops as the demo plays, and on `Finalization` sets
/// `headshot` on every `kill_positions` entry.
pub fn use_headshot_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    match event {
        AnalyzerEvent::EngineMessage(EngineMessage::SvcResourceList(list)) => {
            for resource in &list.resources {
                if resource.type_.to_u8() != RESOURCE_TYPE_EVENT {
                    continue;
                }
                let name = resource.name.get_string();
                if name.trim_end_matches('\0').trim() == HELMET_EVENT {
                    state
                        .headshots
                        .helmet_event_ids
                        .insert(resource.index.to_u32());
                }
            }
        }
        AnalyzerEvent::EngineMessage(EngineMessage::SvcEvent(events)) => {
            for e in &events.events {
                if !state
                    .headshots
                    .helmet_event_ids
                    .contains(&e.event_index.to_u32())
                {
                    continue;
                }
                state.headshots.pops.push(HelmetPop {
                    at_secs: state.current_time.real_offset.as_secs_f32(),
                    origin: e.delta.as_ref().and_then(event_origin),
                });
            }
        }
        AnalyzerEvent::Finalization => {
            let verdicts = pair(&state.kill_positions, &state.headshots.pops);
            for (kill, verdict) in state.kill_positions.iter_mut().zip(verdicts) {
                kill.headshot = verdict;
            }
        }
        _ => {}
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// What one pop turned out to belong to.
enum Owner {
    /// This kill (an index into the kills).
    Kill(usize),
    /// One of these kills, and nothing tells which.
    Either(Vec<usize>),
    /// No kill: a helmet that came off a player who lived.
    None,
}

/// The kill a pop belongs to: the nearest in time, and among kills on the
/// same frame the one whose victim stood at the pop's origin.
fn owner_of(pop: &HelmetPop, kills: &[KillPosition]) -> Owner {
    let mut candidates: Vec<(usize, f32)> = kills
        .iter()
        .enumerate()
        .filter_map(|(i, kill)| {
            let after = pop.at_secs - kill.time.real_offset.as_secs_f32();
            (-POP_BEFORE_KILL_SECS..=POP_AFTER_KILL_SECS)
                .contains(&after)
                .then_some((i, after.abs()))
        })
        .collect();
    let Some(nearest) = candidates.iter().map(|c| c.1).reduce(f32::min) else {
        return Owner::None;
    };
    candidates.retain(|c| c.1 == nearest);
    if let [(only, _)] = candidates[..] {
        return Owner::Kill(only);
    }
    let by_position: Option<Vec<(usize, f32)>> = pop.origin.and_then(|head| {
        candidates
            .iter()
            .map(|(i, _)| Some((*i, distance(head, kills[*i].victim_origin?))))
            .collect()
    });
    if let Some(mut near) = by_position {
        near.sort_by(|a, b| a.1.total_cmp(&b.1));
        // The nearest victim, when it is within reach and clearly nearer than
        // the next one.
        if near[0].1 <= HEAD_REACH && near[1].1 > near[0].1 * 1.5 {
            return Owner::Kill(near[0].0);
        }
    }
    Owner::Either(candidates.into_iter().map(|c| c.0).collect())
}

/// The verdict for every kill, in `kills`' order.
pub(crate) fn pair(kills: &[KillPosition], pops: &[HelmetPop]) -> Vec<Headshot> {
    let mut verdicts: Vec<Option<Headshot>> = vec![None; kills.len()];
    let mut unsure: HashSet<usize> = HashSet::new();
    for pop in pops {
        match owner_of(pop, kills) {
            Owner::Kill(i) => verdicts[i] = Some(Headshot::Yes),
            Owner::Either(maybe) => unsure.extend(maybe),
            Owner::None => {}
        }
    }
    kills
        .iter()
        .zip(verdicts)
        .enumerate()
        .map(|(i, (kill, verdict))| match verdict {
            Some(verdict) => verdict,
            None if kill.victim_team == Some(Team::British) || unsure.contains(&i) => {
                Headshot::Unknown
            }
            None => Headshot::No,
        })
        .collect()
}

/// The verdict recorded for the kill of `victim` at `time`, `Unknown` when
/// the demo has no such kill. `kills` is `kill_positions`, in time order.
pub fn headshot_of(
    kills: &[KillPosition],
    time: &GameTime,
    victim: &crate::PlayerGlobalId,
) -> Headshot {
    let first = kills.partition_point(|k| k.time.frame_index < time.frame_index);
    kills[first..]
        .iter()
        .take_while(|k| k.time.frame_index == time.frame_index)
        .find(|k| k.victim.as_ref() == Some(victim))
        .map_or(Headshot::Unknown, |k| k.headshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dod::Weapon;
    use std::time::Duration;

    fn kill(secs: f32, victim_origin: Option<[f32; 3]>, victim_team: Team) -> KillPosition {
        KillPosition {
            time: GameTime {
                real_offset: Duration::from_secs_f32(secs),
                viewdemo_offset: Duration::from_secs_f32(secs),
                frame_index: (secs * 100.0) as usize,
            },
            weapon: Weapon::Garand,
            killer: None,
            victim: None,
            killer_team: None,
            victim_team: Some(victim_team),
            teamkill: false,
            killer_origin: None,
            victim_origin,
            distance: None,
            headshot: Headshot::Unknown,
        }
    }

    fn pop(secs: f32, origin: Option<[f32; 3]>) -> HelmetPop {
        HelmetPop {
            at_secs: secs,
            origin,
        }
    }

    #[test]
    fn a_pop_on_the_kills_frame_makes_it_a_headshot() {
        let kills = [kill(10.0, None, Team::Axis), kill(20.0, None, Team::Axis)];
        let verdicts = pair(&kills, &[pop(10.0, None)]);
        assert_eq!(verdicts, [Headshot::Yes, Headshot::No]);
    }

    #[test]
    fn a_pop_a_fraction_after_the_kill_still_counts() {
        let kills = [kill(10.0, None, Team::Axis)];
        assert_eq!(pair(&kills, &[pop(10.2, None)]), [Headshot::Yes]);
        assert_eq!(pair(&kills, &[pop(10.4, None)]), [Headshot::No]);
    }

    #[test]
    fn a_pop_well_before_a_kill_is_not_its_pop() {
        let kills = [kill(10.0, None, Team::Axis)];
        assert_eq!(pair(&kills, &[pop(9.9, None)]), [Headshot::No]);
    }

    #[test]
    fn a_pop_goes_to_the_nearer_of_two_kills_on_different_frames() {
        let kills = [kill(10.0, None, Team::Axis), kill(10.2, None, Team::Axis)];
        let verdicts = pair(&kills, &[pop(10.2, None)]);
        assert_eq!(verdicts, [Headshot::No, Headshot::Yes]);
    }

    #[test]
    fn two_kills_on_one_frame_are_told_apart_by_where_the_victims_stood() {
        let kills = [
            kill(10.0, Some([0.0, 0.0, 0.0]), Team::Axis),
            kill(10.0, Some([500.0, 0.0, 0.0]), Team::Axis),
        ];
        let verdicts = pair(&kills, &[pop(10.0, Some([480.0, 5.0, 40.0]))]);
        assert_eq!(verdicts, [Headshot::No, Headshot::Yes]);
    }

    #[test]
    fn two_kills_on_one_frame_with_no_positions_are_both_unknown() {
        let kills = [kill(10.0, None, Team::Axis), kill(10.0, None, Team::Axis)];
        assert_eq!(
            pair(&kills, &[pop(10.0, None)]),
            [Headshot::Unknown, Headshot::Unknown]
        );
    }

    #[test]
    fn two_victims_standing_together_are_unknown_even_with_a_position() {
        let kills = [
            kill(10.0, Some([0.0, 0.0, 0.0]), Team::Axis),
            kill(10.0, Some([30.0, 0.0, 0.0]), Team::Axis),
        ];
        assert_eq!(
            pair(&kills, &[pop(10.0, Some([15.0, 0.0, 40.0]))]),
            [Headshot::Unknown, Headshot::Unknown]
        );
    }

    #[test]
    fn a_pop_far_from_both_victims_is_not_given_to_either() {
        let kills = [
            kill(10.0, Some([0.0, 0.0, 0.0]), Team::Axis),
            kill(10.0, Some([500.0, 0.0, 0.0]), Team::Axis),
        ];
        let verdicts = pair(&kills, &[pop(10.0, Some([250.0, 0.0, 0.0]))]);
        assert_eq!(verdicts, [Headshot::Unknown, Headshot::Unknown]);
    }

    #[test]
    fn a_double_kill_with_a_pop_each_is_two_headshots() {
        let kills = [kill(10.0, None, Team::Axis), kill(10.0, None, Team::Axis)];
        let pops = [
            pop(10.0, Some([0.0, 0.0, 0.0])),
            pop(10.0, Some([900.0, 0.0, 0.0])),
        ];
        // No victim positions: each pop alone is ambiguous, so both stay unsure.
        assert_eq!(pair(&kills, &pops), [Headshot::Unknown, Headshot::Unknown]);
        let kills = [
            kill(10.0, Some([0.0, 0.0, 0.0]), Team::Axis),
            kill(10.0, Some([900.0, 0.0, 0.0]), Team::Axis),
        ];
        assert_eq!(pair(&kills, &pops), [Headshot::Yes, Headshot::Yes]);
    }

    #[test]
    fn a_british_victim_with_no_pop_is_unknown_not_no() {
        let kills = [
            kill(10.0, None, Team::British),
            kill(20.0, None, Team::Axis),
        ];
        assert_eq!(pair(&kills, &[]), [Headshot::Unknown, Headshot::No]);
    }

    #[test]
    fn a_british_victim_with_a_pop_is_a_headshot() {
        let kills = [kill(10.0, None, Team::British)];
        assert_eq!(pair(&kills, &[pop(10.0, None)]), [Headshot::Yes]);
    }

    #[test]
    fn a_pop_with_no_kill_is_ignored() {
        let kills = [kill(10.0, None, Team::Axis)];
        assert_eq!(pair(&kills, &[pop(50.0, None)]), [Headshot::No]);
    }

    #[test]
    fn the_verdict_is_found_by_frame_and_victim() {
        let id = |name: &str| crate::Player::new_mock(1, name).id;
        let mut a = kill(10.0, None, Team::Axis);
        a.victim = Some(id("a"));
        a.headshot = Headshot::Yes;
        let mut b = kill(10.0, None, Team::Axis);
        b.victim = Some(id("b"));
        b.headshot = Headshot::No;
        let kills = [a, b];
        let at = kills[1].time.clone();
        assert_eq!(headshot_of(&kills, &at, &id("b")), Headshot::No);
        assert_eq!(headshot_of(&kills, &at, &id("zz")), Headshot::Unknown);
    }

    /// Times of the recorder's head-hit red screen (ScreenFade 128,0,0, #606).
    fn red_screens(bytes: &[u8]) -> Vec<f32> {
        use dem::types::{FrameData, MessageData, NetMessage};
        let demo = dem::open_demo_from_bytes(bytes).expect("demo opens");
        let mut times = Vec::new();
        for entry in demo.directory.entries.iter() {
            for frame in &entry.frames {
                let FrameData::NetworkMessage(nm) = &frame.frame_data else {
                    continue;
                };
                let MessageData::Parsed(messages) = &nm.1.messages else {
                    continue;
                };
                for message in messages {
                    if let NetMessage::UserMessage(um) = message
                        && um.name.starts_with(b"ScreenFade")
                        && um.data.len() >= 10
                        && um.data[6..9] == [128, 0, 0]
                    {
                        times.push(frame.time);
                    }
                }
            }
        }
        times
    }

    /// The recorder's deaths in `analysis`, as the time of each.
    fn recorder_deaths(analysis: &crate::Analysis) -> Vec<f32> {
        let slot = analysis.state.pov_player_index;
        let recorder = analysis.state.players.iter().find(|p| {
            matches!(p.connection, crate::Connection::Connected { client_id }
                if Some(client_id) == slot)
        });
        let Some(recorder) = recorder else {
            return Vec::new();
        };
        analysis
            .state
            .kill_positions
            .iter()
            .filter(|k| k.victim.as_ref() == Some(&recorder.id))
            .map(|k| k.time.real_offset.as_secs_f32())
            .collect()
    }

    /// How well the helmet pop agrees with the recorder's own red screen, per
    /// demo (#1128): the check the 97% over 14 players came from. Demos from
    /// `DOD_ANALYSIS_DEMOS` (a folder), else the PRE install's `dod` folder;
    /// `solifotw-*.dem` and `wsod25_*.dem` POV demos. Fails when it finds none.
    /// `cargo test -p analysis --release -- --ignored --nocapture helmet_pop`.
    #[test]
    #[ignore]
    fn the_helmet_pop_agrees_with_the_red_screen_on_real_demos() {
        let folder = std::env::var_os("DOD_ANALYSIS_DEMOS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
                )
            });
        let mut demos: Vec<_> = std::fs::read_dir(&folder)
            .unwrap_or_else(|e| panic!("{}: {e} (set DOD_ANALYSIS_DEMOS)", folder.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                name.ends_with(".dem")
                    && (name.starts_with("solifotw-") || name.starts_with("wsod25_"))
                    && !name.contains("hltv")
            })
            .collect();
        demos.sort();
        assert!(!demos.is_empty(), "no demos in {}", folder.display());
        // (red and helmet, red only, helmet only, neither)
        let mut total = [0usize; 4];
        for path in &demos {
            let bytes = std::fs::read(path).unwrap();
            let Ok(analysis) = crate::Analysis::try_from_bytes(&bytes) else {
                continue;
            };
            let red = red_screens(&bytes);
            let mut row = [0usize; 4];
            for at in recorder_deaths(&analysis) {
                let kill = analysis
                    .state
                    .kill_positions
                    .iter()
                    .find(|k| (k.time.real_offset.as_secs_f32() - at).abs() < 1e-6)
                    .unwrap();
                let is_red = red.iter().any(|r| (r - at).abs() <= 0.1);
                let is_helmet = kill.headshot == Headshot::Yes;
                row[match (is_red, is_helmet) {
                    (true, true) => 0,
                    (true, false) => 1,
                    (false, true) => 2,
                    (false, false) => 3,
                }] += 1;
            }
            println!(
                "{}: red+helmet {}, red only {}, helmet only {}, neither {}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                row[0],
                row[1],
                row[2],
                row[3]
            );
            for (t, r) in total.iter_mut().zip(row) {
                *t += r;
            }
        }
        let deaths: usize = total.iter().sum();
        let agree = (total[0] + total[3]) as f32 / deaths.max(1) as f32;
        println!(
            "{} demos, {deaths} deaths, agreement {:.1}%",
            demos.len(),
            agree * 100.0
        );
        assert!(deaths > 0, "no recorder deaths found");
        assert!(agree >= 0.9, "agreement {agree}");
    }
}
