//! Taking a `Mutex` or `RwLock` and carrying on when a panicking thread left
//! it poisoned (#1377). `analysis`, `goldsrc-hooks` and `hl-demo-auditor`
//! don't depend on `native`, so each keeps a private copy of these: one of
//! four copies until the helpers get one shared home.
//!
//! Release builds abort on a panic, so a lock is only ever poisoned in debug
//! and test builds, where one panicking thread would otherwise make every
//! later `.lock().unwrap()` panic too and bury the first failure. Use these
//! only where the data behind the lock is still valid after a panic
//! mid-update; if it might not be, unwrap and let the panic stand
//! (`docs/coding_standards.md`, section 4).

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Locks `mutex`, taking the guard even if the lock is poisoned.
pub fn lock_or_recover<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Read-locks `lock`, taking the guard even if the lock is poisoned.
pub fn read_or_recover<T: ?Sized>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

/// Write-locks `lock`, taking the guard even if the lock is poisoned.
pub fn write_or_recover<T: ?Sized>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Panics on another thread while holding the lock, which poisons it.
    fn poison_mutex(mutex: &Arc<Mutex<i32>>) {
        let held = Arc::clone(mutex);
        let _ = std::thread::spawn(move || {
            let _guard = held.lock().unwrap();
            panic!("poisoning the lock on purpose");
        })
        .join();
    }

    #[test]
    fn a_poisoned_mutex_still_locks_and_keeps_its_data() {
        let mutex = Arc::new(Mutex::new(7));
        poison_mutex(&mutex);
        assert!(mutex.is_poisoned());

        *lock_or_recover(&mutex) += 1;

        assert_eq!(*lock_or_recover(&mutex), 8);
    }

    #[test]
    fn a_poisoned_rwlock_still_reads_and_writes() {
        let lock = Arc::new(RwLock::new(String::from("kept")));
        let held = Arc::clone(&lock);
        let _ = std::thread::spawn(move || {
            let _guard = held.write().unwrap();
            panic!("poisoning the lock on purpose");
        })
        .join();
        assert!(lock.is_poisoned());

        assert_eq!(*read_or_recover(&lock), "kept");
        write_or_recover(&lock).push_str(" still");
        assert_eq!(*read_or_recover(&lock), "kept still");
    }

    #[test]
    fn an_unpoisoned_lock_behaves_as_usual() {
        let mutex = Mutex::new(1);
        assert_eq!(*lock_or_recover(&mutex), 1);
    }
}
