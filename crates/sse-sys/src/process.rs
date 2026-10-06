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
}

#[cfg(unix)]
mod platform {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    const SIGKILL: i32 = 9;

    pub struct ProcessTree {
        process_group: i32,
    }

    // SAFETY: ACCEPTANCE.md Part IV §3 confines native ABI declarations to sse-sys; `kill` uses the POSIX C ABI.
    unsafe extern "C" {
        fn kill(process: i32, signal: i32) -> i32;
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
        // SAFETY: ACCEPTANCE.md Part IV §3 confines this OS call to sse-sys; the process group id comes from the child
        // spawned with `process_group(0)`, and SIGKILL is sent only to that private group.
        let _ = unsafe { kill(tree.process_group.saturating_neg(), SIGKILL) };
        let _ = child.kill();
        let _ = child.wait();
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

    pub struct ProcessTree {
        job: Option<NonNull<c_void>>,
    }

    #[link(name = "kernel32")]
    // SAFETY: ACCEPTANCE.md Part IV §3 confines native ABI declarations to sse-sys; these signatures match kernel32.
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> *mut c_void;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    pub fn configure(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    pub fn attach(child: &Child) -> io::Result<ProcessTree> {
        // SAFETY: ACCEPTANCE.md Part IV §3 confines Win32 calls to sse-sys; null attributes/name request an unnamed job.
        let job = NonNull::new(unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) });
        let Some(job) = job else {
            return Ok(ProcessTree { job: None });
        };
        // SAFETY: the process handle is borrowed from a live Child and the job handle was checked non-null above.
        let assigned = unsafe { AssignProcessToJobObject(job.as_ptr(), child.as_raw_handle().cast()) };
        if assigned == 0 {
            // SAFETY: this handle was returned by CreateJobObjectW and is closed once when assignment fails.
            let _ = unsafe { CloseHandle(job.as_ptr()) };
            return Ok(ProcessTree { job: None });
        }
        Ok(ProcessTree { job: Some(job) })
    }

    pub fn terminate(tree: &ProcessTree, child: &mut Child) {
        if let Some(job) = tree.job {
            // SAFETY: this live job handle owns the worker process and its descendants.
            let _ = unsafe { TerminateJobObject(job.as_ptr(), 1) };
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            if let Some(job) = self.job.take() {
                // SAFETY: this handle was returned by CreateJobObjectW and is closed exactly once.
                let _ = unsafe { CloseHandle(job.as_ptr()) };
            }
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
}
