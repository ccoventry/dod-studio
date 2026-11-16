//! The per-frame callback the rest of the DLL hangs off: driven by the real
//! `HUD_Frame` trampoline, with a fallback timer thread for when that hook
//! never lands, and the playback clock summed from `HUD_Frame`'s deltas.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::System::Threading::{CreateThread, Sleep};

use super::client_exports::{HudFrameFn, REAL_HUD_FRAME};
use super::engfuncs;

/// Registers `callback` to run once per rendered frame.
///
/// Normally driven by the real `HUD_Frame` trampoline. A ~60Hz timer thread
/// is started as a safety net for the case where the `HUD_Frame` hook never
/// lands (an engine build that resolves it by some path we don't intercept).
/// It stands by while the hook is in the client's table, so the callbacks run
/// on the engine thread only, and retires once a real frame arrives.
///
/// Standing by matters: the callbacks patch `client.dll` code, and two threads
/// patching the same byte crash the game. On the first frame the settings.cfg
/// cvars flip, the timer thread and `HUD_Frame` both saw it, and one thread's
/// `VirtualProtect` restore landed between the other's unprotect and its
/// write (`spectator_eye`, `spectator_follow`: an access violation writing
/// `client.dll` code, about 1 launch in 10).
static PER_FRAME_CALLBACK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();
static TIMER_THREAD_STARTED: AtomicBool = AtomicBool::new(false);
/// Set by the `HUD_Frame` trampoline the first time the engine actually
/// renders a frame through us; retires the fallback timer thread.
static HUD_FRAME_DRIVING: AtomicBool = AtomicBool::new(false);

/// Whether the per-frame callbacks run on the engine's own thread (from
/// `HUD_Frame`), so they may call into VGUI, rather than on the fallback timer
/// thread.
pub(crate) fn hud_frame_driving() -> bool {
    HUD_FRAME_DRIVING.load(Ordering::Acquire)
}
/// Held while the per-frame callbacks run, so the timer thread and
/// `HUD_Frame` can never run them at the same time, even in the moment a late
/// hook takes over. A try-lock: whoever finds it taken skips that frame
/// instead of waiting, since this is the frame path.
static PER_FRAME_RUNNING: AtomicBool = AtomicBool::new(false);

/// A turn at running the per-frame callbacks; the flag clears when it drops.
struct PerFrameTurn<'a>(&'a AtomicBool);

