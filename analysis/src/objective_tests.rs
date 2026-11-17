use super::*;
use crate::{Player, time::use_timing_updates};
use dem::types::{Frame, FrameData};

// Wire team bytes.
const ALLIES: u8 = 1;
const AXIS: u8 = 2;

fn run(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    use_timing_updates(state, event);
    use_objective_updates(state, event);
}

fn next_frame(state: &mut AnalyzerState) {
    let frame = Frame {
        time: 0.0,
        frame: 0,
        frame_data: FrameData::DemoStart,
    };
    run(state, &AnalyzerEvent::Frame(&frame));
}

/// Sends one user message, as its wire bytes.
fn send(state: &mut AnalyzerState, name: &str, data: &[u8]) {
    let Ok(msg) = UserMessage::new(name.as_bytes(), data) else {
        panic!("{name} {data:?} did not parse");
    };
    run(state, &AnalyzerEvent::UserMessage(msg));
}

fn obj_score(state: &mut AnalyzerState, client_index: u8, score: i16) {
    let [lo, hi] = score.to_le_bytes();
    send(state, "ObjScore", &[client_index, lo, hi]);
}

fn cap_msg(state: &mut AnalyzerState, client_index: u8, name: &str, team: u8) {
    let mut data = vec![client_index];
    data.extend_from_slice(name.as_bytes());
    data.extend_from_slice(&[0, team]);
    send(state, "CapMsg", &data);
}

fn player(slot: u8, name: &str, team: Team) -> Player {
    let mut player = Player::new_mock(slot, name);
    player.team = Some(team);
    player
}

fn id(state: &AnalyzerState, name: &str) -> PlayerGlobalId {
    get(state, name).id.clone()
}

fn get<'a>(state: &'a AnalyzerState, name: &str) -> &'a Player {
    state.players.iter().find(|p| p.name == name).unwrap()
}

fn outcomes(state: &AnalyzerState) -> Vec<AttemptOutcome> {
    state
        .objectives
        .attempts
        .iter()
        .map(|a| a.outcome.clone())
        .collect()
}

/// Slots 0-2 Axis, slot 3 Allies. Client indices on the wire are 1-based.
fn two_teams() -> AnalyzerState {
    let mut state = AnalyzerState::default();
    state.players.push(player(0, "a1", Team::Axis));
    state.players.push(player(1, "a2", Team::Axis));
    state.players.push(player(2, "a3", Team::Axis));
    state.players.push(player(3, "b1", Team::Allies));
    next_frame(&mut state);
    state
}

#[test]
fn co_cappers_are_the_same_frame_obj_score_rises() {
    let mut state = two_teams();
    // As the wire orders a capture: the ObjScores, then SetObj, then CapMsg.
    obj_score(&mut state, 1, 1);
    obj_score(&mut state, 2, 1);
    obj_score(&mut state, 4, 1); // an Allied player, not a co-capper
    send(&mut state, "SetObj", &[1, AXIS, 0]);
    cap_msg(&mut state, 2, "Grassy Knoll", AXIS);
    obj_score(&mut state, 3, 0); // no rise, no credit
    next_frame(&mut state);

    let captures = &state.objectives.captures;
    assert_eq!(captures.len(), 1);
    let capture = &captures[0];
    assert_eq!(capture.capper, Some(id(&state, "a2")));
    assert_eq!(capture.co_cappers, vec![id(&state, "a1")]);
    assert_eq!(capture.area_index, Some(1));
    assert_eq!(capture.team, Team::Axis);
    // No InitObj and no earlier SetObj: who held it before is unknown.
    assert_eq!(capture.previous_owner, None);
    assert!(!capture.taken_from_enemy());

    assert_eq!(get(&state, "a1").cap_credits, 1);
    assert_eq!(get(&state, "a2").cap_credits, 1);
    assert_eq!(get(&state, "a3").cap_credits, 0);
    assert_eq!(get(&state, "b1").cap_credits, 0);
    assert_eq!(get(&state, "b1").obj_points, 1);

    let flag = state.objectives.flag(1).unwrap();
    assert_eq!(flag.name.as_deref(), Some("Grassy Knoll"));
    assert_eq!(flag.owner, Team::Axis);
}

