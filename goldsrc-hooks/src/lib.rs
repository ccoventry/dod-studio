//! Standalone companion DLL for DoD 1.3 GoldSrc capture sessions, injected
//! into `hl.exe` alongside (not instead of) HLAE's own `AfxHookGoldSrc.dll`.
//!
//! Unlike patching HLAE's own source, this doesn't need HLAE's build
//! toolchain, and doesn't touch any function HLAE itself already hooks (see
//! `engine.rs` for exactly which functions this DLL depends on). It's built
//! as a separate workspace crate specifically so it doesn't have to live
//! inside a forked copy of HLAE's own repository. How it gets a foothold in
//! `client.dll` is genuinely non-obvious -- patching that DLL's export table
//! does nothing on a "secured" build, because the engine resolves the whole
//! client interface through a single `F` export instead. See `engine.rs`'s
//! module docs and `docs/goldsrc_client_dll_internals.md`.
//!
//! Implements two fixes and eight control surfaces, each independent of the
//! others and each safe to inject without them:
//! - `dodstudio_spec_match_pov`, one cvar over five modules, for making a
//!   spectated first-person view look and sound like the player's own
//!   recording:
//!   - `anim_fix`: drive the first-person viewmodel's animations -- shoot,
//!     reload, draw, idle, grenades -- which the engine otherwise leaves
//!     static. Full design write-up in `docs/goldsrc_hltv_animation_fix.md`.
//!   - `missing_shots`: an HLTV demo carries the fire event for well under
//!     half the rounds fired; the rest are found from the shooter's body
//!     animation restarting and get the weapon's own event handler called for
//!     them (sound, flash, impact).
//!   - `spectator_crosshair`: draw the spectator crosshair from the same
//!     sprite and tile a player's own `cl_xhair_style` picks, since the two
//!     are drawn by different code paths and do not otherwise share a look.
//!   - `spectator_eye`: the in-eye camera drops to the ground for a prone
//!     player, where the game leaves it at crouch height.
//! - `fire_sounds`: the `EV_PlaySound` hook the first two of those hear
//!   gunshots through. It changes no sound.
//! - `deathmsg`: the `dodstudio_deathmsg` command -- raise the four-line cap on
//!   the kill feed, move it, hide frags, or inject one. HLAE's own
//!   `mirv_deathmsg` covers only `cstrike` and `tfc`, so none of it works for
//!   DoD. Full design write-up in `docs/goldsrc_death_notices.md`.
//! - `msglog`: the `dodstudio_debug_msglog` command -- dump chosen DoD user messages
//!   and their payloads to the log, forwarded to the game untouched. Full
//!   design write-up in the module doc itself.
//! - `hide_entity`: the `dodstudio_hide_entity <model-path>...` command (old
//!   name `dodstudio_hide_sprite`, #333) -- suppress specific world entities
//!   by model path, an allow-list rather than a blanket toggle. Full design
//!   write-up in the module doc itself (issue #315).
//! - `spectator_follow`: `dodstudio_spec_lock`, which keeps the camera on a
//!   player through his death in an HLTV demo, and `dodstudio_spec_target`,
//!   which puts it on a player by number (issue #206).
//! - `scoreboard`: the `dodstudio_hide_scoreboard` cvar -- stop a POV demo's
//!   recorded TAB presses from putting the scoreboard over the shot, without
//!   editing `ScoreBoard.res`. Full design write-up in
//!   `docs/goldsrc_scoreboard.md`.
//! - `voice`: the `dodstudio_mute_voice_commands` cvar -- silence "fire in the
//!   hole!" and the rest, without overwriting the game's own `.wav` files.
//! - `crosshair`: the `dodstudio_hide_crosshair` cvar -- hide the crosshair and have
//!   it stay hidden, which the stock `crosshair` cvar cannot do because
//!   `CHud::Redraw` forces the value back every frame.
//! - `objicons`: the `dodstudio_objectives` command -- place the objective
//!   (territory flag) icon row and timer, which the game itself draws at a
//!   different y while spectating than it does in a POV demo. Full design
//!   write-up in `docs/goldsrc_objective_icons.md`.
//!
//! - `lightmap_gamma`: the first demo of a session no longer renders with its
//!   lighting far too dark (issue #365): the gamma tables are refreshed from the
//!   current cvars before a map's lightmaps are built. On by default;
//!   `GOLDSRC_HOOKS_LIGHTMAP_GAMMA=0` turns it off.
//! - `tempent_fix`: stop DoD's client crashing when the engine has no temp
//!   effect entity to give it (issue #374). On by default, since it only acts
//!   where the game would otherwise crash; `GOLDSRC_HOOKS_TEMPENT_FIX=0` turns
//!   it off.
//! - `hull_trace_guard`: stop the engine crashing when a player-movement trace
//!   walks a previous map's collision data (issue #384). On by default for the
//!   same reason; `GOLDSRC_HOOKS_HULL_TRACE_GUARD=0` turns it off.
//! - `demo_list_folders`: the `dodstudio_demo_list_folders` cvar -- the Load
//!   Demo window lists folders (and `../`) as well as demos, and opens them,
//!   so `viewdemo` can reach demos outside `dod/` (issue #408).
//! - `demo_seek`: the `dodstudio_seek_to` / `dodstudio_seek_by` commands --
//!   jump `viewdemo` playback to a time, as the demo editor's Goto does,
//!   through `DemoPlayer.dll`'s own interface (issue #405). Nothing calls them
//!   yet; they are there for a live test.
//! - `frame_esc`: keep GameUI's windows -- the VCR bar, the events list, the
//!   Load Demo window, the console -- open when ESC is pressed on the 25th
//!   Anniversary build (issues #369, #408). Does nothing on the pre-Anniversary
//!   build, which never closed them; `GOLDSRC_HOOKS_FRAME_ESC=0` turns it off.
//! - `window_layout`: the `dodstudio_resizable_windows` and
//!   `dodstudio_remember_window_layout` cvars -- every GameUI window can be
//!   resized, and each comes back where it was left after a restart (#408).
//! - `studio_panel`: `dodstudio_panel` -- DoD Studio's own window in the game,
//!   a GameUI `Frame` with real tabs (a `PropertySheet` of `PropertyPage`s)
//!   and VCR buttons, each tab laid out by its own `.res` (#408, plan item 4).
//! - `events`: the game tells DoD Studio what a capture batch is doing over a
//!   second local named pipe, `\\.\pipe\dodstudio-hl-<pid>-events` (issue #434,
//!   step 1), instead of Studio reading `qconsole.log`. `GOLDSRC_HOOKS_EVENTS=0`
//!   turns it off.
//! - `batch_end`: when a batch's `BATCH_COMPLETE` goes by and no Studio is on
//!   the events pipe, the game quits itself after a few seconds instead of
//!   sitting there (issue #545). On with the events pipe.
//! - `pmove_guard`: stop the session's first demo crashing when it sends
//!   `InitHUD` before the engine has pointed `pmove` anywhere (issue #546).
//!   One pointer write at start-up, both builds. On by default;
//!   `GOLDSRC_HOOKS_PMOVE_GUARD=0` turns it off.
//! - `sprite_blend`: `gl_spriteblend 0` at the session's first sprite load no
//!   longer darkens sprites until the game restarts (issue #467). Two bytes in
//!   `GL_Upload32`, both builds. On by default; `GOLDSRC_HOOKS_SPRITEBLEND_FIX=0`
//!   turns it off.
//! - `remote`: DoD Studio can send console commands to the running game over
//!   a local named pipe, `\\.\pipe\dodstudio-hl-<pid>` (issue #413) -- e.g.
//!   Launch Preview while the game is open. `GOLDSRC_HOOKS_REMOTE=0` turns it
//!   off.
//! - `connect_guard`: refuse `connect`, `retry`, `reconnect` and `listen`
//!   while the DLL is loaded, since joining a VAC-secured server with it
//!   loaded is a ban risk (issue #451). `connect local` (`map`) still works;
//!   `GOLDSRC_HOOKS_ALLOW_CONNECT=1` turns it off.
//! - `world_shaders`: the `dodstudio_allow_shaders` cvar -- let the 25th
//!   Anniversary engine draw the world through `platform/gl_shaders` during
//!   demo playback, which its `sv_allow_shaders` gate otherwise forbids.
//! - `map_text`: the `dodstudio_hide_map_text` cvar -- hide the text a map
//!   puts on screen itself (the anzio mortar warning, the round result), and
//!   pass DoD's own `HudText` prompts through (issue #287).
//! - `hltv_messages`: the `dodstudio_hide_hltv_messages` cvar -- drop the
//!   HLTV proxy's on-screen text ("You're watching HLTV...") as it arrives,
//!   instead of patching it out of the demo (issue #30).
//! - `spectator_bars`: the `dodstudio_hide_spectator_bars` cvar -- hide the
//!   two dark bands a spectator sees and everything on them, on screen and
//!   without a capture running (issue #328). A filter on vgui2's `IPanel::PaintTraverse`;
//!   see `docs/goldsrc_spectator_bars.md`.
//! - `spectator_hud`: no command of its own -- while spectating, keeps the
//!   objectives, the objective timer, the kill feed and the minimap just below
//!   the spectator bar, or at the top as in a POV demo while the bar is hidden.
//!
//! The scoreboard/voice/crosshair/spectator_crosshair four are all in
//! `docs/goldsrc_hud_suppression.md`.
//!
//! See each module's docs for the full R&D reasoning.
//!
//! The settings are `dodstudio_*` **cvars**, so they behave like any other
//! engine setting: `dodstudio_spec_match_pov 1` from the console,
//! `+dodstudio_spec_match_pov 1` on the launch line, or a line in any `.cfg` the
//! user execs. `commands.rs` copies them into the runtime flags once per
//! frame, and `dodstudio_debug_status` reports what each part is actually
//! doing rather than only what it is set to.
//!
//! The `GOLDSRC_HOOKS_SPEC_MATCH_POV` environment variable sets that cvar's
//! starting value, but the launch line is the better route: it is visible in
//! the command that started the session.

