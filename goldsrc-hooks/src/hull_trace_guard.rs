//! Stops the engine crashing when a player-movement trace walks collision
//! data left over from the previous map (issue #384).
//!
//! ## The bug
//!
//! `playdemo` of an HLTV demo on some maps (dod_anzio, dod_harrington) right
//! after a demo on any other map crashes the engine while the new map loads:
//!
//! ```text
//! CRASH: stack overflow (0xc00000fd) at hw.dll+0x6c839
//!   [esp+0x024] hw.dll+0x6c9a5   (repeated all the way down the stack)
//! ```
//!
//! or, less often, an access violation at `hw.dll+0x6c8d1` reading an address
//! nowhere near any map data. Both are inside `PM_RecursiveHullCheck`
//! (`hw.dll+0x6c830`), the recursive clip-hull trace that `PM_PlayerTraceEx`
//! (`+0x6bf00`) runs once per physent. It happens in the plain game with
//! nothing injected, and on the stock 25th Anniversary engine too.
//!
//! The hull it is handed is not the new map's. Both crash records' hull
//! pointers (`hw.dll+0xefb140`, `+0xf123d0`) sit exactly 242 `model_t`s
//! (392 bytes each) apart, so both are hulls inside the engine's static model
//! table, and anzio's own hulls are valid and at most 44 levels deep (checked
//! offline). The likely story: a physent still points at a brush model from
//! the previous map, whose `model_t` survives the map change but whose
//! clipnodes and planes pointed into hunk memory the new map has since reused.
//! Walking that finds either a loop, which recurses until the stack runs out
//! (`+0x6c839`), or a garbage plane number, which reads unmapped memory
//! (`+0x6c8d1`). Why only some maps' HLTV demos, and only through `playdemo`,
//! is still open; see the issue.
//!
//! ## The fix: refuse a node the hull can't have
//!
//! One detour on the function's entry, which every level of the recursion goes
//! through (it calls itself at its own address). Before the game's code runs,
//! the stub checks three things, and on any failure returns 0 -- the
//! function's own "stop here" result, which every caller already handles by
//! ending the trace -- instead of reading the node:
//!
//! 1. **Stack left.** Under [`STACK_MARGIN`] bytes between `esp` and the
//!    thread's stack floor (`fs:[0xE0C]`, the TEB's `DeallocationStack`) means
//!    a loop: a valid hull is at most a few dozen levels deep, one level costs
//!    80 bytes, and the stack is 1 MiB. Measuring the stack rather than
//!    counting depth needs no return hook, and it is exactly the thing that
//!    runs out.
//! 2. **The node number** is inside the hull's own `firstclipnode ..=
//!    lastclipnode`. Quake's `SV_RecursiveHullCheck` made this check (with a
//!    `Sys_Error`), and GoldSrc kept it in `PM_HullPointContents`
//!    (`+0x6b810`), which walks the same hulls -- so real map data always
//!    passes it. GoldSrc dropped it from this one function.
//! 3. **The node's plane number** is under [`MAX_PLANE`], far above any real
//!    map (`MAX_MAP_PLANES` is 32767) and far below the garbage that crashed.
//!
//! Leaf numbers (negative) and hulls with no clipnodes at all
//! (`firstclipnode >= lastclipnode`, the function's own early-out) go straight
//! to the game's code, unchecked, as before. The stub only touches `eax`,
//! `ecx`, `edx` and the flags, all of which a cdecl callee may clobber, and a
//! leaf never gets past `eax`.
//!
//! A refused trace reports whatever it had found so far -- usually "start
//! solid", which `PM_PlayerTraceEx` starts every trace as. That is one bad
//! trace against a model that isn't really there, during a load.
//!
//! It is on by default, like `tempent_fix`, because it only acts where the
//! engine would otherwise crash. `GOLDSRC_HOOKS_HULL_TRACE_GUARD=0` turns it
//! off for a session.
//!
//! ## Both engines
//!
//! Analysis subjects: the pre-Anniversary `hw.dll` (1,641,376 bytes), where
//! the function is at `+0x6c830`, and the 25th Anniversary `hw.dll`
//! (3,598,176 bytes), which has the same bug at `+0x1e2540`. The Anniversary
//! build compiles it with a stack cookie and `sub esp, 0x3c` rather than
//! `0x20`, so it has its own signature and its own six stolen bytes
//! ([`BUILDS`]); everything the stub reads -- the arguments at `[esp+4]` and
//! `[esp+8]`, the hull's layout, cdecl with 7 arguments -- is the same in
//! both. The stub runs before the prologue, so it never meets the cookie.
//!
//! The Anniversary engine's other near-copy, `+0x237540`, is
//! `SV_RecursiveHullCheck` ("SV_RecursiveHullCheck: bad node number"), not a
//! second `PM_RecursiveHullCheck`. The pre-Anniversary engine has it too
//! (`+0xc6a90`), and it is left alone on both: it already refuses an
//! out-of-range node (with a `Sys_Error`), and every #384 crash so far was in
//! the PM function.
//!
//! `tools/verify_hull_trace_offsets.py` checks every fact above against each
//! binary.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::detour;
use crate::scan;

