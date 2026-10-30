//! Two settings that silence sounds a map plays (#284), without touching the
//! game's `.wav` files or the demo:
//!
//! - `ds_sound_mute_victory_music`: the music a map plays to everyone when a
//!   team wins the round.
//! - `ds_sound_mute_flag_capture`: the point-captured announcements, and the
//!   objective-secured ones a few maps play the same way (dod_escape's
//!   `usobjectivesecure`).
//!
//! Each is on its own, so a clip can keep one and lose the other.
//!
//! ## Where the sounds come from
//!
//! Both are map data the server plays, so the client only ever sees them as
//! server messages, and neither goes through `EV_PlaySound` (which `voice.rs`
//! and `fire_sounds.rs` hook):
//!
//! - **The victory music** is an `ambient_generic` the round-end wiring
//!   fires (`targetname` `allies` / `axis` / `no_winner`, or a `dod_score_ent`
//!   like dod_vicenza's `allies_win`). It arrives as `svc_spawnstaticsound`;
//!   a trigger sends a start and a `SND_STOP`.
//! - **The capture sound** is a `dod_control_point`'s
//!   `point_allies_capsound` / `point_axis_capsound` (and on a few maps a
//!   `dod_object`'s take/drop sound). It arrives as `svc_sound` on the
//!   point's entity.
//!
//! ## Where this cuts in
//!
//! The two `svc_` handlers hand every sound to `S_StartStaticSound` or
//! `S_StartDynamicSound` (cdecl, `(entnum, channel, sfx, origin, volume,
//! attenuation, flags, pitch)`), at three call sites per build: `svc_sound`'s
//! two (static when the channel is `CHAN_STATIC`, else dynamic) and
//! `svc_spawnstaticsound`'s one. Each call is redirected to [`start_static`]
//! or [`start_dynamic`], which drop a sound the current map names for a family
//! (or that family's fixed list does, see below) while that family's setting
//! is on, and call the engine's function for everything else.
//!
//! Why the call sites rather than a function:
//!
//! - **Not `S_LoadSound`**, the sample cache loader, though returning NULL
//!   there is a path its play-time callers handle. The mixer
//!   (pre-Anniversary `hw.dll+0x8e5c0`) also calls it for every playing
//!   channel on every paint and reads the result without a NULL test, so a
//!   sound already playing when a setting went on would crash it.
//! - **Not `S_StartDynamicSound`'s prologue**: HLAE detours it there while it
//!   records sound (`docs/goldsrc_hw_dll_survey.md` §12.3). A redirected call
//!   still calls the function's own address, so it goes through HLAE's detour
//!   whenever HLAE has one; HLAE patches nothing at these call sites.
//! - Only sounds the server sent pass through here: a client-side sound
//!   (gunshot events, footsteps the client predicts, voice commands) never
//!   does, and a map's own ambience only does when the server sends it.
//!
//! A `SND_STOP` always goes through, so turning a setting on while the music
//! plays still lets its stop arrive. Turning one on or off acts on the next
//! sound the server sends; a sound already playing finishes.
//!
//! ## Which names
//!
//! **The map's own, read as it loads.** Once per map, on the engine thread
//! from the per-frame poll, [`poll`] reads the world model's entity lump out
//! of the engine's memory (`model_t.entities`, `com_model.h`: the copy
//! `Mod_LoadEntities` keeps, NUL-terminated, at `model_t+0x180` in both
//! builds) and runs the same structural selection the demo patcher and the
//! offline survey use (`dod::map_sounds`): the win music the round-end wiring
//! fires, through one relay if need be, and the capture entities' sound
//! keyvalues. So a custom map with its own file names is muted too. The sound
//! hook reads the result through an `RwLock` it never waits on
//! (`try_read`), and the read swaps it whole on the engine thread.
//!
//! **And two fixed lists, always.** The union over the 250 maps in both movie
//! installs of the same selection, `round_win` for [`VICTORY_MUSIC`] and
//! `capture` for [`FLAG_CAPTURE`] (`native/examples/map_sound_table.rs`
//! prints both). A sound is muted when it is in the map's set *or* its
//! family's list, so a map whose lump can't be read (or a build whose
//! `model_t` moved) still gets today's behaviour, and dod_vicenza keeps its
//! victory music muted: it fires `uswin` / `germanwin` through a
//! `dod_score_ent`, which the selection doesn't follow, and the lists carry
//! both from the maps that wire them the usual way. No name is in both lists.
//! Matching on a word like "win" instead would silence `ambience/windy.wav`
//! on 28 maps. dod_escape plays `usobjectivesecure` when a mortar objective
//! is destroyed, the same kind of announcement, so it stays in its list.
//! `ds_debug_log_server_sounds 1` says which matched: `(map)` or `(list)`.
//!
//! `tools/verify_map_sounds_offsets.py` checks the call sites in both builds,
//! `tools/verify_map_entities_offsets.py` the `model_t` offsets.

use std::collections::BTreeSet;
use std::ffi::CStr;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self};
use crate::names::console_name;
use crate::scan;
use crate::setting::Cvar;

pub const VICTORY_MUSIC_NAME: &str = console_name!("sound_mute_victory_music");
pub const FLAG_CAPTURE_NAME: &str = console_name!("sound_mute_flag_capture");
pub const LOG_NAME: &str = console_name!("debug_log_server_sounds");

/// `SND_STOP` in the sound flags (`common/const.h`).
const SND_STOP: i32 = 1 << 5;
/// The longest name the engine keeps (`sfx_t.name`, and `svc_sound`'s
/// sentence buffer).
const MAX_NAME: usize = 64;

/// Which engine function a call site calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Static,
    Dynamic,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Static => "static",
            Kind::Dynamic => "dynamic",
        }
    }
}

/// One call from an `svc_` handler into `S_Start*Sound`.
struct Site {
    what: &'static str,
    kind: Kind,
    /// Unique in its build's `hw.dll`, matching nothing in the other's.
    pattern: &'static str,
    /// Offset of the `E8 rel32` call.
    call_at: usize,
}

struct Build {
    name: &'static str,
    sites: [Site; 3],
    /// `model_t.type` (`modtype_t`, 0 is `mod_brush`): `Mod_LoadBrushModel`
    /// zeroes it first thing.
    model_type_at: usize,
    /// `model_t.entities`: `Mod_LoadEntities` stores its hunk copy of the
    /// entity lump here, or 0 for a map without one.
    model_entities_at: usize,
}

