//! Can the match and the half of a demo be told from its contents (#685)?
//!
//! Groups a folder's demos by match and half using only what the analysis
//! sees, then scores the grouping against the file names, which carry the
//! answer (`<match>_h1_<recorder>.dem`; KTP season 8's
//! `ktps8w<week>-<recorder>_<opponent>[_m<n>]_<map>_h1.dem`).
//!
//! - Same half: same map, most players shared and on the same sides, and
//!   runs of kills in common. Two recordings of one half share their kill
//!   feed; the same two teams meeting again on that map share players and
//!   sides but no kills.
//! - Other half: same map and players with the sides swapped, and the same
//!   names (players restyle their names between events, rarely mid-match).
//! - Which half is first: the server's own "2nd HALF STARTING" (KTP), then
//!   end-of-half chat (players say "gh" when a first half ends and "gg" when
//!   a match does), then file dates, used only when a recorder's two files
//!   are as far apart as a half takes (copies and edits move dates).
//!
//! The server name is not used: HLTV reports its own, and one match's halves
//! can be on two servers.
//!
//!     cargo run --release -p analysis --example match_half_probe -- [folder] [filter]
//!         [--features-cache <dir>] [--threads N] [--dump] [--tags-only]
//!         [--roster-only] [--alias <name key>=<name key>]
//!
//! `folder` defaults to `DOD_ANALYSIS_DEMOS`, then the PRE install's `dod\`;
//! `filter` (a substring of the file name) to `wsod25_grp`. Analyses come
//! from Studio's analyzer cache (`%APPDATA%\dod-studio\analyzer_cache`) when
//! it has a fresh entry, otherwise from a parse. `--features-cache` keeps the
//! few kilobytes this probe needs per demo, so a rerun skips both. `--dump`
//! prints each demo's roster, scores and server text, for finding signals.
//! `--tags-only` relates demos by clan tags instead of SteamIDs;
//! `--roster-only` drops the kill and name checks, to show what they add.
//! `--alias`
//! tells the scoring that two name schemes are the same matches, e.g.
//! `--alias monday-wsod25_r07=wsod25_ply2` (both are WSOD25 playoff round 2).

use analysis::Analysis;
use dod::Team;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::SystemTime;

const PRE_DOD: &str =
    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod";

/// The side a player ended the demo on: the Allied side (US or British) or Axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Side {
    Allied,
    Axis,
}

/// What the grouping needs from one demo.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Features {
    name: String,
    size: u64,
    modified_unix: u64,
    demo_type: String,
    map: String,
    signon_maps: Vec<String>,
    server_name: Option<String>,
    server_address: Option<String>,
    hltv_name: Option<String>,
    clan_match: bool,
    playback_secs: f32,
    /// SteamID (or other global id) → (name, side at the end).
    roster: BTreeMap<String, (String, Side)>,
    allied_tag: Option<String>,
    axis_tag: Option<String>,
    /// (seconds into the demo, side, score) for every TeamScore.
    scores: Vec<(f32, Side, i32)>,
    /// Server and player text, "secs  kind  sender: text".
    text: Vec<String>,
    rounds: usize,
    /// The recording player's id (POV), or `hltv:<proxy name>` (HLTV).
    recorder: Option<String>,
    /// Public and team chat: (seconds, sender id or name, text).
    say: Vec<(f32, String, String)>,
    /// When play went live: the first round start or "match is live" after
    /// the last clan-match countdown, else the first round start, else 0.
    /// Chat before it is warm-up talk, often about the previous map.
    live_secs: f32,
    /// The half the server itself announced ("2nd HALF STARTING"), if it did.
    announced_half: Option<u8>,
    /// Every kill in order: (killer name, victim name, weapon).
    kills: Vec<(String, String, String)>,
    /// The round clock's first and last "time left" (seconds), so two
    /// recordings can be checked for covering the same stretch of a half.
    clock: Option<(f32, f32)>,
}

fn side_of(team: &Option<Team>) -> Option<Side> {
    match team {
        Some(Team::Allies | Team::British) => Some(Side::Allied),
        Some(Team::Axis) => Some(Side::Axis),
        _ => None,
    }
}

