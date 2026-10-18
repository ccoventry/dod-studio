//! Sprays in demos from Studio's Players table (#428, #368): every listed
//! player's spray built from their PNG, whether or not the demo saved their own,
//! and the fallbacks for everyone else -- one route for POV and HLTV.
//!
//! ## Why a demo draws the bullet hole
//!
//! A spray reaches a client as `TE_PLAYERDECAL` (player, position, entity,
//! decal index). `CL_ParseTEnt` (pre-Anniversary `hw+0x23dc0`, 25th
//! Anniversary `hw+0x1ade70`) looks up the player's `customdata`, the
//! customization the server relayed with `svc_customization`, and if it has
//! one calls `R_CustomDecalShoot(texture, player, entity, 0, pos, 4)`, which
//! shoots decal index `~player`. Without one it shoots `decals.wad` entry
//! `decal index` instead. An HLTV demo never carries `svc_customization`, and
//! every spray's decal index is 0, `{cement2`: the bullet hole (#368 scanned
//! 14 HLTV demos). Most POV demos lack it too for players who joined before
//! the recording started.
//!
//! The decal itself remembers only its index. Every frame the renderer asks
//! `Draw_DecalTexture(index)` (pre-Anniversary `hw+0x2f890`, 25th Anniversary
//! `hw+0x1bf520`) for the texture again, and for a negative index that reads
//! `cl.players[~index].customdata` afresh, returning NULL (and the caller
//! crashing on it) when there is none. So shooting a texture of ours is half
//! the job; the per-frame lookup has to find it too.
//!
//! ## The route: two detours and a texture of ours
//!
//! 1. **At `CL_ParseTEnt`'s `customdata` test**, after its `r_decals` check
//!    (pre-Anniversary `hw+0x24689`, 25th Anniversary `hw+0x1ae907`): for
//!    every spray, the stub asks [`slot_texture`] for a texture. With one, it
//!    continues into the engine's own custom branch, which calls
//!    `R_CustomDecalShoot` with it; with none, the engine's own test runs as
//!    before (their saved spray, or the wad decal). The two builds keep the
//!    player number, the texture and the `customdata` pointer in different
//!    places ([`BUILDS`]). The 25th Anniversary build keeps the texture of a
//!    saved spray in `ecx`, which the stub saves around the call; every other
//!    scratch register is written before it is read on both paths
//!    (`tools/verify_hltv_spray_offsets.py`).
//! 2. **At `Draw_DecalTexture`'s entry**: a negative index gets our texture
//!    back, before the engine's own lookup can fail -- from [`SLOT_TEX`] for
//!    a player with no `customdata`, from [`SLOT_OVER`] for one whose saved
//!    spray the table replaces. A player with `customdata` and no
//!    replacement gets the engine's.
//! 3. **The texture** is built by the engine's own `Draw_MiptexTexture`, the
//!    function a customization goes through, from a buffer laid out the way
//!    `Draw_CacheLoadFromCustom` lays one out (32 bytes for the `texture_t`
//!    in front of the lump). It names, sizes and uploads it (`GLT_DECAL`)
//!    exactly as it would a real spray, and with HD textures on the upload
//!    passes `texture_hires`' spray arm, which swaps the HD PNG in.
//!
//! What a spray becomes is Studio's Players table (`player_table.rs`), by
//! SteamID or name: a listed player's PNG, built with `spray.rs`'s writer at
//! the tab's spray size (the longer side; 64 by default, a typical saved
//! spray's) whoever's demo it is and whether or not it saved their own; the spray
//! the demo saved for "From the demo" (the fallback when it saved none); else the fallback for the demo's type.
//! "No spray" is a fully transparent spray; the game's own is untouched.
//! Each PNG is read on a background thread whenever the table changes, so a
//! 3840 px PNG never stalls a demo; a spray that isn't ready yet is left to
//! the engine.
//!
//! "No spray" draws a transparent texture rather than skipping the shoot:
//! skipping would need another patch inside `CL_ParseTEnt` on both builds,
//! and the custom branch with a texture of ours is the path already proven
//! on both. Every pixel is palette index 255, the cut-out key, so nothing
//! shows, and the decal costs one slot of the ring like any spray.
//!
//! ## Installing
//!
//! Only once the table asks for something (`Table::active`) and
//! `dodstudio_player_overrides` is 1: [`poll`] reads the file's time once a
//! second and installs both detours the first time. The `Draw_DecalTexture`
//! one goes in first, because a decal shot through step 1 without step 2
//! would crash the next frame. Each site's bytes are checked before
//! patching, and [`poll`] checks the jumps are still there. Detours are
//! never removed (see [`detour::Detour`]); with the table back to the
//! defaults, [`slot_texture`] answers "none" and the engine runs as before.
//!
//! `tools/verify_hltv_spray_offsets.py` checks every offset and register
//! above against both builds' `hw.dll`. The reading behind it is in
//! `local/r-and-d/428/notes.md` (a working file, not in the tree).

use std::collections::HashMap;
use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use crate::player_table::{self, Choice, Table};
use crate::spray::{self, Key};
use crate::{detour, engine, scan};

/// One engine build's three sites.
struct Build {
    name: &'static str,
    /// `CL_ParseTEnt`'s `TE_PLAYERDECAL` case from the `customdata` test
    /// after `r_decals`, through the custom branch's `R_CustomDecalShoot`
    /// call, to the wad-decal branch's first call through the decal table.
    te_pattern: &'static str,
    /// The bytes the jump overwrites: [`Build::cust_test`], its `je` to the
    /// wad-decal branch, and [`Build::tex_test`].
    te_stolen: &'static [u8],
    /// Is there a `customdata`? (`cmp edi, esi` with `esi` 0 / `test esi, esi`)
    cust_test: &'static [u8],
    /// Did the custom branch get a texture? The flags it leaves are what the
    /// `je` at the end of the span reads.
    tex_test: &'static [u8],
    /// Puts `eax` where the custom branch reads its texture from
    /// (`mov [ebp-0x2c], eax` / `mov ecx, eax`).
    tex_store: &'static [u8],
    /// The player number's frame slot, as the `disp8` of `[ebp+disp8]`.
    playernum_disp: u8,
    /// The custom branch's first instruction, from the site.
    custom_at: usize,
    /// The wad-decal branch's first instruction, from the site.
    else_at: usize,
    /// `Draw_DecalTexture`'s entry, through its `customdata` load.
    decal_texture: &'static str,
    /// Where in that match the `customdata` table's address (`disp32`) is.
    customdata_at: usize,
    /// `Draw_MiptexTexture`'s entry, through its `cacheExtra == 0x20` test.
    miptex_texture: &'static str,
}

