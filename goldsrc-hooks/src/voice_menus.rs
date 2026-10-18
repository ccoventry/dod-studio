//! `dodstudio_hide_voice_menus`: hide the numbered voice-command and
//! hand-signal menus a POV demo's player opened, so they don't pop up over the
//! shot. `dodstudio_mute_voice_commands` silences what the player then picked
//! and `dodstudio_hide_hand_signals` hides the gesture; this hides the list
//! they picked it from.
//!
//! ## Two menus, two mechanisms
//!
//! Read from the shipped `client.dll` (one file, byte-identical on the PRE and
//! POST installs) and the server's `dlls/dod.dll`:
//!
//! - **Voice commands** (`voice_menu1`, `voice_menu2`, `voice_menu3`, the C, V
//!   and B keys by default) are server commands. The server answers each with
//!   a `ShowMenu` user message carrying one of nine keys,
//!   `#Menu_{Amer,Brit,Ger}Voice{1,2,3}` (slots `0x03ff`, 127 s, one chunk).
//!   Those nine are the only `ShowMenu` menus `dod.dll` sends. `client.dll`
//!   hands them to `CHudMenu::MsgFunc_ShowMenu` (`+0x3e3f0`, through the
//!   `+0x3de50` thunk on `gHUD.m_Menu`), which sets `HUD_ACTIVE` in the
//!   element's `m_iFlags` (`+0x10`) and `m_fMenuDisplayed` (`+0x14`), and
//!   `CHudMenu::Draw` (`+0x3dee0`) draws the list. `voice_menuN` itself never
//!   reaches the demo, but the `ShowMenu` does, so the menu replays.
//! - **Hand signals** are the `HAND SIGNALS` submenus of the command menu, the
//!   VGUI2 `CCommandMenu` built from the playing client's own
//!   `dod/commandmenu.txt` and opened by `+commandmenu` (X by default).
//!   `+commandmenu` (`+0x20cf0`) is the scoreboard handler's shape: test
//!   `gViewPort`, `je` past a `jmp [eax+0x74]` (`ShowCommandMenu`). POV demos
//!   record `+commandmenu`/`-commandmenu` as console-command frames, so it
//!   replays too.
//!
//! ## The voice menu: flags, not code
//!
//! `CHud::Redraw` (`+0x36e20`) calls an element's `Draw` only while bit 0 of
//! its `m_iFlags` is set (`test al, 1` at `+0x36f75`), and only
//! `MsgFunc_ShowMenu` ever sets it on `CHudMenu`. So a `ShowMenu` hook (the
//! `clan_text` mechanism: `pfnHookUserMsg` prepends, the game's handler is
//! called through `msglog`'s thunk table) lets the game handle every menu
//! exactly as recorded, then clears that one bit when it was a voice menu.
//!
//! `m_fMenuDisplayed` is left set on purpose. It is what sends the recorded
//! `slot1`..`slot10` presses to the menu rather than to weapon selection, the
//! way they went in the recording; selecting clears both fields
//! (`+0x28880`). The one thing `Draw` does besides drawing is time the menu
//! out: its first branch clears both fields once `m_flShutoffTime`
//! (`+0x1c`) passes `gHUD.m_flTime`, before it draws anything. While the menu
//! is hidden, [`poll`] watches that time and puts the bit back the moment it
//! passes, so the game's own `Draw` closes it without drawing a frame.
//!
//! Server plugin menus (AMX Mod X votes and the like) come through the same
//! `ShowMenu` and are left alone: only the nine voice keys are hidden.
//! `dodstudio_hide_hudelement menu 1` still hides every text menu.
//!
//! ## The command menu: one byte
//!
//! The `je` in `+commandmenu` becomes a `jmp`, so the handler always falls
//! through to its own `ret` -- `scoreboard.rs`'s patch, on its sibling.
//! `-commandmenu` is left working, so a menu already open when this is turned
//! on closes at the recorded key release. It also blocks the command menu in
//! live play while the cvar is on, which is the only way it can work: the
//! game cannot tell a replayed key press from a live one here.
//!
//! ## Turning it on or off mid-menu
//!
//! [`poll`] runs every frame, before `HUD_Redraw`: it hides a voice menu
//! already on screen, and puts back one it hid when the cvar goes to 0.

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("hide_voice_menus");

