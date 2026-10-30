//! Upgrades for analyzer cache entries an older schema wrote, so a schema
//! bump does not mean parsing every demo again (a full parse of 460 demos
//! takes about 15 minutes on 6 threads; an upgrade, milliseconds each).
//!
//! An entry from `v<N>` goes through each step from `N` up to
//! [`SCHEMA_VERSION`](super::SCHEMA_VERSION), on its JSON: the structs the
//! older schema wrote no longer exist. A step may read parts the bump left
//! alone through today's types; should a later bump change one of those
//! types, the step fails to read it and asks for a re-parse rather than
//! misread it. When a step can't work out a new field from what the entry
//! holds, it says so ([`Upgrade::NeedsReparse`]) and the demo is analysed
//! again, as if there were no older entry. A bump with no step simply has no
//! upgrade: its old entries are a miss, as before upgrades existed.

use crate::objective::recount_cancels;
use crate::{CaptureAttempt, Connection, KillPosition, PlayerGlobalId};
use serde::Deserialize;
use serde_json::Value;

/// What one step made of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upgrade {
    /// The entry now holds what the step's `to` schema writes.
    Done,
    /// The entry lacks something the new schema needs, so the demo must be
    /// analysed again. The reason is counted and logged. The entry may be
    /// half rewritten and must be thrown away.
    NeedsReparse(&'static str),
}

/// One step: rewrites an entry's `analysis` from schema `from` to `to`.
pub struct Migration {
    pub from: u32,
    pub to: u32,
    pub upgrade: fn(&mut Value) -> Upgrade,
}

/// Every step there is. Numbers may be skipped: a step goes from one schema
/// that was written to disk to the next one that was.
pub const MIGRATIONS: &[Migration] = &[
    // v8 never shipped: #743 held it while #744 merged as 9, then took 10.
    Migration {
        from: 7,
        to: 9,
        upgrade: v7_to_v9,
    },
    Migration {
        from: 9,
        to: 10,
        upgrade: v9_to_v10,
    },
    Migration {
        from: 10,
        to: 11,
        upgrade: v10_to_v11,
    },
    Migration {
        from: 11,
        to: 12,
        upgrade: v11_to_v12,
    },
    Migration {
        from: 12,
        to: 13,
        upgrade: v12_to_v13,
    },
    // No step to 14 (#1128): a kill's headshot comes from the demo's helmet
    // events, which no entry keeps, so every entry with a kill would need a
    // re-parse anyway. With no step, older entries are a plain miss: the
    // rescan estimate counts them as full reads (an "upgradable" entry is
    // estimated at milliseconds), and the stale-cache sweep can delete the
    // old folders. The steps below still run in tests.
];

const MALFORMED: &str = "the entry is not laid out as its schema writes it";
const LEFT: &str = "a player with kills left before the demo ended";
const RECONNECTED: &str = "a player with kills reconnected";

/// The steps the cache runs: [`MIGRATIONS`], or in a test the table it set
/// with [`use_steps`].
#[cfg(not(test))]
fn steps() -> &'static [Migration] {
    MIGRATIONS
}

#[cfg(test)]
thread_local! {
    static TEST_STEPS: std::cell::Cell<&'static [Migration]> = const { std::cell::Cell::new(MIGRATIONS) };
}

#[cfg(test)]
fn steps() -> &'static [Migration] {
    TEST_STEPS.with(std::cell::Cell::get)
}

