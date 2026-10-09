//! Demo integrity probe (#41): does a demo's delta chain hold together?
//!
//! A GoldSrc demo is a run of length-prefixed frames with no checksum and no
//! expected frame count, so a file that lost frames in the middle can still
//! parse cleanly here and then crash the engine on playback. What the engine
//! cannot survive is a broken *delta chain*: most of a network frame is encoded
//! as a difference against an earlier frame, and if that earlier frame is not
//! in the file the engine decodes against whatever stale state sits in its
//! frame ring.
//!
//! This walks every frame itself, so a byte-level failure reports how far it
//! got rather than just "cannot parse", and checks:
//!
//! - **Structure**: the frame walk reaches the directory exactly, and each
//!   directory entry's `frame_offset` / `file_length` match what is there.
//! - **Packet-entity delta chain** (the load-bearing one):
//!   `SvcDeltaPacketEntities` names the frame it is a delta from by the low 8
//!   bits of that frame's `incoming_sequence`.
//!   - The engine discards the packet outright when
//!     `((incoming_sequence - delta_sequence) & 0xFF) >= 63`
//!     (`CL_FlushEntityPacket`, see `docs/goldsrc_demo_salvage.md`): missing
//!     world, but no bad decode.
//!   - Otherwise it decodes against ring slot `delta_sequence & 63`. That frame
//!     must actually be in the file, and must have carried packet entities; if
//!     it is not, the engine decodes against stale state. This is the case a
//!     dropped-frames copy produces, and the one `flush_predict` cannot see.
//!   - Applying the delta to that frame's entity set must give exactly the
//!     `entity_count` the message declares (it declares the size of the
//!     *resulting* snapshot).
//! - **Client-data delta chain**: `SvcClientData`'s optional 8-bit from-frame
//!   must be a frame in the file that carried client data. Not checked for HLTV
//!   demos, whose client data `dem-patch` does not parse.
//! - **Prerequisites**: `SvcDeltaDescription` and `SvcSpawnBaseline` arrive
//!   before anything decodes against them. (`dem-patch` already refuses to
//!   decode with a missing delta description, so that half surfaces as a
//!   message parse failure.)
//! - **Hints, not verdicts**: `incoming_sequence` gaps inside a segment (real
//!   packet loss makes them too), frame time / `SvcTime` stepping backwards,
//!   and the largest forward time jump (the "cheap heuristic" from #41).
//!
//! `--skip A..B` leaves walk frames A..B out of the checks (nothing is
//! written), to show what a demo missing those frames would look like.
//!
//! Read-only. Usage:
//!
//!     cargo run --release -p native --example demo_integrity -- a.dem [b.dem ...]
//!     cargo run --release -p native --example demo_integrity -- --skip 200000..300000 a.dem

use clap::Parser;
use dem::bit::BitSliceCast;
use dem::demo_parser::{parse_frame, parse_header};
use dem::types::{
    Aux, EngineMessage, Frame, FrameData, MessageData, MessageDataParseMode, NetMessage,
};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// `CL_UPDATE_BACKUP - 1`: the engine flushes a delta whose from-frame is this
/// many packets back or more (`hw.dll` .data `0x1e3afcc`).
const FLUSH_LIMIT: i32 = 63;

/// How many example issues to keep per check.
const MAX_EXAMPLES: usize = 5;

#[derive(Parser, Debug)]
#[command(about = "Checks a GoldSrc demo's frame structure and delta continuity (#41).")]
struct Args {
    /// Leave walk frames A..B out of the checks, simulating a copy that lost
    /// them. May be repeated.
    #[arg(long, value_parser = parse_range)]
    skip: Vec<Range<u64>>,

    /// One or more .dem files
    #[arg(required = true)]
    files: Vec<PathBuf>,
}

fn parse_range(s: &str) -> Result<Range<u64>, String> {
    let (a, b) = s.split_once("..").ok_or("expected A..B")?;
    let a = a.parse().map_err(|e| format!("{e}"))?;
    let b = b.parse().map_err(|e| format!("{e}"))?;
    Ok(a..b)
}

