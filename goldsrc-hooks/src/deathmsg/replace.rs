//! `ds_hud_deathmsg replace` (#1021): change who got a kill, who died or the
//! weapon on one kill-feed line, or hide that line.
//!
//! ```text
//! replace [killer <id>] [victim <id>] [at <time>] [in <demo>[,<demo>...]] with [killer <id>] [victim <id>] [weapon <weapon>]
//! replace ... with hide
//! replace list | remove <n> | clear
//! ```
//!
//! The parts before `with` pick the kill (a part left out matches any); the
//! parts after it say what changes (a part left out stays as recorded). It
//! has more than three values and optional ones, so the parameters are named
//! and come in any order (`docs/dodstudio_commands.md`).
//!
//! It works on the `DeathMsg` the hook already catches: three bytes, killer,
//! victim and weapon index. A matched message reaches `client.dll` with those
//! bytes rewritten, or not at all. The game picks each name's colour from the
//! slot's team, so the colours follow the new killer and victim by
//! themselves. Nothing else reads `DeathMsg` for what this changes: the
//! scoreboard's frags come from `ScoreInfo`, the red screen and the kill sound
//! are messages of their own, and the demo file is never written.
//!
//! Matching keeps no state between kills, so a kill played again after a seek
//! (forward, or the backward seek that replays notices twice) matches again.
//!
//! The rules live in two places: [`RULES`], under a mutex that only console
//! commands take, and [`LIVE`], an immutable copy the message handler reads
//! through one atomic load. A change publishes a new copy and leaks the old
//! one (a few hundred bytes per command), so a handler mid-read never sees
//! it freed.

use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use super::{COMMAND, Player, WEAPON_SPRITES, Who, parse_player, parse_weapon, rejoin_colons};

/// How far `at` may be from the kill, in seconds of the demo clock.
const AT_TOLERANCE: f64 = 2.0;

/// The highest slot a `DeathMsg` names; 0 is the world.
const MAX_SLOT: i32 = 32;

/// One `replace` line.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Rule {
    killer: Option<Player>,
    victim: Option<Player>,
    /// The demo clock, seconds.
    at: Option<f64>,
    /// File-name patterns, lower case and without `.dem`; empty matches every
    /// demo.
    demos: Vec<String>,
    change: Change,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Change {
    Hide,
    Set {
        killer: Option<Player>,
        victim: Option<Player>,
        weapon: Option<u8>,
    },
}

/// What the handler knows about the moment a kill arrives.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Context<'a> {
    /// The span the kill's time lies in, or `None` with no clock. One point
    /// in play; one seek step wide while a stepped seek replays several
    /// seconds in one frame (see [`window`]).
    window: Option<(f64, f64)>,
    /// The loaded demo's file name, lower case and without `.dem`.
    demo: Option<&'a str>,
}

/// What to do with one `DeathMsg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Keep,
    /// Rule `n` (0-based) hides it.
    Hide(usize),
    /// Rule `n` rewrites its three bytes.
    Rewrite(usize, [u8; 3]),
    /// Rule `n` matched, but this player isn't in the demo, so the line is
    /// left as recorded.
    Unresolved(usize, Player),
}

impl Rule {
    fn players(&self) -> impl Iterator<Item = Player> + '_ {
        let after = match self.change {
            Change::Hide => [None, None],
            Change::Set { killer, victim, .. } => [killer, victim],
        };
        [self.killer, self.victim]
            .into_iter()
            .chain(after)
            .flatten()
    }

    /// Whether picking the kill needs more than the two slot numbers.
    fn needs_player_info(&self) -> bool {
        [self.killer, self.victim]
            .into_iter()
            .flatten()
            .any(|p| p.needs_player_info())
    }

    fn matches(&self, killer: &Who, victim: &Who, ctx: &Context) -> bool {
        self.killer.is_none_or(|p| p.matches(killer))
            && self.victim.is_none_or(|p| p.matches(victim))
            && self.at.is_none_or(|at| {
                ctx.window
                    .is_some_and(|(lo, hi)| at - AT_TOLERANCE <= hi && at + AT_TOLERANCE >= lo)
            })
            && (self.demos.is_empty()
                || ctx
                    .demo
                    .is_some_and(|demo| self.demos.iter().any(|p| glob(p, demo))))
    }

    /// The line as `list` shows it, in the order the usage gives.
    fn describe(&self) -> String {
        let mut out = Vec::new();
        if let Some(p) = self.killer {
            out.push(format!("killer {p}"));
        }
        if let Some(p) = self.victim {
            out.push(format!("victim {p}"));
        }
        if let Some(at) = self.at {
            out.push(format!("at {}", clock_text(at)));
        }
        if !self.demos.is_empty() {
            out.push(format!("in {}", self.demos.join(",")));
        }
        out.push("with".to_string());
        match self.change {
            Change::Hide => out.push("hide".to_string()),
            Change::Set {
                killer,
                victim,
                weapon,
            } => {
                if let Some(p) = killer {
                    out.push(format!("killer {p}"));
                }
                if let Some(p) = victim {
                    out.push(format!("victim {p}"));
                }
                if let Some(w) = weapon {
                    out.push(format!("weapon {}", weapon_text(w)));
                }
            }
        }
        out.join(" ")
    }
}

