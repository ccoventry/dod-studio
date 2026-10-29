//! Releases the mouse after Studio ends the game (#1095).
//!
//! A game in front confines the pointer to its window with `ClipCursor`, a
//! desktop-wide setting. A game that is killed never releases it, and Windows
//! only resets it on the next foreground change, so after a batch taskkills
//! `hl.exe` the pointer stays boxed in until the user Alt+Tabs.

/// Lifts the pointer confinement, if there is one. Call it only once the game
/// has left the process list, so it never takes the mouse from a running game.
/// Returns whether a confinement was lifted.
#[cfg(windows)]
pub fn release() -> bool {
    if win::clipped() {
        win::unclip();
        true
    } else {
        false
    }
}

#[cfg(not(windows))]
pub fn release() -> bool {
    false
}

#[cfg(windows)]
mod win {
    #[repr(C)]
    #[derive(Default, PartialEq, Debug)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    const SM_XVIRTUALSCREEN: i32 = 76;
    const SM_YVIRTUALSCREEN: i32 = 77;
    const SM_CXVIRTUALSCREEN: i32 = 78;
    const SM_CYVIRTUALSCREEN: i32 = 79;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn ClipCursor(rect: *const Rect) -> i32;
        fn GetClipCursor(rect: *mut Rect) -> i32;
        fn GetSystemMetrics(index: i32) -> i32;
    }

    /// The whole desktop: what `GetClipCursor` reports with no confinement.
    fn desktop() -> Rect {
        unsafe {
            let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            Rect {
                left,
                top,
                right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
                bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
            }
        }
    }

    pub fn current() -> Option<Rect> {
        let mut rect = Rect::default();
        (unsafe { GetClipCursor(&mut rect) } != 0).then_some(rect)
    }

    pub fn clipped() -> bool {
        current().is_some_and(|rect| rect != desktop())
    }

    pub fn unclip() {
        unsafe {
            ClipCursor(std::ptr::null());
        }
    }

    #[cfg(test)]
    pub fn clip(rect: &Rect) {
        unsafe {
            ClipCursor(rect);
        }
    }

    /// Moves the real pointer's limits, so it only runs by hand:
    /// `cargo test -p native cursor_clip -- --ignored`.
    #[test]
    #[ignore]
    fn a_confinement_left_behind_is_lifted() {
        unclip();
        assert!(!clipped(), "nothing confined to start with");
        assert!(!super::release());
        clip(&Rect {
            left: 0,
            top: 0,
            right: 200,
            bottom: 200,
        });
        assert!(clipped());
        assert!(super::release());
        assert!(!clipped());
        assert_eq!(current(), Some(desktop()));
    }
}
