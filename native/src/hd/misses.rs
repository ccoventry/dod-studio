//! The HD page's misses view (#372 part 3): the last miss list the game
//! wrote to the hook log, read back and grouped by map.
//!
//! `dodstudio_debug_hd_misses`, typed in the game's console, prints every
//! texture that kept its original this session and why, map by map
//! (`texture_hires.rs`'s `misses_report`). It also writes the same lines to
//! the hook log, `<logs>\dodstudio_goldsrc_hooks_<date>.log`, under a
//! `texture_hires: dodstudio_debug_hd_misses --` line and up to a blank line.
//! That block is what this module reads; the game keeps the list in memory
//! only, so there is nothing to read until the command has been typed.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The console command whose output this reads.
pub const MISSES_COMMAND: &str = "dodstudio_debug_hd_misses";

/// The hook's log files: `dodstudio_goldsrc_hooks_YYYYMMDD.log`
/// (`goldsrc-hooks/src/debug.rs`'s `LOG_FILE_PREFIX`).
const LOG_PREFIX: &str = "dodstudio_goldsrc_hooks_";
const LOG_SUFFIX: &str = ".log";

/// Why a texture kept its original: `texture_hires.rs`'s `Miss`, by the
/// heading it prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissReason {
    WrongVersion,
    NoFile,
    Failed,
    OnPurpose,
    /// A heading this copy of the app doesn't know (a newer hook).
    Other,
}

/// `Miss::heading` for each reason, in the hook's order.
const HEADINGS: [(&str, MissReason); 4] = [
    (
        "HD file is for a different version of the texture",
        MissReason::WrongVersion,
    ),
    ("no HD file", MissReason::NoFile),
    ("HD file found but not usable", MissReason::Failed),
    ("left alone on purpose", MissReason::OnPurpose),
];

/// One texture that kept its original.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissEntry {
    /// `world`, `model`, `sprite`, `detail` or `sky`.
    pub asset_type: String,
    /// A world texture's name, `<model> <skin>`, a sprite's path, a file.
    pub name: String,
    /// What the hook said about it, e.g. `128x128, tool texture, never built`.
    pub detail: String,
    /// How many loads hit it (a sprite's frames count separately).
    pub loads: u32,
    /// The other maps it is listed under.
    pub also_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissGroup {
    pub reason: MissReason,
    /// The hook's own heading, shown as is.
    pub heading: String,
    pub entries: Vec<MissEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapMisses {
    pub map: String,
    pub total: usize,
    pub on_purpose: usize,
    pub groups: Vec<MissGroup>,
}

/// The newest miss list in the log.
#[derive(Debug, Clone, Serialize)]
pub struct MissReport {
    pub log_file: String,
    /// `YYYY-MM-DD`, from the log file's name.
    pub date: String,
    /// `hh:mm:ss`, when the command was typed.
    pub time: String,
    /// The list's first line: the totals and the style.
    pub summary: String,
    /// The style the game was using, when the summary names it.
    pub style: Option<String>,
    pub maps: Vec<MapMisses>,
}

/// What the misses view shows: the command to type, and the newest list.
#[derive(Debug, Clone, Serialize)]
pub struct MissesView {
    pub command: &'static str,
    pub report: Option<MissReport>,
}

/// The misses view for the hook logs in `log_dir`.
pub fn view(log_dir: &Path) -> MissesView {
    MissesView {
        command: MISSES_COMMAND,
        report: latest(log_dir),
    }
}

/// The hook's log files in `log_dir`, newest first.
fn hook_logs(log_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return Vec::new();
    };
    let mut logs: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let date = name.strip_prefix(LOG_PREFIX)?.strip_suffix(LOG_SUFFIX)?;
            (date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()))
                .then(|| (date.to_string(), e.path()))
        })
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    logs.into_iter().map(|(_, path)| path).collect()
}