mod anim_fix;
mod batch_end;
mod cmd_list;
mod commands;
mod connect_guard;
mod crash;
mod crosshair;
mod deathmsg;
mod debug;
mod decals;
mod demo_file;
mod demo_list_folders;
mod demo_reload;
mod demo_rosters;
mod demo_seek;
mod detour;
mod engine;
mod events;
mod ex_interp;
mod fire_sounds;
mod folder_counts;
mod frame_esc;
mod hand_signals;
mod hide_entity;
mod hltv_messages;
mod hudelement;
mod hull_trace_guard;
mod lightmap_gamma;
mod map_text;
mod missing_shots;
mod msglog;
mod names;
mod objicons;
mod overview_map;
mod patch;
mod pe;
mod pmove_guard;
mod remote;
mod scan;
mod scoreboard;
mod server_query;
mod spectator_bars;
mod spectator_crosshair;
mod spectator_eye;
mod spectator_follow;
mod spectator_gun;
mod spectator_hud;
mod spectator_target;
mod sprite_blend;
mod streaks;
mod studio_panel;
mod tempent_fix;
mod texture_hires;
mod voice;
mod window_layout;
mod world_shaders;

use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateThread, Sleep};

/// Reads a `GOLDSRC_HOOKS_*` flag, falling back to `default` when unset.
///
/// An explicit "0" always wins, so a fix that defaults on can still be turned
/// off without a rebuild.
fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => value.trim() == "1",
        Err(_) => default,
    }
}

