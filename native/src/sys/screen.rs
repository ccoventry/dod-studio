//! The primary screen's size, for the capture resolution a new install starts
//! at (Studio's `settings_manager::default_resolution`) and for whether the
//! game's window fits the screen with a title bar (#1337).
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

/// Rows a titled window's frame needs beyond its client area: the caption
/// and borders took 39 at 100 % display scaling (#1337), with room for a
/// bigger scaling's taller caption.
const TITLED_FRAME_ROWS: i32 = 64;
/// Columns the same frame needs: 16 at 100 %.
const TITLED_FRAME_COLUMNS: i32 = 32;

/// Whether the pre-Anniversary game's window needs `-noborder` to get a
/// client area of `size` on a screen of `screen` (the caller checks the
/// build). The pre-Anniversary build opens a
/// resizable window, which Windows caps at about the screen's size plus 20
/// rows; with its 39-row frame, a 2560x1440 capture on a 1440-row screen got
/// a 2560x1421 client area, so PRE drew 1421 rows of a 1440-row frame: 19
/// black rows on top and the picture 9 px low (#1337). The Anniversary
/// build's window isn't resizable and isn't capped. A borderless window has
/// no frame to subtract on either. `None` (the size can't be read) keeps the
/// title bar.
pub fn needs_borderless_window(size: (i32, i32), screen: Option<(i32, i32)>) -> bool {
    let Some((screen_width, screen_height)) = screen else {
        return false;
    };
    let (width, height) = size;
    width.saturating_add(TITLED_FRAME_COLUMNS) > screen_width
        || height.saturating_add(TITLED_FRAME_ROWS) > screen_height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_as_tall_as_the_screen_goes_borderless() {
        // The case #1337 measured: 2560x1440 on a 3440x1440 screen.
        assert!(needs_borderless_window((2560, 1440), Some((3440, 1440))));
        assert!(needs_borderless_window((1920, 1080), Some((1920, 1080))));
        assert!(needs_borderless_window((1920, 1050), Some((1920, 1080))));
    }

    #[test]
    fn a_window_as_wide_as_the_screen_goes_borderless() {
        assert!(needs_borderless_window((3440, 1080), Some((3440, 1440))));
    }

    #[test]
    fn a_window_that_fits_with_its_title_bar_keeps_it() {
        assert!(!needs_borderless_window((1280, 720), Some((3440, 1440))));
        assert!(!needs_borderless_window((1920, 1080), Some((2560, 1440))));
        assert!(!needs_borderless_window((2560, 1440), Some((3840, 2160))));
    }

    #[test]
    fn an_unknown_screen_keeps_the_title_bar() {
        assert!(!needs_borderless_window((2560, 1440), None));
    }

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
