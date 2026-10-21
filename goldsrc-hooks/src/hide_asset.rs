//! `ds_map_hide_asset` — stop the game drawing an asset (a sprite, model or
//! brush entity, named by its file path). Today: specific world entities by their model
//! path (e.g. `sprites/mapsprites/flames.spr`, a prop's `.mdl`, a brush
//! entity's `*12`), DoD's particles (`particle_hide`), and the HUD's own 2D
//! sprites by sheet path or `hud.txt` icon name (`hud_sprites`, #808).
//!
//! ## Renamed from `dodstudio_hide_sprite` (#333)
//!
//! `HUD_AddEntity` fires for every entity the engine is about to draw, and the
//! match has only ever been on the model path, so the command was never
//! sprite-only; the old name promised a restriction the code didn't have.
//! Then `dodstudio_hide_entity`, and now `ds_map_hide_asset` (the user's
//! pick, 2026-10-06): what is named is a game file, and #614 extends the same
//! list to temporary effects, which aren't world entities to a user.
//! The old names and the old "plain paths replace the list" form are gone,
//! not kept as aliases: neither was ever in a released build (the user's
//! call, 2026-10-06). The command keeps a list like HLAE's
//! `mirv_matte_entities`: `list`, `add`, `del`, `clear`.
//!
//! Each entry records whether the engine has drawn anything by that path this
//! session, and the status says so, so a typo stops failing silently.
//!
//! ## Why `ds_hud_hide_element` can't reach this
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
//! `ds_hud_deathmsg block <id>...` already uses for players.
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
//! `ds_hud_hide_element` can't reach this" above, which turns out to cut
//! both ways.

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::hud_sprites::{HudIcon, Wrect};
use crate::names::console_name;

/// The one name it answers to.
pub const COMMAND_NAMES: &[&str] = &[COMMAND];
const COMMAND: &str = console_name!("map_hide_asset");

/// Which drawing path asked about a model (#614). The same list is checked in
/// each, and the status says where each entry turned up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawnAs {
    /// A world entity: `HUD_AddEntity` with any type but 2.
    Entity,
    /// An engine temporary effect (`R_TempModel`, `R_TempSprite`): the engine
    /// passes it to `HUD_AddEntity` as type 2 (`ET_TEMPENTITY`).
    Effect,
    /// One of DoD's own particles, which never reach `HUD_AddEntity`: see
    /// `particle_hide.rs`.
    Particle,
    /// A sheet the HUD drew in 2D through `SPR_Set`/`SPR_Draw*`, or one
    /// `hud.txt` icon of it: see `hud_sprites.rs` (#808).
    HudSprite,
}

impl DrawnAs {
    fn bit(self) -> u8 {
        match self {
            DrawnAs::Entity => 1,
            DrawnAs::Effect => 2,
            DrawnAs::Particle => 4,
            DrawnAs::HudSprite => 8,
        }
    }

    const ALL: [DrawnAs; 4] = [
        DrawnAs::Entity,
        DrawnAs::Effect,
        DrawnAs::Particle,
        DrawnAs::HudSprite,
    ];

    fn words(self) -> &'static str {
        match self {
            DrawnAs::Entity => "an entity",
            DrawnAs::Effect => "an effect",
            DrawnAs::Particle => "a particle",
            DrawnAs::HudSprite => "a HUD sprite",
        }
    }
}

/// One model path to suppress, and which drawing paths have met it since it
/// was set (a [`DrawnAs::bit`] mask). A `hud.txt` icon name carries the
/// sheet and rectangle it resolved to (#808); its `path` is the name.
struct Entry {
    path: String,
    seen: AtomicU8,
    hud: Option<HudIcon>,
}

impl Entry {
    fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            seen: AtomicU8::new(0),
            hud: None,
        }
    }

    fn icon(icon: HudIcon) -> Self {
        Self {
            path: icon.name.clone(),
            seen: AtomicU8::new(0),
            hud: Some(icon),
        }
    }
}