/// The builds this knows, tried in order.
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        te_pattern: "3B FE 74 2B 39 75 D4 0F 84 ?? ?? ?? ?? 8B 45 CC 8B 4D C8 8D 55 B0 6A 04 52 \
                     8B 55 D4 56 50 51 52 E8 ?? ?? ?? ?? 83 C4 18 5F 5E 5B 8B E5 5D C3 8B 4D CC \
                     8D 45 B0 6A 04 50 56 51 53 FF 15",
        te_stolen: &[0x3b, 0xfe, 0x74, 0x2b, 0x39, 0x75, 0xd4],
        cust_test: &[0x3b, 0xfe],
        tex_test: &[0x39, 0x75, 0xd4],
        tex_store: &[0x89, 0x45, 0xd4],
        playernum_disp: 0xc8,
        custom_at: 0x0d,
        else_at: 0x2f,
        decal_texture: "55 8B EC 8B 4D 08 56 85 C9 57 7D 79 83 C8 FF 2B C1 8D 0C C0 8D 14 88 C1 E2 04 \
                        8B 8A ?? ?? ?? ?? 85 C9 74 46",
        customdata_at: 28,
        miptex_texture: "55 8B EC 83 EC 28 53 56 8B 75 08 57 83 7E 18 20 74 10 8B 06 50 68",
    },
    // The texture stays in `ecx` here instead of a local, and the player
    // number sits lower in a frame that also holds a stack cookie.
    Build {
        name: "25th Anniversary",
        te_pattern: "85 F6 74 2E 85 C9 0F 84 ?? ?? ?? ?? 6A 04 8D 45 F0 50 6A 00 53 FF 75 A4 51 \
                     E8 ?? ?? ?? ?? 83 C4 18 5F 5E 5B 8B 4D FC 33 CD E8 ?? ?? ?? ?? 8B E5 5D C3 \
                     6A 04 8D 45 F0 50 6A 00 53 57 FF 15",
        te_stolen: &[0x85, 0xf6, 0x74, 0x2e, 0x85, 0xc9],
        cust_test: &[0x85, 0xf6],
        tex_test: &[0x85, 0xc9],
        tex_store: &[0x8b, 0xc8],
        playernum_disp: 0xa4,
        custom_at: 0x0c,
        else_at: 0x32,
        decal_texture: "55 8B EC 8B 4D 08 85 C9 79 74 F7 D1 69 D1 50 02 00 00 8B 82 ?? ?? ?? ?? 85 C0 \
                        74 49",
        customdata_at: 20,
        miptex_texture: "55 8B EC 83 EC 34 A1 ?? ?? ?? ?? 33 C5 89 45 FC 53 8B 5D 0C 56 8B 75 08 57 \
                         89 75 CC 89 5D D0 8B 46 18 83 F8 20 74 12",
    },
];

/// `push ebp; mov ebp, esp; mov ecx, [ebp+8]`: `Draw_DecalTexture`'s first
/// six bytes on both builds, reproduced in its stub.
const DT_STOLEN: [u8; 6] = [0x55, 0x8b, 0xec, 0x8b, 0x4d, 0x08];
/// A player record in `cl.players`, the same on both builds.
const PLAYER_STRIDE: u32 = 0x250;
/// `MAX_CLIENTS`: the slots `CL_ParseTEnt` looks a `customdata` up for.
const MAX_PLAYERS: usize = 32;
/// `cachewad_t::cacheExtra` for the decal cache: the `texture_t` prefix in
/// front of each lump, which `Draw_MiptexTexture` insists on.
const TEX_PREFIX: usize = 0x20;
/// `GL_Upload8` reads a whole palette whatever the count says, so every
/// buffer has this much zero after the lump.
const PALETTE_PAD: usize = 768;

/// The texture each slot's last spray of ours was shot with, for a player
/// with no `customdata`, read by the `Draw_DecalTexture` stub every frame.
/// Never set back to 0 once set: a decal already on a wall would then find
/// nothing, and the engine's lookup returns NULL for them.
static SLOT_TEX: [AtomicUsize; MAX_PLAYERS] = [const { AtomicUsize::new(0) }; MAX_PLAYERS];
/// The same for a player whose `customdata` is there: our texture over their
/// saved spray, or 0 for the engine's, which can always answer for them.
static SLOT_OVER: [AtomicUsize; MAX_PLAYERS] = [const { AtomicUsize::new(0) }; MAX_PLAYERS];
/// `cl.players[0].customdata`'s list head, read out of `Draw_DecalTexture`;
/// a slot's is [`PLAYER_STRIDE`] further on per slot.
static CUSTOMDATA: AtomicUsize = AtomicUsize::new(0);
/// Where the `CL_ParseTEnt` stub jumps: the custom branch, the wad-decal
/// branch, and back after the span.
static TE_CUSTOM: AtomicUsize = AtomicUsize::new(0);
static TE_ELSE: AtomicUsize = AtomicUsize::new(0);
static TE_RESUME: AtomicUsize = AtomicUsize::new(0);
/// Where the `Draw_DecalTexture` stub resumes the engine's code.
static DT_RESUME: AtomicUsize = AtomicUsize::new(0);
/// `Draw_MiptexTexture`.
static MIPTEX: AtomicUsize = AtomicUsize::new(0);
/// The two patched sites, for [`poll`]'s check.
static TE_SITE: AtomicUsize = AtomicUsize::new(0);
static DT_SITE: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Set when installing failed for good, so it isn't retried every second.
static GAVE_UP: AtomicBool = AtomicBool::new(false);
/// Set once a patched site was found overwritten, so it is said once.
static OVERWRITTEN: AtomicBool = AtomicBool::new(false);
static BUILD_NAME: Mutex<&str> = Mutex::new("");
/// See [`detour::Detour`] on why these are never undone.
static DETOURS: Mutex<Vec<detour::Detour>> = Mutex::new(Vec::new());