#[rustfmt::skip]
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        sites: [
            // hw+0x1aa4c in svc_sound (hw+0x1a8f0): channel 6 is CHAN_STATIC.
            Site {
                what: "svc_sound's static call",
                kind: Kind::Static,
                //         pitch,flags,attn,vol    push edx ebx eax  lea edx,[ebp-24] push ecx edx esi  push 6  push edi call
                pattern: "8B 55 F8 8B 45 F4 8B 4D F0 52 53 50 8D 55 DC 51 52 56 6A 06 57 E8 ?? ?? ?? ?? 83 C4 20",
                call_at: 21,
            },
            // hw+0x1aa6f, the same handler's other channels.
            Site {
                what: "svc_sound's dynamic call",
                kind: Kind::Dynamic,
                pattern: "8B 4D F8 8B 55 F4 51 8B 4D F0 53 52 8D 55 DC 51 52 56 50 57 E8 ?? ?? ?? ?? 83 C4 20",
                call_at: 20,
            },
            // hw+0x1cb48 in CL_ParseStaticSound (hw+0x1ca90, svc_spawnstaticsound's handler jumps there).
            Site {
                what: "svc_spawnstaticsound's call",
                kind: Kind::Static,
                pattern: "8B 55 FC 8B 45 F8 8B 4D F4 52 53 50 8B 45 F0 8D 55 E0 51 52 56 6A 06 50 E8 ?? ?? ?? ?? 83 C4 20",
                call_at: 24,
            },
        ],
        // Mod_LoadBrushModel hw+0x41bb0, Mod_LoadEntities hw+0x40a20.
        model_type_at: 0x44,
        model_entities_at: 0x180,
    },
    Build {
        name: "25th Anniversary",
        sites: [
            // hw+0x1a5f98 in svc_sound (hw+0x1a5e10).
            Site {
                what: "svc_sound's static call",
                kind: Kind::Static,
                //         cmp ecx,6 jne    lea ecx,[ebp-10] push ecx eax  push 6  push edi call
                pattern: "83 F9 06 75 21 8D 4D F0 51 50 6A 06 57 E8 ?? ?? ?? ?? 83 C4 20",
                call_at: 13,
            },
            // hw+0x1a5fb8, right after the static branch's return.
            Site {
                what: "svc_sound's dynamic call",
                kind: Kind::Dynamic,
                pattern: "8B E5 5D C3 8D 55 F0 52 50 51 57 E8 ?? ?? ?? ?? 83 C4 20",
                call_at: 11,
            },
            // hw+0x1a84e6 in CL_ParseStaticSound (hw+0x1a8410).
            Site {
                what: "svc_spawnstaticsound's call",
                kind: Kind::Static,
                pattern: "8D 45 F0 53 83 EC 08 F3 0F 11 44 24 04 F3 0F 10 45 9C F3 0F 11 04 24 50 56 6A 06 FF 75 98 E8 ?? ?? ?? ?? 8B 4D FC 83 C4 20",
                call_at: 30,
            },
        ],
        // Mod_LoadBrushModel hw+0x23f060, Mod_LoadEntities hw+0x23ff90.
        model_type_at: 0x44,
        model_entities_at: 0x180,
    },
];

/// The victory music ([`VICTORY_MUSIC_NAME`]): what each map's round-end
/// wiring fires. Lowercase with forward slashes, sorted for [`listed_in`]'s
/// binary search.
#[rustfmt::skip]
const VICTORY_MUSIC: &[&str] = &[
    "ambience/adrenaxis.wav",
    "ambience/adrenbrit.wav",
    "ambience/alliesscore.wav",
    "ambience/axisscore.wav",
    "ambience/bluffallieswin.wav",
    "ambience/bluffaxiswin.wav",
    "ambience/bouleallie.wav",
    "ambience/bouleaxis.wav",
    "ambience/britwin.wav",
    "ambience/brwin.wav",
    "ambience/bsl/alliesscore.wav",
    "ambience/bsl/axisscore.wav",
    "ambience/darkside.wav",
    "ambience/derka.wav",
    "ambience/germanwin.wav",
    "ambience/littlegeorf.wav",
    "ambience/no_winner.wav",
    "ambience/plexallieswin.wav",
    "ambience/ramelle-alliesscore.wav",
    "ambience/ramelle-axisscore.wav",
    "ambience/shermanaxis.wav",
    "ambience/uswin.wav",
    "blitzkrieg_snow/ramelle-alliesscore.wav",
    "blitzkrieg_snow/ramelle-axisscore.wav",
    "eric_the_pixie/etp_axis_win.wav",
    "joe_dirt/parents.wav",
    "legos/allieswin.wav",
    "legos/axiswin.wav",
    "library/vicallies.wav",
    "library/vicaxis.wav",
    "mapsounds/boogie.wav",
    "mapsounds/panzerlied.wav",
    "mapsounds/ramelle-alliesscore.wav",
    "mapsounds/ramelle-axisscore.wav",
    "sponge/dotoday.wav",
    "sponge/weenie.wav",
    "uswin.wav",
];