/// A weapon by the name `fake` takes, or its number where two indices share
/// a sprite (`d_stick`, `d_grenade`, `d_mortar`) and the name would pick the
/// first.
fn weapon_text(index: u8) -> String {
    let name = WEAPON_SPRITES[index as usize];
    let first = WEAPON_SPRITES.iter().position(|n| *n == name);
    match first {
        Some(i) if i == index as usize => name.trim_start_matches("d_").to_string(),
        _ => index.to_string(),
    }
}

/// `m:ss`, `h:mm:ss`, with tenths when there are any.
fn clock_text(seconds: f64) -> String {
    let tenths = (seconds * 10.0).round() as u64;
    let (h, m, s, t) = (
        tenths / 36_000,
        tenths / 600 % 60,
        tenths / 10 % 60,
        tenths % 10,
    );
    let secs = if t == 0 {
        format!("{s:02}")
    } else {
        format!("{s:02}.{t}")
    };
    if h > 0 {
        format!("{h}:{m:02}:{secs}")
    } else {
        format!("{m}:{secs}")
    }
}

/// The first rule that matches decides; later ones are not looked at.
/// `slot_of` finds where a player is in the loaded demo.
pub(super) fn decide(
    rules: &[Rule],
    bytes: [u8; 3],
    killer: &Who,
    victim: &Who,
    ctx: &Context,
    slot_of: impl Fn(Player) -> Option<i32>,
) -> Outcome {
    let Some((n, rule)) = rules
        .iter()
        .enumerate()
        .find(|(_, r)| r.matches(killer, victim, ctx))
    else {
        return Outcome::Keep;
    };
    let Change::Set {
        killer,
        victim,
        weapon,
    } = rule.change
    else {
        return Outcome::Hide(n);
    };
    let mut out = bytes;
    for (byte, player) in [(0, killer), (1, victim)] {
        let Some(player) = player else { continue };
        match slot_of(player).filter(|s| (0..=MAX_SLOT).contains(s)) {
            Some(slot) => out[byte] = slot as u8,
            None => return Outcome::Unresolved(n, player),
        }
    }
    if let Some(w) = weapon {
        out[2] = w;
    }
    Outcome::Rewrite(n, out)
}

/// `*` and `?` over `text`, both already lower case.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// A demo name as `in` compares it: the file name only, lower case, no
/// `.dem`. Both the pattern and the loaded demo go through this.
fn demo_stem(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let lower = file.to_ascii_lowercase();
    lower.strip_suffix(".dem").unwrap_or(&lower).to_string()
}

