//! Partial `#[repr(C)]` mirrors of the Half-Life SDK structs and function
//! types the hooks read or call: the engine's `cl_enginefuncs_s` table and
//! the APIs it points at, entity and model layouts, and the cvar record.
//! Layouts and small field readers only: no hooks and no engine calls.

use std::ffi::{c_char, c_void};

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
/// Slot 31, `pfnCenterPrint(text)`: the engine's wrapper that hands `text`,
/// unformatted, to `SCR_CenterPrint` (which copies up to 1023 bytes). Call
/// it only through `demo_damage::tell::center_print`, which checks the slot
/// still points at the wrapper each build is known to have.
pub type CenterPrintFn = unsafe extern "C" fn(text: *const c_char);
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
    /// The client's simulated velocity; in a POV demo, the recorder's
    /// (`pov_crosshair_gap`).
    pub simvel: [f32; 3],
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
    /// Slot 12, `call [gEngfuncs+0x30]` in `CHud::Think` (`client+0x36b32`).
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
    /// Slot 31. See [`CenterPrintFn`]; the engine's table holds it at
    /// pre-Anniversary `hw.dll+0x1362dc` (`+0xb0e0`) and 25th Anniversary
    /// `+0x319b84` (`+0x195480`), 31 slots in (`tools/verify_center_print_offsets.py`).
    pub pfn_center_print: CenterPrintFn,
    _slots_before_cmd_argc: [*mut c_void; 6], // GetWindowCenterX .. Cvar_SetValue
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