/// One frame that carried packet entities (or client data), kept so later
/// deltas can be checked against it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChainFrame {
    sequence: i32,
    entities: BTreeSet<u16>,
    /// Built on top of a broken delta, so its entity set is a guess. Later
    /// deltas from it still count as chained, but their entity count is not
    /// checked: one missing frame should be reported once, not cascade.
    tainted: bool,
}

/// What a packet-entities message says about one entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityOp {
    Update(u16),
    Remove(u16),
}

/// How the engine would treat a delta, and whether the file can back it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DeltaLink<'a> {
    /// The from-frame is in the file and inside the ring.
    Ok(&'a ChainFrame),
    /// The engine discards the packet (`CL_FlushEntityPacket`).
    Flushed { gap: i32 },
    /// The engine accepts the packet but the frame it means is not in the
    /// file: it decodes against stale ring state. `found` is the sequence of
    /// the frame that does hold those low 8 bits, if any.
    Absent { from: u8, found: Option<i32> },
}

/// The pure part of the check: recent frames keyed by the low 8 bits of their
/// sequence, as the wire format keys them.
#[derive(Debug, Default)]
struct DeltaChain {
    frames: HashMap<u8, ChainFrame>,
}

impl DeltaChain {
    /// Resolves an 8-bit from-sequence as the engine would at `current`.
    fn resolve(&self, current: i32, from: u8) -> DeltaLink<'_> {
        let gap = current.wrapping_sub(from as i32) & 0xFF;
        if gap >= FLUSH_LIMIT {
            return DeltaLink::Flushed { gap };
        }
        // The frame the server meant: `gap` packets back, with these low bits.
        let meant = current.wrapping_sub(gap);
        match self.frames.get(&from) {
            Some(frame) if frame.sequence == meant => DeltaLink::Ok(frame),
            other => DeltaLink::Absent {
                from,
                found: other.map(|f| f.sequence),
            },
        }
    }

    fn record(&mut self, sequence: i32, entities: BTreeSet<u16>, tainted: bool) {
        self.frames.insert(
            sequence as u8,
            ChainFrame {
                sequence,
                entities,
                tainted,
            },
        );
    }
}

/// Applies a delta's operations to its from-frame's entity set. Returns the new
/// set and how many removals named an entity the from-frame did not have.
fn apply_delta(from: &BTreeSet<u16>, ops: &[EntityOp]) -> (BTreeSet<u16>, usize) {
    let mut set = from.clone();
    let mut unknown_removals = 0;
    for op in ops {
        match *op {
            EntityOp::Update(index) => {
                set.insert(index);
            }
            EntityOp::Remove(index) => {
                if !set.remove(&index) {
                    unknown_removals += 1;
                }
            }
        }
    }
    (set, unknown_removals)
}

/// A counter plus the first few places it fired.
#[derive(Debug, Default)]
struct Tally {
    count: u64,
    examples: Vec<String>,
}

impl Tally {
    fn hit(&mut self, example: impl FnOnce() -> String) {
        self.count += 1;
        if self.examples.len() < MAX_EXAMPLES {
            self.examples.push(example());
        }
    }
}

/// The largest value seen, and where.
#[derive(Debug, Default)]
struct Peak {
    value: f64,
    at: String,
}

impl Peak {
    fn offer(&mut self, value: f64, at: impl FnOnce() -> String) {
        if value > self.value {
            self.value = value;
            self.at = at();
        }
    }
}

#[derive(Debug, Default)]
struct Report {
    file_len: usize,
    directory_offset: i32,
    directory: Vec<DirEntry>,

    frames: u64,
    net_frames: u64,
    skipped: u64,
    segments: Vec<Segment>,
    walk_end: usize,
    walk_error: Option<String>,
    netmsg_parse_failures: Tally,

    is_hltv: bool,
    delta_descriptions: u64,
    server_infos: u64,
    baselines: u64,
    first_baseline: Option<u64>,

    seq_repeats: u64,
    seq_backwards: Tally,
    seq_gaps: u64,
    seq_lost: u64,
    seq_max_gap: Peak,

