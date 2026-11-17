//! Repairing a damaged demo so it plays to the end (#15).
//!
//! A demo damaged on disk has holes: stretches of bytes no frame header makes
//! sense of. The engine stops (or crashes) at the first one, and so does the
//! analyzer (#811). A repair cuts the holes out and makes each join playable:
//!
//! 1. **Bridge** (`bridge.rs`): walk the frames, cut at each hole, find where
//!    the recording resumes, splice the stretches together.
//! 2. **Joins** (`joins.rs`, `join_state.rs`, `snapshot.rs`): at each join,
//!    a full entity snapshot of what the client held, and a ramp of no-op
//!    frames up to it.
//!    A frame whose view (origin, angles, health) the damage reached, a ramp
//!    frame copying one included, takes a clean frame's (`view_range.rs`).
//! 3. **Entity counts** (`entity_count.rs`): every packet's declared count
//!    restated as the set the engine really builds.
//! 4. **Checks**: the result parses, the engine discards no entity packet, and
//!    every count agrees. A result that fails one is refused, never returned.
//!
//! Everything here is bytes in, bytes and a report out: no files, no threads,
//! so it also builds for the wasm web analyzer. Why each step is the way it
//! is, and every live crash that taught it: `docs/goldsrc_demo_salvage.md`.

mod bridge;
pub mod entity_carry;
mod entity_count;
mod frame_walk;
mod join_state;
mod joins;
mod snapshot;
mod view_range;

pub use snapshot::JoinReport;

use bridge::{BridgeError, BridgeHooks, Bridged};
use dem::types::Demo;

/// A step of a repair, for a progress line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairStep {
    /// Walking the file for holes and where the recording resumes.
    FindingHoles,
    /// Reading the spliced recording.
    Reading,
    /// Rebuilding what the client holds at each join.
    RebuildingJoins,
    /// Writing the repaired demo.
    Writing,
    /// Reading the result back and checking it.
    Checking,
}

impl RepairStep {
    /// Where this step starts and how much of the whole it is, so one bar can
    /// run through every step. Measured on the four Monday demos: the two
    /// parses are most of a repair.
    fn span(self) -> (f32, f32) {
        match self {
            RepairStep::FindingHoles => (0.0, 0.2),
            RepairStep::Reading => (0.2, 0.3),
            RepairStep::RebuildingJoins => (0.5, 0.05),
            RepairStep::Writing => (0.55, 0.1),
            RepairStep::Checking => (0.65, 0.35),
        }
    }
}

/// Progress through a repair.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct RepairProgress {
    pub step: RepairStep,
    /// Through the whole repair, 0.0 to 1.0.
    pub fraction: f32,
}

/// One hole, in the original recording's time (each frame's own time, the
/// seconds since the recording started).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Hole {
    /// The last moment kept before the hole.
    pub before_secs: f32,
    /// The first moment kept after it.
    pub after_secs: f32,
    /// Recording lost to the hole: `after_secs - before_secs`, or 0 where the
    /// clock steps back across it.
    pub lost_secs: f32,
    /// Bytes of the original file left out at the hole.
    pub bytes_skipped: u64,
}

/// What the checks on a repaired demo found.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct RepairChecks {
    /// The result parses with the strict parser the patcher and the engine's
    /// own reading agree with; the reason when it does not.
    pub parse_error: Option<String>,
    /// Playback frames in the result.
    pub frames: usize,
    /// Packet-entity messages checked.
    pub entity_packets: usize,
    /// Entity packets the engine would throw away (`CL_FlushEntityPacket`):
    /// must be 0.
    pub discarded_packets: usize,
    /// Packets whose declared entity count is not the set the engine carries:
    /// must be 0, or the engine writes past the end of the frame it allocated.
    pub count_disagreements: usize,
}

/// Why a repair was refused.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Refusal {
    /// The repaired demo does not parse.
    DoesNotParse { reason: String },
    /// Fewer frames survived than the bridge kept: a directory entry ends
    /// early.
    FramesLost { kept: usize, parsed: usize },
    /// The engine would throw away this many entity packets.
    PacketsDiscarded { count: usize },
    /// This many entity counts still disagree with what the engine carries.
    CountsDisagree { count: usize },
    /// A join got no snapshot: no frame carrying entity data followed it.
    JoinWithoutSnapshot { join: usize },
}

/// What a repair found and did.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct RepairReport {
    pub holes: Vec<Hole>,
    /// The recording breaks once more after the last hole (or, with none,
    /// once at all) and never resumes: the repair ends at that break, which
    /// is not one of `holes`.
    pub ends_at_last_hole: bool,
    pub joins: Vec<JoinReport>,
    /// Damaged frames left out at the edges of holes.
    pub edge_frames_dropped: usize,
    /// Frames whose view (origin, angles, health) the damage reached, ramp
    /// frames copying one included, given a clean frame's view.
    pub views_restated: usize,
    /// Bytes of the original's frames kept, and the original's frames.
    pub bytes_kept: u64,
    pub bytes_total: u64,
    pub checks: RepairChecks,
}

