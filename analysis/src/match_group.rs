//! Which match, and which half of it, a demo records, from its contents
//! alone (#685).
//!
//! File names and server details can't be trusted for this: HLTV demos
//! report the proxy's own server name (`<name>:0`), a loopback address and a
//! map checksum of 0, and one match's halves can be played on two servers.
//! What every recording of a half shares is its players, their sides and the
//! kill feed, so the grouping rests on those.
//!
//! - **Same half:** same map, at least [`MIN_SHARE`] of the smaller roster's
//!   SteamIDs in both, at least [`SIDE_AGREEMENT`] of those on the same side,
//!   and the kill feeds pair ([`is_same_half`]). Two recordings of one half
//!   carry the same kills; the same two teams meeting again on that map
//!   share players and sides but no kills. Without kills to pair (a piece of
//!   a half recorded at another time), the players' names must agree
//!   instead ([`SAME_NAMES`]).
//! - **Other half of the same match:** same map and players with the sides
//!   swapped, and the same names: players restyle their names between
//!   events, rarely within a match. A match has at most two halves; a half
//!   with several candidates pairs with the one most of its recordings agree
//!   with, then the one whose names agree best.
//! - **Demos without SteamIDs** (per-demo `CONNECTION_n` ids) relate by their
//!   sides' clan tags ([`detect_team_tag`]) and shared names instead.
//!
//! Which half is first, in order, the first signal that decides wins:
//!
//! 1. The server's own announcement (KTP's `=== 2nd HALF STARTING ===`).
//! 2. End-of-half chat after the match went live: "gh", "nh" or "good half"
//!    ends a first half, "gg" ends a match. Chat before live is warm-up talk,
//!    often a "gg" about the previous map.
//! 3. The server clock (`svc_time`, the `viewdemo` clock): when the server
//!    kept it running over the half, one half's recordings start after the
//!    other's end.
//! 4. The warm-up length, the server clock when the match went live: teams
//!    gather for a long time before a first half (616-1447 s in WSOD25) and
//!    not before a second (108-241 s). A habit, so it decides only with a
//!    wide margin.
//! 5. File dates, only when a recorder's two files are about a half's
//!    length apart. Copied or downloaded files carry copy times seconds
//!    apart, in whatever order the copy ran.
//!
//! Nothing else decides: an undecided half is [`Half::Unknown`], never a
//! default first half.
//!
//! Measured with the probe this module comes from (`rnd/match-half-685`,
//! `analysis/examples/match_half_probe.rs`): every one of the 371 named
//! demos in the movie install's `dod` folder in the right match, 369 in the
//! right half (the two misses look misnamed). Untested: the same two teams
//! on the same map with unchanged names and recordings that share no kills
//! would be put in one match.

use crate::{
    Analysis, ChatMessage, ChatType, MIN_PAIRS, Round, Team, detect_team_tag, is_same_half,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Fewest shared players, over the smaller roster, for two demos to be the
/// same game at all. A 6v6 half seen by two recorders shares about all of
/// them; two games between one clan and two opponents share that clan's
/// half (0.46-0.67 in WSOD25, where one clan played every match).
const MIN_SHARE: f64 = 0.6;
/// Share of the shared players on the same side (same half) or the other
/// side (other half). Measured 0.93-1.00 within a half.
const SIDE_AGREEMENT: f64 = 0.8;
/// Share of shared players showing the same name. Measured 0.83-1.00 within
/// a match (both halves), 0.33-0.64 between rematches months apart.
const SAME_NAMES: f64 = 0.75;
/// Fewer players with a SteamID (or `*fid`) than this and the demo relates
/// to others by clan tags and names instead.
const MIN_ROSTER: usize = 2;

/// Two dates closer than this share of the later recording's length are not
/// recording times (a recording's date is when its last frame was written,
/// so one recorder's two halves end at least a half apart).
const PLAUSIBLE_GAP_SHARE: f64 = 0.75;
/// Two halves are played back to back, so their recordings end at most the
/// later one's length plus a break apart; a wider gap is a file touched
/// later.
const MAX_BREAK_SECS: f64 = 3600.0;
/// The warm-up decides only when the longer one is at least this many times
/// the shorter...
const WARM_UP_RATIO: f64 = 2.0;
/// ...and at least this many seconds longer.
const WARM_UP_MARGIN_SECS: f64 = 120.0;

/// The side a team played: the Allied side (US or British) or Axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Allied,
    Axis,
}

impl Side {
    fn of(team: Option<&Team>) -> Option<Side> {
        match team {
            Some(Team::Allies | Team::British) => Some(Side::Allied),
            Some(Team::Axis) => Some(Side::Axis),
            _ => None,
        }
    }

    pub fn other(self) -> Side {
        match self {
            Side::Allied => Side::Axis,
            Side::Axis => Side::Allied,
        }
    }
}

/// Which half of its match a demo records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Half {
    First,
    Second,
    /// No signal decided it. A real answer, not an error: a `{half}` built on
    /// it shows that it doesn't know rather than guessing.
    Unknown,
}

impl Half {
    /// 1 or 2, `None` when unknown.
    pub fn number(self) -> Option<u8> {
        match self {
            Half::First => Some(1),
            Half::Second => Some(2),
            Half::Unknown => None,
        }
    }
}

/// The signal that decided a [`Half`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HalfReason {
    /// The server announced the half (KTP's `=== 2nd HALF STARTING ===`).
    ServerText,
    /// End-of-half chat after the match went live: "gh" ends a first half,
    /// "gg" a match.
    EndOfHalfChat,
    /// The server clock kept running over the half: one half's recordings
    /// start after the other's end.
    ServerClock,
    /// The server clock when the match went live: a long warm-up before the
    /// first half, a short one before the second.
    WarmUpLength,
    /// A recorder's two files are dated about a half's length apart.
    FileDates,
    /// Nothing decided it.
    Undecided,
}

/// A [`Half`] with the signal behind it and the numbers that signal saw.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HalfCall {
    pub half: Half,
    pub reason: HalfReason,
    /// What every signal saw, for a tooltip or a log.
    pub detail: String,
}

/// One team of a match: its players and the clan tag on their names.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MatchTeam {
    /// Most common tag on this team's side over the recordings, if any.
    pub tag: Option<String>,
    /// Global ids (SteamID64, else `PLAYER_<fid>`) seen on this team, sorted.
    pub players: Vec<String>,
}

/// One half of a match: the demos that recorded it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MatchHalf<I> {
    /// In input order.
    pub demos: Vec<I>,
    /// The side each of [`MatchGroup::teams`] played in this half.
    pub sides: [Side; 2],
    pub call: HalfCall,
}

impl<I> MatchHalf<I> {
    /// 1 or 2, `None` when unknown.
    pub fn order(&self) -> Option<u8> {
        self.call.half.number()
    }
}