#[test]
fn a_rise_in_a_neighbouring_frame_is_not_a_co_capper() {
    let mut state = two_teams();
    obj_score(&mut state, 1, 1);
    next_frame(&mut state);
    send(&mut state, "SetObj", &[1, AXIS, 0]);
    cap_msg(&mut state, 2, "Grassy Knoll", AXIS);
    next_frame(&mut state);

    let capture = &state.objectives.captures[0];
    assert!(capture.co_cappers.is_empty());
    assert_eq!(get(&state, "a1").cap_credits, 0);
    assert_eq!(get(&state, "a2").cap_credits, 1);
}

#[test]
fn taking_an_enemy_flag_is_taken_from_enemy_and_the_learned_name_finds_its_area() {
    let mut state = two_teams();
    send(&mut state, "SetObj", &[0, ALLIES, 0]);
    cap_msg(&mut state, 4, "Church", ALLIES);
    next_frame(&mut state);

    send(&mut state, "SetObj", &[0, AXIS, 0]);
    cap_msg(&mut state, 1, "Church", AXIS);
    next_frame(&mut state);

    // No SetObj this time: the name learned above still places it.
    cap_msg(&mut state, 4, "Church", ALLIES);
    next_frame(&mut state);

    let captures = &state.objectives.captures;
    assert!(!captures[0].taken_from_enemy());
    assert!(captures[1].taken_from_enemy());
    assert_eq!(captures[1].previous_owner, Some(Team::Allies));
    assert_eq!(captures[1].owners_before, vec![(0, Team::Allies)]);
    assert_eq!(captures[2].area_index, Some(0));
}

#[test]
fn objective_points_add_up_rises_across_a_counter_reset() {
    let mut state = two_teams();
    for value in [1, 3, 3, 0, 1, 1] {
        obj_score(&mut state, 1, value);
        next_frame(&mut state);
    }
    // 1, then 2 more, a resend, a reset to 0, 1 more, and a resend.
    assert_eq!(get(&state, "a1").obj_points, 4);
}

#[test]
fn a_cancelled_attempt_is_a_block_for_the_defenders() {
    let mut state = two_teams();
    send(&mut state, "StartProg", &[1, AXIS, 2, 0]);
    next_frame(&mut state);
    send(&mut state, "CancelProg", &[1, 0]);
    next_frame(&mut state);

    send(&mut state, "StartProg", &[1, AXIS, 2, 0]);
    next_frame(&mut state);
    send(&mut state, "SetObj", &[1, AXIS, 0]);
    cap_msg(&mut state, 1, "Grassy Knoll", AXIS);
    next_frame(&mut state);

    assert_eq!(
        outcomes(&state),
        [AttemptOutcome::Cancelled, AttemptOutcome::Captured]
    );
    assert_eq!(state.objectives.blocked_attempts(&Team::Axis), 1);
    assert_eq!(state.objectives.blocked_attempts(&Team::Allies), 0);
}

#[test]
fn a_cancel_in_the_frame_the_flag_falls_is_not_a_block() {
    let mut state = two_teams();
    send(&mut state, "StartProg", &[1, AXIS, 2, 0]);
    next_frame(&mut state);
    send(&mut state, "CancelProg", &[1, 0]);
    send(&mut state, "SetObj", &[1, AXIS, 0]);
    cap_msg(&mut state, 1, "Grassy Knoll", AXIS);
    next_frame(&mut state);

    assert_eq!(outcomes(&state), [AttemptOutcome::Captured]);
    assert_eq!(state.objectives.blocked_attempts(&Team::Axis), 0);
}

#[test]
fn a_restart_or_a_round_reset_ends_an_attempt_without_a_block() {
    let mut state = two_teams();
    send(&mut state, "StartProg", &[2, ALLIES, 5, 0]);
    next_frame(&mut state);
    send(&mut state, "StartProg", &[2, ALLIES, 5, 0]);
    next_frame(&mut state);
    send(&mut state, "RoundState", &[0]);
    // The round reset's own SetObjs hand flags back; none is a capture.
    send(&mut state, "SetObj", &[2, 0, 0]);
    next_frame(&mut state);

    assert_eq!(
        outcomes(&state),
        [AttemptOutcome::Restarted, AttemptOutcome::RoundEnded]
    );
    assert_eq!(state.objectives.blocked_attempts(&Team::Allies), 0);
    assert!(state.objectives.captures.is_empty());
    assert_eq!(state.objectives.flag(2).unwrap().owner, Team::Unassigned);
}

