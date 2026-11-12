//! Notices when a demo stops playing because the file is damaged, and tells
//! the player and Studio (part of #1373).
//!
//! ## What a damaged demo does
//!
//! The engine reads a demo frame by frame and gives up at the first frame it
//! cannot make sense of, with no crash: the demo just ends. Measured on four
//! damaged Monday demos (2026-11-10, both builds), it ends one of two ways:
//!
//! - `Host_EndGame: Demo message > MAX_POSSIBLE_MSG`, from the frame reader,
//!   which also ends with `Bad demo length.`, `Demo message length < 0.` and
//!   `Error reading demo message data.` (`docs/goldsrc_hw_dll_survey.md`
//!   §13.2);
//! - `Host_Error: CL_ParseServerMessage: Illegible server message - svc_bad`,
//!   from the message parser, which also stops with `Bad server message` when
//!   a message runs past its end.
//!
//! A healthy `playdemo` ends through `Host_EndGame("End of demo.")` from the
//! reader's last `NextSection`, which is not on the list (11 healthy demos,
//! POV and HLTV, measured on PRE).
//!
//! ## Too many entities is not damage
//!
//! `Host_EndGame("CL_ParsePacketEntities: newindex == MAX_PACKET_ENTITIES")`
//! stops a demo that sends more entities than the engine holds: 256 on the
//! pre-Anniversary build, 1024 on the Anniversary one (#207). The file is
//! fine, so it gets its own line and its own event, `DEMO_ENTITY_LIMIT`, with
//! no offer to repair it.
//!
//! ## What it does not catch yet
//!
//! - `viewdemo`: `DemoPlayer`'s loader (`Core.dll`) reads the whole file at
//!   load, prints `WARNING! DemoFile::ReadDemoPacket: ...` there, and plays
//!   the part before the damage; neither function is called.
//! - A file cut short: it has lost its directory, so `playdemo` refuses it at
//!   the start (`Error: Corrupt demo file.`) and nothing plays.
//! - `timedemo`'s demo name: it goes through the same reader and is caught,
//!   but `demo_reload` forgets the remembered name when it runs, so its
//!   notice says the demo is unknown and its event carries no path, rather
//!   than naming the demo played before it.
//!
//! ## The hook
//!
//! One detour on the entry of each of `Host_EndGame` and `Host_Error`, both
//! `void (const char *fmt, ...)`, cdecl. Before the prologue runs, the stub
//! hands [`on_host_end`] the format string and the address of the first
//! variadic argument; the function then runs as before (and never returns:
//! both `longjmp` out of the frame). Matching the format text, not a call
//! site, keeps it to two detours per build and names the reason exactly.
//!
//! [`on_host_end`] runs on the engine's thread, inside the error. It only
//! reads: is a demo playing, which one, where its clock is. It stores that
//! for [`poll`], which on the next frame prints the console line (after the
//! engine's own), logs it and sends one `DEMO_DAMAGED` (or
//! `DEMO_ENTITY_LIMIT`) event to Studio over the events pipe. Nothing runs
//! per frame but one atomic load.
//!
//! `GOLDSRC_HOOKS_DEMO_DAMAGE=0` turns it off for a session.
//!
//! ## Both engines
//!
//! Pre-Anniversary `hw.dll` (1,641,376 bytes): `Host_EndGame` `+0x55ea0`,
//! `Host_Error` `+0x55f60`. 25th Anniversary (3,598,176 bytes): `+0x1d26e0`
//! and `+0x1d27c0`, each with a stack cookie after the same three-instruction
//! prologue. `tools/verify_demo_damage_offsets.py` checks every fact above
//! against each binary.

use std::ffi::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::detour;
use crate::scan;

/// Which of the two functions the stub is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum Which {
    EndGame = 0,
    Error = 1,
}

impl Which {
    fn name(self) -> &'static str {
        match self {
            Which::EndGame => "Host_EndGame",
            Which::Error => "Host_Error",
        }
    }
}

