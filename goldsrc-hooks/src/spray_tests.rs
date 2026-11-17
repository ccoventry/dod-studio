use super::*;

#[test]
fn md5_matches_rfc_1321() {
    assert_eq!(hex(&md5(&[b""])), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hex(&md5(&[b"abc"])), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(
        hex(&md5(&[b"The quick brown fox jumps ", b"over the lazy dog"])),
        "9e107d9d372bb6826bd81d3542a419d6"
    );
    // 80 bytes: the padding needs a second block.
    assert_eq!(
        hex(&md5(&[
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
        ])),
        "57edf4a22be3c955ac49da2e2107b67a"
    );
}

fn sample() -> Rgba {
    // 16x16: a red square on a transparent field, a blue dot, a soft edge.
    let (w, h) = (16u32, 16u32);
    let mut pixels = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let inside = (4..12).contains(&x) && (4..12).contains(&y);
            let px = if x == 0 && y == 0 {
                [0, 0, 255, 255]
            } else if inside {
                [200, 30 + x as u8, 30, 255]
            } else if x == 12 && y == 8 {
                [200, 40, 30, 100]
            } else {
                [0, 0, 0, 0]
            };
            pixels.extend_from_slice(&px);
        }
    }
    Rgba {
        width: w,
        height: h,
        pixels,
    }
}

/// A WAD the writer makes passes the engine's checks as `wad_key` applies
/// them, and the key read back from the file is the key of the upload the
/// engine would make from it: the same bytes `spray_key` hashes.
#[test]
fn a_written_wad_round_trips_through_the_engines_layout() {
    let img = sample();
    let (wad, key) = build_wad(&img).unwrap();
    let (read, w, h) = wad_key(&wad).unwrap();
    assert_eq!((w, h), (16, 16));
    assert_eq!(read, key);

    // The container as the engine's check reads it.
    assert_eq!(&wad[..4], b"WAD3");
    let dir_at = u32::from_le_bytes(wad[8..12].try_into().unwrap()) as usize;
    assert_eq!(dir_at + 32, wad.len());
    assert_eq!(dir_at % 4, 0);
    let (filepos, disksize, size) = (
        u32::from_le_bytes(wad[dir_at..dir_at + 4].try_into().unwrap()) as usize,
        u32::from_le_bytes(wad[dir_at + 4..dir_at + 8].try_into().unwrap()) as usize,
        u32::from_le_bytes(wad[dir_at + 8..dir_at + 12].try_into().unwrap()) as usize,
    );
    assert_eq!(filepos, 12);
    assert_eq!(disksize, size);
    assert!(filepos + disksize <= dir_at);
    assert_eq!(wad[dir_at + 12], 0x43);
    assert_eq!(&wad[dir_at + 16..dir_at + 21], b"{LOGO");

    // The lump as Draw_ValidateCustomLogo reads it.
    let lump = &wad[filepos..filepos + size];
    assert_eq!(&lump[..5], b"{LOGO");
    let pix = 16 * 16;
    let offsets: Vec<usize> = (0..4)
        .map(|i| u32::from_le_bytes(lump[24 + 4 * i..28 + 4 * i].try_into().unwrap()) as usize)
        .collect();
    assert_eq!(
        offsets,
        vec![
            40,
            40 + pix,
            40 + pix + pix / 4,
            40 + pix + pix / 4 + pix / 16
        ]
    );
    let pal_at = offsets[3] + pix / 64;
    assert_eq!(u16::from_le_bytes([lump[pal_at], lump[pal_at + 1]]), 256);
    assert_eq!(lump.len(), pal_at + 2 + 768);
    assert_eq!(&lump[lump.len() - 3..], &CUTOUT_KEY);

    // Transparent where the PNG was, red where it was red, index 255 for
    // the key only.
    let indices = &lump[40..40 + pix];
    assert_eq!(indices[1], TRANSPARENT);
    assert_ne!(indices[8 * 16 + 8], TRANSPARENT);
    assert_eq!(indices[8 * 16 + 12], TRANSPARENT, "alpha 100 is cut out");
    let pal = &lump[pal_at + 2..];
    let red = indices[8 * 16 + 8] as usize;
    assert!(pal[3 * red] > 150 && pal[3 * red + 2] < 80);
}

#[test]
fn the_wad_fits_the_size_a_server_takes() {
    let solid = |width: u32, height: u32| Rgba {
        width,
        height,
        pixels: vec![255; (width * height * 4) as usize],
    };
    // The team sprays' size: 112x112 is about 17.5 KB, 128x128 would be
    // over the 20480 a server takes.
    let (wad, _) = build_wad(&solid(3840, 3840)).unwrap();
    let (_, w, h) = wad_key(&wad).unwrap();
    assert_eq!((w, h), (112, 112));
    assert_eq!(wad.len(), 17516);
    assert!(wad_bytes(128, 128) > MAX_WAD_BYTES);
    // Tall: the long side as far as the file allows, the short one
    // scaled with it in steps of 16.
    let (wad, _) = build_wad(&solid(300, 1000)).unwrap();
    let (_, w, h) = wad_key(&wad).unwrap();
    assert_eq!((w, h), (64, 224));
    assert!(wad.len() <= MAX_WAD_BYTES);
    // Small pictures aren't scaled up past themselves.
    let (wad, _) = build_wad(&solid(50, 40)).unwrap();
    let (_, w, h) = wad_key(&wad).unwrap();
    assert_eq!((w, h), (48, 32));
    assert!(wad.len() >= MIN_WAD_BYTES);
    let (wad, _) = build_wad(&solid(4, 4)).unwrap();
    assert_eq!(wad_key(&wad).unwrap().1, 16);
    // Every size it picks stays inside the server's range.
    for (w, h) in [(1, 4000), (4000, 1), (256, 255), (17, 33), (1000, 999)] {
        let (lw, lh) = logo_size(w, h, MAX_LOGO_SIDE, MAX_WAD_BYTES);
        assert!(lw % 16 == 0 && lh % 16 == 0, "{w}x{h} -> {lw}x{lh}");
        assert!(
            (MIN_WAD_BYTES..=MAX_WAD_BYTES).contains(&wad_bytes(lw, lh)),
            "{w}x{h}"
        );
    }
    assert_eq!(pot_floor(3840, 1024), 1024);
    assert_eq!(pot_floor(3840, 4096), 2048);
    assert_eq!(pot_floor(112, 256), 64);
    assert_eq!(pot_floor(0, 256), 1);
}

