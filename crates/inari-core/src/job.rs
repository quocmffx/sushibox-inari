//! Win32 Job Object wrapper.
//!
//! Assigns every spawned child to a job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
//! so all children are terminated automatically when Inari.exe exits — even on
//! crash or task-kill.  No-op on non-Windows platforms.

#[cfg(windows)]
mod imp {
    use anyhow::Result;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};
    use windows::core::PCWSTR;

    pub struct JobObject(HANDLE);

    // HANDLE is a raw pointer; we own it exclusively.
    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}

    impl JobObject {
        pub fn new() -> Result<Self> {
            unsafe {
                let handle = CreateJobObjectW(None, PCWSTR::null())?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &raw const info as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )?;
                Ok(Self(handle))
            }
        }

        pub fn assign(&self, pid: u32) -> Result<()> {
            unsafe {
                let proc = OpenProcess(PROCESS_ALL_ACCESS, false, pid)?;
                AssignProcessToJobObject(self.0, proc)?;
                let _ = CloseHandle(proc);
            }
            Ok(())
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            unsafe { let _ = CloseHandle(self.0); }
        }
    }
}

#[cfg(windows)]
pub use imp::JobObject;

#[cfg(not(windows))]
pub struct JobObject;

#[cfg(not(windows))]
impl JobObject {
    pub fn new() -> anyhow::Result<Self> { Ok(Self) }
    pub fn assign(&self, _pid: u32) -> anyhow::Result<()> { Ok(()) }
}