fn features_of(name: &str, size: u64, modified_unix: u64, a: &Analysis) -> Features {
    let s = &a.state;
    let mut roster = BTreeMap::new();
    for p in &s.players {
        let id = p.id.to_string();
        // A per-demo connection number identifies nobody across demos.
        if id.starts_with("CONNECTION_") {
            continue;
        }
        if let Some(side) = side_of(&p.team) {
            roster.insert(id, (p.name.clone(), side));
        }
    }
    let mut allied_tag = None;
    let mut axis_tag = None;
    for t in analysis::team_tags(s) {
        if t.side == "Axis" {
            axis_tag = t.tag;
        } else {
            allied_tag = t.tag;
        }
    }
    let scores = s
        .team_scores
        .iter()
        .filter_map(|(t, team, pts)| {
            side_of(&Some(team.clone())).map(|side| (t.real_offset.as_secs_f32(), side, *pts))
        })
        .collect();
    let text = s
        .chat_messages
        .iter()
        .map(|m| {
            format!(
                "{:7.1}  {:?}  {}: {}{}",
                m.time.real_offset.as_secs_f32(),
                m.chat_type,
                m.sender_name.as_deref().unwrap_or("-"),
                m.text,
                m.system_token
                    .as_deref()
                    .map(|t| format!(" [{t} {:?}]", m.system_args))
                    .unwrap_or_default()
            )
        })
        .collect();
    let recorder = if a.demo_info.demo_type == "HLTV" {
        Some(format!(
            "hltv:{}",
            s.hltv_name
                .as_deref()
                .or(s.server_name.as_deref())
                .unwrap_or("?")
        ))
    } else {
        analysis::cache::players_in(a)
            .players
            .into_iter()
            .find(|p| p.recorder)
            .map(|p| p.id)
    };
    let say = s
        .chat_messages
        .iter()
        .filter(|m| m.chat_type != analysis::ChatType::System)
        .map(|m| {
            (
                m.time.real_offset.as_secs_f32(),
                m.sender_name.clone().unwrap_or_default(),
                m.text.clone(),
            )
        })
        .collect();
    let is_server = |m: &analysis::ChatMessage| {
        m.chat_type == analysis::ChatType::System
            || m.sender_name
                .as_deref()
                .is_none_or(|n| n == "Console/Server")
    };
    let goes_live = |m: &analysis::ChatMessage| {
        is_server(m)
            && (m
                .system_token
                .as_deref()
                .is_some_and(|t| t.starts_with("#game_roundstart"))
                || m.text.to_lowercase().contains("match is live"))
    };
    let countdown = s
        .chat_messages
        .iter()
        .filter(|m| m.system_token.as_deref() == Some("#Clan_time_remaining"))
        .map(|m| m.time.real_offset.as_secs_f32())
        .fold(0.0f32, f32::max);
    let live_secs = s
        .chat_messages
        .iter()
        .filter(|m| goes_live(m) && m.time.real_offset.as_secs_f32() >= countdown)
        .map(|m| m.time.real_offset.as_secs_f32())
        .next()
        .or_else(|| {
            s.chat_messages
                .iter()
                .find(|m| goes_live(m))
                .map(|m| m.time.real_offset.as_secs_f32())
        })
        .unwrap_or(0.0);
    let announced_half = s
        .chat_messages
        .iter()
        .filter(|m| is_server(m))
        .find_map(|m| announced(&m.text));
    // By name, not id, so demos without SteamIDs compare too.
    let name_of = |id: &Option<analysis::PlayerGlobalId>| {
        id.as_ref()
            .and_then(|id| s.players.iter().find(|p| &p.id == id))
            .map(|p| p.name.clone())
            .unwrap_or_default()
    };
    let kills = s
        .kill_positions
        .iter()
        .map(|k| {
            (
                name_of(&k.killer),
                name_of(&k.victim),
                format!("{:?}", k.weapon),
            )
        })
        .collect();
    Features {
        name: name.to_string(),
        size,
        modified_unix,
        demo_type: a.demo_info.demo_type.clone(),
        map: a.demo_info.map_name.trim_end_matches('\0').to_lowercase(),
        signon_maps: s.signon_maps.clone(),
        server_name: s.server_name.clone(),
        server_address: s.server_address.clone(),
        hltv_name: s.hltv_name.clone(),
        clan_match: s.is_clan_match(),
        playback_secs: a.demo_info.playback_time,
        roster,
        allied_tag,
        axis_tag,
        scores,
        text,
        rounds: s.rounds.len(),
        recorder,
        say,
        live_secs,
        announced_half,
        kills,
        clock: s
            .first_time_left
            .zip(s.last_time_left)
            .map(|(a, b)| (a.as_secs_f32(), b.as_secs_f32())),
    }
}

/// The half a server message names: "2nd half", "second half", "half 2"
/// (KTP prints `=== 2nd HALF STARTING ===`), or the first.
fn announced(text: &str) -> Option<u8> {
    let ws = words(text);
    for p in ws.windows(2) {
        let n = match (p[0].as_str(), p[1].as_str()) {
            ("1st" | "first", "half") | ("half", "1") => 1,
            ("2nd" | "second", "half") | ("half", "2") => 2,
            _ => continue,
        };
        return Some(n);
    }
    None
}

/// The answer the file name carries: everything before `_h1`/`_h2` is the
/// match, and the half is the digit. A name with more after the recorder
/// than one token (a split's `_dod_lennon2_1`) is a derived file.
#[derive(Clone, Debug)]
struct Truth {
    match_key: String,
    half: u8,
    note: Option<String>,
}

fn unalias(key: String) -> String {
    for (from, to) in ALIASES.lock().unwrap().iter() {
        if let Some(rest) = key.strip_prefix(from.as_str()) {
            return format!("{to}{rest}");
        }
    }
    key
}

fn truth_of(name: &str) -> Option<Truth> {
    let stem = name.strip_suffix(".dem")?;
    // KTP season 8 names put the recorder first and the opponent and map
    // after it (`ktps8w10qf-m00cat_soul_m1_saints_h1`), with typos in the
    // map; one opponent a week, so the week and map number name the match.
    if let Some((week, rest)) = stem.split_once('-')
        && week.starts_with("ktps8w")
    {
        let mut t = truth_of(&format!("x_{rest}.dem"))?;
        let map_no = rest.split(['_', '-']).find(|w| {
            w.len() == 2 && w.starts_with('m') && w[1..].chars().all(|c| c.is_ascii_digit())
        });
        t.match_key = match map_no {
            Some(m) => format!("{week}_{m}"),
            None => week.to_string(),
        };
        return Some(t);
    }
    let tokens: Vec<&str> = stem.split('_').collect();
    for (i, tok) in tokens.iter().enumerate() {
        let (half_tok, dup) = match tok.split_once('-') {
            Some((h, rest)) => (h, Some(rest)),
            None => (*tok, None),
        };
        let half = match half_tok {
            "h1" => 1,
            "h2" => 2,
            _ => continue,
        };
        let after = &tokens[i + 1..];
        let mut note = None;
        if dup.is_some_and(|d| d.len() <= 2 && d.chars().all(|c| c.is_ascii_digit()))
            || after.last().is_some_and(|t| t.ends_with("-1"))
        {
            note = Some("duplicate-numbered copy (-1)".to_string());
        }
        // A split's output: `<demo>_<map>_<n>.dem`.
        if dup.is_none()
            && after.len() > 2
            && after.contains(&"dod")
            && after
                .last()
                .is_some_and(|t| t.chars().all(|c| c.is_ascii_digit()))
        {
            note = Some(format!(
                "derived file (extra suffix _{})",
                after[1..].join("_")
            ));
        }
        return Some(Truth {
            match_key: unalias(tokens[..i].join("_")),
            half,
            note,
        });
    }
    None
}