    full_packets: u64,
    full_count_mismatch: Tally,
    delta_packets: u64,
    delta_linked: u64,
    delta_count_ok: u64,
    delta_flushed: Tally,
    delta_absent: Tally,
    delta_count_mismatch: Tally,
    delta_unknown_removal: Tally,
    delta_before_baseline: Tally,

    clientdata_deltas: u64,
    clientdata_ok: u64,
    clientdata_absent: Tally,

    time_backwards: Tally,
    time_max_jump: Peak,
    svc_time_backwards: Tally,
    svc_time_max_jump: Peak,
}

#[derive(Debug)]
struct DirEntry {
    type_: i32,
    description: String,
    frame_count: i32,
    frame_offset: i32,
    file_length: i32,
}

#[derive(Debug, Default)]
struct Segment {
    start: usize,
    end: usize,
    frames: u64,
    net_frames: u64,
    first_time: f32,
    last_time: f32,
}

/// Everything the walk carries from one frame to the next.
#[derive(Default)]
struct WalkState {
    last_seq: Option<i32>,
    segment_start: bool,
    /// Past entry 0. Its frame times are the recorder's own clock and jump by
    /// minutes in every healthy demo, so the time hints skip it.
    past_loading: bool,
    last_time: Option<f32>,
    last_svc_time: Option<f32>,
    entities: DeltaChain,
    clientdata: DeltaChain,
}

