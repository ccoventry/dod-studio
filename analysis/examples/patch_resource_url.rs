//! Replace the demo's own recorded SvcResourceLocation (download_url) --
//! a dead 2015-era fastdl host baked into the recorded network stream at
//! connect time -- with a working mirror, so demo playback stops trying
//! (and failing) to reach a host that no longer resolves (#15).
//!
//!     cargo run --release -p analysis --example patch_resource_url -- <in.dem> <out.dem> <new_url>
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let mut a = std::env::args().skip(1);
    let input = a.next().expect("usage: patch_resource_url <in.dem> <out.dem> <new_url>");
    let output = a.next().expect("out.dem");
    let new_url = a.next().expect("new_url");

    let bytes = std::fs::read(&input).expect("read");
    let mut demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut new_bytes = new_url.clone().into_bytes();
    new_bytes.push(0); // null_string's write() copies download_url verbatim, terminator included

    let mut patched = 0usize;
    for entry in demo.directory.entries.iter_mut() {
        for f in entry.frames.iter_mut() {
            let FrameData::NetworkMessage(bt) = &mut f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &mut bt.1.messages else { continue };
            for m in msgs.iter_mut() {
                if let NetMessage::EngineMessage(em) = m {
                    if let EngineMessage::SvcResourceLocation(rl) = &mut **em {
                        println!("frame time={:.2}s: {:?} -> {new_url:?}",
                            f.time, String::from_utf8_lossy(&rl.download_url));
                        rl.download_url = new_bytes.clone();
                        patched += 1;
                    }
                }
            }
        }
    }
    println!("patched {patched} SvcResourceLocation message(s)");
    if patched == 0 { println!("nothing to write"); return }

    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write");
    println!("wrote {output} ({:.1} MB)", out.len() as f64 / 1e6);

    match open_demo_from_bytes(&out) {
        Ok(d) => println!("re-parse OK: {} frames",
            d.directory.entries.iter().map(|e| e.frames.len()).sum::<usize>()),
        Err(e) => println!("re-parse FAILED: {e}"),
    }
}