/// The user message both menus' text comes in.
pub(crate) const SHOW_MENU: &str = "ShowMenu";

/// The nine keys `dod.dll` sends, by their shared starts.
const VOICE_KEYS: [&[u8]; 3] = [b"#Menu_AmerVoice", b"#Menu_BritVoice", b"#Menu_GerVoice"];

/// `ShowMenu`'s header: `short` valid slots, `char` display time, `byte`
/// "more to come"; the text follows.
const HEADER: usize = 4;
const NEED_MORE_AT: usize = 3;

/// `CHudMenu`'s fields, as `MsgFunc_ShowMenu` and `Draw` use them.
const FLAGS: usize = 0x10;
const DISPLAYED: usize = 0x14;
const SHUTOFF: usize = 0x1c;
/// `HUD_ACTIVE`: what `CHud::Redraw` tests before calling `Draw`.
const HUD_ACTIVE: u32 = 1;

/// The `ShowMenu` thunk, up to its `mov ecx, &gHUD.m_Menu`: forward the three
/// arguments, then the instance. Every user-message thunk on a HUD element
/// starts this way, so this checks the one at `msglog`'s `ShowMenu` RVA
/// rather than scanning for it.
const THUNK_START: &[u8] = &[
    0x8B, 0x44, 0x24, 0x0C, 0x8B, 0x4C, 0x24, 0x08, 0x8B, 0x54, 0x24, 0x04, 0x50, 0x51, 0x52, 0xB9,
];

/// `MsgFunc_ShowMenu` setting the shut-off time: `fild [esp+0x18]` (the
/// seconds), `fadd [gHUD.m_flTime]`, `fstp [esi+0x1c]`. Unique in
/// `client.dll`.
const SHUTOFF_PATTERN: &str = "DB 44 24 18 D8 05 ?? ?? ?? ?? D9 5E 1C";
/// Where `gHUD.m_flTime`'s address sits in [`SHUTOFF_PATTERN`].
const HUD_TIME_AT: usize = 6;

/// The whole `+commandmenu` handler. `-commandmenu` is the same bytes up to
/// the vftable displacement (`FF 60 7C`), which is why the pattern runs to
/// `74`.
const COMMAND_MENU_PATTERN: &str = "8B 0D ?? ?? ?? ?? 85 C9 74 05 8B 01 FF 60 74 C3";
/// The `je` within [`COMMAND_MENU_PATTERN`].
const JE_AT: usize = 8;
const JE: u8 = 0x74;
const JMP: u8 = 0xEB;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The cvar as [`poll`] last read it: what the message hook reads.
static WANTED: AtomicBool = AtomicBool::new(false);

/// Whether the menu the game holds now is a voice menu, from the first chunk
/// of the last `ShowMenu`.
static CURRENT_IS_VOICE: AtomicBool = AtomicBool::new(false);
/// Whether the last `ShowMenu` said more text follows, so the next one
/// continues the same menu.
static WAITING_FOR_MORE: AtomicBool = AtomicBool::new(false);
/// Whether this module cleared `HUD_ACTIVE` on the menu now held, so turning
/// the cvar off puts back only what this took away.
static HID_CURRENT: AtomicBool = AtomicBool::new(false);
/// Voice menus kept off screen this session.
static HIDDEN: AtomicU32 = AtomicU32::new(0);

/// `&gHUD.m_Menu`, `&gHUD.m_flTime` and the `je`, with the base they were
/// resolved against.
static MENU: AtomicUsize = AtomicUsize::new(0);
static HUD_TIME: AtomicUsize = AtomicUsize::new(0);
static JE_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static RESOLVED_BASE: AtomicUsize = AtomicUsize::new(0);

