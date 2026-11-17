use super::*;

#[test]
fn the_offset_stub_assembles_to_what_the_comment_claims() {
    let code = offset_stub(0x1111_1111, 0x2222_2222, 0x3333_3333);
    assert_eq!(
        code,
        vec![
            0x80, 0x3d, 0x11, 0x11, 0x11, 0x11, 0x00, // cmp byte [active], 0
            0x74, 0x09, // je  over the substitution
            0xa1, 0x22, 0x22, 0x22, 0x22, // mov eax, [value]
            0x89, 0x44, 0x24, 0x04, // mov [esp+4], eax
            0x53, 0x55, 0x56, 0x57, 0x33, 0xff, // the stolen instructions
            0xff, 0x25, 0x33, 0x33, 0x33, 0x33, // jmp dword [resume]
        ]
    );
}

#[test]
fn the_conditional_jump_skips_exactly_the_substitution() {
    // Hand-assembled, so the one thing a typo would silently break is the
    // `je` displacement: too small lands mid-instruction, too large skips
    // an instruction the game needs. Derive it from the bytes themselves.
    let code = offset_stub(0xaaaa_aaaa, 0xbbbb_bbbb, 0xcccc_cccc);
    let je_at = code
        .iter()
        .position(|&b| b == 0x74)
        .expect("a je in the stub");
    let landing = je_at + 2 + code[je_at + 1] as usize;
    assert_eq!(
        &code[landing..landing + Y_STOLEN.len()],
        Y_STOLEN,
        "the je lands somewhere other than the stolen instructions"
    );
}

#[test]
fn the_stolen_span_is_long_enough_to_hold_the_jump_that_replaces_it() {
    assert!(Y_STOLEN.len() >= 5, "a near jump needs five bytes");
    // 0 is a legitimate offset, so it must not be the "never set" sentinel.
    assert_ne!(OFFSET_UNSET, 0);
    assert!(!(MIN_OFFSET..=MAX_OFFSET).contains(&OFFSET_UNSET));
}

#[test]
fn the_restamped_fields_are_ones_the_game_itself_indexes() {
    // FIELD_ID and FIELD_DISPLAY_TIME are written straight into the array,
    // bypassing the code that normally maintains it, so they have to be the
    // offsets client.dll actually uses -- not ones taken from the SDK
    // header and assumed to have survived DoD's build. Every field the game
    // touches shows up in ARRAY_REFS, which is derived from the binary.
    for field in [FIELD_ID, FIELD_DISPLAY_TIME] {
        assert!(
            ARRAY_REFS.iter().any(|&(_, offset)| offset == field),
            "client.dll never indexes +{field:#x}; restamping it would write into a field it does not use"
        );
    }
}

#[test]
fn a_module_is_verified_once_and_a_reloaded_one_again() {
    // Never seen: must verify.
    assert!(needs_verification(0, 0x1000_0000));
    // Already approved: must not re-run, or our own patched bytes read as
    // a mismatch -- which is the bug that sent `max` into "this client.dll
    // is not the build these offsets were derived from" after `offset`
    // had legitimately written a non-stock value.
    assert!(!needs_verification(0x1000_0000, 0x1000_0000));
    // client.dll reloaded elsewhere: stock code again, verify again.
    assert!(needs_verification(0x1000_0000, 0x2000_0000));
}

#[test]
fn count_constants_match_the_shipped_values() {
    // The six operands as they are in the untouched client.dll. If any of
    // these drift, `verify_stock` would start refusing a correct module.
    assert_eq!(CountKind::MemsetDwords.value_for(STOCK_MAX), 0xc3);
    assert_eq!(CountKind::MemmoveBytes.value_for(STOCK_MAX), 0x270);
    assert_eq!(CountKind::Max.value_for(STOCK_MAX), 4);
    assert_eq!(CountKind::MaxMinusOne.value_for(STOCK_MAX), 3);
}

#[test]
fn the_item_stride_and_slot_count_agree_with_the_memset() {
    // InitHUDData clears 0xc3 dwords; that has to be exactly MAX+1 slots,
    // which is the check that proves the 156-byte stride.
    assert_eq!(0xc3 * 4, (STOCK_MAX as usize + 1) * ITEM);
}