fn le_i32(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Reads the directory by hand: a count, then 92-byte entries.
fn read_directory(bytes: &[u8], offset: i32) -> Vec<DirEntry> {
    let mut out = vec![];
    if offset <= 0 {
        return out;
    }
    let offset = offset as usize;
    let Some(count) = le_i32(bytes, offset) else {
        return out;
    };
    for n in 0..count.clamp(0, 64) as usize {
        let at = offset + 4 + n * 92;
        let (Some(type_), Some(desc)) = (le_i32(bytes, at), bytes.get(at + 4..at + 68)) else {
            break;
        };
        out.push(DirEntry {
            type_,
            description: cstr(desc),
            frame_count: le_i32(bytes, at + 80).unwrap_or(-1),
            frame_offset: le_i32(bytes, at + 84).unwrap_or(-1),
            file_length: le_i32(bytes, at + 88).unwrap_or(-1),
        });
    }
    out
}

fn check(bytes: &[u8], skip: &[Range<u64>]) -> Result<Report, String> {
    let (rest, header) = parse_header(bytes).map_err(|e| format!("header: {e:?}"))?;
    let mut report = Report {
        file_len: bytes.len(),
        directory_offset: header.directory_offset,
        directory: read_directory(bytes, header.directory_offset),
        ..Default::default()
    };

    let dir = header.directory_offset;
    let end = if dir > 0 && (dir as usize) <= bytes.len() {
        dir as usize
    } else {
        bytes.len()
    };
    let mut pos = bytes.len() - rest.len();
    let aux = Aux::new2();
    let mut state = WalkState::default();
    let mut segment = Segment {
        start: pos,
        end: pos,
        ..Default::default()
    };

    while pos < end {
        let input = &bytes[pos..end];
        let parsed = match parse_frame(input, MessageDataParseMode::Parse, aux.clone()) {
            Ok(ok) => Ok(ok),
            Err(parse_err) => {
                // The frame is intact but a message inside it is not: keep the
                // frame (and its sequence numbers) and carry on.
                let raw = parse_frame(input, MessageDataParseMode::Raw, aux.clone());
                if raw.is_ok() {
                    let idx = report.frames;
                    report
                        .netmsg_parse_failures
                        .hit(|| format!("frame #{idx} @ byte {pos}: {}", nom_reason(&parse_err)));
                }
                raw
            }
        };
        let (rest, frame) = match parsed {
            Ok(ok) => ok,
            Err(e) => {
                report.walk_error = Some(format!(
                    "frame #{} @ byte {pos} ({:.1}% of file): {}",
                    report.frames,
                    pos as f64 * 100.0 / bytes.len() as f64,
                    nom_reason(&e)
                ));
                break;
            }
        };
        let next = end - rest.len();
        let is_section_end = matches!(frame.frame_data, FrameData::NextSection);

        if segment.frames == 0 {
            segment.first_time = frame.time;
        }
        segment.frames += 1;
        segment.last_time = frame.time;
        segment.end = next;
        if matches!(frame.frame_data, FrameData::NetworkMessage(_)) {
            segment.net_frames += 1;
        }

        if skip.iter().any(|r| r.contains(&report.frames)) {
            report.skipped += 1;
        } else {
            visit(&mut report, &mut state, &frame, aux.borrow().is_hltv);
        }
        report.frames += 1;
        pos = next;

        if is_section_end {
            let done = std::mem::replace(
                &mut segment,
                Segment {
                    start: pos,
                    end: pos,
                    ..Default::default()
                },
            );
            // A segment can end on a run of section-end frames
            // (`docs/goldsrc_demo_salvage.md`); fold the extras into it.
            match report.segments.last_mut() {
                Some(prev) if done.frames == 1 => {
                    prev.end = done.end;
                    prev.frames += 1;
                }
                _ => report.segments.push(done),
            }
            // Time restarts per segment; the sequence does not.
            state.last_time = None;
            state.last_svc_time = None;
            state.segment_start = true;
            state.past_loading = true;
        }
    }
    if segment.frames > 0 {
        report.segments.push(segment);
    }
    report.walk_end = pos;
    report.is_hltv = aux.borrow().is_hltv;
    Ok(report)
}

/// The text a `nom_fail` carries, rather than its byte dump. `dem-patch` puts
/// its message in the error's `input`, and `native` has no direct `nom`
/// dependency to name the error type with, so this reads it back out of the
/// `Debug` form. Anything not shaped like that comes back as it is.
fn nom_reason(e: &impl std::fmt::Debug) -> String {
    let debug = format!("{e:?}");
    let bytes = debug
        .split_once("input: [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| {
            list.split(", ")
                .map(|n| n.trim().parse::<u8>())
                .collect::<Result<Vec<u8>, _>>()
        });
    match bytes {
        Some(Ok(bytes)) if !bytes.is_empty() && bytes.len() <= 200 => {
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => debug.chars().take(200).collect(),
    }
}

fn visit(report: &mut Report, state: &mut WalkState, frame: &Frame, is_hltv: bool) {
    let idx = report.frames;

    if let Some(prev) = state.last_time
        && state.past_loading
    {
        if frame.time < prev {
            report
                .time_backwards
                .hit(|| format!("frame #{idx}: {prev:.3}s -> {:.3}s", frame.time));
        } else {
            report.time_max_jump.offer((frame.time - prev) as f64, || {
                format!("frame #{idx} at {prev:.2}s")
            });
        }
    }
    state.last_time = Some(frame.time);

    let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
        return;
    };
    let net = &boxed.1;
    report.net_frames += 1;
    let seq = net.sequence_info.incoming_sequence;

    if let Some(prev) = state.last_seq
        && !std::mem::take(&mut state.segment_start)
    {
        let step = seq.wrapping_sub(prev);
        if step == 0 {
            report.seq_repeats += 1;
        } else if step < 0 {
            report
                .seq_backwards
                .hit(|| format!("frame #{idx}: {prev} -> {seq}"));
        } else if step > 1 {
            report.seq_gaps += 1;
            report.seq_lost += (step - 1) as u64;
            report.seq_max_gap.offer((step - 1) as f64, || {
                format!("frame #{idx}: {prev} -> {seq}")
            });
        }
    }
    state.last_seq = Some(seq);

    let MessageData::Parsed(messages) = &net.messages else {
        return;
    };
    for message in messages {
        let NetMessage::EngineMessage(engine) = message else {
            continue;
        };
        match engine.as_ref() {
            EngineMessage::SvcServerInfo(_) => report.server_infos += 1,
            EngineMessage::SvcDeltaDescription(_) => report.delta_descriptions += 1,
            EngineMessage::SvcSpawnBaseline(_) => {
                report.baselines += 1;
                report.first_baseline.get_or_insert(idx);
            }
            EngineMessage::SvcTime(t) => {
                if let Some(prev) = state.last_svc_time {
                    if t.time < prev {
                        report
                            .svc_time_backwards
                            .hit(|| format!("frame #{idx}: {prev:.3}s -> {:.3}s", t.time));
                    } else {
                        report.svc_time_max_jump.offer((t.time - prev) as f64, || {
                            format!("frame #{idx} at {prev:.2}s")
                        });
                    }
                }
                state.last_svc_time = Some(t.time);
            }
            EngineMessage::SvcPacketEntities(p) => {
                report.full_packets += 1;
                let set: BTreeSet<u16> = p.entity_states.iter().map(|e| e.entity_index).collect();
                let declared = p.entity_count.to_u16() as usize;
                if declared != set.len() {
                    report.full_count_mismatch.hit(|| {
                        format!(
                            "frame #{idx} seq {seq}: declares {declared}, lists {}",
                            set.len()
                        )
                    });
                }
                state.entities.record(seq, set, false);
            }
            EngineMessage::SvcDeltaPacketEntities(p) => {
                report.delta_packets += 1;
                if report.first_baseline.is_none() {
                    report
                        .delta_before_baseline
                        .hit(|| format!("frame #{idx} seq {seq}"));
                }
                let ops: Vec<EntityOp> = p
                    .entity_states
                    .iter()
                    .map(|e| {
                        if e.remove_entity {
                            EntityOp::Remove(e.entity_index)
                        } else {
                            EntityOp::Update(e.entity_index)
                        }
                    })
                    .collect();
                let declared = p.entity_count.to_u16() as usize;
                match state.entities.resolve(seq, p.delta_sequence.to_u8()) {
                    DeltaLink::Ok(base) => {
                        report.delta_linked += 1;
                        let (set, unknown) = apply_delta(&base.entities, &ops);
                        let (base_seq, tainted) = (base.sequence, base.tainted);
                        if !tainted {
                            if unknown > 0 {
                                report.delta_unknown_removal.hit(|| {
                                    format!(
                                        "frame #{idx} seq {seq} from {base_seq}: {unknown} removals"
                                    )
                                });
                            }
                            if declared != set.len() {
                                report.delta_count_mismatch.hit(|| {
                                    format!(
                                        "frame #{idx} seq {seq} from {base_seq}: declares {declared}, gets {}",
                                        set.len()
                                    )
                                });
                            } else {
                                report.delta_count_ok += 1;
                            }
                        }
                        state.entities.record(seq, set, tainted);
                    }
                    DeltaLink::Flushed { gap } => {
                        // The engine drops it; nothing new enters the ring.
                        report
                            .delta_flushed
                            .hit(|| format!("frame #{idx} seq {seq}: gap {gap}"));
                    }
                    DeltaLink::Absent { from, found } => {
                        report.delta_absent.hit(|| {
                            format!(
                                "frame #{idx} seq {seq}: from-frame ..{from:02x} not in file (slot holds {found:?})"
                            )
                        });
                        let set = apply_delta(&BTreeSet::new(), &ops).0;
                        state.entities.record(seq, set, true);
                    }
                }
            }
            EngineMessage::SvcClientData(c) if !is_hltv => {
                if let Some(mask) = &c.delta_update_mask {
                    report.clientdata_deltas += 1;
                    match state.clientdata.resolve(seq, mask.to_u8()) {
                        DeltaLink::Ok(_) => report.clientdata_ok += 1,
                        other => report
                            .clientdata_absent
                            .hit(|| format!("frame #{idx} seq {seq}: {other:?}")),
                    }
                }
                state.clientdata.record(seq, BTreeSet::new(), false);
            }
            _ => {}
        }
    }
}