/// Whether `+commandmenu` is blocked in the loaded module.
static COMMAND_MENU_BLOCKED: AtomicBool = AtomicBool::new(false);
/// So a failure is reported once, not every frame.
static COMPLAINED: AtomicBool = AtomicBool::new(false);

/// Called by `commands.rs` once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

// ── Pure decisions ───────────────────────────────────────────────────────────

/// Whether a `ShowMenu` payload's text starts with one of the voice keys.
fn is_voice_menu(payload: &[u8]) -> bool {
    payload
        .get(HEADER..)
        .is_some_and(|text| VOICE_KEYS.iter().any(|key| text.starts_with(key)))
}

/// Whether a `ShowMenu` payload says another chunk of the same menu follows.
fn needs_more(payload: &[u8]) -> bool {
    payload.get(NEED_MORE_AT).is_some_and(|&b| b != 0)
}

/// What to do with `CHudMenu`'s flags this frame, or `None` to leave them.
///
/// `hide`: the cvar. `voice`: the held menu is a voice menu. `hid`: this module
/// cleared the bit on it. `timed_out`: its shut-off time has passed, so `Draw`
/// would close it without drawing.
fn wanted_flags(
    flags: u32,
    displayed: bool,
    hide: bool,
    voice: bool,
    hid: bool,
    timed_out: bool,
) -> Option<u32> {
    if !displayed {
        return None;
    }
    let active = flags & HUD_ACTIVE != 0;
    if hide && voice && !timed_out {
        return active.then_some(flags & !HUD_ACTIVE);
    }
    // Showing it again, or letting the game's Draw time it out.
    if hid && !active {
        return Some(flags | HUD_ACTIVE);
    }
    None
}

// ── Engine side ──────────────────────────────────────────────────────────────

/// Resolves `&gHUD.m_Menu`, `&gHUD.m_flTime` and the `+commandmenu` `je` for
/// the loaded `client.dll`.
fn resolve() -> Result<(usize, usize, usize), String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    if RESOLVED_BASE.load(Ordering::Acquire) == base {
        return Ok((
            MENU.load(Ordering::Acquire),
            HUD_TIME.load(Ordering::Acquire),
            JE_ADDRESS.load(Ordering::Acquire),
        ));
    }

    let thunk =
        crate::msglog::original_thunk(SHOW_MENU).ok_or("no ShowMenu thunk on record")? as usize;
    // Safety: the thunk is code inside the mapped module; the bytes read are
    // the ones THUNK_START and the following imm32 cover.
    let start = unsafe { std::slice::from_raw_parts(thunk as *const u8, THUNK_START.len()) };
    if start != THUNK_START {
        return Err(format!(
            "the ShowMenu thunk at +{:#x} starts {start:02x?}, not the expected forward-and-mov-ecx",
            thunk - base
        ));
    }
    let menu =
        unsafe { std::ptr::read_unaligned((thunk + THUNK_START.len()) as *const u32) } as usize;
    let element = crate::hudelement::find("menu")
        .map(|index| &crate::hudelement::ELEMENTS[index])
        .ok_or("hudelement has no menu entry")?;
    let vftable = base + element.vftable_rva;
    if !(base..base + 0x0100_0000).contains(&menu)
        || unsafe { std::ptr::read_unaligned(menu as *const u32) } as usize != vftable
    {
        return Err(format!(
            "the ShowMenu thunk's object {menu:#x} is not a CHudMenu"
        ));
    }
    // Safety: `vftable` is inside the module, so its RTTI pointers are too.
    let class = unsafe { crate::hudelement::rtti_class_name(vftable) };
    if class.as_deref() != Some(element.class) {
        return Err(format!(
            "+{:#x} identifies as {class:?}, not {}",
            element.vftable_rva, element.class
        ));
    }

    // Safety: `client_module_base` only returns a base for a mapped module.
    let at = unsafe { scan::find_unique(base, SHUTOFF_PATTERN) }
        .map_err(|why| format!("could not find ShowMenu's shut-off time -- {why}"))?;
    let hud_time = unsafe { std::ptr::read_unaligned((at + HUD_TIME_AT) as *const u32) } as usize;
    if !(base..base + 0x0100_0000).contains(&hud_time) {
        return Err(format!("gHUD.m_flTime {hud_time:#x} is outside client.dll"));
    }

    let found = unsafe { scan::find_unique(base, COMMAND_MENU_PATTERN) }
        .map_err(|why| format!("could not find the +commandmenu handler -- {why}"))?;
    let je = found + JE_AT;
    let present = unsafe { *(je as *const u8) };
    if present != JE {
        return Err(format!(
            "+{:#x} holds {present:#04x}, not the expected {JE:#04x}",
            je - base
        ));
    }

    MENU.store(menu, Ordering::Release);
    HUD_TIME.store(hud_time, Ordering::Release);
    JE_ADDRESS.store(je, Ordering::Release);
    RESOLVED_BASE.store(base, Ordering::Release);
    Ok((menu, hud_time, je))
}

