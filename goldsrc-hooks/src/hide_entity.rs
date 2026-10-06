//! `dodstudio_hide_entity` — suppress specific world entities by their model
//! path (e.g. `sprites/mapsprites/flames.spr`, a prop's `.mdl`, a brush
//! entity's `*12`).
//!
//! ## Renamed from `dodstudio_hide_sprite` (#333)
//!
//! `HUD_AddEntity` fires for every entity the engine is about to draw, and the
//! match has only ever been on the model path, so the command was never
//! sprite-only; the old name promised a restriction the code didn't have.
//! `dodstudio_hide_sprite` stays registered as a second name for the same
//! command for a release (`COMMAND_NAMES`), so configs using it keep working.
//!
//! Each entry records whether the engine has drawn anything by that path this
//! session, and the status says so, so a typo stops failing silently.
//!
//! ## Why `dodstudio_hide_hudelement` can't reach this
//!
//! That module (issue #265) patches the classic 2D HUD element list —
//! `CHud::Redraw` walking `CHudBase`-derived elements' vftable slot 3. An
//! `env_sprite` is a completely different code path: server-replicated
//! entity data the map's own BSP entity lump points at, rendered through the
//! engine's normal entity renderer. There is no HUD element vftable to swap.
//! `client.dll` has **zero** references to the string `"caparea"` anywhere in
//! the binary (confirmed by a full binary string search) — it doesn't load
//! or draw this sprite by name at all, unlike `mapsprites/speakerIcon.spr`/
//! `voiceIcon.spr`, which are hardcoded HUD elements and already reachable
//! through `voice.rs`.
//!
//! ## The mechanism: `HUD_AddEntity`
//!
//! Slot 20 of the `cldll_func_t` table `engine.rs` already partially owns
//! (three of its 43 slots were already swapped for `Initialize`, `HUD_Frame`
//! and `HUD_GetStudioModelInterface` — see `docs/goldsrc_client_dll_internals.md`
//! §2). The slot index itself is checked the same rigorous way as the other
//! three, independently of the constant in `engine.rs`: disassemble `F`
//! directly and resolve what it actually writes into slot 20 back to its own
//! export name (`tools/verify_hide_sprite_slot.py`). The engine calls
//! `HUD_AddEntity(int type, cl_entity_t *ent, const
//! char *modelname)` once per entity it is about to add to the render list;
//! the client returns 0 to suppress that one entity, or nonzero to let it
//! through. This is the standard Half-Life SDK contract for that slot, and
//! is not established here by disassembling `hw.dll`'s own caller (a closed,
//! heavily obfuscated binary — see `docs/goldsrc_hw_dll_survey.md`) — it is
//! cross-checked the same way `engine.rs`'s own slot table is, against
//! Xash3D's open-source engine, whose `CL_AddVisibleEntity`-equivalent path
//! explicitly treats a zero return from this slot as "don't add". That is a
//! different, weaker standard of evidence than the byte-level verification
//! this crate holds a *patch* to (nothing here is patched), and is stated as
//! such rather than presented as disassembly-confirmed.
//!
//! ## Design: an allow-list, not a blanket toggle
//!
//! Deliberately not a `dodstudio_hide_map_sprites 1` switch. Most of
//! `sprites/mapsprites/`'s neighbours are decorative and meaningful —
//! smoke, fire, tracers — and indiscriminately suppressing every map-placed
//! sprite would remove things nobody asked to have removed. This command
//! only ever hides model paths named explicitly, the same shape
//! `dodstudio_deathmsg block <id>...` already uses for players.
//!
//! Analysis subject: `dod/cl_dlls/client.dll`, 977,816 bytes, byte-identical
//! across the stock, pre-Anniversary and post-Anniversary installs.
//!
//! ## Live test (2026-09-21)
//!
//! Confirmed against `dod_railroad2_s9a`: `sprites/mapsprites/flames.spr`
//! visibly disappeared and came back across a `clear`/re-set cycle, which
//! confirms `HUD_AddEntity`'s return-value contract (0 = suppress) actually
//! holds in this build, not only in Xash3D's open-source equivalent -- the
//! weaker-evidence caveat above no longer applies to the mechanism itself.
//!
//! The path was found by scanning the map's own BSP entity lump (lump 0, a
//! plain-text sequence of Quake-style `{ "key" "value" ... }` blocks) for
//! `env_sprite` classnames and their `model` key -- a pure offline text
//! parse, no engine involved. Two earlier attempts on the same session
//! (`sprites/mapsprites/caparea.spr`, `sprites/customxhair.spr`) never
//! matched anything, not because of a path typo but because neither is a
//! genuine `env_sprite`: the capture-area icon and the crosshair are both
//! ordinary 2D HUD elements (`CHudDodIcons`/`CHudDoDCrossHair`), which this
//! command was never able to reach in the first place -- see "Why
//! `dodstudio_hide_hudelement` can't reach this" above, which turns out to cut
//! both ways.

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::names::console_name;

