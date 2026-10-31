//! Demo Analyzer load-path profiler.
//!
//! Profiles the phases the Tauri `analyze_demo_full` command actually pays for
//! when the Demo Analyzer opens a demo, so "it takes a few seconds" can be
//! attributed rather than guessed at:
//!
//!   read        fs::read of the whole .dem into a `Vec<u8>`
//!   structural  Demo::parse_from_bytes(None) — walk directory/frames only
//!   decode      Demo::parse_from_bytes(Parse) — decode every netmessage
//!   events      the AnalyzerState event loop over every frame/netmessage
//!   serialize   serde_json of the IPC payload the webview receives
//!
//! It also classifies every netmessage in the stream as CONSUMED (something in
//! the `analysis` crate matches on it) or DISCARDED (fully decoded, then
//! dropped). The discarded share is the headroom available to a selective
//! parse mode.
//!
//! Every phase also reports heap blocks allocated and the heap's peak
//! (`alloc_counter`), and the decoded tree's delta tables are counted
//! (`tree_census`), so a slow phase can be pinned on a representation (#37).
//!
//! `--without-sounds` decodes the way the capture-queue scan does, leaving
//! the sounds out (`analysis::Sounds::Skip`, #37).
//!
//! Usage: cargo run --release -p dod-benchmark -- <demo.dem | directory>...

mod alloc_counter;
mod tree_census;

use dem::types::{Demo, FrameData, MessageData, MessageDataParseMode, NetMessage};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[global_allocator]
static ALLOCATOR: alloc_counter::Counting = alloc_counter::Counting;

/// EngineMessage variants the `analysis` crate matches on.
/// Keep in sync with `grep -rho 'EngineMessage::[A-Za-z0-9]*' analysis/src/`.
const CONSUMED_ENGINE: &[&str] = &[
    "SvcDirector",
    "SvcHltv",
    "SvcServerInfo",
    "SvcStuffText",
    "SvcTime",
    "SvcUpdateUserInfo",
    // The entity replay behind kill positions (#448).
    "SvcSpawnBaseline",
    "SvcPacketEntities",
    "SvcDeltaPacketEntities",
    // The recorder's own position on a POV demo.
    "SvcClientData",
    // Headshots (#1128), objectives and the rest.
    "SvcEvent",
    "SvcResourceList",
    "SvcTempEntity",
    "SvcPrint",
    "SvcSpawnStaticSound",
];

/// UserMessage names the analyzer accepts.
/// Mirrors `is_relevant_message` in analysis/src/lib.rs.
const CONSUMED_USER: &[&str] = &[
    "RoundState",
    "ClanTimer",
    "TimeLeft",
    "WaveTime",
    "TeamScore",
    "ScoreShort",
    "ObjScore",
    "Frags",
    "PClass",
    "PTeam",
    "ScoreInfo",
    "ScoreInfoLong",
    "SayText",
    "TextMsg",
    "DeathMsg",
    "PStatus",
    "Scope",
    "CurWeapon",
    "ReloadDone",
    "ResetHUD",
    "Health",
];

struct Row {
    name: String,
    size_mb: f64,
    read_ms: f64,
    structural_ms: f64,
    decode_ms: f64,
    drop_ms: f64,
    events_ms: f64,
    serial_ms: f64,
    json_mb: f64,
    decode_allocs: usize,
    delta_tables: usize,
    delta_fields: usize,
    decode_peak_mb: f64,
    analysis_peak_mb: f64,
    analysis_allocs: usize,
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dump_dir = take_dump_dir(&mut args);
    let sounds = take_sounds(&mut args);
    let decode_mode = match sounds {
        analysis::Sounds::Decode => MessageDataParseMode::Parse,
        analysis::Sounds::Skip => MessageDataParseMode::ParseWithoutSounds,
        analysis::Sounds::Tier4Decode => MessageDataParseMode::ExperimentalTier4Decode,
        analysis::Sounds::Tier4Walk => MessageDataParseMode::ExperimentalTier4Walk,
    };
    if args.is_empty() {
        eprintln!(
            "Usage: dod-benchmark [--dump-json <dir>] [--without-sounds] <demo.dem | directory>..."
        );
        eprintln!("Profiles the Demo Analyzer load path phase by phase.");
        eprintln!("--dump-json writes each demo's analysis there, keys sorted, so two");
        eprintln!("builds' outputs can be compared file by file.");
        eprintln!("--without-sounds decodes as the capture-queue scan does.");
        std::process::exit(1);
    }