#[test]
fn init_obj_lays_out_the_flags() {
    let mut state = two_teams();
    // Per objective: u16 entity, u8 area, u8 team, four icon bytes, i16 x, i16 y.
    let mut data = vec![2];
    data.extend_from_slice(&[213, 0, 2, 0, 1, 0, 0, 0]);
    data.extend_from_slice(&6368i16.to_le_bytes());
    data.extend_from_slice(&(-1985i16).to_le_bytes());
    data.extend_from_slice(&[217, 0, 0, ALLIES, 1, 0, 0, 0]);
    data.extend_from_slice(&1664i16.to_le_bytes());
    data.extend_from_slice(&(-15264i16).to_le_bytes());
    send(&mut state, "InitObj", &data);

    let flag = state.objectives.flag(2).unwrap();
    assert_eq!(flag.entity_index, Some(213));
    assert_eq!(flag.origin, Some((6368, -1985)));
    assert_eq!(flag.owner, Team::Unassigned);
    assert_eq!(state.objectives.flag(0).unwrap().owner, Team::Allies);
}

/// Sets the demo clock, as an `SvcTime` would.
fn clock(state: &mut AnalyzerState, secs: f32) {
    state.current_time.viewdemo_offset = Duration::from_secs_f32(secs);
}

/// A death as `use_position_updates` records it. A `killer` of `None` is
/// the world.
fn kill(state: &mut AnalyzerState, killer: Option<&str>, victim: &str) {
    let victim = get(state, victim);
    let killer = killer.map(|name| get(state, name));
    let kill = KillPosition {
        time: state.current_time.clone(),
        weapon: dod::Weapon::K98,
        killer: killer.map(|p| p.id.clone()),
        victim: Some(victim.id.clone()),
        killer_team: killer.and_then(|p| p.team.clone()),
        victim_team: victim.team.clone(),
        teamkill: killer.is_some_and(|p| p.team == victim.team),
        killer_origin: None,
        victim_origin: None,
        distance: None,
        headshot: Default::default(),
        capture_area: Default::default(),
    };
    state.kill_positions.push(kill);
}

/// Axis starts a timed capture of area 1 at `secs`.
fn axis_attempt(state: &mut AnalyzerState, secs: f32) {
    clock(state, secs);
    send(state, "StartProg", &[1, AXIS, 5, 0]);
    next_frame(state);
}

fn cancel(state: &mut AnalyzerState, secs: f32) {
    clock(state, secs);
    send(state, "CancelProg", &[1, 0]);
    next_frame(state);
}

fn causes(state: &AnalyzerState) -> Vec<Option<CancelCause>> {
    state
        .objectives
        .attempts
        .iter()
        .map(|a| a.cancel_cause.clone())
        .collect()
}

fn breaks(state: &AnalyzerState) -> Vec<(&str, u32)> {
    state
        .players
        .iter()
        .map(|p| (p.name.as_str(), p.cap_breaks))
        .collect()
}

/// What the analyzer cache's v9 → v10 upgrade works out from the finished
/// state (#743) is what the live analysis counted as it went.
fn assert_recount_matches_live(state: &AnalyzerState) {
    let tally = recount_cancels(&state.objectives.attempts, &state.kill_positions).unwrap();
    assert_eq!(tally.causes, causes(state));
    for p in &state.players {
        let recounted = tally.breaks.get(&p.id).copied().unwrap_or(0);
        assert_eq!(recounted, p.cap_breaks, "{}", p.name);
    }
    assert_eq!(
        tally.blocks_unattributed,
        state.objectives.blocks_unattributed
    );
    assert_eq!(
        tally.attempts_abandoned,
        state.objectives.attempts_abandoned
    );
}