/// Reads a `GOLDSRC_HOOKS_*` iteration number, falling back to `default`.
///
/// Out of range is clamped rather than refused, so a value left over from an
/// older setting with more steps still means on. Unparseable reads as off,
/// which is the safe way to land.
fn env_level(name: &str, default: i32) -> i32 {
    match std::env::var(name) {
        Ok(value) => value.trim().parse::<i32>().unwrap_or(0),
        Err(_) => default,
    }
    .clamp(anim_fix::LEVEL_OFF, anim_fix::LEVEL_MAX)
}

/// Matching POV starts **off**: a capture pipeline should not silently alter
/// what a spectated view shows for anyone who happens to have the DLL loaded.
/// Turn it on per session with `dodstudio_spec_match_pov 1`, or set
/// `GOLDSRC_HOOKS_SPEC_MATCH_POV` to have it start on. The value lands in
/// `anim_fix::LEVEL`, which `commands.rs` hands the engine as the cvar's
/// default, and the cvar then drives every part.
const SPEC_MATCH_POV_DEFAULT: i32 = anim_fix::LEVEL_OFF;

/// Whether to install `texture_hires` at startup: see
/// `texture_hires::starts_on`. Off, `dodstudio_hd_enabled 1` can still install it
/// later in the session.
static TEXTURE_HIRES_ENABLED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn worker_thread(_lp_param: *mut std::ffi::c_void) -> u32 {
    // First, so the file is there before GameUI reads the main menu.
    if env_flag("GOLDSRC_HOOKS_GAME_MENU", true) {
        studio_panel::write_game_menu();
    }
    anim_fix::LEVEL.store(
        env_level("GOLDSRC_HOOKS_SPEC_MATCH_POV", SPEC_MATCH_POV_DEFAULT),
        Ordering::Relaxed,
    );
    // Only makes the first map's lighting match every later map's, so on
    // unless asked not to.
    lightmap_gamma::ENABLED.store(
        env_flag("GOLDSRC_HOOKS_LIGHTMAP_GAMMA", true),
        Ordering::Relaxed,
    );
    // A crash fix rather than a capture setting, so on unless asked not to.
    tempent_fix::ENABLED.store(
        env_flag("GOLDSRC_HOOKS_TEMPENT_FIX", true),
        Ordering::Relaxed,
    );
    hull_trace_guard::ENABLED.store(
        env_flag("GOLDSRC_HOOKS_HULL_TRACE_GUARD", true),
        Ordering::Relaxed,
    );
    // Restores the pre-Anniversary behaviour on the Anniversary build and
    // does nothing on the pre-Anniversary one, so on unless asked not to.
    frame_esc::ENABLED.store(env_flag("GOLDSRC_HOOKS_FRAME_ESC", true), Ordering::Relaxed);
    // A crash fix, so on unless asked not to.
    pmove_guard::ENABLED.store(
        env_flag("GOLDSRC_HOOKS_PMOVE_GUARD", true),
        Ordering::Relaxed,
    );
    // Restores the engine's own default upload behaviour, so on unless asked
    // not to.
    sprite_blend::ENABLED.store(
        env_flag("GOLDSRC_HOOKS_SPRITEBLEND_FIX", true),
        Ordering::Relaxed,
    );
    // Only this user's own processes can reach the pipe, and only a game
    // Studio launched has it, so on unless asked not to.
    remote::ENABLED.store(env_flag("GOLDSRC_HOOKS_REMOTE", true), Ordering::Relaxed);
    // VAC safety (#451), so on unless asked not to.
    connect_guard::ENABLED.store(
        !connect_guard::allowed_by_env(std::env::var(connect_guard::ALLOW_ENV).ok().as_deref()),
        Ordering::Relaxed,
    );
    // Studio falls back to qconsole.log without it, so on unless asked not to.
    events::ENABLED.store(env_flag("GOLDSRC_HOOKS_EVENTS", true), Ordering::Relaxed);
    // HD textures: on when there's a dod/dodstudio_hd folder to load from,
    // unless GOLDSRC_HOOKS_TEXTURE_HIRES says otherwise (see
    // texture_hires::starts_on for why startup decides). `dodstudio_hd_enabled` turns
    // it on and off in game.
    let hd = texture_hires::starts_on();
    TEXTURE_HIRES_ENABLED.store(hd, Ordering::Relaxed);
    texture_hires::set_enabled(hd);

    unsafe { debug::new_session_separator() };
    unsafe { debug::report("goldsrc-hooks worker thread started") };

    // Matching POV defaults to OFF, so a session where the hooks all install
    // correctly but nothing visibly changes is the expected outcome of simply
    // not having turned it on. Log the starting state so that case is obvious
    // from the log rather than mistaken for a broken hook.
    unsafe {
        debug::report(&format!(
            "goldsrc-hooks: starting state -- {}: {} ({}) (GOLDSRC_HOOKS_SPEC_MATCH_POV sets the default; the cvar toggles it live)",
            names::SPEC_MATCH_POV,
            anim_fix::level(),
            anim_fix::level_description(anim_fix::level()),
        ))
    };

    // Before anything else hooks anything: GoldSrc swallows its own
    // unhandled exceptions and exits without a dump or an event-log entry,
    // so without this a crash in an engine-thread callback is untraceable.
    crash::install();

    // Registered before install() so there's no window in which Initialize
    // could fire before the callback exists.
    engine::set_on_engine_ready(install_fixes);

    // Hooks hw.dll's LoadLibraryA and GetProcAddress imports, so we see
    // client.dll load and can substitute our own entry points as the engine
    // resolves it -- see engine.rs's module docs for the full mechanism and
    // why patching client.dll's export table does nothing.
    engine::install();

    // Nothing left to do but report if the fixes never activated. pEngfuncs
    // arrives when the engine calls client.dll's Initialize during normal
    // startup, and `install_fixes` runs from there, on the engine's thread.
    let mut waited = 0u32;
    while engine::engfuncs().is_none() {
        if waited >= 30_000 {
            unsafe {
                debug::report(
                    "goldsrc-hooks: timed out waiting for client.dll's Initialize to run; fixes not installed this session",
                )
            };
            return 0;
        }
        unsafe { Sleep(50) };
        waited += 50;
    }

    0
}