/// `at`'s value: seconds, `m:ss` or `h:mm:ss`, each with decimals allowed on
/// the last part.
fn parse_time(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let (last, whole) = parts.split_last()?;
    let mut seconds = 0.0;
    for part in whole {
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    let last: f64 = last.parse().ok()?;
    if !last.is_finite() || last < 0.0 || (!whole.is_empty() && last >= 60.0) {
        return None;
    }
    Some(seconds * 60.0 + last)
}

/// A parsed line and what the console should warn about.
#[derive(Debug)]
pub(super) struct Parsed {
    rule: Rule,
    warnings: Vec<String>,
}

fn parse_id(word: &str, value: &str) -> Result<Player, String> {
    let player = parse_player(value)
        .ok_or_else(|| format!("`{word}` takes a slot number, a SteamID or self, not {value:?}"))?;
    if let Player::Slot(slot) = player
        && !(0..=MAX_SLOT).contains(&slot)
    {
        return Err(format!(
            "`{word}`: slot {slot} is out of range (0..{MAX_SLOT})"
        ));
    }
    Ok(player)
}

/// Puts `a,` `b` and `a` `,b` back into `a,b`: `in` takes a comma list, and
/// a space after a comma is an easy slip.
fn take_list(tokens: &[String], at: &mut usize) -> String {
    let mut value = tokens[*at].clone();
    let keyword = |t: &str| {
        ["with", "killer", "victim", "at", "in"]
            .iter()
            .any(|k| t.eq_ignore_ascii_case(k))
    };
    while *at + 1 < tokens.len()
        && !keyword(&tokens[*at + 1])
        && (value.ends_with(',') || tokens[*at + 1].starts_with(','))
    {
        *at += 1;
        value.push_str(&tokens[*at]);
    }
    value
}

/// Everything after `replace`, when it isn't `list`, `remove` or `clear`.
pub(super) fn parse(args: &[String]) -> Result<Parsed, String> {
    let tokens = rejoin_colons(args);
    let mut before_with = true;
    let (mut killer, mut victim, mut at, mut demos) = (None, None, None, None::<Vec<String>>);
    let (mut new_killer, mut new_victim, mut weapon, mut hide) = (None, None, None, false);
    let mut i = 0;
    while i < tokens.len() {
        let word = tokens[i].to_ascii_lowercase();
        let twice = || {
            Err(format!(
                "`{word}` is given twice {} `with`",
                if before_with { "before" } else { "after" }
            ))
        };
        if word == "with" {
            if !before_with {
                return Err("`with` is given twice".to_string());
            }
            before_with = false;
            i += 1;
            continue;
        }
        if word == "hide" && !before_with {
            if hide {
                return twice();
            }
            hide = true;
            i += 1;
            continue;
        }
        let takes_value = matches!(word.as_str(), "killer" | "victim")
            || (before_with && matches!(word.as_str(), "at" | "in"))
            || (!before_with && word == "weapon");
        if !takes_value {
            let hint = match (word.as_str(), before_with) {
                ("at" | "in", false) => " (it goes before `with`)",
                ("weapon" | "hide", true) => " (it goes after `with`)",
                _ => "",
            };
            return Err(format!("unknown word `{}`{hint}", tokens[i]));
        }
        if tokens
            .get(i + 1)
            .is_none_or(|next| next.eq_ignore_ascii_case("with"))
        {
            return Err(format!("`{word}` needs a value after it"));
        }
        i += 1;
        let value = tokens[i].as_str();
        match (word.as_str(), before_with) {
            ("killer", true) if killer.is_some() => return twice(),
            ("killer", true) => killer = Some(parse_id(&word, value)?),
            ("victim", true) if victim.is_some() => return twice(),
            ("victim", true) => victim = Some(parse_id(&word, value)?),
            ("killer", false) if new_killer.is_some() => return twice(),
            ("killer", false) => new_killer = Some(parse_id(&word, value)?),
            ("victim", false) if new_victim.is_some() => return twice(),
            ("victim", false) => new_victim = Some(parse_id(&word, value)?),
            ("at", _) if at.is_some() => return twice(),
            ("at", _) => {
                at = Some(parse_time(value).ok_or_else(|| {
                    format!("`at` takes the demo clock as m:ss or seconds, not {value:?}")
                })?)
            }
            ("in", _) if demos.is_some() => return twice(),
            ("in", _) => {
                let list = take_list(&tokens, &mut i);
                let names: Vec<String> = list
                    .split(',')
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(demo_stem)
                    .collect();
                if names.is_empty() {
                    return Err("`in` needs a demo name".to_string());
                }
                demos = Some(names);
            }
            ("weapon", _) if weapon.is_some() => return twice(),
            ("weapon", _) => {
                let index = parse_weapon(value).map_err(|why| format!("`weapon`: {why}"))?;
                weapon = Some(index as u8);
            }
            _ => unreachable!("takes_value covers every word"),
        }
        i += 1;
    }
    if before_with {
        return Err(
            "no `with`: say what changes, as in `with killer <id>`, `with weapon garand` or `with hide`"
                .to_string(),
        );
    }
    if killer.is_none() && victim.is_none() {
        return Err(
            "name the `killer` or the `victim` before `with`, so a slip can't change every kill"
                .to_string(),
        );
    }
    let set = new_killer.is_some() || new_victim.is_some() || weapon.is_some();
    if hide && set {
        return Err("`hide` goes alone after `with`: a hidden line has nothing to change".into());
    }
    if !hide && !set {
        return Err(
            "nothing after `with`: give `killer <id>`, `victim <id>`, `weapon <weapon>` or `hide`"
                .to_string(),
        );
    }
    let rule = Rule {
        killer,
        victim,
        at,
        demos: demos.unwrap_or_default(),
        change: if hide {
            Change::Hide
        } else {
            Change::Set {
                killer: new_killer,
                victim: new_victim,
                weapon,
            }
        },
    };
    let warnings = warnings(&rule);
    Ok(Parsed { rule, warnings })
}

/// `self` and slots are a different player in each demo, so without `in` the
/// line may change the wrong kill in the next one. Warned, not refused: one
/// demo loaded by hand is a fair use.
fn warnings(rule: &Rule) -> Vec<String> {
    if !rule.demos.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if rule.players().any(|p| p == Player::OwnPov) {
        out.push(
            "self is a different player in each POV demo; add `in <demo>` or use a SteamID"
                .to_string(),
        );
    }
    let mut slots: Vec<i32> = rule
        .players()
        .filter_map(|p| match p {
            Player::Slot(s) => Some(s),
            _ => None,
        })
        .collect();
    slots.sort_unstable();
    slots.dedup();
    for slot in slots {
        out.push(format!(
            "slot {slot} is a different player in each demo; add `in <demo>` or use a SteamID"
        ));
    }
    out
}

// ── The live list ────────────────────────────────────────────────────────────

/// The list as commands change it. Only console commands take this lock.
static RULES: Mutex<Vec<Rule>> = Mutex::new(Vec::new());

/// The copy the `DeathMsg` handler reads; null while the list is empty.
static LIVE: AtomicPtr<Vec<Rule>> = AtomicPtr::new(std::ptr::null_mut());

/// One bit per rule (the first 64) that has already said a player it names
/// isn't in the demo, so each says it once. Cleared by every change.
static UNRESOLVED_SAID: AtomicU64 = AtomicU64::new(0);

fn publish(rules: &[Rule]) {
    let fresh = if rules.is_empty() {
        std::ptr::null_mut()
    } else {
        Box::into_raw(Box::new(rules.to_vec()))
    };
    // The old copy is leaked, never freed: see the module doc.
    LIVE.swap(fresh, Ordering::AcqRel);
    UNRESOLVED_SAID.store(0, Ordering::Relaxed);
}

/// The rules in force, for the handler. Lock-free.
pub(super) fn live() -> &'static [Rule] {
    let ptr = LIVE.load(Ordering::Acquire);
    if ptr.is_null() {
        return &[];
    }
    // Safety: every pointer stored in LIVE came from `Box::into_raw` and is
    // never freed.
    unsafe { &*ptr }
}

