//! A job object that ends the processes put in it when Studio exits (#1439).
//!
//! `kill_on_drop` only fires when a child's handle is dropped, and a process
//! that exits (the window closed, the app quit, a crash) drops nothing: three
//! render FFmpegs outlived a closed Studio that way. Windows closes every
//! handle of a process that ends, however it ends, so a job with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` whose only handle is Studio's ends
//! its processes with Studio. Jobs nest (Windows 8 and later), so this works
//! inside cargo's own job too (`super::job`).
//!
//! Only processes Studio owns go in it. The game is started outside every job
//! on purpose (`super::wmi_launch`, #545) and never joins this one.

/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
#[cfg(windows)]
const KILL_ON_JOB_CLOSE: u32 = 0x2000;

/// A job whose processes end when its handle closes: when it is dropped, or
/// when Studio exits.
pub struct ChildJob {
    #[cfg(windows)]
    handle: isize,
}

impl ChildJob {
    /// A new, empty job, or why Windows refused to make one.
    pub fn new() -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            win::create_kill_on_close_job().map(|handle| Self { handle })
        }
        #[cfg(not(windows))]
        {
            Ok(Self {})
        }
    }

    /// Puts the running process `child` in the job, so it ends with the job.
    /// Fails when the process has already exited or Windows refuses.
    #[cfg_attr(not(windows), allow(unused_variables))]
    pub fn assign(&self, child: &tokio::process::Child) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            let process = child
                .raw_handle()
                .ok_or_else(|| std::io::Error::other("the process has already exited"))?;
            win::assign(self.handle, process as isize)
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }

    /// Ends every process in the job now. The job stays usable for new ones.
    pub fn end_all(&self) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            win::terminate(self.handle)
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }
}

#[cfg(windows)]
impl Drop for ChildJob {
    fn drop(&mut self) {
        win::close(self.handle);
    }
}

/// The job every process that must not outlive Studio joins. Never dropped:
/// its handle closes when Studio's process ends. `None` when Windows would
/// not make one, which `end_with_studio` reports.
fn studio_job() -> Option<&'static ChildJob> {
    static JOB: std::sync::OnceLock<Option<ChildJob>> = std::sync::OnceLock::new();
    JOB.get_or_init(|| ChildJob::new().ok()).as_ref()
}

/// Makes `child` end when Studio exits, however Studio exits. An error says
/// why it could not, for the caller to log; the child still runs.
pub fn end_with_studio(child: &tokio::process::Child) -> std::io::Result<()> {
    studio_job()
        .ok_or_else(|| std::io::Error::other("Windows would not make a job object"))?
        .assign(child)
}

/// Ends every process `end_with_studio` was given, at once. For Studio's own
/// way out, before it removes what they were writing.
pub fn end_all_with_studio() -> std::io::Result<()> {
    match studio_job() {
        Some(job) => job.end_all(),
        None => Ok(()),
    }
}

#[cfg(windows)]
mod win {
    use super::KILL_ON_JOB_CLOSE;

    /// `JobObjectExtendedLimitInformation`.
    const EXTENDED_LIMIT_INFORMATION: i32 = 9;
    /// `sizeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION)` on x64.
    const EXTENDED_LIMIT_INFORMATION_SIZE: usize = 144;
    /// Where `BasicLimitInformation.LimitFlags` sits in it: after two
    /// `LARGE_INTEGER`s.
    const LIMIT_FLAGS_OFFSET: usize = 16;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const u8, name: *const u16) -> isize;
        fn SetInformationJobObject(job: isize, class: i32, info: *const u8, length: u32) -> i32;
        fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
        fn TerminateJobObject(job: isize, exit_code: u32) -> i32;
        // Declared as `junction.rs` and `modules.rs` do: one signature per
        // import, or the compiler warns.
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }

    fn last_error() -> std::io::Error {
        std::io::Error::last_os_error()
    }

    pub fn create_kill_on_close_job() -> std::io::Result<isize> {
        // SAFETY: no attributes and no name: an unnamed job only we hold.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job == 0 {
            return Err(last_error());
        }
        // u64s, so the buffer has the struct's 8-byte alignment.
        let mut info = [0u64; EXTENDED_LIMIT_INFORMATION_SIZE / 8];
        info[LIMIT_FLAGS_OFFSET / 8] = u64::from(KILL_ON_JOB_CLOSE);
        // SAFETY: `info` is the size passed and outlives the call.
        let ok = unsafe {
            SetInformationJobObject(
                job,
                EXTENDED_LIMIT_INFORMATION,
                info.as_ptr().cast(),
                EXTENDED_LIMIT_INFORMATION_SIZE as u32,
            )
        };
        if ok == 0 {
            let error = last_error();
            close(job);
            return Err(error);
        }
        Ok(job)
    }

    pub fn assign(job: isize, process: isize) -> std::io::Result<()> {
        // SAFETY: both handles are open: the job is ours, the process handle
        // belongs to a child the caller still holds.
        if unsafe { AssignProcessToJobObject(job, process) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn terminate(job: isize) -> std::io::Result<()> {
        /// The exit code the ended processes report: 1, like `Child::kill`.
        const ENDED_EXIT_CODE: u32 = 1;
        // SAFETY: the job handle is open for as long as its `ChildJob` lives.
        if unsafe { TerminateJobObject(job, ENDED_EXIT_CODE) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn close(handle: isize) {
        // SAFETY: called once, from `Drop` or on a job never handed out.
        // Closing our own handle can only fail if it were already closed.
        unsafe { CloseHandle(handle as *mut std::ffi::c_void) };
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::ChildJob;
    use std::time::Duration;

    /// A process that runs for about a minute unless ended, with no window.
    fn long_child() -> tokio::process::Child {
        let mut cmd = tokio::process::Command::new("ping");
        cmd.args(["-n", "60", "127.0.0.1"]);
        cmd.creation_flags(0x08000000);
        cmd.stdout(std::process::Stdio::null());
        cmd.kill_on_drop(true);
        cmd.spawn().unwrap()
    }

    async fn exits_within(child: &mut tokio::process::Child, wait: Duration) -> bool {
        tokio::time::timeout(wait, child.wait()).await.is_ok()
    }

    #[tokio::test]
    async fn ending_the_job_ends_its_processes() {
        let job = ChildJob::new().unwrap();
        let mut child = long_child();
        job.assign(&child).unwrap();
        job.end_all().unwrap();
        assert!(exits_within(&mut child, Duration::from_secs(5)).await);
    }

    /// The same as Studio's process ending: its last handle to the job closes.
    #[tokio::test]
    async fn closing_the_job_ends_its_processes() {
        let job = ChildJob::new().unwrap();
        let mut child = long_child();
        job.assign(&child).unwrap();
        drop(job);
        assert!(exits_within(&mut child, Duration::from_secs(5)).await);
    }

    #[tokio::test]
    async fn a_process_outside_the_job_keeps_running() {
        let job = ChildJob::new().unwrap();
        let mut inside = long_child();
        let mut outside = long_child();
        job.assign(&inside).unwrap();
        job.end_all().unwrap();
        assert!(exits_within(&mut inside, Duration::from_secs(5)).await);
        assert!(!exits_within(&mut outside, Duration::from_millis(300)).await);
        outside.kill().await.unwrap();
    }
}
