//! Configuration › Render's "Resumable renders" as Settings has it now
//! (#1451): each render reads it as it starts, the way the export folders
//! are followed (#1430, `render_export_dirs.rs`), so turning it off or on
//! reaches the renders a running batch starts next. Render While Capturing
//! keeps one render batch going for a whole capture batch, so a value read
//! once at Start could stay wrong for days.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The value Settings saved last. On until Settings load, as the setting's
/// own default.
#[derive(Clone)]
pub(crate) struct LiveResumable(Arc<AtomicBool>);

impl Default for LiveResumable {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}

impl LiveResumable {
    /// Settings were loaded or saved with `on`. Returns whether that changed
    /// it, for the activity log.
    pub fn saved(&self, on: bool) -> bool {
        self.0.swap(on, Ordering::SeqCst) != on
    }

    /// Whether a render starting now may be split into pieces.
    pub fn is_on(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_on_until_settings_say_otherwise() {
        assert!(LiveResumable::default().is_on());
    }

    /// The scheduler's copy sees a save made after the batch started.
    #[test]
    fn a_change_saved_mid_batch_reaches_the_next_render() {
        let settings_side = LiveResumable::default();
        let scheduler_side = settings_side.clone();
        assert!(!settings_side.saved(true), "the same value is no change");
        assert!(settings_side.saved(false));
        assert!(!scheduler_side.is_on());
        assert!(settings_side.saved(true));
        assert!(scheduler_side.is_on());
    }
}
