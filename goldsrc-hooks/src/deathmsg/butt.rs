//! The butt stroke's icon per line under `layout even` (#1088).
//!
//! DoD's `DeathMsg` sends one weapon, 42 (`d_garandbutt`), for both rifles'
//! butt strokes, the Garand's and the K43's, so the game draws one icon for
//! both. `killfeed_icons.py --even` puts four variants in the atlas: the
//! killer's rifle (Garand or K43) by the victim's head (Axis or US),
//! [`VARIANT_NAMES`], all in one box size so any of them fits the line the
//! game laid out for `d_garandbutt` (`killfeed_composites.py`).
//!
//! When a butt-stroke notice arrives, [`note`] works out its variant: the
//! rifle from the model the killer holds (`entity_state_t::weaponmodel`,
//! `p_k43.mdl` or `p_garand.mdl`), else from his team (Axis carry the K43),
//! and the head from the victim's team (`g_PlayerExtraInfo[i].teamnumber`).
//! British victims get the US head until #1089. It keeps that against the
//! line it just went into, keyed by the line's own fields (its expiry time
//! and the victim's name), since `Draw` moves lines up the array as older
//! ones expire. The icon draw then asks [`line_variant`] for the line it is
//! drawing (the line-start stub stores `Draw`'s slot offset, `edi`).
//!
//! The default layout draws from Valve's sheet, whose one rect holds the
//! Garand on an Axis head; only the even layout tells them apart.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// `DeathMsg`'s weapon index for a butt stroke, `d_garandbutt` in
/// [`super::WEAPON_SPRITES`] (`dod::Weapon::ButtStock`).
pub(super) const BUTT_WEAPON: u8 = 42;

/// The killer's rifle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rifle {
    Garand,
    K43,
}

/// The victim's head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Head {
    Axis,
    Us,
}

/// The atlas names of the variants, as `killfeed_composites.BUTT_VARIANTS`
/// writes them (a test there checks the same four). The first is
/// `hud.txt`'s own, the one the game's line is laid out for.
pub(super) const VARIANT_NAMES: [(&str, Rifle, Head); 4] = [
    ("d_garandbutt", Rifle::Garand, Head::Axis),
    ("d_garandbutt_us", Rifle::Garand, Head::Us),
    ("d_k43butt_axis", Rifle::K43, Head::Axis),
    ("d_k43butt_us", Rifle::K43, Head::Us),
];

/// `g_PlayerExtraInfo`'s team numbers.
const ALLIES: i32 = 1;
const AXIS: i32 = 2;

/// The index into [`VARIANT_NAMES`] of a rifle and head.
pub(super) fn variant_index(rifle: Rifle, head: Head) -> usize {
    VARIANT_NAMES
        .iter()
        .position(|&(_, r, h)| r == rifle && h == head)
        .unwrap_or(0)
}

/// The killer's rifle: the K43 when he holds `p_k43.mdl`, the Garand when
/// he holds `p_garand.mdl`, else by his team (`None` for unknown: the
/// Garand, the icon the game draws).
pub(super) fn rifle_for(held_model: Option<&str>, killer_team: Option<i32>) -> Rifle {
    let held = held_model.unwrap_or("").to_ascii_lowercase();
    if held.contains("k43") {
        Rifle::K43
    } else if held.contains("garand") {
        Rifle::Garand
    } else if killer_team == Some(AXIS) {
        Rifle::K43
    } else {
        Rifle::Garand
    }
}

/// The victim's head: US for Allies (British too, until #1089), Axis for
/// Axis and for an unknown team (the icon the game draws).
pub(super) fn head_for(victim_team: Option<i32>) -> Head {
    if victim_team == Some(ALLIES) {
        Head::Us
    } else {
        Head::Axis
    }
}