fn load_one(path: &Path, cache_root: Option<&Path>) -> Result<(Analysis, &'static str), String> {
    if let Some(root) = cache_root
        && let Some((_, a)) = analysis::cache::load(root, path)
    {
        return Ok((a, "analyzer cache"));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Analysis::try_from_bytes(&bytes)
        .map(|a| (a, "parse"))
        .map_err(|e| format!("analyse {}: {e}", path.display()))
}

fn modified_unix(path: &Path) -> (u64, u64) {
    let m = std::fs::metadata(path).ok();
    let size = m.as_ref().map_or(0, |m| m.len());
    let t = m
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    (size, t)
}

fn load_features(
    paths: &[PathBuf],
    cache_root: Option<&Path>,
    feature_dir: Option<&Path>,
    threads: usize,
) -> Vec<Option<Features>> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<Features>>> = Mutex::new(vec![None; paths.len()]);
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else { break };
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    let (size, mtime) = modified_unix(path);
                    let fpath = feature_dir.map(|d| d.join(format!("{name}.{size}.{mtime}.json")));
                    let cached = fpath
                        .as_ref()
                        .and_then(|p| std::fs::read(p).ok())
                        .and_then(|b| serde_json::from_slice::<Features>(&b).ok());
                    let feats = match cached {
                        Some(f) => Some(f),
                        None => match load_one(path, cache_root) {
                            Ok((a, how)) => {
                                eprintln!("  {name}: {how}");
                                let f = features_of(&name, size, mtime, &a);
                                if let Some(p) = &fpath {
                                    let _ = std::fs::write(p, serde_json::to_vec(&f).unwrap());
                                }
                                Some(f)
                            }
                            Err(e) => {
                                eprintln!("  {name}: {e}");
                                None
                            }
                        },
                    };
                    out.lock().unwrap()[i] = feats;
                }
            });
        }
    });
    out.into_inner().unwrap()
}

/// How two demos' rosters relate: how many players they share, and how many
/// of those are on the same side in both.
struct Overlap {
    common: usize,
    same_side: usize,
    smaller: usize,
}

impl Overlap {
    fn of(a: &Features, b: &Features) -> Self {
        let mut common = 0;
        let mut same_side = 0;
        for (id, (_, side)) in &a.roster {
            if let Some((_, other)) = b.roster.get(id) {
                common += 1;
                if side == other {
                    same_side += 1;
                }
            }
        }
        Overlap {
            common,
            same_side,
            smaller: a.roster.len().min(b.roster.len()),
        }
    }
    /// Share of the smaller roster that is in both.
    fn share(&self) -> f64 {
        if self.smaller == 0 {
            0.0
        } else {
            self.common as f64 / self.smaller as f64
        }
    }
    fn same_share(&self) -> f64 {
        if self.common == 0 {
            0.0
        } else {
            self.same_side as f64 / self.common as f64
        }
    }
}

/// Fewest shared players for two demos to be the same game at all. A 6v6
/// half seen by two recorders shares ~12; two different games between one
/// clan and two opponents share ~6 (the clan) — hence the side rule too.
const MIN_SHARE: f64 = 0.6;
/// Share of the shared players that must be on the same side (same half) or
/// on the other side (other half).
const SIDE_AGREEMENT: f64 = 0.8;
/// Share of 3-kill runs two recordings of one half have in common, at
/// least. Measured: 0.00..0.01 between different games, near 1 for
/// overlapping recordings of one half.
const SHARED_KILL_RUNS: f64 = 0.3;
/// Share of shared players showing the same name. Measured: 0.83..1.00
/// within a match (both halves), 0.33..0.64 between rematches months apart.
const SAME_NAMES: f64 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    SameHalf,
    OtherHalf,
    Unrelated,
}

/// Set by `--tags-only`: relate demos by their sides' clan tags instead of
/// SteamIDs, as for demos whose players have none.
static TAGS_ONLY: AtomicBool = AtomicBool::new(false);

/// Set by `--roster-only`: the first version's rule, roster and sides
/// only, without the kill and name checks that keep rematches apart.
static ROSTER_ONLY: AtomicBool = AtomicBool::new(false);

/// Name keys that are the same match recorded under two naming schemes
/// (`--alias old=new`), so scoring doesn't count their merge as a miss.
static ALIASES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn relation(a: &Features, b: &Features) -> Relation {
    if a.map != b.map {
        return Relation::Unrelated;
    }
    let (by_roster, names) = if TAGS_ONLY.load(Ordering::Relaxed) {
        (tag_relation(a, b), name_overlap(a, b))
    } else {
        let o = Overlap::of(a, b);
        if o.share() < MIN_SHARE {
            return Relation::Unrelated;
        }
        let same = o.same_share();
        let by_roster = if same >= SIDE_AGREEMENT {
            Relation::SameHalf
        } else if 1.0 - same >= SIDE_AGREEMENT {
            Relation::OtherHalf
        } else {
            Relation::Unrelated
        };
        (by_roster, name_agreement(a, b))
    };
    if ROSTER_ONLY.load(Ordering::Relaxed) {
        return by_roster;
    }
    // The same two teams meet again on the same map (a league week, then
    // the playoffs): rosters and sides match, the game doesn't.
    match by_roster {
        Relation::SameHalf => match kill_overlap(a, b) {
            Some(k) if k >= SHARED_KILL_RUNS => Relation::SameHalf,
            // No kills in common: different stretches of play. Pieces of
            // one half (a recording stopped and started again) keep their
            // names; a rematch weeks later mostly doesn't.
            _ if names >= SAME_NAMES => Relation::SameHalf,
            _ => Relation::Unrelated,
        },
        Relation::OtherHalf if names >= SAME_NAMES => Relation::OtherHalf,
        _ => Relation::Unrelated,
    }
}

