//! Lambda sprays in demos (#1322): the Players table decides them too.
//!
//! When the server has no custom spray for a player, DoD's spray can places
//! the stock lambda (`}lambda06`) as a plain `TE_WORLDDECAL`, with no player
//! attached, instead of a `TE_PLAYERDECAL`. So `spray_hltv`'s route never
//! sees it, and the table's choices never reach it. This module adds:
//!
//! - **A lambda the recorder of a POV demo sprayed** gets that recorder's
//!   choice in the table. [`plan`] says which those are, from the demo file.
//! - **Any other lambda** names no player: no guessing between players by
//!   position. It gets the fallback for the demo type, or nothing.
//!
//! ## The site
//!
//! `CL_ParseTEnt` has one case for `TE_DECAL`, `TE_WORLDDECAL` and the like.
//! The detour goes right after its `r_decals` check, where the message's
//! index, entity and position are all read. Only a message says which decal
//! this is: the pool's slot for the lambda changes by session, and calling
//! `Draw_DecalTexture` with the message's index is a fatal error when that
//! slot isn't loaded ("Cache wad indexed before load decals.wad").
//!
//! The stub asks [`world_decal`], which does what the table says through
//! `spray_hltv::shoot`, the engine's own `R_CustomDecalShoot`, and answers
//! non-zero. The stub then leaves through the case's own "`r_decals` is off"
//! exit, so the engine shoots nothing. With 0 the engine goes on as before.
//! "No spray" shoots nothing at all here: unlike `TE_PLAYERDECAL`'s route,
//! this site has a way out that needs no transparent decal.
//!
//! `R_DecalShoot` gives `}lambda06` by name the custom flag `R_CustomDecalShoot`
//! shoots with, so a lambda's replacement sits on the wall as the lambda did.
//!
//! ## Installing
//!
//! After `spray_hltv`'s two detours, whose `Draw_DecalTexture` one draws what
//! this shoots. Without this one, lambdas are the engine's as before.
//! `tools/verify_lambda_spray_offsets.py` checks every offset here against
//! both builds' `hw.dll`.

mod plan;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::lookahead::timeline::ReadError;
use crate::missing_shots::{self, Playing};
use crate::player_table::Choice;
use crate::spray_hltv;
use crate::{demo_reload, detour, engine, scan};

use plan::{LAMBDA_DECAL, Plan, Vec3, Verdict};

/// One engine build's site in `CL_ParseTEnt`'s decal case.
struct Build {
    name: &'static str,
    /// From the `jnp` that leaves the case when `r_decals` is 0, through the
    /// site, to the code that shoots the decal.
    pattern: &'static str,
    /// Where in that match the site is.
    site_at: usize,
    /// Where in that match the `jnp`'s `rel32` is.
    exit_rel_at: usize,
    /// The bytes the jump overwrites, reproduced in the stub: position
    /// independent, and they read nothing the callback changes.
    stolen: &'static [u8],
    /// Pushes the callback's arguments, last first: the position, whether
    /// this is a `TE_BSPDECAL`, the entity and the index. May use `eax`.
    push_args: &'static [u8],
}

/// The builds this knows, tried in order.
const BUILDS: [Build; 2] = [
    // The index is at [ebp-0x34], the entity in ebx, the TE_BSPDECAL flag at
    // [ebp-0x2c], the position at [ebp-0x50].
    Build {
        name: "pre-Anniversary",
        pattern: "0F 8B ?? ?? ?? ?? 8B 55 D4 8B 4D CC 8D 45 B0 52 50 56 53 51 E9",
        site_at: 6,
        exit_rel_at: 2,
        stolen: &[0x8b, 0x55, 0xd4, 0x8b, 0x4d, 0xcc],
        push_args: &[
            0x8d, 0x45, 0xb0, // lea  eax, [ebp-0x50]
            0x50, // push eax
            0xff, 0x75, 0xd4, // push [ebp-0x2c]
            0x53, // push ebx
            0xff, 0x75, 0xcc, // push [ebp-0x34]
        ],
    },
    // The index is in edi, the entity in esi, the TE_BSPDECAL flag in ebx,
    // the position at [ebp-0x10].
    Build {
        name: "25th Anniversary",
        pattern: "0F 8B ?? ?? ?? ?? 53 8D 45 F0 50 FF 75 A4 56 57 FF 15 ?? ?? ?? ?? 83 C4 04 50 FF 15",
        site_at: 6,
        exit_rel_at: 2,
        stolen: &[0x53, 0x8d, 0x45, 0xf0, 0x50],
        push_args: &[
            0x8d, 0x45, 0xf0, // lea  eax, [ebp-0x10]
            0x50, // push eax
            0x53, // push ebx
            0x56, // push esi
            0x57, // push edi
        ],
    },
];

