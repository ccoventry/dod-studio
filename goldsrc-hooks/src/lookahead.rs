//! Look-ahead (issue #1154, R&D): what the watched player is about to do,
//! read out of the demo file before playback gets there.
//!
//! An HLTV demo shows some changes late against the player's own view. The
//! clearest is a crouch: POV eases the camera down over 0.4 s from the key,
//! and the server flips the hull, the first thing an HLTV demo shows, only
//! when that ease ends. The game holds nothing newer than the snapshot it
//! last read (it draws 0.1 s behind that, `ex_interp`, but the hull flip is
//! already in the newest snapshot when it arrives), so the only way to start
//! the camera's ease with POV's is to know the flip is coming.
//!
//! So when an HLTV demo plays with `ds_spec_match_pov` on, a thread of its
//! own at below-normal priority reads the `.dem` once ([`timeline::read`])
//! and publishes a [`Timeline`]. Until it answers, and for any demo it can't
//! read, everything works as without it. The engine thread only ever looks
//! the answer up: [`timeline`] costs an atomic load a frame, and a
//! non-blocking `try_read` the one frame a new timeline lands.
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
//! leaves it stale for at most that long.

pub mod timeline;

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

pub use timeline::Timeline;

/// Bumped each time a timeline is published.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The newest timeline and the demo it is for. Written by the reading
/// thread; read by the engine thread with `try_read` only.
static PUBLISHED: RwLock<Option<(String, Arc<Timeline>)>> = RwLock::new(None);

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

    /// For the log: the next ground crouch at all, on the file's clock.
    fn next_crouch_svc(&self, entity: i32, newest: f32) -> Option<f32> {
        let svc = (f64::from(newest) + self.svc_offset) as f32;
        self.timeline.next_crouch_down(entity, svc, f32::MAX)
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

/// Reads `name` (as `viewdemo` was given it) on a thread of its own.
fn start_read(name: String) {
    let path = crate::streaks::demo_path(&crate::texture_hires::game_dir(), &name);
    let spawned = std::thread::Builder::new()
        .name("ds_lookahead".into())
        .spawn(move || {
            lower_priority();
            let start = std::time::Instant::now();
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    report(&format!("can't read {}: {e}", path.display()));
                    return;
                }
            };
            let read_ms = start.elapsed().as_millis();
            let size = bytes.len();
            let found = timeline::read(&bytes);
            drop(bytes);
            let Some(found) = found else {
                report(&format!("{} isn't a demo it can read", path.display()));
                return;
            };
            report(&format!(
                "{}: {} ({} MB file read in {read_ms} ms, parsed in {} ms in all)",
                path.display(),
                timeline::summary(&found),
                size >> 20,
                start.elapsed().as_millis()
            ));
            let mut published = PUBLISHED.write().unwrap_or_else(|e| e.into_inner());
            *published = Some((name, Arc::new(found)));
            GENERATION.fetch_add(1, Ordering::Release);
        });
    if let Err(e) = spawned {
        report(&format!("couldn't start the reading thread: {e}"));
    }
}

/// The per-frame cost and the flips' arrival, for the R&D log
/// (`ds_debug_log_weapon_model 1`): every [`COST_FRAMES`] frames the mean
/// and the worst time the look-ahead took, and each looked-ahead flip's
/// `msg_time` against the demo clock when it arrived.
pub fn note_frame(
    cost: std::time::Duration,
    timeline: Option<&View>,
    target: i32,
    time: f64,
    newest: f32,
    small_hull: bool,
    upcoming: Option<f64>,
) {
    if !crate::anim_fix::LOG_HELD_MODELS.load(Ordering::Relaxed) {
        return;
    }
    STATS.with(|stats| {
        let mut st = stats.borrow_mut();
        let ns = u64::try_from(cost.as_nanos()).unwrap_or(u64::MAX);
        st.frames += 1;
        st.total_ns += ns;
        st.worst_ns = st.worst_ns.max(ns);
        if st.frames >= COST_FRAMES {
            report(&format!(
                "per frame over {} frames: mean {} ns, worst {} ns; target {target}, newest msg_time {newest:.3}, clock {time:.3}, {}",
                st.frames,
                st.total_ns / st.frames,
                st.worst_ns,
                match timeline {
                    None => "no timeline yet".to_string(),
                    Some(t) => format!(
                        "world clock - game clock {:.3}, his next ground crouch in the file at {:?}",
                        t.svc_offset,
                        t.next_crouch_svc(target, newest)
                    ),
                }
            ));
            st.frames = 0;
            st.total_ns = 0;
            st.worst_ns = 0;
        }
        // The flip is the frame his newest snapshot turns small-hull.
        if let Some(flip) = st.waiting
            && small_hull
            && !st.small_hull
        {
            report(&format!(
                "flip due at {flip:.4} arrived at demo clock {time:.4} ({:+.4} s): newest msg_time {newest:.4}",
                time - flip
            ));
            st.waiting = None;
            st.reported = Some(flip);
        }
        st.small_hull = small_hull;
        // The same flip comes back a little moved as the clock difference
        // is read again; once reported it isn't waited for twice.
        if let Some(next) = upcoming
            && st.reported.is_none_or(|r| (next - r).abs() > 0.2)
        {
            st.waiting = Some(next);
        }
    });
}

const COST_FRAMES: u64 = 3000;

#[derive(Default)]
struct Stats {
    frames: u64,
    total_ns: u64,
    worst_ns: u64,
    waiting: Option<f64>,
    reported: Option<f64>,
    small_hull: bool,
}

thread_local! {
    static STATS: RefCell<Stats> = RefCell::new(Stats::default());
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
