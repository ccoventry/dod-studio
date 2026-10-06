//! R&D: find the highlights in a demo without watching it.
//!
//! For every life with a kill (the same streaks the Highlights table lists)
//! it tags each kill with what made it unusual -- a noscope, a flick, a wall
//! shot, a jump shot, a double kill -- and gives the life a score, so the
//! best ones can be looked at first. Tags come from `analysis::KillMoment`
//! (the recording player's own view, zoom, keys and movement in a POV demo;
//! both players' positions in any demo) and from line-of-sight traces through
//! the map's BSP (`native::patch::bsp_trace`).
//!
//! Writes one JSON line per demo.
//!
//!     cargo run --release -p native --example highlight_probe -- <out.jsonl> <demo or @list.txt>...
//!
//! `@list.txt` is a file with one demo path per line.

use analysis::{Analysis, KillMoment, PlayerGlobalId, Team, Weapon};
use native::patch::bsp::Bsp;
use native::patch::bsp_entities::parse_entities;
use native::patch::bsp_trace::{Sight, SightMap};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

const STEAM: &str = r"C:\Program Files (x86)\Steam\steamapps\common";
const INSTALLS: [&str; 3] = [
    "Half-Life - PRE-Anniversary for Movies",
    "Half-Life - POST-Anniversary for Movies",
    "Half-Life",
];
/// One world unit is an inch.
const METRES_PER_UNIT: f32 = 0.0254;
/// Standing eye height above a player's origin, for players whose real eye
/// is unknown.
const EYE: f32 = 28.0;
/// A flick: how far the view swept on screen in the 0.25 s before the shot,
/// and how fast at its fastest.
const FLICK_TURN: f32 = 20.0;
const FLICK_PEAK: f32 = 250.0;
/// Moving sideways faster than this at the shot.
const STRAFE_SPEED: f32 = 90.0;
/// Points on a victim checked for sight: head, chest, hips.
const BODY: [f32; 3] = [24.0, 6.0, -14.0];

