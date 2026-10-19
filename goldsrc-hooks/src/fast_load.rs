//! `dodstudio_demo_fast_load`: a `viewdemo` demo loads as fast as the frame
//! allows, not 33 packets a frame (#798).
//!
//! ## The limit
//!
//! `viewdemo` reads the whole demo into an HLTV world (`Core.dll`) while
//! playback has already started (`docs/goldsrc_viewdemo.md` §5).
//! `Server::RunFrame` reads it, and its loop stops for the frame after 33
//! packets:
//!
//! ```text
//! pre-Anniversary  Core.dll+0x10dae      25th Anniversary  Core.dll+0x12db6
//!   mov  eax, [esp+0xc]   ; packets read     mov  eax, [ebp-4]
//!   inc  eax                                 inc  eax
//!   cmp  eax, 0x20                           mov  [ebp-4], eax
//!   mov  [esp+0xc], eax                      cmp  eax, 0x20
//!   jg   stop for this frame                 jg   stop for this frame
//!   ...read the next packet; loop while there is one...
//! ```
//!
//! The count is the only per-frame limit: the loop otherwise runs until the
//! reader has nothing left. Measured on #797, the loader ran about 163 engine
//! frames a second on both builds, 59.6 s of demo per wall second, so a 21-min
//! HLTV demo took about 21 s to load and a 2-hour one about two minutes.
//!
//! ## The patch: a time budget instead of a count
//!
//! A detour replaces `inc eax` through the `cmp` (the stolen bytes, 8 on the
//! pre-Anniversary build and 7 on the 25th Anniversary one). The stub redoes
//! the increment and the store, asks [`packet_cap`] for this packet's cap,
//! and compares the count against it, so the engine's own `jg` right after the
//! span decides:
//!
//! ```asm
//!     inc  eax
//!     mov  <count>, eax       ; before any push: [esp+0xc] is still the count
//!     push ecx
//!     push edx
//!     push eax                ; the count, packet_cap's argument
//!     mov  eax, packet_cap
//!     call eax                ; cdecl: eax = the cap
//!     add  esp, 4
//!     pop  edx
//!     pop  ecx
//!     cmp  <count>, eax       ; count - cap: the flags the engine's jg reads
//!     jmp  dword ptr [RESUME] ; the jg, which a jmp leaves the flags for
//! ```
//!
//! [`packet_cap`] returns `i32::MAX` while this frame's loading has taken less
//! than the budget, and the engine's own `0x20` once it has, so a frame still
//! reads at least the engine's 33 packets and loading is never slower than
//! stock. The frame's clock starts at its first packet (count 1).
//! `dodstudio_demo_fast_load 0` makes it return `0x20` every time: exactly the
//! engine's own decision, through the stub.
//!
//! Why the stub may do what it does, checked against both builds by
//! `tools/verify_fast_load_offsets.py`:
//!
//! - `eax` is dead after the `jg`: both of its successors load `eax` first,
//!   so leaving the cap in it changes nothing.
//! - `ecx` and `edx` are saved around the call anyway; `ebx`, `esi`, `edi` and
//!   `ebp` are callee-saved under cdecl, which [`packet_cap`] is.
//! - The span starts right after a `call`, so no XMM or x87 value is live
//!   across it (all are caller-saved on 32-bit Windows) and the direction flag
//!   is clear, as a call needs.
//! - Nothing branches into the span, and the count is initialised to 0 at the
//!   function's entry, before the first packet.
//!
//! ## When it installs
//!
//! `Core.dll` loads only when a demo is played with `viewdemo`, so [`poll`]
//! looks it up each frame (`GetModuleHandleA`, the same as `demo_seek` does
//! for `DemoPlayer.dll`) and installs once it appears. Once in, a frame costs
//! that lookup and a one-byte read that the jump is still there; a `Core.dll`
//! unloaded and loaded again gets the patch again.
//!
//! ## The budget
//!
//! [`DEFAULT_BUDGET_MS`] of loading per frame leaves the frame time for the
//! rest of the engine, so playback stays smooth while the demo loads. The
//! cvar's value is the budget in milliseconds above 1; `1` is the default
//! budget and `0` the engine's own 33 packets.
//!
//! The hook log says how each load went, so a budget can be judged by numbers:
//! `fast_load: <demo> loaded in <s>, <n> packets, ...`.

