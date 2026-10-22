use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug)]
struct FrameInfo {
    pos: u64,
    frame_type: u8,
    time: f32,
    frame_num: i32,
}

fn read_next_frame(file: &mut std::fs::File, demo_protocol: i32) -> Option<(FrameInfo, Vec<u8>)> {
    let pos = file.stream_position().unwrap();
    let mut frame_type_byte = [0u8; 1];
    if file.read_exact(&mut frame_type_byte).is_err() {
        return None;
    }
    let frame_type = frame_type_byte[0];

    let mut time_bytes = [0u8; 4];
    if file.read_exact(&mut time_bytes).is_err() {
        return None;
    }
    let time = f32::from_le_bytes(time_bytes);

    let mut frame_num_bytes = [0u8; 4];
    if file.read_exact(&mut frame_num_bytes).is_err() {
        return None;
    }
    let frame_num = i32::from_le_bytes(frame_num_bytes);

    let mut payload = Vec::new();

    if !(2..=9).contains(&frame_type) {
        // Network Message
        let skip_size = if demo_protocol == 3 { 460 } else { 464 };
        let mut info_bytes = vec![0u8; skip_size];
        file.read_exact(&mut info_bytes).unwrap();
        payload.extend_from_slice(&info_bytes);

        let mut msg_len_bytes = [0u8; 4];
        file.read_exact(&mut msg_len_bytes).unwrap();
        payload.extend_from_slice(&msg_len_bytes);
        let msg_len = u32::from_le_bytes(msg_len_bytes);

        // println!("  NetMsg at pos={}, Tick={}, msg_len={}", pos, frame_num, msg_len);

        let mut msg_bytes = vec![0u8; msg_len as usize];
        if let Err(e) = file.read_exact(&mut msg_bytes) {
            panic!(
                "UnexpectedEof at pos {} for NetMsg tick {} with msg_len {}: {:?}",
                pos, frame_num, msg_len, e
            );
        }
        payload.extend_from_slice(&msg_bytes);
    } else if frame_type == 2 || frame_type == 5 {
        // DemoStart / NextSection - no payload
    } else if frame_type == 3 {
        // ConsoleCommand
        let mut cmd_bytes = vec![0u8; 64];
        file.read_exact(&mut cmd_bytes).unwrap();
        payload.extend_from_slice(&cmd_bytes);
    } else if frame_type == 4 {
        // ClientData
        let mut cd_bytes = vec![0u8; 32];
        file.read_exact(&mut cd_bytes).unwrap();
        payload.extend_from_slice(&cd_bytes);
    } else if frame_type == 6 {
        // Event
        let mut ev_bytes = vec![0u8; 84];
        file.read_exact(&mut ev_bytes).unwrap();
        payload.extend_from_slice(&ev_bytes);
    } else if frame_type == 7 {
        // WeaponAnim
        let mut wa_bytes = vec![0u8; 8];
        file.read_exact(&mut wa_bytes).unwrap();
        payload.extend_from_slice(&wa_bytes);
    } else if frame_type == 8 {
        // Sound
        let mut prefix = vec![0u8; 8];
        file.read_exact(&mut prefix).unwrap();
        let sample_len = u32::from_le_bytes(prefix[4..8].try_into().unwrap());
        payload.extend_from_slice(&prefix);

        let mut sample_bytes = vec![0u8; (sample_len + 16) as usize];
        file.read_exact(&mut sample_bytes).unwrap();
        payload.extend_from_slice(&sample_bytes);
    } else if frame_type == 9 {
        // DemoBuffer
        let mut prefix = vec![0u8; 4];
        file.read_exact(&mut prefix).unwrap();
        let buf_len = u32::from_le_bytes(prefix.clone().try_into().unwrap());
        payload.extend_from_slice(&prefix);

        let mut buf_bytes = vec![0u8; buf_len as usize];
        file.read_exact(&mut buf_bytes).unwrap();
        payload.extend_from_slice(&buf_bytes);
    }

    Some((
        FrameInfo {
            pos,
            frame_type,
            time,
            frame_num,
        },
        payload,
    ))
}

