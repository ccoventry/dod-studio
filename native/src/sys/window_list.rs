//! Another process's top-level windows: listing them, which process owns
//! one, and a process's visible window. One set of user32 declarations for
//! `sys::minimize`, `sys::dialogs` and `sys::bring_forward`.

pub type Hwnd = isize;
pub(super) type Lparam = isize;
pub(super) type Bool = i32;
pub(super) type EnumProc = unsafe extern "system" fn(Hwnd, Lparam) -> Bool;

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(callback: EnumProc, param: Lparam) -> Bool;
    fn GetWindowThreadProcessId(window: Hwnd, pid: *mut u32) -> u32;
    fn IsWindowVisible(window: Hwnd) -> Bool;
}

/// An `EnumWindows` / `EnumChildWindows` callback that pushes each window
/// into the `&mut Vec<Hwnd>` passed as `param`.
///
/// # Safety
///
/// `param` must be a `*mut Vec<Hwnd>` that outlives the enumeration.
pub(super) unsafe extern "system" fn collect(window: Hwnd, param: Lparam) -> Bool {
    // SAFETY: the caller passes a `&mut Vec<Hwnd>` alive for the whole
    // enumeration (the contract above).
    let found = unsafe { &mut *(param as *mut Vec<Hwnd>) };
    found.push(window);
    1
}

/// Every top-level window on the desktop, in z-order.
pub fn top_level_windows() -> Vec<Hwnd> {
    let mut found: Vec<Hwnd> = Vec::new();
    // SAFETY: `collect` only pushes into `found`, which outlives the call.
    unsafe { EnumWindows(collect, &mut found as *mut Vec<Hwnd> as Lparam) };
    found
}

/// The pid of the process that owns `window`; 0 for a window that has closed
/// (or for no window).
pub fn owner(window: Hwnd) -> u32 {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer; a closed window only fails.
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    pid
}

/// Whether `window` is visible; false for one that has closed.
pub fn is_visible(window: Hwnd) -> bool {
    // SAFETY: plain query on a window handle; a stale one returns 0.
    unsafe { IsWindowVisible(window) != 0 }
}

/// A visible top-level window of `pid`, if it has one yet.
pub fn visible_window(pid: u32) -> Option<Hwnd> {
    top_level_windows()
        .into_iter()
        .find(|&window| owner(window) == pid && is_visible(window))
}