/// Same tags on the same sides is the same half; the same two tags on
/// swapped sides is the other half. Needs both sides tagged in both demos.
fn tag_relation(a: &Features, b: &Features) -> Relation {
    let (Some(aa), Some(ax), Some(ba), Some(bx)) = (
        a.allied_tag.as_deref(),
        a.axis_tag.as_deref(),
        b.allied_tag.as_deref(),
        b.axis_tag.as_deref(),
    ) else {
        return Relation::Unrelated;
    };
    let eq = |x: &str, y: &str| x.eq_ignore_ascii_case(y);
    if eq(aa, ba) && eq(ax, bx) {
        Relation::SameHalf
    } else if eq(aa, bx) && eq(ax, ba) {
        Relation::OtherHalf
    } else {
        Relation::Unrelated
    }
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut j = i;
    while parent[j] != r {
        let next = parent[j];
        parent[j] = r;
        j = next;
    }
    r
}

/// One half: the demos that recorded it.
struct HalfGroup {
    members: Vec<usize>,
}

struct Prediction {
    match_id: usize,
    half: Option<u8>,
    /// The half each signal alone gives, for scoring signals separately.
    by_server: Option<u8>,
    by_dates: Option<u8>,
    by_chat: Option<u8>,
    half_group: usize,
    why_half: String,
}

fn group(demos: &[Features]) -> (Vec<HalfGroup>, Vec<Prediction>) {
    let n = demos.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            if relation(&demos[i], &demos[j]) == Relation::SameHalf {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[b] = a;
                }
            }
        }
    }
    let mut by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        by_root.entry(r).or_default().push(i);
    }
    let halves: Vec<HalfGroup> = by_root
        .into_values()
        .map(|members| HalfGroup { members })
        .collect();

    // Pair halves into matches: votes of OtherHalf relations between members.
    let h = halves.len();
    let mut votes = vec![vec![0usize; h]; h];
    for a in 0..h {
        for b in a + 1..h {
            let mut v = 0;
            for &i in &halves[a].members {
                for &j in &halves[b].members {
                    if relation(&demos[i], &demos[j]) == Relation::OtherHalf {
                        v += 1;
                    }
                }
            }
            votes[a][b] = v;
            votes[b][a] = v;
        }
    }
    let mut pairs: Vec<(usize, usize, usize)> = Vec::new();
    for a in 0..h {
        for b in a + 1..h {
            if votes[a][b] > 0 {
                pairs.push((votes[a][b], a, b));
            }
        }
    }
    pairs.sort_by_key(|p| std::cmp::Reverse(p.0));
    let mut partner: Vec<Option<usize>> = vec![None; h];
    for (_, a, b) in pairs {
        if partner[a].is_none() && partner[b].is_none() {
            partner[a] = Some(b);
            partner[b] = Some(a);
        }
    }

    let mut preds: Vec<Option<Prediction>> = (0..n).map(|_| None).collect();
    let mut match_id = 0;
    let mut done = vec![false; h];
    for a in 0..h {
        if done[a] {
            continue;
        }
        done[a] = true;
        match partner[a] {
            Some(b) => {
                done[b] = true;
                let order = first_half(demos, &halves[a], &halves[b]);
                // `Some(true)` puts group `a` first; the half of a member of
                // group `g` follows from that.
                let half_of = |verdict: Option<bool>, g: usize| {
                    verdict.map(|a_first| if a_first == (g == a) { 1u8 } else { 2u8 })
                };
                for g in [a, b] {
                    for &i in &halves[g].members {
                        preds[i] = Some(Prediction {
                            match_id,
                            half: half_of(order.decided, g),
                            by_server: half_of(order.server, g),
                            by_dates: half_of(order.dates, g),
                            by_chat: half_of(order.chat, g),
                            half_group: g,
                            why_half: order.why.clone(),
                        });
                    }
                }
            }
            None => {
                // No other half to compare with: the group's own evidence,
                // what the server announced, else gg against "good half".
                let g = &halves[a];
                let server = g.members.iter().find_map(|&i| demos[i].announced_half);
                let (gg, gh) = end_chat(demos, g);
                let chat = match gg.cmp(&gh) {
                    std::cmp::Ordering::Greater => Some(2),
                    std::cmp::Ordering::Less => Some(1),
                    std::cmp::Ordering::Equal => None,
                };
                for &i in &g.members {
                    preds[i] = Some(Prediction {
                        match_id,
                        half: server.or(chat),
                        by_server: server,
                        by_dates: None,
                        by_chat: chat,
                        half_group: a,
                        why_half: format!(
                            "no other half found; server says {server:?}, chat gg/gh {gg}/{gh}"
                        ),
                    });
                }
            }
        }
        match_id += 1;
    }
    (halves, preds.into_iter().map(Option::unwrap).collect())
}

