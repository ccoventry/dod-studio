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
#[path = "match_group_tests.rs"]
mod tests;