/// The newest miss list in `log_dir`'s hook logs, or `None` when no log has
/// one (the command was never typed, or its logs have aged out).
pub fn latest(log_dir: &Path) -> Option<MissReport> {
    hook_logs(log_dir).into_iter().find_map(|path| {
        // Read as bytes: a line cut short by a crash can leave invalid UTF-8.
        let bytes = std::fs::read(&path).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        let mut report = parse_last(&text)?;
        report.log_file = path.to_string_lossy().to_string();
        let stem = path.file_stem()?.to_string_lossy().to_string();
        let date = stem.strip_prefix(LOG_PREFIX)?;
        report.date = format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..8]);
        Some(report)
    })
}

/// The last miss list in one log file's text.
pub fn parse_last(log: &str) -> Option<MissReport> {
    let marker = format!("] texture_hires: {MISSES_COMMAND} --");
    let lines: Vec<&str> = log.lines().collect();
    let start = lines
        .iter()
        .rposition(|l| l.trim_end().ends_with(&marker))?;
    // "[17:24:31.261] [demo   548.425] [dodstudio_goldsrc_hooks] texture_hires: ..."
    let time = lines[start]
        .strip_prefix('[')
        .and_then(|l| l.get(..8))
        .unwrap_or_default()
        .to_string();
    let body: Vec<&str> = lines[start + 1..]
        .iter()
        .map(|l| l.trim_end())
        .take_while(|l| !l.is_empty() && !l.starts_with('['))
        .collect();
    let (summary, rest) = body.split_first()?;
    let summary = summary
        .strip_prefix(MISSES_COMMAND)
        .and_then(|s| s.strip_prefix(": "))
        .unwrap_or(summary)
        .to_string();
    Some(MissReport {
        log_file: String::new(),
        date: String::new(),
        time,
        style: style_of(&summary),
        summary,
        maps: parse_maps(rest),
    })
}

/// `... (style "plain"). ...` -> `plain`.
fn style_of(summary: &str) -> Option<String> {
    let rest = &summary[summary.find("(style \"")? + 8..];
    Some(rest[..rest.find('"')?].to_string())
}

fn parse_maps(lines: &[&str]) -> Vec<MapMisses> {
    let mut maps: Vec<MapMisses> = Vec::new();
    for line in lines {
        // "== dod_harrington: 21 (21 on purpose, 21 shared with other maps) =="
        if let Some(head) = line.strip_prefix("== ").and_then(|l| l.strip_suffix(" ==")) {
            let Some((map, counts)) = head.rsplit_once(": ") else {
                continue;
            };
            let mut numbers = counts
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|n| n.parse::<usize>().ok());
            maps.push(MapMisses {
                map: map.to_string(),
                total: numbers.next().unwrap_or(0),
                on_purpose: numbers.next().unwrap_or(0),
                groups: Vec::new(),
            });
        } else if let Some(head) = line.strip_prefix("-- ").and_then(|l| l.strip_suffix(':')) {
            // "-- left alone on purpose (21):"
            let heading = head
                .rsplit_once(" (")
                .map_or(head, |(heading, _)| heading)
                .to_string();
            let reason = HEADINGS
                .iter()
                .find(|(h, _)| *h == heading)
                .map_or(MissReason::Other, |&(_, r)| r);
            if let Some(map) = maps.last_mut() {
                map.groups.push(MissGroup {
                    reason,
                    heading,
                    entries: Vec::new(),
                });
            }
        } else if let Some(entry) = parse_entry(line)
            && let Some(group) = maps.last_mut().and_then(|m| m.groups.last_mut())
        {
            group.entries.push(entry);
        }
    }
    maps
}