// The detour and the module lookup are only built for the 32-bit DLL; a host
// build compiles the pure parts for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::cell::RefCell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, AtomicU32, AtomicUsize, Ordering};
use std::time::Instant;

use crate::engine::CvarSPartial;
use crate::names::console_name;

pub const NAME: &str = console_name!("demo_fast_load");

/// The engine's own cap: `jg` after `cmp count, 0x20` stops after 33 packets.
const ENGINE_CAP: i32 = 0x20;
/// What [`packet_cap`] returns while the frame is under budget: `jg` never
/// fires on it.
const NO_CAP: i32 = i32::MAX;

/// Milliseconds of loading per frame that `dodstudio_demo_fast_load 1` means.
pub const DEFAULT_BUDGET_MS: u32 = 8;
/// The largest budget accepted. More would freeze the picture for a tenth of
/// a second at a time, which no load is worth.
pub const MAX_BUDGET_MS: u32 = 100;

/// One `Core.dll` build's packet loop.
struct Build {
    name: &'static str,
    /// Bytes around the counter's increment, unique in that build's
    /// `Core.dll`: the reader call before it, the load, the stolen bytes, the
    /// `jg`, and the next packet's read.
    pattern: &'static str,
    /// Where the stolen bytes start within `pattern`.
    site: usize,
    /// The bytes the jump overwrites, checked before writing.
    stolen: &'static [u8],
    /// The count's memory operand, as the ModRM (reg field `eax`), SIB and
    /// displacement bytes that follow `89` (`mov r/m32, eax`) and `39`
    /// (`cmp r/m32, eax`).
    count: &'static [u8],
}

/// The builds the patch knows, tried in order.
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        pattern: "E8 ?? ?? ?? ?? 8B 44 24 0C 40 83 F8 20 89 44 24 0C 7F ?? 8B 07 8B CF FF 50 60",
        site: 9,
        // inc eax; cmp eax, 0x20; mov [esp+0xc], eax
        stolen: &[0x40, 0x83, 0xf8, 0x20, 0x89, 0x44, 0x24, 0x0c],
        // [esp+0xc]
        count: &[0x44, 0x24, 0x0c],
    },
    Build {
        name: "25th Anniversary",
        pattern: "E8 ?? ?? ?? ?? 8B 45 FC 40 89 45 FC 83 F8 20 7F ?? 8B 07 8B CF 8B 40 60 FF D0",
        site: 8,
        // inc eax; mov [ebp-4], eax; cmp eax, 0x20
        stolen: &[0x40, 0x89, 0x45, 0xfc, 0x83, 0xf8, 0x20],
        // [ebp-4]
        count: &[0x45, 0xfc],
    },
];

/// The budget a cvar value asks for, in milliseconds; 0 is the engine's own
/// 33 packets a frame. Anything that isn't a positive number is 0, the way
/// the engine behaves.
pub fn budget_ms(value: f32) -> u32 {
    if !value.is_finite() || value < 1.0 {
        return 0;
    }
    // `as` saturates, and the value is at least 1 here.
    match value as u32 {
        1 => DEFAULT_BUDGET_MS,
        ms => ms.min(MAX_BUDGET_MS),
    }
}

/// The cap for the packet just read: none while the frame's loading has
/// taken less than `budget_ns`, the engine's own once it has. `budget_ns` 0
/// is the engine's cap every time.
fn cap(elapsed_ns: u64, budget_ns: u64) -> i32 {
    if budget_ns == 0 || elapsed_ns >= budget_ns {
        ENGINE_CAP
    } else {
        NO_CAP
    }
}

