//! Keeps the VCR bar's time slider in step with playback while the bar is
//! hidden, so the first ESC after a second `viewdemo`, or after a demo started
//! from the launch line, doesn't throw playback back to the demo's start
//! (#663; stock DoD does it too).
//!
//! ## What GameUI does
//!
//! `CDemoPlayerDialog::OnThink` (pre-Anniversary `GameUI.dll+0x1f9f0`, 25th
//! Anniversary `+0x26b50`) runs only while the bar shows. Each time it sets
//! the slider's range to the demo's first and last frame, then compares the
//! slider's value with the one it set itself last time (`m_iLastValue`):
//!
//! - different: it takes that as the user dragging the slider and calls
//!   `SetWorldTime(value)`, then pauses when the old value was above 0;
//! - the same: it puts the slider at the current time and remembers that.
//!
//! While the bar is hidden none of that runs, so after a second `viewdemo`
//! the slider still holds the first demo's state (traced: its value lands on
//! the new range's minimum) and `m_iLastValue` the first demo's time. The
//! first think after ESC sees them differ and "seeks" to the slider's value:
//! the demo's first whole second, paused (traced on PRE: `SetWorldTime(726.00)`
//! then `SetPaused(1)`, from 1706 s into the demo).
//!
//! ## The fix
//!
//! Every frame while the bar is hidden and a demo plays, [`update`] does what
//! that think would do with nothing dragged: the slider's range to the demo's
//! span, its value to the current second, and `m_iLastValue` to match. The
//! first think after ESC then finds nothing changed.
//!
//! It runs whether or not our window was ever opened. A demo started from the
//! launch line (`+viewdemo`, as previews and batches start one) plays before
//! the `viewdemo` wrapper is in, so the window never opens; while this ran
//! only with the window built, the first ESC of such a game seeked to the
//! demo's first whole second (traced on both builds: `SetWorldTime(293.00)`
//! from 812 s on PRE, `SetWorldTime(4.00)` from 739 s on POST), and the
//! player's gap skip then carried it to the first playback frame.
//!
//! It also runs while the demo is still being read. Playback starts long
//! before the read ends (13-18 s for a 21-minute demo), and an ESC in that
//! time found the slider as the previous demo left it: playback jumped to
//! 8:11, then 8:15 on a second try, three seconds into a demo loaded from the
//! Demos tab right after another (POST, both ESCs at 20-30% read). The
//! player's start and end are the span read so far while it reads, the
//! range the think sets too.
//!
//! The dialog's fields are per GameUI build ([`BUILDS`]); before writing, the
//! dialog's player pointer must be the demo player's own interface, and the
//! slider pointer must be the bar's `TimeSlider` (or the one lent to the
//! Playback tab).
//! `tools/verify_vcr_sync_offsets.py` checks the table against both builds.

use std::ffi::c_void;
use std::sync::atomic::AtomicBool;

use super::*;

/// One GameUI build's `CDemoPlayerDialog` layout.
struct Build {
    name: &'static str,
    time_date_stamp: u32,
    size_of_image: u32,
    /// `IDemoPlayer *`, the slider and `m_iLastValue`.
    player: usize,
    slider: usize,
    last_value: usize,
}

const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_cefc,
        size_of_image: 0xe3000,
        player: 0x28c,
        slider: 0x250,
        last_value: 0x29c,
    },
    Build {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_99b2,
        size_of_image: 0xdf000,
        player: 0x294,
        slider: 0x258,
        last_value: 0x2a4,
    },
];

/// `vgui::Slider` slots, the same in both builds (`OnThink` calls them).
static REPORTED: AtomicBool = AtomicBool::new(false);

const SLIDER_SET_VALUE: usize = 0x218 / 4;
const SLIDER_GET_VALUE: usize = 0x21c / 4;
const SLIDER_SET_RANGE: usize = 0x220 / 4;