impl RepairReport {
    /// The original's frames kept, 0 to 100.
    pub fn percent_kept(&self) -> f32 {
        if self.bytes_total == 0 {
            return 100.0;
        }
        (self.bytes_kept as f64 / self.bytes_total as f64 * 100.0) as f32
    }

    /// Seconds of recording lost to all holes.
    pub fn seconds_lost(&self) -> f32 {
        self.holes.iter().map(|h| h.lost_secs).sum()
    }
}

/// What a repair came to.
#[derive(Clone, Debug, PartialEq)]
pub enum RepairOutcome {
    /// The frames walk to the end: there is no hole to repair.
    NotDamaged,
    /// The repaired demo, which passed every check.
    Repaired {
        bytes: Vec<u8>,
        report: RepairReport,
    },
    /// A result was built and failed a check, so it is not returned.
    Refused {
        report: RepairReport,
        reasons: Vec<Refusal>,
    },
}

/// Why a demo cannot be repaired at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepairError {
    /// The file's signon (the start every demo needs) does not read, so there
    /// is nothing to build on.
    NoSignon,
    /// The recording before the first hole does not parse.
    NothingParses,
    /// The spliced recording does not parse; the reason in words.
    DoesNotParse(String),
    Cancelled,
}

impl std::fmt::Display for RepairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepairError::NoSignon => f.write_str("the start of the demo does not read"),
            RepairError::NothingParses => {
                f.write_str("the recording before the first hole does not read")
            }
            RepairError::DoesNotParse(why) => {
                write!(f, "the joined recording does not read: {why}")
            }
            RepairError::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for RepairError {}

impl From<BridgeError> for RepairError {
    fn from(e: BridgeError) -> Self {
        match e {
            BridgeError::NoSignon => RepairError::NoSignon,
            BridgeError::NothingParses => RepairError::NothingParses,
            BridgeError::Cancelled => RepairError::Cancelled,
        }
    }
}

/// What a repair needs from its caller.
pub struct RepairHooks<'a> {
    /// Called as the repair goes, a few hundred times at most.
    pub progress: &'a mut dyn FnMut(RepairProgress),
    /// Checked between steps and inside the long ones.
    pub is_cancelled: &'a dyn Fn() -> bool,
}

/// A [`RepairHooks`] for a caller that wants neither.
pub fn no_hooks<R>(run: impl FnOnce(RepairHooks) -> R) -> R {
    run(RepairHooks {
        progress: &mut |_| {},
        is_cancelled: &|| false,
    })
}

/// Repairs the demo in `bytes`: cuts out every hole, rebuilds each join, and
/// checks the result. Never returns a result that fails a check
/// ([`RepairOutcome::Refused`] says which). Pure: reads nothing, writes
/// nothing.
///
/// # Errors
/// [`RepairError`] when the demo cannot be repaired at all, or was cancelled.
pub fn repair_demo(bytes: &[u8], hooks: RepairHooks) -> Result<RepairOutcome, RepairError> {
    let RepairHooks {
        progress,
        is_cancelled,
    } = hooks;
    let mut report_step = |step: RepairStep, within: f32| {
        let (start, width) = step.span();
        progress(RepairProgress {
            step,
            fraction: start + width * within.clamp(0.0, 1.0),
        });
    };
    let cancelled = || {
        if is_cancelled() {
            Err(RepairError::Cancelled)
        } else {
            Ok(())
        }
    };

    let bridged = bridge::bridge(
        bytes,
        BridgeHooks {
            progress: &mut |f| report_step(RepairStep::FindingHoles, f),
            is_cancelled,
        },
    )?;
    if bridged.cuts.is_empty() && bridged.bytes.is_empty() {
        return Ok(RepairOutcome::NotDamaged);
    }
    let mut report = report_from_bridge(&bridged);
    let frames_kept = frame_walk::frame_starts(
        &bridged.bytes,
        frame_walk::DEMO_HEADER_SIZE,
        frame_walk::frames_end(&bridged.bytes),
    )
    .0
    .len();

    cancelled()?;
    let mut demo = parse(&bridged.bytes, &mut |f| report_step(RepairStep::Reading, f))?;
    cancelled()?;
    report_step(RepairStep::RebuildingJoins, 0.0);
    let joins = joins::find_joins(&demo);
    let states = join_state::states_at_joins(&demo, &joins);
    report.joins = snapshot::inject(&mut demo, &joins, &states);
    drop(states);
    // After the ramps are in: each copies the view of the first network frame
    // after its hole, which the damage can reach.
    report.views_restated = view_range::restate_damaged_views(&mut demo);
    entity_count::restate_entity_counts(&mut demo);

    cancelled()?;
    report_step(RepairStep::Writing, 0.0);
    let ramp_frames: usize = report.joins.iter().map(|j| j.ramp_frames).sum();
    let repaired = demo
        .write_to_bytes_reusing_source_cancellable(&bridged.bytes, is_cancelled)
        .ok_or(RepairError::Cancelled)?;
    drop(demo);
    drop(bridged);

    report_step(RepairStep::Checking, 0.0);
    report.checks = check(&repaired, &mut |f| report_step(RepairStep::Checking, f));
    let mut reasons = refusals(&report, frames_kept + ramp_frames);
    if report.checks.parse_error.is_some() {
        reasons.retain(|r| matches!(r, Refusal::DoesNotParse { .. }));
    }
    report_step(RepairStep::Checking, 1.0);
    Ok(if reasons.is_empty() {
        RepairOutcome::Repaired {
            bytes: repaired,
            report,
        }
    } else {
        RepairOutcome::Refused { report, reasons }
    })
}