#[test]
fn a_death_in_the_frame_after_the_cancel_does_not_count_even_at_the_same_clock() {
    let mut state = two_teams();
    axis_attempt(&mut state, 10.0);
    // Resolved as the next frame starts, before this death arrives in it;
    // no SvcTime between them, so both read 15.0 s.
    cancel(&mut state, 15.0);
    kill(&mut state, Some("b1"), "a1");
    next_frame(&mut state);
    assert_eq!(causes(&state), [Some(CancelCause::Abandoned)]);
    assert_recount_matches_live(&state);
}

#[test]
fn a_cancelled_attempt_without_an_end_time_cannot_be_recounted() {
    let mut state = two_teams();
    axis_attempt(&mut state, 10.0);
    cancel(&mut state, 15.0);
    state.objectives.attempts[0].ended = None;
    assert!(recount_cancels(&state.objectives.attempts, &state.kill_positions).is_err());
}

#[test]
fn a_capper_killed_by_a_defender_just_before_the_cancel_is_a_break_for_that_defender() {
    let mut state = two_teams();
    axis_attempt(&mut state, 10.0);
    // The cancel trails the death by 0.5 to 1.1 s on real demos.
    clock(&mut state, 14.3);
    kill(&mut state, Some("b1"), "a1");
    cancel(&mut state, 15.0);

    assert_eq!(
        causes(&state),
        [Some(CancelCause::Break {
            breaker: id(&state, "b1")
        })]
    );
    assert_eq!(breaks(&state), [("a1", 0), ("a2", 0), ("a3", 0), ("b1", 1)]);
    assert_eq!(state.objectives.blocks_unattributed, 0);
    assert_eq!(state.objectives.attempts_abandoned, 0);
    assert_recount_matches_live(&state);
}

#[test]
fn a_death_in_the_cancels_own_packet_counts() {
    let mut state = two_teams();
    axis_attempt(&mut state, 10.0);
    clock(&mut state, 15.0);
    send(&mut state, "CancelProg", &[1, 0]);
    // The DeathMsg after the CancelProg in the same packet.
    kill(&mut state, Some("b1"), "a1");
    next_frame(&mut state);
    assert_eq!(breaks(&state)[3], ("b1", 1));
    assert_recount_matches_live(&state);
}

#[test]
fn a_defender_kill_wins_over_a_teamkill_in_the_same_window() {
    let mut state = two_teams();
    axis_attempt(&mut state, 10.0);
    clock(&mut state, 14.7);
    kill(&mut state, Some("b1"), "a3");
    clock(&mut state, 14.9);
    kill(&mut state, Some("a2"), "a1");
    cancel(&mut state, 15.0);
    assert_eq!(breaks(&state)[3], ("b1", 1));
    assert_recount_matches_live(&state);
}

#[test]
fn a_capper_killed_by_a_teammate_or_the_world_leaves_the_block_unattributed() {
    let mut state = two_teams();
    axis_attempt(&mut state, 10.0);
    clock(&mut state, 14.9);
    kill(&mut state, Some("a2"), "a1");
    cancel(&mut state, 15.0);

    axis_attempt(&mut state, 20.0);
    clock(&mut state, 24.9);
    kill(&mut state, None, "a3");
    cancel(&mut state, 25.0);

    assert_eq!(
        causes(&state),
        [
            Some(CancelCause::Unattributed),
            Some(CancelCause::Unattributed)
        ]
    );
    assert!(breaks(&state).iter().all(|(_, n)| *n == 0));
    assert_eq!(state.objectives.blocks_unattributed, 2);
    assert_eq!(state.objectives.attempts_abandoned, 0);
    // Still blocks for the defending team.
    assert_eq!(state.objectives.blocked_attempts(&Team::Axis), 2);
    assert_recount_matches_live(&state);
}