/// The point-captured and objective-secured announcements
/// ([`FLAG_CAPTURE_NAME`]): each map's capture entities' sound keyvalues.
/// Spelled and sorted as [`VICTORY_MUSIC`] is.
#[rustfmt::skip]
const FLAG_CAPTURE: &[&str] = &[
    "ambience/action1ak.wav",
    "ambience/alliescap.wav",
    "ambience/axiscap.wav",
    "ambience/boualliesscore.wav",
    "ambience/bouaxisscore.wav",
    "ambience/britareasecure.wav",
    "ambience/britobjectivesecure.wav",
    "ambience/britpointcaptured.wav",
    "ambience/ger_areacaptured.wav",
    "ambience/gerareasecure.wav",
    "ambience/gerobjectivesecure.wav",
    "ambience/gerpointcaptured.wav",
    "ambience/merderet-bridgeallie.wav",
    "ambience/merderet-bridgeaxis.wav",
    "ambience/midcap2.wav",
    "ambience/mkfate.wav",
    "ambience/tankcap.wav",
    "ambience/us_areasecure.wav",
    "ambience/usareasecure.wav",
    "ambience/usobjectivesecure.wav",
    "ambience/uspointcaptured.wav",
    "gsg5/gerpointcaptured.wav",
    "gsg5/uspointcaptured.wav",
    "joe_dirt/bowl.wav",
    "joe_dirt/buffout.wav",
    "joe_dirt/chuckles.wav",
    "joe_dirt/fight.wav",
    "joe_dirt/friend.wav",
    "joe_dirt/here.wav",
    "joe_dirt/hot.wav",
    "joe_dirt/nuts_frozen.wav",
    "joe_dirt/piglicker.wav",
    "joe_dirt/scream.wav",
    "joe_dirt/sister.wav",
    "joe_dirt/song.wav",
    "joe_dirt/sucks.wav",
    "joe_dirt/wig.wav",
    "legos/capture_carosel.wav",
    "legos/capture_chess.wav",
    "legos/capture_drop.wav",
    "legos/capture_ferris.wav",
    "legos/capture_love.wav",
    "legos/capture_rocket.wav",
    "legos/capture_sky.wav",
    "mapsounds/alliescap.wav",
    "mapsounds/axiscap.wav",
    "mapsounds/gerpointcaptured.wav",
    "mapsounds/uspointcaptured.wav",
    "sponge/46853.wav",
    "sponge/airisnot.wav",
    "sponge/aprilfools_drinks.wav",
    "sponge/coolest.wav",
    "sponge/finland.wav",
    "sponge/fire.wav",
    "sponge/heartonstick.wav",
    "sponge/living.wav",
    "sponge/ripped.wav",
    "sponge/sedan.wav",
    "u-boatb3/morse.wav",
    "u-boatb3/uboatsound.wav",
    "uboat/morse.wav",
];

type StartSoundFn = unsafe extern "C" fn(i32, i32, *const u8, *const f32, f32, f32, i32, i32);

static VICTORY_MUSIC_CVAR: Cvar = Cvar::new();
static FLAG_CAPTURE_CVAR: Cvar = Cvar::new();
pub static LOG_CVAR: Cvar = Cvar::new();
/// `S_StartStaticSound` and `S_StartDynamicSound`, once the calls are
/// redirected; 0 before.
static ORIGINAL_STATIC: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_DYNAMIC: AtomicUsize = AtomicUsize::new(0);
/// Index into [`BUILDS`] + 1 once redirected.
static REDIRECTED: AtomicUsize = AtomicUsize::new(0);
static GAVE_UP: AtomicBool = AtomicBool::new(false);
static MUTED: AtomicU32 = AtomicU32::new(0);

/// Largest entity lump read. The biggest in the 250 installed maps is ~440 KB
/// (`dod_lennon4`); a string running past this is not an entity lump.
const MAX_ENTITY_TEXT: usize = 2 * 1024 * 1024;

/// The current map's own names, from [`read_map`]. Written only by the
/// per-frame poll on the engine thread; the sound hook only `try_read`s it.
static CURRENT: RwLock<MapState> = RwLock::new(MapState::NoMap);

/// What the current map's names were read from, so a new level, a reloaded
/// world model or a moved lump is read again: (level name hash, world model,
/// entity text pointer).
static READ_LEVEL: AtomicU32 = AtomicU32::new(0);
static READ_MODEL: AtomicUsize = AtomicUsize::new(0);
static READ_ENTITIES: AtomicUsize = AtomicUsize::new(0);

/// Where a name that muted a sound was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// The current map's entities.
    Map,
    /// The family's fixed list.
    List,
}

impl Source {
    fn word(self) -> &'static str {
        match self {
            Source::Map => "map",
            Source::List => "list",
        }
    }
}

/// One map's own victory and capture names, normalised as the hook compares
/// them ([`normalize`]).
#[derive(Debug, Default, PartialEq, Eq)]
struct MapNames {
    victory: BTreeSet<Vec<u8>>,
    capture: BTreeSet<Vec<u8>>,
}

impl MapNames {
    fn of(&self, family: Family) -> &BTreeSet<Vec<u8>> {
        match family {
            Family::VictoryMusic => &self.victory,
            Family::FlagCapture => &self.capture,
        }
    }
}

/// The current map, as far as the names go.
#[derive(Debug, PartialEq, Eq)]
enum MapState {
    /// No level loaded yet.
    NoMap,
    /// A level is loading and its world model isn't ready yet: the lists alone.
    Waiting { map: String },
    /// Read from the map's entities.
    Read { map: String, names: MapNames },
    /// The entities couldn't be read or parsed: the lists alone.
    Unreadable { map: String, why: String },
}

impl MapState {
    fn names(&self) -> Option<&MapNames> {
        match self {
            MapState::Read { names, .. } => Some(names),
            _ => None,
        }
    }

    /// For `ds_debug_status` and the log.
    fn describe(&self) -> String {
        match self {
            MapState::NoMap => "map sounds: no map loaded yet".into(),
            MapState::Waiting { map } => format!("map sounds: {map} not read yet, the lists alone"),
            MapState::Read { map, names } => format!(
                "map sounds: {map} {} victory, {} capture (from the map)",
                names.victory.len(),
                names.capture.len()
            ),
            MapState::Unreadable { map, why } => {
                format!("map sounds: {map} unreadable ({why}), the lists alone")
            }
        }
    }
}

/// The names one map's entity lump declares, as the engine keeps it
/// (`model_t.entities`: text up to a NUL), or why there are none.
fn classify(text: &[u8]) -> Result<MapNames, String> {
    let entities = dod::map_entities::parse_entity_text(&dod::map_entities::text_up_to_nul(text))?;
    let sounds = dod::map_sounds::map_sounds(&entities);
    let hook_form = |names: &BTreeSet<String>| -> BTreeSet<Vec<u8>> {
        names
            .iter()
            .map(|name| {
                let mut buf = [0u8; MAX_NAME];
                normalize(name.as_bytes(), &mut buf).to_vec()
            })
            .collect()
    };
    Ok(MapNames {
        victory: hook_form(&sounds.round_win),
        capture: hook_form(&sounds.capture),
    })
}

/// Whether `info` describes memory a read can't fault on.
fn readable_region(info: &windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION) -> bool {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD,
        PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
    };
    let readable = PAGE_READONLY
        | PAGE_READWRITE
        | PAGE_WRITECOPY
        | PAGE_EXECUTE_READ
        | PAGE_EXECUTE_READWRITE
        | PAGE_EXECUTE_WRITECOPY;
    info.State == MEM_COMMIT
        && info.Protect & readable != 0
        && info.Protect & (PAGE_GUARD | PAGE_NOACCESS) == 0
}