/// Empty for a demo that should play, else the reasons it should not.
fn verdict(r: &Report) -> Vec<String> {
    let mut why = vec![];
    if let Some(e) = &r.walk_error {
        why.push(format!("frame walk stopped: {e}"));
    } else if r.directory_offset > 0 && r.walk_end != r.directory_offset as usize {
        why.push(format!(
            "frames end at byte {} but the directory starts at {}",
            r.walk_end, r.directory_offset
        ));
    }
    if r.netmsg_parse_failures.count > 0 {
        why.push(format!(
            "{} network frames failed to parse",
            r.netmsg_parse_failures.count
        ));
    }
    if r.delta_absent.count > 0 {
        why.push(format!(
            "{} packet-entity deltas decode against a frame the file does not have",
            r.delta_absent.count
        ));
    }
    if r.delta_flushed.count > 0 {
        why.push(format!(
            "{} packet-entity deltas the engine will discard (CL_FlushEntityPacket)",
            r.delta_flushed.count
        ));
    }
    if r.delta_count_mismatch.count > 0 {
        why.push(format!(
            "{} packet-entity deltas give the wrong entity count",
            r.delta_count_mismatch.count
        ));
    }
    if r.clientdata_absent.count > 0 {
        why.push(format!(
            "{} client-data deltas from a frame the file does not have",
            r.clientdata_absent.count
        ));
    }
    if r.delta_before_baseline.count > 0 {
        why.push("packet entities before any SvcSpawnBaseline".into());
    }
    // Only meaningful when the walk covered the whole file.
    if r.walk_error.is_none() {
        for (seg, dir) in r.segments.iter().zip(&r.directory) {
            let len = (seg.end - seg.start) as i64;
            // The engine counts network frames, and writes 0 for entry 0.
            if dir.frame_count > 0 && dir.frame_count as u64 != seg.net_frames {
                why.push(format!(
                    "directory entry {} claims {} network frames, walk found {}",
                    dir.description, dir.frame_count, seg.net_frames
                ));
            }
            if dir.frame_offset as i64 != seg.start as i64 || dir.file_length as i64 != len {
                why.push(format!(
                    "directory entry {} claims {}+{} bytes, walk found {}+{}",
                    dir.description, dir.frame_offset, dir.file_length, seg.start, len
                ));
            }
        }
    }
    why
}

