//! Progress while a demo is parsed, in bytes of the file read so far.
//!
//! Reading a demo's structure is most of an analysis' time (1.0 s of 1.17 s
//! for a 72 MB demo), and until now nothing reported from inside it, so every
//! progress bar sat at 0% and then jumped. [`with_progress`] runs a parse with
//! a callback that `parse_frame` calls about every 1% of the file.
//!
//! The callback is held in a thread-local for the duration of the parse only,
//! so the parser's own signatures stay as they are.

use std::cell::Cell;

type Hook = *mut (dyn FnMut(usize, usize) + 'static);

thread_local! {
    static HOOK: Cell<Option<Hook>> = const { Cell::new(None) };
    /// The next byte count worth reporting.
    static NEXT: Cell<usize> = const { Cell::new(0) };
}

/// Runs `parse` (on this thread) with `progress(bytes_read, bytes_total)`
/// called as frames are read, about every 1% of the file.
pub fn with_progress<R>(progress: &mut dyn FnMut(usize, usize), parse: impl FnOnce() -> R) -> R {
    let hook: *mut (dyn FnMut(usize, usize) + '_) = progress;
    // Safety: only the lifetime bound is erased. The pointer is used only
    // inside `parse`, while `progress` is still mutably borrowed here, and the
    // guard below removes it again on return or unwind.
    let hook: Hook = unsafe { std::mem::transmute(hook) };
    struct Restore(Option<Hook>, usize);
    impl Drop for Restore {
        fn drop(&mut self) {
            HOOK.with(|h| h.set(self.0));
            NEXT.with(|n| n.set(self.1));
        }
    }
    let _restore = Restore(
        HOOK.with(|h| h.replace(Some(hook))),
        NEXT.with(|n| n.replace(0)),
    );
    parse()
}

/// Called by the parser after each frame: `read` bytes of `total` so far.
pub(crate) fn report(read: usize, total: usize) {
    let Some(hook) = HOOK.with(|h| h.get()) else {
        return;
    };
    if read < NEXT.with(|n| n.get()) {
        return;
    }
    NEXT.with(|n| n.set(read + total / 100 + 1));
    // Safety: set by `with_progress`, which is still on the stack.
    unsafe { (*hook)(read, total) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_about_every_percent_only_while_asked() {
        report(10, 100); // no hook: nothing happens
        let mut seen = Vec::new();
        with_progress(&mut |read, total| seen.push((read, total)), || {
            for read in (0..=1000).step_by(3) {
                report(read, 1000);
            }
        });
        assert!(seen.len() > 50 && seen.len() <= 101, "{}", seen.len());
        assert!(seen.windows(2).all(|w| w[1].0 >= w[0].0 + 10));
        let after = seen.len();
        report(999, 1000);
        assert_eq!(seen.len(), after);
    }
}