fn find_bsp(map: &str) -> Option<PathBuf> {
    for install in INSTALLS {
        for dir in ["dod", "dod_downloads", "dod_addon"] {
            let p = Path::new(STEAM)
                .join(install)
                .join(dir)
                .join("maps")
                .join(format!("{map}.bsp"));
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn load_map(map: &str) -> Option<SightMap> {
    let path = find_bsp(map)?;
    let bytes = std::fs::read(&path).ok()?;
    let bsp = Bsp::parse(&bytes).ok()?;
    let entities = parse_entities(&bytes).unwrap_or_default();
    Some(SightMap::new(bsp, &entities))
}

fn is_sniper(w: &Weapon) -> bool {
    matches!(
        w,
        Weapon::Springfield | Weapon::ScopedK98 | Weapon::ScopedFg42 | Weapon::ScopedLeeEnfield
    )
}
fn is_pistol(w: &Weapon) -> bool {
    matches!(w, Weapon::M1911 | Weapon::Luger | Weapon::Webley)
}
fn is_bolt(w: &Weapon) -> bool {
    matches!(w, Weapon::K98 | Weapon::LeeEnfield) || is_sniper(w)
}
fn is_auto(w: &Weapon) -> bool {
    matches!(
        w,
        Weapon::Thompson
            | Weapon::Stg44
            | Weapon::Bar
            | Weapon::Mp40
            | Weapon::Mg42
            | Weapon::Browning30Cal
            | Weapon::Mg34
            | Weapon::GreaseGun
            | Weapon::Fg42
            | Weapon::Sten
            | Weapon::Bren
    )
}
fn is_explosive(w: &Weapon) -> bool {
    w.is_grenade()
        || matches!(
            w,
            Weapon::Bazooka | Weapon::Panzerschreck | Weapon::Piat | Weapon::Mortar
        )
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn view_angles_to(v: [f32; 3]) -> [f32; 2] {
    let yaw = v[1].atan2(v[0]).to_degrees();
    let pitch = -(v[2].atan2((v[0] * v[0] + v[1] * v[1]).sqrt())).to_degrees();
    [pitch, yaw]
}
fn angle_between(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dir = |p: [f32; 2]| {
        let (pitch, yaw) = (p[0].to_radians(), p[1].to_radians());
        [
            pitch.cos() * yaw.cos(),
            pitch.cos() * yaw.sin(),
            -pitch.sin(),
        ]
    };
    let (u, v) = (dir(a), dir(b));
    (u[0] * v[0] + u[1] * v[1] + u[2] * v[2])
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

/// Best sight from an eye to any of a victim's body points.
fn sight_to(map: &SightMap, eye: [f32; 3], victim: [f32; 3]) -> Sight {
    let mut best = Sight::World;
    for dz in BODY {
        match map.sight(eye, [victim[0], victim[1], victim[2] + dz]) {
            Sight::Clear => return Sight::Clear,
            Sight::Object => best = Sight::Object,
            Sight::World => {}
        }
    }
    best
}

struct Tagged {
    tags: Vec<(String, String)>,
    detail: Value,
}

fn tag_kill(k: &KillMoment, map: Option<&SightMap>) -> Tagged {
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut add = |t: &str, why: String| tags.push((t.to_string(), why));
    let w = &k.weapon;
    let bullet = !is_explosive(w) && !analysis::kill_moment::is_melee(w);

    let eye_of = |s: &analysis::PoseSample| {
        s.killer.map(|p| {
            if s.killer_eye_is_real {
                p
            } else {
                [p[0], p[1], p[2] + EYE]
            }
        })
    };
    let at_shot = k.samples.first();
    let distance = at_shot.and_then(|s| Some(len(sub(s.victim?, s.killer?))));
    let mut sights: Vec<Option<Sight>> = Vec::new();
    if let Some(map) = map {
        for s in &k.samples {
            sights.push(match (eye_of(s), s.victim) {
                (Some(eye), Some(v)) if !map.in_solid(eye) => Some(sight_to(map, eye, v)),
                _ => None,
            });
        }
    }
    let shot_sight = sights.first().copied().flatten();
    // From the eye at the shot to where the victim was at each instant: the
    // shooter's screen shows other players about 0.1 s in the past.
    let victim_seen_from_shot: String = match (map, at_shot.and_then(eye_of)) {
        (Some(map), Some(eye)) if !map.in_solid(eye) => k
            .samples
            .iter()
            .map(|s| match s.victim.map(|v| sight_to(map, eye, v)) {
                Some(Sight::Clear) => 'C',
                Some(Sight::Object) => 'O',
                Some(Sight::World) => 'W',
                None => '-',
            })
            .collect(),
        _ => String::new(),
    };
    // How long the victim had been in sight at the shot.
    let exposed_for = if shot_sight == Some(Sight::Clear) {
        let mut exposed = 1.0f32;
        for (s, sight) in k.samples.iter().zip(&sights).skip(1) {
            if *sight != Some(Sight::Clear) {
                exposed = s.before;
                break;
            }
        }
        Some(exposed)
    } else {
        None
    };

    if analysis::kill_moment::is_melee(w) {
        add("melee", format!("{w:?}"));
    }
    if is_explosive(w) {
        add("explosive", format!("{w:?}"));
    }
    if bullet {
        match shot_sight {
            Some(Sight::World) => add(
                "wall",
                "no line of sight through the map at the shot".into(),
            ),
            Some(Sight::Object) => add(
                "through-object",
                "a door, table or other brush between them".into(),
            ),
            _ => {}
        }
        if let Some(exposed) = exposed_for
            && exposed <= 0.1
        {
            add(
                "prefire",
                format!("in sight for under {:.0} ms", exposed * 1000.0 + 100.0),
            );
        }
    }
    if let Some(d) = distance {
        let m = d * METRES_PER_UNIT;
        let long = if is_pistol(w) {
            m >= 25.0
        } else if is_sniper(w) || is_explosive(w) || analysis::kill_moment::is_melee(w) {
            false
        } else if is_bolt(w) {
            m >= 50.0
        } else {
            m >= 38.0
        };
        if long {
            add("long-range", format!("{m:.0} m"));
        }
    }

    let mut aim_error = None;
    if let Some(v) = &k.view {
        if bullet {
            if is_sniper(w) && !v.scoped {
                add("noscope", "sniper rifle, not zoomed".into());
            }
            if is_sniper(w) && v.scoped && v.scoped_for < 0.35 {
                add(
                    "quickscope",
                    format!("zoomed {:.0} ms before the shot", v.scoped_for * 1000.0),
                );
            }
            // Zoomed in, a small turn sweeps the screen as far as a big one.
            let zoom = if v.fov > 1.0 && v.fov < 89.0 {
                90.0 / v.fov
            } else {
                1.0
            };
            let (screen_turn, screen_peak) = (v.turn_250ms * zoom, v.peak_turn_speed * zoom);
            if screen_turn >= FLICK_TURN && screen_peak >= FLICK_PEAK {
                add(
                    "flick",
                    format!(
                        "{screen_turn:.0} deg on screen in 0.25 s, peak {screen_peak:.0} deg/s"
                    ),
                );
            }
            if v.turn_1200ms >= 120.0 {
                add("turnaround", format!("{:.0} deg in 1.2 s", v.turn_1200ms));
            }
            if let (Some(s), Some(eye)) = (at_shot, at_shot.and_then(eye_of))
                && let Some(victim) = s.victim
            {
                let to = view_angles_to(sub([victim[0], victim[1], victim[2] + BODY[1]], eye));
                let err = angle_between(v.angles, to);
                aim_error = Some(err);
                // Off target by more than the victim's own width at that range.
                let width =
                    distance.map_or(10.0, |d| (16.0f32).atan2(d.max(1.0)).to_degrees() * 3.0);
                if err > width.max(6.0) && distance.is_some_and(|d| d > 150.0) {
                    add(
                        "off-target",
                        format!("crosshair {err:.0} deg off the victim"),
                    );
                }
            }
            if is_auto(w) && v.shots_before >= 10 {
                add(
                    "spray",
                    format!("{} shots in the 3 s before", v.shots_before),
                );
            }
        }
        if !v.on_ground || v.airborne_recently {
            if v.duck_held {
                add("crouch-hop", "in the air, crouching".into());
            } else {
                add("airborne", "off the ground at the shot".into());
            }
        }
        if v.on_ground && v.horizontal_speed < 60.0 && v.max_speed_600ms >= 160.0 && !is_auto(w) {
            add(
                "peek",
                format!("moving {:.0} u/s, stopped to shoot", v.max_speed_600ms),
            );
        }
        if v.on_ground && v.side_move.abs() > 1.0 && v.horizontal_speed > STRAFE_SPEED {
            add(
                "strafing",
                format!("{:.0} u/s sideways keys held", v.horizontal_speed),
            );
        }
        if v.sprint_held && v.horizontal_speed > 150.0 {
            add("sprinting", format!("{:.0} u/s", v.horizontal_speed));
        } else if is_bolt(w) && v.horizontal_speed > 120.0 && v.on_ground {
            add(
                "moving",
                format!("{:.0} u/s with a bolt rifle", v.horizontal_speed),
            );
        }
        if v.prone {
            add("prone", String::new());
        }
        if v.health > 0 && v.health <= 25 {
            add("low-health", format!("{} hp", v.health));
        }
    }

    Tagged {
        tags,
        detail: json!({
            "distance_m": distance.map(|d| (d * METRES_PER_UNIT * 10.0).round() / 10.0),
            "sight": shot_sight.map(|s| format!("{s:?}")),
            "sights": sights.iter().map(|s| match s { Some(Sight::Clear) => 'C', Some(Sight::Object) => 'O', Some(Sight::World) => 'W', None => '-' }).collect::<String>(),
            "seen_from_shot": victim_seen_from_shot,
            "path_blocked": match map {
                Some(map) => k.samples.windows(2).map(|w| {
                    let e = |s: &analysis::PoseSample| eye_of(s);
                    let a = match (e(&w[0]), e(&w[1])) { (Some(a), Some(b)) => Some(map.sight(a, b) != Sight::Clear), _ => None };
                    let b = match (w[0].victim, w[1].victim) { (Some(a), Some(b)) => Some(map.sight(a, b) != Sight::Clear), _ => None };
                    (a, b)
                }).collect::<Vec<_>>(),
                None => Vec::new(),
            },
            "hit_texture": match (map, at_shot.and_then(eye_of), at_shot.and_then(|s| s.victim)) {
                (Some(map), Some(eye), Some(v)) => map.world_hit_texture(eye, [v[0], v[1], v[2] + BODY[1]]),
                _ => None,
            },
            "hits": match (map, at_shot.and_then(eye_of), at_shot.and_then(|s| s.victim)) {
                (Some(map), Some(eye), Some(v)) => [36.0f32, 24.0, 12.0, 0.0, -12.0, -24.0].iter().map(|dz| {
                    let target = [v[0], v[1], v[2] + dz];
                    map.first_world_hit(eye, target).map(|f| ((f * len(sub(target, eye))) as i32, ((1.0 - f) * len(sub(target, eye))) as i32))
                }).collect::<Vec<_>>(),
                _ => Vec::new(),
            },
            "exposed_s": exposed_for,
            "aim_error": aim_error.map(|e| (e * 10.0).round() / 10.0),
            "view": k.view.as_ref().map(|v| json!({
                "turn250": (v.turn_250ms*10.0).round()/10.0, "turn600": (v.turn_600ms*10.0).round()/10.0,
                "turn1200": v.turn_1200ms.round(), "fov": v.fov, "side": v.side_move, "fwd": v.forward_move, "from": v.shot_from,
                "peak": v.peak_turn_speed.round(), "settled": (v.settled_for*1000.0).round(),
                "scoped": v.scoped, "scoped_for": (v.scoped_for*1000.0).round(),
                "ground": v.on_ground, "speed": v.horizontal_speed.round(),
                "buttons": [v.jump_held, v.duck_held, v.sprint_held], "prone": v.prone,
                "hp": v.health, "shots_before": v.shots_before, "lead": (v.shot_lead*1000.0).round(),
                "shot_seen": v.shot_seen,
            })),
        }),
    }
}

fn mmss(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Points a tag is worth toward a life's score.
fn tag_points(tag: &str) -> f32 {
    match tag {
        "noscope" => 22.0,
        "wall" => 22.0,
        "through-object" => 12.0,
        "flick" => 18.0,
        "turnaround" => 14.0,
        "multi" => 25.0,
        "crouch-hop" => 16.0,
        "airborne" => 12.0,
        "melee" => 14.0,
        "long-range" => 8.0,
        "quickscope" => 8.0,
        "prefire" => 6.0,
        "off-target" => 8.0,
        "sprinting" => 6.0,
        "strafing" => 6.0,
        "peek" => 4.0,
        "quick-double" => 10.0,
        "cap-block" => 18.0,
        "final-seconds" => 14.0,
        "moving" => 4.0,
        "low-health" => 6.0,
        "cap" => 12.0,
        "capout" => 20.0,
        _ => 0.0,
    }
}

fn analyse(path: &Path, maps: &mut HashMap<String, Option<SightMap>>) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let analysis = Analysis::try_from_bytes(&bytes)?;
    let st = &analysis.state;
    let map_name = st.initial_map_name.clone().unwrap_or_default();
    let map_key = map_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&map_name)
        .trim_end_matches(".bsp")
        .to_string();
    let map = maps
        .entry(map_key.clone())
        .or_insert_with(|| load_map(&map_key))
        .as_ref();
    // The last kill or capture: after it the recording is the scoreboard.
    let play_end = st
        .kill_moments
        .iter()
        .map(|k| k.time.real_offset.as_secs_f64())
        .chain(
            st.objectives
                .captures
                .iter()
                .map(|c| c.time.real_offset.as_secs_f64()),
        )
        .fold(0.0f64, f64::max)
        .max(0.0);
    let play_end = play_end.max(f64::from(analysis.demo_info.playback_time).min(play_end + 30.0));
    let pov = st.pov_player_index.is_some() && analysis.demo_info.demo_type != "HLTV";
    let name_of = |id: &Option<PlayerGlobalId>| {
        id.as_ref()
            .and_then(|id| st.players.iter().find(|p| &p.id == id))
            .map(|p| p.name.clone())
    };
    let team_of = |id: &Option<PlayerGlobalId>| -> Option<Team> {
        id.as_ref()
            .and_then(|id| st.players.iter().find(|p| &p.id == id))
            .and_then(|p| p.team.clone())
    };

    // Kills of interest, by killer, in time order.
    let mut by_killer: HashMap<String, Vec<&KillMoment>> = HashMap::new();
    for k in &st.kill_moments {
        if k.suicide || k.teamkill {
            continue;
        }
        if pov && !k.by_recorder {
            continue;
        }
        if let Some(id) = &k.killer {
            by_killer.entry(id.to_string()).or_default().push(k);
        }
    }

    let mut lives = Vec::new();
    for (killer_id, kills) in &by_killer {
        // Split into lives at each of the killer's deaths.
        let deaths: Vec<f64> = st
            .kill_moments
            .iter()
            .filter(|k| {
                k.victim
                    .as_ref()
                    .is_some_and(|v| &v.to_string() == killer_id)
            })
            .map(|k| k.time.real_offset.as_secs_f64())
            .collect();
        let mut groups: Vec<Vec<&KillMoment>> = Vec::new();
        let mut last_life = None;
        for k in kills {
            let t = k.time.real_offset.as_secs_f64();
            let life = deaths.iter().filter(|d| **d < t - 0.05).count();
            if last_life != Some(life) {
                groups.push(Vec::new());
                last_life = Some(life);
            }
            groups.last_mut().unwrap().push(k);
        }
        for group in groups {
            let mut kill_rows = Vec::new();
            let mut tag_counts: HashMap<String, u32> = HashMap::new();
            let mut tag_score = 0.0f32;
            // Kills less than a moment apart by one shot or blast.
            for (i, k) in group.iter().enumerate() {
                let tagged = tag_kill(k, map);
                let mut tags = tagged.tags;
                let t = k.time.real_offset.as_secs_f64();
                let window = if is_explosive(&k.weapon) { 0.4 } else { 0.08 };
                let multi = group.iter().enumerate().any(|(j, o)| {
                    j != i
                        && o.weapon == k.weapon
                        && (o.time.real_offset.as_secs_f64() - t).abs() <= window
                });
                // A kill during the other team's capture, which then fell
                // apart within a second and a half: a cap block.
                let blocked = st.objectives.attempts.iter().any(|a| {
                    Some(&a.team) != team_of(&k.killer).as_ref()
                        && a.outcome == analysis::AttemptOutcome::Cancelled
                        && a.started.real_offset.as_secs_f64() <= t + 0.05
                        && a.ended.as_ref().is_some_and(|e| {
                            let e = e.real_offset.as_secs_f64();
                            e >= t - 0.05 && e - t <= 1.5
                        })
                });
                if blocked {
                    tags.push((
                        "cap-block".into(),
                        "their capture fell apart right after this kill".into(),
                    ));
                }
                // No match clock in a demo unless someone typed `timeleft`;
                // the end of the recording stands in for the end of the half.
                let left = k
                    .match_time_left
                    .map(f64::from)
                    .unwrap_or(play_end - k.time.real_offset.as_secs_f64());
                if left <= 45.0 {
                    tags.push((
                        "final-seconds".into(),
                        format!("{left:.0} s before the end"),
                    ));
                }
                if multi {
                    tags.push((
                        "multi".into(),
                        "two or more kills from one shot or blast".into(),
                    ));
                } else if group
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != i && (o.time.real_offset.as_secs_f64() - t).abs() <= 1.5)
                {
                    tags.push(("quick-double".into(), "another kill within 1.5 s".into()));
                }
                let best = tags
                    .iter()
                    .map(|(t, _)| tag_points(t))
                    .fold(0.0f32, f32::max);
                let sum: f32 = tags.iter().map(|(t, _)| tag_points(t)).sum();
                tag_score += best + 0.35 * (sum - best);
                for (t, _) in &tags {
                    *tag_counts.entry(t.clone()).or_default() += 1;
                }
                kill_rows.push(json!({
                    "time": mmss(k.time.viewdemo_offset.as_secs_f64()),
                    "t": (k.time.viewdemo_offset.as_secs_f64() * 100.0).round() / 100.0,
                    "weapon": format!("{:?}", k.weapon),
                    "left": k.match_time_left.map(|x| x.round()),
                    "victim": name_of(&k.victim),
                    "tags": tags.iter().map(|(t, why)| json!([t, why])).collect::<Vec<_>>(),
                    "detail": tagged.detail,
                }));
            }
            let times: Vec<f64> = group
                .iter()
                .map(|k| k.time.viewdemo_offset.as_secs_f64())
                .collect();
            let n = group.len();
            let span = times.last().unwrap() - times.first().unwrap();
            let gaps: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).collect();
            let max_gap = gaps.iter().copied().fold(0.0f64, f64::max);
            // Tightest run: most kills inside any 10 s window.
            let mut best_run = (1usize, 0usize);
            for a in 0..n {
                let mut b = a;
                while b + 1 < n && times[b + 1] - times[a] <= 10.0 {
                    b += 1;
                }
                if b - a + 1 > best_run.0 {
                    best_run = (b - a + 1, a);
                }
            }
            // Caps by the killer's team in this life or 15 s after it.
            let team = team_of(&group[0].killer);
            let killer_pid = group[0].killer.clone();
            let real_times: Vec<f64> = group
                .iter()
                .map(|k| k.time.real_offset.as_secs_f64())
                .collect();
            let (r0, r1) = (real_times[0], *real_times.last().unwrap());
            let mut cap_notes = Vec::new();
            let mut cap_points = 0.0f32;
            for c in &st.objectives.captures {
                let ct = c.time.real_offset.as_secs_f64();
                if Some(&c.team) != team.as_ref() || ct < r0 - 2.0 || ct > r1 + 15.0 {
                    continue;
                }
                let own = killer_pid
                    .as_ref()
                    .is_some_and(|id| c.cappers().any(|p| p == id));
                let round_end = st.rounds.iter().any(|r| match r {
                    analysis::Round::Completed { end_time, .. } => {
                        let e = end_time.real_offset.as_secs_f64();
                        e >= ct - 0.5 && e <= ct + 4.0
                    }
                    _ => false,
                });
                let what = match (own, round_end) {
                    (true, true) => "capout",
                    (false, true) => "team-capout",
                    (true, false) => "cap",
                    (false, false) => "team-cap",
                };
                cap_points = cap_points.max(match what {
                    "capout" => tag_points("capout"),
                    "cap" => tag_points("cap"),
                    "team-capout" => 8.0,
                    _ => 3.0,
                });
                cap_notes.push(json!([
                    what,
                    c.flag_name,
                    mmss(c.time.viewdemo_offset.as_secs_f64())
                ]));
            }
            let kill_points = match n {
                1 => 4.0,
                2 => 12.0,
                3 => 24.0,
                4 => 36.0,
                5 => 46.0,
                _ => 52.0 + (n as f32 - 6.0) * 3.0,
            };
            // Pace: kills spread over a long life are worth less.
            let pace = if n >= 2 { span / (n as f64 - 1.0) } else { 0.0 };
            let pace_factor = if n < 2 {
                1.0
            } else if pace <= 2.5 {
                1.2
            } else if pace <= 6.0 {
                1.0
            } else if pace <= 12.0 {
                0.8
            } else {
                0.65
            };
            let score = kill_points * pace_factor as f32 + tag_score + cap_points;
            lives.push(json!({
                "player": name_of(&group[0].killer),
                "start": mmss(times[0]),
                "end": mmss(*times.last().unwrap()),
                "t0": times[0],
                "kills": n,
                "span_s": (span * 10.0).round() / 10.0,
                "max_gap_s": (max_gap * 10.0).round() / 10.0,
                "tight_run": [best_run.0, mmss(times[best_run.1]), mmss(times[best_run.1 + best_run.0 - 1])],
                "tags": tag_counts,
                "caps": cap_notes,
                "score": (score * 10.0).round() / 10.0,
                "kill_list": kill_rows,
            }));
        }
    }
    lives.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .partial_cmp(&a["score"].as_f64())
            .unwrap()
    });
    // Caps worth a look on their own: the player whose highlights these are
    // capped with nobody else and without a kill in the 15 s before.
    let mut cap_moments = Vec::new();
    for c in &st.objectives.captures {
        let Some(capper) = &c.capper else { continue };
        if pov {
            let recorder = st.kill_moments.iter().find_map(|k| {
                if k.by_recorder {
                    k.killer.clone()
                } else if k.victim_is_recorder {
                    k.victim.clone()
                } else {
                    None
                }
            });
            if recorder.as_ref() != Some(capper) {
                continue;
            }
        }
        let ct = c.time.real_offset.as_secs_f64();
        let fought = st.kill_moments.iter().any(|k| {
            k.killer.as_ref() == Some(capper) && {
                let t = k.time.real_offset.as_secs_f64();
                t <= ct && ct - t <= 15.0
            }
        });
        let round_end = st.rounds.iter().any(|r| match r {
            analysis::Round::Completed { end_time, .. } => {
                let e = end_time.real_offset.as_secs_f64();
                e >= ct - 0.5 && e <= ct + 4.0
            }
            _ => false,
        });
        let solo = c.co_cappers.is_empty();
        if solo && !fought {
            cap_moments.push(json!({
                "time": mmss(c.time.viewdemo_offset.as_secs_f64()),
                "t": c.time.viewdemo_offset.as_secs_f64(),
                "player": name_of(&Some(capper.clone())),
                "flag": c.flag_name,
                "what": if round_end { "ninja capout" } else { "ninja cap" },
            }));
        }
    }
    // Bloopers: the recorder's teamkills, suicides and deaths to a teammate.
    let mut bloopers = Vec::new();
    for k in &st.kill_moments {
        let involves = if pov {
            k.by_recorder || k.victim_is_recorder
        } else {
            true
        };
        if !involves {
            continue;
        }
        let what = if k.suicide {
            Some(if is_explosive(&k.weapon) {
                "blew themself up"
            } else {
                "suicide"
            })
        } else if k.teamkill {
            Some("teamkill")
        } else if k.victim_is_recorder && analysis::kill_moment::is_melee(&k.weapon) {
            Some("got knifed")
        } else {
            None
        };
        if let Some(what) = what {
            bloopers.push(json!({
                "time": mmss(k.time.viewdemo_offset.as_secs_f64()),
                "what": what,
                "killer": name_of(&k.killer),
                "victim": name_of(&k.victim),
                "weapon": format!("{:?}", k.weapon),
            }));
        }
    }
    Ok(json!({
        "demo": path.file_name().map(|f| f.to_string_lossy().into_owned()),
        "path": path.to_string_lossy(),
        "map": map_key,
        "map_loaded": map.is_some(),
        "pov": pov,
        "recorder": st.kill_moments.iter().find_map(|k| {
            if k.by_recorder { name_of(&k.killer) } else if k.victim_is_recorder { name_of(&k.victim) } else { None }
        }),
        "lives": lives,
        "cap_moments": cap_moments,
        "bloopers": bloopers,
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: highlight_probe <out.jsonl> <demo or @list.txt>...";
    let out = args.first().expect(usage);
    let mut demos = Vec::new();
    for a in &args[1..] {
        if let Some(list) = a.strip_prefix('@') {
            let text = std::fs::read_to_string(list).expect("read list");
            demos.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(PathBuf::from),
            );
        } else {
            demos.push(PathBuf::from(a));
        }
    }
    let mut file = std::fs::File::create(out).expect("create output");
    let mut maps = HashMap::new();
    for (i, demo) in demos.iter().enumerate() {
        let started = std::time::Instant::now();
        match analyse(demo, &mut maps) {
            Ok(v) => {
                writeln!(file, "{v}").expect("write");
                eprintln!(
                    "[{}/{}] {} -- {} lives, map {}, {:.1}s",
                    i + 1,
                    demos.len(),
                    demo.display(),
                    v["lives"].as_array().map_or(0, Vec::len),
                    if v["map_loaded"].as_bool() == Some(true) {
                        "loaded"
                    } else {
                        "MISSING"
                    },
                    started.elapsed().as_secs_f32()
                );
            }
            Err(e) => eprintln!(
                "[{}/{}] {} -- ERROR {e}",
                i + 1,
                demos.len(),
                demo.display()
            ),
        }
    }
}