/// The recordings of one match on one map: one or two halves.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MatchGroup<I> {
    /// Lower-cased map name.
    pub map: String,
    /// Team 0 is the Allied side of the first half listed.
    pub teams: [MatchTeam; 2],
    /// One or two; the first half first when the order is known.
    pub halves: Vec<MatchHalf<I>>,
}

/// The match and half `id` was grouped into by [`group_matches`].
pub fn find_demo<'g, I: PartialEq>(
    groups: &'g [MatchGroup<I>],
    id: &I,
) -> Option<(&'g MatchGroup<I>, &'g MatchHalf<I>)> {
    groups.iter().find_map(|g| {
        g.halves
            .iter()
            .find(|h| h.demos.contains(id))
            .map(|h| (g, h))
    })
}

/// Drops what the grouping never reads, so a whole folder's analyses can
/// wait in memory for one [`group_matches`] call: per-player health
/// histories, weapon breakdowns and the flag history are the bulk of an
/// analysis, and the grouping reads players, kills, chat, rounds and the
/// clock. The rest of the analysis stays as it is.
pub fn trim_for_grouping(analysis: &mut Analysis) {
    let s = &mut analysis.state;
    for p in &mut s.players {
        p.mortality = Vec::new();
        p.weapon_breakdown = HashMap::new();
    }
    s.objectives = Default::default();
}

/// What the grouping needs from one demo.
struct Features<'a> {
    analysis: &'a Analysis,
    map: String,
    modified_unix_secs: Option<u64>,
    playback_secs: f64,
    /// Global id → (name, side), for ids that name a player across demos.
    roster: BTreeMap<String, (String, Side)>,
    /// Every sided player's (name, side), ids or not.
    named: Vec<(String, Side)>,
    allied_tag: Option<String>,
    axis_tag: Option<String>,
    /// Kills after the match went live (`kill_streaks` restart at live).
    kills: usize,
    /// The recording player's id (POV), or `hltv:<proxy name>` (HLTV).
    recorder: Option<String>,
    /// Recording seconds (`real_offset`) when the match went live; 0 when it
    /// never did in this recording.
    live_secs: f64,
    /// The server clock when the match went live, when the recording saw it
    /// go live (the scoreboard-zeroing Reset→Start).
    clock_at_live: Option<f64>,
    /// The server clock at the recording's first and last frame.
    clock_start: f64,
    clock_end: f64,
    /// The half the server announced, if it did.
    announced: Option<u8>,
}

impl<'a> Features<'a> {
    fn of(analysis: &'a Analysis, modified_unix_secs: Option<u64>) -> Self {
        let s = &analysis.state;
        let mut map = analysis
            .demo_info
            .map_name
            .trim_end_matches('\0')
            .to_lowercase();
        if map.is_empty() {
            map = s
                .initial_map_name
                .clone()
                .unwrap_or_default()
                .to_lowercase();
        }

        // The side a player's kills and deaths were on, else the side at the
        // end: a player who drops to spectator at the end keeps his side.
        let mut votes: HashMap<String, [usize; 2]> = HashMap::new();
        for k in &s.kill_positions {
            for (id, team) in [(&k.killer, &k.killer_team), (&k.victim, &k.victim_team)] {
                if let (Some(id), Some(side)) = (id, Side::of(team.as_ref())) {
                    votes.entry(id.to_string()).or_default()[side as usize] += 1;
                }
            }
        }
        let mut roster = BTreeMap::new();
        let mut named = Vec::new();
        for p in &s.players {
            let id = p.id.to_string();
            let side = match votes.get(&id) {
                Some([allied, axis]) if allied != axis => Some(if allied > axis {
                    Side::Allied
                } else {
                    Side::Axis
                }),
                _ => Side::of(p.team.as_ref()),
            };
            let Some(side) = side else { continue };
            named.push((p.name.clone(), side));
            // A per-demo connection number identifies nobody across demos.
            if !id.starts_with("CONNECTION_") {
                roster.insert(id, (p.name.clone(), side));
            }
        }
        let tag_of = |side: Side| {
            let names: Vec<&str> = named
                .iter()
                .filter(|(_, s)| *s == side)
                .map(|(n, _)| n.as_str())
                .collect();
            detect_team_tag(&names)
        };
        let allied_tag = tag_of(Side::Allied);
        let axis_tag = tag_of(Side::Axis);

        let kills = s
            .players
            .iter()
            .flat_map(|p| &p.kill_streaks)
            .map(|k| k.kills.len())
            .sum();

        let recorder = if analysis.demo_info.demo_type == "HLTV" {
            Some(format!(
                "hltv:{}",
                s.hltv_name
                    .as_deref()
                    .or(s.server_name.as_deref())
                    .unwrap_or("?")
            ))
        } else {
            s.pov_player_index.and_then(|slot| {
                s.players
                    .iter()
                    .find(|p| {
                        matches!(p.connection, crate::Connection::Connected { client_id } if client_id == slot)
                    })
                    .map(|p| p.id.to_string())
            })
        };

        let live_round = s.rounds.first().map(|r| match r {
            Round::Active { start_time, .. } | Round::Completed { start_time, .. } => start_time,
        });
        let (live_secs, clock_at_live) = match live_round {
            Some(t) if s.match_start_witnessed => (
                t.real_offset.as_secs_f64(),
                Some(t.viewdemo_offset.as_secs_f64()),
            ),
            _ => (chat_live_secs(&s.chat_messages).unwrap_or(0.0), None),
        };

        let clock_end = s.current_time.viewdemo_offset.as_secs_f64();
        let clock_start = clock_end - s.current_time.real_offset.as_secs_f64();
        let announced = s
            .chat_messages
            .iter()
            .filter(|m| is_server(m))
            .find_map(|m| announced_half(&m.text));

        let playback = analysis.demo_info.playback_time as f64;
        Features {
            analysis,
            map,
            modified_unix_secs,
            playback_secs: if playback.is_finite() { playback } else { 0.0 },
            roster,
            named,
            allied_tag,
            axis_tag,
            kills,
            recorder,
            live_secs,
            clock_at_live,
            clock_start,
            clock_end,
            announced,
        }
    }
}

/// A message from the server rather than a player.
fn is_server(m: &ChatMessage) -> bool {
    m.chat_type == ChatType::System
        || m.sender_name
            .as_deref()
            .is_none_or(|n| n == "Console/Server")
}

/// When the match went live by the server's text, for a recording whose
/// scoreboard never showed the Reset→Start: the first round start or "match
/// is live" after the last clan-match countdown, else the first at all.
fn chat_live_secs(chat: &[ChatMessage]) -> Option<f64> {
    let goes_live = |m: &ChatMessage| {
        is_server(m)
            && (m
                .system_token
                .as_deref()
                .is_some_and(|t| t.starts_with("#game_roundstart"))
                || m.text.to_lowercase().contains("match is live"))
    };
    let secs = |m: &ChatMessage| m.time.real_offset.as_secs_f64();
    let countdown = chat
        .iter()
        .filter(|m| m.system_token.as_deref() == Some("#Clan_time_remaining"))
        .map(secs)
        .fold(0.0, f64::max);
    chat.iter()
        .find(|m| goes_live(m) && secs(m) >= countdown)
        .or_else(|| chat.iter().find(|m| goes_live(m)))
        .map(secs)
}

