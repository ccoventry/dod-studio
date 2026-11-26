//! Where a demo's clock jumps, where its frames stop, which maps it signs on
//! to and whether a strict parse reads it: the first look at a damaged or
//! salvaged demo (#15, #41).
//!
//! The walk reads frame headers straight from the bytes, so it gets past
//! damage the strict parser (`dem::open_demo_from_bytes`) refuses, and says
//! where it stopped when it can't:
//!
//! - **clock**: first and last frame time, every jump forward of more than
//!   `--jump` seconds (default 2) and every step back of more than 1 s;
//! - **holes**: an unknown frame type or a frame running past the end, with
//!   its byte offset (the walk stops there);
//! - **maps**: each network frame carrying an `SvcServerInfo` (a signon: the
//!   start, or a level change inside the recording), with its time;
//! - **directory**: the header's entries, and whether the walk reached the
//!   directory where the header says it is;
//! - **strict parse**: whether `dem` reads the whole file, and its error if not.
//!
//!     cargo run --release -p native --example demo_gaps -- a.dem [b.dem ...] [--jump 5]
//!
//! `hl-demo-auditor --integrity` (and the `demo_integrity` example) answers a
//! different question: which deltas build on a frame the file lacks.

const HEADER_BYTES: usize = 544;
const NETWORK_PROTOCOL_AT: usize = 12;
const DIRECTORY_OFFSET_AT: usize = 540;
const FRAME_HEADER_BYTES: usize = 9;
/// A network frame's demo info and sequence info come before its message length.
const NETWORK_INFO_BYTES: usize = 464;
const BACK_STEP_S: f32 = 1.0;
const DIRECTORY_ENTRY_BYTES: usize = 92;

#[derive(Debug, PartialEq)]
enum Stop {
    ReachedDirectory,
    UnknownType { at: usize, kind: u8 },
    Truncated { at: usize, kind: u8 },
}

#[derive(Debug, Default)]
struct Walk {
    frames: usize,
    /// First and last clock of each directory section (LOADING, then Playback):
    /// each section's clock starts again, so a step back between them is no jump.
    sections: Vec<(f32, f32)>,
    jumps: Vec<(usize, f32, f32)>,
    signons: Vec<(usize, f32, String)>,
    stop: Option<Stop>,
}

fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    b.get(at..at + 4)
        .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn f32_at(b: &[u8], at: usize) -> Option<f32> {
    b.get(at..at + 4)
        .map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Bytes after the 9-byte frame header that a frame of `kind` takes, or
/// `None` for a type GoldSrc doesn't write or a length field past the end.
fn body_len(b: &[u8], at: usize, kind: u8) -> Option<usize> {
    let len_at = |off: usize| i32_at(b, at + off).and_then(|n| usize::try_from(n).ok());
    match kind {
        0 | 1 => len_at(NETWORK_INFO_BYTES).map(|n| NETWORK_INFO_BYTES + 4 + n),
        2 | 5 => Some(0),
        3 => Some(64),
        4 => Some(32),
        6 => Some(84),
        7 => Some(8),
        8 => len_at(4).map(|n| 8 + n + 16),
        9 => len_at(0).map(|n| 4 + n),
        _ => None,
    }
}

/// Walks every frame from the end of the header to the directory.
fn walk(b: &[u8], jump_s: f32) -> Walk {
    let directory = i32_at(b, DIRECTORY_OFFSET_AT)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|&n| n >= HEADER_BYTES && n <= b.len())
        .unwrap_or(b.len());
    let protocol = i32_at(b, NETWORK_PROTOCOL_AT).unwrap_or(48);
    let mut w = Walk::default();
    let mut pos = HEADER_BYTES;
    let mut in_section = false;
    while pos + FRAME_HEADER_BYTES <= directory {
        let kind = b[pos];
        let time = f32_at(b, pos + 1).unwrap_or(0.0);
        let body = pos + FRAME_HEADER_BYTES;
        let Some(len) = body_len(b, body, kind) else {
            w.stop = Some(if kind > 9 {
                Stop::UnknownType { at: pos, kind }
            } else {
                Stop::Truncated { at: pos, kind }
            });
            return w;
        };
        if body + len > directory {
            w.stop = Some(Stop::Truncated { at: pos, kind });
            return w;
        }
        w.frames += 1;
        if kind <= 1 {
            let messages = &b[body + NETWORK_INFO_BYTES + 4..body + len];
            for map in native::demo_split::quick_maps(messages, protocol) {
                w.signons.push((pos, time, map));
            }
        }
        if kind == 5 {
            in_section = false;
        } else if kind != 2 && time > 0.0 {
            // DemoStart and NextSection carry no clock of their own.
            match w.sections.last_mut() {
                Some((_, prev)) if in_section => {
                    if time - *prev > jump_s || time < *prev - BACK_STEP_S {
                        w.jumps.push((pos, *prev, time));
                    }
                    *prev = time;
                }
                _ => {
                    w.sections.push((time, time));
                    in_section = true;
                }
            }
        }
        pos = body + len;
    }
    w.stop = Some(Stop::ReachedDirectory);
    w
}