/// Runs on the engine's own thread, once `client.dll`'s `Initialize` has
/// returned and `pEngfuncs` is live -- see `engine::set_on_engine_ready` for
/// why this must not run from the worker thread.
fn install_fixes() {
    unsafe { debug::report("goldsrc-hooks: engfuncs captured, installing fixes") };
    fire_sounds::install();

    // anim_fix additionally needs engine_studio, captured when the engine
    // calls HUD_GetStudioModelInterface; install() itself only registers the
    // per-frame callback, which re-checks that availability on every call, so
    // it's safe to install even if that capture hasn't landed yet.
    anim_fix::install();

    // The dodstudio_* console surface. dodstudio_spec_match_pov drives the same
    // flag the env var above set as its starting value.
    commands::install();

    // hw.dll, once; before the first map loads, which is the one it's for.
    lightmap_gamma::install();

    // Studio's console commands can only run once the engine's function
    // table is live, which is now.
    remote::start();

    // Also re-runs if client.dll is ever loaded again: install() compares the
    // module base and patches the new copy.
    tempent_fix::install();
    // hw.dll is loaded for the whole session, so once is enough.
    hull_trace_guard::install();
    // Before any map loads, so before the first demo's InitHUD.
    pmove_guard::install();
    // Before any map loads, so before the first HUD sprite is uploaded.
    sprite_blend::install();

    if TEXTURE_HIRES_ENABLED.load(Ordering::Relaxed) {
        match texture_hires::install() {
            Ok(()) => unsafe {
                debug::report("goldsrc-hooks: texture_hires hook installed (HD textures on)")
            },
            Err(why) => unsafe {
                debug::report(&format!(
                    "goldsrc-hooks: texture_hires hook not installed -- {why}"
                ))
            },
        }
    }
}

/// # Safety
///
/// Only ever called by the Windows loader itself, per the standard `DllMain`
/// contract -- never call this directly.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    _hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        // Do as little as possible directly in DllMain (loader-lock rules --
        // no LoadLibrary, no waiting, ideally no allocation). Hand off to a
        // worker thread immediately instead.
        unsafe {
            CreateThread(
                std::ptr::null(),
                0,
                Some(worker_thread),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            );
        }
    }
    TRUE
}