type SetValueFn = unsafe extern "thiscall" fn(*mut c_void, i32, u32);
type GetValueFn = unsafe extern "thiscall" fn(*mut c_void) -> i32;
type SetRangeFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32);

fn build() -> Option<&'static Build> {
    let base = module(c"GameUI.dll")?;
    // Safety: a module the loader mapped.
    let (stamp, size) = unsafe { crate::pe::image_identity(base as *mut u8) }?;
    BUILDS
        .iter()
        .find(|b| b.time_date_stamp == stamp && b.size_of_image == size)
}

/// The values `OnThink` would leave: the range in whole seconds and the
/// current second, which it truncates the same way.
fn slider_values(now: f64, start: f64, end: f64) -> (i32, i32, i32) {
    (start as i32, end as i32, now as i32)
}

/// Every frame. Cheap when there is no demo or the bar shows.
pub(super) unsafe fn update(vgui: &Vgui) {
    // It runs from the first frame, and reading the demo player's clock
    // during start-up crashed DemoPlayer.dll (`schedule.rs`); `viewdemo`
    // loads Core.dll, so without it there is no demo to follow.
    if module(c"Core.dll").is_none() {
        return;
    }
    let Some(clock) = crate::demo_seek::clock() else {
        return;
    };
    if !clock.active {
        return;
    }
    let Some(build) = build() else {
        return;
    };
    unsafe {
        let Some(bar) = vgui.bar() else {
            return;
        };
        if vgui.shown(bar) {
            return;
        }
        let dialog = vgui.object(bar) as *mut u8;
        if dialog.is_null() {
            return;
        }
        let player = (dialog.add(build.player) as *const *mut c_void).read_unaligned();
        if player.is_null() || Some(player) != crate::demo_seek::player_object() {
            return;
        }
        let slider = (dialog.add(build.slider) as *const *mut c_void).read_unaligned();
        if slider.is_null() || !is_time_slider(vgui, bar, slider) {
            return;
        }
        let (min, max, now) = slider_values(clock.now, clock.start, clock.end);
        let last = dialog.add(build.last_value) as *mut i32;
        let get: GetValueFn = slot(slider, SLIDER_GET_VALUE);
        if last.read_unaligned() == now && get(slider) == now {
            return;
        }
        let set_range: SetRangeFn = slot(slider, SLIDER_SET_RANGE);
        let set_value: SetValueFn = slot(slider, SLIDER_SET_VALUE);
        set_range(slider, min, max);
        // No change message: nothing listens for it while hidden, and the
        // think compares values, not messages.
        set_value(slider, now, 0);
        last.write_unaligned(get(slider));
        if !REPORTED.swap(true, Ordering::Relaxed) {
            crate::debug::report(&format!(
                "studio_panel: the hidden VCR bar's slider follows playback ({} GameUI, #663)",
                build.name
            ));
        }
    }
}

/// Whether `slider` is the bar's `TimeSlider`: still on the bar, or lent to
/// our Playback tab.
unsafe fn is_time_slider(vgui: &Vgui, bar: Vpanel, slider: *mut c_void) -> bool {
    unsafe {
        if let Some(child) = vgui.child_named(bar, "TimeSlider") {
            return vgui.object(child) == slider;
        }
        let page = vpanel_of(PAGE_OBJECTS[PLAYBACK_PAGE].load(Ordering::Acquire) as *mut c_void);
        page != 0
            && vgui
                .child_named(page, "TimeSlider")
                .is_some_and(|child| vgui.object(child) == slider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_truncate_like_the_think() {
        assert_eq!(slider_values(1706.57, 726.21, 2012.51), (726, 2012, 1706));
    }

    #[test]
    fn the_slots_are_the_ones_on_think_calls() {
        assert_eq!(
            (SLIDER_SET_VALUE, SLIDER_GET_VALUE, SLIDER_SET_RANGE),
            (134, 135, 136)
        );
    }
}
