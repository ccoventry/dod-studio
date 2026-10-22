//! Preview patches: the playdemo-safe name and the director messages a
//! preview injects.

use super::*;

/// Makes a demo stem safe as a `playdemo`/`viewdemo` target.
///
/// `launch_demo_preview` passes the output stem straight into HLAE's
/// `-cmdLine`, which becomes hl.exe's own startup command line — GoldSrc
/// tokenizes that on whitespace and treats any `+`/`-` prefixed token as the
/// start of a new launch parm, so an embedded hyphen (common in
/// match-recorded demo names, e.g. "team1-vs-team2") silently truncates the
/// `+viewdemo` target at the first one instead of failing loudly. Confirmed
/// live: a source stem of "wsod25-po_r3_sf-..." loaded as bare "wsod25".
/// Launch-line playdemo/viewdemo targets may also need to stay under ~40
/// characters: the console takes longer names, but the launch line is
/// untested (docs/goldsrc_dod_quirks.md, #376). This stem was already over that before appending
/// "_preview" — both constraints are enforced here, once, at the point the
/// output filename is chosen, so neither preview entry point (this one, or
/// `generate_all_previews`'s later manual load) can hit it again.
pub fn playdemo_safe_stem(raw: &str) -> String {
    // Reserve room for the "_preview" suffix appended below, and stay a few
    // characters under the documented ~40 char limit rather than right at it.
    const SUFFIX_LEN: usize = "_preview".len();
    const BUDGET: usize = 36;
    const BASE_BUDGET: usize = BUDGET - SUFFIX_LEN;

    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.len() <= BASE_BUDGET {
        return sanitized;
    }
    // Too long even after sanitizing — truncate, but append a hash of the
    // *original* stem so two long names sharing a prefix don't collide onto
    // the same output file.
    let hash_suffix = format!(
        "_{:08x}",
        crate::utils::demo_hasher::fnv1a_hash(raw.as_bytes()) as u32
    );
    let keep = BASE_BUDGET.saturating_sub(hash_suffix.len());
    format!("{}{}", &sanitized[..keep], hash_suffix)
}

pub fn build_preview_patch_jobs(
    raw_streaks: Vec<CaptureStreak>,
    output_dir: Option<&std::path::Path>,
) -> Vec<PatchJob> {
    // Group all streaks by source demo path.
    let mut grouped: std::collections::HashMap<String, Vec<CaptureStreak>> =
        std::collections::HashMap::new();
    for streak in raw_streaks {
        grouped
            .entry(streak.source_demo.clone())
            .or_default()
            .push(streak);
    }

    let mut jobs = Vec::new();

    for (source_demo, mut streaks) in grouped {
        // Sort chronologically.
        streaks.sort_by_key(|s| s.start_tick);

        // Build (tick, label) for each streak — same format as the highlight table.
        let mut director_events: Vec<(i32, String)> = streaks
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let label = format!(
                    "#{}/{}: {} kills: {}",
                    i + 1,
                    streaks.len(),
                    s.kill_count,
                    s.timeline_string
                );
                let preview_tick =
                    find_tick_backwards(s.start_tick as usize, 3.0, &s.frame_times, s.demo_fps);
                (preview_tick, label)
            })
            .collect();

        if let Some(first_streak) = streaks.first() {
            let match_frame_idx = 0; // Float time unavailable for match start
            director_events.push((match_frame_idx, "echo [dod-studio] MATCH_START".to_string()));
            let total_demo_frames = if first_streak.total_demo_frames > 0 {
                first_streak.total_demo_frames
            } else {
                first_streak.frame_times.len() as i32
            };
            let demo_end_tick = total_demo_frames;
            director_events.push((demo_end_tick, "echo [dod-studio] DEMO_END".to_string()));
        }
        director_events.sort_by_key(|e| e.0);

        // Resolve output path: "<stem>_preview.dem" beside original, or in output_dir.
        let source_path = std::path::PathBuf::from(&source_demo);
        let stem = source_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let safe_stem = playdemo_safe_stem(&stem);
        let preview_name = format!("{}_preview.dem", safe_stem);
        let output_demo = if let Some(dir) = output_dir {
            dir.join(&preview_name)
        } else {
            source_path.with_file_name(&preview_name)
        };

        jobs.push(PatchJob {
            source_demo,
            output_demo,
            streaks,
            target_player: None,
            // No capture init commands — preview only.
            init_commands: Vec::new(),
            // No scheduled capture commands — preview only.
            scheduled_commands: Vec::new(),
            director_events,
            block_routes: Vec::new(),
            blocks: Vec::new(),
        });
    }

    jobs
}

// ── svc_director payload builder ──────────────────────────────────────────────