/// What each signal says about which of two half groups came first.
struct HalfOrder {
    /// `Some(true)`: group `a` is the first half.
    server: Option<bool>,
    dates: Option<bool>,
    chat: Option<bool>,
    decided: Option<bool>,
    why: String,
}

/// A recording's modified time is when its last frame was written, so two
/// recordings by one recorder end at least the later one's length apart.
/// Dates closer than this share of it are not recording times (a copy, a
/// download, an unzip) and say nothing about order.
const PLAUSIBLE_GAP_SHARE: f64 = 0.75;
/// The two halves of one match are played back to back, so their recordings
/// end at most the later one's length plus a break apart. A wider gap is a
/// file touched later (copied or edited one at a time).
const MAX_BREAK_SECS: f64 = 3600.0;

/// Per recorder that has a demo in both halves, which one their dates put
/// first, counting only plausible gaps. Returns (votes for a, votes for b,
/// implausible pairs).
fn date_votes(
    demos: &[Features],
    a: &HalfGroup,
    b: &HalfGroup,
) -> (usize, usize, usize, Vec<String>) {
    let (mut va, mut vb, mut bad) = (0, 0, 0);
    let mut voters = Vec::new();
    for &i in &a.members {
        let Some(ri) = &demos[i].recorder else {
            continue;
        };
        for &j in &b.members {
            if demos[j].recorder.as_ref() != Some(ri) {
                continue;
            }
            let (di, dj) = (&demos[i], &demos[j]);
            let later = if di.modified_unix >= dj.modified_unix {
                di
            } else {
                dj
            };
            let gap = di.modified_unix.abs_diff(dj.modified_unix) as f64;
            let len = later.playback_secs as f64;
            // A salvaged demo's header can carry no length (0 or NaN).
            let plausible = len.is_finite()
                && len > 0.0
                && gap >= PLAUSIBLE_GAP_SHARE * len
                && gap <= len + MAX_BREAK_SECS;
            if !plausible {
                bad += 1;
            } else {
                voters.push(format!("{} ~ {} {gap}s", di.name, dj.name));
                if di.modified_unix < dj.modified_unix {
                    va += 1;
                } else {
                    vb += 1;
                }
            }
        }
    }
    (va, vb, bad, voters)
}

/// Lower-cased words of a chat line, letters and digits only.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// "gg" and its spellings: what players say when a match ends.
fn is_gg(w: &str) -> bool {
    (w.len() >= 2 && w.chars().all(|c| c == 'g')) || matches!(w, "ggs" | "ggwp" | "gge")
}

/// "good half": what players say when the first half ends.
fn is_gh(words: &[String]) -> bool {
    words
        .iter()
        .any(|w| matches!(w.as_str(), "gh" | "ghs" | "nh"))
        || words
            .windows(2)
            .any(|p| p[1] == "half" && matches!(p[0].as_str(), "good" | "nice" | "gj"))
}

/// Distinct players saying gg, and saying "good half", in a half group's
/// demos (the same chat reaches every recorder, so it's a union).
fn end_chat(demos: &[Features], g: &HalfGroup) -> (usize, usize) {
    let mut gg = BTreeSet::new();
    let mut gh = BTreeSet::new();
    for &i in &g.members {
        for (t, sender, text) in &demos[i].say {
            if *t < demos[i].live_secs {
                continue;
            }
            let ws = words(text);
            if ws.iter().any(|w| is_gg(w)) {
                gg.insert(sender.clone());
            }
            if is_gh(&ws) {
                gh.insert(sender.clone());
            }
        }
    }
    (gg.len(), gh.len())
}

/// Which of two half groups came first, and why: the server's own
/// announcement when it makes one, then end-of-half chat (gg ends a match,
/// "gh" a first half), then plausible file dates.
fn first_half(demos: &[Features], a: &HalfGroup, b: &HalfGroup) -> HalfOrder {
    let (va, vb, bad, voters) = date_votes(demos, a, b);
    let dates = match va.cmp(&vb) {
        std::cmp::Ordering::Greater => Some(true),
        std::cmp::Ordering::Less => Some(false),
        std::cmp::Ordering::Equal => None,
    };
    let (gg_a, gh_a) = end_chat(demos, a);
    let (gg_b, gh_b) = end_chat(demos, b);
    // Evidence that a group ends the match: gg minus "good half".
    let end_a = gg_a as i64 - gh_a as i64;
    let end_b = gg_b as i64 - gh_b as i64;
    let chat = match end_a.cmp(&end_b) {
        std::cmp::Ordering::Less => Some(true),
        std::cmp::Ordering::Greater => Some(false),
        std::cmp::Ordering::Equal => None,
    };
    let said = |g: &HalfGroup| {
        let mut n = [0usize; 3];
        for &i in &g.members {
            if let Some(h) = demos[i].announced_half {
                n[h as usize] += 1;
            }
        }
        n
    };
    let (sa, sb) = (said(a), said(b));
    // Votes that `a` is first: `a` says first half or `b` says second.
    let a_first = sa[1] + sb[2];
    let b_first = sb[1] + sa[2];
    let server = match a_first.cmp(&b_first) {
        std::cmp::Ordering::Greater => Some(true),
        std::cmp::Ordering::Less => Some(false),
        std::cmp::Ordering::Equal => None,
    };
    let decided = server.or(chat).or(dates);
    let why = format!(
        "server says {a_first}:{b_first}, chat gg/gh {gg_a}/{gh_a} vs {gg_b}/{gh_b}, dates {va}:{vb} ({bad} implausible) {voters:?}"
    );
    HalfOrder {
        server,
        dates,
        chat,
        decided,
        why,
    }
}