    let mut files = vec![];
    for arg in &args {
        let p = PathBuf::from(arg);
        if p.is_dir() {
            collect(&p, &mut files);
        } else if p.is_file() {
            files.push(p);
        }
    }
    files.sort();

    if files.is_empty() {
        eprintln!("No .dem files found in the given paths.");
        std::process::exit(1);
    }

    let mut rows = vec![];
    let mut histogram: HashMap<(String, bool), usize> = HashMap::new();

    for path in &files {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        eprintln!("Profiling {}...", name);

        // ── read ─────────────────────────────────────────────────────────────
        alloc_counter::settle();
        let t = Instant::now();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  skipped ({})", e);
                continue;
            }
        };
        let read_ms = ms_since(t);
        let size_mb = bytes.len() as f64 / 1_048_576.0;

        // ── structural-only decode (the floor: no netmessage parsing) ────────
        let t = Instant::now();
        match Demo::parse_from_bytes(&bytes, MessageDataParseMode::None) {
            Ok(d) => drop(d),
            Err(e) => {
                eprintln!("  skipped (structural parse error: {})", e);
                continue;
            }
        }
        let structural_ms = ms_since(t);

        // ── full decode ──────────────────────────────────────────────────────
        alloc_counter::settle();
        alloc_counter::reset_peak();
        let base_live = alloc_counter::live_bytes();
        let base_allocs = alloc_counter::allocations();
        let t = Instant::now();
        let demo = match Demo::parse_from_bytes(&bytes, decode_mode) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  skipped (parse error: {})", e);
                continue;
            }
        };
        let decode_ms = ms_since(t);
        let decode_allocs = alloc_counter::allocations() - base_allocs;
        let decode_peak_mb = mb(alloc_counter::peak_bytes() - base_live);
        let census = tree_census::take(&demo);

        // Classify the stream while the decoded demo is in hand.
        tally(&demo, &mut histogram);

        // ── drop of the decoded tree ─────────────────────────────────────────
        // Freeing the frame tree is NOT free: a 90 MB demo decodes into
        // millions of small allocations (the heap table counts them), and
        // releasing them costs real time. `try_from_bytes` does it on a
        // thread of its own (#511); this shows what that thread pays.
        let t = Instant::now();
        drop(demo);
        let drop_ms = ms_since(t);

        // ── event loop ───────────────────────────────────────────────────────
        // `try_from_bytes` = decode + event loop; it frees the tree on a thread
        // of its own (#511), so back out only the decode just measured.
        alloc_counter::settle();
        alloc_counter::reset_peak();
        let base_live = alloc_counter::live_bytes();
        let base_allocs = alloc_counter::allocations();
        let t = Instant::now();
        let analysis = match analysis::Analysis::try_from_bytes_decoding(&bytes, sounds, |_, _| {})
        {
            Ok(a) => a,
            Err(e) => {
                eprintln!("  skipped (analysis error: {})", e);
                continue;
            }
        };
        let events_ms = (ms_since(t) - decode_ms).max(0.0);
        let analysis_allocs = alloc_counter::allocations() - base_allocs;
        let analysis_peak_mb = mb(alloc_counter::peak_bytes() - base_live);

        // ── serialize the IPC payload (mirrors AnalyzerReportPayload) ────────
        let t = Instant::now();
        let json = serde_json::to_string(&serde_json::json!({
            "demo_info": &analysis.demo_info,
            "state": &analysis.state,
        }))
        .unwrap_or_default();
        let serial_ms = ms_since(t);
        let json_mb = json.len() as f64 / 1_048_576.0;
        if let Some(dir) = &dump_dir {
            dump_analysis(dir, &name, &analysis);
        }

        rows.push(Row {
            name,
            size_mb,
            read_ms,
            structural_ms,
            decode_ms,
            drop_ms,
            events_ms,
            serial_ms,
            json_mb,
            decode_allocs,
            delta_tables: census.delta_tables,
            delta_fields: census.delta_fields,
            decode_peak_mb,
            analysis_peak_mb,
            analysis_allocs,
        });
    }

    report(&rows, &histogram);
}

