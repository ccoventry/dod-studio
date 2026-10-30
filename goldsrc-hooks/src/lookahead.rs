//! Look-ahead (issue #1154): what the watched player is about to do, read
//! out of the demo file before playback gets there.
//!
//! An HLTV demo shows some changes late against the player's own view. The
//! clearest is a crouch: POV eases the camera down over 0.4 s from the key,
//! and the server flips the hull, the first thing an HLTV demo shows, only
//! when that ease ends. The game holds nothing newer than the snapshot it
//! last read (it draws 0.1 s behind that, `ex_interp`, but the hull flip is
//! already in the newest snapshot when it arrives), so the only way to start
//! the camera's ease with POV's is to know the flip is coming. The other
//! user is the sprint let-go in a fall off an edge (#704): the body leaving
//! `sprint_*` says the key was let go, and the file says it first.
//!
//! So when an HLTV demo plays with `ds_spec_match_pov` on, a thread of its
//! own at below-normal priority reads the `.dem` once ([`timeline::read`],
//! about 1 s for a 40 MB half, holding about 1 MB of the file at a time) and
//! publishes a [`Timeline`]. Until it answers, and for any demo it can't
//! read, everything works as without it. The engine thread only ever looks
//! the answer up: [`timeline`] costs an atomic load a frame, a non-blocking
//! `try_read` the one frame a new timeline lands, and a read of the demo
//! player's clock every [`OFFSET_SECONDS`] (about 1 µs a frame in all,
//! measured on PRE and POST).
//!
//! ## Two clocks
//!
//! The timeline is on `svc_time`, the server clock the file carries and the
//! demo player's world clock (what `ds_demo_seek_to` takes). The game's own
//! clock, `ref_params_s::time`, and with it every entity state's `msg_time`,
//! is not: under `viewdemo` it runs on from the load and a seek doesn't move
//! it (measured on PRE: `msg_time` 49.93 against a world clock near 1064).
//! So the lookup goes through the difference between the two, read off the
//! demo player ([`crate::demo_seek::clock`]) every [`OFFSET_SECONDS`]: a seek
//! leaves it stale for at most that long, and the crouch ease lets go of a
//! looked-ahead flip the file stops promising (`spectator_eye::crouch`).

mod frames;
pub mod timeline;

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

pub use timeline::Timeline;

/// Bumped each time a timeline is published.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The newest timeline and the demo it is for. Written by the reading
/// thread; read by the engine thread with `try_read` only.
static PUBLISHED: RwLock<Option<(String, Arc<Timeline>)>> = RwLock::new(None);
/// The newest read asked for. A reading thread whose ticket is older stops,
/// so a demo changed mid-read doesn't leave a read running for nothing or
/// publish over the newer one.
static LATEST_READ: AtomicU64 = AtomicU64::new(0);
/// What the newest read did, for `ds_debug_status`. Written by the reading
/// threads only, so the engine thread's per-frame path never waits on it.
static STATUS: RwLock<String> = RwLock::new(String::new());

/// How often (demo seconds) the engine thread checks which demo is playing.
const CHECK_SECONDS: f64 = 0.5;
/// How often it reads the world clock against the game's.
const OFFSET_SECONDS: f64 = 0.1;
/// A snapshot arrives this long after the world clock (as read every
/// [`OFFSET_SECONDS`]) passes its `svc_time`: negative, it arrives first.
/// Measured on m00cat's looked-ahead crouches: PRE -0.035 s (14 flips, sd
/// 0.011), POST -0.031 s (15, sd 0.010).
const ARRIVAL_LAG_SECONDS: f64 = -0.033;

#[derive(Default)]
struct Cache {
    /// The demo a read was started for.
    requested: Option<String>,
    /// The timeline in use, its demo, and the generation it came from.
    timeline: Option<(String, Arc<Timeline>)>,
    generation: u64,
    last_check: f64,
    /// World clock minus the game's clock, and when it was read.
    svc_offset: Option<f64>,
    offset_at: f64,
}