/// True while `Draw_MiptexTexture` uploads a texture of ours, so
/// `texture_hires` treats it as a spray (`spray::is_custom_upload`).
static BUILDING: AtomicBool = AtomicBool::new(false);
/// Sprays drawn through the custom branch this session.
static DRAWN: AtomicU32 = AtomicU32::new(0);

/// Whether the Players table asks for anything (`Table::active`).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The table as last read by [`poll`].
static TABLE: RwLock<Option<Arc<Table>>> = RwLock::new(None);
/// The table file's modification time when last read, in nanoseconds
/// since the epoch plus two; 0 before the first read, 1 with no file.
static TABLE_STAMP: AtomicU64 = AtomicU64::new(0);
static FRAMES: AtomicU32 = AtomicU32::new(0);
/// Frames between two looks at the table file's time: about a second.
const STAMP_EVERY: u32 = 60;

/// A spray ready to become a texture: one lump, checked as the engine
/// checks a cached customization.
struct Lump {
    bytes: Vec<u8>,
    key: Key,
    width: u32,
    height: u32,
}

/// A source read, or why it couldn't be.
type Prepared = Result<Arc<Lump>, String>;

/// Each PNG the table names, and the blank spray, read (or failed) on the
/// background thread. Keyed by [`Choice::Png`] and [`Choice::Blank`].
static PREPARED: LazyLock<RwLock<HashMap<Choice, Prepared>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static PREPARING: AtomicBool = AtomicBool::new(false);
static PREPARE_AGAIN: AtomicBool = AtomicBool::new(false);
/// The PNG each built-from-PNG spray's key stands for: its HD version.
static HD_FOR_KEY: LazyLock<RwLock<HashMap<Key, PathBuf>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// A texture of ours: a buffer laid out like the engine's decal cache entry,
/// leaked so its address stays valid for every decal that names it.
struct Texture {
    buf: &'static mut [u8],
    lump: Arc<Lump>,
    name: [u8; 16],
}

/// Built textures, by the spray's content key. Only the main thread touches
/// it (`CL_ParseTEnt`, the console), when a spray message arrives.
static TEXTURES: LazyLock<Mutex<HashMap<Key, Texture>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Sprays already reported unusable, so each is said once per table.
static WARNED: Mutex<Vec<Choice>> = Mutex::new(Vec::new());

/// Whether a texture of ours is being uploaded right now.
pub fn building() -> bool {
    BUILDING.load(Ordering::Relaxed)
}

/// The HD PNG for a spray this module built from a PNG.
pub fn png_for_built_key(key: &Key) -> Option<PathBuf> {
    HD_FOR_KEY.read().ok()?.get(key).cloned()
}

// ── The stubs ───────────────────────────────────────────────────────────────

/// `jmp dword ptr [abs32]`.
fn jmp_indirect(code: &mut Vec<u8>, slot: usize) {
    code.extend_from_slice(&[0xff, 0x25]);
    code.extend_from_slice(&(slot as u32).to_le_bytes());
}

/// The `CL_ParseTEnt` stub. On entry the `r_decals` check has passed and
/// the frame holds what both branches read.
///
/// ```asm
///         push  ecx                  ; a saved spray's texture (25th Anniversary)
///         push  dword [ebp+<playernum>]
///         mov   eax, slot_texture
///         call  eax                  ; cdecl: keeps ebx, esi, edi, ebp
///         add   esp, 4
///         pop   ecx
///         test  eax, eax
///         jz    .game
///         <tex_store>                ; where the custom branch reads it
///         jmp   dword [TE_CUSTOM]
/// .game:  <cust_test>                ; is there a customdata?
///         jne   .cust
///         jmp   dword [TE_ELSE]
/// .cust:  <tex_test>                 ; the engine's own, for its `je`
///         jmp   dword [TE_RESUME]
/// ```
///
/// Neither branch reads `eax`, `ecx` or `edx` before writing them except
/// the texture register, which `ecx` saves; the x87 stack is empty after
/// `r_decals`' `fcomp` (pre-Anniversary) or was never used (25th
/// Anniversary, SSE).
fn te_stub(build: &Build, callback: usize, custom: usize, els: usize, resume: usize) -> Vec<u8> {
    let mut code = vec![0x51]; // push ecx
    code.extend_from_slice(&[0xff, 0x75, build.playernum_disp]); // push [ebp+disp8]
    code.push(0xb8); // mov eax, imm32
    code.extend_from_slice(&(callback as u32).to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0]); // call eax
    code.extend_from_slice(&[0x83, 0xc4, 0x04]); // add esp, 4
    code.push(0x59); // pop ecx
    code.extend_from_slice(&[0x85, 0xc0]); // test eax, eax
    let jz_at = code.len();
    code.extend_from_slice(&[0x74, 0]); // jz .game, patched below
    code.extend_from_slice(build.tex_store);
    jmp_indirect(&mut code, custom);
    let game_label = code.len();
    code.extend_from_slice(build.cust_test);
    let jne_at = code.len();
    code.extend_from_slice(&[0x75, 0]); // jne .cust, patched below
    jmp_indirect(&mut code, els);
    let cust_label = code.len();
    code.extend_from_slice(build.tex_test);
    jmp_indirect(&mut code, resume);
    code[jz_at + 1] = (game_label - (jz_at + 2)) as u8;
    code[jne_at + 1] = (cust_label - (jne_at + 2)) as u8;
    code
}

