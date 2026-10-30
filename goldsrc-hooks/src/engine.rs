//! Captures the engine's `cl_enginefuncs_s` table (`pEngfuncs`), the
//! `engine_studio_api_s` table (`pstudio`), a genuine per-frame callback, and
//! a per-entity suppress-or-forward hook for `hide_asset.rs`, by
//! intercepting how `hw.dll` resolves `client.dll`'s entry points in the
//! first place.
//!
//! ## Why this isn't the "obvious" export hook
//!
//! Two earlier designs failed, and understanding why is what makes this one
//! correct (see `docs/goldsrc_client_dll_internals.md` for the full
//! reverse-engineering write-up, and issue #204 for the investigation trail):
//!
//! 1. **Patching `client.dll`'s export table** for `Initialize` / `HUD_Frame`
//!    / `HUD_GetStudioModelInterface`. The patch was provably applied (the
//!    real Win32 `GetProcAddress` read our value back), yet nothing ever
//!    called through it. Root cause, confirmed by disassembling `hw.dll`:
//!    **the engine never asks for those names at all** on a "secured"
//!    `client.dll`. It calls `GetProcAddress(hClient, "F")` and, if that
//!    single export exists, calls it and stops -- if `F` is absent it
//!    `FreeLibrary`s and fails outright, with no per-name fallback on that
//!    path. DoD 1.3's `client.dll` does export `F`, so the classic names are
//!    dead weight in it. Xash3D's own GoldSrc-compatible loader calls this
//!    exact case "single callback export (secured client dlls)".
//!
//! 2. **Byte-signature scanning for `pEngfuncs`**, ported faithfully from
//!    HLAE's `hl_addresses.cpp`. Its anchor string `"ScreenFade"` simply does
//!    not exist anywhere in DoD's `client.dll` (verified both in-process and
//!    by offline string extraction from the file), so the scan can't start.
//!
//! ## What this module does instead
//!
//! It hooks **`hw.dll`'s Import Address Table entry for
//! `KERNEL32!GetProcAddress`** -- the exact call site the disassembly shows
//! the engine using -- alongside the `LoadLibraryA` entry it already hooked
//! to spot `client.dll` loading. This is the same IAT technique HLAE's own
//! `CAfxImportDllHook` uses, and the `LoadLibraryA` half was already proven
//! reliable in testing; the `GetProcAddress` half is a direct extension of it.
//!
//! Every lookup against the `client.dll` module is then handled in one place,
//! covering **both** engine conventions with one mechanism and no byte
//! patterns or hardcoded offsets:
//!
//! - `"F"` (secured builds, what DoD 1.3 actually uses) -- we return our own
//!   wrapper, which calls the real `F` to let it fill the caller's
//!   `cldll_func_t` table, then swaps nine of its 43 slots for our
//!   trampolines before handing it back to the engine.
//! - `"Initialize"` / `"HUD_Frame"` / `"HUD_AddEntity"` / `"HUD_Redraw"` /
//!   `"HUD_Key_Event"` / `"HUD_DirectorMessage"` /
//!   `"HUD_GetStudioModelInterface"` (classic non-secured builds) -- we
//!   return the trampoline directly.
//!
//! Either way the trampolines receive what we need as ordinary arguments:
//! `Initialize` hands us `pEnginefuncs`, `HUD_GetStudioModelInterface` hands
//! us `pstudio`, `HUD_Frame` gives a real per-frame tick, and `HUD_AddEntity`
//! hands us the model path of every entity about to be added to the render
//! list -- the first of two that aren't a one-time capture, since it runs the
//! suppress/forward decision itself, once per entity. `HUD_DirectorMessage`
//! is the second: it hands us every HLTV director message, so
//! `hltv_messages.rs` can drop the proxy's on-screen text. `HUD_Redraw` and
//! `HUD_Key_Event` carry the review's on-screen panel and answer keys
//! (`review_keys.rs`, #660).

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Threading::{CreateThread, Sleep};

use crate::pe;

/// Partial mirror of GoldSrc's `event_api_s` (see `common/event_api.h` in the
/// Half-Life SDK). Only the fields up to and including `EV_PlaySound` are
/// declared -- we never touch anything past it, and C struct layout is
/// sequential, so the offset of `EV_PlaySound` only depends on what precedes
/// it (just `version`), not on anything declared after it here.
#[repr(C)]
pub struct EventApiPartial {
    pub version: i32,
    pub ev_play_sound: unsafe extern "C" fn(
        ent: i32,
        origin: *mut f32,
        channel: i32,
        sample: *const c_char,
        volume: f32,
        attenuation: f32,
        f_flags: i32,
        pitch: i32,
    ),
    /// `EV_StopSound` .. `EV_PlayerTrace`, untouched.
    _slots_before_weapon_animation: [*mut c_void; 14],
    /// `EV_WeaponAnimation(int sequence, int body)`: what a fire handler
    /// plays on the first-person gun. At `+0x40`, confirmed by the Garand
    /// handler's `call [ecx+0x40]` at `client+0x7b34`.
    pub ev_weapon_animation: unsafe extern "C" fn(sequence: i32, body: i32),
}

/// Partial mirror of `demo_api_s` (`common/demo_api.h`): `IsRecording`, then
/// `IsPlayingback`. Only those two are declared. DoD's own `client.dll` calls
/// `[pDemoAPI+0]` and `[pDemoAPI+4]` and tests the `int` result (for example
/// at `client.dll+0x22110` and `+0x22139`), which pins both offsets.
#[repr(C)]
pub struct DemoApiPartial {
    pub is_recording: unsafe extern "C" fn() -> i32,
    pub is_playingback: unsafe extern "C" fn() -> i32,
}

/// Partial mirror of `triangleapi_s` (`common/triangleapi.h`) from `version`
/// through `WorldToScreen`. Every offset is pinned by a call in DoD's
/// `CHudScope::DrawTriApiScope` (`client+0x46690`): `RenderMode` `+0x4`,
/// `Begin` `+0x8`, `End` `+0xc`, `Color4f` `+0x10`, `TexCoord2f` `+0x18`,
/// `Vertex3fv` `+0x1c`, `Brightness` `+0x24`, `CullFace` `+0x28`,
/// `SpriteTexture` `+0x2c`; `Vertex3f` `+0x20` and `WorldToScreen` `+0x30`
/// by other client calls through the same pointer.
#[repr(C)]
pub struct TriangleApiPartial {
    pub version: i32,
    pub render_mode: unsafe extern "C" fn(mode: i32),
    pub begin: unsafe extern "C" fn(primitive: i32),
    pub end: unsafe extern "C" fn(),
    pub color4f: unsafe extern "C" fn(r: f32, g: f32, b: f32, a: f32),
    _color4ub: *mut c_void,
    pub tex_coord2f: unsafe extern "C" fn(u: f32, v: f32),
    pub vertex3fv: unsafe extern "C" fn(point: *const f32),
    pub vertex3f: unsafe extern "C" fn(x: f32, y: f32, z: f32),
    pub brightness: unsafe extern "C" fn(brightness: f32),
    pub cull_face: unsafe extern "C" fn(style: i32),
    pub sprite_texture: unsafe extern "C" fn(model: *mut ModelSPartial, frame: i32) -> i32,
    /// Writes the point's normalised screen position (-1..1, y up) and
    /// returns 1 when it is behind the eye.
    pub world_to_screen: unsafe extern "C" fn(world: *const f32, screen: *mut f32) -> i32,
}

/// `SCREENINFO` (`cdll_int.h`): `iSize`, `iWidth`, `iHeight`, `iFlags`,
/// `iCharHeight`, `charWidths[256]`. `CHud::VidInit` sets `iSize` to 0x214
/// before `pfnGetScreenInfo` (`client+0x36b2c`), which pins the size.
#[repr(C)]
pub struct ScreenInfo {
    pub size: i32,
    pub width: i32,
    pub height: i32,
    pub flags: i32,
    pub char_height: i32,
    pub char_widths: [i16; 256],
}

const _: () = assert!(size_of::<ScreenInfo>() == 0x214);

/// `gEngfuncs.pfnGetScreenInfo` (slot 12): fills `info`, whose `size` the
/// caller sets first.
pub type GetScreenInfoFn = unsafe extern "C" fn(info: *mut ScreenInfo) -> i32;

/// `gEngfuncs.pfnGetCvarPointer` (slot 72). Looks up any registered cvar by
/// name -- the engine's own included -- and returns its `cvar_s`, or null.
/// DoD's `client.dll` calls this slot 11 times with a cvar name
/// (`"r_decals"`, `"crosshair"`, `"developer"`, ...) and stores the result.
pub type GetCvarPointerFn = unsafe extern "C" fn(name: *const c_char) -> *mut CvarSPartial;

/// `gEngfuncs.pfnDrawConsoleString` (slot 27): draws `string` in the engine
/// font (`EngineFont` of the default scheme) with its top-left at `(x, y)`,
/// in the colour the last `pfnDrawSetTextColor` set, then puts the colour
/// back to `con_color`. Returns `x` plus the text's width.
pub type DrawConsoleStringFn = unsafe extern "C" fn(x: i32, y: i32, string: *const c_char) -> i32;
/// `gEngfuncs.pfnDrawSetTextColor` (slot 28): the colour for the next
/// `pfnDrawConsoleString`, each component 0.0..=1.0.
pub type DrawSetTextColorFn = unsafe extern "C" fn(r: f32, g: f32, b: f32);
/// `gEngfuncs.pfnDrawConsoleStringLen` (slot 29): the width and height
/// `pfnDrawConsoleString` would give `string`.
pub type DrawConsoleStringLenFn =
    unsafe extern "C" fn(string: *const c_char, width: *mut i32, height: *mut i32);