thread_local! {
    /// The engine thread's view.
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The timeline and the clock difference to read it with.
#[derive(Clone)]
pub struct View {
    timeline: Arc<Timeline>,
    /// `svc_time` = the game's clock (`msg_time`) + this.
    svc_offset: f64,
}

impl View {
    /// Entity `entity`'s next ground crouch after `newest` (his newest
    /// snapshot's `msg_time`) and no more than `ahead` seconds past it: when
    /// its flip will arrive, on the game's clock.
    pub fn next_crouch_down(&self, entity: i32, newest: f32, ahead: f32) -> Option<f64> {
        let svc = (f64::from(newest) + self.svc_offset) as f32;
        self.timeline
            .next_crouch_down(entity, svc, svc + ahead)
            .map(|t| f64::from(t) - self.svc_offset + ARRIVAL_LAG_SECONDS)
    }

    /// Entity `entity`'s body changes after `newest` (a `msg_time`) and no
    /// more than `ahead` seconds past it.
    pub fn body_changes(&self, entity: i32, newest: f32, ahead: f32) -> &[(f32, u16)] {
        let svc = (f64::from(newest) + self.svc_offset) as f32;
        self.timeline.body_changes(entity, svc, svc + ahead)
    }
}

/// The playing demo's timeline, once read and while the demo player's clock
/// can be read. Engine thread, every frame the look-ahead could be used;
/// starts the read the first time a demo is seen.
///
/// `time` must be on the `msg_time` clock, and the same one for every
/// caller, since the clock difference is cached for them all: callers pass
/// the watched player's newest `msg_time`. (The hook's `client_time` runs on
/// its own, about 0.35 s from it; one caller using it put the other's
/// lookups that far off.)
pub fn timeline(time: f64) -> Option<View> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !(0.0..CHECK_SECONDS).contains(&(time - cache.last_check)) {
            cache.last_check = time;
            let playing = crate::demo_reload::current_demo();
            if playing.is_some() && playing != cache.requested {
                cache.requested = playing.clone();
                cache.timeline = None;
                if let Some(name) = playing {
                    start_read(name);
                }
            }
        }
        let generation = GENERATION.load(Ordering::Acquire);
        if generation != cache.generation
            && let Ok(published) = PUBLISHED.try_read()
        {
            cache.generation = generation;
            cache.timeline = published.clone();
        }
        let requested = cache.requested.as_deref()?;
        let timeline = cache
            .timeline
            .as_ref()
            .filter(|(demo, _)| demo == requested)
            .map(|(_, t)| t.clone())?;
        if !(0.0..OFFSET_SECONDS).contains(&(time - cache.offset_at)) {
            cache.offset_at = time;
            cache.svc_offset = crate::demo_seek::clock()
                .filter(|c| c.active && !c.loading)
                .map(|c| c.now - time);
        }
        Some(View {
            timeline,
            svc_offset: cache.svc_offset?,
        })
    })
}

/// One line for `ds_debug_status`: which demo the look-ahead has read, or
/// why it has none.
pub fn status() -> String {
    match STATUS.read() {
        Ok(status) if !status.is_empty() => status.clone(),
        Ok(_) => "no HLTV demo watched in first person yet".to_string(),
        Err(_) => "unknown".to_string(),
    }
}

/// How a read ended.
enum Outcome {
    Read(Timeline),
    Failed(String),
    /// Another demo was asked for meanwhile; its read owns the status.
    Superseded,
}

/// Reads `name` (as `viewdemo` was given it) on a thread of its own, and
/// publishes its timeline unless another demo was asked for meanwhile.
fn start_read(name: String) {
    let ticket = LATEST_READ.fetch_add(1, Ordering::AcqRel) + 1;
    let path = crate::streaks::demo_path(&crate::texture_hires::game_dir(), &name);
    let spawned = std::thread::Builder::new()
        .name("ds_lookahead".into())
        .spawn(move || {
            lower_priority();
            set_status(ticket, format!("reading {}", path.display()));
            let start = std::time::Instant::now();
            match read_demo(&path, ticket) {
                Outcome::Read(found) => {
                    let line = format!(
                        "{}: {}, read in {:.1} s",
                        path.display(),
                        found.summary(),
                        start.elapsed().as_secs_f64()
                    );
                    if publish(ticket, name, found) {
                        set_status(ticket, line.clone());
                        report(&line);
                    }
                }
                Outcome::Failed(why) => {
                    set_status(ticket, why.clone());
                    report(&why);
                }
                Outcome::Superseded => {}
            }
        });
    if let Err(e) = spawned {
        // Not in the status: the engine thread leaves that lock alone.
        report(&format!("couldn't start the reading thread: {e}"));
    }
}

/// Reads the demo at `path` into a timeline, giving up once `ticket` is no
/// longer the newest read.
fn read_demo(path: &Path, ticket: u64) -> Outcome {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) => return Outcome::Failed(format!("can't open {}: {e}", path.display())),
    };
    let superseded = || LATEST_READ.load(Ordering::Acquire) != ticket;
    match timeline::read(file, &superseded) {
        Ok(found) => Outcome::Read(found),
        Err(timeline::ReadError::NotADemo) => {
            Outcome::Failed(format!("{} has no demo directory to read", path.display()))
        }
        Err(timeline::ReadError::Superseded) => Outcome::Superseded,
    }
}

/// Makes `found` the published timeline, if `ticket` is still the newest
/// read. Checked under the write lock, so a read that finishes late can't
/// publish over a newer one.
fn publish(ticket: u64, name: String, found: Timeline) -> bool {
    let mut published = PUBLISHED.write().unwrap_or_else(|e| e.into_inner());
    if LATEST_READ.load(Ordering::Acquire) != ticket {
        return false;
    }
    *published = Some((name, Arc::new(found)));
    GENERATION.fetch_add(1, Ordering::Release);
    true
}

/// Sets the status line, unless a newer read than `ticket` owns it.
fn set_status(ticket: u64, line: String) {
    let mut status = STATUS.write().unwrap_or_else(|e| e.into_inner());
    if LATEST_READ.load(Ordering::Acquire) == ticket {
        *status = line;
    }
}

fn report(message: &str) {
    // Safety: `report` only appends to the hook log; any thread may call it.
    unsafe { crate::debug::report(&format!("lookahead: {message}")) };
}

/// The read must not take frames from the game.
fn lower_priority() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        // Safety: the pseudo-handle of the calling thread.
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
    }
}