fn report(rows: &[Row], histogram: &HashMap<(String, bool), usize>) {
    if rows.is_empty() {
        eprintln!("Nothing profiled.");
        return;
    }

    println!("\n### Demo Analyzer load-path phase attribution\n");
    println!(
        "| File | Size (MB) | read | structural | decode | drop | events | serialize | JSON (MB) | total |"
    );
    println!(
        "|------|-----------|------|------------|--------|------|--------|-----------|-----------|-------|"
    );
    for r in rows {
        let total = r.read_ms + r.decode_ms + r.drop_ms + r.events_ms + r.serial_ms;
        println!(
            "| {} | {:.1} | {:.0} ms | {:.0} ms | {:.0} ms | {:.0} ms | {:.0} ms | {:.0} ms | {:.2} | {:.0} ms |",
            r.name,
            r.size_mb,
            r.read_ms,
            r.structural_ms,
            r.decode_ms,
            r.drop_ms,
            r.events_ms,
            r.serial_ms,
            r.json_mb,
            total
        );
    }

    println!("\n### Heap: blocks allocated and peak\n");
    println!(
        "| File | decode blocks | delta tables | delta fields | decode peak (MB) | analysis blocks | analysis peak (MB) |"
    );
    println!(
        "|------|---------------|-----------------------|---------------|------------------|-----------------|--------------------|"
    );
    for r in rows {
        println!(
            "| {} | {:.2} M | {:.2} M | {:.2} M | {:.0} | {:.2} M | {:.0} |",
            r.name,
            r.decode_allocs as f64 / 1e6,
            r.delta_tables as f64 / 1e6,
            r.delta_fields as f64 / 1e6,
            r.decode_peak_mb,
            r.analysis_allocs as f64 / 1e6,
            r.analysis_peak_mb
        );
    }

    let n = rows.len() as f64;
    let sum = |f: fn(&Row) -> f64| rows.iter().map(f).sum::<f64>();
    let (read, structural, decode, drop_t, events, serial) = (
        sum(|r| r.read_ms),
        sum(|r| r.structural_ms),
        sum(|r| r.decode_ms),
        sum(|r| r.drop_ms),
        sum(|r| r.events_ms),
        sum(|r| r.serial_ms),
    );
    let total = read + decode + drop_t + events + serial;

    println!("\n### Share of load time (n = {})\n", rows.len());
    let line = |label: &str, v: f64| {
        println!(
            "* {:<11} {:>7.0} ms total, {:>6.0} ms avg  ({:>4.1}%)",
            label,
            v,
            v / n,
            v / total * 100.0
        )
    };
    line("read:", read);
    line("decode:", decode);
    line("drop:", drop_t);
    line("events:", events);
    line("serialize:", serial);
    println!(
        "\n* **Average end-to-end backend load: {:.0} ms**",
        total / n
    );
    println!(
        "* Average IPC JSON payload: {:.2} MB",
        sum(|r| r.json_mb) / n
    );
    println!(
        "* Structural walk alone is {:.0} ms avg — so {:.0} ms ({:.0}%) of decode is netmessage parsing.",
        structural / n,
        (decode - structural) / n,
        (decode - structural) / decode * 100.0
    );

    // ── consumed vs discarded ────────────────────────────────────────────────
    let mut counts: Vec<(&(String, bool), &usize)> = histogram.iter().collect();
    counts.sort_by(|a, b| b.1.cmp(a.1));
    let grand: usize = counts.iter().map(|(_, c)| **c).sum();
    let consumed: usize = counts
        .iter()
        .filter(|((_, used), _)| *used)
        .map(|(_, c)| **c)
        .sum();

    println!(
        "\n### Netmessage stream: consumed vs discarded (total = {})\n",
        grand
    );
    println!(
        "* CONSUMED by the analyzer: {} ({:.1}%)",
        consumed,
        consumed as f64 / grand as f64 * 100.0
    );
    println!(
        "* DISCARDED after full decode: {} ({:.1}%)  <- selective-parse headroom",
        grand - consumed,
        (grand - consumed) as f64 / grand as f64 * 100.0
    );

    println!("\n| Message | Count | % of all | analyzer |");
    println!("|---------|-------|----------|----------|");
    for ((name, used), count) in counts.iter().take(25) {
        println!(
            "| {} | {} | {:.1}% | {} |",
            name,
            count,
            **count as f64 / grand as f64 * 100.0,
            if *used { "consumed" } else { "discarded" }
        );
    }
}

