use super::*;

/// #415: a new build goes in dod_addon; an install built before it keeps
/// reading dod until dod_addon has its own.
#[test]
fn the_hd_files_are_in_dod_addon_unless_only_dod_has_them() {
    assert_eq!(hd_home(false, false), HdHome::Addon);
    assert_eq!(hd_home(false, true), HdHome::Dod);
    assert_eq!(hd_home(true, false), HdHome::Addon);
    assert_eq!(hd_home(true, true), HdHome::Addon);
}

fn tokens(pattern: &str) -> Vec<String> {
    pattern.split_whitespace().map(str::to_string).collect()
}

fn fixed(pattern: &str, at: usize, expected: &[u8]) {
    let toks = tokens(pattern);
    for (i, &b) in expected.iter().enumerate() {
        assert_eq!(
            u8::from_str_radix(&toks[at + i], 16).ok(),
            Some(b),
            "pattern byte {} should be {b:02X}",
            at + i
        );
    }
}

#[test]
fn patterns_parse() {
    scan::Pattern::parse(LOAD_TEXTURE2_TAIL).unwrap();
    scan::Pattern::parse(UPLOAD32).unwrap();
}

#[test]
fn tail_offsets_land_on_the_instructions_they_name() {
    assert_eq!(tokens(LOAD_TEXTURE2_TAIL).len(), tail::AFTER);
    fixed(LOAD_TEXTURE2_TAIL, tail::BRANCH, BRANCH_STOLEN);
    fixed(LOAD_TEXTURE2_TAIL, tail::RESUME, &[0x83, 0x7D, 0x20, 0x04]);
    fixed(LOAD_TEXTURE2_TAIL, tail::CALL_UPLOAD32, &[0xE8]);
    fixed(LOAD_TEXTURE2_TAIL, tail::UPLOAD16_ARGS, &[0x8B, 0x45, 0x28]);
    fixed(LOAD_TEXTURE2_TAIL, tail::CALL_UPLOAD16, &[0xE8]);
    fixed(LOAD_TEXTURE2_TAIL, tail::AFTER - 3, &[0x83, 0xC4, 0x1C]);
}

#[test]
fn sky_offsets_land_on_what_they_name() {
    scan::Pattern::parse(SKY_LOADER).unwrap();
    scan::Pattern::parse(SKY_UPLOAD).unwrap();
    fixed(SKY_LOADER, SKY_MALLOC_AT, SKY_STOCK_PUSH);
    assert_eq!(tokens(SKY_LOADER).len(), SKY_MALLOC_AT + 5 + 8);
    fixed(SKY_UPLOAD, sky::HOOK_AT, &[0x8B, 0x04, 0x9D]);
    fixed(SKY_UPLOAD, sky::HEIGHT_PUSH, SKY_DIM_PUSH);
    fixed(SKY_UPLOAD, sky::WIDTH_PUSH, SKY_DIM_PUSH);
    // The 4x of a stock face, the same 4 MB as before the ceiling was
    // raised: R_LoadSkys mallocs it on every sky load, unchecked.
    assert_eq!(SKY_MAX_BYTES, 4 * 1024 * 1024);
}

#[test]
fn sky_stub_reproduces_the_stolen_instruction() {
    let stolen = [0x8B, 0x04, 0x9D, 0x60, 0x34, 0x34, 0x02];
    let code = sky_stub(0x56, &stolen);
    assert_eq!(&code[..2], &[0x56, 0x55]);
    assert_eq!(&code[8..11], &[0x83, 0xC4, 0x08]);
    assert_eq!(&code[11..18], &stolen);
    assert_eq!(&code[18..20], &[0xFF, 0x25]);
    assert_eq!(code.len(), 24);
}

#[test]
fn sky_expansion_matches_the_engine() {
    let mut gamma = [0u8; 256];
    for (i, g) in gamma.iter_mut().enumerate() {
        *g = (255 - i) as u8;
    }
    let mut px = vec![3, 0xfc, 0xff, 7];
    match_sky_expansion(&mut px, &gamma, true);
    // 3 -> gamma[3 | 0] = 252; 0xfc and up untouched; alpha opaque.
    assert_eq!(px, vec![252, 0xfc, 0xff, 255]);
    let mut px = vec![3, 4, 5, 7];
    match_sky_expansion(&mut px, &gamma, false);
    assert_eq!(px, vec![3, 4, 5, 255]);
}

#[test]
fn model_skins_are_keyed_by_texture_name() {
    assert_eq!(
        studio_texture_name("models/v_garand.mdlgarand.bmp"),
        "garand.bmp"
    );
    assert_eq!(
        studio_texture_name("models/player/us-inf/us-inf.MDLHead1.bmp"),
        "Head1.bmp"
    );
    assert_eq!(studio_texture_name("no_model_here"), "no_model_here");
    assert_eq!(studio_texture_name("models/x.mdl"), "models/x.mdl");
}

#[test]
fn style_names_cannot_leave_the_hd_folder() {
    assert_eq!(clean_style(" UltraSharp "), Some("ultrasharp".to_string()));
    assert_eq!(
        clean_style("general-v3_2"),
        Some("general-v3_2".to_string())
    );
    assert_eq!(clean_style(".."), None);
    assert_eq!(clean_style("a/b"), None);
    assert_eq!(clean_style("c:"), None);
    assert_eq!(clean_style(""), None);
}