/// The bytes of the NUL-terminated string at `at`, without the NUL, reading
/// only committed, readable pages. `None` if any of it isn't readable or no
/// NUL comes within `max` bytes.
fn read_c_string(at: usize, max: usize) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};
    if at == 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut cursor = at;
    while out.len() < max {
        // Safety: VirtualQuery writes a struct we own and never faults.
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let written = unsafe {
            VirtualQuery(
                cursor as *const std::ffi::c_void,
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if written == 0 || !readable_region(&info) {
            return None;
        }
        let region_end = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        let take = region_end.saturating_sub(cursor).min(max - out.len());
        if take == 0 {
            return None;
        }
        // Safety: `take` bytes from `cursor` lie inside one committed,
        // readable region, checked just above.
        let chunk = unsafe { std::slice::from_raw_parts(cursor as *const u8, take) };
        if let Some(nul) = chunk.iter().position(|&b| b == 0) {
            out.extend_from_slice(&chunk[..nul]);
            return Some(out);
        }
        out.extend_from_slice(chunk);
        cursor += take;
    }
    None
}

/// Reads the current map's names once per map, from the world model the
/// engine loaded. Every frame once the calls are redirected, on the engine
/// thread; a frame where nothing changed costs a hash of the level name, one
/// `VirtualQuery` and a few pointer reads, and allocates nothing.
fn read_map(build: &Build) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // Safety: a pointer into the engine's client state, valid for the
    // session; checked for null before reading.
    let raw = unsafe { (engfuncs.pfn_get_level_name)() };
    let level: &[u8] = if raw.is_null() {
        &[]
    } else {
        unsafe { CStr::from_ptr(raw) }.to_bytes()
    };
    if level.is_empty() {
        return;
    }
    let level_hash = level.iter().fold(0x811c_9dc5_u32, |h, &b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    // Only built when something changed: most frames allocate nothing.
    let map = || crate::map_text::short_name(&String::from_utf8_lossy(level)).to_string();
    if READ_LEVEL.swap(level_hash, Ordering::Relaxed) != level_hash {
        // A new level: the last map's names no longer apply.
        READ_MODEL.store(0, Ordering::Relaxed);
        READ_ENTITIES.store(0, Ordering::Relaxed);
        set_state(MapState::Waiting { map: map() });
    }

    // The world is entity 0's model (`cl_entities[0].model`).
    // Safety: the engine's own accessor, bounds-checked by the engine.
    let entity = unsafe { (engfuncs.get_entity_by_index)(0) };
    if entity.is_null() {
        return;
    }
    // Safety: a cl_entity_t the engine owns; `model` is the SDK layout.
    let model = unsafe { (*entity).model } as usize;
    if model == 0 || !crate::crash::readable(model, build.model_entities_at + 4) {
        return;
    }
    // Safety: readable, checked above; `name` is a NUL-padded char[64].
    let name = unsafe { &(*(model as *const engine::ModelSPartial)).name };
    let name_len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    // Only this level's own world, fully loaded: until the new map's world
    // model replaces the old one, the old one's lump is hunk memory the new
    // map may already be reusing.
    if !name[..name_len].eq_ignore_ascii_case(level) {
        return;
    }
    // Safety: both inside the readable span checked above.
    let model_type =
        unsafe { std::ptr::read_unaligned((model + build.model_type_at) as *const i32) };
    // A 32-bit pointer: this DLL only ever runs in the 32-bit engine.
    let entities =
        unsafe { std::ptr::read_unaligned((model + build.model_entities_at) as *const u32) }
            as usize;
    if model_type != 0 {
        return;
    }
    if READ_MODEL.load(Ordering::Relaxed) == model
        && READ_ENTITIES.load(Ordering::Relaxed) == entities
    {
        return;
    }
    READ_MODEL.store(model, Ordering::Relaxed);
    READ_ENTITIES.store(entities, Ordering::Relaxed);

    let map = map();
    let state = if entities == 0 {
        MapState::Unreadable {
            map,
            why: "the map has no entity lump".into(),
        }
    } else {
        match read_c_string(entities, MAX_ENTITY_TEXT) {
            None => MapState::Unreadable {
                map,
                why: format!("its entity text isn't readable or runs past {MAX_ENTITY_TEXT} bytes"),
            },
            Some(text) => match classify(&text) {
                Ok(names) => MapState::Read { map, names },
                Err(why) => MapState::Unreadable { map, why },
            },
        }
    };
    let line = state.describe();
    set_state(state);
    // Safety: plain file append.
    unsafe { crate::debug::report(&line) };
}

fn set_state(state: MapState) {
    // A poisoned lock only means a panic elsewhere held it; the state is a
    // plain value, so take it back.
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = state;
}

/// The two kinds of map sound, each with its own list and setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    VictoryMusic,
    FlagCapture,
}

impl Family {
    pub const ALL: [Family; 2] = [Family::VictoryMusic, Family::FlagCapture];

    /// The setting that mutes this family.
    pub fn setting(self) -> &'static str {
        match self {
            Family::VictoryMusic => VICTORY_MUSIC_NAME,
            Family::FlagCapture => FLAG_CAPTURE_NAME,
        }
    }

    fn names(self) -> &'static [&'static str] {
        match self {
            Family::VictoryMusic => VICTORY_MUSIC,
            Family::FlagCapture => FLAG_CAPTURE,
        }
    }

    pub fn cvar(self) -> &'static Cvar {
        match self {
            Family::VictoryMusic => &VICTORY_MUSIC_CVAR,
            Family::FlagCapture => &FLAG_CAPTURE_CVAR,
        }
    }

    /// Whether this family's setting is on.
    pub fn on(self) -> bool {
        self.cvar().on()
    }
}

fn any_on() -> bool {
    Family::ALL.into_iter().any(Family::on)
}

fn logging() -> bool {
    LOG_CVAR.on()
}

/// `raw` as the list spells names: lowercase, forward slashes, without the
/// engine's `*` (streamed) marker.
fn normalize<'a>(raw: &[u8], out: &'a mut [u8; MAX_NAME]) -> &'a [u8] {
    let raw = raw.strip_prefix(b"*").unwrap_or(raw);
    let len = raw.len().min(MAX_NAME);
    for (dst, &src) in out.iter_mut().zip(&raw[..len]) {
        *dst = match src {
            b'\\' => b'/',
            other => other.to_ascii_lowercase(),
        };
    }
    &out[..len]
}