/// For the status line.
pub(super) fn count() -> usize {
    live().len()
}

/// The span a kill arriving now happened in, on the demo bar's clock.
///
/// The demo player's world clock is the one `at` is written in; in play it
/// reads the kill's own time (PRE, greetz1: 442.5, 464.4 and 537.9, the
/// analyzer's times to the tenth). A stepped seek moves it 5 s a frame and
/// the messages in between are read within that frame, so a kill read then
/// lies up to one step behind it, and the whole step counts. `cl.time` is no
/// help there: under `viewdemo` it counts playback since the demo started
/// and doesn't jump with a seek (47.3 at world 614). Without a demo player
/// (`playdemo`) it is the only clock there is, so it is used then, unmeasured.
fn window() -> Option<(f64, f64)> {
    match crate::demo_seek::clock().filter(|c| c.active) {
        Some(c) if crate::demo_seek::seeking() => {
            Some((c.now - crate::demo_seek::STEP_SECONDS, c.now))
        }
        Some(c) => Some((c.now, c.now)),
        None => crate::engine::engine_client_time().map(|t| (f64::from(t), f64::from(t))),
    }
}

/// Where `player` is in the loaded demo.
fn slot_of(player: Player) -> Option<i32> {
    let hltv = super::is_hltv();
    match player {
        Player::Slot(slot) => Some(slot),
        Player::OwnPov if hltv => None,
        Player::OwnPov => (1..=MAX_SLOT).find(|&s| super::resolve(s, hltv).is_own_pov),
        Player::SteamId(id) => (1..=MAX_SLOT).find(|&s| super::resolve(s, hltv).steam_id == id),
    }
}