/// One engine build's two functions.
struct Build {
    name: &'static str,
    /// `Host_EndGame`'s entry: unique in that build's `hw.dll` and matching
    /// nothing in the other's.
    end_game: &'static str,
    /// `Host_Error`'s entry, the same way.
    error: &'static str,
    /// `push ebp; mov ebp, esp; sub esp, <locals>`, the start of both: the
    /// bytes the jump overwrites, reproduced verbatim in the stub. Not
    /// relative, and none reads the flags.
    stolen: [u8; STOLEN_LEN],
}

/// The builds the hook knows, tried in order.
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        end_game: "55 8B EC 81 EC 00 04 00 00 8B 4D 08 8D 45 0C 56 50 51 8D 95 00 FC FF FF \
                   68 00 04 00 00 52 E8",
        error: "55 8B EC 81 EC 00 04 00 00 A1 ?? ?? ?? ?? 85 C0 74 0D 68 ?? ?? ?? ?? \
                E8 ?? ?? ?? ?? 83 C4 04 C7 05 ?? ?? ?? ?? 01 00 00 00 E8 ?? ?? ?? ?? \
                8B 4D 08 8D 45 0C 50 51",
        stolen: [0x55, 0x8b, 0xec, 0x81, 0xec, 0x00, 0x04, 0x00, 0x00],
    },
    // The stack cookie's address is a wildcard. `Host_EndGame`'s first 60
    // bytes are also the start of the printer at +0x21fc20 it calls; the
    // `lea`/`push` after the import call tells them apart.
    Build {
        name: "25th Anniversary",
        end_game: "55 8B EC 81 EC 04 04 00 00 A1 ?? ?? ?? ?? 33 C5 89 45 FC 56 8D 45 0C 50 \
                   6A 00 FF 75 08 8D 85 FC FB FF FF 68 00 04 00 00 50 E8 ?? ?? ?? ?? \
                   8B 08 FF 70 04 83 C9 01 51 FF 15 ?? ?? ?? ?? 8D 85 FC FB FF FF 50 68",
        error: "55 8B EC 81 EC 04 04 00 00 A1 ?? ?? ?? ?? 33 C5 89 45 FC 83 3D ?? ?? ?? ?? \
                00 74 0D 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 04 C7 05 ?? ?? ?? ?? \
                01 00 00 00 E8 ?? ?? ?? ?? 8D 45 0C 50 FF 75 08",
        stolen: [0x55, 0x8b, 0xec, 0x81, 0xec, 0x04, 0x04, 0x00, 0x00],
    },
];

/// Both functions start with three instructions, nine bytes.
const STOLEN_LEN: usize = 9;
const _: () = assert!(STOLEN_LEN >= 5, "a jmp rel32 needs 5 bytes");

/// The ends that mean the file is damaged: the function, the start of its
/// format string, and the reason's key in the event.
const REASONS: [(Which, &str, &str); 6] = [
    (
        Which::EndGame,
        "Demo message > MAX_POSSIBLE_MSG",
        "message_too_big",
    ),
    (Which::EndGame, "Bad demo length.", "bad_length"),
    (
        Which::EndGame,
        "Demo message length < 0.",
        "negative_length",
    ),
    (
        Which::EndGame,
        "Error reading demo message data.",
        "read_error",
    ),
    (
        Which::Error,
        "CL_ParseServerMessage: Illegible server message",
        "illegible_message",
    ),
    (
        Which::Error,
        "CL_ParseServerMessage: Bad server message",
        "bad_server_message",
    ),
];

/// The ends that mean the demo has more entities than the engine holds: not
/// damage (see the module doc). The same shape as [`REASONS`].
const LIMITS: [(Which, &str, &str); 1] = [(
    Which::EndGame,
    "CL_ParsePacketEntities: newindex == MAX_PACKET_ENTITIES",
    "entity_limit",
)];