/// Where the stub goes back to the engine's shot, and where it leaves.
static RESUME: AtomicUsize = AtomicUsize::new(0);
static EXIT: AtomicUsize = AtomicUsize::new(0);
/// The patched site, for [`check_site`].
static SITE: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Set when installing failed for good, so it isn't retried every second.
static GAVE_UP: AtomicBool = AtomicBool::new(false);
/// Set once the patched site was found overwritten, so it is said once.
static OVERWRITTEN: AtomicBool = AtomicBool::new(false);
static BUILD_NAME: Mutex<&str> = Mutex::new("");
/// See [`detour::Detour`] on why this is never undone.
static DETOUR: Mutex<Option<detour::Detour>> = Mutex::new(None);
/// Lambdas replaced or removed this session.
static HANDLED: AtomicU32 = AtomicU32::new(0);

/// The demo a read was asked for last. Looked at once a second.
static REQUESTED: Mutex<Option<String>> = Mutex::new(None);
/// The newest read asked for: an older read stops, as `lookahead`'s does.
static LATEST_READ: AtomicU64 = AtomicU64::new(0);
/// The newest plan and the demo it is for. Written by the reading thread,
/// read by the engine thread with `try_read` only.
static PLAN: RwLock<Option<(String, Arc<Plan>)>> = RwLock::new(None);

/// The `CL_ParseTEnt` stub, at the site.
///
/// ```asm
///         <push_args>
///         mov   eax, world_decal
///         call  eax                 ; cdecl: keeps ebx, esi, edi, ebp
///         add   esp, 16
///         test  eax, eax
///         jnz   .done
///         <stolen>
///         jmp   dword [RESUME]
/// .done:  jmp   dword [EXIT]
/// ```
///
/// `eax`, `ecx` and `edx` are written before they are read on both paths
/// (`tools/verify_lambda_spray_offsets.py`), and the x87 stack is empty after
/// `r_decals`' `fcomp` (pre-Anniversary) or was never used (25th Anniversary).
fn stub(build: &Build, callback: usize, resume: usize, exit: usize) -> Vec<u8> {
    let mut code = build.push_args.to_vec();
    code.push(0xb8); // mov eax, imm32
    code.extend_from_slice(&(callback as u32).to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0]); // call eax
    code.extend_from_slice(&[0x83, 0xc4, 0x10]); // add esp, 16
    code.extend_from_slice(&[0x85, 0xc0]); // test eax, eax
    let jnz_at = code.len();
    code.extend_from_slice(&[0x75, 0]); // jnz .done, patched below
    code.extend_from_slice(build.stolen);
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword [resume]
    code.extend_from_slice(&(resume as u32).to_le_bytes());
    code[jnz_at + 1] = (code.len() - (jnz_at + 2)) as u8;
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword [exit]
    code.extend_from_slice(&(exit as u32).to_le_bytes());
    code
}

/// What one build's signature found in the loaded `hw.dll`.
struct Site {
    build: &'static Build,
    at: usize,
    exit: usize,
}

