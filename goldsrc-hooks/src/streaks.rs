//! The DoD Studio window's Killstreaks tab (#565): every kill streak in the
//! demo that is playing, found by the same analysis Studio runs.
//!
//! ## Where the analysis comes from
//!
//! The analyzer cache Studio shares (`analysis::cache`, under
//! `%APPDATA%\dod-studio\analyzer_cache`): a demo Studio's Demo Analyzer or
//! Master Queue scan already read, or one shown here before, is there and
//! takes a few milliseconds. Anything else is analysed here, on a thread of its
//! own at below-normal priority, and saved there, for Studio and the next time.
//!
//! Only on request (the tab asks while it is showing), never because a demo
//! started: captures play demos too, and an analysis would take CPU from the
//! recording.
//!
//! ## Times
//!
//! A streak's time is its first kill's on the `viewdemo` bar, the units
//! `dodstudio_seek_to` takes (`analysis`' `viewdemo_offset`).

// Only the 32-bit build has the window; a host check still compiles this.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Every life with a kill is listed; the tab's Min kills box narrows the
/// list, so nothing is hidden by a number fixed here.
pub const MIN_KILLS: usize = 1;
/// Go jumps this long before a streak's first kill.
pub const LEAD_IN_SECS: f32 = 5.0;

/// One row of the Killstreaks tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Streak {
    pub player: String,
    pub kills: usize,
    /// The weapons used, each once, in the order first used.
    pub weapons: String,
    /// The first kill's time on the `viewdemo` bar, in seconds.
    pub first_kill: f32,
    /// The last kill's, the same way.
    pub last_kill: f32,
    /// Each kill's weapon, and from the second on the time since the one
    /// before: "K98, (+0:04) K98, (+0:12) Grenade", the Capture page's
    /// Details column (`CaptureStreak::update_visuals`).
    pub details: String,
    /// The player's number (entity index, what `dodstudio_spec_target`
    /// takes), when they were still in the game at the demo's end.
    pub player_number: Option<u8>,
}

impl Streak {
    /// A streak from its player and kills (bar time, weapon), or `None` when
    /// it is too short to list.
    pub fn new(player: &str, kills: &[(f32, String)]) -> Option<Self> {
        if kills.len() < MIN_KILLS {
            return None;
        }
        let mut weapons: Vec<&str> = Vec::new();
        for (_, weapon) in kills {
            if !weapons.contains(&weapon.as_str()) {
                weapons.push(weapon);
            }
        }
        let mut in_order: Vec<&(f32, String)> = kills.iter().collect();
        in_order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let details = in_order
            .iter()
            .enumerate()
            .map(|(i, (time, weapon))| match i {
                0 => weapon.clone(),
                _ => format!("(+{}) {weapon}", gap_text(*time - in_order[i - 1].0)),
            })
            .collect::<Vec<_>>()
            .join(", ");
        Some(Self {
            player: player.to_string(),
            kills: kills.len(),
            weapons: weapons.join(", "),
            first_kill: in_order[0].0,
            last_kill: in_order[in_order.len() - 1].0,
            details,
            player_number: None,
        })
    }

    /// From the first kill to the last, as the Capture page's Dur. column
    /// shows it.
    pub fn duration_text(&self) -> String {
        gap_text(self.last_kill - self.first_kill)
    }

    /// Where Go jumps to: a little before the first kill.
    pub fn seek_secs(&self) -> f32 {
        (self.first_kill - LEAD_IN_SECS).max(0.0)
    }
}

/// A demo's streaks, and what kind of demo it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub streaks: Vec<Streak>,
    /// An HLTV demo: every player's streaks, and Go puts the camera on the
    /// player.
    pub hltv: bool,
    /// A POV demo's recording player, whose streaks alone are listed.
    pub recorder: Option<String>,
}

/// One player's kills, as [`select`] takes them.
pub struct PlayerKills {
    pub name: String,
    pub number: Option<u8>,
    pub recorder: bool,
    /// Each streak's kills: (bar time, weapon).
    pub streaks: Vec<Vec<(f32, String)>>,
}