/// Brings `CHudMenu`'s flags in line with the cvar. Cheap; called from the
/// hook after each `ShowMenu` and every frame.
fn apply_menu(hide: bool) -> Result<(), String> {
    let (menu, hud_time, _) = resolve()?;
    // Safety: a static `CHudMenu` inside client.dll's image, checked by RTTI
    // in `resolve`; the game reads and writes these fields on this thread.
    unsafe {
        let flags = std::ptr::read_volatile((menu + FLAGS) as *const u32);
        let displayed = std::ptr::read_volatile((menu + DISPLAYED) as *const i32) != 0;
        let shutoff = std::ptr::read_volatile((menu + SHUTOFF) as *const f32);
        let now = std::ptr::read_volatile(hud_time as *const f32);
        let timed_out = shutoff > 0.0 && shutoff <= now;
        let hid = HID_CURRENT.load(Ordering::Relaxed);
        let voice = CURRENT_IS_VOICE.load(Ordering::Relaxed);
        if let Some(new) = wanted_flags(flags, displayed, hide, voice, hid, timed_out) {
            std::ptr::write_volatile((menu + FLAGS) as *mut u32, new);
            HID_CURRENT.store(new & HUD_ACTIVE == 0, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// Blocks or unblocks `+commandmenu`, writing only when the byte differs.
fn apply_command_menu(hide: bool) -> Result<(), String> {
    let (_, _, je) = resolve()?;
    let want = if hide { JMP } else { JE };
    let present = unsafe { *(je as *const u8) };
    if present != want {
        if present != JE && present != JMP {
            return Err(format!(
                "the +commandmenu handler holds {present:#04x}, which is neither {JE:#04x} nor {JMP:#04x} -- something else has patched it"
            ));
        }
        if !unsafe { crate::patch::write_code_bytes(je, &[want]) } {
            return Err("could not make the +commandmenu handler writable".to_string());
        }
    }
    COMMAND_MENU_BLOCKED.store(hide, Ordering::Relaxed);
    Ok(())
}

fn complain_once(why: &str) {
    if !COMPLAINED.swap(true, Ordering::Relaxed) {
        crate::commands::console_print(&format!("{NAME}: {why}\n"));
        unsafe { crate::debug::report(&format!("voice_menus: {why}")) };
    }
}

/// Lets the game handle a `ShowMenu`, then hides it if it is a voice menu
/// and the cvar says so. `Some` for `ShowMenu` (always handled here), `None`
/// for any other message. Also called from `msglog`'s handler while it
/// watches `ShowMenu` (see `clan_text`'s module doc).
pub(crate) fn filter(name: &str, size: i32, buf: *mut c_void) -> Option<i32> {
    if !name.eq_ignore_ascii_case(SHOW_MENU) {
        return None;
    }
    let payload: &[u8] = if size > 0 && !buf.is_null() {
        unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) }
    } else {
        &[]
    };
    // A continuation chunk keeps the first chunk's verdict.
    if !WAITING_FOR_MORE.load(Ordering::Relaxed) {
        let voice = is_voice_menu(payload);
        CURRENT_IS_VOICE.store(voice, Ordering::Relaxed);
        HID_CURRENT.store(false, Ordering::Relaxed);
        if voice && WANTED.load(Ordering::Relaxed) {
            HIDDEN.fetch_add(1, Ordering::Relaxed);
        }
    }
    WAITING_FOR_MORE.store(needs_more(payload), Ordering::Relaxed);

    let result = match (
        crate::msglog::original_thunk(SHOW_MENU),
        CString::new(SHOW_MENU),
    ) {
        // Safety: the game's own pfnUserMsgHook, with the engine's buffer.
        (Some(original), Ok(c_name)) => unsafe { original(c_name.as_ptr(), size, buf) },
        _ => 1,
    };
    if let Err(why) = apply_menu(WANTED.load(Ordering::Relaxed)) {
        complain_once(&why);
    }
    Some(result)
}

unsafe extern "C" fn hooked(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    if name.is_null() {
        return 1;
    }
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    if let Some(result) = filter(&name_str, size, buf) {
        return result;
    }
    match crate::msglog::original_thunk(&name_str) {
        Some(original) => unsafe { original(name, size, buf) },
        None => 1,
    }
}

/// Called every frame from `commands::poll`: follows the cvar, keeps the
/// `ShowMenu` hook prepended once a level is loaded (the timing `clan_text`
/// uses, for the same reason), and re-applies both halves.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    if wanted != WANTED.swap(wanted, Ordering::Relaxed) {
        COMPLAINED.store(false, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "voice_menus: {NAME} = {}",
                if wanted {
                    "1 (voice and command menus hidden)"
                } else {
                    "0 (normal)"
                }
            ))
        };
    }
    if engine::client_module_base().is_none() {
        return;
    }
    // Only touch the code once someone has asked: at 0 and never used, the
    // module stays exactly as shipped.
    if (wanted || COMMAND_MENU_BLOCKED.load(Ordering::Relaxed))
        && let Err(why) = apply_command_menu(wanted)
    {
        complain_once(&why);
    }
    if crate::map_text::level_name().is_none() {
        return;
    }
    if !crate::msglog::watching(SHOW_MENU)
        && let (Some(engfuncs), Ok(c_name)) = (engine::engfuncs(), CString::new(SHOW_MENU))
    {
        unsafe { (engfuncs.pfn_hook_user_msg)(c_name.as_ptr(), hooked) };
    }
    if (wanted || HID_CURRENT.load(Ordering::Relaxed))
        && let Err(why) = apply_menu(wanted)
    {
        complain_once(&why);
    }
}