#[test]
fn a_cancel_with_every_attacker_alive_is_abandoned() {
    let mut state = two_teams();
    // Nobody died.
    axis_attempt(&mut state, 10.0);
    cancel(&mut state, 15.0);
    // An attacker died, but longer than BREAK_WINDOW before the cancel.
    axis_attempt(&mut state, 20.0);
    clock(&mut state, 23.5);
    kill(&mut state, Some("b1"), "a1");
    cancel(&mut state, 25.0);
    // Only a defender died.
    axis_attempt(&mut state, 30.0);
    clock(&mut state, 34.9);
    kill(&mut state, Some("a1"), "b1");
    cancel(&mut state, 35.0);
    // The attacker died before this attempt started.
    clock(&mut state, 39.9);
    kill(&mut state, Some("b1"), "a2");
    axis_attempt(&mut state, 40.0);
    cancel(&mut state, 40.2);

    assert_eq!(causes(&state), vec![Some(CancelCause::Abandoned); 4]);
    assert!(breaks(&state).iter().all(|(_, n)| *n == 0));
    assert_eq!(state.objectives.attempts_abandoned, 4);
    assert_eq!(state.objectives.blocks_unattributed, 0);
    assert_recount_matches_live(&state);

    state.objectives.clear_match_totals();
    assert_eq!(state.objectives.attempts_abandoned, 0);
}

#[test]
fn one_of_two_cappers_killed_is_no_cancel_and_no_break() {
    let mut state = two_teams();
    axis_attempt(&mut state, 10.0);
    clock(&mut state, 12.0);
    kill(&mut state, Some("b1"), "a1");
    next_frame(&mut state);
    // a2 stays on the point and the flag falls.
    clock(&mut state, 15.0);
    send(&mut state, "SetObj", &[1, AXIS, 0]);
    cap_msg(&mut state, 2, "Grassy Knoll", AXIS);
    next_frame(&mut state);

    assert_eq!(outcomes(&state), [AttemptOutcome::Captured]);
    assert_eq!(causes(&state), [None]);
    assert!(breaks(&state).iter().all(|(_, n)| *n == 0));
    assert_eq!(state.objectives.blocks_unattributed, 0);
    assert_eq!(state.objectives.attempts_abandoned, 0);
    assert_recount_matches_live(&state);
}

/// Cap breaks over a few real demos (`DOD_ANALYSIS_DEMOS`, a folder or one
/// file, else six spread over the PRE install's `dod` folder), printed for
/// a person to read. Ignored because CI has no demos:
/// `cargo test -p analysis --release -- --ignored cap_breaks_on_real_demos --nocapture`.
#[test]
#[ignore]
fn cap_breaks_on_real_demos() {
    let paths = crate::tests::comparison_demos();
    assert!(
        !paths.is_empty(),
        "no demos found: set DOD_ANALYSIS_DEMOS to a folder or a .dem file"
    );
    let mut totals = [0u32; 4];
    for path in &paths {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let analysis = crate::Analysis::try_from_bytes(&bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let objectives = &analysis.state.objectives;
        let cancelled: Vec<&CaptureAttempt> = objectives
            .attempts
            .iter()
            .filter(|a| a.outcome == AttemptOutcome::Cancelled)
            .collect();
        let breaks = cancelled
            .iter()
            .filter(|a| matches!(a.cancel_cause, Some(CancelCause::Break { .. })))
            .count() as u32;
        let credited: u32 = analysis.state.players.iter().map(|p| p.cap_breaks).sum();
        let file = path.display();
        // Every cancel has exactly one cause, and every break a player.
        assert!(cancelled.iter().all(|a| a.cancel_cause.is_some()), "{file}");
        assert_eq!(
            breaks + objectives.blocks_unattributed + objectives.attempts_abandoned,
            cancelled.len() as u32,
            "{file}"
        );
        assert_eq!(credited, breaks, "{file}");
        println!(
            "{file}: {} cancelled, {breaks} breaks, {} unattributed, {} abandoned",
            cancelled.len(),
            objectives.blocks_unattributed,
            objectives.attempts_abandoned
        );
        for (total, n) in totals.iter_mut().zip([
            cancelled.len() as u32,
            breaks,
            objectives.blocks_unattributed,
            objectives.attempts_abandoned,
        ]) {
            *total += n;
        }
    }
    println!(
        "{} demos: {} cancelled, {} breaks, {} unattributed, {} abandoned",
        paths.len(),
        totals[0],
        totals[1],
        totals[2],
        totals[3]
    );
}