#[test]
fn detail_paths_redirect_only_when_an_hd_copy_exists() {
    let index: HashMap<String, String> = [
        ("1.tga", "dodstudio_hd/detail/ultrasharp/1.tga"),
        (
            "sub/dt_grass1.tga",
            "dodstudio_hd/detail/overrides/sub/dt_grass1.tga",
        ),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    assert_eq!(
        detail_override("gfx/detail/1.tga", &index),
        Some("dodstudio_hd/detail/ultrasharp/1.tga".to_string())
    );
    assert_eq!(
        detail_override("GFX\\Detail\\Sub/DT_Grass1.TGA", &index),
        Some("dodstudio_hd/detail/overrides/sub/dt_grass1.tga".to_string())
    );
    assert_eq!(detail_override("gfx/detail/2.tga", &index), None);
    assert_eq!(detail_override("gfx/env/sky.tga", &index), None);
}

#[test]
fn detail_path_stub_reproduces_the_stolen_lea() {
    fixed(DETAIL_LOADER, DETAIL_PATH_AT, DETAIL_PATH_STOLEN);
    let code = detail_path_stub(DETAIL_PATH_STOLEN);
    assert_eq!(&code[..6], DETAIL_PATH_STOLEN);
    assert_eq!(code[6], 0x50);
    assert_eq!(&code[16..22], DETAIL_PATH_STOLEN);
    assert_eq!(&code[22..24], &[0xFF, 0x25]);
    assert_eq!(code.len(), 28);
}

#[test]
fn detail_offsets_land_on_the_size_pushes() {
    scan::Pattern::parse(DETAIL_LOADER).unwrap();
    fixed(DETAIL_LOADER, detail::ALLOC_SIZE, DETAIL_STOCK_PUSH);
    fixed(DETAIL_LOADER, detail::LOADTGA_SIZE, DETAIL_STOCK_PUSH);
    // Sized for the 4x of a 512 original, not the upload ceiling: the
    // loader mallocs it for every detail texture.
    assert_eq!(DETAIL_MAX_BYTES, 16 * 1024 * 1024);
    const { assert!(DETAIL_MAX_BYTES <= RAISED_MAX_PIXELS * 4) };
}

#[test]
fn the_size_check_is_the_end_of_the_upload32_pattern() {
    let toks = tokens(UPLOAD32);
    assert_eq!(toks.len(), upload32::SIZE_CHECK + SIZE_CHECK_STOLEN.len());
    fixed(UPLOAD32, upload32::SIZE_CHECK, SIZE_CHECK_STOLEN);
    // `jbe +0xd` from just past the span lands on SIZE_OK; the span is
    // followed directly by the Sys_Error push.
    let past = upload32::SIZE_CHECK + SIZE_CHECK_STOLEN.len();
    assert_eq!(past, upload32::TOO_BIG);
    assert_eq!(past + 0x0d, upload32::SIZE_OK);
}

#[test]
fn anniversary_swap_stub_resumes_on_its_own_compare() {
    let code = anniversary::swap_stub();
    assert_eq!(&code[..2], &[0x53, 0x55], "push ebx; push ebp");
    assert_eq!(&code[8..11], &[0x83, 0xC4, 0x08]);
    // jnz at [13] skips the stolen span and the RESUME jump exactly.
    assert_eq!(code[13], 0x75);
    let swap = 15 + code[14] as usize;
    assert_eq!(&code[15..22], anniversary::BRANCH_STOLEN);
    assert_eq!(&code[22..24], &[0xFF, 0x25], "jmp [RESUME]");
    assert_eq!(swap, 28);
    assert_eq!(&code[swap..swap + 3], &[0xFF, 0x75, 0x28]);
    // filter, iType, then the mipmap from PENDING, not the engine's frame (#728).
    assert_eq!(&code[swap + 6..swap + 9], &PUSH_MIPMAP);
    assert_eq!(
        &code[code.len() - 6..code.len() - 4],
        &[0xFF, 0x25],
        "jmp [AFTER]"
    );
}

/// #728: a replaced HUD sprite goes up with mipmaps, everything else with
/// whatever the engine asked for; the stubs read it from `PENDING`'s
/// fourth dword, which is why that slot exists.
#[test]
fn hud_sprite_replacements_are_mipmapped() {
    assert_eq!(replacement_mipmap(GLT_HUDSPRITE, 0), 1);
    assert_eq!(replacement_mipmap(GLT_HUDSPRITE, 1), 1);
    for kind in [GLT_WORLD, GLT_STUDIO, GLT_SPRITE, GLT_DECAL] {
        assert_eq!(replacement_mipmap(kind, 0), 0);
        assert_eq!(replacement_mipmap(kind, 1), 1);
    }
    assert_eq!(PENDING.len(), 4);
    assert_eq!(PUSH_MIPMAP, [0xFF, 0x70, 0x0C], "push dword ptr [eax+0xc]");
    assert_eq!(
        PUSH_MIPMAP.len(),
        3,
        "same length as the old push [ebp+0x1c]"
    );
}

/// #733: crosshair sheets stay stock, by their frame name (lower-cased
/// by `sprite_frame_name`); the other HUD atlases don't.
#[test]
fn crosshair_sheets_are_left_stock() {
    for id in [
        "sprites/crosshairs.spr_0",
        "sprites/customXHair.spr_0",
        "sprites/customxhair-diceWSOD.spr_0",
        "sprites/customogxhair.spr_0",
        "sprites/dodcross1.spr_0",
    ] {
        assert!(is_crosshair_sheet(&sprite_frame_name(id)), "{id}");
    }
    for id in [
        "sprites/weapons1.spr_0",
        "sprites/scope_k43.spr_0",
        "sprites/hud_icons.spr_0",
    ] {
        assert!(!is_crosshair_sheet(&sprite_frame_name(id)), "{id}");
    }
}

/// #728: HUD atlases are sprites to everything but the counters -- same
/// iTypes, same file names, same miss display -- so `sprites_hd.py`'s
/// output for them needs no HUD-specific naming.
#[test]
fn hud_sprites_are_keyed_like_world_sprites() {
    for i_type in [TEX_TYPE_NONE, TEX_TYPE_ALPHA, TEX_TYPE_ALPHA_GRADIENT] {
        assert_eq!(
            replaceable_i_type(GLT_HUDSPRITE, i_type),
            replaceable_i_type(GLT_SPRITE, i_type)
        );
    }
    assert!(!replaceable_i_type(GLT_HUDSPRITE, TEX_TYPE_RGBA));
    assert_eq!(sprite_frame_name("sprites/weapons1.spr_0"), "weapons1_0");
    assert_eq!(sprite_frame_name("sprites/scope_k43.spr_3"), "scope_k43_3");
    assert_eq!(
        miss_display_name(GLT_HUDSPRITE, "sprites/hud_icons.spr_0"),
        "sprites/hud_icons.spr"
    );
    assert_eq!(
        owner_of("hud", "sprites/Weapons1.spr_0"),
        "hud:sprites/weapons1.spr_0"
    );
}

#[test]
fn anniversary_offsets_land_on_the_bytes_they_name() {
    scan::Pattern::parse(anniversary::TAIL).unwrap();
    scan::Pattern::parse(anniversary::HEAD).unwrap();
    fixed(
        anniversary::TAIL,
        anniversary::tail::BRANCH,
        anniversary::BRANCH_STOLEN,
    );
    fixed(
        anniversary::TAIL,
        anniversary::tail::JNE_UPLOAD16,
        &[0x0F, 0x85],
    );
    fixed(anniversary::TAIL, anniversary::tail::CALL_UPLOAD32, &[0xE8]);
    fixed(anniversary::TAIL, anniversary::tail::JMP_AFTER, &[0xE9]);
    fixed(
        anniversary::HEAD,
        anniversary::head::NAME_MATCHED,
        anniversary::NAME_MATCHED_STOLEN,
    );
    fixed(anniversary::HEAD, anniversary::head::SIZE_BRANCH, &[0x75]);
    fixed(anniversary::HEAD, anniversary::head::JE_HIT, &[0x0F, 0x84]);
    // The stolen span runs straight into the jne the stub resumes at.
    assert_eq!(
        anniversary::tail::BRANCH + anniversary::BRANCH_STOLEN.len(),
        anniversary::tail::JNE_UPLOAD16
    );
    assert_eq!(
        anniversary::head::NAME_MATCHED + anniversary::NAME_MATCHED_STOLEN.len(),
        anniversary::head::SIZE_BRANCH
    );
}

#[test]
fn anniversary_ceiling_detail_and_sky_offsets_land_on_what_they_name() {
    let c = anniversary::CEILING;
    // `jbe +0xd` from just past the span lands on SIZE_OK; the Sys_Error
    // push follows the span directly.
    assert_eq!(c.size_check + c.stolen.len(), c.too_big);
    assert_eq!(c.too_big + 0x0d, c.size_ok);
    assert_eq!(c.stolen[5..], [0x76, 0x0D]);
    let stub = (c.stub)();
    assert_eq!(
        stub[..2],
        [0x81, 0xFE],
        "the stub starts at the width check"
    );
    // Same shape as the pre-Anniversary stub: no stolen store to replay,
    // but one more register to restore after a shrink.
    assert_eq!(stub.len(), size_check_stub().len());

    let d = anniversary::DETAIL;
    scan::Pattern::parse(d.pattern).unwrap();
    for off in d.sizes {
        fixed(d.pattern, off, DETAIL_STOCK_PUSH);
    }
    fixed(d.pattern, d.path_at, d.path_stolen);
    // Then `push edi; push eax; call LoadTGA`.
    fixed(
        d.pattern,
        d.path_at + d.path_stolen.len(),
        &[0x57, 0x50, 0xE8],
    );

    let s = anniversary::SKY;
    scan::Pattern::parse(s.loader).unwrap();
    scan::Pattern::parse(s.upload).unwrap();
    fixed(s.loader, s.malloc_at, SKY_STOCK_PUSH);
    assert_eq!(tokens(s.loader).len(), s.malloc_at + 5 + 8);
    fixed(s.upload, s.hook_at, s.hook_opcode);
    fixed(s.upload, s.height_push, SKY_DIM_PUSH);
    fixed(s.upload, s.width_push, SKY_DIM_PUSH);
    let code = sky_stub(s.push_buffer, &[0x8B, 0x04, 0xB5, 0x2C, 0x6A, 0x9A, 0x10]);
    assert_eq!(code[..2], [0x53, 0x55], "push ebx; push ebp");
}

#[test]
fn anniversary_cache_stub_pushes_esi() {
    let code = anniversary::cache_stub();
    assert_eq!(&code[..2], &[0x56, 0x55], "push esi; push ebp");
    let stolen = code.len() - 6 - anniversary::NAME_MATCHED_STOLEN.len();
    assert_eq!(
        &code[stolen..stolen + anniversary::NAME_MATCHED_STOLEN.len()],
        anniversary::NAME_MATCHED_STOLEN
    );
}

#[test]
fn swap_stub_branches_land_where_the_comments_say() {
    let code = swap_stub();
    assert_eq!(&code[..2], &[0x56, 0x55]);
    assert_eq!(&code[8..11], &[0x83, 0xC4, 0x08]);
    // jnz at [13]: target = 15 + 0x12 = the first push.
    assert_eq!(&code[13..15], &[0x75, 0x12]);
    assert_eq!(&code[15 + 0x12..15 + 0x12 + 3], &[0xFF, 0x75, 0x28]);
    // filter, iType, then the mipmap from PENDING, not the engine's frame (#728).
    assert_eq!(&code[15 + 0x12 + 6..15 + 0x12 + 9], &PUSH_MIPMAP);
    // The stolen compare, then a jne +6 that skips exactly the RESUME jump.
    assert_eq!(&code[15..19], &BRANCH_STOLEN[..4]);
    assert_eq!(&code[19..21], &[0x75, 0x06]);
    assert_eq!(&code[21..23], &[0xFF, 0x25]);
    assert_eq!(&code[27..29], &[0xFF, 0x25]);
    assert_eq!(code.len(), 33 + 17 + 6 + 3 + 6);
}

/// Walks the size-check stub as the CPU would for one texture, and says
/// where it ends up: `Ok((scaled_w, scaled_h, product))` past the check,
/// or `Err(())` at the Sys_Error jump. A tiny interpreter for exactly the
/// instructions `size_check_code` emits.
fn run_size_check(
    regs: &SizeRegs,
    sw: u32,
    sh: u32,
    ow: u32,
    oh: u32,
) -> Result<(u32, u32, u32), ()> {
    let code = size_check_code(regs);
    let (mut esi, mut scaled_h, mut eax) = (sw, sh, sw.wrapping_mul(sh));
    let mut pc = regs.prologue.len();
    let imm = |at: usize| u32::from_le_bytes(code[at..at + 4].try_into().unwrap());
    let mut flags_below_eq = false;
    let mut flags_eq = false;
    loop {
        match code[pc] {
            0x81 if code[pc + 1] == 0xFE => {
                (flags_below_eq, flags_eq) = (esi <= imm(pc + 2), esi == imm(pc + 2));
                pc += 6;
            }
            0x81 => {
                assert_eq!(code[pc + 1], 0xF8 | regs.scaled_h as u8);
                (flags_below_eq, flags_eq) = (scaled_h <= imm(pc + 2), scaled_h == imm(pc + 2));
                pc += 6;
            }
            0x3B if code[pc + 1] & 0xC0 == 0xC0 => {
                assert_eq!(code[pc + 1], 0xC0 | (6 << 3) | regs.orig_w as u8);
                (flags_below_eq, flags_eq) = (esi <= ow, esi == ow);
                pc += 2;
            }
            0x3B => {
                assert_eq!(
                    &code[pc + 1..pc + 3],
                    &[0x40 | ((regs.scaled_h as u8) << 3) | 5, 0x10]
                );
                (flags_below_eq, flags_eq) = (scaled_h <= oh, scaled_h == oh);
                pc += 3;
            }
            0x76 => {
                pc += 2 + if flags_below_eq {
                    code[pc + 1] as usize
                } else {
                    0
                }
            }
            0x75 => pc += 2 + if !flags_eq { code[pc + 1] as usize } else { 0 },
            0x74 => pc += 2 + if flags_eq { code[pc + 1] as usize } else { 0 },
            0x77 => {
                if !flags_below_eq {
                    return Err(());
                }
                pc += 2;
            }
            0xBE => {
                esi = imm(pc + 1);
                pc += 5;
            }
            op if op == 0xB8 | regs.scaled_h as u8 => {
                scaled_h = imm(pc + 1);
                pc += 5;
            }
            0x8B => {
                assert_eq!(code[pc + 1], 0xC6);
                eax = esi;
                pc += 2;
            }
            0x0F => {
                assert_eq!(&code[pc + 1..pc + 3], &[0xAF, 0xC0 | regs.scaled_h as u8]);
                eax = eax.wrapping_mul(scaled_h);
                pc += 3;
            }
            0x89 => {
                // A store of eax (the product) or esi (the width) to a
                // frame slot; the slot is the build's own business.
                assert!(matches!(code[pc + 1], 0x45 | 0x75));
                pc += 3;
            }
            0x3D => {
                (flags_below_eq, flags_eq) = (eax <= imm(pc + 1), eax == imm(pc + 1));
                pc += 5;
            }
            0xFF => {
                assert_eq!(code[pc + 1], 0x25);
                // The first indirect jump is SIZE_OK: reached only past the check.
                assert_eq!(pc, code.len() - 12);
                return Ok((esi, scaled_h, eax));
            }
            op => panic!("unexpected opcode {op:#x} at {pc}"),
        }
    }
}

#[test]
fn the_size_check_keeps_wide_textures_that_need_no_resample() {
    for regs in [&PRE_SIZE_REGS, &anniversary::SIZE_REGS] {
        // Under the resample bound: through, whatever the original size.
        assert_eq!(
            run_size_check(regs, 1024, 1024, 4096, 4096),
            Ok((1024, 1024, 1024 * 1024))
        );
        assert_eq!(
            run_size_check(regs, 256, 256, 300, 200),
            Ok((256, 256, 256 * 256))
        );
        // Wider, at its own size: through, up to the ceiling.
        assert_eq!(
            run_size_check(regs, 2048, 2048, 2048, 2048),
            Ok((2048, 2048, 2048 * 2048))
        );
        assert_eq!(
            run_size_check(regs, 4096, 4096, 4096, 4096),
            Ok((4096, 4096, 4096 * 4096))
        );
        assert_eq!(
            run_size_check(regs, 2048, 256, 2048, 256),
            Ok((2048, 256, 2048 * 256))
        );
        // Wider, but rounded or clamped: resampled to 1024 wide instead.
        assert_eq!(
            run_size_check(regs, 2048, 2048, 4096, 4096),
            Ok((1024, 1024, 1024 * 1024))
        );
        assert_eq!(
            run_size_check(regs, 2048, 512, 2048, 500),
            Ok((1024, 512, 1024 * 512))
        );
        assert_eq!(
            run_size_check(regs, 2048, 1024, 2000, 1024),
            Ok((1024, 1024, 1024 * 1024))
        );
        // Over the buffer: the engine's own error, as before.
        assert_eq!(run_size_check(regs, 8192, 8192, 8192, 8192), Err(()));
        assert_eq!(
            run_size_check(regs, 1024, 8192, 1024, 8192),
            Ok((1024, 8192, 1024 * 8192))
        );
        assert_eq!(run_size_check(regs, 4096, 8192, 4096, 8192), Err(()));
    }
}

#[test]
fn size_check_stub_branches_land_where_the_comments_say() {
    let code = size_check_stub();
    let ann = anniversary::size_check_stub();
    assert_eq!(&code[..3], PRE_SIZE_REGS.prologue);
    // cmp esi, 1024; jbe check; cmp esi, ebx; jne shrink; cmp edi, [ebp+0x10]; je check
    assert_eq!(
        &code[3..20],
        &[
            0x81, 0xFE, 0, 4, 0, 0, 0x76, 0x23, 0x3B, 0xF3, 0x75, 0x05, 0x3B, 0x7D, 0x10, 0x74,
            0x1A
        ]
    );
    // shrink: mov esi, 1024; cmp edi, 1024; jbe +5; mov edi, 1024; mov eax, esi; imul eax, edi; mov [ebp-0xc], eax
    assert_eq!(
        &code[20..46],
        &[
            0xBE, 0, 4, 0, 0, 0x81, 0xFF, 0, 4, 0, 0, 0x76, 0x05, 0xBF, 0, 4, 0, 0, 0x8B, 0xC6,
            0x0F, 0xAF, 0xC7, 0x89, 0x45, 0xF4
        ]
    );
    // check: cmp eax, 4096*4096; ja too_big; jmp [SIZE_OK]; too_big: jmp [TOO_BIG]
    assert_eq!(&code[46..53], &[0x3D, 0, 0, 0, 1, 0x77, 0x06]);
    assert_eq!(&code[53..55], &[0xFF, 0x25]);
    assert_eq!(&code[59..61], &[0xFF, 0x25]);
    assert_eq!(code.len(), 65);
    // Anniversary: no prologue; ebx is the rounded height and edi the
    // original width; the shrink stores the width for the mipmap loop
    // and the product where its check found it.
    assert_eq!(
        &ann[..17],
        &[
            0x81, 0xFE, 0, 4, 0, 0, 0x76, 0x26, 0x3B, 0xF7, 0x75, 0x05, 0x3B, 0x5D, 0x10, 0x74,
            0x1D
        ]
    );
    assert_eq!(
        &ann[17..46],
        &[
            0xBE, 0, 4, 0, 0, 0x81, 0xFB, 0, 4, 0, 0, 0x76, 0x05, 0xBB, 0, 4, 0, 0, 0x8B, 0xC6,
            0x0F, 0xAF, 0xC3, 0x89, 0x75, 0xFC, 0x89, 0x45, 0xF0
        ]
    );
    assert_eq!(&ann[46..], &code[46..]);
    assert_eq!(ann.len(), 65);
}

#[test]
fn the_replacement_cap_follows_gl_max_size_as_the_engine_bounds_it() {
    assert_eq!(pot_floor(256.0), 256);
    assert_eq!(pot_floor(1024.0), 1024);
    assert_eq!(pot_floor(1500.0), 1024);
    assert_eq!(pot_floor(2048.0), 2048);
    assert_eq!(pot_floor(4096.0), 4096);
    assert_eq!(pot_floor(16384.0), MAX_SIDE);
    assert_eq!(pot_floor(0.0), MIN_GL_MAX_SIZE);
    assert_eq!(pot_floor(-1.0), MIN_GL_MAX_SIZE);
    assert_eq!(pot_floor(f32::NAN), MIN_GL_MAX_SIZE);
    assert_eq!(pot_floor(200.0), 128);
}

#[test]
fn a_gl_max_size_that_shrinks_hd_files_is_warned_about() {
    let line = gl_max_size_warning(256.0).unwrap();
    assert_eq!(
        line,
        "HD textures are being shrunk to 256 px: gl_max_size is 256. Set gl_max_size 1024 (or 2048) in movie.cfg."
    );
    assert!(gl_max_size_warning(1000.0).unwrap().contains("512 px"));
    assert_eq!(gl_max_size_warning(1024.0), None);
    assert_eq!(gl_max_size_warning(2048.0), None);
}

#[test]
fn fnv_matches_reference_vectors() {
    assert_eq!(fnv1a32(&[b""]), 0x811c_9dc5);
    assert_eq!(fnv1a32(&[b"a"]), 0xe40c_292c);
    assert_eq!(fnv1a32(&[b"foo", b"bar"]), fnv1a32(&[b"foobar"]));
}

#[test]
fn file_names_round_trip() {
    assert_eq!(file_stem_name("{Cancello"), "{cancello");
    assert_eq!(file_stem_name("a*b?c"), "a_b_c");
    assert_eq!(
        parse_file_name("+0lambda_1a2b3c4d.tga"),
        Some(("+0lambda".to_string(), 0x1a2b_3c4d))
    );
    assert_eq!(
        parse_file_name("under_score_DEADBEEF.TGA"),
        Some(("under_score".to_string(), 0xdead_beef))
    );
    assert_eq!(parse_file_name("nohash.tga"), None);
    // The key a miss message and ds_debug_texture print is the file's stem.
    assert_eq!(hd_key("+0Lambda", 0x1a2b_3c4d), "+0lambda_1a2b3c4d");
    assert_eq!(
        parse_file_name(&format!("{}.tga", hd_key("XCONCRETE_C54_Y", 7))),
        Some(("xconcrete_c54_y".to_string(), 7))
    );
    assert_eq!(parse_file_name("x_123.tga"), None);
    assert_eq!(parse_file_name("x_1a2b3c4d.png"), None);
}

fn tga(image_type: u8, bpp: u8, top_first: bool, w: u16, h: u16, body: &[u8]) -> Vec<u8> {
    let mut v = vec![0, 0, image_type, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    v.extend_from_slice(&w.to_le_bytes());
    v.extend_from_slice(&h.to_le_bytes());
    v.push(bpp);
    v.push(if top_first { 0x20 } else { 0 });
    v.extend_from_slice(body);
    v
}

#[test]
fn decodes_uncompressed_bottom_up_24bit() {
    // 1x2, bottom row first: bottom = blue, top = red (stored BGR).
    let img = decode_tga(&tga(2, 24, false, 1, 2, &[255, 0, 0, 0, 0, 255])).unwrap();
    assert_eq!((img.width, img.height), (1, 2));
    assert_eq!(img.pixels, vec![255, 0, 0, 255, 0, 0, 255, 255]);
}

#[test]
fn decodes_rle_32bit() {
    // One run packet of 3 green, then a raw packet of 1 half-alpha red.
    let body = [0x82, 0, 255, 0, 255, 0x00, 0, 0, 255, 128];
    let img = decode_tga(&tga(10, 32, true, 4, 1, &body)).unwrap();
    assert_eq!(
        img.pixels,
        vec![
            0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 128
        ]
    );
}

#[test]
fn refuses_what_it_does_not_understand() {
    assert!(decode_tga(&tga(1, 8, true, 1, 1, &[0])).is_err());
    assert!(decode_tga(&tga(2, 16, true, 1, 1, &[0, 0])).is_err());
    assert!(decode_tga(&tga(2, 24, true, 2, 2, &[0; 5])).is_err());
    assert!(decode_tga(&tga(10, 24, true, 1, 1, &[0x85, 1, 2, 3])).is_err());
}

#[test]
fn halving_averages_and_clamps() {
    let img = Rgba {
        width: 3,
        height: 1,
        pixels: vec![0, 0, 0, 0, 200, 200, 200, 200, 9, 9, 9, 9],
    };
    let half = halve(&img);
    assert_eq!((half.width, half.height), (1, 1));
    assert_eq!(half.pixels, vec![100; 4]);
}

#[test]
fn expansion_matches_the_engine() {
    let mut gamma = [0u8; 256];
    for (i, g) in gamma.iter_mut().enumerate() {
        *g = (255 - i) as u8;
    }
    // Opaque with dither: gamma then c | c >> 6; alpha forced to 255.
    let mut px = vec![0, 3, 255, 7];
    match_engine_expansion(&mut px, &gamma, TEX_TYPE_NONE, true, [0; 3]);
    assert_eq!(px, vec![255, 252 | 3, 0, 255]);
    // Masked: below half alpha becomes all-zero, above becomes opaque.
    let mut px = vec![10, 10, 10, 100, 10, 10, 10, 200];
    match_engine_expansion(&mut px, &gamma, TEX_TYPE_ALPHA, true, [0; 3]);
    assert_eq!(px, vec![0, 0, 0, 0, 245, 245, 245, 255]);
    // Index alpha: the palette-255 tint through gamma, alpha kept as is,
    // whatever RGB the file had; no dither.
    let mut px = vec![9, 9, 9, 0, 1, 2, 3, 77];
    match_engine_expansion(&mut px, &gamma, TEX_TYPE_ALPHA_GRADIENT, true, [0, 3, 255]);
    assert_eq!(px, vec![255, 252, 0, 0, 255, 252, 0, 77]);
}

#[test]
fn misses_show_readable_names() {
    assert_eq!(
        miss_display_name(GLT_STUDIO, "models/v_garand.mdlgarand.bmp"),
        "models/v_garand.mdl garand.bmp"
    );
    assert_eq!(
        miss_display_name(GLT_SPRITE, "sprites/shot_smoke1.spr_203"),
        "sprites/shot_smoke1.spr"
    );
    assert_eq!(miss_display_name(GLT_WORLD, "{Fence7"), "{Fence7");
}

#[test]
fn misses_are_listed_under_every_map_that_uses_them() {
    // The list is process-wide; this test is the only one that writes it.
    add_map_uses(
        "dod_zalec",
        [
            "world:clip".to_string(),
            "model:models/m.mdl".to_string(),
            "sprite:sprites/a.spr".to_string(),
        ],
    );
    record_miss_on("dod_zalec", Miss::OnPurpose, "world", "CLIP", || {
        "tool".into()
    });
    record_miss_on(
        "dod_zalec",
        Miss::WrongVersion,
        "model",
        "models/m.mdl g.bmp",
        || "x".into(),
    );
    record_miss_on("dod_zalec", Miss::NoFile, "sprite", "sprites/a.spr", || {
        "y".into()
    });
    record_miss_on("dod_zalec", Miss::NoFile, "sprite", "sprites/a.spr", || {
        unreachable!("a repeat keeps the first detail")
    });
    // The second map shares the sprite and the tool texture, which the
    // game never loads again -- only its own wall reaches the hook.
    add_map_uses(
        "dod_anzio",
        [
            "world:wall".to_string(),
            "world:clip".to_string(),
            "sprite:sprites/a.spr".to_string(),
        ],
    );
    record_miss_on("dod_anzio", Miss::NoFile, "world", "wall", || "z".into());
    record_miss_on(
        NO_MAP,
        Miss::NoFile,
        "sprite",
        "sprites/startup.spr",
        || "s".into(),
    );

    let report = misses_report(None).concat();
    let at = |s: &str| {
        report
            .find(s)
            .unwrap_or_else(|| panic!("{s:?} in {report}"))
    };
    // Maps in load order, each with its real total; NO_MAP last.
    assert!(
        at("== dod_zalec: 3 (1 on purpose, 2 shared")
            < at("== dod_anzio: 3 (1 on purpose, 2 shared")
    );
    assert!(at("== dod_anzio") < at(&format!("== {NO_MAP}: 1")));
    assert!(report.contains("sprites/a.spr  y (2 frame loads) [also on dod_anzio]"));
    assert!(report.contains("sprites/a.spr  y (2 frame loads) [also on dod_zalec]"));
    assert!(report.contains("7 miss(es) this session, 5 different texture(s)"));
    // Within a map, worst first.
    let zalec = &report[at("== dod_zalec")..at("== dod_anzio")];
    let wrong = zalec.find(Miss::WrongVersion.heading()).unwrap();
    let none = zalec.find(Miss::NoFile.heading()).unwrap();
    let fine = zalec.find(Miss::OnPurpose.heading()).unwrap();
    assert!(wrong < none && none < fine);

    let anzio = misses_report(Some("DOD_ANZIO")).concat();
    assert!(anzio.contains("3 miss(es) on DOD_ANZIO (style"));
    assert!(!anzio.contains("== dod_zalec"));
    assert!(
        misses_report(Some("dod_caen"))
            .concat()
            .contains("was replaced")
    );
}

#[test]
fn owners_match_what_maps_record() {
    assert_eq!(
        owner_of("model", "models/V_Garand.mdl garand.bmp"),
        "model:models/v_garand.mdl"
    );
    assert_eq!(owner_of("sprite", "sprites/A.spr"), "sprite:sprites/a.spr");
    assert_eq!(
        owner_of("detail", "GFX\\Detail\\X.tga"),
        "detail:gfx/detail/x.tga"
    );
    assert_eq!(
        detail_file_uses("{fence detail/dt_fence 8 8\n// x y\n\nwall detail\\d2 4 4"),
        ["detail:gfx/detail/dt_fence.tga", "detail:gfx/detail/d2.tga"]
    );
}

#[test]
fn bsp_texture_names_and_sky_are_read() {
    // A minimal BSP: header (version + 15 lumps), an entity lump and a
    // texture lump holding two miptex names.
    let entities = b"{\n\"classname\" \"worldspawn\"\n\"skyname\" \"Kraftstoff\"\n}\n\0";
    let mut textures = Vec::new();
    textures.extend_from_slice(&2i32.to_le_bytes());
    textures.extend_from_slice(&12i32.to_le_bytes());
    textures.extend_from_slice(&52i32.to_le_bytes());
    for name in [&b"{Fence"[..], &b"WALL01"[..]] {
        let mut miptex = [0u8; 40];
        miptex[..name.len()].copy_from_slice(name);
        textures.extend_from_slice(&miptex);
    }
    let header = 4 + 15 * 8;
    let mut bsp = vec![0u8; header];
    bsp[..4].copy_from_slice(&30i32.to_le_bytes());
    let put = |bsp: &mut Vec<u8>, lump: usize, data: &[u8]| {
        let off = bsp.len();
        bsp[4 + lump * 8..8 + lump * 8].copy_from_slice(&(off as i32).to_le_bytes());
        bsp[8 + lump * 8..12 + lump * 8].copy_from_slice(&(data.len() as i32).to_le_bytes());
        bsp.extend_from_slice(data);
    };
    put(&mut bsp, 0, entities);
    put(&mut bsp, 2, &textures);
    let (names, sky) = bsp_textures_and_sky(&bsp);
    assert_eq!(names, ["{Fence", "WALL01"]);
    assert_eq!(sky.as_deref(), Some("kraftstoff"));
    assert_eq!(bsp_textures_and_sky(&[1, 2, 3]), (vec![], None));
}

#[test]
fn cache_lookup_offsets_land_on_what_they_name() {
    scan::Pattern::parse(LOAD_TEXTURE2_HEAD).unwrap();
    fixed(
        LOAD_TEXTURE2_HEAD,
        head::NEXT_RECORD,
        &[0x46, 0x83, 0xC7, 0x54],
    );
    fixed(LOAD_TEXTURE2_HEAD, head::NAME_MATCHED, NAME_MATCHED_STOLEN);
    fixed(LOAD_TEXTURE2_HEAD, head::SIZE_COMPARE, &[0x3B, 0xC1]);
    fixed(
        LOAD_TEXTURE2_HEAD,
        head::HOST_SPAWN_COUNT,
        &[0x66, 0x8B, 0x15],
    );
    // The `je` after the name compare lands exactly on the detour: `74 06`
    // right before NEXT_RECORD's 6 bytes.
    fixed(LOAD_TEXTURE2_HEAD, head::NEXT_RECORD - 2, &[0x74, 0x06]);
    assert_eq!(head::NEXT_RECORD + 6, head::NAME_MATCHED);
    assert_eq!(
        tokens(LOAD_TEXTURE2_HEAD).len(),
        head::HOST_SPAWN_COUNT + 11
    );
}

#[test]
fn cache_stub_branches_land_where_the_comments_say() {
    let code = cache_stub();
    assert_eq!(&code[..2], &[0x57, 0x55]);
    assert_eq!(&code[8..11], &[0x83, 0xC4, 0x08]);
    // jnz +6 at [13] skips exactly the 6-byte next-record jump.
    assert_eq!(&code[13..15], &[0x75, 0x06]);
    assert_eq!(&code[15..17], &[0xFF, 0x25]);
    assert_eq!(&code[21..27], NAME_MATCHED_STOLEN);
    assert_eq!(&code[27..29], &[0xFF, 0x25]);
    assert_eq!(code.len(), 33);
}

#[test]
fn a_texture_the_engine_renamed_is_still_found() {
    let files = [
        (("wall01".to_string(), 0xaaaa_aaaa), PathBuf::from("a")),
        (("wall01".to_string(), 0xbbbb_bbbb), PathBuf::from("b")),
        (("wbll01".to_string(), 0xcccc_cccc), PathBuf::from("c")),
        (("{fen".to_string(), 0xdddd_dddd), PathBuf::from("d")),
    ]
    .into_iter()
    .collect();
    let index = Index {
        dir: PathBuf::new(),
        files,
    };
    // `wall01` renamed once or several times: same pixels, 4th letter moved.
    let found = |seen: &str, hash| {
        renamed_by_engine(&index, seen, hash).map(|(n, p)| (n.clone(), p.clone()))
    };
    assert_eq!(
        found("walm01", 0xbbbb_bbbb),
        Some(("wall01".into(), "b".into()))
    );
    assert_eq!(
        found("walp01", 0xaaaa_aaaa),
        Some(("wall01".into(), "a".into()))
    );
    assert_eq!(
        found("{feo", 0xdddd_dddd),
        Some(("{fen".into(), "d".into()))
    );
    // The hash must match, only the 4th letter may differ, and the name
    // itself isn't a rename.
    assert_eq!(found("walm01", 0x1234_5678), None);
    assert_eq!(found("xalm01", 0xbbbb_bbbb), None);
    assert_eq!(found("walm011", 0xbbbb_bbbb), None);
    assert_eq!(found("wall01", 0xbbbb_bbbb), None);
    assert_eq!(found("wal", 0xbbbb_bbbb), None);
}

#[test]
fn map_names_are_trimmed() {
    assert_eq!(map_short_name("maps/dod_Anzio.bsp"), "dod_anzio");
    assert_eq!(map_short_name("dod_caen"), "dod_caen");
    assert_eq!(map_short_name(""), NO_MAP);
}

/// #729: a decal's file is named as the engine uploads it (`{` or `}`
/// and the rest of the lump name), and only the two iTypes the decal
/// loader sends are taken; the other kinds keep theirs.
#[test]
fn decals_are_keyed_as_the_engine_names_them() {
    assert_eq!(file_stem_name("}Scorch1"), "}scorch1");
    assert_eq!(
        parse_file_name("}scorch1_1a2b3c4d.tga"),
        Some(("}scorch1".to_string(), 0x1a2b_3c4d))
    );
    assert_eq!(
        parse_file_name("{shot5_deadbeef.tga"),
        Some(("{shot5".to_string(), 0xdead_beef))
    );
    assert_eq!(miss_display_name(GLT_DECAL, "}scorch1"), "}scorch1");
    assert_eq!(owner_of("decal", "{Shot5"), "decal:{shot5");

    assert!(replaceable_i_type(GLT_DECAL, TEX_TYPE_ALPHA));
    assert!(replaceable_i_type(GLT_DECAL, TEX_TYPE_ALPHA_GRADIENT));
    assert!(!replaceable_i_type(GLT_DECAL, TEX_TYPE_NONE));
    assert!(!replaceable_i_type(GLT_DECAL, TEX_TYPE_RGBA));
    for kind in [GLT_WORLD, GLT_STUDIO] {
        assert!(replaceable_i_type(kind, TEX_TYPE_NONE));
        assert!(replaceable_i_type(kind, TEX_TYPE_ALPHA));
        assert!(!replaceable_i_type(kind, TEX_TYPE_ALPHA_GRADIENT));
    }
    assert!(replaceable_i_type(GLT_SPRITE, TEX_TYPE_ALPHA_GRADIENT));
    assert!(!replaceable_i_type(GLT_SPRITE, TEX_TYPE_RGBA));
}

#[test]
fn sprite_frames_are_keyed_by_file_name_and_frame() {
    assert_eq!(
        sprite_frame_name("sprites/muzzleflash1.spr_0"),
        "muzzleflash1_0"
    );
    assert_eq!(
        sprite_frame_name("sprites/effects/Debris_Dirt3.SPR_203"),
        "debris_dirt3_203"
    );
    assert_eq!(sprite_frame_name("sprites\\x.spr_1"), "x_1");
    assert_eq!(sprite_frame_name("odd_name"), "odd_name");
    // What the Python pipeline writes for the same frame parses back to it.
    assert_eq!(
        parse_file_name("muzzleflash1_0_1a2b3c4d.tga"),
        Some(("muzzleflash1_0".to_string(), 0x1a2b_3c4d))
    );
}

#[test]
fn status_reports_before_any_load() {
    if !has_observed() {
        assert!(status().contains("0 of 0"));
    }
}