/// The `Draw_DecalTexture` stub, before the engine's prologue.
///
/// ```asm
///         mov   eax, [esp+4]             ; index
///         not   eax                      ; the player, if index < 0
///         cmp   eax, 32
///         jae   .game                    ; index >= 0 (or out of range)
///         mov   ecx, eax
///         imul  ecx, ecx, PLAYER_STRIDE
///         cmp   dword [ecx + customdata], 0
///         jne   .saved
///         mov   ecx, [SLOT_TEX + eax*4]  ; no customdata: ours, if any
///         jmp   .test
/// .saved: mov   ecx, [SLOT_OVER + eax*4] ; their saved one, or ours over it
/// .test:  test  ecx, ecx
///         jz    .game
///         mov   eax, ecx
///         ret
/// .game:  push ebp; mov ebp, esp; mov ecx, [ebp+8]
///         jmp   dword [DT_RESUME]
/// ```
///
/// Touches `eax`, `ecx` and the flags, which a cdecl callee may clobber.
fn dt_stub(slot_tex: usize, slot_over: usize, customdata: usize, resume: usize) -> Vec<u8> {
    let mut code = vec![0x8b, 0x44, 0x24, 0x04]; // mov eax, [esp+4]
    code.extend_from_slice(&[0xf7, 0xd0]); // not eax
    code.extend_from_slice(&[0x83, 0xf8, MAX_PLAYERS as u8]); // cmp eax, 32
    let jae_at = code.len();
    code.extend_from_slice(&[0x73, 0]); // jae .game
    code.extend_from_slice(&[0x8b, 0xc8]); // mov ecx, eax
    code.extend_from_slice(&[0x69, 0xc9]); // imul ecx, ecx, imm32
    code.extend_from_slice(&PLAYER_STRIDE.to_le_bytes());
    code.extend_from_slice(&[0x83, 0xb9]); // cmp dword [ecx + abs32], imm8
    code.extend_from_slice(&(customdata as u32).to_le_bytes());
    code.push(0);
    let jne_at = code.len();
    code.extend_from_slice(&[0x75, 0]); // jne .saved
    code.extend_from_slice(&[0x8b, 0x0c, 0x85]); // mov ecx, [abs32 + eax*4]
    code.extend_from_slice(&(slot_tex as u32).to_le_bytes());
    let jmp_at = code.len();
    code.extend_from_slice(&[0xeb, 0]); // jmp .test
    let saved = code.len();
    code.extend_from_slice(&[0x8b, 0x0c, 0x85]); // mov ecx, [abs32 + eax*4]
    code.extend_from_slice(&(slot_over as u32).to_le_bytes());
    let test = code.len();
    code.extend_from_slice(&[0x85, 0xc9]); // test ecx, ecx
    let jz_at = code.len();
    code.extend_from_slice(&[0x74, 0]); // jz .game
    code.extend_from_slice(&[0x8b, 0xc1, 0xc3]); // mov eax, ecx; ret
    let game = code.len();
    code.extend_from_slice(&DT_STOLEN);
    jmp_indirect(&mut code, resume);
    for at in [jae_at, jz_at] {
        code[at + 1] = (game - (at + 2)) as u8;
    }
    code[jne_at + 1] = (saved - (jne_at + 2)) as u8;
    code[jmp_at + 1] = (test - (jmp_at + 2)) as u8;
    code
}

// ── Installing ──────────────────────────────────────────────────────────────

/// What one build's signatures found in the loaded `hw.dll`.
struct Sites {
    build: &'static Build,
    te: usize,
    dt: usize,
    customdata: usize,
    miptex: usize,
}

fn find_sites(base: usize) -> Result<Sites, String> {
    let mut misses = Vec::new();
    for build in &BUILDS {
        // Safety: `base` is hw.dll's module handle, mapped for the session.
        let te = match unsafe { scan::find_unique(base, build.te_pattern) } {
            Ok(at) => at,
            Err(why) => {
                misses.push(format!("{} CL_ParseTEnt: {why}", build.name));
                continue;
            }
        };
        let dt = unsafe { scan::find_unique(base, build.decal_texture) }
            .map_err(|why| format!("{} Draw_DecalTexture: {why}", build.name))?;
        let miptex = unsafe { scan::find_unique(base, build.miptex_texture) }
            .map_err(|why| format!("{} Draw_MiptexTexture: {why}", build.name))?;
        // Safety: inside the match just found.
        let customdata =
            unsafe { ((dt + build.customdata_at) as *const u32).read_unaligned() } as usize;
        let present = |at: usize, want: &[u8]| {
            // Safety: inside a match at least this long.
            unsafe { std::slice::from_raw_parts(at as *const u8, want.len()) == want }
        };
        if !present(te, build.te_stolen) || !present(dt, &DT_STOLEN) {
            return Err(format!(
                "{}: the bytes to patch aren't the expected ones",
                build.name
            ));
        }
        if customdata == 0 {
            return Err(format!("{}: no customdata table", build.name));
        }
        return Ok(Sites {
            build,
            te,
            dt,
            customdata,
            miptex,
        });
    }
    Err(misses.join("; "))
}

fn install() {
    if INSTALLED.load(Ordering::Acquire) || GAVE_UP.load(Ordering::Relaxed) {
        return;
    }
    let Some(base) = engine::engine_module_base() else {
        return; // hw.dll isn't loaded yet: the next poll tries again.
    };
    let result = find_sites(base).and_then(|sites| {
        let mut detours = Vec::new();
        DT_RESUME.store(sites.dt + DT_STOLEN.len(), Ordering::Release);
        CUSTOMDATA.store(sites.customdata, Ordering::Release);
        let dt = dt_stub(
            SLOT_TEX.as_ptr() as usize,
            SLOT_OVER.as_ptr() as usize,
            sites.customdata,
            DT_RESUME.as_ptr() as usize,
        );
        // Safety: the bytes were checked above, and nothing branches into
        // the span (tools/verify_hltv_spray_offsets.py).
        detours.push(unsafe { detour::install(sites.dt, DT_STOLEN.len(), &dt) }?);
        MIPTEX.store(sites.miptex, Ordering::Release);
        let build = sites.build;
        TE_CUSTOM.store(sites.te + build.custom_at, Ordering::Release);
        TE_ELSE.store(sites.te + build.else_at, Ordering::Release);
        TE_RESUME.store(sites.te + build.te_stolen.len(), Ordering::Release);
        let te = te_stub(
            build,
            slot_texture as *const () as usize,
            TE_CUSTOM.as_ptr() as usize,
            TE_ELSE.as_ptr() as usize,
            TE_RESUME.as_ptr() as usize,
        );
        // Safety: as above. Draw_DecalTexture is already covered, so a
        // spray shot through here can be drawn.
        detours.push(unsafe { detour::install(sites.te, build.te_stolen.len(), &te) }?);
        Ok((sites, detours))
    });
    match result {
        Ok((sites, detours)) => {
            if let Ok(mut slot) = DETOURS.lock() {
                slot.extend(detours);
            }
            if let Ok(mut name) = BUILD_NAME.lock() {
                *name = sites.build.name;
            }
            TE_SITE.store(sites.te, Ordering::Relaxed);
            DT_SITE.store(sites.dt, Ordering::Relaxed);
            INSTALLED.store(true, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "spray: demo sprays on ({} hw.dll): CL_ParseTEnt +{:#x}, Draw_DecalTexture +{:#x}, \
                     Draw_MiptexTexture +{:#x}, customdata {:#x} (#428)",
                    sites.build.name,
                    sites.te - base,
                    sites.dt - base,
                    sites.miptex - base,
                    sites.customdata
                ))
            };
        }
        Err(why) => {
            GAVE_UP.store(true, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "spray: demo sprays not installed -- {why}; sprays with no image draw the bullet hole as before"
                ))
            };
        }
    }
}