fn find_site(base: usize) -> Result<Site, String> {
    let mut misses = Vec::new();
    for build in &BUILDS {
        // Safety: `base` is hw.dll's module handle, mapped for the session.
        let found = match unsafe { scan::find_unique(base, build.pattern) } {
            Ok(found) => found,
            Err(why) => {
                misses.push(format!("{}: {why}", build.name));
                continue;
            }
        };
        let at = found + build.site_at;
        // Safety: inside the match, checked by the pattern to be a `jnp`.
        let rel = unsafe { ((found + build.exit_rel_at) as *const i32).read_unaligned() };
        return Ok(Site {
            build,
            at,
            exit: at.wrapping_add_signed(rel as isize),
        });
    }
    Err(misses.join("; "))
}

fn install() {
    if INSTALLED.load(Ordering::Acquire) || GAVE_UP.load(Ordering::Relaxed) {
        return;
    }
    let Some(base) = engine::engine_module_base() else {
        return;
    };
    let result = find_site(base).and_then(|site| {
        let build = site.build;
        RESUME.store(site.at + build.stolen.len(), Ordering::Release);
        EXIT.store(site.exit, Ordering::Release);
        let code = stub(
            build,
            world_decal as *const () as usize,
            RESUME.as_ptr() as usize,
            EXIT.as_ptr() as usize,
        );
        // Safety: the signature holds the stolen bytes, and nothing branches
        // into the span (tools/verify_lambda_spray_offsets.py).
        let detour = unsafe { detour::install(site.at, build.stolen.len(), &code) }?;
        Ok((site, detour))
    });
    match result {
        Ok((site, detour)) => {
            if let Ok(mut slot) = DETOUR.lock() {
                *slot = Some(detour);
            }
            if let Ok(mut name) = BUILD_NAME.lock() {
                *name = site.build.name;
            }
            SITE.store(site.at, Ordering::Relaxed);
            INSTALLED.store(true, Ordering::Release);
            report(&format!(
                "on ({} hw.dll): CL_ParseTEnt +{:#x}",
                site.build.name,
                site.at - base
            ));
        }
        Err(why) => {
            GAVE_UP.store(true, Ordering::Relaxed);
            report(&format!("not installed -- {why}; lambdas stay the game's"));
        }
    }
}

/// Says once if something overwrote the jump.
fn check_site() {
    let at = SITE.load(Ordering::Relaxed);
    if at == 0 || OVERWRITTEN.load(Ordering::Relaxed) {
        return;
    }
    // Safety: a site in hw.dll's code, patched by `install`.
    if unsafe { (at as *const u8).read_volatile() } != 0xe9 {
        OVERWRITTEN.store(true, Ordering::Relaxed);
        report("the jump in CL_ParseTEnt was overwritten by something else");
    }
}

/// Once a second, once `spray_hltv`'s detours are in: installs this one and,
/// in a POV demo not read yet, starts reading it.
pub fn poll() {
    install();
    check_site();
    if !matches!(missing_shots::playing_now(), Playing::Pov { .. }) {
        return;
    }
    let Some(name) = demo_reload::current_demo() else {
        return;
    };
    let Ok(mut requested) = REQUESTED.try_lock() else {
        return;
    };
    if requested.as_ref() != Some(&name) {
        *requested = Some(name.clone());
        start_read(name);
    }
}

/// Reads `name` (as `viewdemo` was given it) on a thread of its own, about a
/// second for a 40 MB demo, and publishes its plan unless another demo was
/// asked for meanwhile. A lambda that arrives first is treated as nobody's.
fn start_read(name: String) {
    let ticket = LATEST_READ.fetch_add(1, Ordering::AcqRel) + 1;
    let path = crate::streaks::demo_path(&crate::texture_hires::game_dir(), &name);
    let spawned = std::thread::Builder::new()
        .name("ds_spray_lambda".into())
        .spawn(move || {
            crate::lookahead::lower_priority();
            let start = std::time::Instant::now();
            match read_plan(&path, ticket) {
                Ok(found) => {
                    report(&format!(
                        "{}: {}, read in {:.1} s",
                        path.display(),
                        found.summary(),
                        start.elapsed().as_secs_f64()
                    ));
                    let mut plan = PLAN.write().unwrap_or_else(|e| e.into_inner());
                    if LATEST_READ.load(Ordering::Acquire) == ticket {
                        *plan = Some((name, Arc::new(found)));
                    }
                }
                Err(ReadError::Superseded) => {}
                Err(ReadError::NotADemo) => {
                    report(&format!("{} has no demo directory to read", path.display()));
                }
            }
        });
    if let Err(e) = spawned {
        report(&format!("couldn't start the reading thread: {e}"));
    }
}