fn print_tally(name: &str, t: &Tally) {
    println!("  {name:<44} {}", t.count);
    for e in &t.examples {
        println!("      {e}");
    }
}

fn print(path: &Path, r: &Report) {
    println!("== {}", path.display());
    println!(
        "  size {} B, directory @ {}, hltv {}",
        r.file_len, r.directory_offset, r.is_hltv
    );
    for d in &r.directory {
        println!(
            "  dir  type {} {:<10} frame_count {:>7} offset {:>10} length {:>10}",
            d.type_, d.description, d.frame_count, d.frame_offset, d.file_length
        );
    }
    for s in &r.segments {
        println!(
            "  walk bytes {:>10}..{:<10} frames {:>7} (network {:>7}) time {:.2}s..{:.2}s",
            s.start, s.end, s.frames, s.net_frames, s.first_time, s.last_time
        );
    }
    println!(
        "  frames {} (network {}, skipped {}), walk ended @ {}",
        r.frames, r.net_frames, r.skipped, r.walk_end
    );
    if let Some(e) = &r.walk_error {
        println!("  walk error: {e}");
    }
    print_tally("network frames failing to parse", &r.netmsg_parse_failures);
    println!(
        "  server infos {}, delta descriptions {}, baselines {} (first at frame #{:?})",
        r.server_infos, r.delta_descriptions, r.baselines, r.first_baseline
    );
    println!(
        "  packet entities: full {}, delta {} (from-frame in file {}, entity count verified {})",
        r.full_packets, r.delta_packets, r.delta_linked, r.delta_count_ok
    );
    print_tally("full packet count mismatch", &r.full_count_mismatch);
    print_tally("delta from a frame not in the file", &r.delta_absent);
    print_tally("delta the engine discards", &r.delta_flushed);
    print_tally("delta entity count mismatch", &r.delta_count_mismatch);
    print_tally(
        "delta removes an entity not there",
        &r.delta_unknown_removal,
    );
    print_tally("delta before baseline", &r.delta_before_baseline);
    println!(
        "  client data: deltas {}, from-frame in file {}",
        r.clientdata_deltas, r.clientdata_ok
    );
    print_tally(
        "client-data delta from a frame not in file",
        &r.clientdata_absent,
    );
    println!("  hints:");
    println!(
        "  sequence in-segment: repeats {}, gaps {} ({} packets), largest {} at {}",
        r.seq_repeats, r.seq_gaps, r.seq_lost, r.seq_max_gap.value, r.seq_max_gap.at
    );
    print_tally("sequence going backwards", &r.seq_backwards);
    print_tally("frame time backwards", &r.time_backwards);
    println!(
        "  frame time largest jump {:.3}s at {}",
        r.time_max_jump.value, r.time_max_jump.at
    );
    print_tally("SvcTime backwards", &r.svc_time_backwards);
    println!(
        "  SvcTime largest jump {:.3}s at {}",
        r.svc_time_max_jump.value, r.svc_time_max_jump.at
    );

    let why = verdict(r);
    if why.is_empty() {
        println!("  VERDICT: OK");
    } else {
        println!("  VERDICT: BROKEN");
        for w in why {
            println!("    - {w}");
        }
    }
    println!();
}