/// The streaks to list, earliest first: in a POV demo only the recording
/// player's (the only ones its footage follows), in an HLTV demo
/// everyone's. A POV demo whose recorder can't be told lists everyone's.
pub fn select(players: Vec<PlayerKills>, hltv: bool) -> Found {
    let recorder = (!hltv)
        .then(|| players.iter().find(|p| p.recorder).map(|p| p.name.clone()))
        .flatten();
    let mut streaks: Vec<Streak> = players
        .iter()
        .filter(|p| recorder.is_none() || p.recorder)
        .flat_map(|p| {
            p.streaks.iter().filter_map(|kills| {
                Streak::new(&p.name, kills).map(|s| Streak {
                    player_number: p.number,
                    ..s
                })
            })
        })
        .collect();
    streaks.sort_by(|a, b| a.first_kill.total_cmp(&b.first_kill));
    Found {
        streaks,
        hltv,
        recorder,
    }
}

/// Every listable streak in `analysis` ([`select`]).
pub fn streaks_of(analysis: &analysis::Analysis) -> Found {
    let hltv = analysis.demo_info.demo_type == "HLTV";
    let recorder_slot = analysis.state.pov_player_index;
    let players = analysis
        .state
        .players
        .iter()
        .map(|player| {
            let slot = match player.connection {
                analysis::Connection::Connected { client_id } => Some(client_id),
                _ => None,
            };
            PlayerKills {
                name: player.name.clone(),
                // The analysis counts slots from 0, entity numbers from 1.
                number: slot.and_then(|s| s.checked_add(1)),
                recorder: !hltv && slot.is_some() && slot == recorder_slot,
                streaks: player
                    .kill_streaks
                    .iter()
                    .map(|streak| {
                        streak
                            .kills
                            .iter()
                            .map(|(time, weapon, _)| {
                                (time.viewdemo_offset.as_secs_f32(), weapon_name(weapon))
                            })
                            .collect()
                    })
                    .collect(),
            }
        })
        .collect();
    select(players, hltv)
}

/// Studio's English weapon names. `analysis::weapon_display_name` reads them
/// from a `localizations` folder it looks for beside the running program,
/// which in the game is `hl.exe`'s: there is none there.
const WEAPON_NAMES: &str = include_str!("../../localizations/dod_studio_english.txt");

/// A weapon's name as Studio shows it (`"weapon.<name>"` keys are the
/// weapon's own name in lower case), or that name itself.
fn weapon_name(weapon: &analysis::Weapon) -> String {
    let own = format!("{weapon:?}");
    let key = format!("\"weapon.{}\"", own.to_ascii_lowercase());
    WEAPON_NAMES
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix(&key)?;
            Some(rest.trim().trim_matches('"').to_string())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or(own)
}