/// Whether any entry is a `hud.txt` icon: the HUD draw hooks skip the
/// rectangle lookup while none is (#808).
static ANY_HUD_ICON: AtomicBool = AtomicBool::new(false);

pub fn has_hud_icons() -> bool {
    ANY_HUD_ICON.load(Ordering::Relaxed)
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
/// Marks the entry it matched as seen, `as` that kind.
fn matches(hidden: &[Entry], model_name: &str, drawn_as: DrawnAs) -> bool {
    match hidden
        .iter()
        .find(|h| h.hud.is_none() && h.path.eq_ignore_ascii_case(model_name))
    {
        Some(entry) => {
            entry.seen.fetch_or(drawn_as.bit(), Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Whether a rectangle the HUD is drawing is `icon`: the same one, or the
/// icon cut down vertically, as `CHudAmmo` draws the bullets in a clip
/// (`clip_garand_full` and friends) to the ammo left: the same horizontal
/// span, a vertical span inside the icon's. Not any rectangle inside it:
/// `number_0` (0,0 20x25) sits inside `icon_mg_deploy` (0,0 64x64) on the
/// same sheet, and hiding the one must not hide the other.
fn draws_icon(icon: &Wrect, drawn: &Wrect) -> bool {
    drawn.left == icon.left
        && drawn.right == icon.right
        && drawn.top >= icon.top
        && drawn.bottom <= icon.bottom
        && drawn.bottom > drawn.top
}

/// Pure matcher for a `hud.txt` icon: the sheet the HUD picked and the
/// rectangle it is drawing, against the icon entries (#808).
fn matches_icon(hidden: &[Entry], sheet: &str, rect: &Wrect) -> bool {
    match hidden.iter().find(|h| {
        h.hud
            .as_ref()
            .is_some_and(|i| draws_icon(&i.rect, rect) && i.sheet.eq_ignore_ascii_case(sheet))
    }) {
        Some(entry) => {
            entry
                .seen
                .fetch_or(DrawnAs::HudSprite.bit(), Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// `a (seen as an effect), b (not seen yet)`.
fn describe(list: &[Entry]) -> String {
    list.iter()
        .map(|e| {
            let mask = e.seen.load(Ordering::Relaxed);
            let kinds: Vec<&str> = DrawnAs::ALL
                .iter()
                .filter(|k| mask & k.bit() != 0)
                .map(|k| k.words())
                .collect();
            let seen = if kinds.is_empty() {
                "not seen this session yet -- check the path".to_string()
            } else {
                format!("seen as {}", kinds.join(" and "))
            };
            match &e.hud {
                Some(i) => format!(
                    "{} ({} at {},{} {}x{}; {seen})",
                    e.path,
                    i.sheet,
                    i.rect.left,
                    i.rect.top,
                    i.rect.right - i.rect.left,
                    i.rect.bottom - i.rect.top
                ),
                None => format!("{} ({seen})", e.path),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Called from `engine::tramp_hud_add_entity` for every entity the engine is
/// about to add to the render list, and from `particle_hide` for every
/// particle DoD makes. `true` means suppress it.
///
/// A plain linear scan over what is expected to be a handful of entries, not
/// the 71-message table `msglog.rs` searches -- this runs once per entity
/// per frame, considerably hotter than a console command's own argument
/// parsing, so it stays a `Vec`, not a data structure sized for a table that
/// will never be large.
pub fn should_hide(model_name: &str, drawn_as: DrawnAs) -> bool {
    match HIDDEN.read() {
        Ok(list) => matches(&list, model_name, drawn_as),
        Err(_) => false,
    }
}

/// Called from `hud_sprites` for every HUD draw with a rectangle, while any
/// `hud.txt` icon is hidden (#808). `true` means skip the draw.
pub fn should_hide_hud_icon(sheet: &str, rect: &Wrect) -> bool {
    match HIDDEN.read() {
        Ok(list) => matches_icon(&list, sheet, rect),
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
         \x20                                    a HUD sheet: {COMMAND} add sprites/hud_icons.spr\n\
         \x20                                    one HUD icon by its hud.txt name: {COMMAND} add icon_bandage\n\
         \x20 {COMMAND} del <model-path>...      stop hiding these\n\
         \x20 {COMMAND} clear                    stop hiding anything\n\
         \x20 no \"all\" -- deliberately an allow-list, not a blanket toggle; see the module doc\n"
    )
}

/// What `rest` (the arguments after the command) does to `list`, and the
/// reply. Pure, so it is tested without the shared [`HIDDEN`] list;
/// `icon` resolves a `hud.txt` name (no folder, no extension) to its sheet
/// and rectangle, `None` when the file has no such name (#808).
fn apply(list: &mut Vec<Entry>, rest: &[String], icon: &dyn Fn(&str) -> Option<HudIcon>) -> String {
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
            let mut unknown = Vec::new();
            for path in paths {
                if on_list(list, path) {
                    continue;
                }
                if crate::hud_sprites::looks_like_icon_name(path) {
                    match icon(path) {
                        Some(found) => list.push(Entry::icon(found)),
                        None => {
                            unknown.push(path.as_str());
                            continue;
                        }
                    }
                } else {
                    list.push(Entry::new(path));
                }
                added.push(path.as_str());
            }
            let mut reply = String::new();
            if !unknown.is_empty() {
                reply.push_str(&format!(
                    "{COMMAND}: not a model path and not a name in sprites/hud.txt: {}\n",
                    unknown.join(", ")
                ));
            }
            if added.is_empty() && unknown.is_empty() {
                reply.push_str(&format!("{COMMAND}: already hiding {}\n", paths.join(", ")));
            } else if !added.is_empty() {
                reply.push_str(&format!(
                    "{COMMAND}: now also hiding {}\n",
                    added.join(", ")
                ));
            }
            reply
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
        _ => format!(
            "{COMMAND}: \"{verb}\" isn't list, add, del or clear -- e.g. {COMMAND} add {}\n",
            rest[0]
        ),
    }
}

fn dispatch(argv: &[String]) -> String {
    // argv[0] is the command name itself, so a bare invocation (or a
    // genuinely empty argv, which cmd_list::argv() returns when engfuncs isn't
    // resolved yet) is a query -- not `&argv[1..]`, which panics on an empty
    // slice and aborts the whole process under this DLL's release
    // `panic = "abort"` profile.
    let rest = if argv.len() > 1 { &argv[1..] } else { &[] };
    if rest.is_empty() {
        return format!("{}{}", status(), usage());
    }
    let reply = match HIDDEN.write() {
        Ok(mut list) => {
            let reply = apply(&mut list, rest, &crate::hud_sprites::lookup);
            ANY_HUD_ICON.store(list.iter().any(|e| e.hud.is_some()), Ordering::Relaxed);
            reply
        }
        Err(_) => return format!("{COMMAND} = (lock poisoned)\n"),
    };
    // `list` (and every change) ends with the list as it is now, and
    // with what the HUD has drawn, for picking a name (#808).
    format!("{reply}{}{}", status(), crate::hud_sprites::drawn_report())
}

pub unsafe extern "C" fn command() {
    let argv = crate::cmd_list::argv();
    // Particles are made where `HUD_AddEntity` never looks: hook their makers
    // the first time anything is hidden (#614).
    if argv
        .get(1)
        .is_some_and(|verb| verb.eq_ignore_ascii_case("add"))
    {
        crate::particle_hide::install();
        crate::hud_sprites::install();
    }
    let reply = dispatch(&argv);
    crate::commands::console_print(&reply);
    unsafe {
        crate::debug::report(&format!(
            "hide_asset: {} -> {}",
            argv.join(" "),
            reply.trim()
        ))
    };
}

/// Folded into `ds_debug_status`, gated on being non-empty like
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

    /// A game with no `hud.txt` names at all.
    fn no_icons(_: &str) -> Option<HudIcon> {
        None
    }

    fn bandage(name: &str) -> Option<HudIcon> {
        name.eq_ignore_ascii_case("icon_bandage").then(|| HudIcon {
            name: "icon_bandage".to_string(),
            sheet: "sprites/hud_icons.spr".to_string(),
            rect: Wrect {
                left: 65,
                right: 129,
                top: 0,
                bottom: 64,
            },
        })
    }

    #[test]
    fn a_hud_txt_name_resolves_to_its_sheet_and_rectangle() {
        let mut list = Vec::new();
        let reply = apply(&mut list, &strings(&["add", "icon_bandage"]), &bandage);
        assert!(reply.contains("now also hiding icon_bandage"), "{reply}");
        assert_eq!(list.len(), 1);
        let rect = Wrect {
            left: 65,
            right: 129,
            top: 0,
            bottom: 64,
        };
        assert!(matches_icon(&list, "SPRITES/HUD_ICONS.SPR", &rect));
        // The same corner with another size is another icon.
        let other = Wrect {
            left: 65,
            right: 85,
            top: 0,
            bottom: 25,
        };
        assert!(!matches_icon(&list, "sprites/hud_icons.spr", &other));
        assert!(!matches_icon(&list, "sprites/clip_bullets.spr", &rect));
        // Cut down to the ammo left (same width, less height): still the icon.
        let cut = Wrect {
            left: 65,
            right: 129,
            top: 40,
            bottom: 64,
        };
        assert!(matches_icon(&list, "sprites/hud_icons.spr", &cut));
        // A narrower rectangle inside it is another icon, not this one.
        let inside = Wrect {
            left: 65,
            right: 85,
            top: 0,
            bottom: 64,
        };
        assert!(!matches_icon(&list, "sprites/hud_icons.spr", &inside));
        // An icon entry is not a model path: the sheet itself stays drawn.
        assert!(!matches(&list, "sprites/hud_icons.spr", DrawnAs::HudSprite));
        assert!(
            describe(&list).contains(
                "icon_bandage (sprites/hud_icons.spr at 65,0 64x64; seen as a HUD sprite)"
            ),
            "{}",
            describe(&list)
        );
    }

    #[test]
    fn a_name_that_is_not_in_hud_txt_is_refused_and_the_rest_still_added() {
        let mut list = Vec::new();
        let reply = apply(
            &mut list,
            &strings(&["add", "icon_nothing", "sprites/puff.spr"]),
            &bandage,
        );
        assert!(
            reply.contains("not a model path and not a name in sprites/hud.txt: icon_nothing"),
            "{reply}"
        );
        assert!(
            reply.contains("now also hiding sprites/puff.spr"),
            "{reply}"
        );
        assert_eq!(paths(&list), vec!["sprites/puff.spr"]);
    }

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
        assert!(matches(
            &hidden,
            "sprites/mapsprites/caparea.spr",
            DrawnAs::Entity
        ));
        assert!(matches(
            &hidden,
            "SPRITES/MAPSPRITES/CAPAREA.SPR",
            DrawnAs::Entity
        ));
        assert!(!matches(
            &hidden,
            "sprites/mapsprites/speakerIcon.spr",
            DrawnAs::Entity
        ));
    }

    #[test]
    fn an_entry_reports_whether_anything_matched_it() {
        let hidden = vec![
            Entry::new("sprites/mapsprites/flames.spr"),
            Entry::new("all"),
        ];
        assert!(describe(&hidden).contains("flames.spr (not seen"));
        matches(&hidden, "sprites/mapsprites/flames.spr", DrawnAs::Entity);
        let text = describe(&hidden);
        assert!(text.contains("flames.spr (seen as an entity)"), "{text}");
        assert!(text.contains("all (not seen"), "{text}");
    }

    /// #614: the status says which drawing path met each entry.
    #[test]
    fn an_entry_says_how_it_was_drawn() {
        let hidden = vec![
            Entry::new("sprites/effects/adrian/dirt_puff.spr"),
            Entry::new("models/shells.mdl"),
        ];
        matches(
            &hidden,
            "sprites/effects/adrian/dirt_puff.spr",
            DrawnAs::Particle,
        );
        matches(&hidden, "models/shells.mdl", DrawnAs::Effect);
        matches(&hidden, "models/shells.mdl", DrawnAs::Entity);
        let text = describe(&hidden);
        assert!(
            text.contains("dirt_puff.spr (seen as a particle)"),
            "{text}"
        );
        assert!(
            text.contains("shells.mdl (seen as an entity and an effect)"),
            "{text}"
        );
    }

    #[test]
    fn only_the_new_name_is_registered() {
        assert_eq!(COMMAND_NAMES, ["ds_map_hide_asset"]);
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
        apply(&mut list, &strings(&["add", "a.spr"]), &no_icons);
        apply(&mut list, &strings(&["add", "b.spr", "A.SPR"]), &no_icons);
        assert_eq!(paths(&list), ["a.spr", "b.spr"], "added once, case ignored");
        let reply = apply(&mut list, &strings(&["del", "a.spr", "c.spr"]), &no_icons);
        assert_eq!(paths(&list), ["b.spr"]);
        assert!(
            reply.contains("no longer hiding a.spr") && reply.contains("wasn't hiding c.spr"),
            "{reply}"
        );
        apply(&mut list, &strings(&["clear"]), &no_icons);
        assert!(list.is_empty());
    }

    #[test]
    fn a_bare_path_is_not_a_command() {
        let mut list = vec![Entry::new("old.spr")];
        let reply = apply(&mut list, &strings(&["a.spr", "b.spr"]), &no_icons);
        assert!(
            reply.contains("isn't list, add, del or clear") && reply.contains("add a.spr"),
            "{reply}"
        );
        assert_eq!(paths(&list), ["old.spr"], "unchanged");
    }

    #[test]
    fn all_is_refused_and_changes_nothing() {
        let mut list = vec![Entry::new("flames.spr")];
        for args in [
            &["all"][..],
            &["add", "all"][..],
            &["add", "a.spr", "ALL"][..],
        ] {
            let reply = apply(&mut list, &strings(args), &no_icons);
            assert!(reply.contains("no \"all\""), "{reply}");
            assert_eq!(paths(&list), ["flames.spr"]);
        }
    }

    #[test]
    fn add_or_del_without_a_path_says_so() {
        let mut list = vec![Entry::new("flames.spr")];
        assert!(apply(&mut list, &strings(&["add"]), &no_icons).contains("needs at least one"));
        assert!(apply(&mut list, &strings(&["del"]), &no_icons).contains("needs at least one"));
        assert_eq!(paths(&list), ["flames.spr"]);
        assert_eq!(apply(&mut list, &strings(&["list"]), &no_icons), "");
    }

    #[test]
    fn an_empty_list_hides_nothing() {
        assert!(!matches(
            &[],
            "sprites/mapsprites/caparea.spr",
            DrawnAs::Entity
        ));
    }

    #[test]
    fn bare_invocation_is_a_status_query_not_a_mutation() {
        let reply = dispatch(&["hide_asset".to_string()]);
        assert!(reply.contains("usage"), "{reply}");
    }

    #[test]
    fn a_genuinely_empty_argv_is_also_a_status_query() {
        // cmd_list::argv() returns Vec::new() whenever engine::engfuncs() isn't
        // resolved yet, not just a one-element argv0-only vec -- `&argv[1..]`
        // panics on that, and this DLL ships with `panic = "abort"`.
        let reply = dispatch(&[]);
        assert!(reply.contains("usage"), "{reply}");
    }
}