/// Once a second: rereads the Players table when its file changed, installs
/// the detours the first time it asks for anything, and checks they are
/// still in place. Runs every frame from `commands::poll`; one atomic add on
/// the other frames.
pub fn poll() {
    if !FRAMES
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(STAMP_EVERY)
    {
        return;
    }
    let stamp = player_table::path()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(1, |d| (d.as_nanos() as u64).wrapping_add(2));
    if TABLE_STAMP.swap(stamp, Ordering::Relaxed) != stamp {
        reload_table();
    }
    if ACTIVE.load(Ordering::Relaxed) && crate::player_overrides::on() {
        install();
    }
    check_sites();
}

fn reload_table() {
    let table = Arc::new(player_table::load());
    let active = table.active();
    unsafe {
        crate::debug::report(&format!(
            "spray: Players table read: {} player(s), POV others -> {}, HLTV others -> {}{}",
            table.players.len(),
            table.pov,
            table.hltv,
            if table.bad.is_empty() {
                String::new()
            } else {
                format!("; line(s) {:?} not understood", table.bad)
            }
        ))
    };
    if let Ok(mut slot) = TABLE.write() {
        *slot = Some(table);
    }
    ACTIVE.store(active, Ordering::Relaxed);
    // A changed file is read again from scratch: a PNG may have been
    // replaced under the same name.
    if let Ok(mut prepared) = PREPARED.write() {
        prepared.clear();
    }
    if let Ok(mut warned) = WARNED.lock() {
        warned.clear();
    }
    if active {
        kick_prepare();
    }
}

/// Says once if something overwrote a jump of ours.
fn check_sites() {
    if !INSTALLED.load(Ordering::Relaxed) || OVERWRITTEN.load(Ordering::Relaxed) {
        return;
    }
    for (what, at) in [
        ("CL_ParseTEnt", TE_SITE.load(Ordering::Relaxed)),
        ("Draw_DecalTexture", DT_SITE.load(Ordering::Relaxed)),
    ] {
        // Safety: a site in hw.dll's code, patched by `install`.
        if at != 0 && unsafe { (at as *const u8).read_volatile() } != 0xe9 {
            OVERWRITTEN.store(true, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "spray: the demo spray jump in {what} was overwritten by something else"
                ))
            };
        }
    }
}

// ── Preparing a source ──────────────────────────────────────────────────────

/// The sides of the "no spray" texture: the smallest the writer makes.
const BLANK_SIDE: u32 = 16;