/// Why a demo stopped, of the ends the hook reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    /// One of [`REASONS`].
    Damaged,
    /// One of [`LIMITS`].
    EntityLimit,
}

/// The console line's text, until #1373 settles the wording. `{at}` is the
/// demo time.
const NOTICE: &str = "This demo is damaged at {at} and stopped playing. \
                      DoD Studio can repair it (Demo Auditor, Repair).";

/// The console line for [`Cause::EntityLimit`]. `{at}` is the demo time.
const ENTITY_NOTICE: &str = "This demo stopped playing at {at}: it shows more \
                             entities than this engine can hold. The file is not damaged.";

/// Added to [`ENTITY_NOTICE`] on the pre-Anniversary build, whose limit is
/// the lower one (#207).
const ENTITY_HINT_PRE: &str = " It plays on the 25th Anniversary engine, which holds more.";

/// `{at}` when the demo's clock could not be read.
const UNKNOWN_TIME: &str = "an unknown time";

/// Whether to install at all -- `GOLDSRC_HOOKS_DEMO_DAMAGE=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Where each stub jumps back to: the instruction after the stolen bytes.
static RESUME_END_GAME: AtomicUsize = AtomicUsize::new(0);
static RESUME_ERROR: AtomicUsize = AtomicUsize::new(0);

/// Set once both jumps are in. `hw.dll` never reloads, so once is enough.
static INSTALLED: AtomicBool = AtomicBool::new(false);
static BUILD_NAME: Mutex<&str> = Mutex::new("");
/// Set when the installed build is the pre-Anniversary one, for
/// [`ENTITY_HINT_PRE`].
static PRE_ANNIVERSARY: AtomicBool = AtomicBool::new(false);
/// See [`detour::Detour`] on why these are never undone.
static DETOURS: Mutex<Vec<detour::Detour>> = Mutex::new(Vec::new());

/// Damaged demos noticed this session, for the status line.
static NOTICED: AtomicU32 = AtomicU32::new(0);
/// Set by [`on_host_end`] when [`PENDING`] holds a notice for [`poll`].
static WAITING: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<Damage>> = Mutex::new(None);

/// What [`on_host_end`] saw when a demo stopped on a damaged frame (or at the
/// entity limit).
#[derive(Debug, Clone, PartialEq)]
struct Damage {
    cause: Cause,
    reason: &'static str,
    /// The engine's message, as it prints it after `Host_EndGame: `.
    message: String,
    /// The demo's name as it was played, relative to the game folder.
    demo: Option<String>,
    /// Seconds on the demo player's clock (`viewdemo`'s bar), or the
    /// client's time under `playdemo`.
    at: Option<f64>,
    clock: &'static str,
}

/// The stub, hand-assembled. On entry `[esp+4]` is the format string and
/// `[esp+8]` the first variadic argument.
///
/// ```asm
///         pushad
///         pushfd
///         cld
///         lea  eax, [esp+0x2c]        ; the variadic arguments
///         push eax
///         push dword ptr [esp+0x2c]   ; fmt
///         push which
///         mov  eax, on_host_end
///         call eax
///         add  esp, 12
///         popfd
///         popad
///         <the stolen prologue>
///         jmp  dword ptr [resume]
/// ```
fn stub(which: Which, stolen: &[u8; STOLEN_LEN], callback: usize, resume: usize) -> Vec<u8> {
    let mut code = vec![0x60, 0x9c, 0xfc]; // pushad; pushfd; cld
    code.extend_from_slice(&[0x8d, 0x44, 0x24, 0x2c]); // lea eax, [esp+0x2c]
    code.push(0x50); // push eax
    code.extend_from_slice(&[0xff, 0x74, 0x24, 0x2c]); // push dword ptr [esp+0x2c]
    code.push(0x68); // push imm32
    code.extend_from_slice(&(which as u32).to_le_bytes());
    code.push(0xb8); // mov eax, imm32
    code.extend_from_slice(&(callback as u32).to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0]); // call eax
    code.extend_from_slice(&[0x83, 0xc4, 0x0c]); // add esp, 12
    code.extend_from_slice(&[0x9d, 0x61]); // popfd; popad
    code.extend_from_slice(stolen);
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume as u32).to_le_bytes());
    code
}

