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

    /// The IDs of the processes in the job now (empty when Windows won't
    /// say).
    pub fn process_ids(&self) -> Vec<u32> {
        #[cfg(windows)]
        {
            win::process_ids(self.handle)
        }
        #[cfg(not(windows))]
        {
            Vec::new()
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
/// not make one, which the activity log says once, here.
fn studio_job() -> Option<&'static ChildJob> {
    static JOB: std::sync::OnceLock<Option<ChildJob>> = std::sync::OnceLock::new();
    JOB.get_or_init(|| match ChildJob::new() {
        Ok(job) => Some(job),
        Err(e) => {
            crate::log_markdown(&crate::messages::no_studio_job(e));
            None
        }
    })
    .as_ref()
}

/// What each process put in Studio's job is (`messages::FFMPEG_*`), by
/// process ID, so Studio's way out can log what it stopped.
fn names() -> &'static std::sync::Mutex<std::collections::HashMap<u32, &'static str>> {
    static NAMES: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<u32, &'static str>>,
    > = std::sync::OnceLock::new();
    NAMES.get_or_init(Default::default)
}

/// Above this many names, the ones of processes no longer in the job go.
const NAMES_KEPT: usize = 32;

fn remember(pid: u32, what: &'static str, job: &ChildJob) {
    // Each change is one insert or one retain: the map stays valid after a
    // panic mid-update.
    let mut names = lock_recover::lock_or_recover(names());
    if names.len() >= NAMES_KEPT {
        let running = job.process_ids();
        names.retain(|pid, _| running.contains(pid));
    }
    names.insert(pid, what);
}

/// Makes `child` end when Studio exits, however Studio exits. `what` names it
/// (`messages::FFMPEG_*`) for the exit's log line. An error says why it
/// could not, for the caller to log; the child still runs. Ok when Windows
/// would not make the job at all: that is logged once, not per child.
pub fn end_with_studio(child: &tokio::process::Child, what: &'static str) -> std::io::Result<()> {
    let Some(job) = studio_job() else {
        return Ok(());
    };
    job.assign(child)?;
    if let Some(pid) = child.id() {
        remember(pid, what, job);
    }
    Ok(())
}

/// Starts `cmd` and makes the child end with Studio, as a render's FFmpeg
/// does (#1496). When it can't be tied to Studio, the activity log says so,
/// naming `what`, and the child runs on anyway: the work still gets done,
/// and only a Studio that closes mid-run would leave it running (#1439).
pub fn spawn_ending_with_studio(
    cmd: &mut tokio::process::Command,
    what: &'static str,
) -> std::io::Result<tokio::process::Child> {
    let child = cmd.spawn()?;
    if let Err(e) = end_with_studio(&child, what) {
        crate::log_markdown(&crate::messages::not_tied_to_studio(what, e));
    }
    Ok(child)
}

/// [`spawn_ending_with_studio`], then waits for the child and collects its
/// output. Unlike `Command::output`, which pipes stdout and stderr itself,
/// the pipes are the ones `cmd` sets: set stdin, stdout and stderr on it.
pub async fn output_ending_with_studio(
    cmd: &mut tokio::process::Command,
    what: &'static str,
) -> std::io::Result<std::process::Output> {
    spawn_ending_with_studio(cmd, what)?
        .wait_with_output()
        .await
}

/// Ends every process `end_with_studio` was given, at once: renders,
/// combines, take checks and the OBS benchmark's FFmpegs alike. For
/// Studio's own way out, before it removes what they were writing. Returns
/// what was still running, counted by name (`messages::FFMPEG_*`). Only
/// named ones count: each console child also has a `conhost.exe` in the
/// job, which is none of Studio's work to report.
pub fn end_all_with_studio() -> std::io::Result<Vec<(&'static str, usize)>> {
    let Some(job) = studio_job() else {
        return Ok(Vec::new());
    };
    let running = job.process_ids();
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    {
        let names = lock_recover::lock_or_recover(names());
        for what in running.iter().filter_map(|pid| names.get(pid).copied()) {
            match counts.iter_mut().find(|(name, _)| *name == what) {
                Some((_, n)) => *n += 1,
                None => counts.push((what, 1)),
            }
        }
    }
    counts.sort();
    job.end_all()?;
    Ok(counts)
}

/// Whether the running process `pid` is in Studio's job, for the tests of
/// code that starts children (#1496). False when it can't be told.
#[cfg(test)]
#[cfg(windows)]
pub(crate) fn in_studio_job(pid: u32) -> bool {
    studio_job().is_some_and(|job| win::contains(job.handle, pid))
}

/// The children of this process named `exe` that are in Studio's job.
#[cfg(test)]
#[cfg(windows)]
pub(crate) fn children_in_studio_job(exe: &str) -> Vec<u32> {
    let sys = crate::sys::process::snapshot();
    crate::sys::process::children_named_in(&sys, &[exe], std::process::id())
        .into_iter()
        .filter(|&pid| in_studio_job(pid))
        .collect()
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
        fn QueryInformationJobObject(
            job: isize,
            class: i32,
            info: *mut u8,
            length: u32,
            returned: *mut u32,
        ) -> i32;
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

    /// `JobObjectBasicProcessIdList`.
    const PROCESS_ID_LIST: i32 = 3;
    /// More than Studio ever runs at once. A fuller job makes Windows fail
    /// the call, and the list comes back empty: the exit then logs no count.
    const MAX_LISTED: usize = 256;

    /// `JOBOBJECT_BASIC_PROCESS_ID_LIST` with room for `MAX_LISTED` IDs.
    #[repr(C)]
    struct ProcessIdList {
        assigned: u32,
        listed: u32,
        ids: [usize; MAX_LISTED],
    }

    pub fn process_ids(job: isize) -> Vec<u32> {
        let mut list = ProcessIdList {
            assigned: 0,
            listed: 0,
            ids: [0; MAX_LISTED],
        };
        // SAFETY: `list` is the size passed and outlives the call.
        let ok = unsafe {
            QueryInformationJobObject(
                job,
                PROCESS_ID_LIST,
                (&raw mut list).cast(),
                std::mem::size_of::<ProcessIdList>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Vec::new();
        }
        let listed = (list.listed as usize).min(MAX_LISTED);
        list.ids[..listed].iter().map(|&id| id as u32).collect()
    }

    pub fn close(handle: isize) {
        // SAFETY: called once, from `Drop` or on a job never handed out.
        // Closing our own handle can only fail if it were already closed.
        unsafe { CloseHandle(handle as *mut std::ffi::c_void) };
    }

    /// Whether process `pid` is in `job`. False when it can't be opened.
    #[cfg(test)]
    pub fn contains(job: isize, pid: u32) -> bool {
        /// `PROCESS_QUERY_LIMITED_INFORMATION`, all `IsProcessInJob` needs.
        const QUERY_LIMITED: u32 = 0x1000;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            // `*mut c_void`, as `modules.rs` declares it: one signature per
            // import, or the compiler warns.
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn IsProcessInJob(process: isize, job: isize, result: *mut i32) -> i32;
        }
        // SAFETY: plain query calls; the handle opened is closed below.
        let process = unsafe { OpenProcess(QUERY_LIMITED, 0, pid) } as isize;
        if process == 0 {
            return false;
        }
        let mut inside = 0;
        // SAFETY: both handles are open; `inside` outlives the call.
        let ok = unsafe { IsProcessInJob(process, job, &mut inside) };
        close(process);
        ok != 0 && inside != 0
    }
}

// Two attributes, not `all(test, windows)`: Clippy only treats a module
// under a plain `cfg(test)` as test code.
#[cfg(test)]
#[cfg(windows)]
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

    /// What combines, frame checks and the OBS benchmark start their FFmpegs
    /// with (#1496): the child is in Studio's job, so it ends with Studio.
    #[tokio::test]
    async fn a_child_started_to_end_with_studio_is_in_studios_job() {
        let mut cmd = tokio::process::Command::new("ping");
        cmd.args(["-n", "60", "127.0.0.1"])
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = super::spawn_ending_with_studio(&mut cmd, "ping").unwrap();
        let pid = child.id().unwrap();
        assert!(super::in_studio_job(pid));
        child.kill().await.unwrap();

        // A child started the plain way is not, so the check can tell.
        let mut plain = long_child();
        assert!(!super::in_studio_job(plain.id().unwrap()));
        plain.kill().await.unwrap();
    }

    #[tokio::test]
    async fn output_ending_with_studio_collects_what_the_child_printed() {
        // Not `waitfor`: the frame-check test looks for its own
        // `waitfor.exe` in Studio's job.
        let mut cmd = tokio::process::Command::new("ping");
        cmd.args(["-n", "1", "127.0.0.1"])
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let out = super::output_ending_with_studio(&mut cmd, "ping")
            .await
            .unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("127.0.0.1"));
    }
}