fn main() {
    let mut positional = Vec::new();
    let mut feature_dir: Option<PathBuf> = None;
    let mut threads = 6usize;
    let mut dump = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--features-cache" => feature_dir = args.next().map(PathBuf::from),
            "--threads" => threads = args.next().and_then(|s| s.parse().ok()).unwrap_or(6),
            "--dump" => dump = true,
            "--tags-only" => TAGS_ONLY.store(true, Ordering::Relaxed),
            "--roster-only" => ROSTER_ONLY.store(true, Ordering::Relaxed),
            "--alias" => {
                if let Some((from, to)) = args.next().as_deref().and_then(|s| s.split_once('=')) {
                    ALIASES
                        .lock()
                        .unwrap()
                        .push((from.to_string(), to.to_string()));
                }
            }
            _ => positional.push(a),
        }
    }
    let folder = positional
        .first()
        .cloned()
        .or_else(|| std::env::var("DOD_ANALYSIS_DEMOS").ok())
        .unwrap_or_else(|| PRE_DOD.to_string());
    let filter = positional
        .get(1)
        .cloned()
        .unwrap_or_else(|| "wsod25_grp".into());
    let cache_root = std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("dod-studio").join("analyzer_cache"));
    if let Some(d) = &feature_dir {
        std::fs::create_dir_all(d).expect("create the features cache folder");
    }

    let mut paths: Vec<PathBuf> = std::fs::read_dir(&folder)
        .unwrap_or_else(|e| panic!("read {folder}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.ends_with(".dem") && n.contains(&filter)
        })
        .collect();
    paths.sort();
    eprintln!("{} demos matching {filter:?} in {folder}", paths.len());

    let loaded = load_features(
        &paths,
        cache_root.as_deref(),
        feature_dir.as_deref(),
        threads,
    );
    let mut demos: Vec<Features> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for f in loaded.into_iter().flatten() {
        match truth_of(&f.name) {
            Some(Truth { note: Some(n), .. }) if n.starts_with("derived") => {
                skipped.push(format!("{}: {n}", f.name))
            }
            _ => demos.push(f),
        }
    }

    if dump {
        for d in &demos {
            dump_demo(d);
        }
    }

    let (halves, preds) = group(&demos);
    report(&demos, &halves, &preds, &skipped);
    pair_margins(&demos);
    solo_halves(&demos);
}

fn dump_demo(d: &Features) {
    println!(
        "=== {} ({}, {} MB)",
        d.name,
        d.demo_type,
        d.size / 1_000_000
    );
    println!(
        "  map {} signons {:?} server {:?} addr {:?} hltv {:?} clan {} {:.0}s rounds {}",
        d.map,
        d.signon_maps,
        d.server_name,
        d.server_address,
        d.hltv_name,
        d.clan_match,
        d.playback_secs,
        d.rounds
    );
    println!("  tags allied {:?} axis {:?}", d.allied_tag, d.axis_tag);
    for side in [Side::Allied, Side::Axis] {
        let names: Vec<&str> = d
            .roster
            .values()
            .filter(|(_, s)| *s == side)
            .map(|(n, _)| n.as_str())
            .collect();
        println!("  {side:?} ({}): {}", names.len(), names.join(", "));
    }
    let sc: Vec<String> = d
        .scores
        .iter()
        .map(|(t, s, p)| format!("{t:.0}s {s:?}={p}"))
        .collect();
    println!("  scores: {}", sc.join(" | "));
    for t in &d.text {
        println!("    {t}");
    }
}

/// How far apart the classes are: for every pair of demos on one map, the
/// share of players they have in common and the share of those on the same
/// side, by what the names say the pair is. The rules' thresholds sit in the
/// gaps between these ranges.
fn pair_margins(demos: &[Features]) {
    let truths: Vec<Option<Truth>> = demos.iter().map(|d| truth_of(&d.name)).collect();
    // Per class: pair count and [min, max] of shared, same side, same names,
    // shared kill runs.
    let mut stats: BTreeMap<&str, (usize, [[f64; 2]; 4])> = BTreeMap::new();
    let mut different_maps = 0usize;
    let mut lookalikes = Vec::new();
    let mut odd_kills = Vec::new();
    for i in 0..demos.len() {
        for j in i + 1..demos.len() {
            let (Some(ti), Some(tj)) = (&truths[i], &truths[j]) else {
                continue;
            };
            if demos[i].map != demos[j].map {
                different_maps += 1;
                continue;
            }
            let class = match (ti.match_key == tj.match_key, ti.half == tj.half) {
                (true, true) => "same half",
                (true, false) => "other half",
                (false, _) => "other match, same map",
            };
            let o = Overlap::of(&demos[i], &demos[j]);
            let names = name_agreement(&demos[i], &demos[j]);
            let kills = kill_overlap(&demos[i], &demos[j]);
            let values = [o.share(), o.same_share(), names, kills.unwrap_or(f64::NAN)];
            let e = stats
                .entry(class)
                .or_insert((0, [[f64::INFINITY, f64::NEG_INFINITY]; 4]));
            e.0 += 1;
            for (range, v) in e.1.iter_mut().zip(values) {
                if !v.is_nan() {
                    range[0] = range[0].min(v);
                    range[1] = range[1].max(v);
                }
            }
            let odd = match (class, kills) {
                ("same half", Some(k)) => k < 0.3,
                ("other match, same map", Some(k)) => k >= 0.3,
                _ => false,
            };
            if odd {
                odd_kills.push(format!(
                    "{class}: {} ~ {}: kill runs {:.2}, clock {:?} vs {:?}",
                    demos[i].name,
                    demos[j].name,
                    kills.unwrap_or(0.0),
                    demos[i].clock,
                    demos[j].clock
                ));
            }
            if class.starts_with("other match") && o.share() >= MIN_SHARE {
                lookalikes.push(format!(
                    "{} ~ {}: shared {:.2}, same side {:.2}, same names {names:.2}, kill runs {}",
                    demos[i].name,
                    demos[j].name,
                    o.share(),
                    o.same_share(),
                    kills.map_or("-".into(), |k| format!("{k:.2}"))
                ));
            }
        }
    }
    println!();
    println!(
        "pair margins: shared players / smaller roster; same side / shared; same name / shared; shared 3-kill runs / fewer runs"
    );
    for (class, (n, r)) in &stats {
        println!(
            "  {class:<22} {n:>5} pairs: shared {:.2}..{:.2}, same side {:.2}..{:.2}, same names {:.2}..{:.2}, kill runs {:.2}..{:.2}",
            r[0][0], r[0][1], r[1][0], r[1][1], r[2][0], r[2][1], r[3][0], r[3][1]
        );
    }
    println!("  (on different maps: {different_maps} pairs, never grouped)");
    println!(
        "same-half pairs sharing few kill runs, other matches sharing many ({}):",
        odd_kills.len()
    );
    for l in &odd_kills {
        println!("  {l}");
    }
    println!(
        "other matches on the same map with the same players ({}):",
        lookalikes.len()
    );
    for l in &lookalikes {
        println!("  {l}");
    }
}