fn main() {
    let args = Args::parse();
    for path in &args.files {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                continue;
            }
        };
        match check(&bytes, &args.skip) {
            Ok(report) => print(path, &report),
            Err(e) => println!("== {}\n  not a demo: {e}\n", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[u16]) -> BTreeSet<u16> {
        items.iter().copied().collect()
    }

    #[test]
    fn resolve_finds_a_recent_frame() {
        let mut chain = DeltaChain::default();
        chain.record(1000, set(&[1, 2]), false);
        let DeltaLink::Ok(frame) = chain.resolve(1003, 1000u32 as u8) else {
            panic!("expected a link");
        };
        assert_eq!(frame.sequence, 1000);
    }

    #[test]
    fn resolve_reports_a_frame_the_file_does_not_have() {
        let mut chain = DeltaChain::default();
        chain.record(1000, set(&[1]), false);
        assert_eq!(
            chain.resolve(1003, 1001u32 as u8),
            DeltaLink::Absent {
                from: 1001u32 as u8,
                found: None
            }
        );
    }

    #[test]
    fn resolve_rejects_a_slot_left_over_from_an_earlier_lap() {
        // Same low byte, 256 packets earlier: the server meant seq 1250, which
        // the file no longer has, and the ring slot still holds 994.
        let mut chain = DeltaChain::default();
        chain.record(994, set(&[1]), false);
        assert_eq!(
            chain.resolve(1253, 1250u32 as u8),
            DeltaLink::Absent {
                from: 1250u32 as u8,
                found: Some(994)
            }
        );
    }

    #[test]
    fn resolve_matches_the_engine_flush_predicate() {
        let mut chain = DeltaChain::default();
        chain.record(1000, set(&[1]), false);
        // 62 back is kept, 63 back is flushed (`>= CL_UPDATE_BACKUP - 1`).
        assert!(matches!(
            chain.resolve(1062, 1000u32 as u8),
            DeltaLink::Ok(_)
        ));
        assert_eq!(
            chain.resolve(1063, 1000u32 as u8),
            DeltaLink::Flushed { gap: 63 }
        );
        // The predicate works on the low byte: 255 back wraps to a gap of 255.
        assert_eq!(
            chain.resolve(1255, 1000u32 as u8),
            DeltaLink::Flushed { gap: 255 }
        );
    }

    #[test]
    fn apply_delta_adds_removes_and_counts_unknown_removals() {
        let (out, unknown) = apply_delta(
            &set(&[1, 2, 3]),
            &[
                EntityOp::Remove(2),
                EntityOp::Update(9),
                EntityOp::Remove(40),
            ],
        );
        assert_eq!(out, set(&[1, 3, 9]));
        assert_eq!(unknown, 1);
    }

    #[test]
    fn nom_reason_reads_the_message_out_of_the_debug_form() {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct Error {
            input: Vec<u8>,
            code: u8,
        }
        let e = Error {
            input: b"message length too long: 9".to_vec(),
            code: 0,
        };
        assert_eq!(nom_reason(&e), "message length too long: 9");
        assert_eq!(nom_reason(&"plain"), "\"plain\"");
    }
}
