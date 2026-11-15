//! Whether DoD Studio runs inside a job object that ends every process in it
//! when the job closes (#545).
//!
//! `cargo run`, which `npm run tauri dev` uses, puts Studio in a job with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and nothing else: no
//! `JOB_OBJECT_LIMIT_BREAKAWAY_OK`, so `CREATE_BREAKAWAY_FROM_JOB` is refused
//! with "Access is denied" (measured 2026-10-30, cargo 1.98: limit flags
//! `0x2000`). Every process Studio starts joins that job, so closing or
//! rebuilding a dev Studio ended the game a batch was capturing with. An
//! installed build is started by the shell and is in no such job, or in one
//! that lets its processes outlive it.

/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
const KILL_ON_JOB_CLOSE: u32 = 0x2000;

/// Whether the job Studio runs in ends its processes when it closes. Read
/// once: a process never leaves its job.
pub fn kills_children_on_close() -> bool {
    static KILLS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *KILLS.get_or_init(|| win::limit_flags().is_some_and(|flags| flags & KILL_ON_JOB_CLOSE != 0))
}

#[cfg(windows)]
mod win {
    /// `JobObjectExtendedLimitInformation`.
    const EXTENDED_LIMIT_INFORMATION: i32 = 9;
    /// `sizeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION)` on x64.
    const EXTENDED_LIMIT_INFORMATION_SIZE: usize = 144;
    /// Where `BasicLimitInformation.LimitFlags` sits in it: after two
    /// `LARGE_INTEGER`s.
    const LIMIT_FLAGS_OFFSET: usize = 16;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn IsProcessInJob(process: isize, job: isize, result: *mut i32) -> i32;
        fn QueryInformationJobObject(
            job: isize,
            class: i32,
            info: *mut u8,
            length: u32,
            returned: *mut u32,
        ) -> i32;
    }

    /// The limit flags of the job this process is in, or `None` when it is in
    /// none or the job can't be read.
    pub fn limit_flags() -> Option<u32> {
        let mut in_job = 0;
        // SAFETY: the pseudo-handle needs no closing, and `in_job` outlives
        // the call.
        if unsafe { IsProcessInJob(GetCurrentProcess(), 0, &mut in_job) } == 0 || in_job == 0 {
            return None;
        }
        // u64s, so the buffer has the struct's 8-byte alignment.
        let mut info = [0u64; EXTENDED_LIMIT_INFORMATION_SIZE / 8];
        let mut returned = 0u32;
        // SAFETY: a null job handle means this process's own job; `info` is
        // the size passed.
        let ok = unsafe {
            QueryInformationJobObject(
                0,
                EXTENDED_LIMIT_INFORMATION,
                info.as_mut_ptr().cast(),
                EXTENDED_LIMIT_INFORMATION_SIZE as u32,
                &mut returned,
            )
        };
        if ok == 0 {
            return None;
        }
        let word = info[LIMIT_FLAGS_OFFSET / 8];
        Some((word & 0xffff_ffff) as u32)
    }
}

#[cfg(not(windows))]
mod win {
    pub fn limit_flags() -> Option<u32> {
        None
    }
}

#[cfg(test)]
mod tests {
    /// `cargo test` runs the tests inside cargo's job, the same one
    /// `cargo run` puts a dev Studio in. `CARGO` is set by cargo, so a test
    /// runner outside it is not held to this.
    #[cfg(windows)]
    #[test]
    fn finds_cargos_job() {
        if std::env::var_os("CARGO").is_some() {
            assert!(super::kills_children_on_close());
        }
    }
}
