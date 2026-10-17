// The `claude` processes Coucou starts, held in a Windows Job Object: when the
// job's last handle closes — the session ends, or Coucou itself dies, however
// it dies — every process in it is killed, the CLI's own children included,
// and together they may not use more than MEMORY_LIMIT. Elsewhere this is a
// no-op: the child is killed with its handle (kill_on_drop).

/// More than Claude Code needs for a chat or a spoken turn by far.
#[cfg(windows)]
const MEMORY_LIMIT: usize = 3 * 1024 * 1024 * 1024;

/// Kept as long as the child should live; dropping it ends the job.
pub struct Job {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

// The handle is only closed, once, in Drop.
#[cfg(windows)]
unsafe impl Send for Job {}

#[cfg(windows)]
pub fn contain(child: &tokio::process::Child) -> Option<Job> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    let raw = child.raw_handle()?;
    // SAFETY: plain Win32 calls on a job we own and the child's live handle;
    // the info struct outlives the call that reads it.
    unsafe {
        let handle = CreateJobObjectW(None, None).ok()?;
        let job = Job { handle };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        info.JobMemoryLimit = MEMORY_LIMIT;
        SetInformationJobObject(
            job.handle,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .ok()?;
        AssignProcessToJobObject(job.handle, HANDLE(raw as _)).ok()?;
        Some(job)
    }
}

#[cfg(not(windows))]
pub fn contain(_child: &tokio::process::Child) -> Option<Job> {
    Some(Job {})
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed only here.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