fn print_directory(b: &[u8]) {
    let Some(at) = i32_at(b, DIRECTORY_OFFSET_AT).and_then(|n| usize::try_from(n).ok()) else {
        return;
    };
    let Some(count) = i32_at(b, at) else {
        println!("  directory: offset {at} is past the end of the file");
        return;
    };
    println!("  directory at {at}: {count} entr(y/ies)");
    for i in 0..usize::try_from(count).unwrap_or(0).min(16) {
        let e = at + 4 + i * DIRECTORY_ENTRY_BYTES;
        let Some(desc) = b.get(e + 4..e + 68) else {
            break;
        };
        let name = String::from_utf8_lossy(desc.split(|&c| c == 0).next().unwrap_or_default());
        println!(
            "    {i}: {name:10} time {:8.2} s, {} frames, at {}, {} bytes",
            f32_at(b, e + 76).unwrap_or(0.0),
            i32_at(b, e + 80).unwrap_or(0),
            i32_at(b, e + 84).unwrap_or(0),
            i32_at(b, e + 88).unwrap_or(0),
        );
    }
}

fn report(path: &std::path::Path, jump_s: f32) {
    println!("{}", path.display());
    let b = match std::fs::read(path) {
        Ok(b) if b.len() >= HEADER_BYTES && b.starts_with(b"HLDEMO") => b,
        Ok(_) => return println!("  not a GoldSrc demo (no HLDEMO header)"),
        Err(e) => return println!("  could not read: {e}"),
    };
    let w = walk(&b, jump_s);
    println!("  {} frames", w.frames);
    for (i, (a, z)) in w.sections.iter().enumerate() {
        println!("  section {i}: clock {a:.2} -> {z:.2} s");
    }
    for (at, a, z) in &w.jumps {
        println!("  jump {a:.2} -> {z:.2} s ({:+.2} s) at byte {at}", z - a);
    }
    for (at, t, map) in &w.signons {
        println!("  signon to {map} at {t:.2} s (byte {at})");
    }
    match &w.stop {
        Some(Stop::ReachedDirectory) | None => println!("  walk reached the directory"),
        Some(Stop::UnknownType { at, kind }) => {
            println!("  HOLE: unknown frame type {kind} at byte {at}; walk stopped")
        }
        Some(Stop::Truncated { at, kind }) => {
            println!("  HOLE: type-{kind} frame at byte {at} runs past the directory; walk stopped")
        }
    }
    print_directory(&b);
    match dem::open_demo_from_bytes(&b) {
        Ok(_) => println!("  strict parse: ok"),
        Err(e) => println!("  strict parse: FAILS: {}", readable(&e.to_string())),
    }
}

