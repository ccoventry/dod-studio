//! Keeps a game Studio starts from taking the foreground (#434).
//!
//! Windows lets a process take the foreground if the foreground process
//! started it. Studio is in front when the user clicks Start, so the game
//! HLAE starts for it inherited that right: with Studio left alone, the game
//! window opened in front, focused, and held the keyboard until the hook DLL
//! could step it aside a second and a half later (PRE, 2026-10-11, a
//! foreground log). When the user had clicked another window in the meantime,
//! the right was gone and the game opened behind.
//!
//! `LockSetForegroundWindow(LSFW_LOCK)`, called by the foreground process,
//! refuses every other process the foreground until it is unlocked or the user
//! changes the foreground themselves (a click, Alt). So Studio locks before it
//! starts the game and unlocks once the game has stepped aside.

/// Unlocks when dropped. Does nothing when Studio wasn't the foreground
/// process, since then the game can't take the foreground from it either.
pub struct ForegroundLock {
    locked: bool,
}

impl ForegroundLock {
    pub fn take() -> Self {
        Self {
            locked: win::set(true),
        }
    }

    /// Whether the lock took.
    pub fn locked(&self) -> bool {
        self.locked
    }

    pub fn release(&mut self) {
        if std::mem::take(&mut self.locked) {
            win::set(false);
        }
    }
}

impl Drop for ForegroundLock {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(windows)]
mod win {
    const LSFW_LOCK: u32 = 1;
    const LSFW_UNLOCK: u32 = 2;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn LockSetForegroundWindow(code: u32) -> i32;
    }

    pub fn set(lock: bool) -> bool {
        unsafe { LockSetForegroundWindow(if lock { LSFW_LOCK } else { LSFW_UNLOCK }) != 0 }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn set(_lock: bool) -> bool {
        false
    }
}