/// Patches both functions' entries in the loaded `hw.dll`. A second call
/// does nothing.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "demo_damage: off (GOLDSRC_HOOKS_DEMO_DAMAGE=0) -- a damaged demo just stops (#1373)",
            )
        };
        return;
    }
    if INSTALLED.load(Ordering::Acquire) {
        return;
    }
    let result = crate::engine::engine_module_base()
        .ok_or_else(|| "hw.dll is not loaded yet".to_string())
        .and_then(install_at);
    match result {
        Ok((detours, build, end_game, error)) => {
            if let Ok(mut slot) = DETOURS.lock() {
                slot.extend(detours);
            }
            if let Ok(mut name) = BUILD_NAME.lock() {
                *name = build;
            }
            PRE_ANNIVERSARY.store(build == BUILDS[0].name, Ordering::Relaxed);
            INSTALLED.store(true, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "demo_damage: watching Host_EndGame (+{end_game:#x}) and Host_Error (+{error:#x}) for a damaged demo's end ({build} hw.dll) (#1373)"
                ))
            };
        }
        Err(why) => unsafe {
            crate::debug::report(&format!("demo_damage: not installed -- {why}"))
        },
    }
}

/// Finds both functions with the first of [`BUILDS`] whose signatures match,
/// and patches them. Returns the detours, the build's name and the offsets.
fn install_at(base: usize) -> Result<(Vec<detour::Detour>, &'static str, usize, usize), String> {
    let mut misses = Vec::new();
    for build in &BUILDS {
        // Safety: `base` is a module handle the loader gave us.
        let found = unsafe {
            scan::find_unique(base, build.end_game)
                .and_then(|e| scan::find_unique(base, build.error).map(|r| (e, r)))
        };
        let (end_game, error) = match found {
            Ok(both) => both,
            Err(why) => {
                misses.push(format!("{} signatures: {why}", build.name));
                continue;
            }
        };
        for target in [end_game, error] {
            // Safety: the match is inside the code section and longer than
            // the stolen bytes.
            let present = unsafe { std::slice::from_raw_parts(target as *const u8, STOLEN_LEN) };
            if present != build.stolen {
                return Err(format!(
                    "expected {:02x?} at +{:#x}, found {present:02x?} (another hook there?)",
                    build.stolen,
                    target - base
                ));
            }
        }
        RESUME_END_GAME.store(end_game + STOLEN_LEN, Ordering::Release);
        RESUME_ERROR.store(error + STOLEN_LEN, Ordering::Release);
        let callback = on_host_end as *const () as usize;
        let mut detours = Vec::new();
        for (which, target, resume) in [
            (Which::EndGame, end_game, &RESUME_END_GAME),
            (Which::Error, error, &RESUME_ERROR),
        ] {
            let code = stub(which, &build.stolen, callback, resume.as_ptr() as usize);
            // Safety: the span was checked byte for byte above, and nothing
            // branches into it -- checked by tools/verify_demo_damage_offsets.py.
            detours.push(unsafe { detour::install(target, STOLEN_LEN, &code) }?);
        }
        return Ok((detours, build.name, end_game - base, error - base));
    }
    Err(misses.join("; "))
}