/// One engine build's `PM_RecursiveHullCheck`.
struct Build {
    name: &'static str,
    /// The function's entry, up to its first branch: the prologue the stub
    /// reproduces, then the node number's load and the leaf tests. Unique in
    /// that build's `hw.dll`, and matching nothing in the other's.
    pattern: &'static str,
    /// `push ebp; mov ebp, esp; sub esp, <locals>` -- the bytes the jump
    /// overwrites, reproduced verbatim in the stub. Not relative, and none
    /// reads the flags.
    stolen: [u8; 6],
}

/// The builds the guard knows, tried in order.
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        pattern: "55 8B EC 83 EC 20 8B 45 0C 53 56 57 85 C0 7D 4F 83 F8 FE 74 34 \
                  8B 4D 20 83 F8 FF C7 01 00 00 00 00",
        stolen: [0x55, 0x8b, 0xec, 0x83, 0xec, 0x20],
    },
    // The stack cookie's address and the locals' offsets are wildcards; the
    // `mov edx, [ebp+0x18]; mov ecx, [ebp+0x1c]` pair tells it apart from
    // SV_RecursiveHullCheck, whose entry loads them the other way round.
    Build {
        name: "25th Anniversary",
        pattern: "55 8B EC 83 EC 3C A1 ?? ?? ?? ?? 33 C5 89 45 FC 8B 45 0C 8B 55 18 \
                  8B 4D 1C 89 55 ?? 89 4D ?? 89 45 ?? 56 8B 75 20 57 8B 7D 08 85 C0 \
                  79 ?? 83 F8 FE 74 ?? C7 06 00 00 00 00 83 F8 FF",
        stolen: [0x55, 0x8b, 0xec, 0x83, 0xec, 0x3c],
    },
];

/// Every build steals the same number of bytes, so the stub's layout is one.
const STOLEN_LEN: usize = 6;
const _: () = assert!(STOLEN_LEN >= 5, "a jmp rel32 needs 5 bytes");

/// Stack that must be left at entry, in bytes. A 44-deep trace uses about
/// 3.5 KiB; this leaves room for everything the unwinding callers still do.
const STACK_MARGIN: u32 = 0x1_0000;

/// A plane number at or above this is garbage. Real maps stop at 32767.
const MAX_PLANE: u32 = 0x1_0000;

/// Whether to install at all -- `GOLDSRC_HOOKS_HULL_TRACE_GUARD=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Where the stub jumps back to: the instruction after the stolen bytes.
static RESUME: AtomicUsize = AtomicUsize::new(0);

/// How many node visits each check refused, incremented by the stub.
static TOO_DEEP: AtomicU32 = AtomicU32::new(0);
static BAD_NODE: AtomicU32 = AtomicU32::new(0);
static BAD_PLANE: AtomicU32 = AtomicU32::new(0);

/// The last total written to the log, so [`poll`] only logs a change.
static LOGGED: AtomicU32 = AtomicU32::new(0);

/// The build the guard was installed on, for the status line.
static BUILD_NAME: Mutex<&str> = Mutex::new("");

/// Set once the jump is in. `hw.dll` never reloads, so once is enough.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// See [`detour::Detour`] on why this is never undone.
static DETOUR: Mutex<Option<detour::Detour>> = Mutex::new(None);