fn read_plan(path: &Path, ticket: u64) -> Result<Plan, ReadError> {
    let file = std::fs::File::open(path).map_err(|e| {
        report(&format!("can't open {}: {e}", path.display()));
        ReadError::NotADemo
    })?;
    plan::read(file, &|| LATEST_READ.load(Ordering::Acquire) != ticket)
}

/// The plan for the demo playing, if its read has finished.
fn current_plan() -> Option<Arc<Plan>> {
    let name = demo_reload::current_demo()?;
    let plan = PLAN.try_read().ok()?;
    let (demo, found) = plan.as_ref()?;
    (*demo == name).then(|| found.clone())
}

/// What is known of a lambda at `spot` while `playing` plays: the recorder's
/// entity when it is theirs, and how that was decided, for the log.
fn owner(playing: Playing, spot: Vec3) -> (Option<usize>, String) {
    let Playing::Pov { recorder } = playing else {
        return (None, "not a POV demo".to_string());
    };
    let Some(plan) = current_plan() else {
        return (None, "the demo is still being read".to_string());
    };
    let verdict = plan.verdict(recorder, spot);
    let theirs = verdict == Verdict::Recorders;
    (theirs.then_some(recorder), verdict.to_string())
}

/// Does what the table says with a lambda at `pos`. True when that took the
/// place of the engine's shot.
fn replace(pos: *const [f32; 3]) -> bool {
    let Some(table) = spray_hltv::table_in_use() else {
        return false;
    };
    let playing = missing_shots::playing_now();
    // Safety: the stub's `lea` of the position `CL_ParseTEnt` just read.
    let spot = unsafe { *pos };
    let (recorder, why) = owner(playing, spot);
    let who = recorder.and_then(|entity| spray_hltv::player(entity - 1));
    let choice = table.lambda_choice(
        who.as_ref().map(|(name, id)| (name.as_str(), *id)),
        spray_hltv::hltv(),
    );
    let done = match &choice {
        Choice::Blank => true,
        // The fallback isn't anyone's, so it has a slot of its own.
        _ => spray_hltv::shoot(
            recorder.map_or(spray_hltv::UNKNOWN_SLOT, |entity| entity - 1),
            &choice,
            pos,
        ),
    };
    if done {
        HANDLED.fetch_add(1, Ordering::Relaxed);
        report(&format!(
            "({:.1} {:.1} {:.1}) -> {choice} ({why})",
            spot[0], spot[1], spot[2]
        ));
    }
    done
}

/// Called by the stub for every decal `CL_ParseTEnt`'s decal case shoots.
/// Non-zero when it dealt with this one, 0 to leave it to the engine. Only a
/// world decal with the lambda's index is a lambda: `entity` is 0 for a
/// `TE_WORLDDECAL`, and the high-index messages have added 256 to the index.
unsafe extern "C" fn world_decal(index: i32, entity: i32, bsp: i32, pos: *const [f32; 3]) -> u32 {
    if index != LAMBDA_DECAL || entity != 0 || bsp != 0 {
        return 0;
    }
    u32::from(replace(pos))
}

fn report(message: &str) {
    // Safety: `report` only appends to the hook log; any thread may call it.
    unsafe { crate::debug::report(&format!("spray lambda: {message}")) };
}