/// Lower-cased words of a chat line, letters and digits only.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// The half a server message names: "2nd half", "second half", "half 2"
/// (KTP prints `=== 2nd HALF STARTING ===`), or the first.
fn announced_half(text: &str) -> Option<u8> {
    words(text)
        .windows(2)
        .find_map(|p| match (p[0].as_str(), p[1].as_str()) {
            ("1st" | "first", "half") | ("half", "1") => Some(1),
            ("2nd" | "second", "half") | ("half", "2") => Some(2),
            _ => None,
        })
}

/// "gg" and its spellings: what players say when a match ends.
fn is_gg(w: &str) -> bool {
    (w.len() >= 2 && w.chars().all(|c| c == 'g')) || matches!(w, "ggs" | "ggwp" | "gge")
}

/// "good half": what players say when a first half ends. Not "ghf", a
/// "glhf" typo seen at the start of matches.
fn is_gh(words: &[String]) -> bool {
    words
        .iter()
        .any(|w| matches!(w.as_str(), "gh" | "ghs" | "nh"))
        || words
            .windows(2)
            .any(|p| p[1] == "half" && matches!(p[0].as_str(), "good" | "nice" | "gj"))
}

/// Distinct players saying gg, and saying "good half", after the match went
/// live in `demos`. The same chat reaches every recorder of a half, so it's
/// a union over them.
fn end_chat(demos: &[&Features]) -> (usize, usize) {
    let mut gg = BTreeSet::new();
    let mut gh = BTreeSet::new();
    for d in demos {
        for m in &d.analysis.state.chat_messages {
            if is_server(m) || m.time.real_offset.as_secs_f64() < d.live_secs {
                continue;
            }
            let sender = m.sender_name.clone().unwrap_or_default();
            let ws = words(&m.text);
            if ws.iter().any(|w| is_gg(w)) {
                gg.insert(sender.clone());
            }
            if is_gh(&ws) {
                gh.insert(sender);
            }
        }
    }
    (gg.len(), gh.len())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    SameHalf,
    OtherHalf,
    Unrelated,
}

/// Of the players two demos share, the share showing the same name in both.
fn name_agreement(a: &Features, b: &Features) -> f64 {
    let (mut shared, mut same) = (0usize, 0usize);
    for (id, (name, _)) in &a.roster {
        if let Some((other, _)) = b.roster.get(id) {
            shared += 1;
            same += (name == other) as usize;
        }
    }
    ratio(same, shared)
}

/// Names two demos share over the smaller list: the name check for demos
/// without ids to pair players by.
fn name_overlap(a: &Features, b: &Features) -> f64 {
    let na: BTreeSet<&str> = a.named.iter().map(|(n, _)| n.as_str()).collect();
    let nb: BTreeSet<&str> = b.named.iter().map(|(n, _)| n.as_str()).collect();
    ratio(na.intersection(&nb).count(), na.len().min(nb.len()))
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

/// Same tags on the same sides is the same half; the same two tags swapped
/// is the other half. Needs both sides tagged in both demos.
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

fn relation(a: &Features, b: &Features) -> Relation {
    if a.map != b.map {
        return Relation::Unrelated;
    }
    let (by_roster, names) = if a.roster.len() >= MIN_ROSTER && b.roster.len() >= MIN_ROSTER {
        let (mut common, mut same_side) = (0usize, 0usize);
        for (id, (_, side)) in &a.roster {
            if let Some((_, other)) = b.roster.get(id) {
                common += 1;
                same_side += (side == other) as usize;
            }
        }
        if ratio(common, a.roster.len().min(b.roster.len())) < MIN_SHARE {
            return Relation::Unrelated;
        }
        let same = ratio(same_side, common);
        let by_roster = if same >= SIDE_AGREEMENT {
            Relation::SameHalf
        } else if 1.0 - same >= SIDE_AGREEMENT {
            Relation::OtherHalf
        } else {
            Relation::Unrelated
        };
        (by_roster, name_agreement(a, b))
    } else {
        (tag_relation(a, b), name_overlap(a, b))
    };
    // The same two teams meet again on the same map (a league week, then
    // the playoffs): rosters and sides match, the game doesn't, and only
    // the kill feed tells.
    match by_roster {
        Relation::SameHalf => {
            if a.kills >= MIN_PAIRS && b.kills >= MIN_PAIRS && is_same_half(a.analysis, b.analysis)
            {
                Relation::SameHalf
            } else if names >= SAME_NAMES {
                // No kills in common: pieces of one half recorded at
                // different times (a recording stopped and restarted) keep
                // their names; a rematch weeks later mostly doesn't.
                Relation::SameHalf
            } else {
                Relation::Unrelated
            }
        }
        Relation::OtherHalf if names >= SAME_NAMES => Relation::OtherHalf,
        _ => Relation::Unrelated,
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

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    })
}

/// `Some(true)` when `a` wins, `Some(false)` when `b` does, `None` on a tie.
fn vote<T: Ord>(a: T, b: T) -> Option<bool> {
    match a.cmp(&b) {
        Ordering::Greater => Some(true),
        Ordering::Less => Some(false),
        Ordering::Equal => None,
    }
}

/// Votes that `a` comes first by the server's announcements: `a` says first
/// half or `b` says second, against the reverse.
fn server_order(a: &[&Features], b: &[&Features]) -> (Option<bool>, String) {
    let said = |g: &[&Features]| {
        let mut n = [0usize; 3];
        for d in g {
            if let Some(h) = d.announced {
                n[h as usize] += 1;
            }
        }
        n
    };
    let (sa, sb) = (said(a), said(b));
    let (a_first, b_first) = (sa[1] + sb[2], sb[1] + sa[2]);
    (
        vote(a_first, b_first),
        format!("server text {a_first}:{b_first}"),
    )
}

/// Evidence that a group ends the match (gg minus "good half"): the group
/// with less of it comes first.
fn chat_order(a: &[&Features], b: &[&Features]) -> (Option<bool>, String) {
    let (gg_a, gh_a) = end_chat(a);
    let (gg_b, gh_b) = end_chat(b);
    let end_a = gg_a as i64 - gh_a as i64;
    let end_b = gg_b as i64 - gh_b as i64;
    (
        vote(end_b, end_a),
        format!("chat gg/gh {gg_a}/{gh_a} vs {gg_b}/{gh_b}"),
    )
}