/// The stub, hand-assembled. On entry `[esp+4]` is the hull and `[esp+8]` the
/// node number.
///
/// ```asm
///         mov  eax, fs:[0xE0C]              ; this thread's stack floor
///         add  eax, STACK_MARGIN
///         cmp  esp, eax
///         jb   .deep
///         mov  eax, [esp+8]                 ; num
///         test eax, eax
///         jl   .game                        ; a leaf: the game's code handles it
///         mov  ecx, [esp+4]                 ; hull
///         mov  edx, [ecx+8]                 ; firstclipnode
///         cmp  edx, [ecx+0xc]               ; lastclipnode
///         jge  .game                        ; no clipnodes: the game's early-out
///         cmp  eax, edx
///         jl   .node
///         cmp  eax, [ecx+0xc]
///         jg   .node
///         mov  edx, [ecx]                   ; clipnodes
///         mov  edx, [edx+eax*8]             ; clipnodes[num].planenum
///         cmp  edx, MAX_PLANE
///         jae  .plane
/// .game:  push ebp; mov ebp, esp; sub esp, <the build's locals>
///         jmp  dword ptr [RESUME]
/// .deep:  lock inc dword ptr [TOO_DEEP]
///         jmp  .stop
/// .node:  lock inc dword ptr [BAD_NODE]
///         jmp  .stop
/// .plane: lock inc dword ptr [BAD_PLANE]
/// .stop:  xor  eax, eax
///         ret
/// ```
fn stub(
    stolen: &[u8; STOLEN_LEN],
    resume: usize,
    too_deep: usize,
    bad_node: usize,
    bad_plane: usize,
) -> Vec<u8> {
    const GAME: usize = 57;
    const DEEP: usize = 69;
    const NODE: usize = 78;
    const PLANE: usize = 87;
    const STOP: usize = 94;
    // A short jump's displacement from the end of the 2-byte jump at `at`.
    let short = |at: usize, to: usize| (to - (at + 2)) as u8;
    let lock_inc = |counter: usize| {
        let mut code = vec![0xf0, 0xff, 0x05]; // lock inc dword ptr [abs32]
        code.extend_from_slice(&(counter as u32).to_le_bytes());
        code
    };

    let mut code = vec![0x64, 0xa1, 0x0c, 0x0e, 0x00, 0x00]; // mov eax, fs:[0xE0C]
    code.push(0x05); // add eax, imm32
    code.extend_from_slice(&STACK_MARGIN.to_le_bytes());
    code.extend_from_slice(&[0x3b, 0xe0]); // cmp esp, eax
    code.extend_from_slice(&[0x72, short(13, DEEP)]); // jb .deep
    code.extend_from_slice(&[0x8b, 0x44, 0x24, 0x08]); // mov eax, [esp+8]
    code.extend_from_slice(&[0x85, 0xc0]); // test eax, eax
    code.extend_from_slice(&[0x7c, short(21, GAME)]); // jl .game
    code.extend_from_slice(&[0x8b, 0x4c, 0x24, 0x04]); // mov ecx, [esp+4]
    code.extend_from_slice(&[0x8b, 0x51, 0x08]); // mov edx, [ecx+8]
    code.extend_from_slice(&[0x3b, 0x51, 0x0c]); // cmp edx, [ecx+0xc]
    code.extend_from_slice(&[0x7d, short(33, GAME)]); // jge .game
    code.extend_from_slice(&[0x3b, 0xc2]); // cmp eax, edx
    code.extend_from_slice(&[0x7c, short(37, NODE)]); // jl .node
    code.extend_from_slice(&[0x3b, 0x41, 0x0c]); // cmp eax, [ecx+0xc]
    code.extend_from_slice(&[0x7f, short(42, NODE)]); // jg .node
    code.extend_from_slice(&[0x8b, 0x11]); // mov edx, [ecx]
    code.extend_from_slice(&[0x8b, 0x14, 0xc2]); // mov edx, [edx+eax*8]
    code.extend_from_slice(&[0x81, 0xfa]); // cmp edx, imm32
    code.extend_from_slice(&MAX_PLANE.to_le_bytes());
    code.extend_from_slice(&[0x73, short(55, PLANE)]); // jae .plane
    debug_assert_eq!(code.len(), GAME);
    code.extend_from_slice(stolen);
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume as u32).to_le_bytes());
    debug_assert_eq!(code.len(), DEEP);
    code.extend_from_slice(&lock_inc(too_deep));
    code.extend_from_slice(&[0xeb, short(76, STOP)]); // jmp .stop
    debug_assert_eq!(code.len(), NODE);
    code.extend_from_slice(&lock_inc(bad_node));
    code.extend_from_slice(&[0xeb, short(85, STOP)]); // jmp .stop
    debug_assert_eq!(code.len(), PLANE);
    code.extend_from_slice(&lock_inc(bad_plane));
    debug_assert_eq!(code.len(), STOP);
    code.extend_from_slice(&[0x33, 0xc0, 0xc3]); // xor eax, eax; ret
    code
}