fn listed_in(names: &[&str], name: &[u8]) -> bool {
    names.binary_search_by(|n| n.as_bytes().cmp(name)).is_ok()
}

/// The family whose setting drops a sound named `raw` with these flags, and
/// where the name was found, when `on` says which settings are on and `map`
/// holds the current map's own names; `None` plays it.
///
/// The map's names and the fixed lists are a union: a name in either mutes.
/// The map is asked first, so a name both have is reported as the map's.
fn muted_by(
    raw: &[u8],
    flags: i32,
    on: impl Fn(Family) -> bool,
    map: Option<&MapNames>,
) -> Option<(Family, Source)> {
    if flags & SND_STOP != 0 {
        return None;
    }
    let mut buf = [0u8; MAX_NAME];
    let name = normalize(raw, &mut buf);
    let wanted = || Family::ALL.into_iter().filter(|&family| on(family));
    if let Some(map) = map
        && let Some(family) = wanted().find(|&family| map.of(family).contains(name))
    {
        return Some((family, Source::Map));
    }
    wanted()
        .find(|&family| listed_in(family.names(), name))
        .map(|family| (family, Source::List))
}

/// The name an `sfx` argument carries: an `sfx_t`, whose `name[64]` comes
/// first, or for a sentence `svc_sound`'s own 64-byte name buffer.
///
/// Safety: `sfx` is null or points at 64 readable bytes.
unsafe fn name_of<'a>(sfx: *const u8) -> &'a [u8] {
    if sfx.is_null() {
        return &[];
    }
    // Read up to the NUL only: past it the buffer may be anything.
    // Safety: as above.
    let len = (0..MAX_NAME)
        .find(|&i| unsafe { *sfx.add(i) } == 0)
        .unwrap_or(MAX_NAME);
    unsafe { std::slice::from_raw_parts(sfx, len) }
}

#[allow(clippy::too_many_arguments)]
unsafe fn start(
    kind: Kind,
    original: &AtomicUsize,
    entnum: i32,
    channel: i32,
    sfx: *const u8,
    origin: *const f32,
    volume: f32,
    attenuation: f32,
    flags: i32,
    pitch: i32,
) {
    // Safety: the engine passes an sfx_t or its 64-byte sentence buffer.
    let name = unsafe { name_of(sfx) };
    let muted = {
        // Never waits: the only writer is the per-frame poll on this same
        // thread, so a lock it can't take means the lists alone.
        let current = CURRENT.try_read().ok();
        muted_by(
            name,
            flags,
            Family::on,
            current.as_deref().and_then(MapState::names),
        )
    };
    if logging() {
        let outcome = match muted {
            Some((family, source)) => {
                format!("muted by {} ({})", family.setting(), source.word())
            }
            None => "played".into(),
        };
        let line = format!(
            "{LOG_NAME}: {} ent {entnum} chan {channel} \"{}\" flags {flags:#x} vol {volume:.2} attn {attenuation:.2} -> {outcome}",
            kind.word(),
            String::from_utf8_lossy(name),
        );
        // Safety: plain file append; the log cvar is a diagnostic.
        unsafe { crate::debug::report(&line) };
    }
    if muted.is_some() {
        MUTED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let original = original.load(Ordering::Acquire);
    // Safety: the address the call went to before it was redirected, stored
    // before the call was.
    let original: StartSoundFn = unsafe { std::mem::transmute(original) };
    unsafe {
        original(
            entnum,
            channel,
            sfx,
            origin,
            volume,
            attenuation,
            flags,
            pitch,
        )
    };
}

/// Stands in for `S_StartStaticSound` at its two `svc_` call sites.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn start_static(
    entnum: i32,
    channel: i32,
    sfx: *const u8,
    origin: *const f32,
    volume: f32,
    attenuation: f32,
    flags: i32,
    pitch: i32,
) {
    unsafe {
        start(
            Kind::Static,
            &ORIGINAL_STATIC,
            entnum,
            channel,
            sfx,
            origin,
            volume,
            attenuation,
            flags,
            pitch,
        )
    }
}

/// Stands in for `S_StartDynamicSound` at `svc_sound`'s call site.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn start_dynamic(
    entnum: i32,
    channel: i32,
    sfx: *const u8,
    origin: *const f32,
    volume: f32,
    attenuation: f32,
    flags: i32,
    pitch: i32,
) {
    unsafe {
        start(
            Kind::Dynamic,
            &ORIGINAL_DYNAMIC,
            entnum,
            channel,
            sfx,
            origin,
            volume,
            attenuation,
            flags,
            pitch,
        )
    }
}

/// The calls of one build, all found: (`E8` address, kind), or why not.
fn find_calls(base: usize, build: &Build) -> Result<[(usize, Kind); 3], String> {
    let mut out = [(0, Kind::Static); 3];
    for (slot, site) in out.iter_mut().zip(&build.sites) {
        // Safety: `base` is a module handle the loader gave us.
        let found = unsafe { scan::find_unique(base, site.pattern) }
            .map_err(|why| format!("{}: {why}", site.what))?;
        *slot = (found + site.call_at, site.kind);
    }
    Ok(out)
}

fn target_of(call: usize) -> usize {
    // Safety: the scan matched an `E8 rel32` here.
    let rel = unsafe { std::ptr::read_unaligned((call + 1) as *const i32) };
    (call as isize + 5 + rel as isize) as usize
}