#[test]
fn every_count_fits_its_operand_at_the_ceiling() {
    for &(_, width, kind) in COUNT_SITES {
        let value = kind.value_for(MAX_LINES);
        if width == 1 {
            assert!(
                value <= 0x7f,
                "{value} will not fit the imm8 it is written into"
            );
        }
    }
}

#[test]
fn array_references_all_land_inside_one_slot_or_the_spare() {
    // Every recorded field offset must be a real DeathNoticeItem field in
    // slot 0, except the two that deliberately point at slot 1 (the
    // memmove source). Anything else means a transcription slip.
    for &(rva, field) in ARRAY_REFS {
        assert!(
            field < 2 * ITEM,
            "reference at {rva:#x} has field offset {field:#x}"
        );
    }
}

#[test]
fn the_weapon_table_is_the_size_the_bounds_check_implies() {
    // MsgFunc_DeathMsg accepts 1..=43 (`jle` on 0, `jge` on 0x2c).
    assert_eq!(WEAPON_SPRITES.len(), 44);
    assert_eq!(WEAPON_SPRITES[0], "");
    assert_eq!(WEAPON_SPRITES[5], "d_garand");
    assert_eq!(WEAPON_SPRITES[43], "d_enfbayonet");
}

#[test]
fn weapons_resolve_by_name_with_or_without_the_prefix() {
    assert_eq!(parse_weapon("d_garand"), Ok(5));
    assert_eq!(parse_weapon("garand"), Ok(5));
    assert_eq!(parse_weapon("GARAND"), Ok(5));
    assert_eq!(parse_weapon("5"), Ok(5));
    assert!(parse_weapon("0").is_err());
    assert!(parse_weapon("44").is_err());
    assert!(parse_weapon("no_such_gun").is_err());
}

fn slot(slot: i32) -> Who {
    Who::slot(slot)
}

/// The example from #468: STEAM_0:0:7654321.
const ME: u64 = 76_561_197_975_574_370;

#[test]
fn a_block_list_hides_only_the_listed_players() {
    let list = BlockList {
        players: vec![Player::Slot(3), Player::Slot(7)],
        allow_list: false,
    };
    assert!(list.blocks(&slot(3), &slot(9)));
    assert!(list.blocks(&slot(9), &slot(7)));
    assert!(!list.blocks(&slot(1), &slot(2)));
}

#[test]
fn an_allow_list_hides_everything_else() {
    let list = BlockList {
        players: vec![Player::Slot(3)],
        allow_list: true,
    };
    assert!(
        !list.blocks(&slot(3), &slot(9)),
        "a frag involving 3 must still show"
    );
    assert!(
        list.blocks(&slot(1), &slot(2)),
        "a frag with nobody listed must be hidden"
    );
}

#[test]
fn an_empty_list_blocks_nothing_in_either_mode() {
    for allow_list in [false, true] {
        let list = BlockList {
            players: Vec::new(),
            allow_list,
        };
        assert!(!list.blocks(&slot(1), &slot(2)));
    }
}

#[test]
fn a_pasted_classic_steam_id_is_one_negated_player_not_a_mix() {
    let argv = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let me = 76_561_197_962_734_863;
    assert_eq!(
        parse_block_args(&argv(&["!STEAM_0", ":", "1", ":", "1234567"])),
        Ok((vec![Player::SteamId(me)], true))
    );
    assert_eq!(
        parse_block_args(&argv(&[
            "!self", "!STEAM_0", ":", "1", ":", "1234567", "!13"
        ])),
        Ok((
            vec![Player::OwnPov, Player::SteamId(me), Player::Slot(13)],
            true
        ))
    );
    assert_eq!(
        parse_block_args(&argv(&["STEAM_0", ":", "1", ":", "1234567"])),
        Ok((vec![Player::SteamId(me)], false))
    );
    // All three SteamID forms on one line, as the console splits them.
    assert_eq!(
        parse_block_args(&argv(&[
            "!STEAM_0",
            ":",
            "1",
            ":",
            "1234567",
            "!76561197975574370",
            "![U",
            ":",
            "1",
            ":",
            "15308642]",
        ])),
        Ok((
            vec![
                Player::SteamId(me),
                Player::SteamId(76_561_197_975_574_370),
                Player::SteamId(76_561_197_975_574_370),
            ],
            true
        ))
    );
    assert!(
        parse_block_args(&argv(&["!3", "5"]))
            .unwrap_err()
            .contains("mix"),
        "a real mix is still refused"
    );
}