/// The current budget in nanoseconds, set by [`poll`] from the cvar each
/// frame and read by [`packet_cap`] per packet.
static BUDGET_NS: AtomicI64 = AtomicI64::new(0);
/// When this frame's first packet was read, in nanoseconds since [`EPOCH`].
static FRAME_START_NS: AtomicI64 = AtomicI64::new(0);
/// Packets read since the session started, for the load's log line.
static PACKETS: AtomicU32 = AtomicU32::new(0);
/// The clock [`packet_cap`] measures from. Set when the patch installs.
static EPOCH: OnceLock<Instant> = OnceLock::new();

fn now_ns() -> i64 {
    match EPOCH.get() {
        // Saturates far beyond any session.
        Some(epoch) => i64::try_from(epoch.elapsed().as_nanos()).unwrap_or(i64::MAX),
        None => 0,
    }
}

/// Called by the stub for every packet `Server::RunFrame` reads, with the
/// count of packets read this frame (1 for the first). Returns the cap the
/// engine's `jg` compares the count against.
///
/// Runs inside the engine's loop: no locks, no allocation, nothing that can
/// panic (a panic is an abort in a release build).
extern "C" fn packet_cap(count: u32) -> i32 {
    PACKETS.fetch_add(1, Ordering::Relaxed);
    let budget = BUDGET_NS.load(Ordering::Relaxed);
    if budget <= 0 {
        return ENGINE_CAP;
    }
    let now = now_ns();
    if count <= 1 {
        FRAME_START_NS.store(now, Ordering::Relaxed);
        return NO_CAP;
    }
    let start = FRAME_START_NS.load(Ordering::Relaxed);
    let elapsed = now.saturating_sub(start).max(0) as u64;
    cap(elapsed, budget as u64)
}

/// The stub, hand-assembled; see the module doc for the listing.
fn stub(count: &[u8], callback: usize, resume_slot: usize) -> Vec<u8> {
    let mut code = vec![0x40]; // inc eax
    code.push(0x89); // mov <count>, eax
    code.extend_from_slice(count);
    code.extend_from_slice(&[0x51, 0x52, 0x50]); // push ecx; push edx; push eax
    code.push(0xb8); // mov eax, imm32
    code.extend_from_slice(&(callback as u32).to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0]); // call eax
    code.extend_from_slice(&[0x83, 0xc4, 0x04]); // add esp, 4
    code.extend_from_slice(&[0x5a, 0x59]); // pop edx; pop ecx
    code.push(0x39); // cmp <count>, eax
    code.extend_from_slice(count);
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume_slot as u32).to_le_bytes());
    code
}

/// `dodstudio_demo_fast_load`, once registered.
static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The fallback toggle's state when the cvar could not be registered: on
/// means the default budget.
pub static FALLBACK_ON: AtomicBool = AtomicBool::new(true);

/// Called by `commands.rs` once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// The budget the setting asks for now, in milliseconds.
fn wanted_budget_ms() -> u32 {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        if FALLBACK_ON.load(Ordering::Relaxed) {
            DEFAULT_BUDGET_MS
        } else {
            0
        }
    } else {
        // Safety: the engine owns the cvar for the session.
        budget_ms(unsafe { (*cvar).value })
    }
}

fn describe_budget(ms: u32) -> String {
    match ms {
        0 => "off (the engine's 33 packets a frame)".to_string(),
        ms => format!("{ms} ms a frame"),
    }
}

/// For the fallback toggle's bare-name query.
pub fn status() -> String {
    format!("demo loading: {}", describe_budget(wanted_budget_ms()))
}