fn redirect() -> Result<&'static str, String> {
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let mut errors = Vec::new();
    for (index, build) in BUILDS.iter().enumerate() {
        let calls = match find_calls(base, build) {
            Ok(calls) => calls,
            Err(why) => {
                errors.push(format!("{}: {why}", build.name));
                continue;
            }
        };
        let targets = |kind: Kind| {
            calls
                .iter()
                .filter(move |(_, k)| *k == kind)
                .map(|(call, _)| target_of(*call))
        };
        let statics: Vec<usize> = targets(Kind::Static).collect();
        let dynamic: Vec<usize> = targets(Kind::Dynamic).collect();
        if statics.windows(2).any(|w| w[0] != w[1]) || dynamic.len() != 1 {
            return Err(format!(
                "{}: the static calls don't share one target",
                build.name
            ));
        }
        // The originals first, so a sound arriving between two writes calls
        // the right function.
        ORIGINAL_STATIC.store(statics[0], Ordering::Release);
        ORIGINAL_DYNAMIC.store(dynamic[0], Ordering::Release);
        for (call, kind) in calls {
            let ours = match kind {
                Kind::Static => start_static as *const () as usize,
                Kind::Dynamic => start_dynamic as *const () as usize,
            };
            let new_rel = (ours as isize - (call as isize + 5)) as i32;
            // Safety: `call + 1` is the call's rel32, in hw.dll's code.
            if !unsafe { crate::patch::write_code_bytes(call + 1, &new_rel.to_le_bytes()) } {
                return Err(format!(
                    "{}: could not make the call at hw.dll+{:#x} writable",
                    build.name,
                    call - base
                ));
            }
        }
        REDIRECTED.store(index + 1, Ordering::Release);
        return Ok(build.name);
    }
    Err(format!(
        "the svc sound calls weren't found ({})",
        errors.join("; ")
    ))
}

/// Redirects the calls the first time either setting or the log is on, then
/// reads each map's own names as it loads. Every frame, from
/// `commands::poll`, on the engine thread.
pub fn poll() {
    if REDIRECTED.load(Ordering::Acquire) == 0 {
        if GAVE_UP.load(Ordering::Relaxed) || !(any_on() || logging()) {
            return;
        }
        let line = match redirect() {
            Ok(build) => format!("map sound mutes: svc sound calls redirected ({build} hw.dll)"),
            Err(why) => {
                GAVE_UP.store(true, Ordering::Relaxed);
                format!("map sound mutes: {why}")
            }
        };
        unsafe { crate::debug::report(&line) };
    }
    if let Some(build) = REDIRECTED
        .load(Ordering::Acquire)
        .checked_sub(1)
        .and_then(|index| BUILDS.get(index))
    {
        read_map(build);
    }
}

/// For `ds_debug_status`.
pub fn status() -> String {
    match REDIRECTED.load(Ordering::Acquire) {
        0 if GAVE_UP.load(Ordering::Relaxed) => {
            "could not find the svc sound calls (see the log)".into()
        }
        0 => "not installed yet (it installs the first time it is on)".into(),
        n => format!(
            "installed ({} hw.dll), {} sounds muted this session",
            BUILDS[n - 1].name,
            MUTED.load(Ordering::Relaxed)
        ),
    }
}

