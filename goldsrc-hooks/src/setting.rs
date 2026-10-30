//! The two shapes every `ds_*` setting takes in the hook DLL (#1204):
//!
//! - [`Cvar`]: the pointer the engine hands back when `commands/install.rs`
//!   registers a cvar. Null until then, and for good if the registration
//!   failed, so an unregistered setting reads as off (or as no value).
//! - [`Setting`]: a [`Cvar`] with a name and a fallback flag, for a setting
//!   whose plain toggle command stands in when the cvar can't be registered.
//!
//! Both are read every frame from the engine's thread (`commands::poll`) and
//! from hot hooks, so a read is one atomic pointer load and one float load:
//! on x86 an `Acquire` load is a plain `mov`, the same instruction the
//! `Relaxed` loads this replaced compiled to. No lock, no allocation.

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::engine::CvarSPartial;

/// One registered engine cvar, or none yet.
pub struct Cvar {
    ptr: AtomicPtr<CvarSPartial>,
}

impl Cvar {
    pub const fn new() -> Self {
        Self {
            ptr: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    /// Called by `commands/install.rs` once the cvar is registered.
    pub fn set(&self, cvar: *mut CvarSPartial) {
        self.ptr.store(cvar, Ordering::Release);
    }

    /// The cvar's value, or `None` while it isn't registered.
    #[inline]
    pub fn value(&self) -> Option<f32> {
        let cvar = self.ptr.load(Ordering::Acquire);
        if cvar.is_null() {
            return None;
        }
        // Safety: a non-null pointer is one `pfnRegisterVariable` returned
        // (or `pfnGetCvarPointer` found), and the engine keeps every cvar for
        // the session. Only `value` is read, at its checked offset
        // (`engine::CvarSPartial`).
        Some(unsafe { (*cvar).value })
    }

    /// Whether the cvar is non-zero, or `None` while it isn't registered.
    #[inline]
    pub fn try_on(&self) -> Option<bool> {
        self.value().map(|value| value != 0.0)
    }

    /// Whether the cvar is non-zero; off while it isn't registered.
    #[inline]
    pub fn on(&self) -> bool {
        self.try_on().unwrap_or(false)
    }

    /// The raw pointer, null while unregistered, for the few modules that
    /// write the cvar back or read a field other than `value`.
    #[inline]
    pub fn ptr(&self) -> *mut CvarSPartial {
        self.ptr.load(Ordering::Acquire)
    }
}

/// An on/off setting: its cvar or, when that couldn't be registered, the
/// fallback toggle command's flag.
pub struct Setting {
    pub name: &'static str,
    /// The fallback toggle's state; starts at the cvar's default.
    pub fallback: AtomicBool,
    pub cvar: Cvar,
}

impl Setting {
    pub const fn new(name: &'static str, on: bool) -> Self {
        Self {
            name,
            fallback: AtomicBool::new(on),
            cvar: Cvar::new(),
        }
    }

    /// Called by `commands/install.rs` once the cvar is registered.
    pub fn set_cvar(&self, cvar: *mut CvarSPartial) {
        self.cvar.set(cvar);
    }

    /// The cvar's value, or `None` on the fallback path.
    #[inline]
    pub fn value(&self) -> Option<f32> {
        self.cvar.value()
    }

    #[inline]
    pub fn on(&self) -> bool {
        match self.cvar.try_on() {
            Some(on) => on,
            None => self.fallback.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_cvar(value: f32) -> CvarSPartial {
        CvarSPartial {
            name: std::ptr::null(),
            string: std::ptr::null(),
            flags: 0,
            value,
            next: std::ptr::null_mut(),
        }
    }

    #[test]
    fn an_unregistered_cvar_is_off_with_no_value() {
        let cvar = Cvar::new();
        assert_eq!(cvar.value(), None);
        assert_eq!(cvar.try_on(), None);
        assert!(!cvar.on());
        assert!(cvar.ptr().is_null());
    }

    #[test]
    fn a_registered_cvar_reads_the_engines_value() {
        let mut engine = engine_cvar(0.0);
        let engine: *mut CvarSPartial = &mut engine;
        let cvar = Cvar::new();
        cvar.set(engine);
        assert_eq!(cvar.try_on(), Some(false));
        unsafe { (*engine).value = 2.0 };
        assert_eq!(cvar.value(), Some(2.0));
        assert!(cvar.on());
        // Any non-zero value is on, negative included, as `!= 0.0` always was.
        unsafe { (*engine).value = -1.0 };
        assert!(cvar.on());
    }

    #[test]
    fn a_setting_follows_its_fallback_until_the_cvar_is_registered() {
        let mut engine = engine_cvar(0.0);
        let engine: *mut CvarSPartial = &mut engine;
        let setting = Setting::new("ds_test_setting", true);
        assert!(setting.on());
        setting.fallback.store(false, Ordering::Relaxed);
        assert!(!setting.on());
        setting.fallback.store(true, Ordering::Relaxed);
        setting.set_cvar(engine);
        assert!(!setting.on(), "the cvar wins once it's registered");
        unsafe { (*engine).value = 1.0 };
        assert!(setting.on());
    }
}
