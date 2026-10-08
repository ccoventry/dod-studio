//! Stops a crash when the session's first demo reaches DoD's map models before
//! the engine has pointed `pmove` anywhere (issue #546).
//!
//! ## The crash
//!
//! `access violation at hw.dll+0x3a77c -- writing 0xbc`, pre-Anniversary, while
//! the first demo of a fresh `hl.exe` loads. Three batches from one
//! `dod_lennon2` POV demo hit it; other demos of the same map did not.
//!
//! ## Why
//!
//! DoD's `InitHUD` user message makes `client.dll` re-read the map's entity
//! lump (`DoD_LoadEntityLump` -> `DoD_ProcessEnts`) and add every static
//! `env_model` (`CClientEnvModel::AddEnvModel`). A model with spawnflag 2 is
//! dropped to the ground: the client calls `pEventAPI->EV_SetTraceHull(2)`
//! and traces 8000 units down. `EV_SetTraceHull` is `pmove->usehull = hull`.
//!
//! `pmove` is a global pointer in `hw.dll`, NULL at start-up. The engine only
//! ever *assigns* it -- `&g_clmove` at the end of every `CL_ReadPackets` and
//! in client prediction, `&g_svmove` on a server -- and never clears it. A
//! session that starts straight into `playdemo` has not finished one
//! `CL_ReadPackets` yet while it parses the demo's first packets, so when a
//! demo carries `InitHUD` in that first batch, the write lands on NULL.
//! Any later demo, or an `InitHUD` one read later, finds `pmove` already set.
//!
//! ## The fix
//!
//! One pointer write, no code patch: at install -- after `client.dll` has
//! initialised and before any map loads -- point `pmove` at the engine's own
//! `g_clmove` if it is still NULL, exactly the value the engine stores there
//! itself a moment later. `g_clmove` is zeroed static data then, so the
//! drop-to-floor trace has no physics entities to hit and the model keeps its
//! authored height: in that one case models are not nudged onto the ground,
//! instead of the game closing.
//!
//! Both addresses come from the engine's own code, per build: `pmove` from
//! `EV_SetTraceHull`'s operand, `g_clmove` from the prediction setup's
//! `mov [pmove], imm32`, and the two must name the same `pmove`.
//! `tools/verify_pmove_guard.py` re-derives both builds offline.
//!
//! On by default, since it only acts where the game would otherwise crash;
//! `GOLDSRC_HOOKS_PMOVE_GUARD=0` turns it off.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::engine;
use crate::scan;

/// Whether to install at all -- `GOLDSRC_HOOKS_PMOVE_GUARD=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Set once `pmove` was found NULL and pointed at `g_clmove`.
static SET: AtomicBool = AtomicBool::new(false);

/// One build's two anchors.
struct Site {
    build: &'static str,
    /// `EV_SetTraceHull`: `mov reg, [pmove]; mov reg2, [ebp+8]; mov [reg+0xbc], reg2`.
    trace_hull: &'static str,
    /// Offset of `pmove`'s address in [`Site::trace_hull`].
    trace_hull_pmove_at: usize,
    /// The client prediction setup that begins `mov [pmove], &g_clmove`.
    setup: &'static str,
    /// Offset of the `C7 05` store in [`Site::setup`]; the address follows
    /// at +2 and the value at +6.
    setup_store_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //          push ebp;mov ebp,esp  mov ecx,[pmove]      mov eax,[ebp+8]  mov [ecx+0xbc],eax  pop  ret
        trace_hull: "55 8B EC 8B 0D ?? ?? ?? ?? 8B 45 08 89 81 BC 00 00 00 5D C3",
        trace_hull_pmove_at: 5,
        //     push ebp..sub esp,8  mov ecx,[..]         push x3  lea       mov [pmove],&g_clmove            shl eax,3 sub eax,ecx
        setup: "55 8B EC 83 EC 08 8B 0D ?? ?? ?? ?? 53 56 57 8D 04 C9 C7 05 ?? ?? ?? ?? ?? ?? ?? ?? C1 E0 03 2B C1",
        setup_store_at: 18,
    },
    Site {
        build: "25th Anniversary",
        //          push ebp;mov ebp,esp  mov eax,[pmove]   mov ecx,[ebp+8]  mov [eax+0xbc],ecx  pop  ret
        trace_hull: "55 8B EC A1 ?? ?? ?? ?? 8B 4D 08 89 88 BC 00 00 00 5D C3",
        trace_hull_pmove_at: 4,
        //     push ebp..sub esp,8  mov edx,[..]         xor  push imul ebx,[..],0x4318          push xor    mov [pmove],&g_clmove            push mov [ebp-4],ecx
        setup: "55 8B EC 83 EC 08 8B 15 ?? ?? ?? ?? 33 C9 53 69 1D ?? ?? ?? ?? 18 43 00 00 56 33 F6 C7 05 ?? ?? ?? ?? ?? ?? ?? ?? 57 89 4D FC",
        setup_store_at: 28,
    },
];

/// Points `pmove` at `g_clmove` if nothing has yet. A second call does
/// nothing.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "pmove_guard: off (GOLDSRC_HOOKS_PMOVE_GUARD=0) -- a first demo that sends InitHUD early can still crash in EV_SetTraceHull (#546)",
            )
        };
        return;
    }
    if SET.load(Ordering::Acquire) {
        return;
    }
    let report = match install_inner() {
        Ok(Outcome::Set {
            build,
            pmove,
            clmove,
        }) => {
            SET.store(true, Ordering::Release);
            format!(
                "pmove_guard: {build} hw.dll -- pmove ({pmove:#x}) was unset, now g_clmove ({clmove:#x}), so an early InitHUD can't crash the first demo (#546)"
            )
        }
        Ok(Outcome::AlreadySet { build, value }) => format!(
            "pmove_guard: {build} hw.dll -- pmove already set ({value:#x}), nothing to do (#546)"
        ),
        Err(why) => format!("pmove_guard: not installed -- {why}"),
    };
    unsafe { crate::debug::report(&report) };
}