pub type WeaponAnimFn = unsafe extern "C" fn(sequence: i32, body: i32);
/// A client event handler, `void (*)(struct event_args_s *args)`.
pub type EventHandlerFn = unsafe extern "C" fn(args: *mut c_void);
/// `gEngfuncs.pfnHookEvent` (slot 69): registers the handler for one event
/// script. DoD's `client.dll` calls it once per script from inside
/// `Initialize` (`push handler; push "events/weapons/colt.sc"; call
/// [gEngfuncs+0x114]` at `client.dll+0x106e5`), 52 slots after the
/// `pfnAddCommand` slot its console commands go through -- the same in both
/// builds.
pub type HookEventFn = unsafe extern "C" fn(name: *const c_char, handler: EventHandlerFn);
pub type GetGameDirectoryFn = unsafe extern "C" fn(sz_get_game_dir: *mut c_char);
pub type IsSpectateOnlyFn = unsafe extern "C" fn() -> i32;
pub type GetLevelNameFn = unsafe extern "C" fn() -> *const c_char;
pub type GetViewModelFn = unsafe extern "C" fn() -> *mut ClEntityS;
/// `GetClientTime`: `cl.time`.
pub type GetClientTimeFn = unsafe extern "C" fn() -> f32;
pub type GetEntityByIndexFn = unsafe extern "C" fn(index: i32) -> *mut ClEntityS;
pub type ConsoleCommandFn = unsafe extern "C" fn();
pub type AddCommandFn = unsafe extern "C" fn(cmd_name: *const c_char, function: ConsoleCommandFn);
/// `int (*pfnHookUserMsg)(char *szMsgName, pfnUserMsgHook pfn)`.
///
/// Installs a handler for a named user message. Worth knowing before relying on
/// it: the engine does **not** overwrite an existing entry. It allocates a
/// fresh 32-byte record, `memcpy`s the matching one over it (so the message
/// number and size carry across), sets the new `pfn`, and *prepends* it to
/// `gClientUserMsgs`. The by-number dispatcher walks that list from the head
/// and stops at the first record whose number matches -- so the most recent
/// hook wins, and `client.dll`'s own handler is still reachable by calling it
/// directly. Verified in `hw.dll` at 0x1d1a830 (hook) and 0x1d1a660
/// (dispatch), pre-Anniversary. See `deathmsg.rs`.
///
/// The engine's *by-name* dispatcher (`hw.dll+0x1a7c0` pre-Anniversary,
/// `+0x1a9260` 25th Anniversary) is different: it calls **every** record
/// whose name matches, newest first, and never stops early. Only messages the
/// engine makes up itself go that way -- the `HudText` that `TE_TEXTMESSAGE`
/// turns into (`CL_HudMessage`) is the one that matters -- so a hook cannot
/// drop those: `client.dll`'s own record runs after it anyway (#677,
/// `plugin_text.rs`).
/// `gEngfuncs.GetLocalPlayer` (slot 51). Returns the local player's
/// `cl_entity_t`. Confirmed by the call site at `client.dll+0x20520`, a
/// three-instruction thunk that calls this slot and immediately
/// dereferences the result -- so the engine is expected to return something
/// readable, and `client.dll` does not check.
pub type GetLocalPlayerFn = unsafe extern "C" fn() -> *mut c_void;

/// `gEngfuncs.pfnGetPlayerInfo` (slot 21): fills a [`HudPlayerInfo`] for a
/// 1-based player slot. Confirmed by DoD's own scoreboard call at
/// `client.dll+0xb4ae`, which passes `(slot, &g_PlayerInfoList[slot])` with a
/// 0x20-byte stride -- the `hud_player_info_t` that carries `m_nSteamID`.
pub type GetPlayerInfoFn = unsafe extern "C" fn(slot: i32, info: *mut HudPlayerInfo);

/// `gEngfuncs.pfnTextMessageGet` (slot 25): the `client_textmessage_t` for a
/// `titles.txt` name, or for one of the engine's own `__NETMESSAGE__1..4`
/// that a `TE_TEXTMESSAGE` fills in. Confirmed by DoD's own five calls through
/// `gEngfuncs+0x64` (`CHudMessage` at `client.dll+0x25386`, `+0x25437`,
/// `+0x254f8`, `CHudTextMessage` at `+0x4c10f`, `+0x4c1cb`): one string
/// argument, popped by the caller, and the result's `+0x28` read as the text.
pub type TextMessageGetFn = unsafe extern "C" fn(name: *const c_char) -> *const ClientTextMessage;

/// `client_textmessage_t` (`common/cl_entity.h`'s neighbour `cdll_int.h`).
/// Only `p_message` is read. Its offset is what `client.dll` reads after
/// `pfnTextMessageGet` (`mov eax, [eax+0x28]`, `+0x4c11c`) and what the
/// engine's `TE_TEXTMESSAGE` handler writes the text pointer to (`[esi+0x28]`,
/// `hw.dll+0x23d8a` pre-Anniversary, `[edi+0x28]` at `+0x1b09f4` Anniversary).
#[repr(C)]
pub struct ClientTextMessage {
    pub effect: i32,
    pub colour1: [u8; 4],
    pub colour2: [u8; 4],
    pub x: f32,
    pub y: f32,
    pub fadein: f32,
    pub fadeout: f32,
    pub holdtime: f32,
    pub fxtime: f32,
    pub p_name: *const c_char,
    pub p_message: *const c_char,
}

#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(std::mem::offset_of!(ClientTextMessage, p_name) == 0x24);
    assert!(std::mem::offset_of!(ClientTextMessage, p_message) == 0x28);
};

/// `hud_player_info_t` (`cdll_int.h`), the Steam-era layout: 0x20 bytes, with
/// `m_nSteamID` at +0x18. The size is what DoD's `client.dll` strides its
/// own array by, so this is the layout the engine writes into.
///
/// What the engine actually fills in, read from both `hw.dll` builds
/// (pre-Anniversary +0xab70, 25th Anniversary +0x195620): an empty slot gets
/// `name = null` and `thisplayer = 0` and nothing else. Otherwise every field
/// is written, except `steam_id`, which is copied from the player's record
/// **only when the game is Counter-Strike or Condition Zero**: the two flags
/// tested (PRE `hw.dll+0xac5f`, Anniversary `+0x1956e4`) are the ones the
/// engine sets for `cstrike`/`czero` (their other readers test `cl_autobuy`,
/// `cl_rebuy`, `czero`). In DoD it is never filled; a 2026-09-30 live check
/// listed every player of an HLTV demo with 0. An earlier note here said the
/// gate was demo playback, which was wrong. Callers zero the struct first and
/// treat 0 as "unknown"; `deathmsg` reads `*sid` from the userinfo instead.
#[repr(C)]
pub struct HudPlayerInfo {
    pub name: *const c_char,
    pub ping: i16,
    /// 1 for `cl.playernum`: the recording player in a POV demo.
    pub thisplayer: u8,
    pub spectator: u8,
    pub packetloss: u8,
    pub model: *const c_char,
    pub topcolor: i16,
    pub bottomcolor: i16,
    /// SteamID64, or 0.
    pub steam_id: u64,
}

impl HudPlayerInfo {
    /// All zero: an empty slot as the engine reports it, and a `steam_id` of
    /// "unknown" until the engine overwrites it.
    pub fn empty() -> Self {
        Self {
            name: std::ptr::null(),
            ping: 0,
            thisplayer: 0,
            spectator: 0,
            packetloss: 0,
            model: std::ptr::null(),
            topcolor: 0,
            bottomcolor: 0,
            steam_id: 0,
        }
    }
}

// The layout the analysis above describes, on the only target this crate
// builds for. A 32-bit field order drift would read the model pointer as a
// SteamID rather than fail.
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<HudPlayerInfo>() == 0x20);
    assert!(std::mem::offset_of!(HudPlayerInfo, thisplayer) == 6);
    assert!(std::mem::offset_of!(HudPlayerInfo, steam_id) == 0x18);
};

/// `gEngfuncs.pfnGetCvarFloat` (slot 15). Reads a console variable's numeric
/// value by name, returning 0 for one that does not exist.
pub type GetCvarFloatFn = unsafe extern "C" fn(name: *const c_char) -> f32;

pub type HookUserMsgFn = unsafe extern "C" fn(msg_name: *const c_char, pfn: UserMsgHookFn) -> i32;
/// `int (*pfnUserMsgHook)(const char *pszName, int iSize, void *pbuf)`.
pub type UserMsgHookFn =
    unsafe extern "C" fn(name: *const c_char, size: i32, buf: *mut c_void) -> i32;
pub type ConsolePrintFn = unsafe extern "C" fn(text: *const c_char);
pub type CmdArgcFn = unsafe extern "C" fn() -> i32;
pub type CmdArgvFn = unsafe extern "C" fn(arg: i32) -> *const c_char;
pub type RegisterVariableFn = unsafe extern "C" fn(
    name: *const c_char,
    value: *const c_char,
    flags: i32,
) -> *mut CvarSPartial;

/// `cvar_s` (`common/cvardef.h`). Small, stable, and unchanged since Quake.
///
/// Only `value` is ever read; `name` exists so a registration can be checked
/// against the name it asked for, which is the cheapest possible proof that
/// this layout and the engine's agree. See `commands::install`.
#[repr(C)]
pub struct CvarSPartial {
    pub name: *const c_char,
    pub string: *const c_char,
    pub flags: i32,
    pub value: f32,
    pub next: *mut CvarSPartial,
}

// Unlike the tables above, `cvar_s` mixes pointers with 32-bit scalars, so its
// size does not scale with the pointer width and the check has to name a target.
// The DLL only ever loads into 32-bit `hl.exe`; `cargo test --workspace` builds
// this crate for the host as well, hence the gate rather than a formula.
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(
        size_of::<CvarSPartial>() == 20,
        "CvarSPartial's layout doesn't match cvardef.h (expected 20 bytes on a 32-bit build)"
    );
    // `value` is the only field read at runtime, so its offset is the one that
    // has to be right.
    assert!(std::mem::offset_of!(CvarSPartial, value) == 12);
};

impl CvarSPartial {
    /// The name the engine holds for this cvar, or `None` if the pointer is
    /// null. Borrowed from the engine's own storage, which outlives us.
    ///
    /// # Safety
    ///
    /// `self` must point at a live cvar the engine returned.
    pub unsafe fn name_str(&self) -> Option<std::borrow::Cow<'_, str>> {
        if self.name.is_null() {
            return None;
        }
        Some(unsafe { std::ffi::CStr::from_ptr(self.name) }.to_string_lossy())
    }
}

/// A 3D vector, matching `vec3_t` (`float[3]`) everywhere it's embedded in a
/// GoldSrc SDK struct below.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Exact field-for-field mirror of `entity_state_s` (`common/entity_state.h`)
/// -- unlike `ClEngineFuncsPartial`'s uniform pointer-sized slots, this
/// struct mixes byte/short/int/float/vec3 fields with real padding between
/// them, so it's transcribed with real types rather than opaque slots:
/// `#[repr(C)]` reproduces MSVC's C struct layout rules exactly, so as long
/// as every field's type and order matches the header, the offsets of
/// `sequence` and `weaponmodel` (what we actually read) come out correct
/// automatically instead of needing hand-computed byte offsets.
#[repr(C)]
pub struct EntityStateS {
    pub entity_type: i32,
    pub number: i32,
    pub msg_time: f32,
    pub messagenum: i32,
    pub origin: Vec3,
    pub angles: Vec3,
    pub modelindex: i32,
    pub sequence: i32,
    pub frame: f32,
    pub colormap: i32,
    pub skin: i16,
    pub solid: i16,
    pub effects: i32,
    pub scale: f32,
    pub eflags: u8,
    pub rendermode: i32,
    pub renderamt: i32,
    pub rendercolor: [u8; 3], // color24 { r, g, b }
    pub renderfx: i32,
    pub movetype: i32,
    pub animtime: f32,
    pub framerate: f32,
    pub body: i32,
    pub controller: [u8; 4],
    pub blending: [u8; 4],
    pub velocity: Vec3,
    pub mins: Vec3,
    pub maxs: Vec3,
    pub aiment: i32,
    pub owner: i32,
    pub friction: f32,
    pub gravity: f32,
    pub team: i32,
    pub playerclass: i32,
    pub health: i32,
    pub spectator: i32, // qboolean
    pub weaponmodel: i32,
    pub gaitsequence: i32,
    pub basevelocity: Vec3,
    pub usehull: i32,
    pub oldbuttons: i32,
    pub onground: i32,
    pub istepleft: i32,
    pub fl_fall_velocity: f32,
    pub fov: f32,
    pub weaponanim: i32,
    pub startpos: Vec3,
    pub endpos: Vec3,
    pub impacttime: f32,
    pub starttime: f32,
    pub iuser1: i32,
    pub iuser2: i32,
    pub iuser3: i32,
    pub iuser4: i32,
    pub fuser1: f32,
    pub fuser2: f32,
    pub fuser3: f32,
    pub fuser4: f32,
    pub vuser1: Vec3,
    pub vuser2: Vec3,
    pub vuser3: Vec3,
    pub vuser4: Vec3,
}