/// The handler's step: what to do with this `DeathMsg`. Runs on the engine
/// thread as the message is read; takes no lock.
pub(super) fn apply(rules: &[Rule], bytes: [u8; 3]) -> Outcome {
    let (k, v) = (i32::from(bytes[0]), i32::from(bytes[1]));
    let (killer, victim) = if rules.iter().any(Rule::needs_player_info) {
        let hltv = super::is_hltv();
        (super::resolve(k, hltv), super::resolve(v, hltv))
    } else {
        (Who::slot(k), Who::slot(v))
    };
    let demo = if rules.iter().any(|r| !r.demos.is_empty()) {
        crate::demo_reload::current_demo().map(|d| demo_stem(&d))
    } else {
        None
    };
    let window = if rules.iter().any(|r| r.at.is_some()) {
        window()
    } else {
        None
    };
    let ctx = Context {
        window,
        demo: demo.as_deref(),
    };
    let outcome = decide(rules, bytes, &killer, &victim, &ctx, slot_of);
    let clock = match window {
        Some((lo, hi)) if hi - lo > 0.05 => format!("{lo:.1}..{hi:.1}"),
        Some((lo, _)) => format!("{lo:.1}"),
        None => "unknown".to_string(),
    };
    let line = match outcome {
        Outcome::Keep => return outcome,
        Outcome::Hide(n) => format!("rule {} hid {k} -> {v} (clock {clock})", n + 1),
        Outcome::Rewrite(n, out) => format!(
            "rule {} made {k} -> {v} {} into {} -> {} {} (clock {clock})",
            n + 1,
            WEAPON_SPRITES.get(bytes[2] as usize).unwrap_or(&"?"),
            out[0],
            out[1],
            WEAPON_SPRITES.get(out[2] as usize).unwrap_or(&"?"),
        ),
        Outcome::Unresolved(n, who) => {
            let bit = 1u64 << n.min(63);
            if UNRESOLVED_SAID.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
                crate::commands::console_print(&format!(
                    "{COMMAND} replace {}: {who} isn't in this demo, so the kill feed shows {k} -> {v} as recorded\n",
                    n + 1
                ));
            }
            format!(
                "rule {} matched {k} -> {v} but {who} isn't in this demo",
                n + 1
            )
        }
    };
    unsafe { crate::debug::report(&format!("deathmsg replace: {line}")) };
    outcome
}

fn list_text(rules: &[Rule]) -> String {
    if rules.is_empty() {
        return format!("{COMMAND} replace: no lines\n");
    }
    let mut out = format!("{COMMAND} replace:\n");
    for (n, rule) in rules.iter().enumerate() {
        out.push_str(&format!("  {}  {}\n", n + 1, rule.describe()));
    }
    out
}