/// Patches the trace's entry in the loaded `hw.dll`. A second call does
/// nothing.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "hull_trace_guard: off (GOLDSRC_HOOKS_HULL_TRACE_GUARD=0) -- a stale hull can still crash the engine (#384)",
            )
        };
        return;
    }
    if INSTALLED.load(Ordering::Acquire) {
        return;
    }
    let result = crate::engine::engine_module_base()
        .ok_or_else(|| "hw.dll is not loaded yet".to_string())
        .and_then(|base| {
            check_stack_floor()?;
            install_at(base)
        });
    match result {
        Ok((detour, build, offset)) => {
            if let Ok(mut slot) = DETOUR.lock() {
                *slot = Some(detour);
            }
            if let Ok(mut name) = BUILD_NAME.lock() {
                *name = build;
            }
            INSTALLED.store(true, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "hull_trace_guard: PM_RecursiveHullCheck guarded against stale hulls ({build} hw.dll, +{offset:#x}) (#384)"
                ))
            };
        }
        Err(why) => unsafe {
            crate::debug::report(&format!("hull_trace_guard: not installed -- {why}"))
        },
    }
}

/// Finds the function with the first of [`BUILDS`] whose signature matches,
/// and patches it. Returns the detour, the build's name and the offset.
fn install_at(base: usize) -> Result<(detour::Detour, &'static str, usize), String> {
    let mut misses = Vec::new();
    for build in &BUILDS {
        // Safety: `base` is a module handle the loader gave us.
        let target = match unsafe { scan::find_unique(base, build.pattern) } {
            Ok(target) => target,
            Err(why) => {
                misses.push(format!("{} signature: {why}", build.name));
                continue;
            }
        };
        // Safety: the match is inside the code section and longer than the
        // stolen bytes.
        let present = unsafe { std::slice::from_raw_parts(target as *const u8, STOLEN_LEN) };
        if present != build.stolen {
            return Err(format!(
                "expected {:02x?} at +{:#x}, found {present:02x?}",
                build.stolen,
                target - base
            ));
        }
        RESUME.store(target + STOLEN_LEN, Ordering::Release);
        let code = stub(
            &build.stolen,
            RESUME.as_ptr() as usize,
            TOO_DEEP.as_ptr() as usize,
            BAD_NODE.as_ptr() as usize,
            BAD_PLANE.as_ptr() as usize,
        );
        // Safety: the span was checked byte for byte above, and nothing
        // branches into it -- checked by tools/verify_hull_trace_offsets.py.
        let detour = unsafe { detour::install(target, STOLEN_LEN, &code) }?;
        return Ok((detour, build.name, target - base));
    }
    Err(misses.join("; "))
}

/// Confirms `fs:[0xE0C]` is this thread's stack floor, as the stub assumes,
/// by comparing it with what Windows reports.
#[cfg(target_arch = "x86")]
fn check_stack_floor() -> Result<(), String> {
    use windows_sys::Win32::System::Threading::GetCurrentThreadStackLimits;
    let floor: usize;
    // Safety: a plain read of this thread's TEB.
    unsafe { core::arch::asm!("mov {}, fs:[0xE0C]", out(reg) floor, options(nostack, readonly)) };
    let (mut low, mut high) = (0usize, 0usize);
    // Safety: two out-pointers to locals.
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    if floor == low {
        Ok(())
    } else {
        Err(format!(
            "fs:[0xE0C] is {floor:#x} but the stack floor is {low:#x}; the stack check would be wrong"
        ))
    }
}

#[cfg(not(target_arch = "x86"))]
fn check_stack_floor() -> Result<(), String> {
    Err("only a 32-bit x86 build can patch hw.dll".to_string())
}

fn refused() -> u32 {
    TOO_DEEP.load(Ordering::Relaxed)
        + BAD_NODE.load(Ordering::Relaxed)
        + BAD_PLANE.load(Ordering::Relaxed)
}

/// Logs when the guard has refused more nodes since the last call. Runs every
/// frame from `commands::poll`; three atomic loads when nothing happened.
///
/// Logs the first refusal and then every tenfold, like `tempent_fix`.
pub fn poll() {
    let now = refused();
    let logged = LOGGED.load(Ordering::Relaxed);
    if now == logged || !crate::tempent_fix::worth_logging(logged, now) {
        return;
    }
    LOGGED.store(now, Ordering::Relaxed);
    unsafe {
        crate::debug::report(&format!(
            "hull_trace_guard: refused a stale hull node ({}) -- trace stopped instead of crashing (#384)",
            breakdown()
        ))
    };
}

/// `"12 too deep, 3 bad node, 0 bad plane"`.
fn breakdown() -> String {
    format!(
        "{} too deep, {} bad node, {} bad plane",
        TOO_DEEP.load(Ordering::Relaxed),
        BAD_NODE.load(Ordering::Relaxed),
        BAD_PLANE.load(Ordering::Relaxed)
    )
}