fn main() {
    let path_a = Path::new("./local/demos/wsod25-grp_r1-dyelife_gskill_armory_h1.dem");
    let path_b = Path::new("./local/demos/chain_01.dem");

    if !path_a.exists() || !path_b.exists() {
        eprintln!("Error: Both primer.dem and chain_01.dem must exist.");
        std::process::exit(1);
    }

    let mut file_a = std::fs::File::open(path_a).unwrap();
    let mut file_b = std::fs::File::open(path_b).unwrap();

    let mut header_a = [0u8; 544];
    let mut header_b = [0u8; 544];
    file_a.read_exact(&mut header_a).unwrap();
    file_b.read_exact(&mut header_b).unwrap();

    let proto_a = i32::from_le_bytes(header_a[8..12].try_into().unwrap());
    let proto_b = i32::from_le_bytes(header_b[8..12].try_into().unwrap());
    println!(
        "Protocol A (primer): {}, Protocol B (chain): {}",
        proto_a, proto_b
    );

    // Seek to first directory entry's frames
    let dir_offset_a = i32::from_le_bytes(header_a[540..544].try_into().unwrap());
    let dir_offset_b = i32::from_le_bytes(header_b[540..544].try_into().unwrap());

    // We'll seek to Playback entry frames (Entry #1)
    // First read Entry #1 offset
    file_a
        .seek(SeekFrom::Start(dir_offset_a as u64 + 4 + 92))
        .unwrap();
    let mut offset_bytes_a = [0u8; 4];
    file_a.seek(SeekFrom::Current(84)).unwrap(); // skip entry type, description, flags, cd_track, track_time, frame_count
    file_a.read_exact(&mut offset_bytes_a).unwrap();
    let playback_offset_a = i32::from_le_bytes(offset_bytes_a);

    file_b
        .seek(SeekFrom::Start(dir_offset_b as u64 + 4 + 92))
        .unwrap();
    let mut offset_bytes_b = [0u8; 4];
    file_b.seek(SeekFrom::Current(84)).unwrap();
    file_b.read_exact(&mut offset_bytes_b).unwrap();
    let playback_offset_b = i32::from_le_bytes(offset_bytes_b);

    println!(
        "Playback offset A: {}, Playback offset B: {}",
        playback_offset_a, playback_offset_b
    );

    file_a
        .seek(SeekFrom::Start(playback_offset_a as u64))
        .unwrap();
    file_b
        .seek(SeekFrom::Start(playback_offset_b as u64))
        .unwrap();

    let mut frame_count_a = 0;
    let mut frame_count_b = 0;

    let mut history_a: std::collections::VecDeque<FrameInfo> = std::collections::VecDeque::new();
    let mut history_b: std::collections::VecDeque<FrameInfo> = std::collections::VecDeque::new();

    let mut next_a = read_next_frame(&mut file_a, proto_a);
    let mut next_b = read_next_frame(&mut file_b, proto_b);

    loop {
        if next_a.is_none() || next_b.is_none() {
            if next_a.is_none() && next_b.is_none() {
                println!("Files are identical (ignoring injections)!");
            } else {
                println!("One file ended early!");
                println!("  Next A: {:?}", next_a.is_some());
                println!("  Next B: {:?}", next_b.is_some());
            }
            break;
        }

        let (info_a, payload_a) = next_a.clone().unwrap();
        let (info_b, payload_b) = next_b.clone().unwrap();

        // Lookahead up to 30 frames in B to find alignment
        let mut align_offset = None;
        if info_a.frame_type != info_b.frame_type
            || info_a.frame_num != info_b.frame_num
            || payload_a.len() != payload_b.len()
        {
            println!(
                "Mismatch at A #{} (pos {}) / B #{} (pos {}):",
                frame_count_a, info_a.pos, frame_count_b, info_b.pos
            );
            println!(
                "  Frame A: type={}, time={}, num={}, payload_len={}",
                info_a.frame_type,
                info_a.time,
                info_a.frame_num,
                payload_a.len()
            );
            if info_a.frame_type == 3 {
                println!(
                    "    Payload A: '{}'",
                    String::from_utf8_lossy(&payload_a).trim_matches('\0')
                );
            }
            println!(
                "  Frame B: type={}, time={}, num={}, payload_len={}",
                info_b.frame_type,
                info_b.time,
                info_b.frame_num,
                payload_b.len()
            );
            if info_b.frame_type == 3 {
                println!(
                    "    Payload B: '{}'",
                    String::from_utf8_lossy(&payload_b).trim_matches('\0')
                );
            }
            println!("  Searching lookahead...");
            let current_pos_b = file_b.stream_position().unwrap();
            let mut temp_file_b = std::fs::File::open(path_b).unwrap();
            temp_file_b.seek(SeekFrom::Start(current_pos_b)).unwrap();

            for k in 1..=30 {
                if let Some((peek_info_b, peek_payload_b)) =
                    read_next_frame(&mut temp_file_b, proto_b)
                {
                    if peek_info_b.frame_type == info_a.frame_type
                        && peek_info_b.frame_num == info_a.frame_num
                        && peek_payload_b.len() == payload_a.len()
                    {
                        println!(
                            "  Found lookahead match in B at +{} frames (pos {})",
                            k, peek_info_b.pos
                        );
                        align_offset = Some(k);
                        break;
                    }
                } else {
                    break;
                }
            }
            if align_offset.is_none() {
                println!("  No lookahead match found.");
            }
        }

        if let Some(k) = align_offset {
            for _ in 0..k {
                let current_b = next_b.clone().unwrap().0;
                history_b.push_back(current_b);
                if history_b.len() > 10 {
                    history_b.pop_front();
                }
                frame_count_b += 1;
                next_b = read_next_frame(&mut file_b, proto_b);
            }
            continue;
        }

        // Compare them
        if info_a.frame_type != info_b.frame_type {
            println!("DIVERGENCE DETECTED!");
            println!(
                "  Frame A (primer) #{} at pos {}: type={}, time={}, num={}",
                frame_count_a, info_a.pos, info_a.frame_type, info_a.time, info_a.frame_num
            );
            if info_a.frame_type == 3 {
                println!(
                    "    Payload A: '{}'",
                    String::from_utf8_lossy(&payload_a).trim_matches('\0')
                );
            }
            println!(
                "  Frame B (chain)  #{} at pos {}: type={}, time={}, num={}",
                frame_count_b, info_b.pos, info_b.frame_type, info_b.time, info_b.frame_num
            );
            if info_b.frame_type == 3 {
                println!(
                    "    Payload B: '{}'",
                    String::from_utf8_lossy(&payload_b).trim_matches('\0')
                );
            }

            // Print history A and B
            println!("  Last 10 frames in A:");
            for (h_idx, f) in history_a.iter().enumerate() {
                println!(
                    "    [{}] pos: {}, type: {}, time: {}, num: {}",
                    h_idx, f.pos, f.frame_type, f.time, f.frame_num
                );
            }
            println!("  Last 10 frames in B:");
            for (h_idx, f) in history_b.iter().enumerate() {
                println!(
                    "    [{}] pos: {}, type: {}, time: {}, num: {}",
                    h_idx, f.pos, f.frame_type, f.time, f.frame_num
                );
            }

            // Dump B lookahead
            let current_pos_b = file_b.stream_position().unwrap();
            let mut temp_file_b = std::fs::File::open(path_b).unwrap();
            temp_file_b.seek(SeekFrom::Start(current_pos_b)).unwrap();
            println!("  Next 30 frames in B:");
            for k in 1..=30 {
                if let Some((peek_info_b, peek_payload_b)) =
                    read_next_frame(&mut temp_file_b, proto_b)
                {
                    let payload_str = if peek_info_b.frame_type == 3 {
                        format!(
                            "cmd='{}'",
                            String::from_utf8_lossy(&peek_payload_b).trim_matches('\0')
                        )
                    } else {
                        format!("len={}", peek_payload_b.len())
                    };
                    println!(
                        "    [{}] pos={}, type={}, time={}, num={}, {}",
                        k,
                        peek_info_b.pos,
                        peek_info_b.frame_type,
                        peek_info_b.time,
                        peek_info_b.frame_num,
                        payload_str
                    );
                } else {
                    break;
                }
            }
            break;
        }

        if info_a.frame_num != info_b.frame_num {
            println!("TICK MISMATCH!");
            println!(
                "  Frame A (primer) #{} at pos {}: type={}, time={}, num={}",
                frame_count_a, info_a.pos, info_a.frame_type, info_a.time, info_a.frame_num
            );
            println!(
                "  Frame B (chain)  #{} at pos {}: type={}, time={}, num={}",
                frame_count_b, info_b.pos, info_b.frame_type, info_b.time, info_b.frame_num
            );
            break;
        }

        if payload_a.len() != payload_b.len() {
            println!("PAYLOAD LENGTH MISMATCH!");
            println!(
                "  Frame A (primer) #{} at pos {}: type={}, time={}, num={}, payload_len={}",
                frame_count_a,
                info_a.pos,
                info_a.frame_type,
                info_a.time,
                info_a.frame_num,
                payload_a.len()
            );
            println!(
                "  Frame B (chain)  #{} at pos {}: type={}, time={}, num={}, payload_len={}",
                frame_count_b,
                info_b.pos,
                info_b.frame_type,
                info_b.time,
                info_b.frame_num,
                payload_b.len()
            );
            break;
        }

        // Advance both
        history_a.push_back(info_a.clone());
        if history_a.len() > 10 {
            history_a.pop_front();
        }
        history_b.push_back(info_b.clone());
        if history_b.len() > 10 {
            history_b.pop_front();
        }

        frame_count_a += 1;
        frame_count_b += 1;
        next_a = read_next_frame(&mut file_a, proto_a);
        next_b = read_next_frame(&mut file_b, proto_b);
    }
}