enum Outcome {
    Set {
        build: &'static str,
        pmove: usize,
        clmove: usize,
    },
    AlreadySet {
        build: &'static str,
        value: usize,
    },
}

fn install_inner() -> Result<Outcome, String> {
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let size =
        unsafe { crate::pe::image_size(base as *mut u8) }.ok_or("hw.dll has no PE header")?;
    let inside = |address: usize| address >= base && address + 4 <= base + size;

    let mut errors = Vec::new();
    for site in SITES {
        // Safety: `base` is a module handle the loader gave us.
        let hull = match unsafe { scan::find_unique(base, site.trace_hull) } {
            Ok(found) => found,
            Err(why) => {
                errors.push(format!("{}: EV_SetTraceHull {why}", site.build));
                continue;
            }
        };
        // Safety: as above.
        let setup = unsafe { scan::find_unique(base, site.setup) }
            .map_err(|why| format!("{}: prediction setup {why}", site.build))?;
        // Safety: every offset read is inside its matched span.
        let (pmove, stored_at, clmove) = unsafe {
            (
                read_u32(hull + site.trace_hull_pmove_at),
                read_u32(setup + site.setup_store_at + 2),
                read_u32(setup + site.setup_store_at + 6),
            )
        };
        check_anchors(pmove, stored_at, clmove, inside)
            .map_err(|why| format!("{}: {why}", site.build))?;
        // Safety: `pmove` is a pointer-sized global inside hw.dll's image,
        // checked above; the engine reads it only on the main thread, which
        // this runs on, before any map has loaded.
        let slot = pmove as *mut usize;
        let value = unsafe { slot.read_volatile() };
        if value != 0 {
            return Ok(Outcome::AlreadySet {
                build: site.build,
                value,
            });
        }
        unsafe { slot.write_volatile(clmove) };
        return Ok(Outcome::Set {
            build: site.build,
            pmove,
            clmove,
        });
    }
    Err(format!("no known build matched ({})", errors.join("; ")))
}

/// Safety: `at..at + 4` must be readable.
unsafe fn read_u32(at: usize) -> usize {
    unsafe { (at as *const u32).read_unaligned() as usize }
}

/// The two anchors must name one `pmove`, and both addresses must lie inside
/// hw.dll: `g_clmove` is static data there.
fn check_anchors(
    pmove: usize,
    stored_at: usize,
    clmove: usize,
    inside: impl Fn(usize) -> bool,
) -> Result<(), String> {
    if pmove != stored_at {
        return Err(format!(
            "EV_SetTraceHull reads pmove at {pmove:#x}, but the prediction setup stores to {stored_at:#x}"
        ));
    }
    if !inside(pmove) {
        return Err(format!("pmove {pmove:#x} is not inside hw.dll"));
    }
    if !inside(clmove) {
        return Err(format!("g_clmove {clmove:#x} is not inside hw.dll"));
    }
    Ok(())
}

/// One `dodstudio_debug_status` line, once it has acted.
pub fn status_line() -> Option<String> {
    SET.load(Ordering::Relaxed)
        .then(|| "first-demo pmove guard: set pmove before the first demo (#546)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(pattern: &str) -> Vec<&str> {
        pattern.split_whitespace().collect()
    }

    #[test]
    fn each_site_points_at_a_wildcarded_pmove_and_a_store_to_it() {
        for site in SITES {
            let t = tokens(site.trace_hull);
            assert!(
                t[site.trace_hull_pmove_at..site.trace_hull_pmove_at + 4]
                    .iter()
                    .all(|b| *b == "??"),
                "{}: pmove operand",
                site.build
            );
            // `mov [reg+0xbc], reg2`: usehull, the field the crash wrote.
            assert_eq!(t[t.len() - 6..t.len() - 2], ["BC", "00", "00", "00"]);
            let s = tokens(site.setup);
            assert_eq!(
                s[site.setup_store_at..site.setup_store_at + 2],
                ["C7", "05"],
                "{}: mov dword [addr], imm32",
                site.build
            );
            assert!(
                s[site.setup_store_at + 2..site.setup_store_at + 10]
                    .iter()
                    .all(|b| *b == "??"),
                "{}: address and value are wildcards",
                site.build
            );
        }
    }

    #[test]
    fn the_builds_patterns_differ() {
        assert_ne!(SITES[0].trace_hull, SITES[1].trace_hull);
        assert_ne!(SITES[0].setup, SITES[1].setup);
    }

    #[test]
    fn anchors_must_agree_and_lie_inside_hw_dll() {
        // PRE's real values: pmove 0x2357f88, g_clmove 0x2d0a000.
        let inside = |a: usize| (0x1d00000..0x3000000).contains(&a);
        assert!(check_anchors(0x2357f88, 0x2357f88, 0x2d0a000, inside).is_ok());
        assert!(check_anchors(0x2357f88, 0x2357f8c, 0x2d0a000, inside).is_err());
        assert!(check_anchors(0x2357f88, 0x2357f88, 0x10, inside).is_err());
    }
}