/// What identifies a line wherever `Draw` has moved it: its expiry time's
/// bits and a 16-bit FNV-1a of the victim's name.
pub(super) fn line_key(display_time_bits: u32, victim: &[u8]) -> u64 {
    let end = victim.iter().position(|&b| b == 0).unwrap_or(victim.len());
    let mut h: u32 = 0x811c_9dc5;
    for &b in &victim[..end] {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    (u64::from(display_time_bits) << 16) | u64::from((h ^ (h >> 16)) & 0xffff)
}

/// A remembered line: its key and the variant, never 0.
fn pack(key: u64, variant: usize) -> u64 {
    (key << 8) | (variant as u64 & 0x7f) | 0x80
}

fn unpack(slot: u64, key: u64) -> Option<usize> {
    (slot != 0 && slot >> 8 == key & 0x00ff_ffff_ffff_ffff).then_some((slot & 0x7f) as usize)
}

/// More than the feed's ceiling of 127 lines would need only if every one
/// were a butt stroke; the oldest is overwritten first.
const REMEMBERED: usize = 32;
static LINES: [AtomicU64; REMEMBERED] = [const { AtomicU64::new(0) }; REMEMBERED];
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Keeps `variant` for the line keyed `key`. Written and read on the
/// engine's thread (message dispatch and the HUD draw); atomics only so no
/// lock is held per frame.
pub(super) fn remember(key: u64, variant: usize) {
    let at = NEXT.fetch_add(1, Ordering::Relaxed) % REMEMBERED;
    LINES[at].store(pack(key, variant), Ordering::Relaxed);
}

/// The variant kept for the line keyed `key`, newest first.
pub(super) fn recall(key: u64) -> Option<usize> {
    let next = NEXT.load(Ordering::Relaxed);
    (1..=REMEMBERED).find_map(|back| {
        let at = (next + REMEMBERED - back % REMEMBERED) % REMEMBERED;
        unpack(LINES[at].load(Ordering::Relaxed), key)
    })
}

#[cfg(target_arch = "x86")]
mod live {
    use super::super::{FIELD_DISPLAY_TIME, FIELD_ID, ITEM, current_array};
    use super::*;
    use crate::engine;

    /// `szVictim` inside a `DeathNoticeItem`.
    const FIELD_VICTIM: usize = 0x40;
    const NAME_LEN: usize = 64;

    /// A player's team from the scoreboard (`g_PlayerExtraInfo`), for
    /// 1..=32 only.
    fn team_of(base: usize, index: i32) -> Option<i32> {
        if !(1..=32).contains(&index) {
            return None;
        }
        let at = base
            + crate::overview_marker::EXTRA_TEAM_RVA
            + crate::overview_marker::EXTRA_STRIDE * index as usize;
        // Safety: a fixed `.data` array of 65 entries in a mapped client.dll.
        Some(i32::from(unsafe { *(at as *const i16) }))
    }

    /// The model the player at `index` holds, if the engine knows it.
    fn held_model(index: i32) -> Option<String> {
        let engfuncs = engine::engfuncs()?;
        let studio = engine::engine_studio()?;
        // Safety: the engine's own accessor, null for an empty slot.
        let entity = unsafe { (engfuncs.get_entity_by_index)(index) };
        if entity.is_null() {
            return None;
        }
        // Safety: a non-null `cl_entity_t`, for this frame, on this thread.
        let model = unsafe { (studio.get_model_by_index)((*entity).curstate.weaponmodel) };
        if model.is_null() {
            return None;
        }
        // Safety: a non-null model from the engine's own lookup.
        Some(unsafe { (*model).name_str() }.into_owned())
    }

    /// The key of the line at `offset` bytes into the live array, if that
    /// is a whole slot inside it holding a notice.
    fn key_at(base: usize, offset: usize) -> Option<u64> {
        let (array, slots) = current_array(base);
        if !offset.is_multiple_of(ITEM) || offset / ITEM >= slots.max(0) as usize {
            return None;
        }
        let item = array + offset;
        // Safety: a slot inside the array the code was patched for (the
        // leaked buffer or client.dll's own `.data`).
        unsafe {
            if std::ptr::read_unaligned((item + FIELD_ID) as *const i32) == 0 {
                return None;
            }
            let time = std::ptr::read_unaligned((item + FIELD_DISPLAY_TIME) as *const u32);
            let victim = std::slice::from_raw_parts((item + FIELD_VICTIM) as *const u8, NAME_LEN);
            Some(line_key(time, victim))
        }
    }

    /// Notes the variant of a butt-stroke notice the game has just added
    /// (the last occupied slot, as `MsgFunc_DeathMsg` fills the first free
    /// one), for [`line_variant`].
    pub(in crate::deathmsg) fn note(killer: i32, victim: i32) {
        let Some(base) = engine::client_module_base() else {
            return;
        };
        let (_, slots) = current_array(base);
        let Some(offset) = (0..slots.max(0) as usize)
            .rev()
            .map(|s| s * ITEM)
            .find(|&o| key_at(base, o).is_some())
        else {
            return;
        };
        let Some(key) = key_at(base, offset) else {
            return;
        };
        let rifle = rifle_for(held_model(killer).as_deref(), team_of(base, killer));
        let head = head_for(team_of(base, victim));
        remember(key, variant_index(rifle, head));
    }

    /// The variant of the line `Draw` is drawing, by the slot offset the
    /// line-start stub stored; `None` for a line not noted (the game's own
    /// icon then).
    pub(in crate::deathmsg) fn line_variant(slot_offset: i32) -> Option<usize> {
        let base = engine::client_module_base()?;
        recall(key_at(base, usize::try_from(slot_offset).ok()?)?)
    }
}

#[cfg(target_arch = "x86")]
pub(super) use live::{line_variant, note};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_rifle_decides_and_the_team_stands_in_for_it() {
        assert_eq!(
            rifle_for(Some("models/p_k43.mdl"), Some(ALLIES)),
            Rifle::K43
        );
        assert_eq!(
            rifle_for(Some("models/p_garand.mdl"), Some(AXIS)),
            Rifle::Garand
        );
        // Out of the client's entity list, or holding something else: his team.
        assert_eq!(rifle_for(None, Some(AXIS)), Rifle::K43);
        assert_eq!(
            rifle_for(Some("models/p_colt.mdl"), Some(ALLIES)),
            Rifle::Garand
        );
        assert_eq!(rifle_for(None, None), Rifle::Garand, "the game's own icon");
    }

    #[test]
    fn the_victims_team_picks_the_head() {
        assert_eq!(head_for(Some(ALLIES)), Head::Us);
        assert_eq!(head_for(Some(AXIS)), Head::Axis);
        assert_eq!(head_for(None), Head::Axis, "the game's own icon");
        assert_eq!(head_for(Some(3)), Head::Axis);
    }

    #[test]
    fn every_pair_has_its_own_variant_and_the_first_is_hud_txts() {
        assert_eq!(VARIANT_NAMES[0].0, "d_garandbutt");
        assert_eq!(
            super::super::WEAPON_SPRITES[BUTT_WEAPON as usize],
            "d_garandbutt"
        );
        let mut seen = std::collections::HashSet::new();
        for rifle in [Rifle::Garand, Rifle::K43] {
            for head in [Head::Axis, Head::Us] {
                let i = variant_index(rifle, head);
                assert_eq!((VARIANT_NAMES[i].1, VARIANT_NAMES[i].2), (rifle, head));
                assert!(seen.insert(i));
            }
        }
        for (name, _, _) in VARIANT_NAMES {
            assert!(name.len() <= super::super::even::SPRITE_NAME_LEN, "{name}");
        }
    }

    #[test]
    fn a_line_is_known_by_its_time_and_victim_and_the_oldest_is_forgotten_first() {
        let mut victim = [0u8; 64];
        victim[..7].copy_from_slice(b"Gorilla");
        let key = line_key(0x4120_0000, &victim);
        assert_eq!(
            key,
            line_key(0x4120_0000, b"Gorilla\0garbage after the end")
        );
        assert_ne!(key, line_key(0x4120_0001, &victim), "another time");
        assert_ne!(key, line_key(0x4120_0000, b"icyHOT"), "another victim");
        remember(key, 3);
        remember(line_key(1, b"someone"), 1);
        assert_eq!(recall(key), Some(3));
        assert_eq!(recall(line_key(2, b"nobody")), None);
        // A later notice for the same line wins.
        remember(key, 2);
        assert_eq!(recall(key), Some(2));

        // The oldest line is forgotten first (one test: the ring is shared).
        let first = line_key(7, b"first");
        remember(first, 1);
        for i in 0..REMEMBERED as u32 {
            remember(line_key(100 + i, b"filler"), 0);
        }
        assert_eq!(recall(first), None);
        assert_eq!(
            recall(line_key(100 + REMEMBERED as u32 - 1, b"filler")),
            Some(0)
        );
    }
}
