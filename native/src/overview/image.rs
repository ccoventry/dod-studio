//! The two image files the game's overview loader takes, chosen by extension
//! (`hw.dll`'s map-sprite loader, PRE `+0x45f50`, Anniversary `+0x243870`):
//!
//! - `.tga`: 32-bit, kept as it is, alpha included;
//! - `.bmp`: 8-bit with a palette, nothing else.
//!
//! Either way pure green (0,255,0) is made transparent, the image is cut into
//! 128-pixel tiles, and it must fit the loader's fixed 3 MB RGBA buffer:
//! 1024x768 at most.

use super::scene::BACKGROUND;

/// An uncompressed 32-bit TGA, rows bottom-up (the engine's own loader
/// reads the classic orientation).
pub fn tga(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    check(width, height, rgba)?;
    let mut out = Vec::with_capacity(18 + rgba.len());
    out.extend_from_slice(&[0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    out.push(32);
    out.push(8); // 8 alpha bits, origin bottom-left
    for row in (0..height as usize).rev() {
        for px in rgba[row * width as usize * 4..(row + 1) * width as usize * 4]
            .as_chunks::<4>()
            .0
        {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    Ok(out)
}

/// An 8-bit BMP. Pixels under half covered become the background green,
/// which the game makes transparent; the rest are matched to at most 255
/// colours (median cut), with green kept for the background alone.
pub fn bmp(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    check(width, height, rgba)?;
    let pixels: Vec<[u8; 3]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            if p[3] < 128 {
                BACKGROUND
            } else if [p[0], p[1], p[2]] == BACKGROUND {
                // A map pixel that happens to be the key would vanish.
                [0, 254, 0]
            } else {
                [p[0], p[1], p[2]]
            }
        })
        .collect();
    let opaque: Vec<[u8; 3]> = pixels
        .iter()
        .copied()
        .filter(|&p| p != BACKGROUND)
        .collect();
    let mut palette = vec![BACKGROUND];
    palette.extend(median_cut(&opaque, 255));
    let mut cache = std::collections::HashMap::new();
    let index: Vec<u8> = pixels
        .iter()
        .map(|&p| {
            if p == BACKGROUND {
                return 0;
            }
            *cache
                .entry(p)
                .or_insert_with(|| nearest(&palette[1..], p) as u8 + 1)
        })
        .collect();

    let row = (width as usize + 3) & !3;
    let data_size = row * height as usize;
    let offset = 14 + 40 + 256 * 4;
    let mut out = Vec::with_capacity(offset + data_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((offset + data_size) as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(data_size as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&256u32.to_le_bytes());
    out.extend_from_slice(&256u32.to_le_bytes());
    for i in 0..256 {
        let c = palette.get(i).copied().unwrap_or([0, 0, 0]);
        out.extend_from_slice(&[c[2], c[1], c[0], 0]);
    }
    for y in (0..height as usize).rev() {
        let start = y * width as usize;
        out.extend_from_slice(&index[start..start + width as usize]);
        out.resize(out.len() + row - width as usize, 0);
    }
    Ok(out)
}

fn check(width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if width == 0 || height == 0 || !width.is_multiple_of(128) || !height.is_multiple_of(128) {
        return Err(format!(
            "{width}x{height}: the game needs both sides a multiple of 128"
        ));
    }
    if width as usize * height as usize * 4 > 0x30_0000 {
        return Err(format!("{width}x{height} is more than the game's 1024x768"));
    }
    if rgba.len() != width as usize * height as usize * 4 {
        return Err("the pixel data is not the image's size".to_string());
    }
    Ok(())
}

fn nearest(palette: &[[u8; 3]], p: [u8; 3]) -> usize {
    palette
        .iter()
        .enumerate()
        .min_by_key(|(_, c)| {
            (0..3)
                .map(|k| (c[k] as i32 - p[k] as i32).pow(2))
                .sum::<i32>()
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// At most `n` colours standing for `pixels`: the exact set when it is small
/// enough, else boxes split on their widest channel at the median.
fn median_cut(pixels: &[[u8; 3]], n: usize) -> Vec<[u8; 3]> {
    let mut counts = std::collections::HashMap::new();
    for &p in pixels {
        *counts.entry(p).or_insert(0u32) += 1;
    }
    if counts.len() <= n {
        let mut exact: Vec<[u8; 3]> = counts.into_keys().collect();
        exact.sort();
        return exact;
    }
    let mut boxes: Vec<Vec<([u8; 3], u32)>> = vec![counts.into_iter().collect()];
    while boxes.len() < n {
        // Split the box with the widest spread that can still be split.
        let Some((at, channel)) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let spread = |k: usize| {
                    let lo = b.iter().map(|(c, _)| c[k]).min().unwrap_or(0);
                    let hi = b.iter().map(|(c, _)| c[k]).max().unwrap_or(0);
                    hi - lo
                };
                let k = (0..3).max_by_key(|&k| spread(k)).unwrap_or(0);
                (i, k, spread(k))
            })
            .max_by_key(|&(_, _, s)| s)
            .map(|(i, k, _)| (i, k))
        else {
            break;
        };
        let mut b = boxes.swap_remove(at);
        b.sort_by_key(|(c, _)| c[channel]);
        let total: u32 = b.iter().map(|(_, n)| n).sum();
        let mut acc = 0;
        let mut cut = 1;
        for (i, (_, n)) in b.iter().enumerate() {
            acc += n;
            if acc * 2 >= total {
                cut = (i + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(cut);
        boxes.push(b);
        boxes.push(rest);
    }
    boxes
        .iter()
        .map(|b| {
            let total: u64 = b.iter().map(|(_, n)| *n as u64).sum::<u64>().max(1);
            let mut c = [0u64; 3];
            for (p, n) in b {
                for k in 0..3 {
                    c[k] += p[k] as u64 * *n as u64;
                }
            }
            [
                (c[0] / total) as u8,
                (c[1] / total) as u8,
                (c[2] / total) as u8,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| f(x, y))
            .collect()
    }

    #[test]
    fn the_tga_header_says_32_bit_bottom_up() {
        let px = image(128, 128, |_, _| [1, 2, 3, 4]);
        let t = tga(128, 128, &px).unwrap();
        assert_eq!(t[2], 2, "uncompressed true colour");
        assert_eq!(t[16], 32);
        assert_eq!(t[17], 8);
        assert_eq!(&t[18..22], &[3, 2, 1, 4], "BGRA");
        assert_eq!(t.len(), 18 + 128 * 128 * 4);
    }

    #[test]
    fn the_tga_writes_the_bottom_row_first() {
        let px = image(128, 128, |_, y| {
            if y == 127 {
                [9, 9, 9, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let t = tga(128, 128, &px).unwrap();
        assert_eq!(&t[18..22], &[9, 9, 9, 255]);
    }

    #[test]
    fn sizes_the_game_refuses_are_refused() {
        assert!(tga(100, 128, &vec![0; 100 * 128 * 4]).is_err());
        assert!(tga(2048, 768, &vec![0; 2048 * 768 * 4]).is_err());
        assert!(tga(1024, 768, &[0; 4]).is_err());
    }

    #[test]
    fn the_bmp_keeps_green_for_the_background_only() {
        let px = image(128, 128, |x, _| match x {
            0..=9 => [0, 0, 0, 0],
            10..=19 => [0, 255, 0, 255],
            _ => [200, 10, 10, 255],
        });
        let b = bmp(128, 128, &px).unwrap();
        assert_eq!(&b[0..2], b"BM");
        assert_eq!(u16::from_le_bytes([b[28], b[29]]), 8, "8-bit");
        let palette = &b[54..54 + 1024];
        assert_eq!(&palette[0..3], &[0, 255, 0], "entry 0 is the key (BGR)");
        let data = &b[54 + 1024..];
        assert_eq!(data[0], 0, "transparent pixels use the key");
        assert_ne!(data[15], 0, "a map pixel that was green does not");
    }

    #[test]
    fn many_colours_come_down_to_the_palette() {
        let px = image(256, 128, |x, y| [x as u8, y as u8 * 2, (x ^ y) as u8, 255]);
        let b = bmp(256, 128, &px).unwrap();
        assert_eq!(b.len(), 54 + 1024 + 256 * 128);
        let colours = median_cut(
            &px.as_chunks::<4>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect::<Vec<_>>(),
            255,
        );
        assert!(colours.len() <= 255);
    }
}