/// One clause for the sprays' status line.
pub fn status() -> String {
    if INSTALLED.load(Ordering::Relaxed) {
        let build = BUILD_NAME.lock().map(|n| *n).unwrap_or_default();
        format!(
            "lambdas on ({build} hw.dll), {} replaced or removed this session",
            HANDLED.load(Ordering::Relaxed)
        )
    } else if GAVE_UP.load(Ordering::Relaxed) {
        "lambdas stay the game's (see the log)".to_string()
    } else {
        "lambdas not on yet".to_string()
    }
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
    fn each_pattern_opens_on_the_jnp_and_holds_the_stolen_bytes_at_the_site() {
        for build in &BUILDS {
            assert!(
                scan::Pattern::parse(build.pattern).is_ok(),
                "{}",
                build.name
            );
            let pattern = bytes(build.pattern);
            assert_eq!(&pattern[..2], &[Some(0x0f), Some(0x8b)], "{}", build.name);
            assert!(
                pattern[build.exit_rel_at..build.exit_rel_at + 4]
                    .iter()
                    .all(Option::is_none),
                "{}",
                build.name
            );
            let span = &pattern[build.site_at..build.site_at + build.stolen.len()];
            let stolen: Vec<Option<u8>> = build.stolen.iter().map(|&b| Some(b)).collect();
            assert_eq!(span, stolen, "{}", build.name);
            assert!(build.stolen.len() >= 5, "{}: room for a jump", build.name);
        }
    }

    #[test]
    fn the_stub_assembles_to_what_the_comment_claims() {
        #[rustfmt::skip]
        assert_eq!(
            stub(&BUILDS[0], 0x1111_1111, 0x2222_2222, 0x3333_3333),
            vec![
                0x8d, 0x45, 0xb0,                   //  0 lea  eax, [ebp-0x50]
                0x50,                               //  3 push eax
                0xff, 0x75, 0xd4,                   //  4 push [ebp-0x2c]
                0x53,                               //  7 push ebx
                0xff, 0x75, 0xcc,                   //  8 push [ebp-0x34]
                0xb8, 0x11, 0x11, 0x11, 0x11,       // 11 mov  eax, world_decal
                0xff, 0xd0,                         // 16 call eax
                0x83, 0xc4, 0x10,                   // 18 add  esp, 16
                0x85, 0xc0,                         // 21 test eax, eax
                0x75, 0x0c,                         // 23 jnz  .done (37)
                0x8b, 0x55, 0xd4,                   // 25 mov  edx, [ebp-0x2c]
                0x8b, 0x4d, 0xcc,                   // 28 mov  ecx, [ebp-0x34]
                0xff, 0x25, 0x22, 0x22, 0x22, 0x22, // 31 jmp  [RESUME]
                0xff, 0x25, 0x33, 0x33, 0x33, 0x33, // 37 .done: jmp [EXIT]
            ]
        );
        #[rustfmt::skip]
        assert_eq!(
            stub(&BUILDS[1], 0x1111_1111, 0x2222_2222, 0x3333_3333),
            vec![
                0x8d, 0x45, 0xf0,                   //  0 lea  eax, [ebp-0x10]
                0x50,                               //  3 push eax
                0x53,                               //  4 push ebx
                0x56,                               //  5 push esi
                0x57,                               //  6 push edi
                0xb8, 0x11, 0x11, 0x11, 0x11,       //  7 mov  eax, world_decal
                0xff, 0xd0,                         // 12 call eax
                0x83, 0xc4, 0x10,                   // 14 add  esp, 16
                0x85, 0xc0,                         // 17 test eax, eax
                0x75, 0x0b,                         // 19 jnz  .done (32)
                0x53,                               // 21 push ebx
                0x8d, 0x45, 0xf0,                   // 22 lea  eax, [ebp-0x10]
                0x50,                               // 25 push eax
                0xff, 0x25, 0x22, 0x22, 0x22, 0x22, // 26 jmp  [RESUME]
                0xff, 0x25, 0x33, 0x33, 0x33, 0x33, // 32 .done: jmp [EXIT]
            ]
        );
    }

    #[test]
    fn anything_but_a_world_lambda_is_left_to_the_engine() {
        let pos = [0.0f32; 3];
        for (index, entity, bsp) in [
            (LAMBDA_DECAL + 1, 0, 0),
            (LAMBDA_DECAL + 0x100, 0, 0),
            (LAMBDA_DECAL, 7, 0),
            (LAMBDA_DECAL, 0, 1),
        ] {
            assert_eq!(unsafe { world_decal(index, entity, bsp, &pos) }, 0);
        }
    }
}