const _: () = assert!(
    size_of::<EntityStateS>() == 340,
    "EntityStateS's layout doesn't match entity_state.h (expected 340 bytes)"
);

#[repr(C)]
struct PositionHistoryT {
    animtime: f32,
    origin: Vec3,
    angles: Vec3,
}

#[repr(C)]
struct MouthT {
    mouthopen: u8,
    sndcount: u8,
    sndavg: i32,
}

#[repr(C)]
struct LatchedVarsT {
    prevanimtime: f32,
    sequencetime: f32,
    prevseqblending: [u8; 2],
    prevorigin: Vec3,
    prevangles: Vec3,
    prevsequence: i32,
    prevframe: f32,
    prevcontroller: [u8; 4],
    prevblending: [u8; 2],
}

/// Partial mirror of `cl_entity_s` (`common/cl_entity.h`), transcribed field
/// for field up through `model` -- everything after that (`efrag`, `topnode`,
/// etc.) is never touched, so it's simply not declared.
#[repr(C)]
pub struct ClEntityS {
    pub index: i32,
    pub player: i32, // qboolean
    pub baseline: EntityStateS,
    pub prevstate: EntityStateS,
    pub curstate: EntityStateS,
    current_position: i32,
    ph: [PositionHistoryT; 64],
    mouth: MouthT,
    latched: LatchedVarsT,
    lastmove: f32,
    /// Where the entity is drawn this frame. For the viewmodel,
    /// `V_CalcRefdef` sets it and the renderer reads it next.
    pub origin: Vec3,
    pub angles: Vec3,
    attachment: [Vec3; 4],
    trivial_accept: i32,
    pub model: *mut ModelSPartial,
}

/// Partial mirror of `ref_params_s` (`common/ref_params.h`) up through the
/// fields used here: the view's vectors, the frame's time, `simorg` and
/// `nextView`. Its layout is the same as a demo frame's `refparams` block,
/// which `dem-patch` reads.
#[repr(C)]
pub struct RefParamsPartial {
    pub vieworg: [f32; 3],
    pub viewangles: [f32; 3],
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub frametime: f32,
    pub time: f32,
    intermission: i32,
    paused: i32,
    spectator: i32,
    onground: i32,
    waterlevel: i32,
    simvel: [f32; 3],
    /// The client's simulated origin. The engine copies it back into
    /// `cl.simorg` after `V_CalcRefdef`, and culls an HLTV demo's entities
    /// from there (`spectator_chase_pvs`).
    pub simorg: [f32; 3],
    viewheight: [f32; 3],
    idealpitch: f32,
    cl_viewangles: [f32; 3],
    health: i32,
    crosshairangle: [f32; 3],
    viewsize: f32,
    punchangle: [f32; 3],
    maxclients: i32,
    viewentity: i32,
    playernum: i32,
    max_entities: i32,
    demoplayback: i32,
    hardware: i32,
    smoothing: i32,
    cmd: *mut c_void,
    movevars: *mut c_void,
    viewport: [i32; 4],
    /// 0 for the main view, 1 for a second view (an inset) the client asked
    /// for on the first.
    pub next_view: i32,
}

#[cfg(target_pointer_width = "32")]
const _: () = {
    // Offsets from `ref_params.h` on a 32-bit build.
    assert!(std::mem::offset_of!(RefParamsPartial, time) == 64);
    assert!(std::mem::offset_of!(RefParamsPartial, simorg) == 100);
    assert!(std::mem::offset_of!(RefParamsPartial, viewentity) == 176);
    assert!(std::mem::offset_of!(RefParamsPartial, next_view) == 224);
};

/// Partial mirror of `model_s` (`common/com_model.h`) -- `name` is its first
/// field, so this is all we need to identify a model by filename substring.
#[repr(C)]
pub struct ModelSPartial {
    pub name: [u8; 64], // MAX_MODEL_NAME
}

impl ModelSPartial {
    pub fn name_str(&self) -> std::borrow::Cow<'_, str> {
        let end = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        String::from_utf8_lossy(&self.name[..end])
    }
}

/// Exact field-for-field mirror of the studio model file header
/// (`studiohdr_t` in `engine/studio.h`) up through `seqindex` -- confirmed
/// against real DoD 1.3 `.mdl` files during the R&D phase (numseq at byte
/// offset 164, seqindex at 168).
#[repr(C)]
pub struct StudioHdrPartial {
    pub id: i32,
    pub version: i32,
    pub name: [u8; 64],
    pub length: i32,
    pub eyeposition: Vec3,
    pub min: Vec3,
    pub max: Vec3,
    pub bbmin: Vec3,
    pub bbmax: Vec3,
    pub flags: i32,
    pub numbones: i32,
    pub boneindex: i32,
    pub numbonecontrollers: i32,
    pub bonecontrollerindex: i32,
    pub numhitboxes: i32,
    pub hitboxindex: i32,
    pub numseq: i32,
    pub seqindex: i32,
}

const _: () = assert!(
    std::mem::offset_of!(StudioHdrPartial, numseq) == 164
        && std::mem::offset_of!(StudioHdrPartial, seqindex) == 168,
    "StudioHdrPartial's layout doesn't match studio.h / the confirmed .mdl offsets"
);

/// Exact field-for-field mirror of `mstudioseqdesc_t` (`engine/studio.h`) up
/// through `label` -- 176 bytes total per entry, `label` is the first 32.
#[repr(C)]
pub struct StudioSeqDescPartial {
    pub label: [u8; 32],
    /// Playback rate, and the frame count at that rate -- together they give
    /// the animation's real duration, which is how long a one-shot sequence
    /// occupies the viewmodel before it sits on its final pose.
    ///
    /// Offsets verified against the shipped models rather than assumed:
    /// `v_stick.mdl` reads back `draw` at 40fps/31 frames (0.775s), `pinpull`
    /// at 30fps/26 (0.867s) and `throw` at 40fps/2 (0.050s), which are
    /// sensible values in the right places. A wrong offset here would produce
    /// garbage durations rather than failing loudly.
    pub fps: f32,
    _flags_activity_actweight_events: [u8; 20],
    pub numframes: i32,
    _rest: [u8; 176 - 60],
}

const _: () = assert!(size_of::<StudioSeqDescPartial>() == 176);

impl StudioSeqDescPartial {
    pub fn label_str(&self) -> std::borrow::Cow<'_, str> {
        let end = self
            .label
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.label.len());
        String::from_utf8_lossy(&self.label[..end])
    }
}

/// Partial mirror of `engine_studio_api_s` (`common/r_studioint.h`) up
/// through `GetViewInfo` -- the functions this crate calls, in order.
#[repr(C)]
pub struct EngineStudioApiPartial {
    _slots_before: [*mut c_void; 3], // Mem_Calloc, Cache_Check, LoadCacheFile
    _mod_for_name: *mut c_void,
    pub mod_extradata: unsafe extern "C" fn(model: *mut ModelSPartial) -> *mut c_void,
    pub get_model_by_index: unsafe extern "C" fn(index: i32) -> *mut ModelSPartial,
    // GetCurrentEntity, PlayerInfo, GetPlayerState, GetViewEntity, GetTimes,
    // GetCvar.
    _slots_before_view_info: [*mut c_void; 6],
    /// `void GetViewInfo(float *origin, float *upv, float *rightv, float *vpnv)`:
    /// the frame's render origin and axes (`r_origin`, `vup`, `vright`, `vpn`).
    pub get_view_info: unsafe extern "C" fn(*mut f32, *mut f32, *mut f32, *mut f32),
}

impl EngineStudioApiPartial {
    /// GetCurrentEntity, PlayerInfo, GetPlayerState, GetViewEntity, GetTimes,
    /// GetCvar, in that order, untyped.
    pub fn slots_before_view_info(&self) -> &[*mut c_void; 6] {
        &self._slots_before_view_info
    }
}