/// `dem`'s nom errors print the failing input as a byte list; when those bytes
/// are text (its own "message length too long: N"), show the text.
fn readable(error: &str) -> String {
    let Some(start) = error.find("input: [") else {
        return error.to_string();
    };
    let list = &error[start + 8..];
    let Some(end) = list.find(']') else {
        return error.to_string();
    };
    let bytes: Option<Vec<u8>> = list[..end]
        .split(',')
        .map(|n| n.trim().parse().ok())
        .collect();
    match bytes {
        Some(b) if b.iter().all(|c| (0x20..0x7f).contains(c)) => {
            format!(
                "{}\"{}\"{}",
                &error[..start + 7],
                String::from_utf8_lossy(&b),
                &list[end + 1..]
            )
        }
        _ => error.to_string(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut jump_s = 2.0;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "--jump" {
            jump_s = it.next().and_then(|v| v.parse().ok()).unwrap_or(jump_s);
        } else {
            files.push(std::path::PathBuf::from(arg));
        }
    }
    if files.is_empty() {
        eprintln!("usage: demo_gaps <demo.dem>... [--jump SECONDS]");
        std::process::exit(2);
    }
    for f in files {
        report(&f, jump_s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(kind: u8, time: f32, body: &[u8]) -> Vec<u8> {
        let mut f = vec![kind];
        f.extend_from_slice(&time.to_le_bytes());
        f.extend_from_slice(&0i32.to_le_bytes());
        f.extend_from_slice(body);
        f
    }

    fn demo(frames: &[Vec<u8>]) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_BYTES];
        b[..6].copy_from_slice(b"HLDEMO");
        for f in frames {
            b.extend_from_slice(f);
        }
        let dir = b.len() as i32;
        b[DIRECTORY_OFFSET_AT..DIRECTORY_OFFSET_AT + 4].copy_from_slice(&dir.to_le_bytes());
        b.extend_from_slice(&0i32.to_le_bytes());
        b
    }

    #[test]
    fn a_clock_jump_past_the_threshold_is_listed() {
        let b = demo(&[
            frame(4, 1.0, &[0; 32]),
            frame(4, 1.5, &[0; 32]),
            frame(4, 9.0, &[0; 32]),
        ]);
        let w = walk(&b, 2.0);
        assert_eq!(w.frames, 3);
        assert_eq!(w.jumps.len(), 1);
        assert_eq!((w.jumps[0].1, w.jumps[0].2), (1.5, 9.0));
        assert_eq!(w.stop, Some(Stop::ReachedDirectory));
    }

    #[test]
    fn an_unknown_frame_type_stops_the_walk_with_its_offset() {
        let b = demo(&[frame(4, 1.0, &[0; 32]), frame(42, 1.1, &[])]);
        let w = walk(&b, 2.0);
        assert_eq!(w.frames, 1);
        assert_eq!(
            w.stop,
            Some(Stop::UnknownType {
                at: HEADER_BYTES + FRAME_HEADER_BYTES + 32,
                kind: 42
            })
        );
    }

    #[test]
    fn demo_start_frames_do_not_count_as_a_clock_step() {
        let b = demo(&[
            frame(4, 5.0, &[0; 32]),
            frame(2, 0.0, &[]),
            frame(4, 5.1, &[0; 32]),
        ]);
        assert!(walk(&b, 2.0).jumps.is_empty());
    }

    #[test]
    fn the_clock_starting_again_in_the_next_section_is_no_jump() {
        let b = demo(&[
            frame(4, 20.0, &[0; 32]),
            frame(5, 0.0, &[]),
            frame(4, 0.5, &[0; 32]),
        ]);
        let w = walk(&b, 2.0);
        assert!(w.jumps.is_empty());
        assert_eq!(w.sections, vec![(20.0, 20.0), (0.5, 0.5)]);
    }

    #[test]
    fn a_text_input_in_a_parse_error_is_shown_as_text() {
        assert_eq!(
            readable("Error { input: [104, 105], code: Fail }"),
            "Error { input: \"hi\", code: Fail }"
        );
        assert_eq!(
            readable("Error { input: [0, 1] }"),
            "Error { input: [0, 1] }"
        );
    }
}
