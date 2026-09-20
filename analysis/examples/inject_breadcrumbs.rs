//! Inject periodic `echo` console commands into a demo so a live tester can
//! read off a running timestamp from the console/qconsole.log, instead of
//! guessing elapsed time from host_framerate or an unreliable kill-feed line
//! (#15). `-condebug` is already always on (#226), so every breadcrumb lands
//! in qconsole.log automatically -- the tester just reports the last one seen
//! before whatever they're describing, and that pins it to within one
//! `interval` of file-relative time, directly comparable to this tool's own
//! `t=X.XXs` output.
//!
//! Reuses the same `ConsoleCommand` frame injection the capture pipeline uses
//! for scheduled commands (`native/patch/highlevel.rs`) -- append-then-sort
//! by `time`, `frame: 0`, well under the 64-byte Cbuf_AddTextToBuffer limit.
//!
//!     cargo run --release -p analysis --example inject_breadcrumbs -- <in.dem> <out.dem> [interval_secs]
use dem::open_demo_from_bytes;
use dem::types::{ByteString, ConsoleCommand, Frame, FrameData};

fn main() {
    let mut a = std::env::args().skip(1);
    let input = a.next().expect("usage: inject_breadcrumbs <in.dem> <out.dem> [interval_secs]");
    let output = a.next().expect("out.dem");
    let interval: f32 = a.next().map(|s| s.parse().expect("interval_secs")).unwrap_or(5.0);

    let bytes = std::fs::read(&input).expect("read");
    let mut demo = open_demo_from_bytes(&bytes).expect("parse");

    let entry = demo.directory.entries.iter_mut().find(|e| e.type_ == 1)
        .expect("no type-1 (Playback) entry");

    let first_time = entry.frames.first().map(|f| f.time).unwrap_or(0.0);
    let last_time = entry.frames.last().map(|f| f.time).unwrap_or(0.0);

    let mut t = first_time + interval;
    let mut n = 0usize;
    while t < last_time {
        let text = format!("echo [BC {t:.1}]");
        entry.frames.push(Frame {
            time: t,
            frame: 0,
            frame_data: FrameData::ConsoleCommand(ConsoleCommand {
                command: ByteString::from(text.as_str()),
            }),
        });
        n += 1;
        t += interval;
    }
    entry.frames.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));

    println!("injected {n} breadcrumbs every {interval:.1}s across {first_time:.1}s..{last_time:.1}s");

    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write");
    println!("wrote {output} ({:.1} MB)", out.len() as f64 / 1e6);

    match open_demo_from_bytes(&out) {
        Ok(d) => println!("re-parse OK: {} frames",
            d.directory.entries.iter().map(|e| e.frames.len()).sum::<usize>()),
        Err(e) => println!("re-parse FAILED: {e}"),
    }
}
