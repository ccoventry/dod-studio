//! The primary screen's size, for the capture resolution a new install starts
//! at (Studio's `settings_manager::default_resolution`).
//!
//! Read with `GetDeviceCaps(DESKTOPHORZRES/DESKTOPVERTRES)` rather than
//! `GetSystemMetrics(SM_CXSCREEN)`: the latter is scaled by Windows' display
//! scaling in a process that is not DPI-aware, so a 1920x1080 screen at 125 %
//! would read as 1536x864. The device caps are the screen's real pixels.

/// The primary screen's width and height in pixels, or `None` when it can't
/// be read (no desktop, or not Windows).
#[cfg(windows)]
pub fn primary_screen_size() -> Option<(i32, i32)> {
    type Hdc = isize;
    const DESKTOPVERTRES: i32 = 117;
    const DESKTOPHORZRES: i32 = 118;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDC(window: isize) -> Hdc;
        fn ReleaseDC(window: isize, dc: Hdc) -> i32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetDeviceCaps(dc: Hdc, index: i32) -> i32;
    }

    // SAFETY: a null window asks for the whole screen's DC, released below;
    // GetDeviceCaps only reads from it.
    let (width, height) = unsafe {
        let dc = GetDC(0);
        if dc == 0 {
            return None;
        }
        let size = (
            GetDeviceCaps(dc, DESKTOPHORZRES),
            GetDeviceCaps(dc, DESKTOPVERTRES),
        );
        ReleaseDC(0, dc);
        size
    };
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(not(windows))]
pub fn primary_screen_size() -> Option<(i32, i32)> {
    None
}

/// The capture resolution for a screen of `size`: 1920x1080 when the screen
/// holds it, else 1280x720 (also when the size is unknown). Never bigger than
/// the screen, since the game opens a window of exactly that size.
pub fn default_capture_resolution(size: Option<(i32, i32)>) -> (i32, i32) {
    match size {
        Some((w, h)) if w >= 1920 && h >= 1080 => (1920, 1080),
        _ => (1280, 720),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_hd_only_when_the_screen_holds_it() {
        assert_eq!(default_capture_resolution(Some((1920, 1080))), (1920, 1080));
        assert_eq!(default_capture_resolution(Some((3840, 2160))), (1920, 1080));
        assert_eq!(default_capture_resolution(Some((2560, 1080))), (1920, 1080));
        assert_eq!(default_capture_resolution(Some((1920, 1050))), (1280, 720));
        assert_eq!(default_capture_resolution(Some((1680, 1050))), (1280, 720));
        assert_eq!(default_capture_resolution(Some((1366, 768))), (1280, 720));
        assert_eq!(default_capture_resolution(None), (1280, 720));
    }

    #[test]
    fn the_screen_size_reads_as_real_pixels_or_nothing() {
        if let Some((w, h)) = primary_screen_size() {
            assert!(w > 0 && h > 0);
        }
    }
}
