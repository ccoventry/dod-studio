//! Locate a distinctive chat line's exact frame/time/sequence, to correlate a
//! live crash's console tail precisely against our own byte/frame structure
//! (#15). Chat text is a far better landmark than a kill event: `SayText` is
//! essentially free text, unlikely to repeat verbatim, where a kill pairing
//! (player X killed player Y) legitimately recurs many times a match.
//!
//!     cargo run --release -p analysis --example find_chat_spot -- <demo.dem> <substring>
use dem::open_demo_from_bytes;
use dem::types::{FrameData, MessageData, NetMessage};
use dod::UserMessage;

fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("usage: find_chat_spot <demo.dem> <substring>");
    let needle = a.next().expect("substring to search for");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut idx = 0usize;
    let mut found = 0usize;
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { idx += 1; continue };
            let seq = bt.1.sequence_info.incoming_sequence;
            let time = f.time;
            let MessageData::Parsed(msgs) = &bt.1.messages else { idx += 1; continue };
            for m in msgs {
                let NetMessage::UserMessage(um) = m else { continue };
                if let Ok(UserMessage::SayText(st)) = UserMessage::new(&um.name, &um.data) {
                    if st.text.contains(&needle) {
                        found += 1;
                        println!("frame_idx={idx} t={time:.2}s seq={seq}: client={} text={:?}",
                            st.client_index, st.text);
                    }
                }
            }
            idx += 1;
        }
    }
    if found == 0 { println!("no SayText containing {needle:?} found") }
}
