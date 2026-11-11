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
//! ## What it does not catch yet
//!
//! - `viewdemo`: `DemoPlayer`'s loader (`Core.dll`) reads the whole file at
//!   load, prints `WARNING! DemoFile::ReadDemoPacket: ...` there, and plays
//!   the part before the damage; neither function is called.
//! - A file cut short: it has lost its directory, so `playdemo` refuses it at
//!   the start (`Error: Corrupt demo file.`) and nothing plays.
//! - `timedemo` goes through the same reader and is caught, but the hook does
//!   not track its demo name, so its event carries no path.
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
//! engine's own), logs it and sends one `DEMO_DAMAGED` event to Studio over
//! the events pipe. Nothing runs per frame but one atomic load.
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

/// The console line's text, until #1373 settles the wording. `{at}` is the
/// demo time.
const NOTICE: &str = "This demo is damaged at {at} and stopped playing. \
                      DoD Studio can repair it (Demo Auditor, Repair).";

/// Whether to install at all -- `GOLDSRC_HOOKS_DEMO_DAMAGE=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Where each stub jumps back to: the instruction after the stolen bytes.
static RESUME_END_GAME: AtomicUsize = AtomicUsize::new(0);
static RESUME_ERROR: AtomicUsize = AtomicUsize::new(0);

/// Set once both jumps are in. `hw.dll` never reloads, so once is enough.
static INSTALLED: AtomicBool = AtomicBool::new(false);
static BUILD_NAME: Mutex<&str> = Mutex::new("");
/// See [`detour::Detour`] on why these are never undone.
static DETOURS: Mutex<Vec<detour::Detour>> = Mutex::new(Vec::new());

/// Damaged demos noticed this session, for the status line.
static NOTICED: AtomicU32 = AtomicU32::new(0);
/// Set by [`on_host_end`] when [`PENDING`] holds a notice for [`poll`].
static WAITING: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<Damage>> = Mutex::new(None);

/// What [`on_host_end`] saw when a demo stopped on a damaged frame.
#[derive(Debug, Clone, PartialEq)]
struct Damage {
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

/// The reason's key when `fmt`, passed to `which`, is one of [`REASONS`].
fn classify(which: Which, fmt: &str) -> Option<&'static str> {
    REASONS
        .iter()
        .find(|(w, start, _)| *w == which && fmt.starts_with(start))
        .map(|(_, _, key)| *key)
}

/// `fmt` with its first `%s` replaced by `arg`, without the trailing
/// newline: what the engine prints. Only the damage reasons' formats are
/// expanded, and they take at most one string.
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
    let reason = classify(which, &fmt);
    if !playing && reason.is_none() {
        return;
    }
    // The only reason taking an argument is the illegible message's svc
    // name; the others have no `%`.
    let arg = if fmt.contains("%s") && !args.is_null() {
        // Safety: the format takes a string, so the first variadic argument
        // is one: `svc_strings[n]`, a literal.
        unsafe { c_text(*args as *const u8, 64) }
    } else {
        None
    };
    let message = expand(&fmt, arg.as_deref());
    let Some(reason) = reason.filter(|_| playing) else {
        unsafe {
            crate::debug::report(&format!(
                "demo_damage: {}: {message} (demo playing: {playing}; not a damaged-demo end)",
                which.name()
            ))
        };
        return;
    };
    let (at, clock) = demo_time();
    let damage = Damage {
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
        .unwrap_or_else(|| "an unknown time".to_string());
    let path = damage.demo.as_deref().map(demo_file_path);
    crate::commands::console_print(&format!("{}\n", NOTICE.replace("{at}", &at_text)));
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

/// `DEMO_DAMAGED<tab>seconds<tab>reason<tab>path`, for Studio. Tabs, as
/// `BATCH_PROBLEM` uses, since the path can hold spaces; the path is last
/// and empty when the demo's name is not known.
fn event_label(damage: &Damage, path: Option<&str>) -> String {
    let at = damage.at.map(|t| format!("{t:.2}")).unwrap_or_default();
    format!(
        "DEMO_DAMAGED\t{at}\t{}\t{}",
        damage.reason,
        path.unwrap_or("")
    )
}

/// The demo's file on disk: its name joined to the game folder (or its
/// `_addon` folder, where the engine also finds demos), `.dem` added,
/// `..` resolved.
fn demo_file_path(name: &str) -> String {
    let game = crate::game_folder::mod_dir();
    let addon = crate::game_folder::game_root().join(crate::game_folder::addon_name());
    let in_game = crate::streaks::demo_path(&game, name);
    let path = if in_game.is_file() {
        in_game
    } else {
        let in_addon = crate::streaks::demo_path(&addon, name);
        if in_addon.is_file() {
            in_addon
        } else {
            in_game
        }
    };
    let resolved = std::fs::canonicalize(&path).unwrap_or(path);
    let text = resolved.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(str::to_string)
        .unwrap_or(text)
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
            assert_eq!(classify(which, fmt), want, "{which:?} {fmt:?}");
        }
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
            reason: "message_too_big",
            message: "Demo message > MAX_POSSIBLE_MSG".to_string(),
            demo: Some("monday-wsod25_r07_m1_h1".to_string()),
            at: Some(369.123),
            clock: "demo player",
        };
        assert_eq!(
            event_label(
                &damage,
                Some(r"C:\Games\Half-Life\dod\monday-wsod25_r07_m1_h1.dem")
            ),
            "DEMO_DAMAGED\t369.12\tmessage_too_big\tC:\\Games\\Half-Life\\dod\\monday-wsod25_r07_m1_h1.dem"
        );
        let unknown = Damage { at: None, ..damage };
        assert_eq!(
            event_label(&unknown, None),
            "DEMO_DAMAGED\t\tmessage_too_big\t"
        );
    }

    #[test]
    fn the_notice_names_the_time() {
        assert!(NOTICE.contains("{at}"));
        assert!(!NOTICE.contains('\n'));
    }
}
