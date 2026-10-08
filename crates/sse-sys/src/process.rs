//! Bounded child-process groups used by native worker hosts.

use std::process::{Child, Command};

/// Owns the process group or job object for one worker child.
pub struct ProcessTree {
    platform: platform::ProcessTree,
}

impl ProcessTree {
    /// Configures a spawned worker so its descendants can be terminated together.
    pub fn configure(command: &mut Command) {
        platform::configure(command);
    }

    /// Attaches the already spawned child to its process group or job object.
    pub fn attach(child: &Child) -> std::io::Result<Self> {
        platform::attach(child).map(|platform| Self { platform })
    }

    /// Terminates the worker and descendants, then reaps the direct child.
    pub fn terminate(&self, child: &mut Child) {
        platform::terminate(&self.platform, child);
    }

    /// Checks whether the child exited while keeping its process identity available for tree cleanup.
    ///
    /// On Unix, the exit status remains waitable until [`Self::terminate`] reaps the child.
    pub fn try_wait(&self, child: &mut Child) -> std::io::Result<Option<std::process::ExitStatus>> {
        platform::try_wait(&self.platform, child)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::ProcessTree;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn exited_child_stays_waitable_until_tree_termination() {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 17"]);
        ProcessTree::configure(&mut command);
        let Ok(mut child) = command.spawn() else {
            panic!("failed to spawn process-tree test child");
        };
        let process_id = child.id();
        let Ok(tree) = ProcessTree::attach(&child) else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("failed to attach process-tree test child");
        };

        let mut status = None;
        for _ in 0..500 {
            match tree.try_wait(&mut child) {
                Ok(Some(exited)) => {
                    status = Some(exited);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(error) => {
                    tree.terminate(&mut child);
                    panic!("failed to observe child exit: {error}");
                }
            }
        }
        let Some(status) = status else {
            tree.terminate(&mut child);
            panic!("test child did not exit");
        };
        assert_eq!(status.code(), Some(17));

        let proc_stat_path = format!("/proc/{process_id}/stat");
        let Ok(proc_stat) = std::fs::read_to_string(&proc_stat_path) else {
            tree.terminate(&mut child);
            panic!("observed child was reaped before its process tree was terminated");
        };
        assert_eq!(proc_stat.split_ascii_whitespace().nth(2), Some("Z"));

        tree.terminate(&mut child);
        assert!(std::fs::read_to_string(proc_stat_path).is_err());
    }

    #[test]
    fn terminating_exited_parent_still_stops_its_descendants() {
        let marker = std::env::temp_dir().join(format!(
            "sse-process-tree-descendant-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("(sleep 1; printf alive > \"$1\") & exit 17")
            .arg("process-tree-test")
            .arg(&marker);
        ProcessTree::configure(&mut command);
        let Ok(mut child) = command.spawn() else {
            panic!("failed to spawn process-tree descendant test child");
        };
        let Ok(tree) = ProcessTree::attach(&child) else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("failed to attach process-tree descendant test child");
        };

        let mut exited = false;
        for _ in 0..500 {
            match tree.try_wait(&mut child) {
                Ok(Some(_)) => {
                    exited = true;
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(error) => {
                    tree.terminate(&mut child);
                    panic!("failed to observe child exit: {error}");
                }
            }
        }
        if !exited {
            tree.terminate(&mut child);
            panic!("test parent did not exit");
        }

        tree.terminate(&mut child);
        thread::sleep(Duration::from_millis(1100));
        let descendant_survived = marker.exists();
        let _ = std::fs::remove_file(marker);
        assert!(!descendant_survived, "a descendant escaped the process group");
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::ProcessTree;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn suspended_child_is_assigned_and_descendants_are_terminated() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let started = std::env::temp_dir().join(format!("sse-process-tree-started-{unique}.txt"));
        let finished = std::env::temp_dir().join(format!("sse-process-tree-finished-{unique}.txt"));
        let mut command = Command::new("cmd.exe");
        command
            .arg("/C")
            .arg("start \"\" /b powershell.exe -NoProfile -Command \"Set-Content -NoNewline -LiteralPath $env:SSE_PROCESS_TREE_STARTED -Value started; Start-Sleep -Milliseconds 1000; Set-Content -NoNewline -LiteralPath $env:SSE_PROCESS_TREE_FINISHED -Value finished\" & exit 17")
            .env("SSE_PROCESS_TREE_STARTED", &started)
            .env("SSE_PROCESS_TREE_FINISHED", &finished);
        ProcessTree::configure(&mut command);
        let Ok(mut child) = command.spawn() else {
            panic!("failed to spawn process-tree test child");
        };
        let Ok(tree) = ProcessTree::attach(&child) else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("failed to attach process-tree test child");
        };

        let mut status = None;
        for _ in 0..5000 {
            match tree.try_wait(&mut child) {
                Ok(Some(exited)) => {
                    status = Some(exited);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(error) => {
                    tree.terminate(&mut child);
                    panic!("failed to observe child exit: {error}");
                }
            }
        }
        let Some(status) = status else {
            tree.terminate(&mut child);
            panic!("test child did not exit after it was resumed");
        };
        assert_eq!(status.code(), Some(17));

        let mut descendant_started = false;
        for _ in 0..1500 {
            if started.exists() {
                descendant_started = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if !descendant_started {
            tree.terminate(&mut child);
            let _ = std::fs::remove_file(&started);
            let _ = std::fs::remove_file(&finished);
            panic!("test descendant did not start");
        }

        tree.terminate(&mut child);
        thread::sleep(Duration::from_millis(1200));
        let descendant_finished = finished.exists();
        let _ = std::fs::remove_file(&started);
        let _ = std::fs::remove_file(&finished);
        assert!(!descendant_finished, "a Windows job descendant escaped termination");
    }
}

#[cfg(unix)]
mod platform {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, ExitStatus};
    use std::{io, os::unix::process::ExitStatusExt};

    const SIGKILL: i32 = 9;
    const P_PID: i32 = 1;
    const WNOHANG: i32 = 1;
    const WEXITED: i32 = 4;
    const CLD_EXITED: i32 = 1;
    const CLD_KILLED: i32 = 2;
    const CLD_DUMPED: i32 = 3;

    #[cfg(target_os = "linux")]
    const WNOWAIT: i32 = 0x0100_0000;

    #[cfg(target_os = "macos")]
    const WNOWAIT: i32 = 0x0000_0020;

    pub struct ProcessTree {
        process_group: i32,
    }

    #[cfg(target_os = "linux")]
    #[repr(C, align(8))]
    #[derive(Default)]
    struct SigInfo {
        signal: i32,
        error: i32,
        code: i32,
        _padding: i32,
        pid: i32,
        uid: u32,
        status: i32,
        _reserved: [u64; 12],
    }

    #[cfg(target_os = "macos")]
    #[repr(C, align(8))]
    #[derive(Default)]
    struct SigInfo {
        signal: i32,
        error: i32,
        code: i32,
        pid: i32,
        uid: u32,
        status: i32,
        _reserved: [u64; 10],
    }

    // SAFETY: these declarations match the POSIX C ABI signatures for `kill` and `waitid` on Linux and macOS.
    unsafe extern "C" {
        fn kill(process: i32, signal: i32) -> i32;

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        fn waitid(id_type: i32, id: u32, info: *mut SigInfo, options: i32) -> i32;
    }

    pub fn configure(command: &mut Command) {
        command.process_group(0);
    }

    pub fn attach(child: &Child) -> std::io::Result<ProcessTree> {
        let process_group = i32::try_from(child.id()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "worker process id is not representable",
            )
        })?;
        Ok(ProcessTree { process_group })
    }

    pub fn terminate(tree: &ProcessTree, child: &mut Child) {
        // SAFETY: the group id belongs to the child launched with `process_group(0)` and kept waitable until cleanup;
        // SIGKILL targets only that private process group.
        let _ = unsafe { kill(tree.process_group.saturating_neg(), SIGKILL) };
        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn try_wait(_tree: &ProcessTree, child: &mut Child) -> io::Result<Option<ExitStatus>> {
        loop {
            let mut info = SigInfo::default();
            // SAFETY: `info` has the target OS siginfo_t layout and alignment; `child.id()` is this process's child,
            // and WNOWAIT leaves its status waitable so its PID cannot be reused before process-group cleanup.
            let result = unsafe { waitid(P_PID, child.id(), &mut info, WEXITED | WNOHANG | WNOWAIT) };
            if result != 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if info.pid == 0 {
                return Ok(None);
            }
            if u32::try_from(info.pid).ok() != Some(child.id()) {
                return Err(io::Error::other("waitid returned a different child process"));
            }
            let wait_status = match info.code {
                CLD_EXITED => (info.status & 0xff) << 8,
                CLD_KILLED => info.status & 0x7f,
                CLD_DUMPED => (info.status & 0x7f) | 0x80,
                _ => return Err(io::Error::other("waitid returned a non-exit child status")),
            };
            return Ok(Some(ExitStatus::from_raw(wait_status)));
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn try_wait(_tree: &ProcessTree, _child: &mut Child) -> io::Result<Option<ExitStatus>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "non-reaping process waits are unsupported on this Unix platform",
        ))
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::ptr::NonNull;
    use std::{ffi::c_void, io};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const JOB_OBJECT_BASIC_LIMIT_INFORMATION: i32 = 2;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

    pub struct ProcessTree {
        job: NonNull<c_void>,
    }

    #[repr(C)]
    struct JobObjectBasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[link(name = "kernel32")]
    // SAFETY: these declarations match the documented kernel32 system ABI; the call sites validate handles and layouts.
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(
            job: *mut c_void,
            information_class: i32,
            information: *mut c_void,
            information_length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    #[link(name = "ntdll")]
    // SAFETY: the signature matches ntdll's NTSTATUS NtResumeProcess(HANDLE) system ABI.
    unsafe extern "system" {
        fn NtResumeProcess(process: *mut c_void) -> i32;
    }

    pub fn configure(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    pub fn attach(child: &Child) -> io::Result<ProcessTree> {
        // SAFETY: null attributes and name are the documented way to request a new unnamed job object.
        let job = NonNull::new(unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) })
            .ok_or_else(io::Error::last_os_error)?;
        let mut limits = JobObjectBasicLimitInformation {
            per_process_user_time_limit: 0,
            per_job_user_time_limit: 0,
            limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            minimum_working_set_size: 0,
            maximum_working_set_size: 0,
            active_process_limit: 0,
            affinity: 0,
            priority_class: 0,
            scheduling_class: 0,
        };
        let Ok(information_length) = u32::try_from(std::mem::size_of::<JobObjectBasicLimitInformation>()) else {
            // SAFETY: this handle was returned by CreateJobObjectW and is closed exactly once on setup failure.
            let _ = unsafe { CloseHandle(job.as_ptr()) };
            return Err(io::Error::other("job limit structure size overflow"));
        };
        // SAFETY: the job handle is live and `limits` has the documented layout for JobObjectBasicLimitInformation.
        let configured = unsafe {
            SetInformationJobObject(
                job.as_ptr(),
                JOB_OBJECT_BASIC_LIMIT_INFORMATION,
                (&mut limits as *mut JobObjectBasicLimitInformation).cast(),
                information_length,
            )
        };
        if configured == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: this handle was returned by CreateJobObjectW and is closed once after setup fails.
            let _ = unsafe { CloseHandle(job.as_ptr()) };
            return Err(error);
        }
        // SAFETY: the process handle is borrowed from a live Child and the job handle was checked non-null above.
        let assigned = unsafe { AssignProcessToJobObject(job.as_ptr(), child.as_raw_handle().cast()) };
        if assigned == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: this handle was returned by CreateJobObjectW and is closed once when assignment fails.
            let _ = unsafe { CloseHandle(job.as_ptr()) };
            return Err(error);
        }
        // SAFETY: `child` owns a live process handle; CREATE_SUSPENDED keeps its primary thread from creating
        // descendants until it has been assigned to the kill-on-close job above.
        let resume_status = unsafe { NtResumeProcess(child.as_raw_handle().cast()) };
        if resume_status < 0 {
            let error = io::Error::other(format!(
                "NtResumeProcess failed with NTSTATUS 0x{:08X}",
                resume_status as u32
            ));
            // SAFETY: closing the configured kill-on-close job terminates its assigned child.
            let _ = unsafe { CloseHandle(job.as_ptr()) };
            return Err(error);
        }
        Ok(ProcessTree { job })
    }

    pub fn terminate(tree: &ProcessTree, child: &mut Child) {
        // SAFETY: this live job handle owns the worker process and its descendants.
        let _ = unsafe { TerminateJobObject(tree.job.as_ptr(), 1) };
        let _ = child.kill();
        let _ = child.wait();
    }

    pub fn try_wait(_tree: &ProcessTree, child: &mut Child) -> io::Result<Option<std::process::ExitStatus>> {
        child.try_wait()
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            // SAFETY: this handle was returned by CreateJobObjectW and is closed exactly once.
            let _ = unsafe { CloseHandle(self.job.as_ptr()) };
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use std::process::{Child, Command};

    pub struct ProcessTree;

    pub fn configure(_command: &mut Command) {}

    pub fn attach(_child: &Child) -> std::io::Result<ProcessTree> {
        Ok(ProcessTree)
    }

    pub fn terminate(_tree: &ProcessTree, child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    pub fn try_wait(_tree: &ProcessTree, child: &mut Child) -> std::io::Result<Option<std::process::ExitStatus>> {
        child.try_wait()
    }
}