/// A `dodstudio_debug_status` line, or `None` while off and never used.
pub fn status_line() -> Option<String> {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return None;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    let hidden = HIDDEN.load(Ordering::Relaxed);
    if !wanted && hidden == 0 {
        return None;
    }
    Some(format!(
        "{NAME} = {} -- {hidden} voice menu(s) hidden this session; +commandmenu (hand signals) {}",
        if wanted { "1" } else { "0" },
        if COMMAND_MENU_BLOCKED.load(Ordering::Relaxed) {
            "blocked"
        } else {
            "opens normally"
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `ShowMenu` as `dod.dll` sends it: slots, time, more, text.
    fn show_menu(more: u8, text: &str) -> Vec<u8> {
        let mut out = vec![0xff, 0x03, 0x7f, more];
        out.extend(text.as_bytes());
        out.push(0);
        out
    }

    #[test]
    fn every_voice_menu_key_is_a_voice_menu() {
        for team in ["Amer", "Brit", "Ger"] {
            for n in 1..=3 {
                let key = format!("#Menu_{team}Voice{n}");
                assert!(is_voice_menu(&show_menu(0, &key)), "{key}");
            }
        }
    }

    #[test]
    fn other_menus_are_not() {
        for text in [
            "#Menu_Spectate",
            "Choose a map:\n1. dod_anzio",
            "Menu_AmerVoice1",
            "#menu_amervoice1",
            "",
        ] {
            assert!(!is_voice_menu(&show_menu(0, text)), "{text:?}");
        }
        assert!(!is_voice_menu(&[]));
        assert!(!is_voice_menu(&[0xff, 0x03, 0x7f]));
    }

    /// The text starts after the four header bytes, not at the payload's start.
    #[test]
    fn the_header_is_not_part_of_the_text() {
        assert!(!is_voice_menu(b"#Menu_GerVoice2\0"));
        assert!(is_voice_menu(b"\x00\x00\x00\x00#Menu_GerVoice2\0"));
    }

    #[test]
    fn need_more_is_the_fourth_byte() {
        assert!(!needs_more(&show_menu(0, "#Menu_GerVoice2")));
        assert!(needs_more(&show_menu(1, "part one")));
        assert!(!needs_more(&[0xff, 0x03]));
    }

    #[test]
    fn a_voice_menu_loses_only_the_active_bit() {
        assert_eq!(wanted_flags(0x3, true, true, true, false, false), Some(0x2));
        // Already hidden: nothing to write.
        assert_eq!(wanted_flags(0x2, true, true, true, true, false), None);
    }

    #[test]
    fn other_menus_are_left_alone() {
        assert_eq!(wanted_flags(0x1, true, true, false, false, false), None);
        assert_eq!(wanted_flags(0x0, true, true, false, false, false), None);
    }

    #[test]
    fn nothing_is_touched_while_no_menu_is_held() {
        assert_eq!(wanted_flags(0x1, false, true, true, false, false), None);
        assert_eq!(wanted_flags(0x0, false, false, true, true, false), None);
    }

    #[test]
    fn turning_it_off_puts_back_only_what_it_hid() {
        assert_eq!(wanted_flags(0x0, true, false, true, true, false), Some(0x1));
        assert_eq!(wanted_flags(0x0, true, false, true, false, false), None);
    }

    /// Past its shut-off time the game's own Draw has to run to close the
    /// menu, and it does so before drawing anything.
    #[test]
    fn a_timed_out_hidden_menu_is_handed_back_to_draw() {
        assert_eq!(wanted_flags(0x0, true, true, true, true, true), Some(0x1));
        assert_eq!(wanted_flags(0x1, true, true, true, false, true), None);
    }

    #[test]
    fn the_command_menu_pattern_indexes_its_je() {
        let tokens: Vec<&str> = COMMAND_MENU_PATTERN.split_whitespace().collect();
        assert_eq!(tokens[JE_AT], "74");
        assert_eq!(tokens[JE_AT + 1], "05", "the displacement stays as it is");
        // Past the vftable displacement, which is all that tells it from
        // -commandmenu's.
        assert_eq!(&tokens[tokens.len() - 4..], &["FF", "60", "74", "C3"]);
        assert_eq!(JMP, 0xEB);
    }

    #[test]
    fn the_shutoff_pattern_indexes_the_hud_time_operand() {
        let tokens: Vec<&str> = SHUTOFF_PATTERN.split_whitespace().collect();
        assert_eq!(&tokens[HUD_TIME_AT - 2..HUD_TIME_AT], &["D8", "05"]);
        assert!(
            tokens[HUD_TIME_AT..HUD_TIME_AT + 4]
                .iter()
                .all(|t| *t == "??")
        );
        // ...and stores into the shut-off field this module reads.
        assert_eq!(&tokens[tokens.len() - 3..], &["D9", "5E", "1C"]);
        assert_eq!(SHUTOFF, 0x1c);
    }

    #[test]
    fn the_patterns_parse() {
        for pattern in [COMMAND_MENU_PATTERN, SHUTOFF_PATTERN] {
            assert!(scan::Pattern::parse(pattern).is_ok(), "{pattern}");
        }
        assert_eq!(*THUNK_START.last().unwrap(), 0xB9, "mov ecx, imm32");
    }
}