#[test]
fn block_arguments_are_a_slot_a_steam_id_or_self() {
    assert_eq!(parse_player("3"), Some(Player::Slot(3)));
    assert_eq!(parse_player("self"), Some(Player::OwnPov));
    assert_eq!(parse_player("SELF"), Some(Player::OwnPov));
    assert_eq!(parse_player("76561197975574370"), Some(Player::SteamId(ME)));
    assert_eq!(parse_player("STEAM_0:0:7654321"), Some(Player::SteamId(ME)));
    assert_eq!(parse_player("pov"), None);
    assert_eq!(parse_player("12345"), None, "neither a slot nor a SteamID");
    assert_eq!(parse_player(""), None);
}

#[test]
fn a_non_player_id_displays_as_the_raw_number() {
    assert_eq!(
        Player::SteamId(90_071_996_842_377_220).to_string(),
        "90071996842377220"
    );
}

#[test]
fn status_shows_classic_steam_ids_and_current_names() {
    // #537's example: typed as `block !STEAM_0:1:1234567`.
    let m00cat = 76_561_197_962_734_863;
    let list = BlockList {
        players: vec![Player::SteamId(m00cat), Player::Slot(13), Player::OwnPov],
        allow_list: true,
    };
    let names = |p: Player| match p {
        Player::SteamId(id) if id == m00cat => Some("dicE[: :]m00cat :D".to_string()),
        Player::Slot(13) => Some("Brain".to_string()),
        _ => None,
    };
    assert_eq!(
        blocking_text(&list, names),
        "everything except STEAM_0:1:1234567 (dicE[: :]m00cat :D), slot 13 (Brain), self"
    );
    // Nobody by those ids in the loaded demo: no names.
    let block = BlockList {
        allow_list: false,
        ..list
    };
    assert_eq!(
        blocking_text(&block, |_| None),
        "frags involving STEAM_0:1:1234567, slot 13, self"
    );
    assert_eq!(blocking_text(&BlockList::default(), |_| None), "nothing");
    // Round trip: what status prints parses back to the same player.
    assert_eq!(
        parse_player(&Player::SteamId(ME).to_string()),
        Some(Player::SteamId(ME))
    );
}

#[test]
fn a_steam_id_matches_whatever_slot_the_player_has() {
    let list = BlockList {
        players: vec![Player::SteamId(ME)],
        allow_list: true,
    };
    let me_in_slot_5 = Who {
        slot: 5,
        steam_id: ME,
        is_own_pov: false,
    };
    let me_in_slot_12 = Who {
        slot: 12,
        ..me_in_slot_5
    };
    let other = Who {
        slot: 7,
        steam_id: ME + 2,
        is_own_pov: false,
    };
    assert!(!list.blocks(&me_in_slot_5, &other));
    assert!(!list.blocks(&other, &me_in_slot_12));
    assert!(list.blocks(&other, &slot(9)));
}

#[test]
fn an_unknown_steam_id_never_matches() {
    // A slot whose SteamID the engine did not report resolves to 0.
    assert!(!Player::SteamId(0).matches(&slot(4)));
}

#[test]
fn self_and_a_steam_id_cover_both_demo_types() {
    // `block !self !<steamid>`, the one line #468 says must work in both.
    let list = BlockList {
        players: vec![Player::OwnPov, Player::SteamId(ME)],
        allow_list: true,
    };
    // POV demo: the recording player is flagged.
    let pov_me = Who {
        slot: 2,
        steam_id: ME,
        is_own_pov: true,
    };
    assert!(!list.blocks(&pov_me, &slot(8)));
    // HLTV demo: `self` resolves for nobody, and the SteamID finds me.
    let hltv_me = Who {
        slot: 14,
        steam_id: ME,
        is_own_pov: false,
    };
    assert!(!list.blocks(&slot(3), &hltv_me));
    assert!(list.blocks(&slot(3), &slot(8)));
}

#[test]
fn only_slot_lists_skip_the_engine_lookup() {
    let slots = BlockList {
        players: vec![Player::Slot(1)],
        allow_list: false,
    };
    assert!(!slots.needs_player_info());
    for player in [Player::OwnPov, Player::SteamId(ME)] {
        let list = BlockList {
            players: vec![Player::Slot(1), player],
            allow_list: false,
        };
        assert!(list.needs_player_info());
    }
}