/// `"  {kind:<6} {name}  {detail}{loads}{also}"`, as `misses_report` writes
/// it: two spaces end the name, which can hold one (`<model> <skin>`).
fn parse_entry(line: &str) -> Option<MissEntry> {
    let line = line.strip_prefix("  ")?;
    let (asset_type, rest) = line.split_once(' ')?;
    let (name, mut detail) = rest.trim_start().split_once("  ")?;
    let mut also_on = Vec::new();
    if let Some(at) = detail.rfind(" [also on ")
        && detail.ends_with(']')
    {
        also_on = detail[at + 10..detail.len() - 1]
            .split(", ")
            .map(str::to_string)
            .collect();
        detail = &detail[..at];
    }
    let mut loads = 1;
    for suffix in [" frame loads)", " loads)"] {
        if let Some(head) = detail.strip_suffix(suffix)
            && let Some((before, n)) = head.rsplit_once(" (")
            && let Ok(n) = n.parse()
        {
            loads = n;
            detail = before;
            break;
        }
    }
    Some(MissEntry {
        asset_type: asset_type.to_string(),
        name: name.to_string(),
        detail: detail.to_string(),
        loads,
        also_on,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// Two lists in one log, as the hook writes them: the second one wins.
    const LOG: &str = "\
[17:24:31.261] [demo   548.425] [dodstudio_goldsrc_hooks] texture_hires: dodstudio_debug_hd_misses --
dodstudio_debug_hd_misses: 1 miss(es) this session, 1 different texture(s) (style \"ultrasharp\"). A texture several maps use is listed under each of them.
== dod_railroad2_test: 1 (1 on purpose, 0 shared with other maps) ==
-- left alone on purpose (1):
  world  black  32x32, tool texture, never built

[17:24:35.718] [demo   552.884] [dodstudio_goldsrc_hooks] commands: dodstudio_debug_status --
dodstudio_hide_scoreboard = 0 -- the scoreboard behaves normally

[22:05:57.463] [demo   828.700] [dodstudio_goldsrc_hooks] texture_hires: dodstudio_debug_hd_misses --
dodstudio_debug_hd_misses: 5 miss(es) this session, 4 different texture(s) (style \"plain\"). A texture several maps use is listed under each of them.
== dod_anzio: 3 (1 on purpose, 1 shared with other maps) ==
-- HD file is for a different version of the texture (1):
  world  bido_wall1  128x128, built from other pixels (2 loads)
-- no HD file (1):
  model  models/v_garand.mdl garand.bmp  256x128, not built yet
-- left alone on purpose (1):
  sprite sprites/puff.spr  256x256, blank (fully transparent), nothing to upscale (4 frame loads) [also on dod_caen]
== dod_caen: 2 (1 on purpose, 1 shared with other maps) ==
-- left alone on purpose (1):
  sprite sprites/puff.spr  256x256, blank (fully transparent), nothing to upscale (4 frame loads) [also on dod_anzio]
-- a heading from a newer hook (1):
  sky    gfx/env/caenbk.tga  no HD face
  ...and 3 more not recorded (list full)

[22:06:01.000] [dodstudio_goldsrc_hooks] something else
";

    #[test]
    fn the_last_list_is_read_map_by_map() {
        let report = parse_last(LOG).unwrap();
        assert_eq!(report.time, "22:05:57");
        assert_eq!(report.style.as_deref(), Some("plain"));
        assert!(report.summary.starts_with("5 miss(es) this session"));
        let maps: Vec<_> = report.maps.iter().map(|m| m.map.as_str()).collect();
        assert_eq!(maps, ["dod_anzio", "dod_caen"]);

        let anzio = &report.maps[0];
        assert_eq!((anzio.total, anzio.on_purpose), (3, 1));
        let reasons: Vec<_> = anzio.groups.iter().map(|g| g.reason).collect();
        assert_eq!(
            reasons,
            [
                MissReason::WrongVersion,
                MissReason::NoFile,
                MissReason::OnPurpose
            ]
        );
        assert_eq!(
            anzio.groups[0].entries[0],
            MissEntry {
                asset_type: "world".into(),
                name: "bido_wall1".into(),
                detail: "128x128, built from other pixels".into(),
                loads: 2,
                also_on: vec![],
            }
        );
        // A skin's name keeps its one space.
        assert_eq!(
            anzio.groups[1].entries[0].name,
            "models/v_garand.mdl garand.bmp"
        );
        let puff = &anzio.groups[2].entries[0];
        assert_eq!(puff.loads, 4);
        assert_eq!(puff.also_on, ["dod_caen"]);
        assert_eq!(
            puff.detail,
            "256x256, blank (fully transparent), nothing to upscale"
        );

        // An unknown heading is kept, and the list-full line isn't an entry.
        let caen = &report.maps[1];
        assert_eq!(caen.groups[1].reason, MissReason::Other);
        assert_eq!(caen.groups[1].heading, "a heading from a newer hook");
        assert_eq!(caen.groups[1].entries.len(), 1);
        assert_eq!(caen.groups[1].entries[0].asset_type, "sky");
    }

    #[test]
    fn a_list_with_nothing_missed_has_no_maps() {
        let log = "[10:00:00.000] [dodstudio_goldsrc_hooks] texture_hires: dodstudio_debug_hd_misses --\n\
                   dodstudio_debug_hd_misses: every HD-eligible texture loaded this session was replaced (style \"plain\")\n\n";
        let report = parse_last(log).unwrap();
        assert!(report.maps.is_empty());
        assert!(report.summary.starts_with("every HD-eligible texture"));
        assert_eq!(report.style.as_deref(), Some("plain"));
        assert!(parse_last("[10:00:00.000] nothing here\n").is_none());
    }

    #[test]
    fn the_newest_log_with_a_list_is_used() {
        let dir = Scratch::new("hd_misses_logs");
        let with = LOG.lines().take(6).collect::<Vec<_>>().join("\n");
        std::fs::write(dir.join("dodstudio_goldsrc_hooks_20260923.log"), LOG).unwrap();
        std::fs::write(dir.join("dodstudio_goldsrc_hooks_20260924.log"), with).unwrap();
        // Newer, but no list in it; and a file that isn't a hook log.
        std::fs::write(
            dir.join("dodstudio_goldsrc_hooks_20260925.log"),
            "[09:00:00.000] hello\n",
        )
        .unwrap();
        std::fs::write(dir.join("activity_20260926.md"), LOG).unwrap();

        let report = latest(&dir).unwrap();
        assert_eq!(report.date, "2026-09-24");
        assert_eq!(report.style.as_deref(), Some("ultrasharp"));
        assert!(
            report
                .log_file
                .ends_with("dodstudio_goldsrc_hooks_20260924.log")
        );
        assert!(latest(&dir.join("nowhere")).is_none());
    }

    /// The text read here is the hook's; it can't be imported (a cdylib), so
    /// its source is read instead, and a change there fails here.
    #[test]
    fn the_format_matches_the_hook() {
        let hook = include_str!("../../../goldsrc-hooks/src/texture_hires.rs");
        assert!(hook.contains("console_name!(\"debug_hd_misses\")"));
        assert_eq!(MISSES_COMMAND, "dodstudio_debug_hd_misses");
        assert!(hook.contains("\"texture_hires: {MISSES_NAME} --\\n{}\""));
        for (heading, _) in HEADINGS {
            assert!(hook.contains(&format!("=> \"{heading}\"")), "{heading}");
        }
        for format in [
            "\"== {map}: {} ({} on purpose, {} shared with other maps) ==\\n\"",
            "\"-- {} ({n}):\\n\"",
            "\"  {kind:<6} {name}  {}{loads}{also}\\n\"",
            "\" [also on {}]\"",
            "\" ({n} frame loads)\"",
            "\" ({n} loads)\"",
            "(style {style:?})",
        ] {
            assert!(hook.contains(format), "{format}");
        }
        let debug = include_str!("../../../goldsrc-hooks/src/debug.rs");
        assert!(debug.contains(&format!("LOG_FILE_PREFIX: &str = \"{LOG_PREFIX}\"")));
        assert!(debug.contains(&format!("LOG_FILE_SUFFIX: &str = \"{LOG_SUFFIX}\"")));
    }
}