/// Where the stub jumps back to: the engine's `jg`.
static RESUME: AtomicUsize = AtomicUsize::new(0);
/// The `Core.dll` the patch is in, and where its jump is; 0 when it isn't.
static PATCHED_BASE: AtomicUsize = AtomicUsize::new(0);
static PATCHED_AT: AtomicUsize = AtomicUsize::new(0);
/// The `Core.dll` the last failed install was on, so a failure is logged once.
static FAILED_BASE: AtomicUsize = AtomicUsize::new(0);
static BUILD_NAME: std::sync::Mutex<&str> = std::sync::Mutex::new("");

/// Finds the loop with the first of [`BUILDS`] whose signature matches and
/// patches it. Returns the build's name, the patched address and the stub's.
fn install_at(base: usize) -> Result<(&'static str, usize, usize), String> {
    let mut misses = Vec::new();
    for build in &BUILDS {
        // Safety: `base` is a module handle the loader gave us.
        let found = match unsafe { crate::scan::find_unique(base, build.pattern) } {
            Ok(found) => found,
            Err(why) => {
                misses.push(format!("{} signature: {why}", build.name));
                continue;
            }
        };
        let target = found + build.site;
        // Safety: inside the match, which is longer than site + stolen.
        let present =
            unsafe { std::slice::from_raw_parts(target as *const u8, build.stolen.len()) };
        if present != build.stolen {
            return Err(format!(
                "expected {:02x?} at +{:#x}, found {present:02x?}",
                build.stolen,
                target - base
            ));
        }
        EPOCH.get_or_init(Instant::now);
        RESUME.store(target + build.stolen.len(), Ordering::Release);
        let code = stub(
            build.count,
            packet_cap as extern "C" fn(u32) -> i32 as usize,
            RESUME.as_ptr() as usize,
        );
        // Safety: the span was checked byte for byte above, and nothing
        // branches into it -- checked by tools/verify_fast_load_offsets.py.
        let detour = unsafe { crate::detour::install(target, build.stolen.len(), &code) }?;
        // Never undone (see `detour::Detour`): a `Core.dll` unloaded under it
        // takes the jump with it, and the stub page stays, unreachable.
        return Ok((build.name, target, detour.stub_address()));
    }
    Err(misses.join("; "))
}

#[cfg(target_arch = "x86")]
fn core_dll() -> usize {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
    // Safety: a plain lookup of a loaded module by name.
    unsafe { GetModuleHandleA(c"Core.dll".as_ptr() as *const u8) as usize }
}

#[cfg(not(target_arch = "x86"))]
fn core_dll() -> usize {
    0
}

/// Installs the patch when `Core.dll` is loaded and doesn't have it.
fn keep_installed() {
    let base = core_dll();
    if base == 0 {
        PATCHED_BASE.store(0, Ordering::Relaxed);
        return;
    }
    if PATCHED_BASE.load(Ordering::Relaxed) == base {
        let at = PATCHED_AT.load(Ordering::Relaxed);
        // Safety: `at` is inside the module just found loaded at `base`.
        if unsafe { *(at as *const u8) } == 0xe9 {
            return;
        }
    }
    if FAILED_BASE.load(Ordering::Relaxed) == base {
        return;
    }
    match install_at(base) {
        Ok((build, at, stub)) => {
            PATCHED_BASE.store(base, Ordering::Relaxed);
            PATCHED_AT.store(at, Ordering::Relaxed);
            if let Ok(mut name) = BUILD_NAME.lock() {
                *name = build;
            }
            unsafe {
                crate::debug::report(&format!(
                    "fast_load: installed ({build} Core.dll, Server::RunFrame packet cap at +{:#x}, stub {stub:#x}) (#798)",
                    at - base
                ))
            };
        }
        Err(why) => {
            FAILED_BASE.store(base, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "fast_load: not installed -- {why}; demos load at the engine's 33 packets a frame"
                ))
            };
        }
    }
}

/// One `viewdemo` load being watched.
struct Load {
    started: Instant,
    last_frame: Instant,
    /// Every frame's length while loading, in milliseconds.
    frames: Vec<f32>,
    packets_at_start: u32,
    budget_ms: u32,
    budget_changed: bool,
    /// Seconds of demo read so far.
    buffered: f64,
}

