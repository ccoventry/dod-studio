//! The recording player's position, speed, ground and keys every frame of a
//! POV demo, as tab-separated columns: for measuring how far and how high
//! DoD's player movement really goes (the overview's reach map took its
//! jump numbers from one of these, `native::overview::reach::leap_reach`).
//!
//! cargo run -p analysis --example jump_probe -- <demo> > moves.tsv
use dem::open_demo_from_bytes;
use dem::types::FrameData;

fn main() {
    let path = std::env::args().nth(1).expect("demo path");
    let bytes = std::fs::read(path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");
    let mut said_vars = false;
    println!("time\tx\ty\tz\tvx\tvy\tvz\tground\tjump\tduck\teye\tmsec");
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let info = &bt.1.info;
            let p = &info.refparams;
            if !said_vars {
                let m = &info.movevars;
                eprintln!(
                    "gravity {} stepsize {} maxspeed {} accelerate {} airaccelerate {} friction {} entgravity {}",
                    m.gravity,
                    m.stepsize,
                    m.maxspeed,
                    m.accelerate,
                    m.airaccelerate,
                    m.friction,
                    m.entgravity
                );
                said_vars = true;
            }
            let g = |v: &Vec<f32>, i: usize| v.get(i).copied().unwrap_or(0.0);
            let b = info.usercmd.buttons;
            println!(
                "{:.3}\t{:.1}\t{:.1}\t{:.1}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}\t{}\t{:.1}\t{}",
                frame.time,
                g(&p.sim_org, 0),
                g(&p.sim_org, 1),
                g(&p.sim_org, 2),
                g(&p.sim_vel, 0),
                g(&p.sim_vel, 1),
                g(&p.sim_vel, 2),
                p.on_ground,
                u8::from(b & 2 != 0),
                u8::from(b & 4 != 0),
                g(&p.view_origin, 2) - g(&p.sim_org, 2),
                info.usercmd.msec,
            );
        }
    }
}