/// For `ds_debug_status`: the current map's own names, or that the lists
/// stand alone.
pub fn map_status() -> String {
    if REDIRECTED.load(Ordering::Acquire) == 0 {
        return "map sounds: read once the calls are redirected".into();
    }
    match CURRENT.try_read() {
        Ok(state) => state.describe(),
        Err(_) => "map sounds: busy".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use Family::{FlagCapture, VictoryMusic};

    /// Whether `name` is muted with every setting on, by the lists alone.
    fn mutes(name: &[u8], flags: i32) -> bool {
        muted_by(name, flags, |_| true, None).is_some()
    }

    /// Who mutes `name` with only `family`'s setting on, by the lists alone.
    fn with_only(family: Family, name: &str) -> Option<Family> {
        muted_by(name.as_bytes(), 0, |f| f == family, None).map(|(f, _)| f)
    }

    /// Anzio's real sound wiring (`test-fixtures/dod_anzio_sound_entities.txt`).
    const ANZIO: &str = include_str!("../../test-fixtures/dod_anzio_sound_entities.txt");

    /// `text` as the engine keeps it: NUL-terminated, with whatever follows.
    fn in_memory(text: &str) -> Vec<u8> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.extend_from_slice(b"\0{ \"classname\" \"after the NUL\" }");
        bytes
    }

    fn names(list: &[&str]) -> BTreeSet<Vec<u8>> {
        list.iter().map(|n| n.as_bytes().to_vec()).collect()
    }

    /// The runtime route (bytes in the engine's memory) and the offline one
    /// (the example's table, `dod::map_sounds` on the parsed lump) agree.
    #[test]
    fn the_runtime_classification_matches_the_offline_table() {
        let runtime = classify(&in_memory(ANZIO)).expect("anzio parses");
        assert_eq!(
            runtime.victory,
            names(&["ambience/germanwin.wav", "ambience/uswin.wav"])
        );
        assert_eq!(
            runtime.capture,
            names(&[
                "ambience/gerpointcaptured.wav",
                "ambience/uspointcaptured.wav"
            ])
        );

        let offline = dod::map_sounds::map_sounds(
            &dod::map_entities::parse_entity_text(ANZIO).expect("anzio parses"),
        );
        let as_bytes = |set: &BTreeSet<String>| -> BTreeSet<Vec<u8>> {
            set.iter().map(|n| n.as_bytes().to_vec()).collect()
        };
        assert_eq!(runtime.victory, as_bytes(&offline.round_win));
        assert_eq!(runtime.capture, as_bytes(&offline.capture));
    }

    /// Every installed map, read both ways: the whole lump as the engine
    /// keeps it, and the offline example's `parse_entities` on the `.bsp`.
    /// `DOD_MAPS_DIR`, or the pre-Anniversary install's `dod\maps`.
    #[test]
    #[ignore = "needs a folder of real maps"]
    fn every_installed_map_classifies_the_same_both_ways() {
        let dir = std::env::var_os("DOD_MAPS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod\maps",
                )
            });
        let mut read = 0;
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("no maps at {}: {e}", dir.display()))
        {
            let path = entry.unwrap().path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bsp"))
            {
                continue;
            }
            let bsp = std::fs::read(&path).unwrap();
            let (offset, length) = dod::map_entities::lump_span(&bsp, 0).unwrap();
            let lump = &bsp[offset..offset + length];
            let offline = dod::map_entities::parse_entities(&bsp)
                .map(|entities| dod::map_sounds::map_sounds(&entities));
            match (classify(lump), offline) {
                (Ok(runtime), Ok(offline)) => {
                    let as_bytes = |set: &BTreeSet<String>| -> BTreeSet<Vec<u8>> {
                        set.iter().map(|n| n.as_bytes().to_vec()).collect()
                    };
                    assert_eq!(
                        runtime.victory,
                        as_bytes(&offline.round_win),
                        "{}",
                        path.display()
                    );
                    assert_eq!(
                        runtime.capture,
                        as_bytes(&offline.capture),
                        "{}",
                        path.display()
                    );
                }
                (Err(_), Err(_)) => {}
                (runtime, offline) => panic!(
                    "{}: runtime {:?}, offline {:?}",
                    path.display(),
                    runtime.is_ok(),
                    offline.is_ok()
                ),
            }
            read += 1;
        }
        assert!(read > 0, "no .bsp in {}", dir.display());
    }

    /// A custom map's own name, in neither list.
    const CUSTOM_WIN: &str = "custom_map/our_team_won.wav";
    const CUSTOM_CAP: &str = "custom_map/flag_taken.wav";

    fn custom_map() -> MapNames {
        MapNames {
            victory: names(&[CUSTOM_WIN, "ambience/uswin.wav"]),
            capture: names(&[CUSTOM_CAP]),
        }
    }

    #[test]
    fn a_name_only_the_map_has_is_muted_from_the_map() {
        let map = custom_map();
        let only =
            |family: Family, name: &str| muted_by(name.as_bytes(), 0, |f| f == family, Some(&map));
        assert_eq!(
            only(VictoryMusic, CUSTOM_WIN),
            Some((VictoryMusic, Source::Map))
        );
        assert_eq!(only(FlagCapture, CUSTOM_WIN), None);
        assert_eq!(
            only(FlagCapture, CUSTOM_CAP),
            Some((FlagCapture, Source::Map))
        );
        assert_eq!(only(VictoryMusic, CUSTOM_CAP), None);
        // Case, slashes and the stream marker, as for the lists.
        assert_eq!(
            only(VictoryMusic, "*Custom_Map\\Our_Team_Won.WAV"),
            Some((VictoryMusic, Source::Map))
        );
        // Without the map it plays: the lists don't know it.
        assert_eq!(muted_by(CUSTOM_WIN.as_bytes(), 0, |_| true, None), None);
    }

    /// The union: everything the lists mute today, they still mute with a
    /// map read, including a name this map doesn't declare.
    #[test]
    fn the_lists_still_mute_beside_the_map() {
        let map = custom_map();
        for name in VICTORY_SAMPLES.iter().chain(&CAPTURE_SAMPLES) {
            let lists = muted_by(name.as_bytes(), 0, |_| true, None).map(|(f, _)| f);
            let both = muted_by(name.as_bytes(), 0, |_| true, Some(&map)).map(|(f, _)| f);
            assert!(lists.is_some(), "{name}");
            assert_eq!(lists, both, "{name}");
        }
        assert_eq!(
            muted_by(b"ambience/gerpointcaptured.wav", 0, |_| true, Some(&map)),
            Some((FlagCapture, Source::List))
        );
        // In both: reported as the map's.
        assert_eq!(
            muted_by(b"ambience/uswin.wav", 0, |_| true, Some(&map)),
            Some((VictoryMusic, Source::Map))
        );
    }

    #[test]
    fn the_map_mutes_nothing_with_both_settings_off_or_on_a_stop() {
        let map = custom_map();
        for name in [CUSTOM_WIN, CUSTOM_CAP, "ambience/uswin.wav"] {
            assert_eq!(muted_by(name.as_bytes(), 0, |_| false, Some(&map)), None);
            assert_eq!(
                muted_by(name.as_bytes(), SND_STOP, |_| true, Some(&map)),
                None
            );
        }
        assert_eq!(
            muted_by(b"ambience/windy.wav", 0, |_| true, Some(&map)),
            None
        );
    }

    /// The hook reads whatever is in the game's memory, and a release build
    /// aborts on a panic: malformed text gives an error, never a panic.
    #[test]
    fn malformed_entity_text_gives_nothing_without_panicking() {
        for text in [
            &b"{ \"classname\" \"dod_control_point\" \"point_allies_capsound"[..],
            b"{ \"key\" }",
            b"\"",
        ] {
            assert!(
                classify(text).is_err(),
                "{:?}",
                String::from_utf8_lossy(text)
            );
        }
        // Not entity text at all: whatever it gives, it names nothing.
        let junk: Vec<u8> = (1..=255u8).cycle().take(8192).collect();
        if let Ok(names) = classify(&junk) {
            assert!(names.victory.is_empty() && names.capture.is_empty());
        }
        assert_eq!(classify(b""), Ok(MapNames::default()));
        assert_eq!(classify(b"\0{ \"x\" \"y\" }"), Ok(MapNames::default()));
        // Every prefix of the real map, as a truncated lump would be.
        for end in (0..ANZIO.len()).step_by(7) {
            let _ = classify(&ANZIO.as_bytes()[..end]);
        }
    }

    #[test]
    fn a_c_string_is_read_up_to_its_nul() {
        let buf = b"maps/dod_anzio.bsp\0after".to_vec();
        assert_eq!(
            read_c_string(buf.as_ptr() as usize, 64).as_deref(),
            Some(&b"maps/dod_anzio.bsp"[..])
        );
        // No NUL within the limit.
        assert_eq!(read_c_string(buf.as_ptr() as usize, 5), None);
        // Null and unmapped memory.
        assert_eq!(read_c_string(0, 64), None);
        assert_eq!(read_c_string(0x10, 64), None);
    }

    #[test]
    fn the_status_line_says_where_the_names_came_from() {
        let read = MapState::Read {
            map: "dod_anzio".into(),
            names: classify(&in_memory(ANZIO)).unwrap(),
        };
        assert_eq!(
            read.describe(),
            "map sounds: dod_anzio 2 victory, 2 capture (from the map)"
        );
        let unreadable = MapState::Unreadable {
            map: "dod_x".into(),
            why: "the map has no entity lump".into(),
        };
        assert!(unreadable.describe().ends_with("the lists alone"));
        assert_eq!(unreadable.names(), None);
    }

    /// `model_t` is the SDK's struct, shared with client.dll, so both builds
    /// agree; `tools/verify_map_entities_offsets.py` checks each `hw.dll`.
    #[test]
    fn the_model_offsets_are_the_sdk_layout() {
        for build in &BUILDS {
            assert_eq!(build.model_type_at, 0x44, "{}", build.name);
            assert_eq!(build.model_entities_at, 0x180, "{}", build.name);
        }
    }

    const VICTORY_SAMPLES: [&str; 6] = [
        "ambience/uswin.wav",
        "ambience/germanwin.wav",
        "ambience/britwin.wav",
        "ambience/alliesscore.wav",
        "ambience/axisscore.wav",
        "ambience/no_winner.wav",
    ];

    const CAPTURE_SAMPLES: [&str; 8] = [
        "ambience/uspointcaptured.wav",
        "ambience/gerpointcaptured.wav",
        "ambience/usareasecure.wav",
        "ambience/gerareasecure.wav",
        "ambience/alliescap.wav",
        "ambience/axiscap.wav",
        // dod_escape also places it as an ambient_generic, for a mortar
        // objective destroyed: still an announcement, so still here.
        "ambience/usobjectivesecure.wav",
        "ambience/gerobjectivesecure.wav",
    ];

    #[test]
    fn the_names_are_sorted_lowercase_and_unique() {
        for family in Family::ALL {
            for pair in family.names().windows(2) {
                assert!(pair[0] < pair[1], "{} then {}", pair[0], pair[1]);
            }
            for name in family.names() {
                assert_eq!(*name, name.to_ascii_lowercase());
                assert!(!name.contains('\\') && name.ends_with(".wav"), "{name}");
                assert!(name.len() < MAX_NAME, "{name}");
            }
        }
    }

    #[test]
    fn no_name_is_in_both_lists_and_the_survey_found_98() {
        for name in VICTORY_MUSIC {
            assert!(!FLAG_CAPTURE.contains(name), "{name}");
        }
        assert_eq!(VICTORY_MUSIC.len(), 37);
        assert_eq!(FLAG_CAPTURE.len(), 61);
    }

    #[test]
    fn victory_music_is_muted_only_by_its_own_setting() {
        for name in VICTORY_SAMPLES {
            assert_eq!(with_only(VictoryMusic, name), Some(VictoryMusic), "{name}");
            assert_eq!(with_only(FlagCapture, name), None, "{name}");
        }
        // dod_vicenza fires these through a dod_score_ent.
        assert_eq!(with_only(VictoryMusic, "uswin.wav"), Some(VictoryMusic));
    }

    #[test]
    fn capture_announcements_are_muted_only_by_their_own_setting() {
        for name in CAPTURE_SAMPLES {
            assert_eq!(with_only(FlagCapture, name), Some(FlagCapture), "{name}");
            assert_eq!(with_only(VictoryMusic, name), None, "{name}");
        }
    }

    #[test]
    fn with_both_settings_off_everything_plays() {
        for name in VICTORY_SAMPLES.iter().chain(&CAPTURE_SAMPLES) {
            assert_eq!(
                muted_by(name.as_bytes(), 0, |_| false, None),
                None,
                "{name}"
            );
        }
    }

    #[test]
    fn names_match_whatever_their_case_slashes_or_stream_marker() {
        assert!(mutes(b"Ambience/USwin.wav", 0));
        assert!(mutes(b"ambience\\gerpointcaptured.wav", 0));
        assert!(mutes(b"*ambience/uswin.wav", 0));
    }

    /// The console's autocomplete swaps a name that is the whole start of
    /// another for the longer one on space.
    #[test]
    fn neither_setting_is_the_start_of_another_name() {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for part in text.split("console_name!(\"").skip(1) {
                        let name = part.split('"').next().unwrap();
                        // `console_name!("")` is the bare prefix.
                        if !name.is_empty() {
                            out.push(name.to_string());
                        }
                    }
                }
            }
        }
        let mut all = Vec::new();
        walk(std::path::Path::new("src"), &mut all);
        assert!(all.len() > 50, "found only {} names", all.len());
        for ours in ["sound_mute_victory_music", "sound_mute_flag_capture"] {
            assert!(all.iter().any(|n| n == ours), "{ours}");
            for other in all.iter().filter(|n| *n != ours) {
                assert!(!other.starts_with(ours), "{ours} is the start of {other}");
                assert!(
                    !ours.starts_with(other.as_str()),
                    "{other} is the start of {ours}"
                );
            }
        }
    }

    #[test]
    fn everything_else_plays() {
        for name in [
            // A substring match on "win" would have taken these (28 maps).
            "ambience/windy.wav",
            "ambience/erriewind.wav",
            // Map ambience placed at map start.
            "ambience/tree.wav",
            "ambience/dodambience3.wav",
            "weapons/kar_shoot.wav",
            "player/pl_step1.wav",
            "player/usattack.wav",
            // A sentence.
            "!HG_ALERT",
            "",
            "ambience/uswin.wav.extra",
            "ambience/uswin",
        ] {
            assert!(!mutes(name.as_bytes(), 0), "{name}");
        }
    }

    #[test]
    fn a_stop_always_goes_through() {
        assert!(!mutes(b"ambience/uswin.wav", SND_STOP));
        assert!(!mutes(b"ambience/uspointcaptured.wav", SND_STOP | 1));
    }

    #[test]
    fn a_long_name_is_cut_at_the_engines_limit() {
        let long = [b'a'; 100];
        let mut buf = [0u8; MAX_NAME];
        assert_eq!(normalize(&long, &mut buf).len(), MAX_NAME);
        assert!(!mutes(&long, 0));
    }

    #[test]
    fn a_name_ends_at_its_nul() {
        let mut sfx = [0u8; MAX_NAME];
        sfx[..18].copy_from_slice(b"ambience/uswin.wav");
        sfx[19] = b'x';
        let name = unsafe { name_of(sfx.as_ptr()) };
        assert_eq!(name, b"ambience/uswin.wav");
        assert!(unsafe { name_of(std::ptr::null()) }.is_empty());
    }

    #[test]
    fn each_call_offset_lands_on_the_call() {
        for build in &BUILDS {
            for site in &build.sites {
                let bytes: Vec<&str> = site.pattern.split(' ').collect();
                assert_eq!(bytes[site.call_at], "E8", "{} {}", build.name, site.what);
                assert!(
                    bytes[site.call_at + 1..site.call_at + 5]
                        .iter()
                        .all(|b| *b == "??"),
                    "{} {}",
                    build.name,
                    site.what
                );
            }
        }
    }

    #[test]
    fn each_build_has_two_static_calls_and_one_dynamic() {
        for build in &BUILDS {
            let kinds: Vec<Kind> = build.sites.iter().map(|s| s.kind).collect();
            assert_eq!(kinds.iter().filter(|k| **k == Kind::Static).count(), 2);
            assert_eq!(kinds.iter().filter(|k| **k == Kind::Dynamic).count(), 1);
        }
    }
}