/// The cause and the reason's key when `fmt`, passed to `which`, is one of
/// [`REASONS`] or [`LIMITS`].
fn classify(which: Which, fmt: &str) -> Option<(Cause, &'static str)> {
    let find = |table: &[(Which, &str, &'static str)]| {
        table
            .iter()
            .find(|(w, start, _)| *w == which && fmt.starts_with(start))
            .map(|(_, _, key)| *key)
    };
    find(&REASONS)
        .map(|key| (Cause::Damaged, key))
        .or_else(|| find(&LIMITS).map(|key| (Cause::EntityLimit, key)))
}

/// `fmt` with its first `%s` replaced by `arg`, without the trailing
/// newline: what the engine prints. Only the reasons' formats are expanded,
/// and they take at most one string.
fn expand(fmt: &str, arg: Option<&str>) -> String {
    let text = match (fmt.find("%s"), arg) {
        (Some(at), Some(arg)) => format!("{}{arg}{}", &fmt[..at], &fmt[at + 2..]),
        _ => fmt.to_string(),
    };
    text.trim_end().to_string()
}

/// A C string of at most `max` bytes at `ptr`, lossily decoded.
///
/// Safety: `ptr` is null or points at readable bytes up to a NUL or `max`.
unsafe fn c_text(ptr: *const u8, max: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut bytes = Vec::new();
    for i in 0..max {
        // Safety: the caller vouches for every byte up to the NUL.
        let b = unsafe { *ptr.add(i) };
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Whether the engine is playing a demo back (`playdemo` or `viewdemo`).
fn playing_demo() -> bool {
    crate::engine::engfuncs().is_some_and(|engfuncs| {
        let api = engfuncs.p_demo_api;
        // Safety: the engine's own accessor, on its thread.
        !api.is_null() && unsafe { ((*api).is_playingback)() } != 0
    })
}

/// Where the demo is: the demo player's clock under `viewdemo`, the
/// client's time otherwise.
fn demo_time() -> (Option<f64>, &'static str) {
    if let Some(clock) = crate::demo_seek::clock().filter(|c| c.active) {
        return (Some(clock.now), "demo player");
    }
    let client = crate::engine::engine_client_time().map(f64::from);
    (client.filter(|t| t.is_finite()), "client time")
}

/// Called by the stubs on entry to `Host_EndGame` / `Host_Error`, on the
/// engine's thread. Records a damaged demo's end for [`poll`]; logs any
/// other end while a demo plays, so a survey of healthy demos reads the log.
unsafe extern "C" fn on_host_end(which: u32, fmt: *const c_char, args: *const usize) {
    let which = if which == Which::Error as u32 {
        Which::Error
    } else {
        Which::EndGame
    };
    // Safety: the engine passes a format string literal.
    let Some(fmt) = (unsafe { c_text(fmt.cast(), 256) }) else {
        return;
    };
    let playing = playing_demo();
    let classified = classify(which, &fmt);
    if !playing && classified.is_none() {
        return;
    }
    let Some((cause, reason)) = classified.filter(|_| playing) else {
        // The raw format: its arguments are only read for the reasons'
        // formats, whose types are known.
        unsafe {
            crate::debug::report(&format!(
                "demo_damage: {}: {} (demo playing: {playing}; not a damaged-demo end)",
                which.name(),
                fmt.trim_end()
            ))
        };
        return;
    };
    // The only reason taking an argument is the illegible message's svc
    // name; the others have no `%`.
    let arg = if fmt.contains("%s") && !args.is_null() {
        // Safety: a reason's format with `%s` takes one string as its first
        // variadic argument: `svc_strings[n]`, a literal.
        unsafe { c_text(*args as *const u8, 64) }
    } else {
        None
    };
    let message = expand(&fmt, arg.as_deref());
    let (at, clock) = demo_time();
    let damage = Damage {
        cause,
        reason,
        message,
        demo: crate::demo_reload::current_demo(),
        at,
        clock,
    };
    if let Ok(mut pending) = PENDING.try_lock() {
        *pending = Some(damage);
        WAITING.store(true, Ordering::Release);
    }
}

/// Reports a damaged demo's end, once, on the frame after it. Runs every
/// frame from `commands::poll`; one atomic load when nothing happened.
pub fn poll() {
    if !WAITING.swap(false, Ordering::AcqRel) {
        return;
    }
    let Some(damage) = PENDING.try_lock().ok().and_then(|mut p| p.take()) else {
        return;
    };
    NOTICED.fetch_add(1, Ordering::Relaxed);
    let at_text = damage
        .at
        .map(crate::demo_seek::clock_text)
        .unwrap_or_else(|| UNKNOWN_TIME.to_string());
    let path = damage.demo.as_deref().map(demo_file_path);
    let pre = PRE_ANNIVERSARY.load(Ordering::Relaxed);
    crate::commands::console_print(&format!("{}\n", notice(damage.cause, pre, &at_text)));
    unsafe {
        crate::debug::report(&format!(
            "demo_damage: {} stopped at {at_text} ({}): \"{}\" ({}) (#1373)",
            path.as_deref().unwrap_or("a demo"),
            damage.clock,
            damage.message,
            damage.reason,
        ))
    };
    crate::events::send(&event_label(&damage, path.as_deref()));
}

/// The console line for `cause`, at `at_text`; `pre` is the pre-Anniversary
/// build.
fn notice(cause: Cause, pre: bool, at_text: &str) -> String {
    match cause {
        Cause::Damaged => NOTICE.replace("{at}", at_text),
        Cause::EntityLimit => {
            let hint = if pre { ENTITY_HINT_PRE } else { "" };
            format!("{}{hint}", ENTITY_NOTICE.replace("{at}", at_text))
        }
    }
}

/// `DEMO_DAMAGED<tab>seconds<tab>reason<tab>path` (`DEMO_ENTITY_LIMIT` for
/// the entity limit), for Studio. Tabs, as `BATCH_PROBLEM` uses, since the
/// path can hold spaces. The path is last and left out, with its tab, when
/// the demo's name is not known: the pipe trims a line's end, so a trailing
/// empty field would not arrive anyway. Seconds are empty when unknown.
fn event_label(damage: &Damage, path: Option<&str>) -> String {
    let name = match damage.cause {
        Cause::Damaged => "DEMO_DAMAGED",
        Cause::EntityLimit => "DEMO_ENTITY_LIMIT",
    };
    let at = damage.at.map(|t| format!("{t:.2}")).unwrap_or_default();
    let mut label = format!("{name}\t{at}\t{}", damage.reason);
    if let Some(path) = path.filter(|p| !p.is_empty()) {
        label.push('\t');
        label.push_str(path);
    }
    label
}

/// The demo's file on disk: its name joined to the game folder (or its
/// `_addon` folder, where the engine also finds demos), `.dem` added,
/// `..` worked out by [`demo_path_in`]. Only the folder choice looks at the
/// disk.
fn demo_file_path(name: &str) -> String {
    let game = crate::game_folder::mod_dir();
    let addon = crate::game_folder::game_root().join(crate::game_folder::addon_name());
    demo_path_in(&game, &addon, name, |path| path.is_file())
        .display()
        .to_string()
}

/// `name`'s file under `game`, or under `addon` when only that one `exists`;
/// `..` resolved without the disk, so a network share stays `\\server\...`
/// (`std::fs::canonicalize` gives `\\?\UNC\server\...`).
fn demo_path_in(
    game: &std::path::Path,
    addon: &std::path::Path,
    name: &str,
    exists: impl Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    let in_game = crate::review_ask::demo_path(game, name);
    if exists(&in_game) {
        return in_game;
    }
    let in_addon = crate::review_ask::demo_path(addon, name);
    if exists(&in_addon) { in_addon } else { in_game }
}

/// The `ds_debug_status` line, or `None` when it was never installed.
pub fn status_line() -> Option<String> {
    if !INSTALLED.load(Ordering::Relaxed) {
        return None;
    }
    let build = BUILD_NAME.lock().map(|name| *name).unwrap_or_default();
    Some(format!(
        "damaged-demo notice: on ({build} hw.dll), {} noticed this session",
        NOTICED.load(Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_assembles_to_what_the_comment_claims() {
        let code = stub(Which::Error, &BUILDS[0].stolen, 0x1111_1111, 0x2222_2222);
        #[rustfmt::skip]
        assert_eq!(
            code,
            vec![
                0x60, 0x9c, 0xfc,                         //  0 pushad; pushfd; cld
                0x8d, 0x44, 0x24, 0x2c,                   //  3 lea  eax, [esp+0x2c]
                0x50,                                     //  7 push eax
                0xff, 0x74, 0x24, 0x2c,                   //  8 push dword [esp+0x2c]
                0x68, 0x01, 0x00, 0x00, 0x00,             // 12 push 1 (Host_Error)
                0xb8, 0x11, 0x11, 0x11, 0x11,             // 17 mov  eax, on_host_end
                0xff, 0xd0,                               // 22 call eax
                0x83, 0xc4, 0x0c,                         // 24 add  esp, 12
                0x9d, 0x61,                               // 27 popfd; popad
                0x55, 0x8b, 0xec, 0x81, 0xec, 0x00, 0x04, 0x00, 0x00, // 29 the prologue
                0xff, 0x25, 0x22, 0x22, 0x22, 0x22,       // 38 jmp  dword [resume]
            ]
        );
    }

    #[test]
    fn each_pattern_starts_with_its_stolen_bytes() {
        for build in &BUILDS {
            for pattern in [build.end_game, build.error] {
                let head: Vec<u8> = pattern
                    .split_whitespace()
                    .take(STOLEN_LEN)
                    .map(|t| u8::from_str_radix(t, 16).expect("no wildcards in the stolen bytes"))
                    .collect();
                assert_eq!(head, build.stolen, "{}", build.name);
                assert!(
                    crate::scan::Pattern::parse(pattern).is_ok(),
                    "{}",
                    build.name
                );
            }
        }
    }

    #[test]
    fn the_damage_ends_are_recognised_and_a_normal_end_is_not() {
        let cases = [
            (
                Which::EndGame,
                "Demo message > MAX_POSSIBLE_MSG",
                Some("message_too_big"),
            ),
            (Which::EndGame, "Bad demo length.\n", Some("bad_length")),
            (
                Which::EndGame,
                "Demo message length < 0.\n",
                Some("negative_length"),
            ),
            (
                Which::EndGame,
                "Error reading demo message data.\n",
                Some("read_error"),
            ),
            (
                Which::Error,
                "CL_ParseServerMessage: Illegible server message - %s\n",
                Some("illegible_message"),
            ),
            (
                Which::Error,
                "CL_ParseServerMessage: Bad server message",
                Some("bad_server_message"),
            ),
            (Which::EndGame, "End of demo.\n", None),
            (Which::Error, "Demo message > MAX_POSSIBLE_MSG", None),
            (
                Which::Error,
                "CL_ParsePacketEntities: newindex == MAX_PACKET_ENTITIES",
                None,
            ),
            (Which::EndGame, "Server disconnected", None),
        ];
        for (which, fmt, want) in cases {
            assert_eq!(
                classify(which, fmt),
                want.map(|key| (Cause::Damaged, key)),
                "{which:?} {fmt:?}"
            );
        }
    }

    #[test]
    fn the_entity_limit_is_its_own_cause_not_damage() {
        assert_eq!(
            classify(
                Which::EndGame,
                "CL_ParsePacketEntities: newindex == MAX_PACKET_ENTITIES"
            ),
            Some((Cause::EntityLimit, "entity_limit"))
        );
        for (_, start, _) in LIMITS {
            assert!(
                !REASONS
                    .iter()
                    .any(|(_, r, _)| r.starts_with(start) || start.starts_with(r)),
                "{start:?} is also a damage reason"
            );
        }
    }

    #[test]
    fn the_entity_notice_says_it_is_not_damaged_and_names_anniversary_only_on_pre() {
        let pre = notice(Cause::EntityLimit, true, "1:02.00");
        let post = notice(Cause::EntityLimit, false, "1:02.00");
        assert!(pre.contains("1:02.00") && pre.contains("not damaged"));
        assert!(pre.ends_with(ENTITY_HINT_PRE));
        assert!(!post.contains("Anniversary"));
        assert!(!post.contains("repair"));
        assert_eq!(
            notice(Cause::Damaged, true, UNKNOWN_TIME),
            NOTICE.replace("{at}", UNKNOWN_TIME)
        );
    }

    #[test]
    fn the_demo_path_resolves_dots_without_the_disk_and_keeps_a_share() {
        let game = std::path::Path::new(r"\\server\share\Half-Life\dod");
        let addon = std::path::Path::new(r"\\server\share\Half-Life\dod_addon");
        assert_eq!(
            demo_path_in(game, addon, "../demos/match1", |_| false),
            std::path::PathBuf::from(r"\\server\share\Half-Life\demos\match1.dem")
        );
        let game = std::path::Path::new(r"C:\Games\Half-Life\dod");
        let addon = std::path::Path::new(r"C:\Games\Half-Life\dod_addon");
        let only_addon = |p: &std::path::Path| p.starts_with(addon);
        assert_eq!(
            demo_path_in(game, addon, "match1.dem", only_addon),
            std::path::PathBuf::from(r"C:\Games\Half-Life\dod_addon\match1.dem")
        );
        assert_eq!(
            demo_path_in(game, addon, "match1", |_| true),
            std::path::PathBuf::from(r"C:\Games\Half-Life\dod\match1.dem")
        );
    }

    #[test]
    fn the_message_reads_as_the_engine_prints_it() {
        assert_eq!(
            expand(
                "CL_ParseServerMessage: Illegible server message - %s\n",
                Some("svc_bad")
            ),
            "CL_ParseServerMessage: Illegible server message - svc_bad"
        );
        assert_eq!(
            expand("Demo message > MAX_POSSIBLE_MSG", None),
            "Demo message > MAX_POSSIBLE_MSG"
        );
        assert_eq!(expand("Bad demo length.\n", None), "Bad demo length.");
    }

    #[test]
    fn the_event_carries_time_reason_and_path() {
        let damage = Damage {
            cause: Cause::Damaged,
            reason: "message_too_big",
            message: "Demo message > MAX_POSSIBLE_MSG".to_string(),
            demo: Some("monday-wsod25_r07_m1_h1".to_string()),
            at: Some(369.123),
            clock: "demo player",
        };
        // What goes on the pipe, not just the label: `events::send` trims.
        let wire = |damage: &Damage, path| {
            crate::events::wire_line(&event_label(damage, path)).expect("a marker line")
        };
        assert_eq!(
            wire(
                &damage,
                Some(r"C:\Games\Half-Life\dod\monday-wsod25_r07_m1_h1.dem")
            ),
            "[dod-studio] DEMO_DAMAGED\t369.12\tmessage_too_big\tC:\\Games\\Half-Life\\dod\\monday-wsod25_r07_m1_h1.dem"
        );
        let unknown = Damage {
            at: None,
            ..damage.clone()
        };
        assert_eq!(
            wire(&unknown, None),
            "[dod-studio] DEMO_DAMAGED\t\tmessage_too_big"
        );
        let limit = Damage {
            cause: Cause::EntityLimit,
            reason: "entity_limit",
            ..damage
        };
        assert_eq!(
            wire(&limit, Some(r"C:\x y\a.dem")),
            "[dod-studio] DEMO_ENTITY_LIMIT\t369.12\tentity_limit\tC:\\x y\\a.dem"
        );
    }

    #[test]
    fn the_notice_names_the_time() {
        assert!(NOTICE.contains("{at}"));
        assert!(!NOTICE.contains('\n'));
    }
}