/// The server clock of the recordings that saw the match go live: when one
/// group's start after the other's end, the server kept its clock running
/// over the half. Recordings that started late are left out, so a piece
/// from the end of a first half can't look like it followed a second half
/// played after a map reload.
fn clock_order(a: &[&Features], b: &[&Features]) -> (Option<bool>, String) {
    let span = |g: &[&Features]| {
        let live: Vec<&&Features> = g.iter().filter(|d| d.clock_at_live.is_some()).collect();
        Some((
            median(live.iter().map(|d| d.clock_start).collect())?,
            median(live.iter().map(|d| d.clock_end).collect())?,
        ))
    };
    match (span(a), span(b)) {
        (Some((sa, ea)), Some((sb, eb))) => {
            let order = if sb > ea {
                Some(true)
            } else if sa > eb {
                Some(false)
            } else {
                None
            };
            (
                order,
                format!("server clock {sa:.0}-{ea:.0} s vs {sb:.0}-{eb:.0} s"),
            )
        }
        _ => (None, "server clock: no recording saw both go live".into()),
    }
}

/// The longer warm-up (server clock at match-live) comes first, when it is
/// clearly longer.
fn warm_up_order(a: &[&Features], b: &[&Features]) -> (Option<bool>, String) {
    let live = |g: &[&Features]| median(g.iter().filter_map(|d| d.clock_at_live).collect());
    let (Some(la), Some(lb)) = (live(a), live(b)) else {
        return (None, "warm-up: not seen in both".into());
    };
    let (long, short) = (la.max(lb), la.min(lb));
    let clear = long >= WARM_UP_RATIO * short && long - short >= WARM_UP_MARGIN_SECS;
    (
        clear.then_some(la > lb),
        format!("warm-up {la:.0} s vs {lb:.0} s"),
    )
}

/// Per recorder with a demo in both groups, which one their dates put
/// first, counting only gaps about a half long.
fn date_order(a: &[&Features], b: &[&Features]) -> (Option<bool>, String) {
    let (mut va, mut vb, mut skipped) = (0usize, 0usize, 0usize);
    for da in a {
        let (Some(ra), Some(ta)) = (&da.recorder, da.modified_unix_secs) else {
            continue;
        };
        for db in b {
            let Some(tb) = db.modified_unix_secs else {
                continue;
            };
            if db.recorder.as_ref() != Some(ra) {
                continue;
            }
            let later = if ta >= tb { da } else { db };
            let gap = ta.abs_diff(tb) as f64;
            let len = later.playback_secs;
            // A salvaged demo's header can carry no length.
            if len > 0.0 && gap >= PLAUSIBLE_GAP_SHARE * len && gap <= len + MAX_BREAK_SECS {
                if ta < tb {
                    va += 1;
                } else {
                    vb += 1;
                }
            } else {
                skipped += 1;
            }
        }
    }
    (
        vote(va, vb),
        format!("dates {va}:{vb} ({skipped} not a half apart)"),
    )
}

/// Which of two halves of one match came first, by the first signal that
/// decides. `Some(true)`: `a` is the first half.
fn order_pair(a: &[&Features], b: &[&Features]) -> (Option<bool>, HalfReason, String) {
    let signals: [(HalfReason, (Option<bool>, String)); 5] = [
        (HalfReason::ServerText, server_order(a, b)),
        (HalfReason::EndOfHalfChat, chat_order(a, b)),
        (HalfReason::ServerClock, clock_order(a, b)),
        (HalfReason::WarmUpLength, warm_up_order(a, b)),
        (HalfReason::FileDates, date_order(a, b)),
    ];
    let detail = signals
        .iter()
        .map(|(_, (_, d))| d.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    match signals.iter().find_map(|(r, (v, _))| v.map(|v| (v, *r))) {
        Some((v, r)) => (Some(v), r, detail),
        None => (None, HalfReason::Undecided, detail),
    }
}

/// The half of a group with no other half to compare with: what the server
/// announced, else "gg" against "good half".
fn order_alone(g: &[&Features]) -> HalfCall {
    let to_half = |n: u8| if n == 1 { Half::First } else { Half::Second };
    let (gg, gh) = end_chat(g);
    let mut said = [0usize; 3];
    for d in g {
        if let Some(h) = d.announced {
            said[h as usize] += 1;
        }
    }
    let detail = format!(
        "server text 1st/2nd {}/{}; chat gg/gh {gg}/{gh}",
        said[1], said[2]
    );
    let (half, reason) = if let Some(first) = vote(said[1], said[2]) {
        (to_half(if first { 1 } else { 2 }), HalfReason::ServerText)
    } else if let Some(second) = vote(gg, gh) {
        (
            to_half(if second { 2 } else { 1 }),
            HalfReason::EndOfHalfChat,
        )
    } else {
        (Half::Unknown, HalfReason::Undecided)
    };
    HalfCall {
        half,
        reason,
        detail,
    }
}

/// The half one demo records, from its own contents: the server's
/// announcement, else end-of-half chat after the match went live. For a
/// file renamed on its own; [`group_matches`] decides more, with the other
/// half's recordings and file dates to compare against.
pub fn half_of(analysis: &Analysis) -> HalfCall {
    order_alone(&[&Features::of(analysis, None)])
}

/// The team rosters and tags of a match, team 0 being the Allied side of
/// `halves[0]`.
fn teams_of(features: &[Features], halves: &[(Vec<usize>, [Side; 2])]) -> [MatchTeam; 2] {
    let mut players: [BTreeSet<String>; 2] = Default::default();
    let mut tags: [BTreeMap<String, usize>; 2] = Default::default();
    for (members, sides) in halves {
        for &i in members {
            let f = &features[i];
            for (team, side) in sides.iter().enumerate() {
                for (id, (_, s)) in &f.roster {
                    if s == side {
                        players[team].insert(id.clone());
                    }
                }
                let tag = match side {
                    Side::Allied => &f.allied_tag,
                    Side::Axis => &f.axis_tag,
                };
                if let Some(t) = tag {
                    *tags[team].entry(t.clone()).or_default() += 1;
                }
            }
        }
    }
    let team = |t: usize| MatchTeam {
        tag: tags[t]
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(tag, _)| tag.clone()),
        players: players[t].iter().cloned().collect(),
    };
    [team(0), team(1)]
}