/// Of the players two demos share, the share showing the same name in both.
/// Players restyle their names between events, rarely within a match.
fn name_agreement(a: &Features, b: &Features) -> f64 {
    let (mut shared, mut same) = (0usize, 0usize);
    for (id, (name, _)) in &a.roster {
        if let Some((other, _)) = b.roster.get(id) {
            shared += 1;
            same += (name == other) as usize;
        }
    }
    if shared == 0 {
        0.0
    } else {
        same as f64 / shared as f64
    }
}

/// Names (not ids) two demos' rosters share, over the smaller roster: the
/// name check for `--tags-only`, where there are no SteamIDs to pair by.
fn name_overlap(a: &Features, b: &Features) -> f64 {
    let na: BTreeSet<&str> = a.roster.values().map(|(n, _)| n.as_str()).collect();
    let nb: BTreeSet<&str> = b.roster.values().map(|(n, _)| n.as_str()).collect();
    let smaller = na.len().min(nb.len());
    if smaller == 0 {
        0.0
    } else {
        na.intersection(&nb).count() as f64 / smaller as f64
    }
}

/// Runs of three consecutive kills (killer, victim, weapon) two demos have in
/// common, over the fewer runs either has: two recordings of one half share
/// nearly all; two different games almost none. `None` with under 10 kills.
fn kill_overlap(a: &Features, b: &Features) -> Option<f64> {
    let runs = |f: &Features| -> BTreeSet<Vec<(String, String, String)>> {
        f.kills.windows(3).map(|w| w.to_vec()).collect()
    };
    if a.kills.len() < 10 || b.kills.len() < 10 {
        return None;
    }
    let (ra, rb) = (runs(a), runs(b));
    let common = ra.intersection(&rb).count();
    Some(common as f64 / ra.len().min(rb.len()) as f64)
}

/// The half from one demo's own chat, with no other demo to compare: more
/// players saying gg than "good half" is a second half, the reverse a first.
/// What a `{half}` placeholder renaming one file at a time would have.
fn solo_halves(demos: &[Features]) {
    let (mut decided, mut right, mut scored) = (0, 0, 0);
    let mut wrong = Vec::new();
    for (i, d) in demos.iter().enumerate() {
        let Some(t) = truth_of(&d.name) else { continue };
        scored += 1;
        let (gg, gh) = end_chat(demos, &HalfGroup { members: vec![i] });
        let half = match gg.cmp(&gh) {
            std::cmp::Ordering::Greater => Some(2),
            std::cmp::Ordering::Less => Some(1),
            std::cmp::Ordering::Equal => None,
        };
        match half {
            Some(h) if h == t.half => {
                decided += 1;
                right += 1;
            }
            Some(_) => {
                decided += 1;
                wrong.push(format!("{} wrong (gg {gg}, gh {gh})", d.name));
            }
            None => wrong.push(format!("{} undecided (gg {gg}, gh {gh})", d.name)),
        }
    }
    println!();
    println!("one demo alone, by its own chat: decided {decided} of {scored}, right {right}");
    for w in wrong {
        println!("  {w}");
    }
}