thread_local! {
    // Only `poll` touches it, on the engine's thread.
    static LOAD: RefCell<Option<Load>> = const { RefCell::new(None) };
}

/// The longest, 99th-percentile and median of `frames` (milliseconds).
fn frame_stats(frames: &[f32]) -> Option<(f32, f32, f32)> {
    if frames.is_empty() {
        return None;
    }
    let mut sorted = frames.to_vec();
    sorted.sort_by(f32::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    Some((sorted[sorted.len() - 1], at(0.99), at(0.5)))
}

fn finish(load: Load) {
    let seconds = load.started.elapsed().as_secs_f64();
    let packets = PACKETS
        .load(Ordering::Relaxed)
        .wrapping_sub(load.packets_at_start);
    let demo = crate::demo_reload::current_demo().unwrap_or_else(|| "the demo".to_string());
    let frames = match frame_stats(&load.frames) {
        Some((longest, p99, median)) => format!(
            "longest frame {longest:.0} ms (99th percentile {p99:.1} ms, median {median:.1} ms, {} frames)",
            load.frames.len()
        ),
        None => "no frames".to_string(),
    };
    let budget = match load.budget_ms {
        0 => "budget off".to_string(),
        ms => format!("budget {ms} ms"),
    };
    let changed = if load.budget_changed {
        " (changed during the load)"
    } else {
        ""
    };
    unsafe {
        crate::debug::report(&format!(
            "fast_load: {demo} loaded in {seconds:.1} s, {packets} packets, {frames}, {budget}{changed}, {:.0} s of demo (#798)",
            load.buffered
        ))
    };
    crate::commands::console_print(&format!(
        "Demo loaded in {seconds:.1} s ({}).\n",
        describe_budget(load.budget_ms)
    ));
}

/// Keeps the patch in and the budget current, and logs how each load went.
/// Called every frame from `commands::poll`. A module lookup per frame while
/// no demo plays; while `Core.dll` is loaded, a check of the demo player too.
pub fn poll() {
    let budget = wanted_budget_ms();
    BUDGET_NS.store(i64::from(budget) * 1_000_000, Ordering::Relaxed);
    keep_installed();
    if PATCHED_BASE.load(Ordering::Relaxed) == 0 {
        LOAD.with_borrow_mut(|load| load.take());
        return;
    }
    let buffered = crate::demo_seek::buffered_while_loading();
    let now = Instant::now();
    LOAD.with_borrow_mut(|slot| match (slot.as_mut(), buffered) {
        (None, Some(seconds)) => {
            *slot = Some(Load {
                started: now,
                last_frame: now,
                frames: Vec::new(),
                packets_at_start: PACKETS.load(Ordering::Relaxed),
                budget_ms: budget,
                budget_changed: false,
                buffered: seconds,
            });
        }
        (Some(load), Some(seconds)) => {
            load.frames
                .push(now.duration_since(load.last_frame).as_secs_f32() * 1000.0);
            load.last_frame = now;
            load.buffered = seconds;
            load.budget_changed |= load.budget_ms != budget;
        }
        (Some(_), None) => {
            if let Some(load) = slot.take() {
                finish(load);
            }
        }
        (None, None) => {}
    });
}

/// The `dodstudio_debug_status` line, or `None` while `Core.dll` isn't
/// patched.
pub fn status_line() -> Option<String> {
    if PATCHED_BASE.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let build = BUILD_NAME.lock().map(|name| *name).unwrap_or_default();
    Some(format!(
        "demo fast load: {} ({build} Core.dll)",
        describe_budget(wanted_budget_ms())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cvar_value_is_a_budget_in_milliseconds() {
        assert_eq!(budget_ms(0.0), 0);
        assert_eq!(budget_ms(-3.0), 0);
        assert_eq!(budget_ms(0.5), 0);
        assert_eq!(budget_ms(f32::NAN), 0);
        assert_eq!(budget_ms(f32::INFINITY), 0);
        assert_eq!(budget_ms(1.0), DEFAULT_BUDGET_MS);
        assert_eq!(budget_ms(1.9), DEFAULT_BUDGET_MS);
        assert_eq!(budget_ms(2.0), 2);
        assert_eq!(budget_ms(12.0), 12);
        assert_eq!(budget_ms(5000.0), MAX_BUDGET_MS);
    }

    #[test]
    fn under_budget_reads_on_and_over_it_stops_at_the_engines_cap() {
        let budget = 8_000_000;
        assert_eq!(cap(0, budget), NO_CAP);
        assert_eq!(cap(7_999_999, budget), NO_CAP);
        assert_eq!(cap(8_000_000, budget), ENGINE_CAP);
        assert_eq!(cap(u64::MAX, budget), ENGINE_CAP);
    }

    #[test]
    fn no_budget_is_the_engines_cap_every_time() {
        assert_eq!(cap(0, 0), ENGINE_CAP);
        assert_eq!(cap(u64::MAX, 0), ENGINE_CAP);
    }

    #[test]
    fn the_stub_assembles_to_what_the_module_doc_claims() {
        #[rustfmt::skip]
        let pre = vec![
            0x40,                         // inc  eax
            0x89, 0x44, 0x24, 0x0c,       // mov  [esp+0xc], eax
            0x51, 0x52, 0x50,             // push ecx; push edx; push eax
            0xb8, 0x44, 0x33, 0x22, 0x11, // mov  eax, packet_cap
            0xff, 0xd0,                   // call eax
            0x83, 0xc4, 0x04,             // add  esp, 4
            0x5a, 0x59,                   // pop  edx; pop ecx
            0x39, 0x44, 0x24, 0x0c,       // cmp  [esp+0xc], eax
            0xff, 0x25, 0x88, 0x77, 0x66, 0x55, // jmp dword [RESUME]
        ];
        assert_eq!(stub(BUILDS[0].count, 0x1122_3344, 0x5566_7788), pre);
        let post = stub(BUILDS[1].count, 0x1122_3344, 0x5566_7788);
        assert_eq!(&post[1..4], &[0x89, 0x45, 0xfc]); // mov [ebp-4], eax
        assert_eq!(&post[19..22], &[0x39, 0x45, 0xfc]); // cmp [ebp-4], eax
    }

    /// The stolen bytes are the pattern's at `site`, store and compare use the
    /// operand the engine's own store does, and the jump fits.
    #[test]
    fn each_build_steals_what_its_pattern_holds() {
        for build in &BUILDS {
            let bytes: Vec<Option<u8>> = build
                .pattern
                .split_whitespace()
                .map(|t| u8::from_str_radix(t, 16).ok())
                .collect();
            let stolen: Vec<Option<u8>> = build.stolen.iter().copied().map(Some).collect();
            assert_eq!(
                &bytes[build.site..build.site + build.stolen.len()],
                &stolen[..],
                "{}",
                build.name
            );
            assert!(build.stolen.len() >= 5, "{}", build.name);
            // The jg follows the stolen bytes.
            assert_eq!(bytes[build.site + build.stolen.len()], Some(0x7f));
            let store: Vec<u8> = std::iter::once(0x89)
                .chain(build.count.iter().copied())
                .collect();
            assert!(
                build.stolen.windows(store.len()).any(|w| w == store),
                "{}",
                build.name
            );
            assert!(crate::scan::Pattern::parse(build.pattern).is_ok());
        }
    }

    #[test]
    fn frame_stats_pick_the_longest_the_tail_and_the_middle() {
        assert_eq!(frame_stats(&[]), None);
        let frames: Vec<f32> = (1..=100).map(|ms| ms as f32).collect();
        assert_eq!(frame_stats(&frames), Some((100.0, 99.0, 51.0)));
    }
}
