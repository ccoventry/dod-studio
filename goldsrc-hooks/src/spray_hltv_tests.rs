use super::*;

fn bytes(pattern: &str) -> Vec<Option<u8>> {
    pattern
        .split_whitespace()
        .map(|t| u8::from_str_radix(t, 16).ok())
        .collect()
}

#[test]
fn patterns_parse_and_start_with_the_stolen_bytes() {
    for build in &BUILDS {
        for p in [build.te_pattern, build.decal_texture, build.miptex_texture] {
            assert!(scan::Pattern::parse(p).is_ok(), "{}: {p}", build.name);
        }
        let te = bytes(build.te_pattern);
        let head: Vec<u8> = te[..build.te_stolen.len()]
            .iter()
            .map(|b| b.expect("no wildcards in the stolen bytes"))
            .collect();
        assert_eq!(head, build.te_stolen, "{}", build.name);
        let dt: Vec<Option<u8>> = DT_STOLEN.iter().map(|&b| Some(b)).collect();
        assert_eq!(bytes(build.decal_texture)[..6], dt[..], "{}", build.name);
        // The customdata address is the match's wildcard dword.
        assert!(
            bytes(build.decal_texture)[build.customdata_at..build.customdata_at + 4]
                .iter()
                .all(Option::is_none),
            "{}",
            build.name
        );
    }
}

/// The span is the customdata test, its `je` to the wad-decal branch,
/// and the texture test; the `je` lands on `else_at`, and the custom
/// branch starts right after the `je` that follows the span.
#[test]
fn the_span_and_targets_agree_with_the_pattern() {
    for build in &BUILDS {
        let n = build.cust_test.len();
        let mut span = build.cust_test.to_vec();
        span.push(0x74);
        span.push(build.te_stolen[n + 1]);
        span.extend_from_slice(build.tex_test);
        assert_eq!(span, build.te_stolen, "{}", build.name);
        assert_eq!(
            n + 2 + build.te_stolen[n + 1] as usize,
            build.else_at,
            "{}: the je's target",
            build.name
        );
        // `je rel32` (6 bytes) right after the span, then the custom branch.
        let te = bytes(build.te_pattern);
        let after = build.te_stolen.len();
        assert_eq!(
            &te[after..after + 2],
            &[Some(0x0f), Some(0x84)],
            "{}",
            build.name
        );
        assert_eq!(after + 6, build.custom_at, "{}", build.name);
    }
}

#[test]
fn the_te_stub_assembles_to_what_the_comment_claims() {
    let code = te_stub(
        &BUILDS[0],
        0x1111_1111,
        0x2222_2222,
        0x3333_3333,
        0x4444_4444,
    );
    #[rustfmt::skip]
    assert_eq!(
        code,
        vec![
            0x51,                               //  0 push ecx
            0xff, 0x75, 0xc8,                   //  1 push [ebp-0x38]
            0xb8, 0x11, 0x11, 0x11, 0x11,       //  4 mov  eax, slot_texture
            0xff, 0xd0,                         //  9 call eax
            0x83, 0xc4, 0x04,                   // 11 add  esp, 4
            0x59,                               // 14 pop  ecx
            0x85, 0xc0,                         // 15 test eax, eax
            0x74, 0x09,                         // 17 jz   .game (28)
            0x89, 0x45, 0xd4,                   // 19 mov  [ebp-0x2c], eax
            0xff, 0x25, 0x22, 0x22, 0x22, 0x22, // 22 jmp  [TE_CUSTOM]
            0x3b, 0xfe,                         // 28 .game: cmp edi, esi
            0x75, 0x06,                         // 30 jne  .cust (38)
            0xff, 0x25, 0x33, 0x33, 0x33, 0x33, // 32 jmp  [TE_ELSE]
            0x39, 0x75, 0xd4,                   // 38 .cust: cmp [ebp-0x2c], esi
            0xff, 0x25, 0x44, 0x44, 0x44, 0x44, // 41 jmp  [TE_RESUME]
        ]
    );
    let post = te_stub(
        &BUILDS[1],
        0x1111_1111,
        0x2222_2222,
        0x3333_3333,
        0x4444_4444,
    );
    #[rustfmt::skip]
    assert_eq!(
        post,
        vec![
            0x51,                               //  0 push ecx (a saved spray's texture)
            0xff, 0x75, 0xa4,                   //  1 push [ebp-0x5c]
            0xb8, 0x11, 0x11, 0x11, 0x11,       //  4 mov  eax, slot_texture
            0xff, 0xd0,                         //  9 call eax
            0x83, 0xc4, 0x04,                   // 11 add  esp, 4
            0x59,                               // 14 pop  ecx
            0x85, 0xc0,                         // 15 test eax, eax
            0x74, 0x08,                         // 17 jz   .game (27)
            0x8b, 0xc8,                         // 19 mov  ecx, eax
            0xff, 0x25, 0x22, 0x22, 0x22, 0x22, // 21 jmp  [TE_CUSTOM]
            0x85, 0xf6,                         // 27 .game: test esi, esi
            0x75, 0x06,                         // 29 jne  .cust (37)
            0xff, 0x25, 0x33, 0x33, 0x33, 0x33, // 31 jmp  [TE_ELSE]
            0x85, 0xc9,                         // 37 .cust: test ecx, ecx
            0xff, 0x25, 0x44, 0x44, 0x44, 0x44, // 39 jmp  [TE_RESUME]
        ]
    );
}

#[test]
fn the_decal_texture_stub_assembles_to_what_the_comment_claims() {
    let code = dt_stub(0x1111_1111, 0x5555_5555, 0x2222_2222, 0x3333_3333);
    #[rustfmt::skip]
    assert_eq!(
        code,
        vec![
            0x8b, 0x44, 0x24, 0x04,                   //  0 mov  eax, [esp+4]
            0xf7, 0xd0,                               //  4 not  eax
            0x83, 0xf8, 0x21,                         //  6 cmp  eax, SLOTS
            0x73, 0x28,                               //  9 jae  .game (51)
            0x8b, 0xc8,                               // 11 mov  ecx, eax
            0x69, 0xc9, 0x50, 0x02, 0x00, 0x00,       // 13 imul ecx, ecx, 0x250
            0x83, 0xb9, 0x22, 0x22, 0x22, 0x22, 0x00, // 19 cmp  dword [ecx + customdata], 0
            0x75, 0x09,                               // 26 jne  .saved (37)
            0x8b, 0x0c, 0x85, 0x11, 0x11, 0x11, 0x11, // 28 mov  ecx, [SLOT_TEX + eax*4]
            0xeb, 0x07,                               // 35 jmp  .test (44)
            0x8b, 0x0c, 0x85, 0x55, 0x55, 0x55, 0x55, // 37 .saved: mov ecx, [SLOT_OVER + eax*4]
            0x85, 0xc9,                               // 44 .test: test ecx, ecx
            0x74, 0x03,                               // 46 jz   .game (51)
            0x8b, 0xc1,                               // 48 mov  eax, ecx
            0xc3,                                     // 50 ret
            0x55, 0x8b, 0xec, 0x8b, 0x4d, 0x08,       // 51 .game: the stolen prologue
            0xff, 0x25, 0x33, 0x33, 0x33, 0x33,       // 57 jmp  [DT_RESUME]
        ]
    );
}

#[test]
fn texture_names_are_unique_and_fit() {
    let a = texture_name(1);
    assert_eq!(&a[..14], b"{hltvspray0001");
    assert_eq!(a[14..], [0, 0]);
    assert_ne!(texture_name(2), a);
    assert!(!spray::looks_like_custom_name("{hltvspray0001"));
}