/// Partial, offset-accurate mirror of `cl_enginefuncs_s` (see
/// `advancedfx/halflife`'s `engine/cdll_int.h`, whose `k_engdstNull` macro
/// lists every field of the real struct in declaration order -- confirmed
/// against it field-by-field. Every field up through `IsSpectateOnly` is
/// present, in order, so the ones we actually use (`pfn_add_command`,
/// `pfn_get_player_info`, `pfn_text_message_get`, `pfn_console_print`, `cmd_argc`,
/// `cmd_argv`, `pfn_weapon_anim`,
/// `pfn_get_game_directory`, `pfn_get_level_name`, `p_event_api`,
/// `is_spectate_only`) land at the
/// correct byte offsets; everything else is kept as an opaque, untyped slot
/// purely to hold the layout together.
///
/// `pfn_register_variable` sits at index 14, which is bracketed on both sides
/// by slots proved from real call sites in DoD's own `client.dll`: index 14
/// itself carries 111 calls (exactly the 111 cvar names recovered in
/// `docs/goldsrc_client_dll_internals.md` §5) and index 17 carries 110 (the
/// 110 command names). Two independently-confirmed anchors three slots apart
/// leave no room for the count between them to be wrong.
#[repr(C)]
pub struct ClEngineFuncsPartial {
    _slots_before_get_screen_info: [*mut c_void; 12], // pfnSPR_Load .. pfnFillRGBA
    /// Slot 12, `call [gEngfuncs+0x30]` in `CHud::VidInit` (`client+0x36b32`).
    pub pfn_get_screen_info: GetScreenInfoFn,
    _slot_set_crosshair: *mut c_void,
    pub pfn_register_variable: RegisterVariableFn,
    pub pfn_get_cvar_float: GetCvarFloatFn,
    _slots_before_add_command: [*mut c_void; 1], // pfnGetCvarString
    pub pfn_add_command: AddCommandFn,
    pub pfn_hook_user_msg: HookUserMsgFn,
    _slots_before_get_player_info: [*mut c_void; 2], // pfnServerCmd, pfnClientCmd
    /// Slot 21. See [`GetPlayerInfoFn`].
    pub pfn_get_player_info: GetPlayerInfoFn,
    _slots_before_text_message_get: [*mut c_void; 3], // pfnPlaySoundByName .. pfnAngleVectors
    /// Slot 25. See [`TextMessageGetFn`].
    pub pfn_text_message_get: TextMessageGetFn,
    _slots_before_draw_console_string: [*mut c_void; 1], // pfnDrawCharacter
    /// Slot 27. See [`DrawConsoleStringFn`]; the kill feed's names go through
    /// it (`CHudDeathNotice::Draw`, `call [gEngfuncs+0x6c]` at
    /// `client.dll+0x2b089` and `+0x2b135`) (#562).
    pub pfn_draw_console_string: DrawConsoleStringFn,
    /// Slot 28. See [`DrawSetTextColorFn`] (`call [gEngfuncs+0x70]` at
    /// `+0x2b079` and `+0x2b125`).
    pub pfn_draw_set_text_color: DrawSetTextColorFn,
    /// Slot 29. See [`DrawConsoleStringLenFn`] (`call [gEngfuncs+0x74]` at
    /// `+0x2b00d` and `+0x2b04e`).
    pub pfn_draw_console_string_len: DrawConsoleStringLenFn,
    pub pfn_console_print: ConsolePrintFn,
    _slots_before_cmd_argc: [*mut c_void; 7], // pfnCenterPrint .. Cvar_SetValue
    pub cmd_argc: CmdArgcFn,
    pub cmd_argv: CmdArgvFn,
    _slots_before_get_local_player: [*mut c_void; 11], // Con_Printf .. IsNoClipping
    pub get_local_player: GetLocalPlayerFn,
    pub get_view_model: GetViewModelFn,
    pub get_entity_by_index: GetEntityByIndexFn,
    /// Slot 54: the engine's client clock (`cl.time`), the one HUD timers run on.
    pub pfn_get_client_time: GetClientTimeFn,
    _slots_before_weapon_anim: [*mut c_void; 11], // V_CalcShake .. pfnPlaybackEvent
    pub pfn_weapon_anim: WeaponAnimFn,
    _slots_before_hook_event: [*mut c_void; 2], // pfnRandomFloat, pfnRandomLong
    /// Slot 69. See [`HookEventFn`].
    pub pfn_hook_event: HookEventFn,
    _slot_con_is_visible: *mut c_void,
    pub pfn_get_game_directory: GetGameDirectoryFn,
    /// Slot 72. See [`GetCvarPointerFn`].
    pub pfn_get_cvar_pointer: GetCvarPointerFn,
    _slot_key_lookup_binding: *mut c_void,
    /// Slot 74: `"maps/<name>.bsp"`. Confirmed in DoD's `client.dll` (7 calls
    /// through `gEngfuncs+0x128`): the one at RVA 0x26c00 prefixes the result
    /// with `"dod/"` and cuts it at the last `'.'` -- a map path.
    pub pfn_get_level_name: GetLevelNameFn,
    _slots_after_level_name: [*mut c_void; 7], // pfnGetScreenFade .. COM_FreeFile
    /// Slot 82: the client's copy sits at `gEngfuncs+0x148`, which
    /// `DrawTriApiScope` loads before every TriAPI call.
    pub p_tri_api: *mut TriangleApiPartial,
    _slot_efx_api: *mut c_void,
    pub p_event_api: *mut EventApiPartial,
    /// Slot 85. See [`DemoApiPartial`].
    pub p_demo_api: *mut DemoApiPartial,
    _slots_after_demo_api: [*mut c_void; 2], // pNetAPI, pVoiceTweak
    pub is_spectate_only: IsSpectateOnlyFn,
}

// Compile-time cross-check against the field count transcribed from
// APIProxy.h (pfnSPR_Load..IsSpectateOnly inclusive = 89 pointer-sized
// slots on a 32-bit build, i.e. 356 bytes). If this ever fails, a slot count
// above drifted from the reference header.
//
// Every field here is confirmed against a real call site in client.dll. A
// field was briefly added past this point for PlayerInfo_ValueForKey, whose
// slot could only be inferred; it is a non-nullable fn pointer, so merely
// forming a reference over it asserted that slot is non-null, with no call
// needed. Do not extend this struct past a field that cannot be verified.
const _: () = assert!(
    size_of::<ClEngineFuncsPartial>() == 89 * size_of::<usize>(),
    "ClEngineFuncsPartial's layout doesn't match APIProxy.h's field count (expected 89 pointer-sized slots)"
);

static ENGFUNCS: AtomicPtr<ClEngineFuncsPartial> = AtomicPtr::new(std::ptr::null_mut());
static ENGINE_STUDIO: AtomicPtr<EngineStudioApiPartial> = AtomicPtr::new(std::ptr::null_mut());

/// `client.dll`'s load address, once the engine has loaded it. `None` before
/// that.
///
/// Every address in `deathmsg.rs` is recorded as an RVA against the analysed
/// image and rebased through this, rather than assumed: `client.dll` opts out
/// of ASLR so it usually lands at its preferred 0x1900000, but a base conflict
/// still relocates it, and all 34 of the death-notice call sites carry base
/// relocations that the loader would have rewritten.
pub fn client_module_base() -> Option<usize> {
    let base = CLIENT_DLL.load(Ordering::Acquire) as usize;
    (base != 0).then_some(base)
}

/// The loaded engine's base address, or `None` before `hw.dll` exists.
///
/// `hw.dll` is loaded once and stays for the session, so asking the loader
/// each time is both correct and cheap. (`client.dll` is tracked through the
/// IAT hook instead; it does not reload between demos either, but whatever
/// else might reload it is untested -- see CLAUDE.md.)
pub fn engine_module_base() -> Option<usize> {
    let name = c"hw.dll";
    let handle = unsafe { GetModuleHandleA(name.as_ptr() as *const u8) };
    (!handle.is_null()).then_some(handle as usize)
}

/// One of the engine's or vgui2's interface singletons, by module and
/// version string (`VGUI_Surface026` from `hw.dll`, `VGUI_Panel007` from
/// `vgui2.dll`), through the module's exported `CreateInterface`. `None`
/// while the module isn't loaded or doesn't serve that name.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub fn create_interface(module: &std::ffi::CStr, name: &std::ffi::CStr) -> Option<*mut c_void> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    let handle = unsafe { GetModuleHandleA(module.as_ptr() as *const u8) };
    if handle.is_null() {
        return None;
    }
    let create = unsafe { GetProcAddress(handle, c"CreateInterface".as_ptr() as *const u8) }?;
    // Safety: the interface factory's signature, the same on every Valve DLL.
    let create: CreateInterfaceFn = unsafe { std::mem::transmute(create) };
    let object = unsafe { create(name.as_ptr(), std::ptr::null_mut()) };
    (!object.is_null()).then_some(object)
}

/// Returns the captured engine function table, once `client.dll` has loaded
/// and been successfully signature-scanned. `None` before that.
pub fn engfuncs() -> Option<&'static ClEngineFuncsPartial> {
    let ptr = ENGFUNCS.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // Safety: only ever set once, from `capture_via_scan`, to a pointer
        // the engine itself owns for the life of the client.dll instance.
        Some(unsafe { &*ptr })
    }
}

/// Returns the captured studio-model engine interface, once `client.dll` has
/// loaded and been successfully signature-scanned. `None` before that (or if
/// the scan found `pEngfuncs` but not `pstudio`).
pub fn engine_studio() -> Option<&'static EngineStudioApiPartial> {
    let ptr = ENGINE_STUDIO.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // Safety: same guarantee as `engfuncs()` above.
        Some(unsafe { &*ptr })
    }
}

/// Registers `callback` to run once per rendered frame.
///
/// Normally driven by the real `HUD_Frame` trampoline. A ~60Hz timer thread
/// is started as a safety net for the case where the `HUD_Frame` hook never
/// lands (an engine build that resolves it by some path we don't intercept).
/// It stands by while the hook is in the client's table, so the callbacks run
/// on the engine thread only, and retires once a real frame arrives.
///
/// Standing by matters: the callbacks patch `client.dll` code, and two threads
/// patching the same byte crash the game. On the first frame the settings.cfg
/// cvars flip, the timer thread and `HUD_Frame` both saw it, and one thread's
/// `VirtualProtect` restore landed between the other's unprotect and its
/// write (`spectator_eye`, `spectator_follow`: an access violation writing
/// `client.dll` code, about 1 launch in 10).
static PER_FRAME_CALLBACK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();
static TIMER_THREAD_STARTED: AtomicBool = AtomicBool::new(false);
/// Set by the `HUD_Frame` trampoline the first time the engine actually
/// renders a frame through us; retires the fallback timer thread.
static HUD_FRAME_DRIVING: AtomicBool = AtomicBool::new(false);

/// Whether the per-frame callbacks run on the engine's own thread (from
/// `HUD_Frame`), so they may call into VGUI, rather than on the fallback timer
/// thread.
pub(crate) fn hud_frame_driving() -> bool {
    HUD_FRAME_DRIVING.load(Ordering::Acquire)
}
/// Held while the per-frame callbacks run, so the timer thread and
/// `HUD_Frame` can never run them at the same time, even in the moment a late
/// hook takes over. A try-lock: whoever finds it taken skips that frame
/// instead of waiting, since this is the frame path.
static PER_FRAME_RUNNING: AtomicBool = AtomicBool::new(false);

/// A turn at running the per-frame callbacks; the flag clears when it drops.
struct PerFrameTurn<'a>(&'a AtomicBool);