#[test]
fn quantise_keeps_within_255_colours_and_the_key_entry() {
    let mut img = sample();
    // Every pixel a different colour: more than 255 to pick from.
    for (i, px) in img.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        *px = [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255];
    }
    let logo = quantise(&img);
    assert!(logo.indices.iter().all(|&i| i != TRANSPARENT));
    assert_eq!(&logo.palette[765..], &CUTOUT_KEY);
    let used: std::collections::HashSet<u8> = logo.indices.iter().copied().collect();
    assert!(used.len() <= 255 && used.len() > 100);
}

#[test]
fn the_same_picture_always_gives_the_same_key() {
    // The same PNG must make the same spray: a key that changed from run
    // to run would name one picture as many sprays in the HD-miss list.
    let mut img = Rgba {
        width: 64,
        height: 64,
        pixels: vec![0; 64 * 64 * 4],
    };
    for (i, px) in img.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        *px = [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255];
    }
    let first = build_wad(&img).unwrap().1;
    for _ in 0..5 {
        assert_eq!(build_wad(&img).unwrap().1, first);
    }
}

#[test]
fn resample_weights_colour_by_alpha() {
    let img = Rgba {
        width: 2,
        height: 1,
        pixels: vec![200, 0, 0, 255, 0, 0, 0, 0],
    };
    let half = resample(&img, 1, 1);
    // The transparent black doesn't darken the red; alpha averages.
    assert_eq!(half.pixels, vec![200, 0, 0, 128]);
}

#[test]
fn lump_key_refuses_what_validate_custom_logo_refuses() {
    let (wad, key) = build_wad_sized(&sample(), 16).unwrap();
    let lump = wad_lump(&wad).unwrap().to_vec();
    assert_eq!(lump_key(&lump).unwrap(), (key, 16, 16));
    // Mip 1 not right after mip 0.
    let mut gap = lump.clone();
    gap[28] += 1;
    assert!(lump_key(&gap).is_err());
    // A side over 256.
    let mut wide = lump.clone();
    wide[16..20].copy_from_slice(&512u32.to_le_bytes());
    assert!(lump_key(&wide).is_err());
    // Cut short anywhere: an error, never a panic.
    for len in 0..lump.len() {
        assert!(lump_key(&lump[..len]).is_err(), "{len}");
    }
}

#[test]
fn custom_names_are_the_slot_and_lump() {
    assert!(looks_like_custom_name("{00100"));
    assert!(looks_like_custom_name("}03101"));
    assert!(looks_like_custom_name("{-0200"));
    assert!(!looks_like_custom_name("{shot5"));
    assert!(!looks_like_custom_name("}scorch1"));
    assert!(!looks_like_custom_name("{0010"));
}

#[test]
fn the_key_is_over_what_the_engine_draws() {
    let indices = [1u8, 2, 3, 4];
    let mut pal = vec![0u8; 768];
    pal[765..].copy_from_slice(&CUTOUT_KEY);
    let full = spray_key(2, 2, &indices, &pal);
    assert_eq!(
        full,
        md5(&[
            &2u32.to_le_bytes(),
            &2u32.to_le_bytes(),
            &indices,
            &256u16.to_le_bytes(),
            &pal
        ])
    );
    // A shorter palette hashes its own count and bytes.
    assert_ne!(spray_key(2, 2, &indices, &pal[..3 * 16]), full);
    // Height and width are not interchangeable.
    assert_ne!(
        spray_key(4, 1, &indices, &pal),
        spray_key(1, 4, &indices, &pal)
    );
}

#[test]
fn decode_png_handles_rgb_and_rgba() {
    for (color, bytes_pp) in [(png::ColorType::Rgb, 3), (png::ColorType::Rgba, 4)] {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            let data: Vec<u8> = if bytes_pp == 3 {
                vec![10, 20, 30, 40, 50, 60]
            } else {
                vec![10, 20, 30, 128, 40, 50, 60, 255]
            };
            w.write_image_data(&data).unwrap();
        }
        let img = decode_png(&out).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.pixels[..3], &[10, 20, 30]);
        assert_eq!(img.pixels[3], if bytes_pp == 3 { 255 } else { 128 });
        assert_eq!(&img.pixels[4..7], &[40, 50, 60]);
    }
    assert!(decode_png(b"not a png").is_err());
}

#[test]
fn patterns_parse_and_name_the_same_instructions() {
    for p in [LOAD_FROM_CUSTOM_PRE, LOAD_FROM_CUSTOM_POST] {
        assert!(crate::scan::Pattern::parse(p).is_ok());
        let toks: Vec<&str> = p.split_whitespace().collect();
        assert_eq!(toks[0], "68", "push <prefix>");
        assert_eq!(toks[5], "68", "push <szCustName>");
        assert_eq!(&toks[10..12], &["C7", "05"], "mov [flag], imm32");
        assert_eq!(&toks[16..20], &["01", "00", "00", "00"]);
        assert_eq!(toks[20], "E8");
    }
}