/// Builds `source`'s spray; one from a PNG gets `side` as its longer side
/// (`Table::spray_side`: the player's own size isn't known on this route,
/// and the sprays #368 counted are 64 to 128 a side). A size change reaches
/// sprays built after the table is read again, like any other change.
fn prepare(source: &Choice, side: u32) -> Result<Lump, String> {
    let (lump, png) = match source {
        Choice::Png(name) => {
            let path = spray::logos_dir().join(name);
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let img = spray::decode_png(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            let (wad, _) = spray::build_wad_sized(&img, side)?;
            (spray::wad_lump(&wad)?.to_vec(), Some(path))
        }
        Choice::Blank => {
            let img = crate::texture_hires::Rgba {
                width: BLANK_SIDE,
                height: BLANK_SIDE,
                pixels: vec![0; (BLANK_SIDE * BLANK_SIDE * 4) as usize],
            };
            let (wad, _) = spray::build_wad_sized(&img, BLANK_SIDE)?;
            (spray::wad_lump(&wad)?.to_vec(), None)
        }
        Choice::Game | Choice::Own => return Err("the spray the demo saved isn't built".into()),
    };
    let (key, width, height) = spray::lump_key(&lump)?;
    if let Some(png) = png
        && let Ok(mut keys) = HD_FOR_KEY.write()
    {
        keys.insert(key, png);
    }
    Ok(Lump {
        bytes: lump,
        key,
        width,
        height,
    })
}

/// The table as last read, for the crosshair (`player_crosshair`) too.
pub fn current_table() -> Option<Arc<Table>> {
    TABLE.read().ok()?.clone()
}

/// What the table can ask to be built: every PNG it names, and the blank.
fn sources(table: &Table) -> Vec<Choice> {
    let mut out: Vec<Choice> = table.pngs().into_iter().map(Choice::Png).collect();
    out.push(Choice::Blank);
    out
}

/// Builds every spray the table can ask for on a thread of its own. A
/// change while it runs makes it go round again.
fn kick_prepare() {
    PREPARE_AGAIN.store(true, Ordering::Release);
    if PREPARING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("dodstudio-hltv-sprays".into())
        .spawn(|| {
            loop {
                while PREPARE_AGAIN.swap(false, Ordering::AcqRel) {
                    let Some(table) = current_table() else {
                        continue;
                    };
                    for source in &sources(&table) {
                        let done = PREPARED
                            .read()
                            .is_ok_and(|p| p.get(source).is_some_and(Result::is_ok));
                        if done {
                            continue;
                        }
                        let result = prepare(source, table.spray_side()).map(Arc::new);
                        let line = match &result {
                            Ok(lump) => format!(
                                "spray: built {source}: {}x{}, key {}",
                                lump.width,
                                lump.height,
                                spray::hex(&lump.key)
                            ),
                            Err(why) => format!("spray: {source} not used -- {why}"),
                        };
                        unsafe { crate::debug::report(&line) };
                        if let Ok(mut prepared) = PREPARED.write() {
                            prepared.insert(source.clone(), result);
                        }
                    }
                }
                PREPARING.store(false, Ordering::Release);
                // A change between the last look and the line above.
                if !PREPARE_AGAIN.load(Ordering::Acquire) || PREPARING.swap(true, Ordering::AcqRel)
                {
                    break;
                }
            }
        });
    if spawned.is_err() {
        PREPARING.store(false, Ordering::Release);
    }
}

// ── The texture ─────────────────────────────────────────────────────────────

/// The cache record `Draw_MiptexTexture` reads: `cacheExtra` at +0x18 must
/// be 0x20; the name at +0 is only printed when it isn't.
static WAD_NAME: &[u8] = b"dodstudio_hltv_sprays\0";

/// The texture's name: `{hltvspray` and a number, unique per spray in the
/// session, so `GL_LoadTexture2` (which hands back an existing texture of
/// the same name and size) never confuses two sprays or one with a real
/// customization's `{%03i%02i`. `Draw_MiptexTexture` rewrites the first
/// character to `{` or `}` from the palette.
fn texture_name(index: usize) -> [u8; 16] {
    let mut name = [0u8; 16];
    let text = format!("{{hltvspray{:04}", index % 10_000);
    name[..text.len()].copy_from_slice(text.as_bytes());
    name
}

impl Texture {
    fn new(lump: Arc<Lump>, index: usize) -> Self {
        let len = TEX_PREFIX + lump.bytes.len() + PALETTE_PAD;
        let buf: &'static mut [u8] = Box::leak(vec![0u8; len].into_boxed_slice());
        Texture {
            buf,
            lump,
            name: texture_name(index),
        }
    }

    /// Lays the lump out as `Draw_CacheLoadFromCustom` would and runs the
    /// engine's `Draw_MiptexTexture` on it. Every spray message does this
    /// again: `GL_LoadTexture2` returns the existing GL texture when it
    /// still has one by this name and size, so it only uploads again after
    /// the engine let it go. Returns the `texture_t`.
    fn build(&mut self, miptex: usize) -> Option<usize> {
        let lump = &self.lump.bytes;
        self.buf.fill(0);
        self.buf[TEX_PREFIX..TEX_PREFIX + lump.len()].copy_from_slice(lump);
        self.buf[TEX_PREFIX..TEX_PREFIX + 16].copy_from_slice(&self.name);
        let mut wad = [0u32; 16];
        wad[0] = WAD_NAME.as_ptr() as usize as u32;
        wad[6] = TEX_PREFIX as u32;
        // Safety: `miptex` is Draw_MiptexTexture(cachewad_t *, void *), cdecl,
        // found by signature; `wad` has the one field it reads, and `buf`
        // holds a lump `spray::lump_key` checked the way
        // Draw_ValidateCustomLogo does, with a whole palette's slack after.
        let draw: unsafe extern "C" fn(*const u32, *mut u8) =
            unsafe { std::mem::transmute::<usize, _>(miptex) };
        BUILDING.store(true, Ordering::Relaxed);
        unsafe { draw(wad.as_ptr(), self.buf.as_mut_ptr()) };
        BUILDING.store(false, Ordering::Relaxed);
        let texnum = u32::from_le_bytes(self.buf[0x18..0x1c].try_into().ok()?);
        (texnum != 0).then_some(self.buf.as_ptr() as usize)
    }
}

/// The `texture_t` for `lump`, built (again) for this message.
fn texture_for(lump: &Arc<Lump>) -> Option<usize> {
    let miptex = MIPTEX.load(Ordering::Acquire);
    if miptex == 0 {
        return None;
    }
    let mut textures = TEXTURES.lock().ok()?;
    let index = textures.len();
    textures
        .entry(lump.key)
        .or_insert_with(|| Texture::new(lump.clone(), index))
        .build(miptex)
}

// ── The callback ────────────────────────────────────────────────────────────

pub fn playing_demo() -> bool {
    let Some(engfuncs) = engine::engfuncs() else {
        return false;
    };
    let api = engfuncs.p_demo_api;
    // Safety: the engine's own demo API table, for the session.
    !api.is_null() && unsafe { ((*api).is_playingback)() } != 0
}

/// A slot's name and SteamID (`slot` 0-based, as `cl.players` and the
/// spray texture names count), the way `dodstudio_deathmsg players` reads
/// them.
pub fn player(slot: usize) -> Option<(String, u64)> {
    let engfuncs = engine::engfuncs()?;
    let mut info = engine::HudPlayerInfo::empty();
    // Safety: the engine's own call, with an entity number 1..=32.
    unsafe { (engfuncs.pfn_get_player_info)(slot as i32 + 1, &mut info) };
    if info.name.is_null() {
        return None;
    }
    // Safety: a NUL-terminated name in the engine's player array.
    let name = unsafe { CStr::from_ptr(info.name) }
        .to_string_lossy()
        .into_owned();
    let steam_id = if info.steam_id != 0 {
        info.steam_id
    } else {
        unsafe { crate::deathmsg::steam_id_from_record(info.name) }
    };
    Some((name, steam_id))
}

fn warn_once(source: &Choice, line: String) {
    let Ok(mut warned) = WARNED.lock() else {
        return;
    };
    if !warned.contains(source) {
        warned.push(source.clone());
        unsafe { crate::debug::report(&line) };
    }
}

/// Whether the demo playing is an HLTV one: a spectator-only client, the
/// test `deathmsg` and `anim_fix` use.
pub fn hltv() -> bool {
    engine::engfuncs().is_some_and(|e| unsafe { (e.is_spectate_only)() } != 0)
}

/// Whether the player in `slot` has a `customdata`: the spray the demo
/// saved for them, the same test the engine's own code makes.
pub fn has_customdata(slot: usize) -> bool {
    let base = CUSTOMDATA.load(Ordering::Acquire);
    if base == 0 || slot >= MAX_PLAYERS {
        return false;
    }
    // Safety: `cl.players[slot]`'s list head in hw.dll's data, read out of
    // Draw_DecalTexture's own code at install.
    unsafe { ((base + slot * PLAYER_STRIDE as usize) as *const u32).read_volatile() != 0 }
}

/// What the Players table makes of the spray `slot` (0-based) shoots now,
/// in the demo playing. `None` outside a demo, with the table at its
/// defaults or `dodstudio_player_overrides` 0, or with nobody in the slot:
/// the engine's own then.
pub fn choice_for_slot(slot: usize) -> Option<Choice> {
    if !ACTIVE.load(Ordering::Relaxed)
        || !crate::player_overrides::on()
        || slot >= MAX_PLAYERS
        || !playing_demo()
    {
        return None;
    }
    let (name, steam_id) = player(slot)?;
    let table = current_table()?;
    let hltv = hltv();
    Some(table.resolve(
        table.choose(hltv, &name, steam_id),
        has_customdata(slot),
        hltv,
    ))
}

/// Called by the `CL_ParseTEnt` stub for every spray. Returns the
/// `texture_t` to shoot it with, or 0 for the engine's own: their saved spray
/// when they have one, the wad decal when not.
unsafe extern "C" fn slot_texture(playernum: i32) -> usize {
    if !ACTIVE.load(Ordering::Relaxed) {
        return 0;
    }
    let Ok(slot) = usize::try_from(playernum) else {
        return 0;
    };
    let saved = has_customdata(slot);
    let choice = match choice_for_slot(slot) {
        Some(Choice::Png(png)) => Choice::Png(png),
        Some(Choice::Blank) => Choice::Blank,
        // The game's own: their saved spray from here on (the decals of ours
        // already up turn into it too, as the engine's own do).
        _ => {
            if saved {
                SLOT_OVER[slot].store(0, Ordering::Release);
            }
            return 0;
        }
    };
    let name = player(slot).map(|(n, _)| n).unwrap_or_default();
    let lump = match PREPARED.read().ok().and_then(|p| p.get(&choice).cloned()) {
        Some(Ok(lump)) => lump,
        Some(Err(_)) => return 0, // said when it failed
        None => {
            warn_once(
                &choice,
                format!(
                    "spray: TE_PLAYERDECAL for {name}: {choice} isn't built yet, the game's own"
                ),
            );
            return 0;
        }
    };
    let Some(texture) = texture_for(&lump) else {
        warn_once(
            &choice,
            format!("spray: TE_PLAYERDECAL for {name}: the engine made no texture from {choice}"),
        );
        return 0;
    };
    SLOT_TEX[slot].store(texture, Ordering::Release);
    if saved {
        SLOT_OVER[slot].store(texture, Ordering::Release);
    }
    DRAWN.fetch_add(1, Ordering::Relaxed);
    unsafe {
        crate::debug::report(&format!(
            "spray: TE_PLAYERDECAL for {name} (slot {}, {}) -> {choice}",
            slot + 1,
            if saved {
                "over the spray the demo saved"
            } else {
                "no spray saved"
            }
        ))
    };
    texture
}

// ── Reporting ───────────────────────────────────────────────────────────────

/// How a PNG the table names stands, for `dodstudio_spray players`.
pub fn png_state(png: &str) -> String {
    let source = Choice::Png(png.to_string());
    match PREPARED.read().ok().and_then(|p| p.get(&source).cloned()) {
        Some(Ok(lump)) => format!("built {}x{}", lump.width, lump.height),
        Some(Err(why)) => format!("not used: {why}"),
        None if ACTIVE.load(Ordering::Relaxed) => "being read".to_string(),
        None => "not read: the table is read once a second".to_string(),
    }
}

/// One line: installed or not, and how many sprays it drew.
pub fn status() -> String {
    if INSTALLED.load(Ordering::Relaxed) {
        let build = BUILD_NAME.lock().map(|n| *n).unwrap_or_default();
        format!(
            "on ({build} hw.dll), {} spray(s) drawn this session",
            DRAWN.load(Ordering::Relaxed)
        )
    } else if GAVE_UP.load(Ordering::Relaxed) {
        "not installed (see the log)".to_string()
    } else if ACTIVE.load(Ordering::Relaxed) && !crate::player_overrides::on() {
        format!("off while {} is 0", crate::player_overrides::NAME)
    } else if ACTIVE.load(Ordering::Relaxed) {
        "installing".to_string()
    } else {
        "off while Studio's Players table is at its defaults".to_string()
    }
}

/// The `dodstudio_debug_status` line, once installed.
pub fn status_line() -> Option<String> {
    INSTALLED
        .load(Ordering::Relaxed)
        .then(|| format!("Sprays with no image in the demo: {}", status()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(pattern: &str) -> Vec<Option<u8>> {
        pattern
            .split_whitespace()
            .map(|t| u8::from_str_radix(t, 16).ok())
            .collect()
    }

    #[test]
    fn patterns_parse_and_start_with_the_stolen_bytes() {
        for build in &BUILDS {
            for p in [build.te_pattern, build.decal_texture, build.miptex_texture] {
                assert!(scan::Pattern::parse(p).is_ok(), "{}: {p}", build.name);
            }
            let te = bytes(build.te_pattern);
            let head: Vec<u8> = te[..build.te_stolen.len()]
                .iter()
                .map(|b| b.expect("no wildcards in the stolen bytes"))
                .collect();
            assert_eq!(head, build.te_stolen, "{}", build.name);
            let dt: Vec<Option<u8>> = DT_STOLEN.iter().map(|&b| Some(b)).collect();
            assert_eq!(bytes(build.decal_texture)[..6], dt[..], "{}", build.name);
            // The customdata address is the match's wildcard dword.
            assert!(
                bytes(build.decal_texture)[build.customdata_at..build.customdata_at + 4]
                    .iter()
                    .all(Option::is_none),
                "{}",
                build.name
            );
        }
    }

    /// The span is the customdata test, its `je` to the wad-decal branch,
    /// and the texture test; the `je` lands on `else_at`, and the custom
    /// branch starts right after the `je` that follows the span.
    #[test]
    fn the_span_and_targets_agree_with_the_pattern() {
        for build in &BUILDS {
            let n = build.cust_test.len();
            let mut span = build.cust_test.to_vec();
            span.push(0x74);
            span.push(build.te_stolen[n + 1]);
            span.extend_from_slice(build.tex_test);
            assert_eq!(span, build.te_stolen, "{}", build.name);
            assert_eq!(
                n + 2 + build.te_stolen[n + 1] as usize,
                build.else_at,
                "{}: the je's target",
                build.name
            );
            // `je rel32` (6 bytes) right after the span, then the custom branch.
            let te = bytes(build.te_pattern);
            let after = build.te_stolen.len();
            assert_eq!(
                &te[after..after + 2],
                &[Some(0x0f), Some(0x84)],
                "{}",
                build.name
            );
            assert_eq!(after + 6, build.custom_at, "{}", build.name);
        }
    }

    #[test]
    fn the_te_stub_assembles_to_what_the_comment_claims() {
        let code = te_stub(
            &BUILDS[0],
            0x1111_1111,
            0x2222_2222,
            0x3333_3333,
            0x4444_4444,
        );
        #[rustfmt::skip]
        assert_eq!(
            code,
            vec![
                0x51,                               //  0 push ecx
                0xff, 0x75, 0xc8,                   //  1 push [ebp-0x38]
                0xb8, 0x11, 0x11, 0x11, 0x11,       //  4 mov  eax, slot_texture
                0xff, 0xd0,                         //  9 call eax
                0x83, 0xc4, 0x04,                   // 11 add  esp, 4
                0x59,                               // 14 pop  ecx
                0x85, 0xc0,                         // 15 test eax, eax
                0x74, 0x09,                         // 17 jz   .game (28)
                0x89, 0x45, 0xd4,                   // 19 mov  [ebp-0x2c], eax
                0xff, 0x25, 0x22, 0x22, 0x22, 0x22, // 22 jmp  [TE_CUSTOM]
                0x3b, 0xfe,                         // 28 .game: cmp edi, esi
                0x75, 0x06,                         // 30 jne  .cust (38)
                0xff, 0x25, 0x33, 0x33, 0x33, 0x33, // 32 jmp  [TE_ELSE]
                0x39, 0x75, 0xd4,                   // 38 .cust: cmp [ebp-0x2c], esi
                0xff, 0x25, 0x44, 0x44, 0x44, 0x44, // 41 jmp  [TE_RESUME]
            ]
        );
        let post = te_stub(
            &BUILDS[1],
            0x1111_1111,
            0x2222_2222,
            0x3333_3333,
            0x4444_4444,
        );
        #[rustfmt::skip]
        assert_eq!(
            post,
            vec![
                0x51,                               //  0 push ecx (a saved spray's texture)
                0xff, 0x75, 0xa4,                   //  1 push [ebp-0x5c]
                0xb8, 0x11, 0x11, 0x11, 0x11,       //  4 mov  eax, slot_texture
                0xff, 0xd0,                         //  9 call eax
                0x83, 0xc4, 0x04,                   // 11 add  esp, 4
                0x59,                               // 14 pop  ecx
                0x85, 0xc0,                         // 15 test eax, eax
                0x74, 0x08,                         // 17 jz   .game (27)
                0x8b, 0xc8,                         // 19 mov  ecx, eax
                0xff, 0x25, 0x22, 0x22, 0x22, 0x22, // 21 jmp  [TE_CUSTOM]
                0x85, 0xf6,                         // 27 .game: test esi, esi
                0x75, 0x06,                         // 29 jne  .cust (37)
                0xff, 0x25, 0x33, 0x33, 0x33, 0x33, // 31 jmp  [TE_ELSE]
                0x85, 0xc9,                         // 37 .cust: test ecx, ecx
                0xff, 0x25, 0x44, 0x44, 0x44, 0x44, // 39 jmp  [TE_RESUME]
            ]
        );
    }

    #[test]
    fn the_decal_texture_stub_assembles_to_what_the_comment_claims() {
        let code = dt_stub(0x1111_1111, 0x5555_5555, 0x2222_2222, 0x3333_3333);
        #[rustfmt::skip]
        assert_eq!(
            code,
            vec![
                0x8b, 0x44, 0x24, 0x04,                   //  0 mov  eax, [esp+4]
                0xf7, 0xd0,                               //  4 not  eax
                0x83, 0xf8, 0x20,                         //  6 cmp  eax, 32
                0x73, 0x28,                               //  9 jae  .game (51)
                0x8b, 0xc8,                               // 11 mov  ecx, eax
                0x69, 0xc9, 0x50, 0x02, 0x00, 0x00,       // 13 imul ecx, ecx, 0x250
                0x83, 0xb9, 0x22, 0x22, 0x22, 0x22, 0x00, // 19 cmp  dword [ecx + customdata], 0
                0x75, 0x09,                               // 26 jne  .saved (37)
                0x8b, 0x0c, 0x85, 0x11, 0x11, 0x11, 0x11, // 28 mov  ecx, [SLOT_TEX + eax*4]
                0xeb, 0x07,                               // 35 jmp  .test (44)
                0x8b, 0x0c, 0x85, 0x55, 0x55, 0x55, 0x55, // 37 .saved: mov ecx, [SLOT_OVER + eax*4]
                0x85, 0xc9,                               // 44 .test: test ecx, ecx
                0x74, 0x03,                               // 46 jz   .game (51)
                0x8b, 0xc1,                               // 48 mov  eax, ecx
                0xc3,                                     // 50 ret
                0x55, 0x8b, 0xec, 0x8b, 0x4d, 0x08,       // 51 .game: the stolen prologue
                0xff, 0x25, 0x33, 0x33, 0x33, 0x33,       // 57 jmp  [DT_RESUME]
            ]
        );
    }

    #[test]
    fn texture_names_are_unique_and_fit() {
        let a = texture_name(1);
        assert_eq!(&a[..14], b"{hltvspray0001");
        assert_eq!(a[14..], [0, 0]);
        assert_ne!(texture_name(2), a);
        assert!(!spray::looks_like_custom_name("{hltvspray0001"));
    }
}