impl<'a> PerFrameTurn<'a> {
    fn try_take(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for PerFrameTurn<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// What the fallback timer thread does on one tick.
#[derive(Debug, PartialEq, Eq)]
enum TimerStep {
    /// A real `HUD_Frame` has run: the hook drives from here on.
    Retire,
    /// The hook is in the client's table, so the engine will call it: leave
    /// the callbacks to the engine thread.
    StandBy,
    /// No hook: this thread is the only thing driving the callbacks.
    Drive,
}

fn timer_step(hud_frame_driving: bool, hud_frame_hooked: bool) -> TimerStep {
    if hud_frame_driving {
        TimerStep::Retire
    } else if hud_frame_hooked {
        TimerStep::StandBy
    } else {
        TimerStep::Drive
    }
}

fn current_timer_step() -> TimerStep {
    timer_step(
        HUD_FRAME_DRIVING.load(Ordering::Acquire),
        !REAL_HUD_FRAME.load(Ordering::Acquire).is_null(),
    )
}

/// Runs immediately before the main per-frame callback.
///
/// Exists so the cvar poll can write the flags `anim_fix::apply()` then reads
/// on the same frame, without either module having to know about the other.
/// Separate `OnceLock`s rather than a list because this is the frame path:
/// no allocation, no locking.
static PER_FRAME_PROLOGUE: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

pub fn set_per_frame_prologue(callback: fn()) {
    let _ = PER_FRAME_PROLOGUE.set(callback);
}

/// Whether something has called `set_per_frame_prologue` yet -- issue #324's
/// regression test needs to observe this without a real engine to drive
/// `run_per_frame_callback` itself.
#[cfg(test)]
pub(crate) fn per_frame_prologue_is_set() -> bool {
    PER_FRAME_PROLOGUE.get().is_some()
}

pub fn set_per_frame_callback(callback: fn()) {
    let _ = PER_FRAME_CALLBACK.set(callback);
    if !TIMER_THREAD_STARTED.swap(true, Ordering::AcqRel) {
        unsafe {
            CreateThread(
                std::ptr::null(),
                0,
                Some(per_frame_timer_thread),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            );
        }
    }
}

fn run_per_frame_callback() {
    if let Some(prologue) = PER_FRAME_PROLOGUE.get() {
        prologue();
    }
    if let Some(callback) = PER_FRAME_CALLBACK.get() {
        callback();
    }
}

unsafe extern "system" fn per_frame_timer_thread(_lp_param: *mut c_void) -> u32 {
    loop {
        unsafe { Sleep(16) };
        match current_timer_step() {
            TimerStep::Retire => {
                unsafe {
                    crate::debug::report(
                        "per-frame: real HUD_Frame hook is driving, retiring the fallback timer thread",
                    )
                };
                return 0;
            }
            TimerStep::StandBy => {}
            TimerStep::Drive => {
                let Some(_turn) = PerFrameTurn::try_take(&PER_FRAME_RUNNING) else {
                    continue;
                };
                // Asked again holding the turn: a hook that landed since the
                // first look gets the frame instead.
                if current_timer_step() == TimerStep::Drive {
                    run_per_frame_callback();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// client.dll's `cldll_func_t` table
// ---------------------------------------------------------------------------

/// Number of function-pointer slots the secured `F` export writes into the
/// buffer the engine hands it. Confirmed two independent ways: by
/// disassembling DoD 1.3's own `F` (`rep movsd` with `ecx = 0x2b` = 43), and
/// against Xash3D's `cldll_func_src_t`, whose 43 members line up name-for-name
/// with the 43 addresses `F` writes (each of which resolves back to the
/// identically-named export in `client.dll`).
const CLDLL_FUNC_SLOTS: usize = 43;

/// Slot indices within that table -- verified by resolving every address DoD's
/// `F` writes back to its own export name.
const SLOT_INITIALIZE: usize = 0;
/// Checked against both installs' `client.dll` (#660): `F` writes
/// `HUD_Redraw` here and `HUD_Key_Event` at slot 34. `hw.dll` calls the first
/// as `(float time, int intermission)` (pre-Anniversary `+0xbec9`, 25th
/// Anniversary `+0x196f5a`) and the second as `(down, key, binding)` from
/// `ClientDLL_Key_Event` (`+0xb98f`, `+0x1971d4`).
const SLOT_HUD_REDRAW: usize = 3;
/// `V_CalcRefdef`, the slot before `HUD_AddEntity` in `cldll_func_t`
/// (`pfnCalcRefdef` in Xash3D's `cldll_func_src_t`).
const SLOT_CALC_REFDEF: usize = 19;
const SLOT_HUD_ADD_ENTITY: usize = 20;
/// Checked 2026-10-11: `F` stores `HUD_DrawTransparentTriangles`
/// (`+0x4c830`) at its table's slot 23.
const SLOT_DRAW_TRANSPARENT_TRIANGLES: usize = 23;
/// `F` stores `HUD_Shutdown` (`+0x2a570`) at slot 26.
const SLOT_HUD_SHUTDOWN: usize = 26;
const SLOT_HUD_FRAME: usize = 33;
const SLOT_HUD_KEY_EVENT: usize = 34;
/// Re-checked 2026-09-29 against both installs' `client.dll` (byte-identical):
/// `F` writes `HUD_DirectorMessage` (`+0x2a6a0`) here.
const SLOT_HUD_DIRECTOR_MESSAGE: usize = 38;
const SLOT_GET_STUDIO_MODEL_INTERFACE: usize = 39;

const _: () = assert!(
    SLOT_INITIALIZE < CLDLL_FUNC_SLOTS
        && SLOT_CALC_REFDEF < CLDLL_FUNC_SLOTS
        && SLOT_HUD_ADD_ENTITY < CLDLL_FUNC_SLOTS
        && SLOT_DRAW_TRANSPARENT_TRIANGLES < CLDLL_FUNC_SLOTS
        && SLOT_HUD_SHUTDOWN < CLDLL_FUNC_SLOTS
        && SLOT_HUD_FRAME < CLDLL_FUNC_SLOTS
        && SLOT_HUD_REDRAW < CLDLL_FUNC_SLOTS
        && SLOT_HUD_KEY_EVENT < CLDLL_FUNC_SLOTS
        && SLOT_HUD_DIRECTOR_MESSAGE < CLDLL_FUNC_SLOTS
        && SLOT_GET_STUDIO_MODEL_INTERFACE < CLDLL_FUNC_SLOTS,
    "a cldll_func_t slot index is outside the table F actually writes"
);

type InitializeFn = unsafe extern "C" fn(*mut ClEngineFuncsPartial, i32) -> i32;
type HudFrameFn = unsafe extern "C" fn(f64);
/// `int HUD_Redraw(float time, int intermission)`.
type HudRedrawFn = unsafe extern "C" fn(f32, i32) -> i32;
/// `int HUD_Key_Event(int down, int keynum, const char *binding)`: 0
/// swallows the key, so the engine runs no bind for it.
type HudKeyEventFn = unsafe extern "C" fn(i32, i32, *const c_char) -> i32;
/// `void V_CalcRefdef(struct ref_params_s *pparams)`.
type CalcRefdefFn = unsafe extern "C" fn(*mut RefParamsPartial);
type GetStudioModelInterfaceFn =
    unsafe extern "C" fn(i32, *mut *mut c_void, *mut EngineStudioApiPartial) -> i32;
/// `int (*pHudAddEntity)(int type, cl_entity_t *ent, const char *modelname)`.
/// `ent` is passed through opaquely -- nothing here reads its fields, only
/// `modelname`, so there's no need to model `cl_entity_t`'s own layout.
type HudAddEntityFn = unsafe extern "C" fn(i32, *mut c_void, *const c_char) -> i32;
/// `void HUD_DrawTransparentTriangles(void)`.
type DrawTransparentTrianglesFn = unsafe extern "C" fn();
/// `void HUD_Shutdown(void)`.
type HudShutdownFn = unsafe extern "C" fn();
/// `void (*pfnDirectorMessage)(int iSize, void *pbuf)`: one `svc_director`
/// message, with the opcode and length byte already stripped.
type HudDirectorMessageFn = unsafe extern "C" fn(i32, *mut c_void);
/// The secured single-callback export: fills the caller-provided buffer with
/// `CLDLL_FUNC_SLOTS` function pointers. `__cdecl`, one pointer argument --
/// confirmed from `hw.dll`'s call site (`push edx; call eax; add esp, 4`).
type ClientApiFn = unsafe extern "C" fn(*mut *mut c_void);

static REAL_F: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_INITIALIZE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_HUD_ADD_ENTITY: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_DRAW_TRANSPARENT_TRIANGLES: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_HUD_SHUTDOWN: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_CALC_REFDEF: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_HUD_FRAME: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_HUD_REDRAW: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_HUD_KEY_EVENT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_HUD_DIRECTOR_MESSAGE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_GET_STUDIO_MODEL_INTERFACE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Runs once, on the engine's own thread, immediately after `client.dll`'s
/// real `Initialize` returns -- see `set_on_engine_ready`.
static ON_ENGINE_READY: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Registers the work that must happen once `pEngfuncs` is live.
///
/// This exists so that work runs **on the engine thread, at a deterministic
/// point**, rather than from our worker thread whenever it happens to notice.
/// It matters because that work mutates engine-owned global state --
/// `pfnAddCommand` prepends to the engine's command list, and `fire_sounds`
/// overwrites a function pointer inside the live `event_api_s` -- none of
/// which is thread-safe against an engine that may be running concurrently.
/// Registering right after `Initialize` also matches where DoD's own client
/// registers its 110 console commands (from `HUD_Init`, immediately after).
///
/// Must be called before `install()`, so there is no window in which
/// `Initialize` could fire before the callback is set.
pub fn set_on_engine_ready(callback: fn()) {
    let _ = ON_ENGINE_READY.set(callback);
}

/// Captures `pEnginefuncs` -- the whole reason this crate exists -- then hands
/// straight off to `client.dll`'s real `Initialize` so the game is unaffected.
unsafe extern "C" fn tramp_initialize(engfuncs: *mut ClEngineFuncsPartial, version: i32) -> i32 {
    if !engfuncs.is_null() {
        ENGFUNCS.store(engfuncs, Ordering::Release);
        unsafe {
            crate::debug::report(&format!(
                "Initialize: captured pEngfuncs at {engfuncs:p} (interface version {version})"
            ))
        };
    } else {
        unsafe {
            crate::debug::report(
                "Initialize: engine passed a null pEnginefuncs -- nothing captured",
            )
        };
    }

    let real = REAL_INITIALIZE.load(Ordering::Acquire);
    if real.is_null() {
        // Can't happen (we only ever install the trampoline after storing the
        // real pointer), but returning "wrong interface version" is the
        // engine's own documented failure signal, so fail visibly, not weirdly.
        return 0;
    }
    let real: InitializeFn = unsafe { std::mem::transmute(real) };
    // Before the client copies the table: high-quality overview tiles.
    unsafe { crate::overview_hd::wrap(engfuncs as *mut std::ffi::c_void) };
    let result = if engfuncs.is_null() {
        unsafe { real(engfuncs, version) }
    } else {
        // The client registers its event handlers from inside Initialize;
        // missing_shots notes the weapon ones as they go past.
        unsafe { crate::missing_shots::noting_event_handlers(engfuncs, || real(engfuncs, version)) }
    };

    // Only once the client is genuinely initialised, and only if we actually
    // captured the table -- installing against a null pEngfuncs would crash
    // the game rather than merely fail.
    // gEngfuncs is only populated once the real Initialize has run, so this
    // has to come after it.
    unsafe { watch_client_commands() };
    // The HUD loads its sheets (`SPR_Load`) at the first VidInit, right after
    // this, so the hooks that map handles to paths go in now, not on the
    // first `ds_map_hide_asset add` (#808).
    crate::hud_sprites::install();

    if !engfuncs.is_null()
        && let Some(callback) = ON_ENGINE_READY.get()
    {
        callback();
    }

    result
}

/// Real per-frame tick. Runs `client.dll`'s own `HUD_Frame` first so our
/// callback observes the state the engine just finished producing.
/// Seconds of playback elapsed, as raw f64 bits, summed from the per-frame
/// deltas `HUD_Frame` is passed.
///
/// Free: the engine already passes the delta every frame and it was otherwise
/// discarded. Playback time is what a log line has to be matched against
/// something seen on screen -- wall-clock time cannot be, once playback is
/// paused, seeked or fast-forwarded.
///
/// The value `HUD_Frame(double time)` receives is the frame's *duration*
/// (`host_frametime`, 6-11ms at ordinary framerates), **not** a clock. Storing
/// it directly was a bug: every log line read `[demo 0.006]` no matter how far
/// into the demo it was, and `anim_fix`'s fire-dedup window ended up comparing
/// two frame durations, which put it permanently inside its own window and
/// suppressed every firing animation. Summing the deltas gives the elapsed
/// reading both uses actually wanted.
///
/// Elapsed since the client loaded rather than since the current demo started:
/// loading a second demo in the same session does not reset it, because the
/// engine gives no clean signal to reset on. Monotonic within a session, which
/// is all either caller needs.
static DEMO_ELAPSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The engine's own client clock (`cl.time`), which the game's HUD timers
/// (kill feed, centre text, the clan countdown) run on; `None` before the
/// engine is ready.
pub fn engine_client_time() -> Option<f32> {
    let engfuncs = engfuncs()?;
    // Safety: the engine's own accessor, called on its thread.
    Some(unsafe { (engfuncs.pfn_get_client_time)() })
}

/// Seconds of playback elapsed, or 0 before the first frame.
pub fn client_time() -> f64 {
    f64::from_bits(DEMO_ELAPSED.load(Ordering::Relaxed))
}

unsafe extern "C" fn tramp_hud_frame(time: f64) {
    // Only ever called from the engine thread, so a plain load/store pair needs
    // no stronger ordering than this.
    let elapsed = f64::from_bits(DEMO_ELAPSED.load(Ordering::Relaxed));
    // Guard against a nonsense delta rather than letting it poison the running
    // total permanently: a hitch, a breakpoint or a paused demo can hand back a
    // very large frametime, and NaN would make every later comparison false.
    if time.is_finite() && time > 0.0 {
        DEMO_ELAPSED.store((elapsed + time.min(1.0)).to_bits(), Ordering::Relaxed);
    }

    let real = REAL_HUD_FRAME.load(Ordering::Acquire);
    if !real.is_null() {
        let real: HudFrameFn = unsafe { std::mem::transmute(real) };
        unsafe { real(time) };
    }

    if !HUD_FRAME_DRIVING.swap(true, Ordering::AcqRel) {
        unsafe { crate::debug::report("HUD_Frame: first real per-frame callback received") };
    }
    // Taken only if the timer thread is mid-run, which can happen once, on the
    // frame a late hook takes over; that frame's callbacks are skipped.
    if let Some(_turn) = PerFrameTurn::try_take(&PER_FRAME_RUNNING) {
        run_per_frame_callback();
    }
}

// ---------------------------------------------------------------------------
// Watching what client.dll tells the engine to do
// ---------------------------------------------------------------------------

/// `pfnClientCmd`'s index in `cl_enginefunc_t`, verified against the binary --
/// DoD's client calls this slot 12 times.
const ENGFUNCS_SLOT_CLIENT_CMD: usize = 20;

type ClientCmdFn = unsafe extern "C" fn(*const c_char);
static REAL_CLIENT_CMD: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Runs `line` as a console command through the engine's `pfnClientCmd`, as
/// if `client.dll` had issued it. Main thread only, like every engine call.
/// False before the engine's function table is captured.
// Only 32-bit code (`remote`, `demo_reload`) calls it.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) fn client_cmd(line: &std::ffi::CStr) -> bool {
    let Some(engfuncs) = engfuncs() else {
        return false;
    };
    // Safety: a slot of the engine's function table, the one this file already
    // redirects for its client-cmd watch.
    unsafe {
        let slot = *(engfuncs as *const ClEngineFuncsPartial as *const usize)
            .add(ENGFUNCS_SLOT_CLIENT_CMD);
        if slot == 0 {
            return false;
        }
        let client_cmd: ClientCmdFn = std::mem::transmute(slot);
        client_cmd(line.as_ptr());
    }
    true
}

/// Logs every console command `client.dll` issues to itself.
///
/// The motivating case is DoD's own cvar-enforcement routine, which runs from
/// `CHud::Redraw` -- every rendered frame -- and responds to `r_drawentities`
/// or `cl_lw` not being 1 by forcing the value back, printing a warning, and
/// then issuing `quit`. It assembles that `quit` byte-by-byte on the stack
/// rather than storing it as a literal, so nothing in the binary's strings
/// gives it away, and if the process exits before `qconsole.log` flushes there
/// is no trace at all. Catching it here is the only reliable way to tell "the
/// game quit on purpose" from "the game crashed". See issue #205.
unsafe extern "C" fn hook_client_cmd(text: *const c_char) {
    if !text.is_null() {
        let raw = unsafe { std::ffi::CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("quit") {
            unsafe {
                crate::debug::report(
                    "!!! client.dll issued 'quit' -- DoD is closing itself deliberately, not crashing. \
                     Its cvar enforcement does this when r_drawentities or cl_lw is not 1 (issue #205); \
                     the preceding console line names which one.",
                )
            };
        } else {
            unsafe {
                crate::debug::report(&format!(
                    "client.dll ClientCmd: \"{}\"",
                    trimmed.escape_debug()
                ))
            };
        }
    }

    let real = REAL_CLIENT_CMD.load(Ordering::Acquire);
    if !real.is_null() {
        let real: ClientCmdFn = unsafe { std::mem::transmute(real) };
        unsafe { real(text) };
    }
}

/// Finds `client.dll`'s own copy of `cl_enginefunc_t` by reading it straight
/// out of the real `Initialize`, which begins by memcpy-ing the engine's table
/// into a fixed global:
///
/// ```text
///   b9 87 00 00 00     mov ecx, 0x87        ; 135 dwords = sizeof(cl_enginefunc_t)
///   ...
///   bf <imm32>         mov edi, gEngfuncs   ; <- the address we want
///   50                 push eax
///   f3 a5              rep movsd
/// ```
///
/// The immediate is read from the *loaded* module, so the loader has already
/// applied relocations to it and it is the runtime address.
///
/// Safety: `real_initialize` must point at the real, mapped `Initialize`.
unsafe fn find_gengfuncs(real_initialize: *const u8) -> Option<*mut *mut c_void> {
    let window = unsafe { std::slice::from_raw_parts(real_initialize, 0x60) };
    for k in 0..window.len().saturating_sub(8) {
        if window[k] == 0xBF
            && window[k + 5] == 0x50
            && window[k + 6] == 0xF3
            && window[k + 7] == 0xA5
        {
            let addr =
                u32::from_le_bytes([window[k + 1], window[k + 2], window[k + 3], window[k + 4]])
                    as usize;
            // Must land inside client.dll's own image, or the pattern matched
            // something that only looked like the prologue.
            let base = CLIENT_DLL.load(Ordering::Acquire) as usize;
            if base != 0 && addr > base && addr < base + 0x0400_0000 {
                return Some(addr as *mut *mut c_void);
            }
        }
    }
    None
}

/// `client.dll`'s own `cl_enginefunc_t` copy, once the real `Initialize` is
/// known (see [`find_gengfuncs`]); `hud_sprites` patches its `SPR_*`
/// pointers the way [`watch_client_commands`] patches `pfnClientCmd`.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) fn client_gengfuncs() -> Option<*mut *mut c_void> {
    let real_initialize = REAL_INITIALIZE.load(Ordering::Acquire) as *const u8;
    if real_initialize.is_null() {
        return None;
    }
    unsafe { find_gengfuncs(real_initialize) }
}

/// Writes `hook` into slot `slot` of client.dll's `gEngfuncs` copy and
/// returns what was there.
///
/// # Safety
/// `gengfuncs` is the live copy from [`client_gengfuncs`], and `hook` has
/// the slot's signature.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) unsafe fn patch_gengfuncs_slot(
    gengfuncs: *mut *mut c_void,
    slot: usize,
    hook: *mut c_void,
) -> Result<*mut c_void, String> {
    unsafe {
        let at = gengfuncs.add(slot);
        let was = *at;
        if was.is_null() {
            return Err(format!("slot {slot} is null"));
        }
        if !crate::patch::write_pointer(at, hook, crate::patch::After::Restore) {
            return Err(format!("slot {slot} could not be made writable"));
        }
        Ok(was)
    }
}

/// Redirects `client.dll`'s own `pfnClientCmd` through `hook_client_cmd`.
///
/// This patches one function pointer inside a struct client.dll owns, rather
/// than swapping a `cldll_func_t` slot the engine also uses -- deliberately,
/// after hooking `V_CalcRefdef` that way closed the game.
unsafe fn watch_client_commands() {
    let real_initialize = REAL_INITIALIZE.load(Ordering::Acquire) as *const u8;
    if real_initialize.is_null() {
        return;
    }
    let Some(gengfuncs) = (unsafe { find_gengfuncs(real_initialize) }) else {
        unsafe {
            crate::debug::report(
                "client-cmd watch: couldn't locate client.dll's gEngfuncs copy; not watching",
            )
        };
        return;
    };

    let slot = unsafe { gengfuncs.add(ENGFUNCS_SLOT_CLIENT_CMD) };
    let real = unsafe { *slot };
    if real.is_null() {
        unsafe {
            crate::debug::report("client-cmd watch: pfnClientCmd slot is null; not watching")
        };
        return;
    }
    REAL_CLIENT_CMD.store(real, Ordering::Release);

    unsafe {
        if !crate::patch::write_pointer(
            slot,
            hook_client_cmd as *mut c_void,
            crate::patch::After::Restore,
        ) {
            crate::debug::report("client-cmd watch: couldn't make the slot writable; not watching");
            return;
        }
        crate::debug::report(&format!(
            "client-cmd watch: gEngfuncs at {gengfuncs:p}, pfnClientCmd was {real:p} -- now logging what client.dll runs"
        ));
    }
}

/// Captures `pstudio`, which `anim_fix` needs to read a model's sequence
/// labels.
unsafe extern "C" fn tramp_get_studio_model_interface(
    version: i32,
    ppinterface: *mut *mut c_void,
    pstudio: *mut EngineStudioApiPartial,
) -> i32 {
    if !pstudio.is_null() {
        ENGINE_STUDIO.store(pstudio, Ordering::Release);
        unsafe {
            crate::debug::report(&format!(
                "HUD_GetStudioModelInterface: captured pstudio at {pstudio:p} (studio interface version {version})"
            ))
        };
    }

    let real = REAL_GET_STUDIO_MODEL_INTERFACE.load(Ordering::Acquire);
    if real.is_null() {
        return 0;
    }
    let real: GetStudioModelInterfaceFn = unsafe { std::mem::transmute(real) };
    let result = unsafe { real(version, ppinterface, pstudio) };
    if result != 0 && !ppinterface.is_null() {
        // Safety: the client just filled it with its renderer interface,
        // a static that lives as long as client.dll.
        unsafe { crate::strafe_twitch::install(*ppinterface as *mut *mut c_void) };
    }
    result
}

/// Runs `client.dll`'s own `V_CalcRefdef`, then puts a chase camera's
/// position where the engine culls an HLTV demo's entities from
/// (`spectator_chase_pvs`), lets the spectated view
/// replace the camera for a dead player (`spectator_deathcam`, before the
/// position is recorded, so the recorded view is the drawn one), adjust the
/// viewmodel it just placed (`spectator_gun`) and keep its live bob for a POV
/// demo (`pov_live_bob`).
unsafe extern "C" fn tramp_calc_refdef(pparams: *mut RefParamsPartial) {
    let real = REAL_CALC_REFDEF.load(Ordering::Acquire);
    if real.is_null() {
        return;
    }
    let real: CalcRefdefFn = unsafe { std::mem::transmute(real) };
    let main_view = unsafe { crate::spectator_chase_pvs::is_main_view(pparams) };
    unsafe { real(pparams) };
    unsafe { crate::spectator_chase_pvs::after_calc_refdef(pparams, main_view) };
    unsafe { crate::spectator_deathcam::after_calc_refdef(pparams) };
    if !pparams.is_null() {
        // Safety: the engine's ref_params_s for this view, valid for the call.
        crate::position::record(unsafe { &*pparams });
    }
    unsafe { crate::spectator_gun::after_calc_refdef(pparams) };
    crate::screen_effects::after_calc_refdef();
    crate::pov_live_bob::after_calc_refdef(pparams);
}

/// Runs `client.dll`'s own `HUD_DrawTransparentTriangles` (where the sniper
/// scope is drawn), after `demo_scope` has read the frame's render view, and
/// with its black fill around the scope (#799).
unsafe extern "C" fn tramp_draw_transparent_triangles() {
    let real = REAL_DRAW_TRANSPARENT_TRIANGLES.load(Ordering::Acquire);
    if real.is_null() {
        return;
    }
    crate::demo_scope::before_transparent_triangles();
    crate::pov_live_bob::before_view_model();
    let real: DrawTransparentTrianglesFn = unsafe { std::mem::transmute(real) };
    // Safety: the client's own export, called on the engine's thread.
    crate::demo_scope::with_black_fill(|| unsafe { real() });
}

/// Runs `client.dll`'s own `HUD_Redraw`, then draws the review's panel
/// over the HUD (#660).
unsafe extern "C" fn tramp_hud_redraw(time: f32, intermission: i32) -> i32 {
    let real = REAL_HUD_REDRAW.load(Ordering::Acquire);
    let result = if real.is_null() {
        1
    } else {
        let real: HudRedrawFn = unsafe { std::mem::transmute(real) };
        // Safety: the client's own export, called on the engine's thread.
        unsafe { real(time, intermission) }
    };
    // Safety: on the engine's thread, inside HUD_Redraw.
    unsafe { crate::review_keys::draw() };
    result
}

/// A key the engine is about to run the bind of, only while keys go to the
/// game (never the console, chat or a window's box). The review's answer
/// keys take theirs here (#660); everything else goes to the client as is.
unsafe extern "C" fn tramp_hud_key_event(down: i32, key: i32, binding: *const c_char) -> i32 {
    if crate::review_keys::key_event(down, key) {
        return 0;
    }
    let real = REAL_HUD_KEY_EVENT.load(Ordering::Acquire);
    if real.is_null() {
        return 1;
    }
    let real: HudKeyEventFn = unsafe { std::mem::transmute(real) };
    // Safety: the client's own export, with the engine's own arguments.
    unsafe { real(down, key, binding) }
}

/// Called once per entity the engine is about to add to the render list.
/// Returning 0 suppresses that one entity; see `hide_asset.rs`'s module doc
/// for the evidence behind that contract and why it isn't patched.
unsafe extern "C" fn tramp_hud_add_entity(
    entity_type: i32,
    ent: *mut c_void,
    modelname: *const c_char,
) -> i32 {
    if !modelname.is_null() {
        let name = unsafe { std::ffi::CStr::from_ptr(modelname) }.to_string_lossy();
        // Type 2 is ET_TEMPENTITY: the engine's tempent callback adds every
        // temporary effect through here too (#614).
        let drawn_as = if entity_type == 2 {
            crate::hide_asset::DrawnAs::Effect
        } else {
            crate::hide_asset::DrawnAs::Entity
        };
        if crate::hide_asset::should_hide(&name, drawn_as) {
            return 0;
        }
    }

    let real = REAL_HUD_ADD_ENTITY.load(Ordering::Acquire);
    if real.is_null() {
        // Can't tell the entity apart from one that should draw, so the safe
        // side to fail on is the one that changes nothing: let it through.
        return 1;
    }
    let real: HudAddEntityFn = unsafe { std::mem::transmute(real) };
    unsafe { real(entity_type, ent, modelname) }
}

/// Set once `client.dll`'s `HUD_Shutdown` has run: the game is quitting.
static CLIENT_SHUT_DOWN: AtomicBool = AtomicBool::new(false);

/// Whether the client has shut down. Once it has, the engine frees every
/// cvar the client side registered, ours included, before SDL takes the
/// window down, so a cvar pointer must not be read any more: `ShowWindow`
/// reaching `behind_imports` read a freed `ds_capture_stay_behind` and
/// crashed the game as it quit (#1080).
pub fn client_shut_down() -> bool {
    CLIENT_SHUT_DOWN.load(Ordering::Acquire)
}

/// Marks the client shut down before `client.dll`'s own `HUD_Shutdown` runs,
/// so nothing reads our cvars past this point.
unsafe extern "C" fn tramp_hud_shutdown() {
    if !CLIENT_SHUT_DOWN.swap(true, Ordering::AcqRel) {
        unsafe {
            crate::debug::report("HUD_Shutdown: the game is quitting; our cvars are no longer read")
        };
    }
    let real = REAL_HUD_SHUTDOWN.load(Ordering::Acquire);
    if !real.is_null() {
        let real: HudShutdownFn = unsafe { std::mem::transmute(real) };
        unsafe { real() };
    }
}

/// Called once per `svc_director` message. Drops the ones
/// `hltv_messages::should_drop` asks for and forwards the rest unchanged.
unsafe extern "C" fn tramp_hud_director_message(size: i32, buf: *mut c_void) {
    if size > 0 && !buf.is_null() {
        // Safety: the engine's own buffer of `size` bytes, alive for the call.
        let bytes = unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) };
        if crate::hltv_messages::should_drop(bytes) {
            return;
        }
    }

    let real = REAL_HUD_DIRECTOR_MESSAGE.load(Ordering::Acquire);
    if real.is_null() {
        return;
    }
    let real: HudDirectorMessageFn = unsafe { std::mem::transmute(real) };
    unsafe { real(size, buf) }
}

/// Swaps one slot of the `cldll_func_t` table `F` just filled for our own
/// trampoline, stashing the real pointer so the trampoline can chain to it.
///
/// Safety: `table` must point to at least `CLDLL_FUNC_SLOTS` writable
/// pointer-sized slots (guaranteed by `F`'s own contract -- it just wrote
/// exactly that many).
unsafe fn swap_slot(
    table: *mut *mut c_void,
    slot: usize,
    real_store: &AtomicPtr<c_void>,
    trampoline: *mut c_void,
    name: &str,
) {
    let entry = unsafe { table.add(slot) };
    let real = unsafe { *entry };
    if real.is_null() {
        unsafe {
            crate::debug::report(&format!(
                "F: slot {slot} ({name}) is null -- leaving it alone"
            ))
        };
        return;
    }
    real_store.store(real, Ordering::Release);
    unsafe { *entry = trampoline };
    unsafe {
        crate::debug::report(&format!(
            "F: hooked slot {slot} ({name}), real implementation at {real:p}"
        ))
    };
}

/// Our stand-in for `client.dll`'s secured `F` export. Lets the real `F` fill
/// the engine's `cldll_func_t` buffer exactly as it normally would, then
/// replaces the five slots we care about before the engine ever reads them.
unsafe extern "C" fn hook_f(table: *mut *mut c_void) {
    let real = REAL_F.load(Ordering::Acquire);
    if real.is_null() {
        unsafe {
            crate::debug::report(
                "F: real export pointer missing -- cannot forward, client will fail to initialise",
            )
        };
        return;
    }
    let real: ClientApiFn = unsafe { std::mem::transmute(real) };
    unsafe { real(table) };

    if table.is_null() {
        unsafe { crate::debug::report("F: engine passed a null table -- nothing hooked") };
        return;
    }

    unsafe {
        swap_slot(
            table,
            SLOT_INITIALIZE,
            &REAL_INITIALIZE,
            tramp_initialize as *mut c_void,
            "Initialize",
        );
        swap_slot(
            table,
            SLOT_HUD_FRAME,
            &REAL_HUD_FRAME,
            tramp_hud_frame as *mut c_void,
            "HUD_Frame",
        );
        swap_slot(
            table,
            SLOT_HUD_REDRAW,
            &REAL_HUD_REDRAW,
            tramp_hud_redraw as *mut c_void,
            "HUD_Redraw",
        );
        swap_slot(
            table,
            SLOT_HUD_KEY_EVENT,
            &REAL_HUD_KEY_EVENT,
            tramp_hud_key_event as *mut c_void,
            "HUD_Key_Event",
        );
        swap_slot(
            table,
            SLOT_CALC_REFDEF,
            &REAL_CALC_REFDEF,
            tramp_calc_refdef as *mut c_void,
            "V_CalcRefdef",
        );
        swap_slot(
            table,
            SLOT_HUD_ADD_ENTITY,
            &REAL_HUD_ADD_ENTITY,
            tramp_hud_add_entity as *mut c_void,
            "HUD_AddEntity",
        );
        swap_slot(
            table,
            SLOT_DRAW_TRANSPARENT_TRIANGLES,
            &REAL_DRAW_TRANSPARENT_TRIANGLES,
            tramp_draw_transparent_triangles as *mut c_void,
            "HUD_DrawTransparentTriangles",
        );
        swap_slot(
            table,
            SLOT_HUD_SHUTDOWN,
            &REAL_HUD_SHUTDOWN,
            tramp_hud_shutdown as *mut c_void,
            "HUD_Shutdown",
        );
        swap_slot(
            table,
            SLOT_HUD_DIRECTOR_MESSAGE,
            &REAL_HUD_DIRECTOR_MESSAGE,
            tramp_hud_director_message as *mut c_void,
            "HUD_DirectorMessage",
        );
        swap_slot(
            table,
            SLOT_GET_STUDIO_MODEL_INTERFACE,
            &REAL_GET_STUDIO_MODEL_INTERFACE,
            tramp_get_studio_model_interface as *mut c_void,
            "HUD_GetStudioModelInterface",
        );
    }
}

// ---------------------------------------------------------------------------
// hw.dll IAT hooks
// ---------------------------------------------------------------------------

type LoadLibraryAFn = unsafe extern "system" fn(*const u8) -> HMODULE;
type GetProcAddressFn = unsafe extern "system" fn(HMODULE, *const u8) -> *mut c_void;
static REAL_LOAD_LIBRARY_A: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_GET_PROC_ADDRESS: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// The `client.dll` module handle, as observed coming back out of
/// `LoadLibraryA`. Used to tell *its* `GetProcAddress` lookups apart from the
/// many others `hw.dll` makes against other modules.
static CLIENT_DLL: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The engine loads the mod's client DLL by a path relative to the game exe
/// (e.g. `dod\cl_dlls\client.dll`), never by the bare filename -- so this
/// compares only the path's basename, not the whole string.
unsafe fn path_basename_eq_ignore_ascii_case(ptr: *const u8, other: &str) -> bool {
    unsafe {
        if ptr.is_null() {
            return false;
        }
        let path = std::ffi::CStr::from_ptr(ptr as *const i8).to_string_lossy();
        let basename = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        basename.eq_ignore_ascii_case(other)
    }
}

unsafe extern "system" fn hook_load_library_a(lp_lib_file_name: *const u8) -> HMODULE {
    let real = REAL_LOAD_LIBRARY_A.load(Ordering::Acquire);
    let real: LoadLibraryAFn = unsafe { std::mem::transmute(real) };
    let result = unsafe { real(lp_lib_file_name) };

    if !result.is_null()
        && unsafe { path_basename_eq_ignore_ascii_case(lp_lib_file_name, "client.dll") }
    {
        CLIENT_DLL.store(result, Ordering::Release);
        unsafe {
            crate::debug::report(&format!(
                "LoadLibraryA: client.dll loaded at {result:p}, watching its GetProcAddress lookups"
            ))
        };
    }

    result
}

/// Intercepts every export `hw.dll` resolves out of `client.dll`, and swaps in
/// our own entry points for the ones we need. See the module docs for why this
/// -- rather than patching `client.dll`'s export table -- is the hook point
/// that actually works.
unsafe extern "system" fn hook_get_proc_address(module: HMODULE, name: *const u8) -> *mut c_void {
    let real = REAL_GET_PROC_ADDRESS.load(Ordering::Acquire);
    let real: GetProcAddressFn = unsafe { std::mem::transmute(real) };
    let result = unsafe { real(module, name) };

    let client = CLIENT_DLL.load(Ordering::Acquire);
    if client.is_null() || !std::ptr::eq(module, client) || result.is_null() {
        return result;
    }

    // An import can be requested by ordinal instead of by name, in which case
    // `name` is not a pointer at all -- it's the ordinal in its low word, and
    // dereferencing it would fault.
    if (name as usize) >> 16 == 0 {
        return result;
    }
    let Ok(requested) = (unsafe { std::ffi::CStr::from_ptr(name as *const c_char) }).to_str()
    else {
        return result;
    };

    unsafe {
        crate::debug::report(&format!(
            "GetProcAddress(client.dll, \"{requested}\") -> {result:p}"
        ))
    };

    // "F" is the secured single-callback export, and is what DoD 1.3 actually
    // uses; the five named entries below are the classic convention, kept so
    // this works unchanged on a non-secured client.dll too.
    match requested {
        "F" => {
            REAL_F.store(result, Ordering::Release);
            unsafe {
                crate::debug::report(
                    "GetProcAddress: secured \"F\" export intercepted -- returning our wrapper",
                )
            };
            hook_f as *mut c_void
        }
        "Initialize" => {
            REAL_INITIALIZE.store(result, Ordering::Release);
            tramp_initialize as *mut c_void
        }
        "HUD_Frame" => {
            REAL_HUD_FRAME.store(result, Ordering::Release);
            tramp_hud_frame as *mut c_void
        }
        "HUD_Redraw" => {
            REAL_HUD_REDRAW.store(result, Ordering::Release);
            tramp_hud_redraw as *mut c_void
        }
        "HUD_Key_Event" => {
            REAL_HUD_KEY_EVENT.store(result, Ordering::Release);
            tramp_hud_key_event as *mut c_void
        }
        "V_CalcRefdef" => {
            REAL_CALC_REFDEF.store(result, Ordering::Release);
            tramp_calc_refdef as *mut c_void
        }
        "HUD_AddEntity" => {
            REAL_HUD_ADD_ENTITY.store(result, Ordering::Release);
            tramp_hud_add_entity as *mut c_void
        }
        "HUD_DrawTransparentTriangles" => {
            REAL_DRAW_TRANSPARENT_TRIANGLES.store(result, Ordering::Release);
            tramp_draw_transparent_triangles as *mut c_void
        }
        "HUD_Shutdown" => {
            REAL_HUD_SHUTDOWN.store(result, Ordering::Release);
            tramp_hud_shutdown as *mut c_void
        }
        "HUD_DirectorMessage" => {
            REAL_HUD_DIRECTOR_MESSAGE.store(result, Ordering::Release);
            tramp_hud_director_message as *mut c_void
        }
        "HUD_GetStudioModelInterface" => {
            REAL_GET_STUDIO_MODEL_INTERFACE.store(result, Ordering::Release);
            tramp_get_studio_model_interface as *mut c_void
        }
        _ => result,
    }
}

/// Waits (briefly) for `module_name` to be loaded -- we're injected before the
/// engine's own modules exist, so this can't assume it's already there.
fn wait_for_module(module_name: &str, timeout_ms: u32) -> Option<HMODULE> {
    let module_name_c = format!("{module_name}\0");
    let mut waited = 0u32;
    loop {
        let h = unsafe { GetModuleHandleA(module_name_c.as_ptr()) };
        if !h.is_null() {
            return Some(h);
        }
        if waited >= timeout_ms {
            return None;
        }
        unsafe { Sleep(25) };
        waited += 25;
    }
}

/// Overwrites one of `module`'s IAT slots so its own calls to
/// `KERNEL32!<import_name>` route through `replacement`, stashing the original
/// in `real_store`. Only that module's calls are affected -- unlike
/// inline-patching the function itself, which would redirect the whole
/// process (HLAE's `CAfxImportDllHook` makes the same trade-off).
fn hook_import(
    module: HMODULE,
    import_name: &str,
    replacement: *mut c_void,
    real_store: &AtomicPtr<c_void>,
) -> bool {
    let Some(slot) = (unsafe { pe::find_iat_slot(module as *mut u8, "KERNEL32.dll", import_name) })
    else {
        return false;
    };

    unsafe {
        real_store.store(*slot, Ordering::Release);
        // Left writable: HLAE hooks imports on this page too (#1059).
        crate::patch::write_pointer(slot, replacement, crate::patch::After::LeaveWritable)
    }
}

/// Installs both `hw.dll` IAT hooks: `LoadLibraryA` (to learn `client.dll`'s
/// module handle) and `GetProcAddress` (to intercept how the engine resolves
/// `client.dll`'s entry points). Call once, from a background thread -- never
/// do this work directly in `DllMain` (see `lib.rs`).
pub fn install() {
    // GoldSrc's OpenGL renderer module is virtually always what's actually
    // loaded in any modern setup (HLAE's own recording pipeline requires
    // it); `sw.dll` (the old software renderer) is not handled here.
    let Some(hw) = wait_for_module("hw.dll", 15_000) else {
        unsafe {
            crate::debug::report(
                "goldsrc-hooks: hw.dll never appeared -- sound/animation fixes are inactive this session.",
            )
        };
        return;
    };

    // Order matters: GetProcAddress first, so there is no window in which
    // client.dll could load and be resolved before that hook is armed.
    let hooked_gpa = hook_import(
        hw,
        "GetProcAddress",
        hook_get_proc_address as *mut c_void,
        &REAL_GET_PROC_ADDRESS,
    );
    let hooked_lla = hook_import(
        hw,
        "LoadLibraryA",
        hook_load_library_a as *mut c_void,
        &REAL_LOAD_LIBRARY_A,
    );

    unsafe {
        crate::debug::report(&format!(
            "goldsrc-hooks: hw.dll IAT hooks -- GetProcAddress: {}, LoadLibraryA: {}",
            if hooked_gpa { "ok" } else { "FAILED" },
            if hooked_lla { "ok" } else { "FAILED" },
        ))
    };

    if !hooked_gpa || !hooked_lla {
        unsafe {
            crate::debug::report(
                "goldsrc-hooks: an IAT slot wasn't found -- sound/animation fixes are inactive this session.",
            )
        };
    }
}

#[cfg(test)]
mod per_frame_tests {
    use super::*;

    /// The timer thread drives only when there is no hook to do it, and gives
    /// way for good once a real frame has run.
    #[test]
    fn the_timer_thread_drives_only_without_a_hook() {
        assert_eq!(timer_step(false, false), TimerStep::Drive);
        assert_eq!(timer_step(false, true), TimerStep::StandBy);
        assert_eq!(timer_step(true, true), TimerStep::Retire);
        assert_eq!(timer_step(true, false), TimerStep::Retire);
    }

    /// One turn at a time, and the turn comes back when it drops.
    #[test]
    fn a_per_frame_turn_excludes_a_second_runner() {
        let flag = AtomicBool::new(false);
        let first = PerFrameTurn::try_take(&flag);
        assert!(first.is_some());
        assert!(PerFrameTurn::try_take(&flag).is_none());
        drop(first);
        assert!(PerFrameTurn::try_take(&flag).is_some());
    }

    /// Two threads hammering the same turn never both hold it: the property
    /// the first-frame crash broke.
    #[test]
    fn two_threads_never_hold_the_turn_together() {
        use std::sync::atomic::AtomicU32;
        static FLAG: AtomicBool = AtomicBool::new(false);
        static INSIDE: AtomicU32 = AtomicU32::new(0);
        static OVERLAPS: AtomicU32 = AtomicU32::new(0);
        let runner = || {
            for _ in 0..20_000 {
                if let Some(_turn) = PerFrameTurn::try_take(&FLAG) {
                    if INSIDE.fetch_add(1, Ordering::SeqCst) != 0 {
                        OVERLAPS.fetch_add(1, Ordering::SeqCst);
                    }
                    std::hint::spin_loop();
                    INSIDE.fetch_sub(1, Ordering::SeqCst);
                }
            }
        };
        let a = std::thread::spawn(runner);
        let b = std::thread::spawn(runner);
        a.join().unwrap();
        b.join().unwrap();
        assert_eq!(OVERLAPS.load(Ordering::SeqCst), 0);
    }
}