fn report(demos: &[Features], halves: &[HalfGroup], preds: &[Prediction], skipped: &[String]) {
    // Map each predicted match to the truth match most of its demos carry,
    // so match ids can be compared.
    let truths: Vec<Option<Truth>> = demos.iter().map(|d| truth_of(&d.name)).collect();
    let mut votes: HashMap<usize, HashMap<String, usize>> = HashMap::new();
    for (p, t) in preds.iter().zip(&truths) {
        if let Some(t) = t {
            *votes
                .entry(p.match_id)
                .or_default()
                .entry(t.match_key.clone())
                .or_default() += 1;
        }
    }
    let label: HashMap<usize, String> = votes
        .iter()
        .map(|(m, v)| {
            let best = v.iter().max_by_key(|(_, c)| **c).unwrap().0.clone();
            (*m, best)
        })
        .collect();
    // A truth match split over two predicted matches counts the smaller part
    // as wrong.
    let mut truth_home: HashMap<String, (usize, usize)> = HashMap::new();
    for (m, v) in &votes {
        for (k, c) in v {
            let e = truth_home.entry(k.clone()).or_insert((*m, 0));
            if *c > e.1 {
                *e = (*m, *c);
            }
        }
    }

    println!();
    println!(
        "{:<44} {:>5} {:>4} {:>5} {:<22} {:<5} notes",
        "demo", "match", "half", "group", "truth", "ok"
    );
    let mut match_ok = 0;
    let mut half_ok = 0;
    let mut both_ok = 0;
    let mut scored = 0;
    // (decided, right) per signal alone.
    let mut server_score = (0usize, 0usize);
    let mut dates_score = (0usize, 0usize);
    let mut chat_score = (0usize, 0usize);
    let mut signal_misses = Vec::new();
    let mut misses = Vec::new();
    for ((d, p), t) in demos.iter().zip(preds).zip(&truths) {
        let Some(t) = t else {
            println!("{:<44} {:>5} (no truth in name)", d.name, p.match_id);
            continue;
        };
        scored += 1;
        // Right only when this demo's match is where most of its name's
        // match went, and most of that predicted match is its name's match:
        // a merge of two matches counts the smaller one wrong.
        let m_ok = truth_home.get(&t.match_key).map(|h| h.0) == Some(p.match_id)
            && label.get(&p.match_id) == Some(&t.match_key);
        let h_ok = p.half == Some(t.half);
        match_ok += m_ok as usize;
        half_ok += h_ok as usize;
        both_ok += (m_ok && h_ok) as usize;
        for (signal, verdict, score) in [
            ("server text", p.by_server, &mut server_score),
            ("dates", p.by_dates, &mut dates_score),
            ("chat", p.by_chat, &mut chat_score),
        ] {
            if let Some(h) = verdict {
                score.0 += 1;
                score.1 += (h == t.half) as usize;
                if h != t.half {
                    signal_misses.push(format!("{signal} says h{h}: {} ({})", d.name, p.why_half));
                }
            }
        }
        let ok = match (m_ok, h_ok) {
            (true, true) => "yes",
            (true, false) => "HALF",
            (false, true) => "MATCH",
            (false, false) => "BOTH",
        };
        println!(
            "{:<44} {:>5} {:>4} {:>5} {:<22} {:<5} {}{}",
            d.name,
            format!("m{}", p.match_id),
            p.half.map_or("?".into(), |h| format!("h{h}")),
            p.half_group,
            format!("{} h{}", t.match_key, t.half),
            ok,
            label.get(&p.match_id).map_or("", |s| s.as_str()),
            t.note
                .as_deref()
                .map(|n| format!("  [{n}]"))
                .unwrap_or_default()
        );
        if ok != "yes" {
            misses.push(format!(
                "{} -> m{} ({}) {:?}; truth {} h{}; half rule: {}",
                d.name,
                p.match_id,
                label.get(&p.match_id).map_or("", |s| s.as_str()),
                p.half,
                t.match_key,
                t.half,
                p.why_half
            ));
        }
    }
    println!();
    println!("half groups: {}", halves.len());
    for (g, h) in halves.iter().enumerate() {
        let names: Vec<&str> = h.members.iter().map(|&i| demos[i].name.as_str()).collect();
        let d = &demos[h.members[0]];
        let (gg, gh) = end_chat(demos, h);
        println!(
            "  group {g}: map {} allied {:?} axis {:?} gg {gg} gh {gh}: {}",
            d.map,
            d.allied_tag,
            d.axis_tag,
            names.join(" ")
        );
    }
    println!();
    println!(
        "scored {scored}: match right {match_ok} ({:.1}%), half right {half_ok} ({:.1}%), both {both_ok} ({:.1}%)",
        pct(match_ok, scored),
        pct(half_ok, scored),
        pct(both_ok, scored)
    );
    println!(
        "half by each signal alone (decided/right of {scored}): server text {}/{}, chat {}/{}, dates {}/{}",
        server_score.0, server_score.1, chat_score.0, chat_score.1, dates_score.0, dates_score.1
    );
    for m in &signal_misses {
        println!("  {m}");
    }
    // Pairwise: of the demo pairs put in one match, how many the names put
    // in one match (precision), and the reverse (recall).
    let (mut tp, mut fp, mut fn_) = (0usize, 0usize, 0usize);
    let mut unnamed_joined = BTreeSet::new();
    for i in 0..demos.len() {
        for j in i + 1..demos.len() {
            let same_pred = preds[i].match_id == preds[j].match_id;
            match (&truths[i], &truths[j]) {
                (Some(a), Some(b)) => {
                    let same_truth = a.match_key == b.match_key;
                    match (same_pred, same_truth) {
                        (true, true) => tp += 1,
                        (true, false) => fp += 1,
                        (false, true) => fn_ += 1,
                        _ => {}
                    }
                }
                (None, Some(_)) if same_pred => {
                    unnamed_joined.insert(demos[i].name.clone());
                }
                (Some(_), None) if same_pred => {
                    unnamed_joined.insert(demos[j].name.clone());
                }
                _ => {}
            }
        }
    }
    println!(
        "same-match pairs: {tp} right, {fp} wrongly joined, {fn_} wrongly apart (precision {:.1}%, recall {:.1}%)",
        pct(tp, tp + fp),
        pct(tp, tp + fn_)
    );
    if !unnamed_joined.is_empty() {
        println!("demos without a name answer grouped into a named match:");
        for n in &unnamed_joined {
            println!("  {n}");
        }
    }
    if !misses.is_empty() {
        println!("misses:");
        for m in &misses {
            println!("  {m}");
        }
    }
    if !skipped.is_empty() {
        println!("skipped:");
        for s in skipped {
            println!("  {s}");
        }
    }
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 {
        0.0
    } else {
        100.0 * a as f64 / b as f64
    }
}