/// `replace ...`, `args` being everything after the word `replace`.
pub(super) fn command(args: &[String]) -> String {
    let Ok(mut rules) = RULES.lock() else {
        return format!("{COMMAND} replace: the list is unavailable\n");
    };
    let first = args.first().map(|a| a.to_ascii_lowercase());
    match first.as_deref() {
        None | Some("list") if args.len() <= 1 => list_text(&rules),
        Some("clear") if args.len() == 1 => {
            rules.clear();
            publish(&rules);
            format!("{COMMAND} replace: no lines\n")
        }
        Some("remove") => {
            let n = match args.get(1).map(|a| a.parse::<usize>()) {
                Some(Ok(n)) if args.len() == 2 && (1..=rules.len()).contains(&n) => n,
                _ => {
                    return format!(
                        "{COMMAND} replace remove: give a line number from `{COMMAND} replace list` (1..{})\n",
                        rules.len()
                    );
                }
            };
            let gone = rules.remove(n - 1);
            publish(&rules);
            format!(
                "{COMMAND} replace: removed {n}: {}\n{}",
                gone.describe(),
                list_text(&rules)
            )
        }
        _ => {
            let parsed = match parse(args) {
                Ok(p) => p,
                Err(why) => return format!("{COMMAND} replace: {why}\n"),
            };
            let mut out = String::new();
            for warning in &parsed.warnings {
                out.push_str(&format!("{COMMAND} replace: warning: {warning}\n"));
            }
            // A batch runs its Initial Commands again for every demo, so the
            // same line arriving twice is kept once.
            if let Some(n) = rules.iter().position(|r| *r == parsed.rule) {
                out.push_str(&format!(
                    "{COMMAND} replace: already line {}: {}\n",
                    n + 1,
                    parsed.rule.describe()
                ));
                return out;
            }
            rules.push(parsed.rule);
            publish(&rules);
            drop(rules);
            super::HOOK_WANTED.store(true, Ordering::Relaxed);
            super::install_hook();
            out.push_str(&format!(
                "{COMMAND} replace {}: {}\n",
                count(),
                live().last().map(Rule::describe).unwrap_or_default()
            ));
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    /// The console's own split: `:` is a token of its own.
    fn console(line: &str) -> Vec<String> {
        args(&line.replace(':', " : "))
    }

    fn rule(line: &str) -> Rule {
        parse(&args(line)).unwrap().rule
    }

    fn err(line: &str) -> String {
        parse(&args(line)).unwrap_err()
    }

    const ALEX: u64 = 76_561_197_960_265_728 + 2 * 5_550_002 + 1; // STEAM_0:1:5550002
    const BEN: u64 = 76_561_197_960_265_728 + 2 * 5_550_001; // STEAM_0:0:5550001

    #[test]
    fn parameters_come_in_any_order() {
        let a = rule(
            "killer self victim STEAM_0:0:5550001 at 7:42 in dice-wsod25_r03 with killer STEAM_0:1:5550002 weapon garand",
        );
        let b = rule(
            "in dice-wsod25_r03 at 7:42 victim STEAM_0:0:5550001 killer self with weapon garand killer STEAM_0:1:5550002",
        );
        assert_eq!(a, b);
        assert_eq!(a.killer, Some(Player::OwnPov));
        assert_eq!(a.victim, Some(Player::SteamId(BEN)));
        assert_eq!(a.at, Some(462.0));
        assert_eq!(a.demos, vec!["dice-wsod25_r03".to_string()]);
        assert_eq!(
            a.change,
            Change::Set {
                killer: Some(Player::SteamId(ALEX)),
                victim: None,
                weapon: Some(5),
            }
        );
    }

    #[test]
    fn the_console_split_at_colons_is_put_back() {
        let typed = parse(&console("killer STEAM_0:0:5550001 at 7:42 with victim 3"))
            .unwrap()
            .rule;
        assert_eq!(typed.killer, Some(Player::SteamId(BEN)));
        assert_eq!(typed.at, Some(462.0));
    }

    #[test]
    fn times_are_m_ss_h_mm_ss_or_seconds() {
        assert_eq!(parse_time("7:42"), Some(462.0));
        assert_eq!(parse_time("0:05.5"), Some(5.5));
        assert_eq!(parse_time("1:02:03"), Some(3723.0));
        assert_eq!(parse_time("462"), Some(462.0));
        assert_eq!(parse_time("462.25"), Some(462.25));
        for bad in ["7:72", "-3", "7:", ":30", "a:10", "1:2:3:4", "nan", "inf"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
        assert!(err("killer 3 at 7:72 with hide").contains("m:ss or seconds"));
    }

    #[test]
    fn in_takes_a_list_patterns_and_strips_dem() {
        let r = rule("killer 3 in Greetz1.dem,wsod25_r03_* with hide");
        assert_eq!(
            r.demos,
            vec!["greetz1".to_string(), "wsod25_r03_*".to_string()]
        );
        let spaced = rule("killer 3 in greetz1, wsod25_r03_* with hide");
        assert_eq!(spaced.demos, r.demos);
        let spaced = rule("killer 3 in greetz1 ,wsod25_r03_* with hide");
        assert_eq!(spaced.demos, r.demos);
    }

    #[test]
    fn hide_and_weapon_only() {
        assert_eq!(rule("victim 4 with hide").change, Change::Hide);
        assert_eq!(
            rule("killer 4 with weapon d_garand").change,
            Change::Set {
                killer: None,
                victim: None,
                weapon: Some(5)
            }
        );
        assert_eq!(
            rule("killer 4 with weapon 43").change,
            Change::Set {
                killer: None,
                victim: None,
                weapon: Some(43)
            }
        );
    }

    #[test]
    fn refusals_say_why() {
        assert!(err("at 7:42 with hide").contains("`killer` or the `victim`"));
        assert!(err("in greetz1 with killer 3").contains("`killer` or the `victim`"));
        assert!(err("killer 3 with").contains("nothing after `with`"));
        assert!(err("killer 3").contains("no `with`"));
        assert!(err("killer 3 vicitm 4 with hide").contains("unknown word `vicitm`"));
        assert!(err("killer 3 with hide frob").contains("unknown word `frob`"));
        assert!(err("killer 3 with at 7:42 hide").contains("goes before `with`"));
        assert!(err("killer 3 weapon garand with hide").contains("goes after `with`"));
        assert!(err("killer 3 killer 4 with hide").contains("twice"));
        assert!(err("killer 3 with weapon garand weapon kar").contains("twice"));
        assert!(err("killer 3 with hide with hide").contains("twice"));
        assert!(err("killer 3 with hide weapon garand").contains("alone"));
        assert!(err("killer bob with hide").contains("slot number, a SteamID or self"));
        assert!(err("killer 40 with hide").contains("out of range"));
        assert!(err("killer 3 with weapon rocket").contains("no weapon called"));
        assert!(err("killer").contains("needs a value"));
        assert!(err("killer 3 in with hide").contains("`in` needs a value"));
        assert!(err("killer 3 in , with hide").contains("needs a demo name"));
    }

    #[test]
    fn self_or_a_slot_without_in_warns_but_is_kept() {
        let p = parse(&args("killer self victim 4 with killer 7")).unwrap();
        assert_eq!(p.warnings.len(), 3, "{:?}", p.warnings);
        assert!(p.warnings[0].starts_with("self is a different player in each POV demo"));
        assert!(p.warnings[1].starts_with("slot 4 "));
        assert!(p.warnings[2].starts_with("slot 7 "));
        let p = parse(&args("killer self in greetz1 with killer 7")).unwrap();
        assert!(p.warnings.is_empty());
        let p = parse(&args("killer 76561197971365756 with hide")).unwrap();
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn list_shows_the_line_in_usage_order() {
        let r = rule("in a,b* at 462.5 victim self with weapon 15 killer STEAM_0:1:5550002");
        assert_eq!(
            r.describe(),
            "victim self at 7:42.5 in a,b* with killer STEAM_0:1:5550002 weapon 15"
        );
        assert_eq!(
            rule("killer 3 with weapon 5").describe(),
            "killer 3 with weapon garand"
        );
        assert_eq!(
            rule("killer 3 at 1:02:03 with hide").describe(),
            "killer 3 at 1:02:03 with hide"
        );
    }

    #[test]
    fn globs() {
        assert!(glob("greetz1", "greetz1"));
        assert!(!glob("greetz1", "greetz10"));
        assert!(glob("wsod25_r03_*", "wsod25_r03_alex"));
        assert!(glob("*greetz*", "solifotw-greetz1"));
        assert!(glob("g?eetz1", "greetz1"));
        assert!(glob("*", ""));
        assert!(!glob("a*b", "acd"));
        assert!(glob("a*b*c", "aXbYbZc"));
        assert_eq!(
            demo_stem("../../Half-Life - PRE/dod/Solifotw-Greetz1.DEM"),
            "solifotw-greetz1"
        );
        assert_eq!(demo_stem("demos\\x.dem"), "x");
    }

    fn who(slot: i32, steam_id: u64, own: bool) -> Who {
        Who {
            slot,
            steam_id,
            is_own_pov: own,
        }
    }

    /// A POV demo: slot 8 is the recorder, slot 3 is Ben, slot 5 is Alex.
    fn pov_slot_of(p: Player) -> Option<i32> {
        match p {
            Player::Slot(s) => Some(s),
            Player::OwnPov => Some(8),
            Player::SteamId(id) if id == BEN => Some(3),
            Player::SteamId(id) if id == ALEX => Some(5),
            Player::SteamId(_) => None,
        }
    }

    fn at(t: f64, demo: Option<&str>) -> Context<'_> {
        Context {
            window: Some((t, t)),
            demo,
        }
    }

    #[test]
    fn the_stolen_kill_becomes_the_teammates() {
        let rules = vec![rule(
            "killer self victim STEAM_0:0:5550001 at 7:42 in solifotw-greetz1 with killer STEAM_0:1:5550002",
        )];
        let (me, ben) = (who(8, 1, true), who(3, BEN, false));
        let ctx = at(463.5, Some("solifotw-greetz1"));
        assert_eq!(
            decide(&rules, [8, 3, 5], &me, &ben, &ctx, pov_slot_of),
            Outcome::Rewrite(0, [5, 3, 5])
        );
        // Outside the 2 s window, in another demo, or another victim: kept.
        let late = at(464.5, Some("solifotw-greetz1"));
        assert_eq!(
            decide(&rules, [8, 3, 5], &me, &ben, &late, pov_slot_of),
            Outcome::Keep
        );
        let other = at(462.0, Some("solifotw-greetz2"));
        assert_eq!(
            decide(&rules, [8, 3, 5], &me, &ben, &other, pov_slot_of),
            Outcome::Keep
        );
        let none = at(462.0, None);
        assert_eq!(
            decide(&rules, [8, 3, 5], &me, &ben, &none, pov_slot_of),
            Outcome::Keep
        );
        let someone = who(4, 77, false);
        assert_eq!(
            decide(&rules, [8, 4, 5], &me, &someone, &ctx, pov_slot_of),
            Outcome::Keep
        );
        // No clock: a rule with `at` can't match.
        let no_clock = Context {
            window: None,
            demo: Some("solifotw-greetz1"),
        };
        assert_eq!(
            decide(&rules, [8, 3, 5], &me, &ben, &no_clock, pov_slot_of),
            Outcome::Keep
        );
    }

    #[test]
    fn a_kill_read_during_a_seek_step_matches_within_the_span() {
        let rules = vec![rule("killer 8 at 100 with hide")];
        let span = Context {
            window: Some((95.0, 100.0 + 3.0)),
            demo: None,
        };
        let k = who(8, 0, false);
        let v = who(2, 0, false);
        assert_eq!(
            decide(&rules, [8, 2, 1], &k, &v, &span, pov_slot_of),
            Outcome::Hide(0)
        );
        let before = Context {
            window: Some((85.0, 97.9)),
            demo: None,
        };
        assert_eq!(
            decide(&rules, [8, 2, 1], &k, &v, &before, pov_slot_of),
            Outcome::Keep
        );
    }

    #[test]
    fn parts_left_out_stay_and_the_first_match_wins() {
        let rules = vec![
            rule("victim 2 with weapon kar"),
            rule("victim 2 with hide"),
            rule("killer 9 with victim 4"),
        ];
        let ctx = Context::default();
        let (k, v) = (who(8, 0, false), who(2, 0, false));
        assert_eq!(
            decide(&rules, [8, 2, 5], &k, &v, &ctx, pov_slot_of),
            Outcome::Rewrite(0, [8, 2, 10])
        );
        let (k, v) = (who(9, 0, false), who(3, 0, false));
        assert_eq!(
            decide(&rules, [9, 3, 5], &k, &v, &ctx, pov_slot_of),
            Outcome::Rewrite(2, [9, 4, 5])
        );
    }

    #[test]
    fn a_new_player_missing_from_the_demo_leaves_the_line() {
        let rules = vec![rule("killer 8 with killer STEAM_0:1:1")];
        let (k, v) = (who(8, 0, false), who(2, 0, false));
        assert!(matches!(
            decide(&rules, [8, 2, 5], &k, &v, &Context::default(), pov_slot_of),
            Outcome::Unresolved(0, Player::SteamId(_))
        ));
    }

    #[test]
    fn a_steam_id_matches_whatever_slot_the_player_has() {
        let rules = vec![rule("victim STEAM_0:0:5550001 with hide")];
        let k = who(8, 1, true);
        for slot in [3, 17] {
            let v = who(slot, BEN, false);
            assert_eq!(
                decide(
                    &rules,
                    [8, slot as u8, 5],
                    &k,
                    &v,
                    &Context::default(),
                    pov_slot_of
                ),
                Outcome::Hide(0)
            );
        }
    }

    #[test]
    fn the_same_line_twice_parses_equal() {
        // `command` keeps one copy of a line a batch sends again per demo.
        assert_eq!(
            rule("killer 3 at 7:42 with hide"),
            rule("at 462 killer 3 with hide")
        );
    }

    #[test]
    fn clock_text_round_trips_through_parse_time() {
        for t in [0.0, 5.5, 462.0, 3723.0, 59.9] {
            assert_eq!(parse_time(&clock_text(t)), Some(t), "{t}");
        }
    }
}