/// The new name first (it is the one usage and replies use), then the old one.
pub const COMMAND_NAMES: &[&str] = &[COMMAND, console_name!("hide_sprite")];
const COMMAND: &str = console_name!("hide_entity");

/// One model path to suppress, and whether the engine has drawn anything by
/// that path since it was set.
struct Entry {
    path: String,
    seen: AtomicBool,
}

impl Entry {
    fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            seen: AtomicBool::new(false),
        }
    }
}

/// Model paths to suppress, exactly as typed (case-insensitive compare).
/// Empty by default -- the whole point is that nothing is hidden until
/// asked for by name. An `RwLock`, not a `Mutex`: `should_hide` below reads
/// this once per entity per frame, considerably hotter than the occasional
/// console-command write, and CLAUDE.md's hot-path rule is explicit that a
/// shared catalog like this belongs behind a lock readers don't block each
/// other on. `seen` is an atomic so a match can record itself under the
/// read lock.
static HIDDEN: RwLock<Vec<Entry>> = RwLock::new(Vec::new());

/// Pure matcher, so it can be unit-tested without touching the shared
/// `HIDDEN` static -- see the test module for why nothing here does that.
/// Marks the entry it matched as seen.
fn matches(hidden: &[Entry], model_name: &str) -> bool {
    match hidden
        .iter()
        .find(|h| h.path.eq_ignore_ascii_case(model_name))
    {
        Some(entry) => {
            entry.seen.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// `a (seen), b (not seen yet)`.
fn describe(list: &[Entry]) -> String {
    list.iter()
        .map(|e| {
            let seen = if e.seen.load(Ordering::Relaxed) {
                "seen"
            } else {
                "not seen this session yet -- check the path"
            };
            format!("{} ({seen})", e.path)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Called from `engine::tramp_hud_add_entity` for every entity the engine is
/// about to add to the render list. `true` means suppress it.
///
/// A plain linear scan over what is expected to be a handful of entries, not
/// the 71-message table `msglog.rs` searches -- this runs once per entity
/// per frame, considerably hotter than a console command's own argument
/// parsing, so it stays a `Vec`, not a data structure sized for a table that
/// will never be large.
pub fn should_hide(model_name: &str) -> bool {
    match HIDDEN.read() {
        Ok(list) => matches(&list, model_name),
        Err(_) => false,
    }
}

fn status() -> String {
    match HIDDEN.read() {
        Ok(list) if list.is_empty() => format!("{COMMAND} = hiding nothing\n"),
        Ok(list) => format!("{COMMAND} = hiding {}\n", describe(&list)),
        Err(_) => format!("{COMMAND} = (lock poisoned)\n"),
    }
}

fn usage() -> String {
    format!(
        "usage (like HLAE's mirv_matte_entities, by model path):\n\
         \x20 {COMMAND} list                     what is being hidden (also: no arguments)\n\
         \x20 {COMMAND} add <model-path>...      hide these too\n\
         \x20                                    e.g. {COMMAND} add sprites/mapsprites/flames.spr\n\
         \x20 {COMMAND} del <model-path>...      stop hiding these\n\
         \x20 {COMMAND} clear                    stop hiding anything\n\
         \x20 {COMMAND} <model-path>...          hide exactly these, replacing the list\n\
         \x20 no \"all\" -- deliberately an allow-list, not a blanket toggle; see the module doc\n"
    )
}

/// What `rest` (the arguments after the command) does to `list`, and the
/// reply. Pure, so it is tested without the shared [`HIDDEN`] list.
fn apply(list: &mut Vec<Entry>, rest: &[String]) -> String {
    let (verb, paths) = match rest.split_first() {
        Some((first, more)) => (first.to_ascii_lowercase(), more),
        None => return String::new(),
    };
    let named = |paths: &[String]| -> bool { !paths.is_empty() };
    // "all" anywhere is refused: it would be taken as a model path that
    // matches nothing, and silently hide nothing (found live 2026-10-06).
    if rest.iter().any(|a| a.eq_ignore_ascii_case("all")) {
        return format!(
            "{COMMAND}: no \"all\" -- name the model paths to hide, e.g. {COMMAND} add sprites/mapsprites/flames.spr\n"
        );
    }
    let on_list =
        |list: &[Entry], path: &str| list.iter().any(|e| e.path.eq_ignore_ascii_case(path));
    match verb.as_str() {
        "list" | "show" => String::new(),
        "clear" => {
            list.clear();
            format!("{COMMAND}: hiding nothing\n")
        }
        "add" if named(paths) => {
            let mut added = Vec::new();
            for path in paths {
                if !on_list(list, path) {
                    list.push(Entry::new(path));
                    added.push(path.as_str());
                }
            }
            if added.is_empty() {
                format!("{COMMAND}: already hiding {}\n", paths.join(", "))
            } else {
                format!("{COMMAND}: now also hiding {}\n", added.join(", "))
            }
        }
        "del" | "remove" if named(paths) => {
            let (gone, missing): (Vec<&String>, Vec<&String>) =
                paths.iter().partition(|p| on_list(list, p));
            list.retain(|e| !paths.iter().any(|p| e.path.eq_ignore_ascii_case(p)));
            let mut reply = String::new();
            if !gone.is_empty() {
                let names: Vec<&str> = gone.iter().map(|p| p.as_str()).collect();
                reply.push_str(&format!(
                    "{COMMAND}: no longer hiding {}\n",
                    names.join(", ")
                ));
            }
            if !missing.is_empty() {
                let names: Vec<&str> = missing.iter().map(|p| p.as_str()).collect();
                reply.push_str(&format!("{COMMAND}: wasn't hiding {}\n", names.join(", ")));
            }
            reply
        }
        "add" | "del" | "remove" => format!("{COMMAND}: {verb} needs at least one model path\n"),
        // Anything else is a list of model paths, replacing whatever was
        // hidden before -- the "each call restates the whole set" shape
        // `dodstudio_deathmsg block <id>...` uses, kept for configs that
        // already use it.
        _ => {
            *list = rest.iter().map(|path| Entry::new(path)).collect();
            format!("{COMMAND}: hiding {}\n", rest.join(", "))
        }
    }
}

fn args() -> Vec<String> {
    let Some(engfuncs) = crate::engine::engfuncs() else {
        return Vec::new();
    };
    let argc = unsafe { (engfuncs.cmd_argc)() };
    (0..argc)
        .filter_map(|i| {
            let ptr = unsafe { (engfuncs.cmd_argv)(i) };
            if ptr.is_null() {
                return None;
            }
            Some(
                unsafe { std::ffi::CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect()
}

fn dispatch(argv: &[String]) -> String {
    // argv[0] is the command name itself, so a bare invocation (or a
    // genuinely empty argv, which args() returns when engfuncs isn't
    // resolved yet) is a query -- not `&argv[1..]`, which panics on an empty
    // slice and aborts the whole process under this DLL's release
    // `panic = "abort"` profile.
    let rest = if argv.len() > 1 { &argv[1..] } else { &[] };
    if rest.is_empty() {
        return format!("{}{}", status(), usage());
    }
    let reply = match HIDDEN.write() {
        Ok(mut list) => apply(&mut list, rest),
        Err(_) => return format!("{COMMAND} = (lock poisoned)\n"),
    };
    // `list` (and every change) ends with the list as it is now.
    format!("{reply}{}", status())
}

pub unsafe extern "C" fn command() {
    let argv = args();
    let reply = dispatch(&argv);
    crate::commands::console_print(&reply);
    unsafe {
        crate::debug::report(&format!(
            "hide_entity: {} -> {}",
            argv.join(" "),
            reply.trim()
        ))
    };
}

/// Folded into `dodstudio_debug_status`, gated on being non-empty like
/// `msglog`'s own status line -- off by default, and a permanent "hiding
/// nothing" line would be noise in the overwhelmingly common case.
pub(crate) fn status_line() -> Option<String> {
    match HIDDEN.read() {
        Ok(list) if !list.is_empty() => Some(format!("{COMMAND} = hiding {}", describe(&list))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nothing here touches the shared HIDDEN static: cargo test runs a
    // crate's tests in parallel by default with no isolation between them,
    // and dispatch()'s list-replacing subcommands mutate it -- the same
    // reasoning msglog.rs's test module documents for its own equivalent
    // WANTED/ACTIVE statics. matches() carries the actual comparison logic
    // and is pure, so it's what's tested; only dispatch()'s one
    // state-free path (bare invocation) is exercised directly.

    #[test]
    fn matching_is_case_insensitive() {
        let hidden = vec![Entry::new("sprites/mapsprites/caparea.spr")];
        assert!(matches(&hidden, "sprites/mapsprites/caparea.spr"));
        assert!(matches(&hidden, "SPRITES/MAPSPRITES/CAPAREA.SPR"));
        assert!(!matches(&hidden, "sprites/mapsprites/speakerIcon.spr"));
    }

    #[test]
    fn an_entry_reports_whether_anything_matched_it() {
        let hidden = vec![
            Entry::new("sprites/mapsprites/flames.spr"),
            Entry::new("all"),
        ];
        assert!(describe(&hidden).contains("flames.spr (not seen"));
        matches(&hidden, "sprites/mapsprites/flames.spr");
        let text = describe(&hidden);
        assert!(text.contains("flames.spr (seen)"), "{text}");
        assert!(text.contains("all (not seen"), "{text}");
    }

    #[test]
    fn the_old_name_still_works() {
        assert_eq!(
            COMMAND_NAMES,
            ["dodstudio_hide_entity", "dodstudio_hide_sprite"]
        );
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn paths(list: &[Entry]) -> Vec<String> {
        list.iter().map(|e| e.path.clone()).collect()
    }

    #[test]
    fn add_and_del_change_the_list_like_mirv_matte_entities() {
        let mut list = Vec::new();
        apply(&mut list, &strings(&["add", "a.spr"]));
        apply(&mut list, &strings(&["add", "b.spr", "A.SPR"]));
        assert_eq!(paths(&list), ["a.spr", "b.spr"], "added once, case ignored");
        let reply = apply(&mut list, &strings(&["del", "a.spr", "c.spr"]));
        assert_eq!(paths(&list), ["b.spr"]);
        assert!(
            reply.contains("no longer hiding a.spr") && reply.contains("wasn't hiding c.spr"),
            "{reply}"
        );
        apply(&mut list, &strings(&["clear"]));
        assert!(list.is_empty());
    }

    #[test]
    fn plain_paths_still_replace_the_list() {
        let mut list = vec![Entry::new("old.spr")];
        apply(&mut list, &strings(&["a.spr", "b.spr"]));
        assert_eq!(paths(&list), ["a.spr", "b.spr"]);
    }

    #[test]
    fn all_is_refused_and_changes_nothing() {
        let mut list = vec![Entry::new("flames.spr")];
        for args in [&["all"][..], &["add", "all"][..], &["a.spr", "ALL"][..]] {
            let reply = apply(&mut list, &strings(args));
            assert!(reply.contains("no \"all\""), "{reply}");
            assert_eq!(paths(&list), ["flames.spr"]);
        }
    }

    #[test]
    fn add_or_del_without_a_path_says_so() {
        let mut list = vec![Entry::new("flames.spr")];
        assert!(apply(&mut list, &strings(&["add"])).contains("needs at least one"));
        assert!(apply(&mut list, &strings(&["del"])).contains("needs at least one"));
        assert_eq!(paths(&list), ["flames.spr"]);
        assert_eq!(apply(&mut list, &strings(&["list"])), "");
    }

    #[test]
    fn an_empty_list_hides_nothing() {
        assert!(!matches(&[], "sprites/mapsprites/caparea.spr"));
    }

    #[test]
    fn bare_invocation_is_a_status_query_not_a_mutation() {
        let reply = dispatch(&["hide_entity".to_string()]);
        assert!(reply.contains("usage"), "{reply}");
    }

    #[test]
    fn a_genuinely_empty_argv_is_also_a_status_query() {
        // args() returns Vec::new() whenever engine::engfuncs() isn't
        // resolved yet, not just a one-element argv0-only vec -- `&argv[1..]`
        // panics on that, and this DLL ships with `panic = "abort"`.
        let reply = dispatch(&[]);
        assert!(reply.contains("usage"), "{reply}");
    }
}