/// Build a GoldSrc `svc_director` (OpCode 0x33) net-message payload for a
/// `DRC_CMD_MESSAGE` (sub-command 0x06) HLTV title card.
///
/// The returned `Vec<u8>` is a self-contained net-message body ready to be
/// embedded inside a `Dem_NetworkBuffer` (frame type 0x00 / 0x01) payload.
///
/// Fixed wire layout (30 bytes before the text):
///
/// | Offset | Size | Value       | Meaning                 |
/// |--------|------|-------------|-------------------------|
/// | 0      | 1    | 0x33        | svc_director opcode     |
/// | 1      | 1    | payload_len | total bytes after opcode|
/// | 2      | 1    | 0x06        | DRC_CMD_MESSAGE         |
/// | 3      | 1    | 0x00        | effect (none)           |
/// | 4      | 4    | FF A0 00 00 | RGBA colour #FFA000FF   |
/// | 8      | 4    | -1.0 f32 LE | position X (centered)   |
/// | 12     | 4    | 0.85 f32 LE | position Y              |
/// | 16     | 4    | 0.5  f32 LE | fade-in  (seconds)      |
/// | 20     | 4    | 0.5  f32 LE | fade-out (seconds)      |
/// | 24     | 4    | 3.0  f32 LE | hold time (seconds)     |
/// | 28     | 4    | 0.0  f32 LE | FX time                 |
/// | 32     | N+1  | text + \0   | null-terminated string  |
///
/// `payload_len` = 30 (fields 2-31) + text_len + 1 (null), capped at 255.
pub fn build_director_message(text: &str) -> Vec<u8> {
    // Null-terminate and clamp so payload_len fits in one byte.
    // payload_len covers everything from the sub-command byte (offset 2) to the
    // end of the null-terminated string, i.e. 30 fixed bytes + string + NUL.
    // Maximum payload_len = 255, so maximum text bytes = 255 - 30 - 1 = 224.
    const FIXED_OVERHEAD: usize = 30; // bytes 2..31 (sub-cmd through FX time)
    const MAX_TEXT_BYTES: usize = 255 - FIXED_OVERHEAD - 1; // 224

    let raw = text.as_bytes();
    let text_len = raw.len().min(MAX_TEXT_BYTES);
    let text_bytes = &raw[..text_len];

    // payload_len is everything after the opcode and length byte itself.
    let payload_len: u8 = (FIXED_OVERHEAD + text_len + 1) as u8;

    let mut msg: Vec<u8> = Vec::with_capacity(2 + FIXED_OVERHEAD + text_len + 1);

    // Opcode + payload length
    msg.push(0x33); // svc_director
    msg.push(payload_len);

    // Sub-command and effect
    msg.push(0x06); // DRC_CMD_MESSAGE
    msg.push(0x00); // effect: none

    // RGBA colour #FFA000FF
    msg.extend_from_slice(&[0xFF, 0xA0, 0x00, 0x00]);

    // Position (X = -1.0 → engine centers horizontally; Y = 0.85)
    msg.extend_from_slice(&(-1.0f32).to_le_bytes());
    msg.extend_from_slice(&(0.85f32).to_le_bytes());

    // Timing
    msg.extend_from_slice(&(0.5f32).to_le_bytes()); // fade in
    msg.extend_from_slice(&(0.5f32).to_le_bytes()); // fade out
    msg.extend_from_slice(&(3.0f32).to_le_bytes()); // hold time
    msg.extend_from_slice(&(0.0f32).to_le_bytes()); // FX time

    // Null-terminated text payload
    msg.extend_from_slice(text_bytes);
    msg.push(0x00);

    msg
}

/// Build a GoldSrc `svc_director` (OpCode 0x33) net-message for a
/// `DRC_CMD_STUFFTEXT` (sub-command 0x0A) executable command.
///
/// The engine executes `command` on the client console when the event fires
/// in the `viewdemo` event list. The returned `Vec<u8>` is a self-contained
/// net-message body ready to embed inside a `Dem_NetworkBuffer` frame.
///
/// Wire layout:
///
/// | Offset | Size | Value        | Meaning              |
/// |--------|------|--------------|----------------------|
/// | 0      | 1    | 0x33         | svc_director opcode  |
/// | 1      | 1    | payload_len  | 1 + text_len + 1     |
/// | 2      | 1    | 0x0A         | DRC_CMD_STUFFTEXT    |
/// | 3      | N    | command      | raw command string   |
/// | 3+N    | 1    | 0x00         | null terminator      |
///
/// Maximum `command` length is 253 bytes (keeps `payload_len` ≤ 255).
pub fn build_director_stufftext(command: &str) -> Vec<u8> {
    const MAX_TEXT_BYTES: usize = 253; // keeps payload_len <= 255

    let raw = command.as_bytes();
    let text_len = raw.len().min(MAX_TEXT_BYTES);
    let text_bytes = &raw[..text_len];

    // payload_len = sub-command byte (1) + text + NUL
    let payload_len: u8 = (1 + text_len + 1) as u8;

    let mut msg: Vec<u8> = Vec::with_capacity(2 + 1 + text_len + 1);
    msg.push(0x33); // svc_director
    msg.push(payload_len);
    msg.push(0x0A); // DRC_CMD_STUFFTEXT
    msg.extend_from_slice(text_bytes);
    msg.push(0x00); // null terminator
    msg
}

// ── Tests ─────────────────────────────────────────────────────────────────────