/// Where a demo's frames first stop making sense: the first hole a repair
/// would cut out.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct FirstHole {
    /// The recording time of the last frame that reads, seconds.
    pub after_secs: f32,
    /// Where the frame that does not read starts in the file.
    pub byte_offset: u64,
    /// From there to the end of the frames.
    pub bytes_unread: u64,
}

/// The first hole in `bytes`, from the frame headers alone (no message is
/// decoded, so this is about as fast as reading the file). `None` when the
/// frames walk to the end, which is when [`repair_demo`] finds nothing to
/// repair. Damage inside a frame whose header is intact is not a hole; the
/// analyzer's tolerant parse (#811) still sees it.
pub fn find_first_hole(bytes: &[u8]) -> Option<FirstHole> {
    let end = frame_walk::frames_end(bytes);
    let run = frame_walk::walk(bytes, frame_walk::DEMO_HEADER_SIZE, end, usize::MAX);
    (run.cut < end.saturating_sub(bridge::END_SLACK_BYTES)).then(|| FirstHole {
        after_secs: bridge::time_at_end(bytes, frame_walk::DEMO_HEADER_SIZE, run.cut)
            .or(run.last_time)
            .unwrap_or(0.0),
        byte_offset: run.cut as u64,
        bytes_unread: (end - run.cut) as u64,
    })
}

/// The strict parse, with progress as a fraction of the bytes read.
fn parse(bytes: &[u8], progress: &mut dyn FnMut(f32)) -> Result<Demo, RepairError> {
    let mut on_bytes = |read: usize, total: usize| progress(read as f32 / total.max(1) as f32);
    dem::open_demo_from_bytes_with_progress(bytes, &mut on_bytes).map_err(|e| {
        RepairError::DoesNotParse(crate::integrity::parse_error_reason(&e.to_string()))
    })
}

/// The holes and byte counts the bridge found.
fn report_from_bridge(bridged: &Bridged) -> RepairReport {
    RepairReport {
        holes: bridged
            .cuts
            .iter()
            .map(|c| Hole {
                before_secs: c.time_before,
                after_secs: c.time_after,
                lost_secs: (c.time_after - c.time_before).max(0.0),
                bytes_skipped: c.resume.saturating_sub(c.stop) as u64,
            })
            .collect(),
        ends_at_last_hole: bridged.stopped_early,
        edge_frames_dropped: bridged.edge_frames_dropped,
        bytes_kept: bridged.bytes_kept as u64,
        bytes_total: bridged.bytes_total as u64,
        ..Default::default()
    }
}

/// Reads a repaired demo back and checks what the engine will make of it.
fn check(repaired: &[u8], progress: &mut dyn FnMut(f32)) -> RepairChecks {
    let demo = match parse(repaired, progress) {
        Ok(demo) => demo,
        Err(e) => {
            return RepairChecks {
                parse_error: Some(e.to_string()),
                ..Default::default()
            };
        }
    };
    let counts = entity_count::check_entity_counts(&demo);
    RepairChecks {
        parse_error: None,
        frames: demo
            .directory
            .entries
            .iter()
            .skip(1)
            .map(|e| e.frames.len())
            .sum(),
        entity_packets: counts.packets,
        discarded_packets: counts.flushed,
        count_disagreements: counts.disagreements,
    }
}

/// Every check `report` fails. `frames_expected` is what the bridge kept plus
/// the ramps: the result must hold nearly all of it, since a directory entry
/// that ends at a stray section end still parses and holds 32 frames.
fn refusals(report: &RepairReport, frames_expected: usize) -> Vec<Refusal> {
    let checks = &report.checks;
    let mut reasons = Vec::new();
    if let Some(reason) = &checks.parse_error {
        reasons.push(Refusal::DoesNotParse {
            reason: reason.clone(),
        });
    }
    // Entry 0's frames are in the walk's count and not in `frames`.
    if checks.frames * 10 < frames_expected * 9 {
        reasons.push(Refusal::FramesLost {
            kept: frames_expected,
            parsed: checks.frames,
        });
    }
    if checks.discarded_packets > 0 {
        reasons.push(Refusal::PacketsDiscarded {
            count: checks.discarded_packets,
        });
    }
    if checks.count_disagreements > 0 {
        reasons.push(Refusal::CountsDisagree {
            count: checks.count_disagreements,
        });
    }
    for (join, j) in report.joins.iter().enumerate() {
        if !j.snapshot_placed {
            reasons.push(Refusal::JoinWithoutSnapshot { join });
        }
    }
    reasons
}

#[cfg(test)]
mod tests;