/// Puts back the steps a test replaced, when it ends.
#[cfg(test)]
pub(crate) struct StepsGuard(&'static [Migration]);

#[cfg(test)]
impl Drop for StepsGuard {
    fn drop(&mut self) {
        TEST_STEPS.with(|t| t.set(self.0));
    }
}

/// Makes the cache use `table` for its steps on this thread until the guard
/// drops. The cache's own tests need a schema that upgrades, which the real
/// table lacks right after a bump with no step (v14, #1128).
#[cfg(test)]
pub(crate) fn use_steps(table: &'static [Migration]) -> StepsGuard {
    StepsGuard(TEST_STEPS.with(|t| t.replace(table)))
}

/// v7 upgraded by the real steps as far as they go (v13), standing in for a
/// step to the current schema: what the cache's plumbing tests upgrade with.
#[cfg(test)]
pub(crate) const V7_UPGRADES: &[Migration] = &[Migration {
    from: 7,
    to: super::SCHEMA_VERSION,
    upgrade: |analysis| upgrade_with(MIGRATIONS, 7, 13, analysis),
}];

/// Whether an entry from schema `version` can be upgraded to the current one.
pub fn upgrades_from(version: u32) -> bool {
    version < super::SCHEMA_VERSION && chain(steps(), version, super::SCHEMA_VERSION).is_some()
}

/// Runs every step from schema `from` to the current one on `analysis` (an
/// entry's `analysis` object).
pub fn upgrade(from: u32, analysis: &mut Value) -> Upgrade {
    upgrade_with(steps(), from, super::SCHEMA_VERSION, analysis)
}

fn upgrade_with(migrations: &[Migration], from: u32, to: u32, analysis: &mut Value) -> Upgrade {
    let Some(steps) = chain(migrations, from, to) else {
        return Upgrade::NeedsReparse("no upgrade from its schema");
    };
    for step in steps {
        if let Upgrade::NeedsReparse(why) = (step.upgrade)(analysis) {
            return Upgrade::NeedsReparse(why);
        }
    }
    Upgrade::Done
}

/// The steps from `from` to `to`, in order, or `None` when one is missing.
fn chain(migrations: &[Migration], from: u32, to: u32) -> Option<Vec<&Migration>> {
    let mut steps = Vec::new();
    let mut at = from;
    while at != to {
        let step = migrations
            .iter()
            .find(|m| m.from == at && m.to > at && m.to <= to)?;
        steps.push(step);
        at = step.to;
    }
    Some(steps)
}

/// #744: each kill streak keeps `slot`, the slot its player held when the
/// kills happened. A player still connected at the end who never reconnected
/// scored every kill from the slot they end in, so that slot is the streak's.
/// Anyone else with kills (they left, or came back, maybe into another slot,
/// where v9 would also have split the streak) needs the demo.
fn v7_to_v9(analysis: &mut Value) -> Upgrade {
    let Some(players) = analysis
        .pointer_mut("/state/players")
        .and_then(Value::as_array_mut)
    else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    for player in players {
        let connection = player.get("connection").map(Connection::deserialize);
        let reconnected = player.get("has_reconnected").and_then(Value::as_bool);
        let (Some(Ok(connection)), Some(reconnected)) = (connection, reconnected) else {
            return Upgrade::NeedsReparse(MALFORMED);
        };
        // The slot every kill of theirs came from, or why it isn't known.
        let slot = match connection {
            Connection::Connected { .. } if reconnected => Err(RECONNECTED),
            Connection::Connected { client_id } => Ok(client_id),
            Connection::Disconnected => Err(LEFT),
        };
        let Some(streaks) = player.get_mut("kill_streaks").and_then(Value::as_array_mut) else {
            return Upgrade::NeedsReparse(MALFORMED);
        };
        for streak in streaks {
            let Some(streak) = streak.as_object_mut() else {
                return Upgrade::NeedsReparse(MALFORMED);
            };
            let Some(kills) = streak.get("kills").and_then(Value::as_array) else {
                return Upgrade::NeedsReparse(MALFORMED);
            };
            // A streak with no kills has no slot, as v9 writes it.
            let streak_slot = match slot {
                _ if kills.is_empty() => None,
                Ok(slot) => Some(slot),
                Err(why) => return Upgrade::NeedsReparse(why),
            };
            streak.insert("slot".into(), streak_slot.map_or(Value::Null, Value::from));
        }
    }
    Upgrade::Done
}

/// #743: cap breaks. Each cancelled attempt's `cancel_cause`, the players'
/// `cap_breaks` and the objectives' `blocks_unattributed` and
/// `attempts_abandoned`, worked out from the attempts and kills the entry
/// already holds by the live analysis' own rule (`objective::recount_cancels`).
/// Nothing was renamed on disk: `is_break` (now `taken_from_enemy`) was a
/// method, never a field.
fn v9_to_v10(analysis: &mut Value) -> Upgrade {
    let Some(state) = analysis.get_mut("state") else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    let Some(Ok(kills)) = state
        .get("kill_positions")
        .map(Vec::<KillPosition>::deserialize)
    else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    let Some(Ok(attempts)) = state
        .pointer("/objectives/attempts")
        .map(Vec::<CaptureAttempt>::deserialize)
    else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    let mut tally = match recount_cancels(&attempts, &kills) {
        Ok(tally) => tally,
        Err(why) => return Upgrade::NeedsReparse(why),
    };

    let Some(objectives) = state.get_mut("objectives").and_then(Value::as_object_mut) else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    let Some(attempts) = objectives.get_mut("attempts").and_then(Value::as_array_mut) else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    for (attempt, cause) in attempts.iter_mut().zip(&tally.causes) {
        let (Some(attempt), Ok(cause)) = (attempt.as_object_mut(), serde_json::to_value(cause))
        else {
            return Upgrade::NeedsReparse(MALFORMED);
        };
        attempt.insert("cancel_cause".into(), cause);
    }
    objectives.insert(
        "blocks_unattributed".into(),
        tally.blocks_unattributed.into(),
    );
    objectives.insert("attempts_abandoned".into(), tally.attempts_abandoned.into());

    let Some(players) = state.get_mut("players").and_then(Value::as_array_mut) else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    for player in players {
        let id = player.get("id").map(PlayerGlobalId::deserialize);
        let (Some(Ok(id)), Some(player)) = (id, player.as_object_mut()) else {
            return Upgrade::NeedsReparse(MALFORMED);
        };
        // Credited to the first player with the id, as the live analysis
        // finds them.
        let breaks = tally.breaks.remove(&id).unwrap_or(0);
        player.insert("cap_breaks".into(), breaks.into());
    }
    Upgrade::Done
}

/// #811: `demo_info.damage`, where a damaged demo stopped being read. A v10
/// entry is for a demo that read to the end (a damaged one had no analysis),
/// so it has none, which is what the field's default says.
fn v10_to_v11(analysis: &mut Value) -> Upgrade {
    match analysis.get("demo_info") {
        Some(Value::Object(_)) => Upgrade::Done,
        _ => Upgrade::NeedsReparse(MALFORMED),
    }
}

/// More frames than this per second of playback means the frame clock barely
/// moves. Recorded demos run at a few hundred frames a second (457 for the CI
/// fixture, 478 for a WSOD25 POV demo); both files found with no camera data
/// (#920) ran 236,586 frames in 0.001 s.
const STALLED_FRAMES_PER_SECOND: f64 = 20_000.0;

const STALLED_CLOCK: &str = "its frame clock barely moves, as in a demo with no camera data";

/// #920: `demo_info.no_camera_data`. Telling needs every frame's view, which
/// the entry doesn't hold, but every demo found without one also has a frame
/// clock that never moves, and the entry holds the clock: such a demo is
/// read again, and any other is taken to have a camera.
fn v11_to_v12(analysis: &mut Value) -> Upgrade {
    let Some(info) = analysis.get_mut("demo_info").and_then(Value::as_object_mut) else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    let time = info.get("playback_time").and_then(Value::as_f64);
    let frames = info.get("playback_frames").and_then(Value::as_f64);
    let (Some(time), Some(frames)) = (time, frames) else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    if frames > time * STALLED_FRAMES_PER_SECOND {
        return Upgrade::NeedsReparse(STALLED_CLOCK);
    }
    info.insert("no_camera_data".into(), false.into());
    Upgrade::Done
}

const ROCKET_OPENS_STREAK: &str = "a kill streak starts with a rocket kill";

/// #1091: a rocket kill after its shooter's death joins the life it was
/// fired in. v12 put it first in the next life's streak, so only an entry
/// with a streak that starts with a rocket kill can differ; every other entry
/// is what v13 writes.
fn v12_to_v13(analysis: &mut Value) -> Upgrade {
    let Some(players) = analysis.pointer("/state/players").and_then(Value::as_array) else {
        return Upgrade::NeedsReparse(MALFORMED);
    };
    for player in players {
        let Some(streaks) = player.get("kill_streaks").and_then(Value::as_array) else {
            return Upgrade::NeedsReparse(MALFORMED);
        };
        for streak in streaks {
            let Some(kills) = streak.get("kills").and_then(Value::as_array) else {
                return Upgrade::NeedsReparse(MALFORMED);
            };
            let Some(first) = kills.first() else {
                continue;
            };
            let Some(Ok(weapon)) = first.get(1).map(dod::Weapon::deserialize) else {
                return Upgrade::NeedsReparse(MALFORMED);
            };
            if weapon.is_rocket() {
                return Upgrade::NeedsReparse(ROCKET_OPENS_STREAK);
            }
        }
    }
    Upgrade::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Analysis;
    use serde_json::json;

    fn step(_: &mut Value) -> Upgrade {
        Upgrade::Done
    }

    fn refuse(_: &mut Value) -> Upgrade {
        Upgrade::NeedsReparse("refused")
    }

    const STEPS: &[Migration] = &[
        Migration {
            from: 3,
            to: 4,
            upgrade: step,
        },
        Migration {
            from: 4,
            to: 6,
            upgrade: step,
        },
        Migration {
            from: 6,
            to: 7,
            upgrade: refuse,
        },
    ];

    fn hops(from: u32, to: u32) -> Option<Vec<(u32, u32)>> {
        chain(STEPS, from, to).map(|steps| steps.iter().map(|m| (m.from, m.to)).collect())
    }

    #[test]
    fn a_chain_follows_the_steps_and_may_skip_a_number() {
        assert_eq!(hops(3, 6), Some(vec![(3, 4), (4, 6)]));
        assert_eq!(hops(4, 6), Some(vec![(4, 6)]));
        assert_eq!(hops(6, 6), Some(vec![]));
        // No step starts at 5 (never written), 2 or 7.
        assert_eq!(hops(5, 6), None);
        assert_eq!(hops(2, 6), None);
        assert_eq!(hops(7, 6), None);
        // A step past the target is not taken.
        assert_eq!(hops(4, 5), None);
    }

    #[test]
    fn a_missing_or_refusing_step_asks_for_a_reparse() {
        let mut value = json!({});
        assert_eq!(upgrade_with(STEPS, 3, 6, &mut value), Upgrade::Done);
        assert_eq!(
            upgrade_with(STEPS, 3, 7, &mut value),
            Upgrade::NeedsReparse("refused")
        );
        assert!(matches!(
            upgrade_with(STEPS, 5, 6, &mut value),
            Upgrade::NeedsReparse(_)
        ));
    }

    #[test]
    fn the_real_chain_goes_7_9_10_11_12_13_and_8_has_none() {
        let hops: Vec<_> = chain(MIGRATIONS, 7, 13)
            .unwrap()
            .iter()
            .map(|m| (m.from, m.to))
            .collect();
        assert_eq!(hops, [(7, 9), (9, 10), (10, 11), (11, 12), (12, 13)]);
        assert!(!upgrades_from(8) && !upgrades_from(6));
        assert!(!upgrades_from(super::super::SCHEMA_VERSION));
    }

    /// v14 (#1128) has no step: a v13 entry has no helmet events to tell a
    /// headshot from, so it, and every older one, is a miss. An "upgradable"
    /// entry would be estimated at milliseconds by the rescan prompt while
    /// it re-parses in full, and would keep its old folder from the sweep.
    #[test]
    fn no_entry_older_than_14_upgrades() {
        assert_eq!(super::super::SCHEMA_VERSION, 14);
        for version in 0..14 {
            assert!(!upgrades_from(version), "v{version}");
        }
    }

    /// The chain up to v13, the last schema with a step: what the fixture's
    /// entries still upgrade through, so the steps stay tested.
    fn upgrade_to_13(from: u32, analysis: &mut Value) -> Upgrade {
        upgrade_with(MIGRATIONS, from, 13, analysis)
    }

    fn player(id: &str, connection: Value, reconnected: bool, streaks: Value) -> Value {
        json!({
            "id": id,
            "connection": connection,
            "has_reconnected": reconnected,
            "kill_streaks": streaks,
        })
    }

    fn kill() -> Value {
        json!([{"real_offset": {"secs": 1, "nanos": 0}, "viewdemo_offset": {"secs": 1, "nanos": 0}, "frame_index": 5}, "K98", "victim"])
    }

    fn analysis_with(players: Vec<Value>) -> Value {
        json!({"state": {"players": players}})
    }

    #[test]
    fn v7_streaks_take_the_end_slot_of_a_player_who_stayed() {
        let mut value = analysis_with(vec![
            player(
                "a",
                json!({"Connected": {"client_id": 4}}),
                false,
                json!([{"kills": [kill(), kill()]}, {"kills": []}]),
            ),
            // Left with no kills: nothing to fill.
            player("b", json!("Disconnected"), true, json!([{"kills": []}])),
        ]);
        assert_eq!(v7_to_v9(&mut value), Upgrade::Done);
        let streaks = &value["state"]["players"][0]["kill_streaks"];
        assert_eq!(streaks[0]["slot"], json!(4));
        assert_eq!(streaks[1]["slot"], Value::Null);
        assert_eq!(
            value["state"]["players"][1]["kill_streaks"][0]["slot"],
            Value::Null
        );
    }

    #[test]
    fn v7_needs_the_demo_for_a_player_with_kills_who_left_or_reconnected() {
        for (connection, reconnected, why) in [
            (json!("Disconnected"), false, LEFT),
            (json!({"Connected": {"client_id": 2}}), true, RECONNECTED),
        ] {
            let mut value = analysis_with(vec![player(
                "a",
                connection,
                reconnected,
                json!([{"kills": [kill()]}]),
            )]);
            assert_eq!(v7_to_v9(&mut value), Upgrade::NeedsReparse(why));
        }
    }

    #[test]
    fn a_malformed_entry_asks_for_a_reparse_and_never_panics() {
        for mut value in [
            json!(null),
            json!([]),
            json!({"state": {}}),
            json!({"state": {"players": 3}}),
            analysis_with(vec![json!({"id": 1})]),
            analysis_with(vec![player("a", json!("Elsewhere"), false, json!([]))]),
            analysis_with(vec![player("a", json!("Disconnected"), false, json!([7]))]),
            json!({"state": {"kill_positions": [], "objectives": {"attempts": "x"}, "players": []}}),
            json!({"state": {"kill_positions": [{"time": 1}], "objectives": {"attempts": []}, "players": []}}),
            json!({"state": {"kill_positions": [], "objectives": {"attempts": []}, "players": [{"id": 5}]}}),
        ] {
            for from in [7, 8, 9] {
                assert!(
                    matches!(upgrade_to_13(from, &mut value), Upgrade::NeedsReparse(_)),
                    "{from}: {value}"
                );
            }
        }
    }

    /// The parts of an analysis each step rewrites, as the current schema
    /// writes them, stripped back to what a v7 entry holds.
    fn as_v7(analysis: &Analysis) -> Value {
        let mut value = serde_json::to_value(analysis).unwrap();
        let state = value["state"].as_object_mut().unwrap();
        for player in state["players"].as_array_mut().unwrap() {
            let player = player.as_object_mut().unwrap();
            player.remove("cap_breaks");
            for streak in player["kill_streaks"].as_array_mut().unwrap() {
                streak.as_object_mut().unwrap().remove("slot");
            }
        }
        let objectives = state["objectives"].as_object_mut().unwrap();
        objectives.remove("blocks_unattributed");
        objectives.remove("attempts_abandoned");
        for attempt in objectives["attempts"].as_array_mut().unwrap() {
            attempt.as_object_mut().unwrap().remove("cancel_cause");
        }
        value
    }

    #[test]
    fn a_v7_entry_of_the_fixture_upgrades_to_exactly_a_fresh_analysis() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let fresh = Analysis::try_from_bytes(&bytes).unwrap();
        let mut old = as_v7(&fresh);
        assert_ne!(old, serde_json::to_value(&fresh).unwrap());
        assert_eq!(upgrade_to_13(7, &mut old), Upgrade::Done);
        let upgraded: Analysis = serde_json::from_value(old).unwrap();
        assert_eq!(
            serde_json::to_value(&upgraded).unwrap(),
            serde_json::to_value(&fresh).unwrap()
        );
        // The fields that matter, said plainly.
        let slots = |a: &Analysis| -> Vec<Option<u8>> {
            crate::highlights::highlights(a)
                .all
                .iter()
                .map(|h| h.slot)
                .collect()
        };
        assert_eq!(slots(&upgraded), slots(&fresh));
        assert!(slots(&upgraded).iter().all(Option::is_some));
    }

    #[test]
    fn v9_cap_breaks_are_recounted_from_the_attempts_and_kills() {
        let time = |secs: u64, frame: usize| json!({"real_offset": {"secs": secs, "nanos": 0}, "viewdemo_offset": {"secs": secs, "nanos": 0}, "frame_index": frame});
        let kill = |secs, frame, killer: &str, killer_team: &str, victim_team: &str| {
            json!({"time": time(secs, frame), "weapon": "K98", "killer": killer, "victim": "v",
                "killer_team": killer_team, "victim_team": victim_team, "teamkill": killer_team == victim_team,
                "killer_origin": null, "victim_origin": null, "distance": null})
        };
        let attempt = |started, ended, outcome: &str| json!({"area_index": 1, "team": "Axis", "started": started, "ended": ended, "outcome": outcome});
        let mut value = json!({"state": {
            "kill_positions": [
                kill(14, 140, "b1", "Allies", "Axis"),
                kill(24, 240, "a2", "Axis", "Axis"),
            ],
            "objectives": {"attempts": [
                attempt(time(10, 100), time(15, 150), "Cancelled"),
                attempt(time(20, 200), time(25, 250), "Cancelled"),
                attempt(time(30, 300), time(35, 350), "Cancelled"),
                attempt(time(40, 400), time(45, 450), "Captured"),
            ]},
            "players": [{"id": "a2"}, {"id": "b1"}],
        }});
        assert_eq!(v9_to_v10(&mut value), Upgrade::Done);
        let state = &value["state"];
        let causes: Vec<&Value> = state["objectives"]["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| &a["cancel_cause"])
            .collect();
        assert_eq!(
            causes,
            [
                &json!({"Break": {"breaker": "b1"}}),
                &json!("Unattributed"),
                &json!("Abandoned"),
                &Value::Null
            ]
        );
        assert_eq!(state["objectives"]["blocks_unattributed"], json!(1));
        assert_eq!(state["objectives"]["attempts_abandoned"], json!(1));
        assert_eq!(state["players"][0]["cap_breaks"], json!(0));
        assert_eq!(state["players"][1]["cap_breaks"], json!(1));
    }

    #[test]
    fn a_v10_entry_loads_as_a_demo_that_read_to_the_end() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let fresh = Analysis::try_from_bytes(&bytes).unwrap();
        let mut old = serde_json::to_value(&fresh).unwrap();
        old["demo_info"].as_object_mut().unwrap().remove("damage");
        assert_eq!(upgrade_to_13(10, &mut old), Upgrade::Done);
        let upgraded: Analysis = serde_json::from_value(old).unwrap();
        assert_eq!(upgraded.demo_info.damage, None);
        assert_eq!(
            serde_json::to_value(&upgraded).unwrap(),
            serde_json::to_value(&fresh).unwrap()
        );
        assert_eq!(
            upgrade_to_13(10, &mut json!({"state": {}})),
            Upgrade::NeedsReparse(MALFORMED)
        );
    }

    #[test]
    fn a_v11_entry_with_a_running_clock_loads_as_a_demo_with_a_camera() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let fresh = Analysis::try_from_bytes(&bytes).unwrap();
        assert!(!fresh.demo_info.no_camera_data);
        let mut old = serde_json::to_value(&fresh).unwrap();
        old["demo_info"]
            .as_object_mut()
            .unwrap()
            .remove("no_camera_data");
        assert_eq!(upgrade_to_13(11, &mut old), Upgrade::Done);
        let upgraded: Analysis = serde_json::from_value(old).unwrap();
        assert_eq!(
            serde_json::to_value(&upgraded).unwrap(),
            serde_json::to_value(&fresh).unwrap()
        );
    }

    #[test]
    fn a_v11_entry_whose_clock_never_moves_needs_the_demo() {
        // The #920 file's directory: 236,586 frames lasting 0.001 s.
        let mut value = json!({"demo_info": {"playback_time": 0.001, "playback_frames": 236_586}});
        assert_eq!(v11_to_v12(&mut value), Upgrade::NeedsReparse(STALLED_CLOCK));
        let mut value = json!({"demo_info": {"playback_time": 0.0, "playback_frames": 3}});
        assert_eq!(v11_to_v12(&mut value), Upgrade::NeedsReparse(STALLED_CLOCK));
        // No frames at all: nothing to read again.
        let mut value = json!({"demo_info": {"playback_time": 0.0, "playback_frames": 0}});
        assert_eq!(v11_to_v12(&mut value), Upgrade::Done);
        assert_eq!(value["demo_info"]["no_camera_data"], json!(false));
        for mut value in [
            json!({}),
            json!({"demo_info": {"playback_frames": 10}}),
            json!({"demo_info": {"playback_time": "x", "playback_frames": 10}}),
        ] {
            assert_eq!(v11_to_v12(&mut value), Upgrade::NeedsReparse(MALFORMED));
        }
    }

    #[test]
    fn a_v12_entry_needs_the_demo_only_when_a_streak_starts_with_a_rocket() {
        use crate::kill::KillStreak;
        let streak = |weapons: &[dod::Weapon]| {
            let mut s = KillStreak::default();
            for w in weapons {
                let victim = serde_json::from_value(json!("STEAM_0:0:1")).unwrap();
                s.kills.push((Default::default(), w.clone(), victim));
            }
            serde_json::to_value(&s).unwrap()
        };
        let entry =
            |streaks: Vec<Value>| json!({"state": {"players": [{"kill_streaks": streaks}]}});

        let mut value = entry(vec![
            streak(&[]),
            streak(&[dod::Weapon::K98, dod::Weapon::Bazooka]),
            streak(&[dod::Weapon::StickGrenade]),
        ]);
        assert_eq!(v12_to_v13(&mut value), Upgrade::Done);
        for rocket in [
            dod::Weapon::Bazooka,
            dod::Weapon::Panzerschreck,
            dod::Weapon::Piat,
        ] {
            let mut value = entry(vec![
                streak(&[dod::Weapon::K98]),
                streak(&[rocket, dod::Weapon::K98]),
            ]);
            assert_eq!(
                v12_to_v13(&mut value),
                Upgrade::NeedsReparse(ROCKET_OPENS_STREAK)
            );
        }
        for mut value in [
            json!({}),
            json!({"state": {"players": [{}]}}),
            json!({"state": {"players": [{"kill_streaks": [{"kills": [[0, "Laser", 1]]}]}]}}),
        ] {
            assert_eq!(v12_to_v13(&mut value), Upgrade::NeedsReparse(MALFORMED));
        }
    }

    #[test]
    fn v9_needs_the_demo_for_a_cancel_with_no_end_time() {
        let mut value = json!({"state": {
            "kill_positions": [],
            "objectives": {"attempts": [{"area_index": 1, "team": "Axis",
                "started": {"real_offset": {"secs": 1, "nanos": 0}, "viewdemo_offset": {"secs": 1, "nanos": 0}, "frame_index": 1},
                "ended": null, "outcome": "Cancelled"}]},
            "players": [],
        }});
        assert_eq!(
            v9_to_v10(&mut value),
            Upgrade::NeedsReparse("a cancelled capture attempt has no end time")
        );
    }
}