/// A span of time as `m:ss`, rounded to the second, as the Capture page
/// shows its gaps and durations.
fn gap_text(secs: f32) -> String {
    let secs = secs.max(0.0).round() as u64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// A highlight's row number as the tab shows it: right-aligned, so it sorts
/// as text in number order.
pub fn row_text(row: usize) -> String {
    format!("{row:>4}")
}

/// A bar time as the tab shows it, `mm:ss`: sorts as text in time order for
/// any demo under 100 minutes.
pub fn time_text(secs: f32) -> String {
    let secs = secs.max(0.0) as u64;
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// A kill count as the tab shows it: right-aligned, so it sorts as text in
/// number order.
pub fn kills_text(kills: usize) -> String {
    format!("{kills:>3}")
}

/// Where the game finds a demo it was given by name: relative to the game
/// folder, `.dem` added when missing.
pub fn demo_path(game_dir: &Path, name: &str) -> PathBuf {
    let name = name.trim().trim_matches('"');
    let has_extension = Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dem"));
    if has_extension {
        game_dir.join(name)
    } else {
        game_dir.join(format!("{name}.dem"))
    }
}

/// What the tab shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Being read; how far is [`progress`].
    Loading,
    Ready(Found),
    Failed(String),
}

struct Job {
    path: PathBuf,
    generation: u64,
    status: Status,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);
/// Bumped for every new demo, so a slower analysis of an older one never
/// replaces a newer one's result.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The current analysis' progress, 0 to 100.
static PERCENT: AtomicU32 = AtomicU32::new(0);

/// Starts finding `path`'s streaks, unless they are being found or were
/// found already.
pub fn request(path: &Path) {
    let mut job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    if job.as_ref().is_some_and(|j| j.path == path) {
        return;
    }
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    PERCENT.store(0, Ordering::Release);
    *job = Some(Job {
        path: path.to_path_buf(),
        generation,
        status: Status::Loading,
    });
    let path = path.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("dodstudio-streaks".into())
        .spawn(move || {
            lower_priority();
            let started = std::time::Instant::now();
            let status = match find(&path, generation) {
                Ok((found, how)) => {
                    log(&format!(
                        "{} streaks in {} ({how}, {:.1} s)",
                        found.streaks.len(),
                        path.display(),
                        started.elapsed().as_secs_f32()
                    ));
                    Status::Ready(found)
                }
                Err(why) => {
                    log(&format!("{}: {why}", path.display()));
                    Status::Failed(why)
                }
            };
            let mut job = JOB.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(job) = job.as_mut().filter(|j| j.generation == generation) {
                job.status = status;
            }
        });
    if let Err(why) = spawned
        && let Some(job) = job.as_mut()
    {
        job.status = Status::Failed(format!("could not start the analysis: {why}"));
    }
}

/// The current request's generation, status and progress (0 to 100), or
/// `None` with nothing requested or while the worker is saving its result
/// (never waits: this runs every frame).
pub fn status() -> Option<(u64, Status, u32)> {
    let job = JOB.try_lock().ok()?;
    let job = job.as_ref()?;
    Some((
        job.generation,
        job.status.clone(),
        PERCENT.load(Ordering::Acquire),
    ))
}

/// The streaks, and where they came from (for the log).
fn find(path: &Path, generation: u64) -> Result<(Found, &'static str), String> {
    let root = cache_root();
    if let Some((_, analysis)) = root
        .as_deref()
        .and_then(|root| analysis::cache::load(root, path))
    {
        return Ok((streaks_of(&analysis), "from the analyzer cache"));
    }
    // DoD Studio, when it runs, analyses it in its own (64-bit) process.
    match ask_studio(path, generation) {
        Some(Ok(())) => {
            return root
                .as_deref()
                .and_then(|root| analysis::cache::load(root, path))
                .map(|(_, analysis)| (streaks_of(&analysis), "analysed by DoD Studio"))
                .ok_or_else(|| {
                    "DoD Studio read it, but its result is not in the cache".to_string()
                });
        }
        Some(Err(why)) => return Err(format!("DoD Studio could not read it: {why}")),
        None => {}
    }
    let size = std::fs::metadata(path)
        .map_err(|e| format!("could not read the demo: {e}"))?
        .len();
    let need = memory_needed(size);
    let block = block_needed(size);
    if let Some((free, largest)) = free_address_space()
        && (free < need || largest < block)
    {
        log(&format!(
            "{}: not analysed in the game -- it needs about {} MB with one {} MB block, the game has {} MB free, {} MB at most in one block",
            path.display(),
            need >> 20,
            block >> 20,
            free >> 20,
            largest >> 20
        ));
        return Err(
            "too big to read inside the game. Open it once in DoD Studio's Demo Analyzer, and it shows here"
                .to_string(),
        );
    }
    log(&format!("analysing {}", path.display()));
    let bytes = std::fs::read(path).map_err(|e| format!("could not read the demo: {e}"))?;
    let analysis = analysis::Analysis::try_from_bytes_with_progress(&bytes, |done, total| {
        if total > 0 && GENERATION.load(Ordering::Acquire) == generation {
            // u64: `done * 100` passes 2^32 on this 32-bit build.
            let percent = (done as u64 * 100 / total as u64).min(100);
            PERCENT.store(percent as u32, Ordering::Release);
        }
    })?;
    drop(bytes);
    let saved = root.as_deref().and_then(|root| {
        let info = analysis::cache::FileInfo::of(path).ok()?;
        analysis::cache::store(root, path, &info, &analysis)
    });
    Ok((
        streaks_of(&analysis),
        if saved.is_some() {
            "analysed, saved to the analyzer cache"
        } else {
            "analysed, not saved"
        },
    ))
}

/// DoD Studio's analysis pipe; must match `native::sys::analysis_server::
/// PIPE_NAME` to the character.
const STUDIO_PIPE: &str = r"\\.\pipe\dodstudio-analyzer";

/// Asks DoD Studio to analyse `path` into the analyzer cache, following its
/// progress. `None` when Studio isn't running (nothing serves the pipe).
fn ask_studio(path: &Path, generation: u64) -> Option<Result<(), String>> {
    use std::io::{BufRead, BufReader, Write};
    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(STUDIO_PIPE)
        .ok()?;
    log(&format!("asking DoD Studio to analyse {}", path.display()));
    let request = format!("analyze {}\n", path.to_string_lossy());
    if let Err(e) = pipe.write_all(request.as_bytes()) {
        return Some(Err(format!("could not ask: {e}")));
    }
    for line in BufReader::new(pipe).lines() {
        let Ok(line) = line else { break };
        if let Some(percent) = line.strip_prefix("progress ") {
            if let Ok(percent) = percent.trim().parse::<u32>()
                && GENERATION.load(Ordering::Acquire) == generation
            {
                PERCENT.store(percent.min(100), Ordering::Release);
            }
        } else if line == "done" {
            return Some(Ok(()));
        } else if let Some(why) = line.strip_prefix("failed ") {
            return Some(Err(why.to_string()));
        }
    }
    Some(Err("DoD Studio stopped answering".to_string()))
}

/// What analysing a demo of `size` bytes takes at its peak: measured 831 MB
/// for a 72 MB demo in a 32-bit process (11.5 times), plus room for the
/// game's own allocations meanwhile. The game is a 32-bit process with 2 GB
/// of address space (pre-Anniversary `hl.exe` is not large-address-aware) and
/// uses over 1 GB of it, so running out kills it outright (an allocation
/// failure aborts): a demo that does not fit is refused, not tried.
fn memory_needed(size: u64) -> u64 {
    size.saturating_mul(12).saturating_add(256 << 20)
}

/// The largest single allocation analysing a demo of `size` bytes makes,
/// with room to spare: the whole file is read into one buffer, and the
/// parser's own buffers grow by doubling. Measured live: the pre-Anniversary
/// game had 587 MB free but no block over 102 MB, too fragmented for a 117 MB
/// demo whatever the total.
fn block_needed(size: u64) -> u64 {
    size.saturating_mul(2).saturating_add(32 << 20)
}

/// The address space this process has left, and its largest free block.
fn free_address_space() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::Memory::{MEM_FREE, MEMORY_BASIC_INFORMATION, VirtualQuery};
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
    let mut info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
    unsafe { GetSystemInfo(&mut info) };
    let (mut address, end) = (
        info.lpMinimumApplicationAddress as usize,
        info.lpMaximumApplicationAddress as usize,
    );
    let (mut free, mut largest) = (0u64, 0u64);
    while address < end {
        let mut region: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let got = unsafe {
            VirtualQuery(
                address as *const _,
                &mut region,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if got == 0 || region.RegionSize == 0 {
            break;
        }
        if region.State == MEM_FREE {
            free += region.RegionSize as u64;
            largest = largest.max(region.RegionSize as u64);
        }
        address = (region.BaseAddress as usize).saturating_add(region.RegionSize);
    }
    (free > 0).then_some((free, largest))
}

fn log(message: &str) {
    unsafe { crate::debug::report(&format!("streaks: {message}")) };
}

/// `%APPDATA%\dod-studio\analyzer_cache`, where Studio keeps it
/// (`native::shared::paths::get_appdata_dir`, `dirs::config_dir()`).
pub fn cache_root() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(|dir| PathBuf::from(dir).join("dod-studio").join("analyzer_cache"))
}

/// The analysis must not take frames from the game.
fn lower_priority() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kills(list: &[(f32, &str)]) -> Vec<(f32, String)> {
        list.iter().map(|(t, w)| (*t, w.to_string())).collect()
    }

    fn player(name: &str, number: u8, recorder: bool, first: f32) -> PlayerKills {
        PlayerKills {
            name: name.to_string(),
            number: Some(number),
            recorder,
            streaks: vec![kills(&[(first, "K98"), (first + 2.0, "K98")])],
        }
    }

    #[test]
    fn a_pov_demo_lists_only_the_recorders_streaks() {
        let found = select(
            vec![
                player("milo", 3, false, 10.0),
                player("brain", 5, true, 20.0),
            ],
            false,
        );
        assert_eq!(found.recorder.as_deref(), Some("brain"));
        assert_eq!(found.streaks.len(), 1);
        assert_eq!(found.streaks[0].player, "brain");
        assert!(!found.hltv);
    }

    #[test]
    fn an_hltv_demo_lists_everyone_with_their_numbers() {
        let found = select(
            vec![
                player("milo", 3, false, 30.0),
                player("brain", 5, false, 20.0),
            ],
            true,
        );
        assert!(found.hltv && found.recorder.is_none());
        let names: Vec<_> = found
            .streaks
            .iter()
            .map(|s| (s.player.as_str(), s.player_number))
            .collect();
        assert_eq!(names, [("brain", Some(5)), ("milo", Some(3))]);
    }

    #[test]
    fn a_pov_demo_without_a_known_recorder_lists_everyone() {
        let found = select(
            vec![
                player("milo", 3, false, 10.0),
                player("brain", 5, false, 20.0),
            ],
            false,
        );
        assert_eq!(found.streaks.len(), 2);
        assert!(found.recorder.is_none());
    }

    #[test]
    fn a_streak_lists_each_weapon_once_and_starts_at_its_first_kill() {
        let streak = Streak::new(
            "milo",
            &kills(&[(75.0, "K98"), (71.5, "Grenade"), (80.0, "K98")]),
        )
        .unwrap();
        assert_eq!(streak.kills, 3);
        assert_eq!(streak.weapons, "K98, Grenade");
        assert_eq!(streak.details, "Grenade, (+0:04) K98, (+0:05) K98");
        assert_eq!(streak.duration_text(), "0:09");
        assert_eq!(streak.first_kill, 71.5);
        assert_eq!(streak.seek_secs(), 66.5);
        assert_eq!(
            Streak::new("milo", &kills(&[(3.0, "K98")])).unwrap().kills,
            1
        );
        assert_eq!(Streak::new("milo", &[]), None);
        // Never before the demo's start.
        assert_eq!(
            Streak::new("a", &kills(&[(2.0, "K98"), (3.0, "K98")]))
                .unwrap()
                .seek_secs(),
            0.0
        );
    }

    #[test]
    fn free_address_space_is_measured() {
        let (free, largest) = free_address_space().unwrap();
        assert!(largest > 0 && largest <= free);
        assert_eq!(block_needed(100 << 20), 232 << 20);
    }

    #[test]
    fn the_studio_pipe_is_the_one_studio_serves() {
        let server = include_str!("../../native/src/sys/analysis_server.rs");
        assert!(server.contains(&format!("pub const PIPE_NAME: &str = r\"{STUDIO_PIPE}\";")));
    }

    #[test]
    fn weapons_have_studios_names() {
        assert_eq!(weapon_name(&analysis::Weapon::StickGrenade), "Stick");
        assert_eq!(weapon_name(&analysis::Weapon::Mp40), "MP40");
        assert_eq!(weapon_name(&analysis::Weapon::K98), "K98");
    }

    #[test]
    fn times_and_counts_sort_as_text_in_order() {
        assert_eq!(time_text(754.9), "12:34");
        assert_eq!(time_text(-1.0), "00:00");
        assert!(time_text(65.0) < time_text(600.0));
        assert!(kills_text(9) < kills_text(10));
        assert_eq!(kills_text(4), "  4");
    }

    #[test]
    fn a_demo_name_is_found_where_the_game_finds_it() {
        let game = Path::new(r"C:\hl\dod");
        assert_eq!(demo_path(game, "match1"), game.join("match1.dem"));
        assert_eq!(demo_path(game, "match1.DEM"), game.join("match1.DEM"));
        assert_eq!(
            demo_path(game, "\"../other/x.dem\""),
            game.join("../other/x.dem")
        );
    }

    /// A real demo, analysed the way the game does it: run with
    /// `DODSTUDIO_STREAKS_DEMO=<path> cargo test -p goldsrc-hooks --target
    /// i686-pc-windows-msvc -- --ignored --nocapture` to see it fits a 32-bit
    /// process.
    #[test]
    #[ignore]
    fn a_real_demo_is_analysed() {
        let path = std::env::var_os("DODSTUDIO_STREAKS_DEMO").expect("DODSTUDIO_STREAKS_DEMO");
        let bytes = std::fs::read(&path).unwrap();
        let analysis = analysis::Analysis::try_from_bytes(&bytes).unwrap();
        let streaks = streaks_of(&analysis);
        println!(
            "{} streaks, hltv {}, recorder {:?}",
            streaks.streaks.len(),
            streaks.hltv,
            streaks.recorder
        );
        for streak in streaks.streaks.iter().take(5) {
            println!("{streak:?}");
        }
    }
}