fn tally(demo: &Demo, histogram: &mut HashMap<(String, bool), usize>) {
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let (key, used) = match m {
                    NetMessage::UserMessage(u) => {
                        let mut n = u.name.len();
                        while n > 0 && u.name[n - 1] == 0 {
                            n -= 1;
                        }
                        let raw = String::from_utf8_lossy(&u.name[..n]).into_owned();
                        let used = CONSUMED_USER.contains(&raw.as_str());
                        (format!("UserMessage::{}", raw), used)
                    }
                    NetMessage::EngineMessage(e) => {
                        // Debug prints the variant name followed by its payload;
                        // the leading identifier is the variant.
                        let s = format!("{:?}", e);
                        let head: String = s.chars().take_while(|c| c.is_alphanumeric()).collect();
                        let used = CONSUMED_ENGINE.contains(&head.as_str());
                        (format!("EngineMessage::{}", head), used)
                    }
                };
                *histogram.entry((key, used)).or_insert(0) += 1;
            }
        }
    }
}

/// Removes `--without-sounds` from `args`, saying whether it was there.
fn take_sounds(args: &mut Vec<String>) -> analysis::Sounds {
    for (flag, mode) in [
        ("--tier4-decode", analysis::Sounds::Tier4Decode),
        ("--tier4-walk", analysis::Sounds::Tier4Walk),
    ] {
        if let Some(at) = args.iter().position(|a| a == flag) {
            args.remove(at);
            return mode;
        }
    }
    match args.iter().position(|a| a == "--without-sounds") {
        Some(at) => {
            args.remove(at);
            analysis::Sounds::Skip
        }
        None => analysis::Sounds::Decode,
    }
}

/// Removes `--dump-json <dir>` from `args`, returning the directory.
fn take_dump_dir(args: &mut Vec<String>) -> Option<PathBuf> {
    let at = args.iter().position(|a| a == "--dump-json")?;
    args.remove(at);
    (at < args.len()).then(|| PathBuf::from(args.remove(at)))
}

/// Writes the analysis as JSON with every object's keys sorted (through
/// `serde_json::Value`), so a map's hash order cannot make two identical
/// analyses differ on disk.
fn dump_analysis(dir: &std::path::Path, name: &str, analysis: &analysis::Analysis) {
    let written = std::fs::create_dir_all(dir)
        .map_err(|e| e.to_string())
        .and_then(|()| serde_json::to_value(analysis).map_err(|e| e.to_string()))
        .and_then(|v| serde_json::to_string_pretty(&v).map_err(|e| e.to_string()))
        .and_then(|json| {
            let path = dir.join(format!("{name}.json"));
            std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))
        });
    if let Err(e) = written {
        eprintln!("  could not dump the analysis of {name}: {e}");
    }
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn collect(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let p = entry.path();
            if p.is_dir() {
                collect(&p, out);
            } else if p.extension().is_some_and(|e| e == "dem") {
                out.push(p);
            }
        }
    }
}