impl<'a> PerFrameTurn<'a> {
    fn try_take(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for PerFrameTurn<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// What the fallback timer thread does on one tick.
#[derive(Debug, PartialEq, Eq)]
enum TimerStep {
    /// A real `HUD_Frame` has run: the hook drives from here on.
    Retire,
    /// The hook is in the client's table, so the engine will call it: leave
    /// the callbacks to the engine thread.
    StandBy,
    /// No hook: this thread is the only thing driving the callbacks.
    Drive,
}

fn timer_step(hud_frame_driving: bool, hud_frame_hooked: bool) -> TimerStep {
    if hud_frame_driving {
        TimerStep::Retire
    } else if hud_frame_hooked {
        TimerStep::StandBy
    } else {
        TimerStep::Drive
    }
}

fn current_timer_step() -> TimerStep {
    timer_step(
        HUD_FRAME_DRIVING.load(Ordering::Acquire),
        !REAL_HUD_FRAME.load(Ordering::Acquire).is_null(),
    )
}

/// Runs immediately before the main per-frame callback.
///
/// Exists so the cvar poll can write the flags `anim_fix::apply()` then reads
/// on the same frame, without either module having to know about the other.
/// Separate `OnceLock`s rather than a list because this is the frame path:
/// no allocation, no locking.
static PER_FRAME_PROLOGUE: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

pub fn set_per_frame_prologue(callback: fn()) {
    let _ = PER_FRAME_PROLOGUE.set(callback);
}

pub fn set_per_frame_callback(callback: fn()) {
    let _ = PER_FRAME_CALLBACK.set(callback);
    if !TIMER_THREAD_STARTED.swap(true, Ordering::AcqRel) {
        unsafe {
            CreateThread(
                std::ptr::null(),
                0,
                Some(per_frame_timer_thread),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            );
        }
    }
}

fn run_per_frame_callback() {
    if let Some(prologue) = PER_FRAME_PROLOGUE.get() {
        prologue();
    }
    if let Some(callback) = PER_FRAME_CALLBACK.get() {
        callback();
    }
}

unsafe extern "system" fn per_frame_timer_thread(_lp_param: *mut c_void) -> u32 {
    loop {
        unsafe { Sleep(16) };
        match current_timer_step() {
            TimerStep::Retire => {
                unsafe {
                    crate::debug::report(
                        "per-frame: real HUD_Frame hook is driving, retiring the fallback timer thread",
                    )
                };
                return 0;
            }
            TimerStep::StandBy => {}
            TimerStep::Drive => {
                let Some(_turn) = PerFrameTurn::try_take(&PER_FRAME_RUNNING) else {
                    continue;
                };
                // Asked again holding the turn: a hook that landed since the
                // first look gets the frame instead.
                if current_timer_step() == TimerStep::Drive {
                    run_per_frame_callback();
                }
            }
        }
    }
}

/// Real per-frame tick. Runs `client.dll`'s own `HUD_Frame` first so our
/// callback observes the state the engine just finished producing.
/// Seconds of playback elapsed, as raw f64 bits, summed from the per-frame
/// deltas `HUD_Frame` is passed.
///
/// Free: the engine already passes the delta every frame and it was otherwise
/// discarded. Playback time is what a log line has to be matched against
/// something seen on screen -- wall-clock time cannot be, once playback is
/// paused, seeked or fast-forwarded.
///
/// The value `HUD_Frame(double time)` receives is the frame's *duration*
/// (`host_frametime`, 6-11ms at ordinary framerates), **not** a clock. Storing
/// it directly was a bug: every log line read `[demo 0.006]` no matter how far
/// into the demo it was, and `anim_fix`'s fire-dedup window ended up comparing
/// two frame durations, which put it permanently inside its own window and
/// suppressed every firing animation. Summing the deltas gives the elapsed
/// reading both uses actually wanted.
///
/// Elapsed since the client loaded rather than since the current demo started:
/// loading a second demo in the same session does not reset it, because the
/// engine gives no clean signal to reset on. Monotonic within a session, which
/// is all either caller needs.
static DEMO_ELAPSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The engine's own client clock (`cl.time`), which the game's HUD timers
/// (kill feed, centre text, the clan countdown) run on; `None` before the
/// engine is ready.
pub fn engine_client_time() -> Option<f32> {
    let engfuncs = engfuncs()?;
    // Safety: the engine's own accessor, called on its thread.
    Some(unsafe { (engfuncs.pfn_get_client_time)() })
}

/// Seconds of playback elapsed, or 0 before the first frame.
pub fn client_time() -> f64 {
    f64::from_bits(DEMO_ELAPSED.load(Ordering::Relaxed))
}

pub(super) unsafe extern "C" fn tramp_hud_frame(time: f64) {
    // Only ever called from the engine thread, so a plain load/store pair needs
    // no stronger ordering than this.
    let elapsed = f64::from_bits(DEMO_ELAPSED.load(Ordering::Relaxed));
    // Guard against a nonsense delta rather than letting it poison the running
    // total permanently: a hitch, a breakpoint or a paused demo can hand back a
    // very large frametime, and NaN would make every later comparison false.
    if time.is_finite() && time > 0.0 {
        DEMO_ELAPSED.store((elapsed + time.min(1.0)).to_bits(), Ordering::Relaxed);
    }

    let real = REAL_HUD_FRAME.load(Ordering::Acquire);
    if !real.is_null() {
        let real: HudFrameFn = unsafe { std::mem::transmute(real) };
        unsafe { real(time) };
    }

    if !HUD_FRAME_DRIVING.swap(true, Ordering::AcqRel) {
        unsafe { crate::debug::report("HUD_Frame: first real per-frame callback received") };
    }
    // Taken only if the timer thread is mid-run, which can happen once, on the
    // frame a late hook takes over; that frame's callbacks are skipped.
    if let Some(_turn) = PerFrameTurn::try_take(&PER_FRAME_RUNNING) {
        run_per_frame_callback();
    }
}

#[cfg(test)]
mod per_frame_tests {
    use super::*;

    /// The timer thread drives only when there is no hook to do it, and gives
    /// way for good once a real frame has run.
    #[test]
    fn the_timer_thread_drives_only_without_a_hook() {
        assert_eq!(timer_step(false, false), TimerStep::Drive);
        assert_eq!(timer_step(false, true), TimerStep::StandBy);
        assert_eq!(timer_step(true, true), TimerStep::Retire);
        assert_eq!(timer_step(true, false), TimerStep::Retire);
    }

    /// One turn at a time, and the turn comes back when it drops.
    #[test]
    fn a_per_frame_turn_excludes_a_second_runner() {
        let flag = AtomicBool::new(false);
        let first = PerFrameTurn::try_take(&flag);
        assert!(first.is_some());
        assert!(PerFrameTurn::try_take(&flag).is_none());
        drop(first);
        assert!(PerFrameTurn::try_take(&flag).is_some());
    }

    /// Two threads hammering the same turn never both hold it: the property
    /// the first-frame crash broke.
    #[test]
    fn two_threads_never_hold_the_turn_together() {
        use std::sync::atomic::AtomicU32;
        static FLAG: AtomicBool = AtomicBool::new(false);
        static INSIDE: AtomicU32 = AtomicU32::new(0);
        static OVERLAPS: AtomicU32 = AtomicU32::new(0);
        let runner = || {
            for _ in 0..20_000 {
                if let Some(_turn) = PerFrameTurn::try_take(&FLAG) {
                    if INSIDE.fetch_add(1, Ordering::SeqCst) != 0 {
                        OVERLAPS.fetch_add(1, Ordering::SeqCst);
                    }
                    std::hint::spin_loop();
                    INSIDE.fetch_sub(1, Ordering::SeqCst);
                }
            }
        };
        let a = std::thread::spawn(runner);
        let b = std::thread::spawn(runner);
        a.join().unwrap();
        b.join().unwrap();
        assert_eq!(OVERLAPS.load(Ordering::SeqCst), 0);
    }
}