/// Groups demos into matches and halves from their contents (see the module
/// docs for the rules). `demos` is `(id, analysis, file modified time in
/// Unix seconds)`; the time only breaks ties and may be `None`. Every demo
/// lands in exactly one half of one group; a demo nothing relates to is a
/// group of its own. Groups come in the order of their first demo.
pub fn group_matches<I: Clone>(demos: &[(I, &Analysis, Option<u64>)]) -> Vec<MatchGroup<I>> {
    let features: Vec<Features> = demos.iter().map(|(_, a, t)| Features::of(a, *t)).collect();
    let n = features.len();

    let mut rel = vec![Relation::Unrelated; n * n];
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            let r = relation(&features[i], &features[j]);
            rel[i * n + j] = r;
            rel[j * n + i] = r;
            if r == Relation::SameHalf {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[b.max(a)] = a.min(b);
                }
            }
        }
    }
    let mut by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        by_root.entry(r).or_default().push(i);
    }
    let halves: Vec<Vec<usize>> = by_root.into_values().collect();

    // Pair halves into matches, two at most: most other-half relations
    // between their recordings first, then the best name agreement.
    let h = halves.len();
    let mut candidates: Vec<(usize, u64, usize, usize)> = Vec::new();
    for a in 0..h {
        for b in a + 1..h {
            let mut votes = 0;
            let mut names = Vec::new();
            for &i in &halves[a] {
                for &j in &halves[b] {
                    if rel[i * n + j] == Relation::OtherHalf {
                        votes += 1;
                        names.push(name_agreement(&features[i], &features[j]));
                    }
                }
            }
            if votes > 0 {
                let mean = names.iter().sum::<f64>() / names.len() as f64;
                candidates.push((votes, (mean * 1e6) as u64, a, b));
            }
        }
    }
    candidates.sort_by(|x, y| (y.0, y.1, x.2, x.3).cmp(&(x.0, x.1, y.2, y.3)));
    let mut partner: Vec<Option<usize>> = vec![None; h];
    for (_, _, a, b) in candidates {
        if partner[a].is_none() && partner[b].is_none() {
            partner[a] = Some(b);
            partner[b] = Some(a);
        }
    }

    let ids =
        |members: &[usize]| -> Vec<I> { members.iter().map(|&i| demos[i].0.clone()).collect() };
    let refs =
        |members: &[usize]| -> Vec<&Features> { members.iter().map(|&i| &features[i]).collect() };
    let mut out = Vec::new();
    let mut done = vec![false; h];
    for a in 0..h {
        if done[a] {
            continue;
        }
        done[a] = true;
        let map = features[halves[a][0]].map.clone();
        let normal = [Side::Allied, Side::Axis];
        let swapped = [Side::Axis, Side::Allied];
        match partner[a] {
            Some(b) => {
                done[b] = true;
                let (order, reason, detail) = order_pair(&refs(&halves[a]), &refs(&halves[b]));
                // The first half first when known, else input order.
                let (first, second) = if order == Some(false) { (b, a) } else { (a, b) };
                let call = |is_first: bool| HalfCall {
                    half: match order {
                        None => Half::Unknown,
                        Some(_) if is_first => Half::First,
                        Some(_) => Half::Second,
                    },
                    reason,
                    detail: detail.clone(),
                };
                let teams = teams_of(
                    &features,
                    &[
                        (halves[first].clone(), normal),
                        (halves[second].clone(), swapped),
                    ],
                );
                out.push(MatchGroup {
                    map,
                    teams,
                    halves: vec![
                        MatchHalf {
                            demos: ids(&halves[first]),
                            sides: normal,
                            call: call(true),
                        },
                        MatchHalf {
                            demos: ids(&halves[second]),
                            sides: swapped,
                            call: call(false),
                        },
                    ],
                });
            }
            None => {
                let teams = teams_of(&features, &[(halves[a].clone(), normal)]);
                out.push(MatchGroup {
                    map,
                    teams,
                    halves: vec![MatchHalf {
                        demos: ids(&halves[a]),
                        sides: normal,
                        call: order_alone(&refs(&halves[a])),
                    }],
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalyzerState, Player, PlayerGlobalId, Weapon, kill::KillStreak, time::GameTime};
    use std::time::Duration;

    fn d(secs: f64) -> Duration {
        Duration::from_secs_f64(secs)
    }

    /// Player names after the tag, with no shared start, so the tag is the
    /// whole of what a team's names share.
    const WORDS: [&str; 6] = ["ace", "bo", "cy", "dax", "eli", "fen"];

    /// One synthetic recording: two six-player teams named `<tag>|<word>`
    /// with ids from `<tag><n>`, a kill feed, chat and clock.
    #[derive(Clone)]
    struct Spec {
        map: &'static str,
        allied: &'static str,
        axis: &'static str,
        hltv: bool,
        /// The recording player's id key; the first Allied player by default.
        recorder: Option<&'static str>,
        /// Every kill in the feed is with this weapon: another weapon is
        /// another game.
        weapon: Weapon,
        /// Added to every kill's clock time (an HLTV recording's offset).
        shift: f64,
        kills: usize,
        /// `(id key, name)`: a player who shows another name.
        renamed: Vec<(&'static str, &'static str)>,
        /// `(recording secs, sender, text)`; no sender is the server.
        chat: Vec<(f64, Option<&'static str>, &'static str)>,
        /// Recording secs the match went live, when the recording saw it.
        live: Option<f64>,
        /// The server clock at the recording's start.
        clock_start: f64,
        length: f64,
        /// Players with a per-demo `CONNECTION_n` id instead of a SteamID.
        no_ids: bool,
    }

    impl Default for Spec {
        fn default() -> Self {
            Spec {
                map: "dod_avalanche",
                allied: "alpha",
                axis: "bravo",
                hltv: false,
                recorder: None,
                weapon: Weapon::Garand,
                shift: 0.0,
                kills: 20,
                renamed: vec![],
                chat: vec![],
                live: None,
                clock_start: 0.0,
                length: 1800.0,
                no_ids: false,
            }
        }
    }

    impl Spec {
        fn analysis(&self) -> Analysis {
            let mut state = AnalyzerState::default();
            let recorder = self
                .recorder
                .map_or(format!("{}1", self.allied), String::from);
            for (side, tag) in [(Team::Allies, self.allied), (Team::Axis, self.axis)] {
                for n in 1..=6 {
                    let key = format!("{tag}{n}");
                    let slot = state.players.len() as u8;
                    if key == recorder && !self.hltv {
                        state.pov_player_index = Some(slot);
                    }
                    let mut p = Player::new_mock(slot, &key);
                    if self.no_ids {
                        p.id = serde_json::from_str::<PlayerGlobalId>(&format!(
                            "\"CONNECTION_{slot}\""
                        ))
                        .unwrap();
                    }
                    p.name = self
                        .renamed
                        .iter()
                        .find(|(k, _)| *k == key)
                        .map_or(format!("{tag}|{}", WORDS[n - 1]), |(_, name)| {
                            name.to_string()
                        });
                    p.team = Some(side.clone());
                    state.players.push(p);
                }
            }
            for i in 0..self.kills {
                let killer = i % 6;
                let victim = 6 + (i / 2) % 6;
                let victim_id = state.players[victim].id.clone();
                let secs = 30.0 + i as f64 * 31.7;
                let time = GameTime {
                    real_offset: d(secs),
                    viewdemo_offset: d(self.clock_start + secs + self.shift),
                    frame_index: 0,
                };
                state.players[killer].kill_streaks.push(KillStreak {
                    kills: vec![(time, self.weapon.clone(), victim_id)],
                    slot: None,
                });
            }
            for &(secs, sender, text) in &self.chat {
                state.chat_messages.push(ChatMessage {
                    time: GameTime {
                        real_offset: d(secs),
                        viewdemo_offset: d(self.clock_start + secs),
                        frame_index: 0,
                    },
                    frame_index: 0,
                    chat_type: if sender.is_some() {
                        ChatType::Mm1
                    } else {
                        ChatType::System
                    },
                    sender_name: sender.map(String::from),
                    sender_team: None,
                    sender_dead: false,
                    text: text.to_string(),
                    system_token: None,
                    system_args: vec![],
                });
            }
            if let Some(live) = self.live {
                state.match_start_witnessed = true;
                state.rounds = vec![Round::Active {
                    allies_kills: 0,
                    axis_kills: 0,
                    start_time: GameTime {
                        real_offset: d(live),
                        viewdemo_offset: d(self.clock_start + live),
                        frame_index: 0,
                    },
                }];
            }
            state.current_time = GameTime {
                real_offset: d(self.length),
                viewdemo_offset: d(self.clock_start + self.length),
                frame_index: 0,
            };
            state.hltv_name = Some("Test Server - HLTV".into());
            Analysis {
                demo_info: crate::DemoInfo {
                    map_name: self.map.to_string(),
                    demo_type: if self.hltv { "HLTV" } else { "POV" }.to_string(),
                    playback_time: self.length as f32,
                    ..Default::default()
                },
                state,
            }
        }
    }

    /// The first half of a match: alpha on the Allied side.
    fn h1() -> Spec {
        Spec::default()
    }

    /// Its second half: sides swapped, another kill feed, the same recorder.
    fn h2() -> Spec {
        Spec {
            allied: "bravo",
            axis: "alpha",
            recorder: Some("alpha1"),
            weapon: Weapon::K98,
            ..Spec::default()
        }
    }

    fn hltv(spec: Spec) -> Spec {
        Spec {
            hltv: true,
            shift: 1.342,
            ..spec
        }
    }

    type Input = (&'static str, Spec, Option<u64>);

    fn group(specs: &[Input]) -> Vec<MatchGroup<&'static str>> {
        let analyses: Vec<Analysis> = specs.iter().map(|(_, s, _)| s.analysis()).collect();
        let input: Vec<(&'static str, &Analysis, Option<u64>)> = specs
            .iter()
            .zip(&analyses)
            .map(|((id, _, t), a)| (*id, a, *t))
            .collect();
        group_matches(&input)
    }

    fn order_of(groups: &[MatchGroup<&'static str>], id: &'static str) -> (Half, HalfReason) {
        let (_, half) = find_demo(groups, &id).expect("grouped");
        (half.call.half, half.call.reason)
    }

    /// Three players saying `text` at 1790 s, after a live at 60 s.
    fn said(text: &'static str) -> Vec<(f64, Option<&'static str>, &'static str)> {
        ["alpha|p2", "bravo|p3", "alpha|p4"]
            .into_iter()
            .map(|who| (1790.0, Some(who), text))
            .collect()
    }

    #[test]
    fn two_recordings_of_each_half_make_one_match_of_two_halves() {
        let first = Spec {
            chat: said("gh"),
            live: Some(60.0),
            ..h1()
        };
        let second = Spec {
            chat: said("gg"),
            live: Some(60.0),
            ..h2()
        };
        // The second half first, to show the output is ordered by half.
        let groups = group(&[
            ("h2_pov", second.clone(), None),
            ("h1_pov", first.clone(), None),
            ("h2_hltv", hltv(second), None),
            ("h1_hltv", hltv(first), None),
        ]);
        assert_eq!(groups.len(), 1, "{groups:#?}");
        let g = &groups[0];
        assert_eq!(g.map, "dod_avalanche");
        assert_eq!(g.halves.len(), 2);
        assert_eq!(g.halves[0].demos, ["h1_pov", "h1_hltv"]);
        assert_eq!(g.halves[1].demos, ["h2_pov", "h2_hltv"]);
        assert_eq!(g.halves[0].order(), Some(1));
        assert_eq!(g.halves[1].order(), Some(2));
        assert_eq!(g.halves[0].call.reason, HalfReason::EndOfHalfChat);
        assert_eq!(g.halves[0].sides, [Side::Allied, Side::Axis]);
        assert_eq!(g.halves[1].sides, [Side::Axis, Side::Allied]);
        assert_eq!(g.teams[0].tag.as_deref(), Some("alpha"));
        assert_eq!(g.teams[1].tag.as_deref(), Some("bravo"));
        assert_eq!(g.teams[0].players.len(), 6);
        assert!(g.teams[0].players.iter().all(|id| id.contains(":alpha")));
    }

    #[test]
    fn trimmed_analyses_group_the_same() {
        let first = Spec {
            chat: said("gh"),
            live: Some(60.0),
            ..h1()
        };
        let second = Spec {
            chat: said("gg"),
            live: Some(60.0),
            ..h2()
        };
        let specs = [
            ("h2_pov", second.clone()),
            ("h1_pov", first.clone()),
            ("h2_hltv", hltv(second)),
            ("h1_hltv", hltv(first)),
        ];
        let whole: Vec<Analysis> = specs.iter().map(|(_, s)| s.analysis()).collect();
        let mut trimmed: Vec<Analysis> = specs.iter().map(|(_, s)| s.analysis()).collect();
        trimmed.iter_mut().for_each(trim_for_grouping);
        let input = |analyses: &[Analysis]| -> Vec<MatchGroup<&'static str>> {
            let demos: Vec<(&'static str, &Analysis, Option<u64>)> = specs
                .iter()
                .zip(analyses)
                .map(|((id, _), a)| (*id, a, None))
                .collect();
            group_matches(&demos)
        };
        assert_eq!(input(&whole), input(&trimmed));
    }

    #[test]
    fn different_maps_never_group() {
        let other_map = Spec {
            map: "dod_anzio",
            ..h1()
        };
        assert_eq!(group(&[("a", h1(), None), ("b", other_map, None)]).len(), 2);
    }

    #[test]
    fn the_same_clan_against_another_opponent_is_another_match() {
        // Six of twelve players shared: under the 60% bar.
        let other = Spec {
            axis: "charlie",
            ..h1()
        };
        let groups = group(&[("a", h1(), None), ("b", other, None)]);
        assert_eq!(groups.len(), 2, "{groups:#?}");
    }

    #[test]
    fn a_rematch_on_the_same_sides_shares_no_kills_and_stays_apart() {
        // Same teams, map and sides, another game (no kill pairs), and some
        // players have restyled their names since.
        let rematch = Spec {
            weapon: Weapon::Thompson,
            renamed: vec![
                ("alpha1", "alpha.p1"),
                ("alpha2", "alpha.p2"),
                ("bravo1", "bravo.p1"),
                ("bravo2", "bravo.p2"),
            ],
            ..h1()
        };
        let groups = group(&[("a", h1(), None), ("b", rematch, None)]);
        assert_eq!(groups.len(), 2, "{groups:#?}");
    }

    #[test]
    fn known_risk_a_rematch_with_unchanged_names_merges() {
        // The open risk the module docs name: no kills in common, but every
        // name agrees, so it reads as pieces of one half. This pins today's
        // answer so a fix has to change it on purpose.
        let rematch = Spec {
            weapon: Weapon::Thompson,
            ..h1()
        };
        let groups = group(&[("a", h1(), None), ("b", rematch, None)]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].halves[0].demos, ["a", "b"]);
    }

    #[test]
    fn same_sides_without_kills_need_the_names_to_agree() {
        let piece = Spec { kills: 0, ..h1() };
        assert_eq!(
            group(&[("a", h1(), None), ("b", piece.clone(), None)]).len(),
            1
        );
        let restyled = Spec {
            renamed: vec![
                ("alpha1", "x1"),
                ("alpha2", "x2"),
                ("bravo1", "x3"),
                ("bravo2", "x4"),
            ],
            ..piece
        };
        assert_eq!(group(&[("a", h1(), None), ("b", restyled, None)]).len(), 2);
    }

    #[test]
    fn a_half_with_two_candidate_partners_takes_the_closer_names_and_the_other_stays_alone() {
        // `x2` and `y2` both have swapped sides against `x1` and both pass
        // the name bar, but they disagree with each other on four names, so
        // they aren't one half. A match has two halves at most: x1 pairs
        // with the one whose names agree best.
        let x2 = Spec {
            renamed: vec![("alpha1", "x.1")],
            ..h2()
        };
        let y2 = Spec {
            weapon: Weapon::Thompson,
            renamed: vec![("alpha3", "y.3"), ("bravo4", "y.4"), ("bravo5", "y.5")],
            ..h2()
        };
        let groups = group(&[("y2", y2, None), ("x1", h1(), None), ("x2", x2, None)]);
        assert_eq!(groups.len(), 2, "{groups:#?}");
        let (g, _) = find_demo(&groups, &"x1").unwrap();
        let ids: Vec<_> = g.halves.iter().flat_map(|h| h.demos.clone()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"x2"), "{groups:#?}");
        let (alone, _) = find_demo(&groups, &"y2").unwrap();
        assert_eq!(alone.halves.len(), 1);
    }

    #[test]
    fn the_servers_announcement_decides_before_chat() {
        // Chat says the opposite (a "gg" in the first half's recording),
        // and the server text wins.
        let first = Spec {
            chat: said("gg"),
            live: Some(60.0),
            ..h1()
        };
        let mut second_chat = said("gh");
        second_chat.push((5.0, None, "=== 2nd HALF STARTING ==="));
        let second = Spec {
            chat: second_chat,
            live: Some(60.0),
            ..h2()
        };
        let groups = group(&[("a", first, None), ("b", second, None)]);
        assert_eq!(
            order_of(&groups, "a"),
            (Half::First, HalfReason::ServerText)
        );
        assert_eq!(
            order_of(&groups, "b"),
            (Half::Second, HalfReason::ServerText)
        );
    }

    #[test]
    fn warm_up_gg_before_live_is_not_end_of_match_chat() {
        // Before live, three players say gg about the previous map; after
        // it, one says "good half". Counted from the start, this first half
        // would read as the end of the match.
        let mut chat: Vec<_> = said("gg")
            .into_iter()
            .map(|(_, who, text)| (20.0, who, text))
            .collect();
        chat.push((1795.0, Some("bravo|p1"), "good half"));
        let first = Spec {
            chat,
            live: Some(60.0),
            ..h1()
        };
        let second = Spec {
            chat: vec![(1790.0, Some("alpha|p1"), "gg")],
            live: Some(60.0),
            ..h2()
        };
        let groups = group(&[("a", first, None), ("b", second, None)]);
        assert_eq!(
            order_of(&groups, "a"),
            (Half::First, HalfReason::EndOfHalfChat)
        );
        assert_eq!(
            order_of(&groups, "b"),
            (Half::Second, HalfReason::EndOfHalfChat)
        );
    }

    #[test]
    fn a_server_clock_kept_running_puts_the_later_half_second() {
        // No chat. The second half's recording starts after the first's
        // ends on the server clock. Its warm-up (clock at live) looks long,
        // which alone would say the opposite: the clock decides first.
        let first = Spec {
            live: Some(100.0),
            ..h1()
        };
        let second = Spec {
            live: Some(100.0),
            clock_start: 1900.0,
            ..h2()
        };
        let groups = group(&[("b", second, None), ("a", first, None)]);
        assert_eq!(
            order_of(&groups, "a"),
            (Half::First, HalfReason::ServerClock)
        );
        assert_eq!(
            order_of(&groups, "b"),
            (Half::Second, HalfReason::ServerClock)
        );
        assert_eq!(groups[0].halves[0].demos, ["a"]);
    }

    #[test]
    fn a_long_warm_up_is_the_first_half_only_by_a_wide_margin() {
        // The map reloaded at the half: both recordings start at clock 0.
        let long = Spec {
            live: Some(900.0),
            length: 2700.0,
            ..h1()
        };
        let short = Spec {
            live: Some(150.0),
            ..h2()
        };
        let groups = group(&[("a", long, None), ("b", short, None)]);
        assert_eq!(
            order_of(&groups, "a"),
            (Half::First, HalfReason::WarmUpLength)
        );
        assert_eq!(
            order_of(&groups, "b"),
            (Half::Second, HalfReason::WarmUpLength)
        );
        // 300 s against 200 s is not a clear habit: no answer.
        let a = Spec {
            live: Some(300.0),
            ..h1()
        };
        let b = Spec {
            live: Some(200.0),
            ..h2()
        };
        let groups = group(&[("a", a, None), ("b", b, None)]);
        assert_eq!(
            order_of(&groups, "a"),
            (Half::Unknown, HalfReason::Undecided)
        );
    }

    #[test]
    fn file_dates_count_only_when_a_recorders_two_files_are_a_half_apart() {
        // Recorded: the second file was finished 2000 s after the first.
        let groups = group(&[
            ("b", h2(), Some(1_700_002_000)),
            ("a", h1(), Some(1_700_000_000)),
        ]);
        assert_eq!(order_of(&groups, "a"), (Half::First, HalfReason::FileDates));
        assert_eq!(
            order_of(&groups, "b"),
            (Half::Second, HalfReason::FileDates)
        );
        // Copied: seconds apart, in whatever order the copy ran.
        let groups = group(&[
            ("b", h2(), Some(1_700_000_005)),
            ("a", h1(), Some(1_700_000_000)),
        ]);
        assert_eq!(
            order_of(&groups, "a"),
            (Half::Unknown, HalfReason::Undecided)
        );
        // Another recorder's file says nothing about this one's.
        let other_recorder = Spec {
            recorder: Some("bravo1"),
            ..h2()
        };
        let groups = group(&[
            ("b", other_recorder, Some(1_700_002_000)),
            ("a", h1(), Some(1_700_000_000)),
        ]);
        assert_eq!(
            order_of(&groups, "a"),
            (Half::Unknown, HalfReason::Undecided)
        );
    }

    #[test]
    fn half_of_one_file_uses_its_server_text_then_its_chat_and_never_guesses() {
        let announced = Spec {
            chat: vec![(5.0, None, "=== 2nd HALF STARTING ===")],
            ..h2()
        };
        let call = half_of(&announced.analysis());
        assert_eq!(
            (call.half, call.reason),
            (Half::Second, HalfReason::ServerText)
        );

        let good_half = Spec {
            chat: said("gh"),
            live: Some(60.0),
            ..h1()
        };
        let call = half_of(&good_half.analysis());
        assert_eq!(
            (call.half, call.reason),
            (Half::First, HalfReason::EndOfHalfChat)
        );
        assert_eq!(call.half.number(), Some(1));

        let call = half_of(&h1().analysis());
        assert_eq!(
            (call.half, call.reason),
            (Half::Unknown, HalfReason::Undecided)
        );
        assert_eq!(call.half.number(), None);
    }

    #[test]
    fn a_half_alone_is_ordered_by_its_own_recordings() {
        let second = Spec {
            chat: said("gg"),
            live: Some(60.0),
            ..h2()
        };
        let elsewhere = Spec {
            map: "dod_anzio",
            ..h1()
        };
        let groups = group(&[("a", second, None), ("b", elsewhere, None)]);
        assert_eq!(groups.len(), 2);
        assert_eq!(
            order_of(&groups, "a"),
            (Half::Second, HalfReason::EndOfHalfChat)
        );
        assert_eq!(
            order_of(&groups, "b"),
            (Half::Unknown, HalfReason::Undecided)
        );
    }

    #[test]
    fn demos_without_steam_ids_relate_by_clan_tags() {
        let no_ids = |s: Spec| Spec { no_ids: true, ..s };
        let other = Spec {
            axis: "charlie",
            ..h1()
        };
        let groups = group(&[
            ("h1_a", no_ids(h1()), None),
            ("h1_b", no_ids(hltv(h1())), None),
            ("h2_a", no_ids(h2()), None),
            ("other", no_ids(other), None),
        ]);
        assert_eq!(groups.len(), 2, "{groups:#?}");
        let (g, half) = find_demo(&groups, &"h1_a").unwrap();
        assert_eq!(half.demos, ["h1_a", "h1_b"]);
        assert_eq!(g.halves.len(), 2);
        assert_eq!(g.teams[0].tag.as_deref(), Some("alpha"));
        // No ids, no players listed.
        assert!(g.teams[0].players.is_empty());
    }

    #[test]
    fn the_fixture_is_a_group_of_its_own() {
        let a = crate::tests_fixture::analysis();
        let groups = group_matches(&[("fixture", a, None)]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].map, "dod_anzio");
        assert_eq!(groups[0].halves[0].demos, ["fixture"]);
    }

    /// The WSOD25 set the probe scored 68/68 (#685): every
    /// `wsod25_grp<N>_h<1|2>_<recorder>.dem` in `DOD_ANALYSIS_DEMOS` (a
    /// folder) or the PRE install's `dod` folder, parsed (the analyzer cache
    /// needs Studio) and scored against the names. Slow: run it in release,
    /// `cargo test -p analysis --release -- --ignored wsod25 --nocapture`.
    #[test]
    #[ignore]
    fn wsod25_matches_and_halves_match_the_file_names() {
        let folder = std::env::var_os("DOD_ANALYSIS_DEMOS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
                )
            });
        let entries = std::fs::read_dir(&folder)
            .unwrap_or_else(|e| panic!("{}: {e} (set DOD_ANALYSIS_DEMOS)", folder.display()));
        // (file name, match number, half); a split's output and other
        // derived names have more tokens and are left out.
        let mut named: Vec<(String, u32, u8)> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let stem = name.strip_suffix(".dem")?;
                let tokens: Vec<&str> = stem.split('_').collect();
                let [prefix, grp, half, _recorder] = tokens[..] else {
                    return None;
                };
                let group = grp.strip_prefix("grp")?.parse().ok()?;
                let half = match half {
                    "h1" => 1,
                    "h2" => 2,
                    _ => return None,
                };
                (prefix == "wsod25").then_some((name, group, half))
            })
            .collect();
        named.sort();
        assert!(
            !named.is_empty(),
            "no wsod25_grp*_h*_*.dem in {} (set DOD_ANALYSIS_DEMOS)",
            folder.display()
        );

        let next = std::sync::atomic::AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
        type Parsed = Option<(Analysis, Option<u64>)>;
        let parsed: std::sync::Mutex<Vec<Parsed>> =
            std::sync::Mutex::new((0..named.len()).map(|_| None).collect());
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((name, _, _)) = named.get(i) else {
                            break;
                        };
                        let path = folder.join(name);
                        let bytes = std::fs::read(&path)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        let analysis = Analysis::try_from_bytes(&bytes)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        let modified = std::fs::metadata(&path)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|t| t.as_secs());
                        parsed.lock().unwrap()[i] = Some((analysis, modified));
                    }
                });
            }
        });
        let parsed: Vec<(Analysis, Option<u64>)> = parsed
            .into_inner()
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let input: Vec<(usize, &Analysis, Option<u64>)> = parsed
            .iter()
            .enumerate()
            .map(|(i, (a, t))| (i, a, *t))
            .collect();
        let groups = group_matches(&input);

        let (mut match_right, mut half_right) = (0, 0);
        let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
        let mut misses = Vec::new();
        for (i, (name, grp, half)) in named.iter().enumerate() {
            let (g, h) = find_demo(&groups, &i).unwrap();
            let in_match: BTreeSet<usize> = g.halves.iter().flat_map(|h| h.demos.clone()).collect();
            let truth_match: BTreeSet<usize> =
                (0..named.len()).filter(|&j| named[j].1 == *grp).collect();
            let in_half: BTreeSet<usize> = h.demos.iter().copied().collect();
            let truth_half: BTreeSet<usize> = (0..named.len())
                .filter(|&j| named[j].1 == *grp && named[j].2 == *half)
                .collect();
            let m_ok = in_match == truth_match;
            let h_ok = in_half == truth_half && h.order() == Some(*half);
            match_right += m_ok as usize;
            half_right += h_ok as usize;
            *reasons.entry(format!("{:?}", h.call.reason)).or_default() += 1;
            if !(m_ok && h_ok) {
                misses.push(format!(
                    "{name}: match {m_ok}, half {h_ok} ({:?} by {:?}: {})",
                    h.call.half, h.call.reason, h.call.detail
                ));
            }
        }
        println!(
            "{} demos, {} matches found: right match {match_right}, right half {half_right}; half decided by {reasons:?}",
            named.len(),
            groups.len()
        );
        for g in &groups {
            println!(
                "  {} {:?} vs {:?}: {:?}",
                g.map,
                g.teams[0].tag,
                g.teams[1].tag,
                g.halves
                    .iter()
                    .map(|h| (h.order(), h.demos.len(), h.call.detail.clone()))
                    .collect::<Vec<_>>()
            );
        }
        assert!(misses.is_empty(), "misses:\n{}", misses.join("\n"));
    }
}