/// The `dodstudio_debug_status` line, or `None` when it was never installed.
pub fn status_line() -> Option<String> {
    if !INSTALLED.load(Ordering::Relaxed) {
        return None;
    }
    let build = BUILD_NAME.lock().map(|name| *name).unwrap_or_default();
    Some(match refused() {
        0 => format!("hull-trace crash guard: on ({build} hw.dll), nothing refused"),
        _ => format!(
            "hull-trace crash guard: on ({build} hw.dll), refused {}",
            breakdown()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_assembles_to_what_the_comment_claims() {
        let code = stub(
            &BUILDS[0].stolen,
            0x1111_1111,
            0x2222_2222,
            0x3333_3333,
            0x4444_4444,
        );
        #[rustfmt::skip]
        assert_eq!(
            code,
            vec![
                0x64, 0xa1, 0x0c, 0x0e, 0x00, 0x00,       //  0 mov  eax, fs:[0xE0C]
                0x05, 0x00, 0x00, 0x01, 0x00,             //  6 add  eax, 0x10000
                0x3b, 0xe0,                               // 11 cmp  esp, eax
                0x72, 0x36,                               // 13 jb   .deep (69)
                0x8b, 0x44, 0x24, 0x08,                   // 15 mov  eax, [esp+8]
                0x85, 0xc0,                               // 19 test eax, eax
                0x7c, 0x22,                               // 21 jl   .game (57)
                0x8b, 0x4c, 0x24, 0x04,                   // 23 mov  ecx, [esp+4]
                0x8b, 0x51, 0x08,                         // 27 mov  edx, [ecx+8]
                0x3b, 0x51, 0x0c,                         // 30 cmp  edx, [ecx+0xc]
                0x7d, 0x16,                               // 33 jge  .game (57)
                0x3b, 0xc2,                               // 35 cmp  eax, edx
                0x7c, 0x27,                               // 37 jl   .node (78)
                0x3b, 0x41, 0x0c,                         // 39 cmp  eax, [ecx+0xc]
                0x7f, 0x22,                               // 42 jg   .node (78)
                0x8b, 0x11,                               // 44 mov  edx, [ecx]
                0x8b, 0x14, 0xc2,                         // 46 mov  edx, [edx+eax*8]
                0x81, 0xfa, 0x00, 0x00, 0x01, 0x00,       // 49 cmp  edx, 0x10000
                0x73, 0x1e,                               // 55 jae  .plane (87)
                0x55, 0x8b, 0xec, 0x83, 0xec, 0x20,       // 57 .game: the stolen bytes
                0xff, 0x25, 0x11, 0x11, 0x11, 0x11,       // 63 jmp  dword [RESUME]
                0xf0, 0xff, 0x05, 0x22, 0x22, 0x22, 0x22, // 69 .deep: lock inc [TOO_DEEP]
                0xeb, 0x10,                               // 76 jmp  .stop (94)
                0xf0, 0xff, 0x05, 0x33, 0x33, 0x33, 0x33, // 78 .node: lock inc [BAD_NODE]
                0xeb, 0x07,                               // 85 jmp  .stop (94)
                0xf0, 0xff, 0x05, 0x44, 0x44, 0x44, 0x44, // 87 .plane: lock inc [BAD_PLANE]
                0x33, 0xc0,                               // 94 .stop: xor eax, eax
                0xc3,                                     // 96 ret
            ]
        );
    }

    #[test]
    fn each_pattern_starts_with_its_stolen_bytes() {
        for build in &BUILDS {
            let head: Vec<u8> = build
                .pattern
                .split_whitespace()
                .take(STOLEN_LEN)
                .map(|t| u8::from_str_radix(t, 16).expect("no wildcards in the stolen bytes"))
                .collect();
            assert_eq!(head, build.stolen, "{}", build.name);
            assert!(
                crate::scan::Pattern::parse(build.pattern).is_ok(),
                "{}",
                build.name
            );
        }
    }

    /// Each build's stub is the same code but for the prologue it replays.
    #[test]
    fn the_builds_stubs_differ_only_in_the_prologue() {
        let stubs: Vec<Vec<u8>> = BUILDS
            .iter()
            .map(|b| {
                stub(
                    &b.stolen,
                    0x1111_1111,
                    0x2222_2222,
                    0x3333_3333,
                    0x4444_4444,
                )
            })
            .collect();
        let differ: Vec<usize> = (0..stubs[0].len())
            .filter(|&i| stubs[0][i] != stubs[1][i])
            .collect();
        // `sub esp, 0x20` against `sub esp, 0x3c`: the immediate, at 57 + 5.
        assert_eq!(differ, [62]);
        assert_eq!((stubs[0][62], stubs[1][62]), (0x20, 0x3c));
    }
}
